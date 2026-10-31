//! semacs spike lab (experiment only, not product code).
//!
//! Purpose: observe how gpui-kit 0.8.1 behaves on the real paths the plan needs
//! verified (S1-S5): interceptor gating, key prefix handling, platform text
//! input observation, own undo, echo area, and a minimal minibuffer target.
//!
//! Env:
//!   SPIKE_LOG      log file (default: log/lab.log)
//!   SPIKE_INITIAL  initial buffer text
//!   SPIKE_PASS     comma separated key names that bypass the interceptor
//!   SPIKE_READONLY accept no platform text input

mod edit;
mod keys;
mod mark;
mod target;
mod undo;

use std::ops::Range;
use std::sync::atomic::AtomicBool;

use gpui_kit::component::input::{Editor, EditorState, Input, InputEvent, InputState};
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use target::EditTarget;
use edit::Editable;

/// Spike stand-in for the real key-map pending prefix (e.g. after C-x).
static PREFIX_PENDING: AtomicBool = AtomicBool::new(false);
/// Deterministic stand-in for an active IME composition in headless cases:
/// GPUI deletes synthetic marked text when the virtual keyboard leaves, so the
/// policy branch is tested with this flag and the real path with a real IME.
static FAKE_PREEDIT: AtomicBool = AtomicBool::new(false);

pub fn prefix_pending() -> bool {
    PREFIX_PENDING.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn readonly_mode() -> bool {
    std::env::var("SPIKE_READONLY").map(|v| v == "1").unwrap_or(false)
}

fn log_line(msg: impl AsRef<str>) {
    use std::io::Write;
    let path = std::env::var("SPIKE_LOG").unwrap_or_else(|_| "log/lab.log".to_string());
    if let Some(dir) = std::path::Path::new(&path).parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{}", msg.as_ref());
        let _ = f.flush();
    }
}

fn snapshot(editor: &Entity<EditorState>, cx: &mut App) -> String {
    let st = editor.read(cx);
    format!(
        "text={:?} cursor={} sel={:?}",
        st.text().to_string(),
        st.cursor(),
        st.selected_range()
    )
}

fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

enum Direction {
    Undo,
    Redo,
}

fn replay<S: edit::Editable + EntityInputHandler>(
    state: &Entity<S>,
    target: &Entity<EditTarget<S>>,
    mark: &mark::SharedMark,
    group: &undo::Group,
    direction: Direction,
    window: &mut Window,
    cx: &mut App,
) {
    undo::set_replaying(true);
    match direction {
        Direction::Undo => {
            for change in group.changes.iter().rev() {
                let doc = state.read(cx).text_string();
                let start = change.range.start;
                let end = start + change.new.len();
                let (s, e) = (utf16_len(&doc[..start]), utf16_len(&doc[..end]));
                log_line(format!("REPLAY undo {start}..{end} <- {:?}", change.old));
                target.update(cx, |t, cx| {
                    EntityInputHandler::replace_text_in_range(t, Some(s..e), &change.old, window, cx)
                });
                if let Some(offset) = change.mark_collapse {
                    mark.borrow_mut().restore_collapsed(start, offset);
                }
            }
            state.update(cx, |st, cx| st.set_point(group.point_before, cx));
        }
        Direction::Redo => {
            for change in group.changes.iter() {
                let doc = state.read(cx).text_string();
                let start = change.range.start;
                let end = start + change.old.len();
                let (s, e) = (utf16_len(&doc[..start]), utf16_len(&doc[..end]));
                log_line(format!("REPLAY redo {start}..{end} <- {:?}", change.new));
                target.update(cx, |t, cx| {
                    EntityInputHandler::replace_text_in_range(t, Some(s..e), &change.new, window, cx)
                });
            }
            state.update(cx, |st, cx| st.set_point(group.redo_point(), cx));
        }
    }
    undo::set_replaying(false);
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Session {
    Buffer,
    Minibuffer,
}

/// The parts of a buffer that must survive A -> B -> A. The lab keeps exactly
/// two buffers and swaps these fields; it is enough to prove per-buffer state
/// and undo isolation without building a real buffer list.
struct BufferSaved {
    editor: Entity<EditorState>,
    target: Entity<EditTarget<EditorState>>,
    focus_handle: FocusHandle,
    undo: undo::SharedUndo,
    mark: mark::SharedMark,
}

struct Lab {
    editor: Entity<EditorState>,
    target: Entity<EditTarget<EditorState>>,
    focus_handle: FocusHandle,

    minibuffer: Entity<InputState>,
    mini_target: Entity<EditTarget<InputState>>,
    mini_focus: FocusHandle,
    mini_mark: mark::SharedMark,

    session: Session,
    prompt: String,
    session_value: String,

    keymap: keys::Keymap,
    pending: keys::KeySeq,
    echo: String,

    mark: mark::SharedMark,

    buffer_undo: undo::SharedUndo,
    mini_undo: undo::SharedUndo,

    ring: edit::SharedKillRing,
    alt: BufferSaved,
    _subs: Vec<Subscription>,
}

impl Lab {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let initial = std::env::var("SPIKE_INITIAL").unwrap_or_default();
        let editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .default_value(initial)
                .soft_wrap(true)
                .line_number(false)
        });
        let buffer_undo = undo::new_stack();
        let mini_undo = undo::new_stack();
        let buffer_mark = mark::new_mark();
        let target = cx.new({
            let undo = buffer_undo.clone();
            let mark = buffer_mark.clone();
            let editor = editor.clone();
            move |_| EditTarget::new(editor.clone(), "buffer", undo, mark)
        });
        let focus_handle = editor.read(cx).focus_handle(cx).clone();
        // Emacs baseline starts at point-max; mirror that so both sides compare.
        let initial_len = editor.read(cx).text().len();
        if initial_len > 0 {
            editor.update(cx, |st, cx| st.set_selected_range(initial_len..initial_len, cx));
        }

        let alt_initial = std::env::var("SPIKE_INITIAL2").unwrap_or_default();
        let alt_editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .default_value(alt_initial)
                .soft_wrap(true)
                .line_number(false)
        });
        let alt_undo = undo::new_stack();
        let alt_mark = mark::new_mark();
        let alt_target = cx.new({
            let undo = alt_undo.clone();
            let mark = alt_mark.clone();
            let editor = alt_editor.clone();
            move |_| EditTarget::new(editor.clone(), "buffer2", undo, mark)
        });
        let alt_focus = alt_editor.read(cx).focus_handle(cx).clone();
        let alt_len = alt_editor.read(cx).text().len();
        if alt_len > 0 {
            alt_editor.update(cx, |st, cx| st.set_selected_range(alt_len..alt_len, cx));
        }
        alt_undo.borrow_mut().mark_saved();

        let minibuffer = cx.new(|cx| InputState::new(window, cx).placeholder(""));
        let mini_mark = mark::new_mark();
        let mini_target = cx.new({
            let undo = mini_undo.clone();
            let mark = mini_mark.clone();
            let minibuffer = minibuffer.clone();
            move |_| EditTarget::new(minibuffer.clone(), "mini", undo, mark)
        });
        let mini_focus = minibuffer.read(cx).focus_handle(cx).clone();

        buffer_undo.borrow_mut().mark_saved();

        log_line(format!(
            "=== lab start pid={} log={:?}",
            std::process::id(),
            std::env::var("SPIKE_LOG").unwrap_or_default()
        ));

        let mut keymap = keys::Keymap::default();
        for (seq, name) in COMMANDS {
            keymap.bind(seq, keys::Binding::Command(name));
        }
        for (seq, _) in PREFIXES {
            keymap.bind(seq, keys::Binding::Prefix);
        }

        let weak = cx.entity().downgrade();
        let subs = vec![
            cx.intercept_keystrokes(move |event: &KeystrokeEvent, window, cx: &mut App| {
                let ks = keys::KeyStroke::from_platform(&event.keystroke);
                log_line(format!(
                    "KEY   key={:?} key_char={:?} ctrl={} alt={} shift={} super={} pref={}",
                    event.keystroke.key,
                    event.keystroke.key_char,
                    ks.ctrl,
                    ks.alt,
                    ks.shift,
                    ks.super_,
                    matches!(event.input_preference, InputPreference::CharacterInput)
                ));
                let consumed = match weak.update(cx, |lab, cx| lab.handle_keystroke(&ks, window, cx))
                {
                    Ok(consumed) => consumed,
                    Err(_) => false,
                };
                if consumed {
                    cx.stop_propagation();
                }
            }),
            cx.subscribe_in(
                &alt_editor,
                window,
                |_lab, source, ev: &InputEvent, _window, cx| {
                    if matches!(ev, InputEvent::Change) {
                        log_line(format!("CHANGE[alt] {}", snapshot(source, cx)));
                    }
                },
            ),
            cx.observe_window_activation(window, |lab, window, cx| {
                if std::env::var("SPIKE_LOG_ACTIVATION")
                    .map(|v| v == "1")
                    .unwrap_or(false)
                {
                    let preedit = lab.preedit_range(window, cx);
                    let mark = lab.mark.borrow();
                    log_line(format!(
                        "ACTIVATION active={} text={:?} point={} mark={:?} active={} preedit={:?} undo={} dirty={}",
                        window.is_window_active(),
                        lab.editor.read(cx).text_string(),
                        lab.editor.read(cx).point(),
                        mark.pos(),
                        mark.active(),
                        preedit,
                        lab.buffer_undo.borrow().depth().0,
                        lab.is_dirty()
                    ));
                }
                if !window.is_window_active()
                    && let Some(range) = lab.real_marked_range(window, cx)
                {
                    log_line(format!("CANCEL focus preedit={range:?}"));
                    lab.cancel_preedit(range, window, cx);
                    log_line(format!(
                        "CANCEL focus done preedit_after={:?}",
                        lab.real_marked_range(window, cx)
                    ));
                }
                log_line(format!(
                    "WINDOW active={} buffer_focused={} mini_focused={}",
                    window.is_window_active(),
                    lab.focus_handle.is_focused(window),
                    lab.mini_focus.is_focused(window)
                ));
            }),
            cx.subscribe_in(
                &editor,
                window,
                |_lab, source, ev: &InputEvent, _window, cx| {
                    if matches!(ev, InputEvent::Change) {
                        log_line(format!("CHANGE {}", snapshot(source, cx)));
                    }
                    cx.notify();
                },
            ),
        ];

        let deferred_focus = focus_handle.clone();
        window.defer(cx, move |window, cx| {
            deferred_focus.focus(window, cx);
            log_line("=== focused");
        });

        Self {
            editor,
            target,
            focus_handle,
            minibuffer,
            mini_target,
            mini_focus,
            mini_mark,
            session: Session::Buffer,
            prompt: String::new(),
            session_value: String::new(),
            keymap,
            pending: Vec::new(),
            echo: String::new(),
            mark: buffer_mark,
            buffer_undo,
            mini_undo,
            ring: std::rc::Rc::new(std::cell::RefCell::new(edit::KillRing::default())),
            alt: BufferSaved {
                editor: alt_editor,
                target: alt_target,
                focus_handle: alt_focus,
                undo: alt_undo,
                mark: alt_mark,
            },
            _subs: subs,
        }
    }

    fn stack(&self) -> undo::SharedUndo {
        match self.session {
            Session::Buffer => self.buffer_undo.clone(),
            Session::Minibuffer => self.mini_undo.clone(),
        }
    }

    fn is_dirty(&self) -> bool {
        self.buffer_undo.borrow().is_dirty()
    }

    fn preedit_range(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<Range<usize>> {
        if FAKE_PREEDIT.load(std::sync::atomic::Ordering::Relaxed) {
            return Some(0..0);
        }
        self.real_marked_range(window, cx)
    }

    fn real_marked_range(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        match self.session {
            Session::Buffer => self.editor.update(cx, |st, cx| {
                EntityInputHandler::marked_text_range(st, window, cx)
            }),
            Session::Minibuffer => self.minibuffer.update(cx, |st, cx| {
                EntityInputHandler::marked_text_range(st, window, cx)
            }),
        }
    }

    /// Cancel the current composition by replacing the marked range with empty
    /// text. Used when the window loses focus or is closing: uncommitted preedit
    /// is not input (D17).
    fn cancel_preedit(
        &mut self,
        range: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.session {
            Session::Buffer => self.target.update(cx, |t, cx| {
                EntityInputHandler::replace_text_in_range(t, Some(range), "", window, cx)
            }),
            Session::Minibuffer => self.mini_target.update(cx, |t, cx| {
                EntityInputHandler::replace_text_in_range(t, Some(range), "", window, cx)
            }),
        }
    }

    /// D17: an uncommitted IME composition means the text is not finished, so
    /// commands must not act on it. Observation probes are exempt.
    fn allow_while_composing(name: &str) -> bool {
        matches!(
            name,
            "spike-dump"
                | "spike-dump-all"
                | "spike-sync-selection"
                | "spike-preedit-start"
                | "spike-preedit-cancel"
                | "spike-mark-ime"
                | "spike-fake-preedit-on"
                | "spike-fake-preedit-off"
        )
    }

    fn text_input_available(&self) -> bool {
        !readonly_mode()
    }

    /// D17 for the compositor close path: uncommitted preedit is not input.
    /// Cancel it and let the close proceed; only committed changes can make the
    /// buffer dirty and require the M0 close confirmation.
    fn should_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if let Some(range) = self.real_marked_range(window, cx) {
            log_line(format!("CANCEL close preedit={range:?}"));
            self.cancel_preedit(range, window, cx);
            log_line(format!(
                "CLOSE allow preedit_after={:?}",
                self.real_marked_range(window, cx)
            ));
        } else if FAKE_PREEDIT.load(std::sync::atomic::Ordering::Relaxed) {
            log_line("CANCEL close preedit=fake");
            log_line("CLOSE allow".to_string());
        } else {
            log_line("CLOSE allow".to_string());
        }
        true
    }

    fn echo(&mut self, message: impl Into<String>, cx: &mut Context<Self>) {
        self.echo = message.into();
        log_line(format!("ECHO  {}", self.echo));
        cx.notify();
    }

    /// Interceptor entry point: resolve the keystroke and run the command.
    fn handle_keystroke(&mut self, ks: &keys::KeyStroke, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let pass = std::env::var("SPIKE_PASS")
            .map(|list| list.split(',').any(|p| p.trim() == ks.key))
            .unwrap_or(false);

        // Pending prefix wins over text input: text is parsed, never inserted.
        let (resolution, pending) = self.keymap.resolve(&self.pending, ks);

        log_line(format!(
            "RESOLVE seq={:?} pending={:?} -> {:?}",
            keys::describe(std::slice::from_ref(ks)),
            keys::describe(&self.pending),
            resolution
        ));

        match resolution {
            keys::Resolution::Execute(name) => {
                self.pending.clear();
                self.set_pending(false);
                log_line(format!("CMD   {name}"));
                self.run_command(name, window, cx);
                true
            }
            keys::Resolution::Prefix => {
                if let Some(preedit) = self.preedit_range(window, cx) {
                    log_line(format!(
                        "REFUSE prefix {} preedit={preedit:?}",
                        keys::describe(&pending)
                    ));
                    self.echo("Input method composition is active; finish it first", cx);
                    return true;
                }
                self.pending = pending;
                self.set_pending(true);
                log_line(format!("PREFIX {}", keys::describe(&self.pending)));
                self.echo(keys::describe(&self.pending), cx);
                true
            }
            keys::Resolution::Undefined => {
                let seq = keys::describe(&self.pending) + " " + &ks.to_string();
                self.pending.clear();
                self.set_pending(false);
                self.echo(format!("{} is undefined", seq.trim()), cx);
                true
            }
            keys::Resolution::Composition => {
                // Dead key / in-progress composition: the platform already
                // delivered the marked text. No command, no undefined echo.
                log_line("COMPOSE in-progress composition keystroke consumed".to_string());
                true
            }
            keys::Resolution::SelfInsert => {
                if pass {
                    return false;
                }
                if !self.text_input_available() {
                    self.pending.clear();
                    self.set_pending(false);
                    self.echo("Buffer is read-only", cx);
                    return true;
                }
                self.pending.clear();
                self.set_pending(false);
                // Text goes to the platform path; a self-insert deactivates the mark.
                self.mark.borrow_mut().set_active(false);
                self.ring.borrow_mut().break_chain();
                false
            }
        }
    }

    fn set_pending(&mut self, pending: bool) {
        PREFIX_PENDING.store(pending, std::sync::atomic::Ordering::Relaxed);
    }

    fn region(&self, cx: &mut Context<Self>) -> Option<Range<usize>> {
        let mark_state = self.mark.borrow();
        if !mark_state.active() {
            return None;
        }
        let point = self.editor.read(cx).point();
        let mark = mark_state.pos()?;
        Some(mark.min(point)..mark.max(point))
    }

    fn run_command(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(preedit) = self.preedit_range(window, cx)
            && !Self::allow_while_composing(name)
        {
            log_line(format!("REFUSE {name} preedit={preedit:?}"));
            self.echo("Input method composition is active; finish it first", cx);
            return;
        }
        let ring = self.ring.clone();
        // Emacs appends only when the previous command was also a kill.
        if !matches!(name, "kill-line" | "kill-region") {
            self.ring.borrow_mut().break_chain();
        }
        match (name, self.session) {
            // ---- keyboard-quit -------------------------------------------------
            ("keyboard-quit", _) => {
                self.pending.clear();
                self.set_pending(false);
                if self.session == Session::Minibuffer {
                    self.cancel_minibuffer(window, cx);
                } else {
                    self.mark.borrow_mut().set_active(false);
                    self.echo("Quit", cx);
                }
            }

            // ---- spikes --------------------------------------------------------
            ("spike-dump", _) => {
                let (undo_depth, redo_depth) = self.buffer_undo.borrow().depth();
                let (mini_undo, mini_redo) = self.mini_undo.borrow().depth();
                let dirty = self.is_dirty();
                let mini = self
                    .minibuffer
                    .read(cx)
                    .text()
                    .to_string();
                let preedit = match self.session {
                    Session::Buffer => self.editor.update(cx, |st, cx| {
                        EntityInputHandler::marked_text_range(st, window, cx)
                    }),
                    Session::Minibuffer => self.minibuffer.update(cx, |st, cx| {
                        EntityInputHandler::marked_text_range(st, window, cx)
                    }),
                };
                let preedit_note = preedit
                    .as_ref()
                    .map(|m| format!(" preedit={m:?}"))
                    .unwrap_or_default();
                let mark_state = self.mark.borrow();
                log_line(format!(
                    "DUMP  {} mark={:?} active={} echo={:?} pending={:?} ring={} session={:?} mini={:?} undo={undo_depth} redo={redo_depth}{preedit_note}",
                    snapshot(&self.editor, cx),
                    mark_state.pos(),
                    mark_state.active(),
                    self.echo,
                    keys::describe(&self.pending),
                    self.ring.borrow().len(),
                    self.session,
                    mini
                ));
                log_line(format!(
                    "DUMP2 dirty={dirty} mini_undo={mini_undo}/{mini_redo}"
                ));
            }
            ("spike-undo", _) => self.command_undo(window, cx),
            ("spike-redo", _) => self.command_redo(window, cx),
            ("spike-delete-backward", Session::Buffer) => {
                edit::delete_backward(&self.editor, &self.target, &self.stack(), window, cx);
            }
            ("spike-insert-z", Session::Buffer) => {
                edit::insert_text(
                    &self.editor,
                    &self.target, &self.stack(),
                    "z",
                    undo::Kind::Typing,
                    window,
                    cx,
                );
            }
            ("spike-forward-char", Session::Buffer) => {
                let doc = self.editor.read(cx).text_string();
                let point = self.editor.read(cx).point();
                let next = doc[point..]
                    .char_indices()
                    .nth(1)
                    .map(|(i, _)| point + i)
                    .unwrap_or(point);
                edit::move_to(&self.editor, next, &self.stack(), cx);
            }
            ("spike-beginning-of-line", Session::Buffer) => {
                let doc = self.editor.read(cx).text_string();
                let point = self.editor.read(cx).point();
                let start = edit::line_start(&doc, point);
                edit::move_to(&self.editor, start, &self.stack(), cx);
            }
            ("spike-insert-typing", Session::Buffer) => {
                let point = self.editor.read(cx).point();
                let doc = self.editor.read(cx).text_string();
                let utf16 = utf16_len(&doc[..point]);
                self.editor.update(cx, |st, cx| {
                    EntityInputHandler::replace_text_in_range(
                        st,
                        Some(utf16..utf16),
                        "y",
                        window,
                        cx,
                    )
                });
            }
            ("spike-backspace-action", Session::Buffer) => {
                window.dispatch_action(
                    Box::new(gpui_kit::component::input::Backspace),
                    cx,
                );
            }
            ("spike-switch-buffer", _) => {
                // A buffer switch ends the outgoing buffer's undo group, as a
                // command boundary in Emacs does; the two histories stay apart.
                self.buffer_undo.borrow_mut().end_command(false);
                self.ring.borrow_mut().break_chain();
                std::mem::swap(&mut self.editor, &mut self.alt.editor);
                std::mem::swap(&mut self.target, &mut self.alt.target);
                std::mem::swap(&mut self.focus_handle, &mut self.alt.focus_handle);
                std::mem::swap(&mut self.buffer_undo, &mut self.alt.undo);
                std::mem::swap(&mut self.mark, &mut self.alt.mark);
                let focus = self.focus_handle.clone();
                focus.focus(window, cx);
                self.pending.clear();
                self.set_pending(false);
                let mark_state = self.mark.borrow();
                log_line(format!(
                    "BUFFER switch -> current point={} mark={:?} active={} undo={}",
                    self.editor.read(cx).point(),
                    mark_state.pos(),
                    mark_state.active(),
                    self.buffer_undo.borrow().depth().0
                ));
            }
            ("spike-sync-selection", _) => {
                let cursor = self.editor.read(cx).cursor();
                let sel = self.editor.read(cx).selected_range();
                {
                    let mut mark_state = self.mark.borrow_mut();
                    if sel.is_empty() {
                        // A click moves point and deactivates the region but keeps
                        // the mark position (Emacs mouse-set-point).
                        mark_state.set_active(false);
                    } else {
                        // The selection is normalized; cursor() is the head, so the
                        // other end is the anchor (mark), even for reverse drags.
                        mark_state.set(if cursor == sel.start { sel.end } else { sel.start });
                        mark_state.set_active(true);
                    }
                }
                self.stack().borrow_mut().end_command(false);
                let mark_state = self.mark.borrow();
                log_line(format!(
                    "SYNC  cursor={cursor} sel={sel:?} -> point={} mark={:?} active={}",
                    self.editor.read(cx).point(),
                    mark_state.pos(),
                    mark_state.active()
                ));
            }
            ("spike-dump-all", _) => {
                let (undo_depth, redo_depth) = self.buffer_undo.borrow().depth();
                let mark_state = self.mark.borrow();
                let alt_mark = self.alt.mark.borrow();
                log_line(format!(
                    "BUFFERS current text={:?} point={} mark={:?} active={} undo={undo_depth}/{redo_depth} | alt text={:?} point={} mark={:?} active={} undo={}",
                    self.editor.read(cx).text_string(),
                    self.editor.read(cx).point(),
                    mark_state.pos(),
                    mark_state.active(),
                    self.alt.editor.read(cx).text_string(),
                    self.alt.editor.read(cx).point(),
                    alt_mark.pos(),
                    alt_mark.active(),
                    self.alt.undo.borrow().depth().0
                ));
            }
            ("spike-toggle-prefix", _) => {
                let next = !prefix_pending();
                self.set_pending(next);
                log_line(format!("PREFIX pending={next}"));
            }
            ("spike-fake-preedit-on", _) => {
                FAKE_PREEDIT.store(true, std::sync::atomic::Ordering::Relaxed);
                log_line("FAKE-PREEDIT on".to_string());
            }
            ("spike-fake-preedit-off", _) => {
                FAKE_PREEDIT.store(false, std::sync::atomic::Ordering::Relaxed);
                log_line("FAKE-PREEDIT off".to_string());
            }
            ("spike-preedit-start", _) => {
                self.target.update(cx, |t, cx| {
                    EntityInputHandler::replace_and_mark_text_in_range(
                        t,
                        None,
                        "ni hao",
                        None,
                        window,
                        cx,
                    );
                });
                log_line("PREEDIT start (left marked)".to_string());
            }
            ("spike-preedit-cancel", _) => {
                let marked = self.preedit_range(window, cx);
                if let Some(range) = marked {
                    log_line(format!("PREEDIT cancel {range:?}"));
                    self.target.update(cx, |t, cx| {
                        EntityInputHandler::replace_text_in_range(t, Some(range), "", window, cx)
                    });
                } else {
                    self.echo("No composition to cancel", cx);
                }
            }
            // Start and commit a composition in one command so the synthetic
            // marked text survives (a virtual-keyboard leave would delete it).
            ("spike-mark-ime", Session::Buffer) => {
                self.target.update(cx, |t, cx| {
                    EntityInputHandler::replace_and_mark_text_in_range(
                        t,
                        None,
                        "ni hao",
                        None,
                        window,
                        cx,
                    );
                });
                match self.preedit_range(window, cx) {
                    Some(range) => {
                        log_line(format!("PREEDIT commit {range:?} -> 你好"));
                        self.target.update(cx, |t, cx| {
                            EntityInputHandler::replace_text_in_range(
                                t,
                                Some(range),
                                "你好",
                                window,
                                cx,
                            )
                        });
                    }
                    None => self.echo("No composition to commit", cx),
                }
            }
            ("spike-preedit", _) => {
                log_line("PREEDIT start");
                self.target.update(cx, |t, cx| {
                    EntityInputHandler::replace_and_mark_text_in_range(
                        t,
                        None,
                        "nihao",
                        None,
                        window,
                        cx,
                    );
                    EntityInputHandler::unmark_text(t, window, cx);
                });
            }

            // ---- editing (S2) --------------------------------------------------
            ("forward-char", Session::Buffer) => {
                let doc = self.editor.read(cx).text_string();
                let point = self.editor.read(cx).point();
                let next = doc[point..]
                    .char_indices()
                    .nth(1)
                    .map(|(i, _)| point + i)
                    .unwrap_or(point);
                edit::move_to(&self.editor, next, &self.stack(), cx);
            }
            ("backward-char", Session::Buffer) => {
                let doc = self.editor.read(cx).text_string();
                let point = self.editor.read(cx).point();
                let prev = doc[..point]
                    .char_indices()
                    .next_back()
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                edit::move_to(&self.editor, prev, &self.stack(), cx);
            }
            ("move-beginning-of-line", Session::Buffer) => {
                let doc = self.editor.read(cx).text_string();
                let point = self.editor.read(cx).point();
                let start = edit::line_start(&doc, point);
                edit::move_to(&self.editor, start, &self.stack(), cx);
            }
            ("move-end-of-line", Session::Buffer) => {
                let doc = self.editor.read(cx).text_string();
                let point = self.editor.read(cx).point();
                let end = edit::line_end(&doc, point);
                edit::move_to(&self.editor, end, &self.stack(), cx);
            }
            ("next-line", Session::Buffer) => {
                let doc = self.editor.read(cx).text_string();
                let point = self.editor.read(cx).point();
                let next = next_line(&doc, point);
                edit::move_to(&self.editor, next, &self.stack(), cx);
            }
            ("previous-line", Session::Buffer) => {
                let doc = self.editor.read(cx).text_string();
                let point = self.editor.read(cx).point();
                let prev = previous_line(&doc, point);
                edit::move_to(&self.editor, prev, &self.stack(), cx);
            }
            ("delete-backward-char", Session::Buffer) => {
                self.mark.borrow_mut().set_active(false);
                edit::delete_backward(&self.editor, &self.target, &self.stack(), window, cx);
            }
            ("delete-char", Session::Buffer) => {
                self.mark.borrow_mut().set_active(false);
                edit::delete_forward(&self.editor, &self.target, &self.stack(), window, cx);
            }
            ("kill-line", Session::Buffer) => {
                self.mark.borrow_mut().set_active(false);
                if edit::kill_line(&self.editor, &self.target, &self.stack(), &ring, window, cx)
                    .is_err()
                {
                    self.echo("End of buffer", cx);
                }
            }
            ("yank", Session::Buffer) => {
                let start = self.editor.read(cx).point();
                if edit::yank(&self.editor, &self.target, &self.stack(), &ring, window, cx) {
                    // Emacs sets the mark at the start of the yanked text.
                    {
                        let mut mark_state = self.mark.borrow_mut();
                        mark_state.set(start);
                        mark_state.set_active(false);
                    }
                    self.echo("Mark set", cx);
                } else {
                    self.echo("Kill ring is empty", cx);
                }
            }
            ("kill-region", Session::Buffer) | ("kill-ring-save", Session::Buffer) => {
                let save_only = name == "kill-ring-save";
                match self.region(cx) {
                    Some(region) if !region.is_empty() => {
                        edit::kill_region(
                            &self.editor,
                            &self.target, &self.stack(),
                            &ring,
                            region,
                            save_only,
                            window,
                            cx,
                        );
                        self.mark.borrow_mut().set_active(false);
                    }
                    _ => {
                        self.echo(
                            "The mark is not set now, so there is no region",
                            cx,
                        );
                        self.stack().borrow_mut().end_command(false);
                    }
                }
            }
            ("set-mark-command", Session::Buffer) => {
                let point = self.editor.read(cx).point();
                {
                    let mut mark_state = self.mark.borrow_mut();
                    mark_state.set(point);
                    mark_state.set_active(true);
                }
                self.ring.borrow_mut().break_chain();
                self.stack().borrow_mut().end_command(false);
                self.echo("Mark set", cx);
            }
            ("newline", Session::Buffer) => {
                self.mark.borrow_mut().set_active(false);
                edit::insert_text(
                    &self.editor,
                    &self.target, &self.stack(),
                    "\n",
                    undo::Kind::Other,
                    window,
                    cx,
                );
            }
            ("indent-for-tab-command", Session::Buffer) => {
                self.echo("indent-for-tab-command (spike: no-op)", cx);
                self.stack().borrow_mut().end_command(false);
            }
            ("save-buffer", Session::Buffer) => {
                let chars = self.editor.read(cx).text().chars().count();
                self.buffer_undo.borrow_mut().mark_saved();
                self.echo(format!("Saving file (spike): {chars} chars"), cx);
                self.stack().borrow_mut().end_command(false);
            }
            ("save-buffers-kill-terminal", Session::Buffer) => {
                log_line("=== quit requested");
                cx.quit();
            }
            ("switch-to-buffer", Session::Buffer) | ("find-file", Session::Buffer) => {
                self.start_minibuffer(name, window, cx);
            }

            // ---- minibuffer editing, same commands ---------------------------
            ("move-beginning-of-line", Session::Minibuffer) => {
                edit::move_to(&self.minibuffer, 0, &self.stack(), cx);
            }
            ("move-end-of-line", Session::Minibuffer) => {
                let len = self.minibuffer.read(cx).text().len();
                edit::move_to(&self.minibuffer, len, &self.stack(), cx);
            }
            ("forward-char", Session::Minibuffer) => {
                let doc = self.minibuffer.read(cx).text_string();
                let point = self.minibuffer.read(cx).point();
                let next = doc[point..]
                    .char_indices()
                    .nth(1)
                    .map(|(i, _)| point + i)
                    .unwrap_or(point);
                edit::move_to(&self.minibuffer, next, &self.stack(), cx);
            }
            ("backward-char", Session::Minibuffer) => {
                let doc = self.minibuffer.read(cx).text_string();
                let point = self.minibuffer.read(cx).point();
                let prev = doc[..point]
                    .char_indices()
                    .next_back()
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                edit::move_to(&self.minibuffer, prev, &self.stack(), cx);
            }
            ("delete-backward-char", Session::Minibuffer) => {
                edit::delete_backward(&self.minibuffer, &self.mini_target, &self.stack(), window, cx);
            }
            ("delete-char", Session::Minibuffer) => {
                edit::delete_forward(&self.minibuffer, &self.mini_target, &self.stack(), window, cx);
            }
            ("kill-line", Session::Minibuffer) => {
                if edit::kill_line(&self.minibuffer, &self.mini_target, &self.stack(), &ring, window, cx)
                    .is_err()
                {
                    self.echo("End of buffer", cx);
                }
            }
            ("yank", Session::Minibuffer) => {
                if !edit::yank(&self.minibuffer, &self.mini_target, &self.stack(), &ring, window, cx) {
                    self.echo("Kill ring is empty", cx);
                }
            }
            ("newline", Session::Minibuffer) => self.finish_minibuffer(true, window, cx),
            ("undo", _) => self.command_undo(window, cx),
            ("undo-redo", _) => self.command_redo(window, cx),

            (other, _) => {
                self.echo(format!("{other} is not implemented in the spike"), cx);
                self.stack().borrow_mut().end_command(false);
            }
        }
    }

    fn command_undo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let before = snapshot(&self.editor, cx);
        match self.stack().borrow_mut().undo_group() {
            Some(group) => {
                match self.session {
                    Session::Buffer => replay(
                        &self.editor,
                        &self.target,
                        &self.mark,
                        &group,
                        Direction::Undo,
                        window,
                        cx,
                    ),
                    Session::Minibuffer => replay(
                        &self.minibuffer,
                        &self.mini_target,
                        &self.mini_mark,
                        &group,
                        Direction::Undo,
                        window,
                        cx,
                    ),
                }
                log_line(format!("OWNUNDO {before} -> {}", snapshot(&self.editor, cx)));
                match self.session {
                    Session::Buffer => self.mark.borrow_mut().set_active(false),
                    Session::Minibuffer => self.mini_mark.borrow_mut().set_active(false),
                }
            }
            None => self.echo("No further undo information", cx),
        }
    }

    fn command_redo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.stack().borrow_mut().redo_group() {
            Some(group) => {
                match self.session {
                    Session::Buffer => replay(
                        &self.editor,
                        &self.target,
                        &self.mark,
                        &group,
                        Direction::Redo,
                        window,
                        cx,
                    ),
                    Session::Minibuffer => replay(
                        &self.minibuffer,
                        &self.mini_target,
                        &self.mini_mark,
                        &group,
                        Direction::Redo,
                        window,
                        cx,
                    ),
                }
                match self.session {
                    Session::Buffer => self.mark.borrow_mut().set_active(false),
                    Session::Minibuffer => self.mini_mark.borrow_mut().set_active(false),
                }
            }
            None => self.echo("No further redo information", cx),
        }
    }

    fn start_minibuffer(&mut self, command: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.session = Session::Minibuffer;
        self.prompt = format!("{command}: ");
        self.session_value.clear();
        self.minibuffer.update(cx, |st, cx| st.set_value("", window, cx));
        self.mini_undo.borrow_mut().clear();
        let focus = self.mini_focus.clone();
        focus.focus(window, cx);
        self.echo(self.prompt.clone(), cx);
    }

    fn cancel_minibuffer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        log_line("MINI  cancelled");
        self.session = Session::Buffer;
        self.prompt.clear();
        let focus = self.focus_handle.clone();
        focus.focus(window, cx);
        self.echo("Quit", cx);
    }

    fn finish_minibuffer(&mut self, confirm: bool, window: &mut Window, cx: &mut Context<Self>) {
        let value = self.minibuffer.read(cx).text().to_string();
        log_line(format!("MINI  value={value:?} confirm={confirm}"));
        self.session = Session::Buffer;
        self.session_value = value.clone();
        self.prompt.clear();
        let focus = self.focus_handle.clone();
        focus.focus(window, cx);
        self.echo(format!("minibuffer value: {value}"), cx);
    }
}

fn next_line(doc: &str, point: usize) -> usize {
    let start = edit::line_start(doc, point);
    let column = point - start;
    let end = edit::line_end(doc, point);
    if end >= doc.len() {
        return point;
    }
    let next_start = end + 1;
    let next_end = edit::line_end(doc, next_start);
    (next_start + column).min(next_end)
}

fn previous_line(doc: &str, point: usize) -> usize {
    let start = edit::line_start(doc, point);
    if start == 0 {
        return point;
    }
    let column = point - start;
    let prev_end = start - 1;
    let prev_start = edit::line_start(doc, prev_end);
    (prev_start + column).min(prev_end)
}

impl Render for Lab {
    fn render(&mut self, _: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let buffer_target = self.target.clone();
        let buffer_focus = self.focus_handle.clone();
        let mini_target = self.mini_target.clone();
        let mini_focus = self.mini_focus.clone();
        let show_mini = self.session == Session::Minibuffer;
        let prompt = self.prompt.clone();
        let echo = self.echo.clone();

        v_flex()
            .id("lab")
            .size_full()
            .child(Editor::new(&self.editor).into_any_element())
            .child(canvas(
                |_, _, _| (),
                move |bounds, _, window, cx| {
                    window.handle_input(
                        &buffer_focus,
                        gpui_kit::ElementInputHandler::new(bounds, buffer_target.clone()),
                        cx,
                    );
                },
            ))
            .when(show_mini, |this| {
                this.child(
                    v_flex()
                        .child(prompt.clone())
                        .child(Input::new(&self.minibuffer))
                        .child(canvas(
                            |_, _, _| (),
                            move |bounds, _, window, cx| {
                                window.handle_input(
                                    &mini_focus,
                                    gpui_kit::ElementInputHandler::new(bounds, mini_target.clone()),
                                    cx,
                                );
                            },
                        )),
                )
            })
            .child(div().child(echo))
    }
}

const PREFIXES: &[(&str, ())] = &[
    ("C-x", ()),
    ("C-c", ()),
    ("C-h", ()),
    ("M-", ()),
];

const COMMANDS: &[(&str, &str)] = &[
    // probes (spike-*)
    ("C-S-s", "spike-dump"),
    ("C-S-u", "spike-undo"),
    ("C-S-r", "spike-redo"),
    ("C-S-d", "spike-delete-backward"),
    ("C-S-z", "spike-insert-z"),
    ("C-S-f", "spike-forward-char"),
    ("C-S-a", "spike-beginning-of-line"),
    ("C-S-y", "spike-insert-typing"),
    ("C-S-b", "spike-backspace-action"),
    ("C-S-p", "spike-toggle-prefix"),
    ("C-S-o", "spike-switch-buffer"),
    ("C-S-l", "spike-dump-all"),
    ("C-S-k", "spike-sync-selection"),
    ("C-S-e", "spike-preedit"),
    // Probe: save with no C-x prefix, so the composition barrier is exercised
    // without the platform disabling the IME first.
    ("f9", "save-buffer"),
    ("C-S-j", "spike-preedit-start"),
    ("C-S-c", "spike-preedit-cancel"),
    ("f10", "spike-fake-preedit-on"),
    ("f11", "spike-fake-preedit-off"),
    ("f8", "spike-mark-ime"),
    // Probe: fcitx5 grabs Ctrl-SPC as its activation toggle, so real cases set
    // the mark through this binding instead.
    ("f7", "set-mark-command"),
    // v1 default keys (S2 subset)
    ("C-f", "forward-char"),
    ("C-b", "backward-char"),
    ("C-n", "next-line"),
    ("C-p", "previous-line"),
    ("C-a", "move-beginning-of-line"),
    ("C-e", "move-end-of-line"),
    ("C-d", "delete-char"),
    ("backspace", "delete-backward-char"),
    ("C-k", "kill-line"),
    ("C-y", "yank"),
    ("C-w", "kill-region"),
    ("M-w", "kill-ring-save"),
    ("C-SPC", "set-mark-command"),
    ("C-@", "set-mark-command"),
    ("C-g", "keyboard-quit"),
    ("C-/", "undo"),
    ("C-_", "undo"),
    ("C-x u", "undo"),
    ("C-?", "undo-redo"),
    ("C-M-_", "undo-redo"),
    ("enter", "newline"),
    ("tab", "indent-for-tab-command"),
    ("C-x C-s", "save-buffer"),
    ("C-x C-c", "save-buffers-kill-terminal"),
    ("C-x b", "switch-to-buffer"),
    ("C-x C-f", "find-file"),
];

fn main() {
    log_line("=== process starting");
    gpui_kit::application().run(|cx| {
        gpui_kit::init(cx);
        gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
            let lab = cx.new(|cx| Lab::new(window, cx));
            let weak = lab.downgrade();
            window.on_window_should_close(cx, move |window, cx| {
                weak.update(cx, |lab, cx| lab.should_close(window, cx))
                    .unwrap_or(true)
            });
            lab
        })
        .expect("failed to open window");
    });
    log_line("=== process exited");
}
