//! AppShell integration for incremental search (`isearch`).
//!
//! Coordinates the active `IsearchEngine` with the target `BufferView`,
//! `IsearchRing`, temporary expansion/re-collapse of hidden sections, IME
//! composition in the echo area, and `M-e` / nonincremental minibuffer editing.

use gpui_kit::{App, Context, Entity, EntityId, Focusable, Keystroke, SharedString, Window};

use super::AppShell;
use crate::buffer::{BufferId, SnapshotId};
use crate::command::Command;
use crate::isearch::{AbortOutcome, IsearchEngine, IsearchRing, SearchMode};
use crate::keymap::Key;
use crate::session::ReadKind;
use crate::view::BufferView;

#[cfg(test)]
#[path = "isearch_tests.rs"]
mod tests;

pub(super) struct IsearchSession {
    pub(super) view: Entity<BufferView>,
    buffer_id: BufferId,
    snapshot: SnapshotId,
    text: String,
    engine: IsearchEngine,
    opened_sections: Vec<String>,
    pending_keys: Vec<(Keystroke, bool)>,
    quote_next: bool,
    preedit: String,
    suspended_for_edit: bool,
    last_visible_range: Option<std::ops::Range<usize>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IsearchAction {
    RepeatForward,
    RepeatBackward,
    Exit,
    Abort,
    Cancel,
    DeleteChar,
    DelChar,
    YankWordOrChar,
    YankSymbolOrChar,
    YankLine,
    YankKill,
    QuoteChar,
    InsertTab,
    InsertNewline,
    RingRetreat,
    RingAdvance,
    EditString,
    ToggleCaseFold,
    ToggleRegexp,
    ToggleWord,
    ToggleSymbol,
    ToggleLaxWhitespace,
    ToggleInvisible,
    BeginningOfBuffer,
    EndOfBuffer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KeyMatch {
    Action(IsearchAction),
    Prefix,
    Unbound,
}

fn canonical_keys(pending: &[(Keystroke, bool)]) -> Vec<Key> {
    let mut keys = Vec::with_capacity(pending.len() * 2);
    for (stroke, _) in pending {
        let mut key = Key::from_keystroke(stroke);
        if stroke.modifiers.alt {
            keys.push(Key::from_keystroke(&Keystroke::parse("escape").unwrap()));
            let mut without_alt = stroke.clone();
            without_alt.modifiers.alt = false;
            key = Key::from_keystroke(&without_alt);
        }
        keys.push(key);
    }
    keys
}

fn lookup_isearch_keys(pending: &[(Keystroke, bool)]) -> KeyMatch {
    let seq = canonical_keys(pending);
    let bindings: &[(&[&str], IsearchAction)] = &[
        (&["ctrl-s"], IsearchAction::RepeatForward),
        (&["escape", "ctrl-s"], IsearchAction::RepeatForward),
        (&["ctrl-r"], IsearchAction::RepeatBackward),
        (&["escape", "ctrl-r"], IsearchAction::RepeatBackward),
        (&["enter"], IsearchAction::Exit),
        (&["ctrl-m"], IsearchAction::Exit),
        (&["ctrl-g"], IsearchAction::Abort),
        (&["escape", "escape", "escape"], IsearchAction::Cancel),
        (&["backspace"], IsearchAction::DeleteChar),
        (&["escape", "ctrl-d"], IsearchAction::DelChar),
        (&["ctrl-w"], IsearchAction::YankWordOrChar),
        (&["escape", "ctrl-w"], IsearchAction::YankSymbolOrChar),
        (&["escape", "s", "ctrl-e"], IsearchAction::YankLine),
        (&["ctrl-y"], IsearchAction::YankKill),
        (&["ctrl-q"], IsearchAction::QuoteChar),
        (&["tab"], IsearchAction::InsertTab),
        (&["ctrl-j"], IsearchAction::InsertNewline),
        (&["escape", "p"], IsearchAction::RingRetreat),
        (&["escape", "n"], IsearchAction::RingAdvance),
        (&["escape", "e"], IsearchAction::EditString),
        (&["escape", "s", "e"], IsearchAction::EditString),
        (&["escape", "c"], IsearchAction::ToggleCaseFold),
        (&["escape", "s", "c"], IsearchAction::ToggleCaseFold),
        (&["escape", "r"], IsearchAction::ToggleRegexp),
        (&["escape", "s", "r"], IsearchAction::ToggleRegexp),
        (&["escape", "s", "w"], IsearchAction::ToggleWord),
        (&["escape", "s", "_"], IsearchAction::ToggleSymbol),
        (
            &["escape", "s", "space"],
            IsearchAction::ToggleLaxWhitespace,
        ),
        (&["escape", "s", "i"], IsearchAction::ToggleInvisible),
        (
            &["escape", "s", "escape", "<"],
            IsearchAction::BeginningOfBuffer,
        ),
        (&["escape", "s", "escape", ">"], IsearchAction::EndOfBuffer),
    ];

    let mut is_prefix = false;
    for &(pattern, action) in bindings {
        if pattern.len() < seq.len() {
            continue;
        }
        let matches = seq.iter().zip(pattern.iter()).all(|(key, pat)| {
            let expected = Key::from_keystroke(&Keystroke::parse(pat).unwrap());
            key == &expected
        });
        if matches {
            if pattern.len() == seq.len() {
                return KeyMatch::Action(action);
            }
            is_prefix = true;
        }
    }
    if is_prefix {
        KeyMatch::Prefix
    } else {
        KeyMatch::Unbound
    }
}

fn is_modifier_only(stroke: &Keystroke) -> bool {
    matches!(
        stroke.key.as_str(),
        "shift" | "control" | "alt" | "platform" | "function"
    )
}

fn is_ime_in_progress(stroke: &Keystroke) -> bool {
    let key = stroke.key.as_str();
    stroke.key_char.is_none()
        && (key.is_empty() || key.chars().count() == 1 || key.starts_with("dead_"))
        && !(stroke.modifiers.platform
            || stroke.modifiers.control
            || stroke.modifiers.function
            || stroke.modifiers.alt)
}

fn printable_text(stroke: &Keystroke) -> Option<&str> {
    let modifiers = stroke.modifiers;
    if modifiers.control || modifiers.alt || modifiers.platform || modifiers.function {
        return None;
    }
    let printable_key = stroke.key.chars().count() == 1 || stroke.key == "space";
    let text = stroke.key_char.as_deref()?;
    if printable_key && !text.is_empty() && text.chars().all(|ch| !ch.is_control()) {
        Some(text)
    } else {
        None
    }
}

fn quoted_char_for_keystroke(stroke: &Keystroke) -> Option<String> {
    if stroke.modifiers.control && !stroke.modifiers.alt && !stroke.modifiers.platform {
        let key = stroke.key.as_str();
        if key.len() == 1 {
            let b = key.as_bytes()[0].to_ascii_lowercase();
            if b.is_ascii_lowercase() {
                let ch = (b - b'a' + 1) as char;
                return Some(ch.to_string());
            }
        }
    }
    match stroke.key.as_str() {
        "tab" => Some("\t".into()),
        "enter" | "return" | "kp-enter" => Some("\n".into()),
        "space" => Some(" ".into()),
        _ => stroke
            .key_char
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned),
    }
}

impl AppShell {
    pub(super) fn isearch_command(
        &mut self,
        command: Command,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.minibuffer.is_some() {
            return;
        }
        if self.isearch.is_some() {
            self.finish_isearch(false, false, false, Some(window), cx);
        }
        let (buffer_id, snapshot, text, opoint) = {
            let buffer = origin.read(cx).buffer();
            (
                buffer.snapshot_id().buffer,
                buffer.snapshot_id(),
                buffer.text().to_string(),
                buffer.point(),
            )
        };
        let engine = match command {
            Command::IsearchForward => IsearchEngine::new(opoint, true, SearchMode::Literal),
            Command::IsearchBackward => IsearchEngine::new(opoint, false, SearchMode::Literal),
            Command::IsearchForwardRegexp => IsearchEngine::new(opoint, true, SearchMode::Regexp),
            Command::IsearchBackwardRegexp => IsearchEngine::new(opoint, false, SearchMode::Regexp),
            Command::IsearchForwardWord => IsearchEngine::new(opoint, true, SearchMode::Word),
            Command::IsearchForwardSymbol => IsearchEngine::new(opoint, true, SearchMode::Symbol),
            Command::IsearchForwardSymbolAtPoint => {
                let is_hidden = |range| origin.read(cx).buffer().is_range_hidden(range);
                IsearchEngine::start_symbol_at_point(opoint, &text, is_hidden)
            }
            _ => {
                debug_assert!(false, "{} is not an isearch command", command.name());
                return;
            }
        };
        origin.update(cx, |view, cx| {
            view.exit_multiple_cursors(cx);
            view.set_isearch_active(true, cx);
        });
        self.isearch = Some(IsearchSession {
            view: origin.clone(),
            buffer_id,
            snapshot,
            text,
            engine,
            opened_sections: Vec::new(),
            pending_keys: Vec::new(),
            quote_next: false,
            preedit: String::new(),
            suspended_for_edit: false,
            last_visible_range: None,
        });
        self.sync_isearch(Some(window), cx);
    }

    pub(super) fn is_isearch_active(&self) -> bool {
        self.isearch
            .as_ref()
            .is_some_and(|session| !session.suspended_for_edit)
    }

    pub(super) fn route_isearch_keystroke(
        &mut self,
        stroke: &Keystroke,
        prefer_character_input: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<bool> {
        let active_view = {
            let session = self.isearch.as_ref()?;
            if session.suspended_for_edit {
                return None;
            }
            session.view.clone()
        };
        if !active_view.focus_handle(cx).is_focused(window) {
            return None;
        }
        if is_modifier_only(stroke) {
            return Some(false);
        }
        // D17 IME command barrier during isearch composition.
        if active_view.read(cx).is_isearch_composing() {
            if prefer_character_input
                || is_ime_in_progress(stroke)
                || printable_text(stroke).is_some()
            {
                return Some(false);
            }
            self.echo = Some((
                active_view.entity_id(),
                SharedString::from("Input method composition is active; finish it first"),
            ));
            cx.notify();
            return Some(true);
        }

        // `C-q` quote-next-char mode.
        if self.isearch.as_ref().is_some_and(|s| s.quote_next) {
            if is_ime_in_progress(stroke) {
                active_view.update(cx, |view, _| view.set_isearch_allow_text_input(true));
                return Some(false);
            }
            if let Some(session) = &mut self.isearch {
                session.quote_next = false;
            }
            active_view.update(cx, |view, _| view.set_isearch_allow_text_input(true));
            if let Some(quoted) = quoted_char_for_keystroke(stroke) {
                self.isearch_append(&quoted, Some(window), cx);
            } else {
                self.sync_isearch_echo(cx);
            }
            return Some(true);
        }

        let has_pending = self
            .isearch
            .as_ref()
            .is_some_and(|s| !s.pending_keys.is_empty());
        if has_pending {
            if is_ime_in_progress(stroke) {
                if let Some(session) = &mut self.isearch {
                    session.pending_keys.clear();
                }
                active_view.update(cx, |view, _| view.set_isearch_allow_text_input(true));
                self.sync_isearch_echo(cx);
                return Some(true);
            }
        } else if prefer_character_input || is_ime_in_progress(stroke) {
            return Some(false);
        }

        if let Some(session) = &mut self.isearch {
            session
                .pending_keys
                .push((stroke.clone(), prefer_character_input));
        }
        let key_match = lookup_isearch_keys(&self.isearch.as_ref().unwrap().pending_keys);
        match key_match {
            KeyMatch::Prefix => {
                active_view.update(cx, |view, _| view.set_isearch_allow_text_input(false));
                self.sync_isearch_echo(cx);
                Some(true)
            }
            KeyMatch::Action(action) => {
                if let Some(session) = &mut self.isearch {
                    session.pending_keys.clear();
                }
                active_view.update(cx, |view, _| view.set_isearch_allow_text_input(true));
                self.run_isearch_action(action, window, cx);
                Some(true)
            }
            KeyMatch::Unbound => {
                let replay = self
                    .isearch
                    .as_mut()
                    .map(|session| std::mem::take(&mut session.pending_keys))
                    .unwrap_or_default();
                active_view.update(cx, |view, _| view.set_isearch_allow_text_input(true));
                if replay.len() == 1
                    && let Some(text) = printable_text(&replay[0].0)
                {
                    let text = text.to_owned();
                    self.isearch_append(&text, Some(window), cx);
                    return Some(true);
                }
                // Non-isearch key exits isearch cleanly and executes normally.
                self.finish_isearch(false, false, true, Some(window), cx);
                let mut consumed = false;
                for (keystroke, prefer) in replay {
                    consumed |= self.route_keystroke(&keystroke, prefer, window, cx);
                }
                Some(consumed)
            }
        }
    }

    pub(super) fn handle_isearch_preedit(
        &mut self,
        source: EntityId,
        preedit: SharedString,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = &mut self.isearch else {
            return;
        };
        if session.suspended_for_edit || session.view.entity_id() != source {
            return;
        }
        session.preedit = preedit.to_string();
        self.sync_isearch_echo(cx);
    }

    pub(super) fn handle_isearch_commit(
        &mut self,
        source: EntityId,
        text: SharedString,
        cx: &mut Context<Self>,
    ) {
        let matches = self.isearch.as_ref().is_some_and(|session| {
            !session.suspended_for_edit && session.view.entity_id() == source
        });
        if !matches {
            return;
        }
        let view = if let Some(session) = &mut self.isearch {
            session.preedit.clear();
            let was_quote = std::mem::take(&mut session.quote_next);
            was_quote.then(|| session.view.clone())
        } else {
            None
        };
        if let Some(view) = view {
            view.update(cx, |view, _| view.set_isearch_allow_text_input(true));
        }
        if !text.is_empty() {
            self.isearch_append(text.as_ref(), None, cx);
        } else {
            self.sync_isearch_echo(cx);
        }
    }

    pub(super) fn handle_isearch_pointer_down(&mut self, cx: &mut Context<Self>) {
        if self.is_isearch_active() {
            self.finish_isearch_inner(false, false, true, true, true, None, cx);
        }
    }

    pub(super) fn refresh_isearch_viewport(&mut self, source: EntityId, cx: &mut Context<Self>) {
        let Some(visible) = self
            .isearch
            .as_ref()
            .filter(|session| session.view.entity_id() == source)
            .and_then(|session| session.view.read(cx).visible_byte_range())
        else {
            return;
        };
        self.refresh_isearch_viewport_range(source, visible, cx);
    }

    pub(super) fn refresh_isearch_viewport_range(
        &mut self,
        source: EntityId,
        visible_range: std::ops::Range<usize>,
        cx: &mut Context<Self>,
    ) {
        self.refresh_isearch_snapshot(cx);
        let Some(session) = &mut self.isearch else {
            return;
        };
        if session.view.entity_id() != source {
            return;
        }
        if session.last_visible_range.as_ref() == Some(&visible_range) {
            return;
        }
        session.last_visible_range = Some(visible_range.clone());
        let view = session.view.clone();
        let text = session.text.clone();
        let lazy = {
            let is_hidden = |range| view.read(cx).buffer().is_range_hidden(range);
            session
                .engine
                .lazy_highlights(&text, visible_range, is_hidden)
        };
        let current = session.engine.match_range();
        view.update(cx, |view, cx| {
            view.set_isearch_highlight(current, lazy, cx);
        });
    }

    fn isearch_append(
        &mut self,
        addition: &str,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) {
        self.refresh_isearch_snapshot(cx);
        let Some(session) = &mut self.isearch else {
            return;
        };
        let view = session.view.clone();
        let text = session.text.clone();
        let is_hidden = |range| view.read(cx).buffer().is_range_hidden(range);
        session.engine.append_text(&text, addition, is_hidden);
        self.sync_isearch(window, cx);
    }

    fn run_isearch_action(
        &mut self,
        action: IsearchAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.refresh_isearch_snapshot(cx);
        match action {
            IsearchAction::Exit => {
                let empty = self
                    .isearch
                    .as_ref()
                    .is_some_and(|session| session.engine.state.query.is_empty());
                if empty {
                    self.begin_isearch_edit(true, window, cx);
                } else {
                    self.finish_isearch(false, false, false, Some(window), cx);
                }
                return;
            }
            IsearchAction::Cancel => {
                self.finish_isearch(true, true, false, Some(window), cx);
                return;
            }
            IsearchAction::Abort => {
                let outcome = self
                    .isearch
                    .as_mut()
                    .map(|session| session.engine.abort())
                    .unwrap_or(AbortOutcome::Cancelled);
                match outcome {
                    AbortOutcome::Cancelled => {
                        self.finish_isearch(true, true, false, Some(window), cx);
                        return;
                    }
                    AbortOutcome::Reverted => {}
                }
            }
            IsearchAction::EditString => {
                self.begin_isearch_edit(false, window, cx);
                return;
            }
            IsearchAction::QuoteChar => {
                if let Some(session) = &mut self.isearch {
                    session.quote_next = true;
                    session
                        .view
                        .update(cx, |view, _| view.set_isearch_allow_text_input(false));
                }
                self.sync_isearch_echo(cx);
                return;
            }
            IsearchAction::InsertTab => {
                self.isearch_append("\t", Some(window), cx);
                return;
            }
            IsearchAction::InsertNewline => {
                self.isearch_append("\n", Some(window), cx);
                return;
            }
            IsearchAction::RepeatForward | IsearchAction::RepeatBackward => {
                let forward = action == IsearchAction::RepeatForward;
                let ring = cx.global::<IsearchRing>().clone();
                if let Some(session) = &mut self.isearch {
                    let view = session.view.clone();
                    let text = session.text.clone();
                    let is_hidden = |range| view.read(cx).buffer().is_range_hidden(range);
                    session.engine.repeat(&text, forward, &ring, is_hidden);
                }
            }
            IsearchAction::DeleteChar => {
                if let Some(session) = &mut self.isearch {
                    session.engine.delete_char();
                }
            }
            IsearchAction::DelChar => {
                if let Some(session) = &mut self.isearch {
                    let view = session.view.clone();
                    let text = session.text.clone();
                    let is_hidden = |range| view.read(cx).buffer().is_range_hidden(range);
                    session.engine.del_char(&text, is_hidden);
                }
            }
            IsearchAction::YankWordOrChar => {
                if let Some(session) = &mut self.isearch {
                    let view = session.view.clone();
                    let text = session.text.clone();
                    let is_hidden = |range| view.read(cx).buffer().is_range_hidden(range);
                    session.engine.yank_word_or_char(&text, is_hidden);
                }
            }
            IsearchAction::YankSymbolOrChar => {
                if let Some(session) = &mut self.isearch {
                    let view = session.view.clone();
                    let text = session.text.clone();
                    let is_hidden = |range| view.read(cx).buffer().is_range_hidden(range);
                    session.engine.yank_symbol_or_char(&text, is_hidden);
                }
            }
            IsearchAction::YankLine => {
                if let Some(session) = &mut self.isearch {
                    let view = session.view.clone();
                    let text = session.text.clone();
                    let is_hidden = |range| view.read(cx).buffer().is_range_hidden(range);
                    session.engine.yank_line(&text, is_hidden);
                }
            }
            IsearchAction::YankKill => {
                let killed = {
                    let clipboard = crate::platform::GpuiClipboard::new(cx);
                    cx.global::<crate::editing::KillRing>()
                        .current_kill(Some(&clipboard))
                        .map(|rope| rope.to_string())
                        .unwrap_or_default()
                };
                if !killed.is_empty()
                    && let Some(session) = &mut self.isearch
                {
                    let view = session.view.clone();
                    let text = session.text.clone();
                    let is_hidden = |range| view.read(cx).buffer().is_range_hidden(range);
                    session.engine.yank_string(&text, &killed, is_hidden);
                }
            }
            IsearchAction::RingRetreat | IsearchAction::RingAdvance => {
                let advance = action == IsearchAction::RingAdvance;
                let ring = cx.global::<IsearchRing>().clone();
                if let Some(session) = &mut self.isearch {
                    let view = session.view.clone();
                    let text = session.text.clone();
                    let is_hidden = |range| view.read(cx).buffer().is_range_hidden(range);
                    session.engine.ring_step(&text, advance, &ring, is_hidden);
                }
            }
            IsearchAction::ToggleCaseFold => {
                if let Some(session) = &mut self.isearch {
                    let view = session.view.clone();
                    let text = session.text.clone();
                    let is_hidden = |range| view.read(cx).buffer().is_range_hidden(range);
                    session.engine.toggle_case_fold(&text, is_hidden);
                }
            }
            IsearchAction::ToggleRegexp => {
                if let Some(session) = &mut self.isearch {
                    let view = session.view.clone();
                    let text = session.text.clone();
                    let is_hidden = |range| view.read(cx).buffer().is_range_hidden(range);
                    session.engine.toggle_regexp(&text, is_hidden);
                }
            }
            IsearchAction::ToggleWord => {
                if let Some(session) = &mut self.isearch {
                    let view = session.view.clone();
                    let text = session.text.clone();
                    let is_hidden = |range| view.read(cx).buffer().is_range_hidden(range);
                    session.engine.toggle_word(&text, is_hidden);
                }
            }
            IsearchAction::ToggleSymbol => {
                if let Some(session) = &mut self.isearch {
                    let view = session.view.clone();
                    let text = session.text.clone();
                    let is_hidden = |range| view.read(cx).buffer().is_range_hidden(range);
                    session.engine.toggle_symbol(&text, is_hidden);
                }
            }
            IsearchAction::ToggleLaxWhitespace => {
                if let Some(session) = &mut self.isearch {
                    let view = session.view.clone();
                    let text = session.text.clone();
                    let is_hidden = |range| view.read(cx).buffer().is_range_hidden(range);
                    session.engine.toggle_lax_whitespace(&text, is_hidden);
                }
            }
            IsearchAction::ToggleInvisible => {
                // If turning invisible search OFF while sections are temporarily expanded,
                // re-collapse them first so `is_range_hidden` sees them as hidden during re-search.
                let collapse_first = self.isearch.as_ref().and_then(|session| {
                    (session.engine.state.search_invisible && !session.opened_sections.is_empty())
                        .then(|| (session.view.clone(), session.buffer_id))
                });
                if let Some((view, buffer_id)) = collapse_first {
                    let mut opened = self
                        .isearch
                        .as_mut()
                        .map(|s| std::mem::take(&mut s.opened_sections))
                        .unwrap_or_default();
                    let changed = view.update(cx, |view, cx| {
                        view.isearch_finalize_opened(None, &mut opened, cx)
                    });
                    if changed {
                        self.refresh_buffer(buffer_id, cx);
                    }
                }
                if let Some(session) = &mut self.isearch {
                    let view = session.view.clone();
                    let text = session.text.clone();
                    let is_hidden = |range| view.read(cx).buffer().is_range_hidden(range);
                    session.engine.toggle_invisible(&text, is_hidden);
                }
            }
            IsearchAction::BeginningOfBuffer | IsearchAction::EndOfBuffer => {
                let beginning = action == IsearchAction::BeginningOfBuffer;
                if let Some(session) = &mut self.isearch {
                    let view = session.view.clone();
                    let text = session.text.clone();
                    let is_hidden = |range| view.read(cx).buffer().is_range_hidden(range);
                    session.engine.jump_to_boundary(&text, beginning, is_hidden);
                }
            }
        }
        self.sync_isearch(Some(window), cx);
    }

    fn begin_isearch_edit(
        &mut self,
        nonincremental: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = &mut self.isearch else {
            return;
        };
        session.suspended_for_edit = true;
        session.preedit.clear();
        session.pending_keys.clear();
        session.quote_next = false;
        let origin = session.view.clone();
        let initial = session.engine.state.query.clone();
        let forward = session.engine.state.forward;
        let prompt = session.engine.message_prefix(nonincremental);
        origin.update(cx, |view, cx| view.set_isearch_active(false, cx));
        if self
            .begin_read(
                ReadKind::IsearchEdit {
                    forward,
                    nonincremental,
                    prompt,
                },
                &origin,
                &initial,
                window,
                cx,
            )
            .is_none()
            && let Some(session) = &mut self.isearch
        {
            session.suspended_for_edit = false;
            origin.update(cx, |view, cx| view.set_isearch_active(true, cx));
        }
    }

    pub(super) fn confirm_isearch_edit(
        &mut self,
        id: u64,
        forward: bool,
        nonincremental: bool,
        force_resume: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut input = self.session_input(cx).unwrap_or_default();
        if self.end_session(id, window, cx).is_none() {
            self.finish_isearch(true, false, true, Some(window), cx);
            return;
        }
        self.refresh_isearch_snapshot(cx);
        let Some(session) = &mut self.isearch else {
            return;
        };
        session.suspended_for_edit = false;
        if input.is_empty() && nonincremental {
            let regexp = session.engine.state.mode.uses_regexp_ring();
            if let Some(last) = cx.global::<IsearchRing>().latest(regexp) {
                input = last.to_owned();
            }
        }
        let view = session.view.clone();
        let text = session.text.clone();
        let is_hidden = |range| view.read(cx).buffer().is_range_hidden(range);
        session
            .engine
            .apply_edited_query(&text, input.clone(), forward, is_hidden);

        if nonincremental && !force_resume {
            let succeeded = session.engine.state.success;
            let error = session.engine.state.error.clone();
            self.sync_isearch(Some(window), cx);
            if succeeded {
                self.finish_isearch(false, false, false, Some(window), cx);
            } else {
                self.finish_isearch(true, false, true, Some(window), cx);
                let message = match error {
                    Some(err) => format!("Search failed ({err}): {input}"),
                    None => format!("Search failed: {input}"),
                };
                self.show_message(&view, message, cx);
            }
        } else {
            view.update(cx, |view, cx| view.set_isearch_active(true, cx));
            self.sync_isearch(Some(window), cx);
        }
    }

    pub(super) fn cancel_isearch_edit(
        &mut self,
        nonincremental: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if nonincremental {
            self.finish_isearch(true, true, false, Some(window), cx);
            return;
        }
        let Some(session) = &mut self.isearch else {
            return;
        };
        session.suspended_for_edit = false;
        let view = session.view.clone();
        view.update(cx, |view, cx| view.set_isearch_active(true, cx));
        self.sync_isearch(Some(window), cx);
    }

    fn refresh_isearch_snapshot(&mut self, cx: &App) {
        let Some(session) = &mut self.isearch else {
            return;
        };
        let buffer = session.view.read(cx).buffer();
        let current = buffer.snapshot_id();
        if current != session.snapshot {
            session.snapshot = current;
            session.text = buffer.text().to_string();
            session.engine.clamp_to_text(&session.text);
        }
    }

    fn sync_isearch(&mut self, window: Option<&mut Window>, cx: &mut Context<Self>) {
        self.refresh_isearch_snapshot(cx);
        let Some(session) = &mut self.isearch else {
            return;
        };
        let view = session.view.clone();
        let buffer_id = session.buffer_id;
        let match_range = session.engine.match_range();
        let point = session.engine.state.point;
        let search_invisible = session.engine.state.search_invisible;

        // Open/re-collapse hidden sections for the current match (`search-invisible = 'open`).
        if search_invisible {
            let reveal_range = match_range.clone().unwrap_or(point..point);
            let mut opened = std::mem::take(&mut session.opened_sections);
            let changed = view.update(cx, |view, cx| {
                view.isearch_temp_open(reveal_range, &mut opened, cx)
            });
            if let Some(session) = &mut self.isearch {
                session.opened_sections = opened;
            }
            if changed {
                self.refresh_buffer(buffer_id, cx);
            }
        } else if !session.opened_sections.is_empty() {
            let mut opened = std::mem::take(&mut session.opened_sections);
            let changed = view.update(cx, |view, cx| {
                view.isearch_finalize_opened(None, &mut opened, cx)
            });
            if changed {
                self.refresh_buffer(buffer_id, cx);
            }
        }

        // Move view point and reveal caret (centered if offscreen, horizontal scroll if truncated).
        view.update(cx, |view, cx| {
            view.show_isearch_point(point, window, cx);
        });

        let Some(session) = &mut self.isearch else {
            return;
        };
        let visible_range = view.read(cx).visible_byte_range().unwrap_or_else(|| {
            let start = point.0.saturating_sub(16 * 1024);
            let end = (point.0 + 16 * 1024).min(session.text.len());
            start..end
        });
        session.last_visible_range = view.read(cx).visible_byte_range();
        let text = session.text.clone();
        let lazy = {
            let is_hidden = |range| view.read(cx).buffer().is_range_hidden(range);
            session
                .engine
                .lazy_highlights(&text, visible_range, is_hidden)
        };
        view.update(cx, |view, cx| {
            view.set_isearch_highlight(match_range, lazy, cx);
        });
        self.sync_isearch_echo(cx);
    }

    fn sync_isearch_echo(&mut self, cx: &mut Context<Self>) {
        let Some(session) = &self.isearch else {
            return;
        };
        if session.suspended_for_edit {
            return;
        }
        let message = if !session.pending_keys.is_empty() {
            let mut base = session
                .engine
                .prompt_message(Some(&session.preedit), session.quote_next);
            base.push_str(" [");
            for (i, (stroke, _)) in session.pending_keys.iter().enumerate() {
                if i > 0 {
                    base.push(' ');
                }
                if stroke.modifiers.control {
                    base.push_str("C-");
                }
                if stroke.modifiers.alt {
                    base.push_str("M-");
                }
                base.push_str(match stroke.key.as_str() {
                    "escape" => "ESC",
                    key => key,
                });
            }
            base.push_str("-]");
            base
        } else {
            session
                .engine
                .prompt_message(Some(&session.preedit), session.quote_next)
        };
        self.echo = Some((session.view.entity_id(), SharedString::from(message)));
        cx.notify();
    }

    /// Ends the active isearch session.
    ///
    /// - `nopush`: do not record `query` in `IsearchRing` (abort / cancel / failure).
    /// - `restore_origin`: jump point back to `opoint` and re-collapse all temp sections.
    /// - `quiet`: do not display `"Mark saved where search started"` or `"Quit"`.
    pub(super) fn finish_isearch(
        &mut self,
        nopush: bool,
        restore_origin: bool,
        quiet: bool,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) {
        self.finish_isearch_inner(nopush, restore_origin, quiet, false, false, window, cx);
    }

    #[allow(clippy::too_many_arguments)]
    fn finish_isearch_inner(
        &mut self,
        nopush: bool,
        restore_origin: bool,
        quiet: bool,
        preserve_echo: bool,
        keep_by_point_only: bool,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) {
        self.refresh_isearch_snapshot(cx);
        let Some(mut session) = self.isearch.take() else {
            return;
        };
        let view = session.view.clone();
        let buffer_id = session.buffer_id;
        let opoint = session.engine.opoint;
        let query = session.engine.state.query.clone();
        let regexp_ring = session.engine.state.mode.uses_regexp_ring();

        if restore_origin {
            let changed = view.update(cx, |view, cx| {
                let changed = view.isearch_finalize_opened(None, &mut session.opened_sections, cx);
                view.show_isearch_point(opoint, window, cx);
                view.clear_isearch(cx);
                changed
            });
            if changed {
                self.refresh_buffer(buffer_id, cx);
            }
            if !quiet {
                self.show_message(&view, "Quit", cx);
            } else if !preserve_echo {
                self.echo = None;
                cx.notify();
            }
            return;
        }

        let current_point = view.read(cx).buffer().point();
        let keep_range = if keep_by_point_only {
            Some(current_point..current_point)
        } else {
            session
                .engine
                .match_range()
                .or(Some(current_point..current_point))
        };
        let changed = view.update(cx, |view, cx| {
            let changed =
                view.isearch_finalize_opened(keep_range, &mut session.opened_sections, cx);
            view.clear_isearch(cx);
            changed
        });
        if changed {
            self.refresh_buffer(buffer_id, cx);
        }

        if !nopush && !query.is_empty() {
            cx.global_mut::<IsearchRing>().push(&query, regexp_ring);
        }

        let saved_mark = !keep_by_point_only
            && current_point != opoint
            && view.update(cx, |view, cx| view.save_mark(opoint, cx));

        if !quiet && saved_mark {
            self.show_message(&view, "Mark saved where search started", cx);
        } else if !preserve_echo {
            self.echo = None;
            cx.notify();
        }
    }

    pub(super) fn finish_isearch_if_buffer(
        &mut self,
        buffer_id: BufferId,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) {
        if self
            .isearch
            .as_ref()
            .is_some_and(|session| session.buffer_id == buffer_id)
        {
            self.finish_isearch(true, false, true, window, cx);
        }
    }
}
