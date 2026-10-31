//! Minimal `EditTarget` adapter for the spike.
//!
//! Every platform text input call is forwarded to the underlying state and
//! recorded as `{range, old, new, kind}` for the own undo stack. This is the
//! shape semacs needs for S4: the platform handler talks to our entity, so
//! ranges, preedit and commit are observable instead of being inferred from
//! `InputEvent::Change`.

use std::ops::Range;

use gpui_kit::*;

use crate::edit::Editable;
use crate::log_line;
use crate::mark;
use crate::undo;

pub struct EditTarget<S: 'static> {
    state: Entity<S>,
    name: &'static str,
    undo: undo::SharedUndo,
    mark: mark::SharedMark,
}

impl<S: 'static> EditTarget<S> {
    pub fn new(
        state: Entity<S>,
        name: &'static str,
        undo: undo::SharedUndo,
        mark: mark::SharedMark,
    ) -> Self {
        Self {
            state,
            name,
            undo,
            mark,
        }
    }

    fn note(&self, what: &str, extra: impl AsRef<str>) {
        log_line(format!("TARGET[{}] {what} {}", self.name, extra.as_ref()));
    }
}

/// UTF-16 range (what the platform speaks) to UTF-8 byte range (what we use).
fn utf16_range_to_byte(text: &str, range: Range<usize>) -> (usize, usize) {
    let mut start = None;
    let mut end = None;
    let mut utf16 = 0usize;
    for (byte, ch) in text.char_indices() {
        if start.is_none() && utf16 >= range.start {
            start = Some(byte);
        }
        if end.is_none() && utf16 >= range.end {
            end = Some(byte);
        }
        if start.is_some() && end.is_some() {
            break;
        }
        utf16 += ch.len_utf16();
    }
    let start = start.unwrap_or(text.len());
    let end = end.unwrap_or(text.len());
    (start, end)
}

impl<S: Editable + EntityInputHandler> EntityInputHandler for EditTarget<S> {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        self.state.update(cx, |state, cx| {
            EntityInputHandler::text_for_range(state, range, adjusted_range, window, cx)
        })
    }

    fn selected_text_range(
        &mut self,
        ignore_disabled_input: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        self.state.update(cx, |state, cx| {
            EntityInputHandler::selected_text_range(state, ignore_disabled_input, window, cx)
        })
    }

    fn marked_text_range(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        let marked = self.state.update(cx, |state, cx| {
            EntityInputHandler::marked_text_range(state, window, cx)
        });
        if marked.is_some() {
            self.note("marked_text_range", format!("{marked:?}"));
        }
        marked
    }

    fn unmark_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.note("unmark_text", "()");
        self.state.update(cx, |state, cx| {
            EntityInputHandler::unmark_text(state, window, cx)
        })
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let doc = self.state.read(cx).text_string();
        // A platform IME commit/cancel speaks about the marked (preedit) range.
        // GPUI passes `None` for commits and the marked range for cancels; both
        // refer to the preedit text that is already in the document but was
        // never a user edit. Resolve them here so the undo record stays true.
        let marked = self.state.update(cx, |state, cx| {
            EntityInputHandler::marked_text_range(state, window, cx)
        });
        let targets_marked = match (&range, &marked) {
            (None, Some(_)) => true,
            (Some(r), Some(m)) => r == m,
            _ => false,
        };
        let (start, end) = if targets_marked {
            utf16_range_to_byte(&doc, marked.clone().unwrap())
        } else {
            match range.clone() {
                Some(range) => utf16_range_to_byte(&doc, range),
                None => {
                    let sel = self.state.read(cx).selection();
                    if sel.is_empty() {
                        let point = self.state.read(cx).point();
                        (point, point)
                    } else {
                        (sel.start, sel.end)
                    }
                }
            }
        };
        let old = doc[start..end].to_string();
        let point_before = self.state.read(cx).point();
        let mark_collapse = if targets_marked {
            None
        } else {
            self.mark.borrow().collapse_offset(start..end)
        };
        let marked_note = marked
            .as_ref()
            .map(|m| format!(" marked={m:?}"))
            .unwrap_or_default();
        self.note(
            "replace_text_in_range",
            format!(
                "range_utf16={range:?}{marked_note} bytes={start}..{end} text={text:?}"
            ),
        );
        self.state.update(cx, |state, cx| {
            EntityInputHandler::replace_text_in_range(state, range, text, window, cx)
        });
        if targets_marked {
            // Preedit text is temporary: it was never marker-adjusted, so only
            // a commit behaves as a real insertion at the preedit start.
            // Cancelling (empty text) leaves the mark untouched.
            if !text.is_empty() {
                self.mark
                    .borrow_mut()
                    .after_replace(start..start, text.len());
                if !undo::replaying() {
                    // Platform input / IME commits are command boundaries too:
                    // they deactivate the region but keep the mark position.
                    self.mark.borrow_mut().set_active(false);
                    // The commit's previous text is empty and undo simply
                    // removes the committed text.
                    self.undo.borrow_mut().record(
                        undo::Change {
                            range: start..start,
                            old: String::new(),
                            new: text.to_string(),
                            kind: undo::Kind::Typing,
                            mark_collapse: None,
                        },
                        point_before,
                    );
                }
            }
            return;
        }
        self.mark.borrow_mut().after_replace(start..end, text.len());
        if !undo::replaying() {
            self.mark.borrow_mut().set_active(false);
            let kind = undo::take_pending_kind().unwrap_or({
                if start == end && !text.contains('\n') {
                    undo::Kind::Typing
                } else {
                    undo::Kind::Other
                }
            });
            self.undo.borrow_mut().record(
                undo::Change {
                    range: start..end,
                    old,
                    new: text.to_string(),
                    kind,
                    mark_collapse,
                },
                point_before,
            );
        }
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        new_text: &str,
        new_selected_range: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.note(
            "replace_and_mark_text_in_range",
            format!("range_utf16={range:?} text={new_text:?} sel={new_selected_range:?}"),
        );
        self.state.update(cx, |state, cx| {
            EntityInputHandler::replace_and_mark_text_in_range(
                state,
                range,
                new_text,
                new_selected_range,
                window,
                cx,
            )
        })
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        element_bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        self.state.update(cx, |state, cx| {
            EntityInputHandler::bounds_for_range(state, range_utf16, element_bounds, window, cx)
        })
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<usize> {
        self.state.update(cx, |state, cx| {
            EntityInputHandler::character_index_for_point(state, point, window, cx)
        })
    }

    fn set_selected_text_range(
        &mut self,
        range_utf16: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.note("set_selected_text_range", format!("{range_utf16:?}"));
        self.state.update(cx, |state, cx| {
            EntityInputHandler::set_selected_text_range(state, range_utf16, window, cx)
        })
    }

    fn text_length_utf16(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<usize> {
        self.state.update(cx, |state, cx| {
            EntityInputHandler::text_length_utf16(state, window, cx)
        })
    }

    /// Gate for platform text input while a prefix is pending or the target is
    /// read-only. The interceptor gates keystrokes separately.
    ///
    /// Turning this off also makes GPUI disable the IME, which cancels an
    /// in-flight composition. There is no public API to commit it first, so
    /// commands that run while composing see the preedit text (see EVIDENCE).
    fn accepts_text_input(&self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let accepts = self.state.update(cx, |state, cx| {
            EntityInputHandler::accepts_text_input(state, window, cx)
        });
        let allowed = accepts && !crate::prefix_pending() && !crate::readonly_mode();
        if !allowed {
            self.note("accepts_text_input", format!("inner={accepts} allowed={allowed}"));
        }
        allowed
    }
}
