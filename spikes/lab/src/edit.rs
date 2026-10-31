//! Editable targets, kill ring and the own-command primitives for the spike (S2).
//!
//! Every command edits through `EditTarget`, so one code path feeds the undo log
//! and (later) the mark updates.

use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

use gpui_kit::component::input::{EditorState, InputState};
use gpui_kit::*;

use crate::target::EditTarget;
use crate::undo;

/// Minimal view of the text state the command layer is allowed to see.
pub trait Editable: 'static + Sized {
    fn text_string(&self) -> String;
    fn point(&self) -> usize;
    fn set_point(&mut self, pos: usize, cx: &mut Context<Self>);
    fn selection(&self) -> Range<usize>;
}

impl Editable for EditorState {
    fn text_string(&self) -> String {
        self.text().to_string()
    }
    fn point(&self) -> usize {
        self.cursor()
    }
    fn set_point(&mut self, pos: usize, cx: &mut Context<Self>) {
        self.set_selected_range(pos..pos, cx);
    }
    fn selection(&self) -> Range<usize> {
        self.selected_range()
    }
}

impl Editable for InputState {
    fn text_string(&self) -> String {
        self.text().to_string()
    }
    fn point(&self) -> usize {
        self.cursor()
    }
    fn set_point(&mut self, pos: usize, cx: &mut Context<Self>) {
        self.set_selected_range(pos..pos, cx);
    }
    fn selection(&self) -> Range<usize> {
        self.selected_range()
    }
}

#[derive(Default)]
pub struct KillRing {
    entries: Vec<String>,
    appending: bool,
}

impl KillRing {
    /// Any command that is not a kill interrupts the append chain.
    pub fn break_chain(&mut self) {
        self.appending = false;
    }

    pub fn kill(&mut self, text: String, append: bool) {
        if text.is_empty() {
            return;
        }
        if append && self.appending {
            if let Some(last) = self.entries.last_mut() {
                last.push_str(&text);
                return;
            }
        }
        self.entries.push(text);
        self.appending = append;
    }

    pub fn latest(&self) -> Option<String> {
        self.entries.last().cloned()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

pub type SharedKillRing = Rc<RefCell<KillRing>>;

/// Apply one edit through the target adapter, then sync point.
pub fn apply<S: Editable + EntityInputHandler>(
    state: &Entity<S>,
    target: &Entity<EditTarget<S>>,
    undo: &undo::SharedUndo,
    range: Range<usize>,
    text: &str,
    kind: undo::Kind,
    window: &mut Window,
    cx: &mut App,
) {
    let doc = state.read(cx).text_string();
    let utf16 = |byte: usize| doc[..byte.min(doc.len())].encode_utf16().count();
    let (start, end) = (utf16(range.start), utf16(range.end));
    let new_point = range.start + text.len();
    undo::with_command_kind(kind, || {
        target.update(cx, |t, cx| {
            EntityInputHandler::replace_text_in_range(t, Some(start..end), text, window, cx)
        });
    });
    state.update(cx, |st, cx| st.set_point(new_point, cx));
    undo.borrow_mut().end_command(true);
}

fn char_before(text: &str, point: usize) -> usize {
    text[..point.min(text.len())]
        .char_indices()
        .next_back()
        .map(|(i, _)| i)
        .unwrap_or(0)
}

fn char_after(text: &str, point: usize) -> usize {
    text[point.min(text.len())..]
        .char_indices()
        .nth(1)
        .map(|(i, _)| point + i)
        .unwrap_or(point.min(text.len()))
}

pub fn delete_backward<S: Editable + EntityInputHandler>(
    state: &Entity<S>,
    target: &Entity<EditTarget<S>>,
    undo: &undo::SharedUndo,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    let doc = state.read(cx).text_string();
    let point = state.read(cx).point();
    if point == 0 {
        undo.borrow_mut().end_command(false);
        return false;
    }
    let start = char_before(&doc, point);
    apply(state, target, undo, start..point, "", undo::Kind::DeleteBackward, window, cx);
    true
}

pub fn delete_forward<S: Editable + EntityInputHandler>(
    state: &Entity<S>,
    target: &Entity<EditTarget<S>>,
    undo: &undo::SharedUndo,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    let doc = state.read(cx).text_string();
    let point = state.read(cx).point();
    if point >= doc.len() {
        undo.borrow_mut().end_command(false);
        return false;
    }
    let end = char_after(&doc, point);
    apply(state, target, undo, point..end, "", undo::Kind::DeleteForward, window, cx);
    true
}

pub fn insert_text<S: Editable + EntityInputHandler>(
    state: &Entity<S>,
    target: &Entity<EditTarget<S>>,
    undo: &undo::SharedUndo,
    text: &str,
    kind: undo::Kind,
    window: &mut Window,
    cx: &mut App,
) {
    let point = state.read(cx).point();
    apply(state, target, undo, point..point, text, kind, window, cx);
}

pub fn move_to<S: Editable>(state: &Entity<S>, point: usize, undo: &undo::SharedUndo, cx: &mut App) {
    state.update(cx, |st, cx| st.set_point(point, cx));
    undo.borrow_mut().end_command(false);
}

pub fn line_start(text: &str, point: usize) -> usize {
    text[..point.min(text.len())]
        .rfind('\n')
        .map(|i| i + 1)
        .unwrap_or(0)
}

pub fn line_end(text: &str, point: usize) -> usize {
    let after = point.min(text.len());
    text[after..]
        .find('\n')
        .map(|i| after + i)
        .unwrap_or(text.len())
}

/// Returns `Err(())` when there is nothing to kill (Emacs reports "End of buffer"
/// and does not create an undo entry).
pub fn kill_line<S: Editable + EntityInputHandler>(
    state: &Entity<S>,
    target: &Entity<EditTarget<S>>,
    undo: &undo::SharedUndo,
    ring: &SharedKillRing,
    window: &mut Window,
    cx: &mut App,
) -> Result<(), ()> {
    let doc = state.read(cx).text_string();
    let point = state.read(cx).point();
    if point >= doc.len() {
        undo.borrow_mut().end_command(false);
        return Err(());
    }
    let end = line_end(&doc, point);
    // Emacs kills the newline when point is at end of line.
    let end = if end == point {
        char_after(&doc, point)
    } else {
        end
    };
    let killed = doc[point..end].to_string();
    ring.borrow_mut().kill(killed, true);
    apply(state, target, undo, point..end, "", undo::Kind::Other, window, cx);
    Ok(())
}

pub fn yank<S: Editable + EntityInputHandler>(
    state: &Entity<S>,
    target: &Entity<EditTarget<S>>,
    undo: &undo::SharedUndo,
    ring: &SharedKillRing,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    let Some(text) = ring.borrow().latest() else {
        undo.borrow_mut().end_command(false);
        return false;
    };
    insert_text(state, target, undo, &text, undo::Kind::Other, window, cx);
    true
}

pub fn kill_region<S: Editable + EntityInputHandler>(
    state: &Entity<S>,
    target: &Entity<EditTarget<S>>,
    undo: &undo::SharedUndo,
    ring: &SharedKillRing,
    region: Range<usize>,
    save_only: bool,
    window: &mut Window,
    cx: &mut App,
) {
    let doc = state.read(cx).text_string();
    let text = doc[region.clone()].to_string();
    ring.borrow_mut().kill(text, false);
    if save_only {
        state.update(cx, |st, cx| st.set_point(region.start, cx));
        undo.borrow_mut().end_command(false);
        return;
    }
    apply(state, target, undo, region, "", undo::Kind::Other, window, cx);
}
