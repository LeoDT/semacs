//! The editing target for a text buffer.
//!
//! `Buffer` owns the gpui-kit `EditorState` and is the only product type that
//! talks to it. Platform text input is routed to `Buffer` as an
//! `EntityInputHandler`, which is also the single place where committed edits
//! will later feed the own undo history and mark markers (D16 / D18). M0 only
//! needs the routing plus a conservative modified flag.

use std::ops::Range;
use std::path::{Path, PathBuf};

use gpui_kit::component::input::{EditorState, InputEvent};
use gpui_kit::*;

use crate::files::FileFormat;

pub mod view;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BufferEvent {
    Changed,
}

pub struct Buffer {
    editor: Entity<EditorState>,
    path: Option<PathBuf>,
    format: FileFormat,
    /// Text at load time. M0 compares against it only while the buffer is
    /// clean; M1 replaces this with the own undo history and save point (D16).
    clean_text: String,
    modified: bool,
    _editor_events: Subscription,
    _editor_observed: Subscription,
}

impl Buffer {
    pub fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        path: Option<PathBuf>,
        text: String,
        format: FileFormat,
    ) -> Self {
        let clean_text = text.clone();
        let editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .default_value(text)
                // M0 uses hard wrapping: soft-wrapping the 19k-char perf lines
                // made the first focused frame take seconds (release) to tens of
                // seconds (debug). Revisit in M3 with measured wrap behavior.
                .soft_wrap(false)
                .line_number(false)
        });
        let editor_events = cx.subscribe_in(
            &editor,
            window,
            |buffer, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::Change) {
                    buffer.on_editor_change(window, cx);
                    cx.emit(BufferEvent::Changed);
                }
            },
        );
        // Cursor movement, selection and native unmark paths repaint the editor
        // without emitting Change; the mode line and view still need a signal.
        let editor_observed = cx.observe(&editor, |_, _, cx| cx.emit(BufferEvent::Changed));
        Self {
            editor,
            path,
            format,
            clean_text,
            modified: false,
            _editor_events: editor_events,
            _editor_observed: editor_observed,
        }
    }

    /// EditorState also accepts native actions and IME commits that never pass
    /// through our command layer in M0. Observe every completed text change and
    /// derive dirty from it; preedit is temporary and therefore skipped.
    fn on_editor_change(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.modified {
            return;
        }
        let marked = self.editor.update(cx, |editor, cx| {
            EntityInputHandler::marked_text_range(editor, window, cx)
        });
        if marked.is_some() {
            return;
        }
        let current_text = self.editor.read(cx).text().to_string();
        if current_text != self.clean_text {
            self.modified = true;
        }
    }

    pub(crate) fn editor(&self) -> Entity<EditorState> {
        self.editor.clone()
    }

    /// Move point without dispatching a base action; M1 uses this for motion.
    pub fn set_point(&mut self, point: usize, cx: &mut Context<Self>) {
        self.editor
            .update(cx, |editor, cx| editor.set_selected_range(point..point, cx));
    }

    pub fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.editor.read(cx).focus_handle(cx).clone()
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn format(&self) -> FileFormat {
        self.format
    }

    pub fn is_modified(&self) -> bool {
        self.modified
    }

    pub fn display_name(&self) -> String {
        self.path
            .as_deref()
            .and_then(Path::file_name)
            .map(|name| name.to_string_lossy().into_owned())
            .or_else(|| {
                self.path
                    .as_deref()
                    .map(|path| path.to_string_lossy().into_owned())
            })
            .unwrap_or_else(|| "*scratch*".to_owned())
    }

    pub fn text(&self, cx: &App) -> String {
        self.editor.read(cx).text().to_string()
    }

    pub fn len(&self, cx: &App) -> usize {
        self.editor.read(cx).text().len()
    }

    pub fn cursor(&self, cx: &App) -> usize {
        self.editor.read(cx).cursor()
    }

    pub fn scroll_offset(&self, cx: &App) -> Point<Pixels> {
        self.editor.read(cx).scroll_offset()
    }

    /// One-based line and character column of point.
    pub fn line_column(&self, cx: &App) -> (usize, usize) {
        let position = self.editor.read(cx).cursor_position();
        (position.line as usize + 1, position.character as usize + 1)
    }

    /// Cancel an uncommitted IME composition, if any (D19).
    ///
    /// Preedit text is temporary and must not change the modified state.
    pub fn cancel_composition(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let marked = self.editor.update(cx, |editor, cx| {
            EntityInputHandler::marked_text_range(editor, window, cx)
        });
        let Some(range) = marked else {
            return false;
        };
        self.editor.update(cx, |editor, cx| {
            EntityInputHandler::replace_text_in_range(editor, Some(range), "", window, cx)
        });
        cx.emit(BufferEvent::Changed);
        true
    }

    /// Finalize an uncommitted composition as text. M0 uses this for the native
    /// Escape action, which otherwise calls EditorState::unmark_text directly
    /// and leaves the retained preedit text invisible to the dirty model.
    pub fn finish_composition(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let marked = self.editor.update(cx, |editor, cx| {
            EntityInputHandler::marked_text_range(editor, window, cx)
        });
        if marked.is_none() {
            return false;
        }
        EntityInputHandler::unmark_text(self, window, cx);
        true
    }
}

impl EventEmitter<BufferEvent> for Buffer {}

impl EntityInputHandler for Buffer {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        self.editor.update(cx, |editor, cx| {
            EntityInputHandler::text_for_range(editor, range, adjusted_range, window, cx)
        })
    }

    fn selected_text_range(
        &mut self,
        ignore_disabled_input: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        self.editor.update(cx, |editor, cx| {
            EntityInputHandler::selected_text_range(editor, ignore_disabled_input, window, cx)
        })
    }

    fn marked_text_range(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.editor.update(cx, |editor, cx| {
            EntityInputHandler::marked_text_range(editor, window, cx)
        })
    }

    fn unmark_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let marked = self.editor.update(cx, |editor, cx| {
            EntityInputHandler::marked_text_range(editor, window, cx)
        });
        self.editor.update(cx, |editor, cx| {
            EntityInputHandler::unmark_text(editor, window, cx)
        });
        // EditorState keeps the preedit text when it is unmarked, so a
        // non-empty marked range became committed text. Unmark emits no Change
        // event; update dirty and repaint synchronously.
        if marked.as_ref().is_some_and(|range| !range.is_empty()) {
            self.modified = true;
        }
        if marked.is_some() {
            cx.emit(BufferEvent::Changed);
        }
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let marked = self.editor.update(cx, |editor, cx| {
            EntityInputHandler::marked_text_range(editor, window, cx)
        });
        let targets_marked = match (&range, &marked) {
            (None, Some(_)) => true,
            (Some(range), Some(marked)) => range == marked,
            _ => false,
        };
        self.editor.update(cx, |editor, cx| {
            EntityInputHandler::replace_text_in_range(editor, range, text, window, cx)
        });
        // A platform commit may speak about the marked range while a cancel
        // passes no range at all. Cancelling preedit removes temporary text and
        // must not dirty the buffer; the queued Change handler confirms the
        // final state against the clean text.
        if platform_replace_dirties(targets_marked, text) {
            self.modified = true;
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
        self.editor.update(cx, |editor, cx| {
            EntityInputHandler::replace_and_mark_text_in_range(
                editor,
                range,
                new_text,
                new_selected_range,
                window,
                cx,
            )
        });
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        element_bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        self.editor.update(cx, |editor, cx| {
            EntityInputHandler::bounds_for_range(editor, range_utf16, element_bounds, window, cx)
        })
    }

    fn selection_bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        element_bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Bounds<Pixels>> {
        self.editor.update(cx, |editor, cx| {
            EntityInputHandler::selection_bounds_for_range(
                editor,
                range_utf16,
                element_bounds,
                window,
                cx,
            )
        })
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<usize> {
        self.editor.update(cx, |editor, cx| {
            EntityInputHandler::character_index_for_point(editor, point, window, cx)
        })
    }

    fn set_selected_text_range(
        &mut self,
        range_utf16: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editor.update(cx, |editor, cx| {
            EntityInputHandler::set_selected_text_range(editor, range_utf16, window, cx)
        });
    }

    fn text_length_utf16(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<usize> {
        self.editor.update(cx, |editor, cx| {
            EntityInputHandler::text_length_utf16(editor, window, cx)
        })
    }

    fn accepts_text_input(&self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.editor.update(cx, |editor, cx| {
            EntityInputHandler::accepts_text_input(editor, window, cx)
        })
    }

    fn text_input_configuration(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> TextInputConfiguration {
        self.editor.update(cx, |editor, cx| {
            EntityInputHandler::text_input_configuration(editor, window, cx)
        })
    }

    fn text_input_editable_range(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.editor.update(cx, |editor, cx| {
            EntityInputHandler::text_input_editable_range(editor, window, cx)
        })
    }
}

fn platform_replace_dirties(targets_marked: bool, replacement: &str) -> bool {
    !(targets_marked && replacement.is_empty())
}

#[cfg(test)]
mod tests {
    use super::platform_replace_dirties;

    #[test]
    fn preedit_cancel_is_not_a_committed_edit() {
        assert!(!platform_replace_dirties(true, ""));
    }

    #[test]
    fn preedit_commit_and_ordinary_edits_are_committed() {
        assert!(platform_replace_dirties(true, "你好"));
        assert!(platform_replace_dirties(false, ""));
        assert!(platform_replace_dirties(false, "x"));
    }
}
