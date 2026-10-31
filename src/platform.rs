//! GPUI callbacks bound to the target, input lifetime, and painted registration.

use std::ops::Range;

use gpui_kit::{
    App, Bounds, ClipboardItem, InputHandler, Pixels, Point, UTF16Selection, WeakEntity, Window,
};

use crate::input::{DisplaySnapshot, SessionToken};
use crate::text::{OffsetUtf16, clip_utf16_range, range_to_utf16};
use crate::view::BufferView;

pub(crate) enum InputOperation<'a> {
    Replace(Option<Range<OffsetUtf16>>, &'a str),
    Mark(
        Option<Range<OffsetUtf16>>,
        &'a str,
        Option<Range<OffsetUtf16>>,
    ),
    Unmark,
}

pub(crate) struct BufferInputHandler {
    pub view: WeakEntity<BufferView>,
    pub token: SessionToken,
    pub registration: u64,
}

impl BufferInputHandler {
    fn snapshot(&self, window: &Window, cx: &App) -> Option<DisplaySnapshot> {
        let view = self.view.upgrade()?;
        view.read(cx)
            .platform_snapshot(self.registration, self.token, window)
    }

    fn apply(&mut self, operation: InputOperation<'_>, window: &mut Window, cx: &mut App) {
        let Ok(next) = self.view.update(cx, |view, cx| {
            view.platform_update(self.registration, self.token, operation, window, cx)
        }) else {
            return;
        };
        if let Some(token) = next {
            // Only this callback's successful mutation may advance its token.
            self.token = token;
        }
    }
}

fn utf16(range: Range<usize>) -> Range<OffsetUtf16> {
    OffsetUtf16(range.start)..OffsetUtf16(range.end)
}

fn units(range: Range<OffsetUtf16>) -> Range<usize> {
    range.start.0..range.end.0
}

impl InputHandler for BufferInputHandler {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<String> {
        *adjusted = None;
        let (text, range) = self.snapshot(window, cx)?.text_for_range(utf16(range))?;
        *adjusted = Some(units(range));
        Some(text)
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: units(self.snapshot(window, cx)?.selected_range_utf16()),
            reversed: false,
        })
    }

    fn marked_text_range(&mut self, window: &mut Window, cx: &mut App) -> Option<Range<usize>> {
        self.snapshot(window, cx)?.marked_range_utf16().map(units)
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.apply(InputOperation::Replace(range.map(utf16), text), window, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.apply(
            InputOperation::Mark(range.map(utf16), text, selected.map(utf16)),
            window,
            cx,
        );
    }

    fn unmark_text(&mut self, window: &mut Window, cx: &mut App) {
        self.apply(InputOperation::Unmark, window, cx);
    }

    fn paste(&mut self, _: ClipboardItem, _: &mut Window, _: &mut App) {
        // Clipboard insertion belongs to yank, not an alternate CUA input path.
    }

    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Bounds<Pixels>> {
        let snapshot = self.snapshot(window, cx)?;
        let range = clip_utf16_range(snapshot.text(), utf16(range))?;
        let view = self.view.upgrade()?;
        let view = view.read(cx);
        view.viewport_layout()?
            .bounds_for_range(range, view.wrap_index()?)
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<usize> {
        let snapshot = self.snapshot(window, cx)?;
        let view = self.view.upgrade()?;
        let offset = view.read(cx).viewport_layout()?.hit_test(point)?;
        Some(range_to_utf16(snapshot.text(), offset..offset)?.start.0)
    }

    fn element_bounds(&mut self, window: &mut Window, cx: &mut App) -> Option<Bounds<Pixels>> {
        self.snapshot(window, cx)?;
        Some(self.view.upgrade()?.read(cx).viewport_layout()?.bounds())
    }

    fn text_length_utf16(&mut self, window: &mut Window, cx: &mut App) -> Option<usize> {
        let snapshot = self.snapshot(window, cx)?;
        Some(
            snapshot
                .text()
                .offset_to_offset_utf16(snapshot.text().len())
                .0,
        )
    }

    fn accepts_text_input(&mut self, window: &mut Window, cx: &mut App) -> bool {
        self.snapshot(window, cx).is_some()
    }

    fn prefers_ime_for_printable_keys(&mut self, window: &mut Window, cx: &mut App) -> bool {
        self.accepts_text_input(window, cx)
    }
}
