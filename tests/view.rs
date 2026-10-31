use gpui_kit as gpui;
use gpui_kit::{Entity, Focusable, TestAppContext, VisualTestContext, font, point, px, size};

use semacs::app::AppShell;
use semacs::buffer::Buffer;
use semacs::files::{DecodedFile, FileFormat};
use semacs::text::ByteOffset;
use semacs::view::BufferView;

fn buffer(text: &str) -> Buffer {
    Buffer::new(
        None,
        DecodedFile {
            text: text.into(),
            format: FileFormat::default(),
        },
    )
}

fn open(text: &str, cx: &mut TestAppContext) -> (VisualTestContext, Entity<BufferView>) {
    cx.update(semacs::app::init);
    let buffer = buffer(text);
    let window = cx.open_window(size(px(400.), px(240.)), |window, cx| {
        AppShell::new(buffer, window, cx)
    });
    let editor = window
        .read_with(cx, |shell, _| shell.editor().clone())
        .unwrap();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.run_until_parked();
    visual.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    (visual, editor)
}

fn redraw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
}

#[gpui::test]
fn body_is_drawn_and_click_sets_scalar_point_from_actual_geometry(cx: &mut TestAppContext) {
    let (mut visual, editor) = open("中🙂e\u{301}\txyz\n\nEOF", cx);
    let (text_bounds, caret) = visual.read(|cx| {
        let view = editor.read(cx);
        assert!(view.layout_ready());
        assert!(view.viewport_layout().unwrap().visible_row_count() >= 3);
        assert_eq!(view.buffer().text().to_string(), "中🙂e\u{301}\txyz\n\nEOF");
        let layout = view.viewport_layout().unwrap();
        let rect = layout
            .bounds_for_range(ByteOffset(3)..ByteOffset(3), view.wrap_index().unwrap())
            .unwrap();
        assert!(layout.bounds().left() > px(0.) && layout.bounds().top() > px(0.));
        (layout.bounds(), rect)
    });
    visual.simulate_click(
        point(caret.left(), caret.top() + px(8.)),
        Default::default(),
    );
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.buffer().point(), ByteOffset(3));
        assert_eq!(view.buffer().line_column(), (1, 1));
        assert!(!view.buffer().is_modified());
        assert_eq!(view.viewport_layout().unwrap().bounds(), text_bounds);
    });
}

#[gpui::test]
fn geometry_roundtrips_tabs_eof_and_nonzero_origin(cx: &mut TestAppContext) {
    let (visual, editor) = open("a\t中🙂\n\nEOF\n", cx);
    visual.read(|cx| {
        let view = editor.read(cx);
        let layout = view.viewport_layout().unwrap();
        let index = view.wrap_index().unwrap();
        let text = view.buffer().text().to_string();
        for byte in text.char_indices().map(|(i, _)| i).chain([text.len()]) {
            let rect = layout
                .bounds_for_range(ByteOffset(byte)..ByteOffset(byte), index)
                .unwrap();
            let hit = layout
                .hit_test(point(rect.left(), rect.top() + px(5.)))
                .unwrap();
            assert_eq!(hit, ByteOffset(byte), "roundtrip at {byte}");
        }
        assert!(
            layout
                .hit_test(point(
                    layout.bounds().left() - px(1.),
                    layout.bounds().top()
                ))
                .is_none()
        );
    });
}

#[gpui::test]
fn hit_testing_uses_both_halves_of_the_last_multibyte_character(cx: &mut TestAppContext) {
    let (visual, editor) = open("中\n🙂", cx);
    visual.read(|cx| {
        let view = editor.read(cx);
        let layout = view.viewport_layout().unwrap();
        let index = view.wrap_index().unwrap();
        for (start, end) in [(0, 3), (4, 8)] {
            let rect = layout
                .bounds_for_range(ByteOffset(start)..ByteOffset(end), index)
                .unwrap();
            assert_eq!(
                layout.hit_test(point(
                    rect.left() + rect.size.width * 0.25,
                    rect.top() + px(8.)
                )),
                Some(ByteOffset(start))
            );
            assert_eq!(
                layout.hit_test(point(
                    rect.left() + rect.size.width * 0.75,
                    rect.top() + px(8.)
                )),
                Some(ByteOffset(end))
            );
        }
    });
}

#[gpui::test]
fn scrolling_reuses_content_identity_but_replaces_viewport_geometry(cx: &mut TestAppContext) {
    let (mut visual, editor) = open(&"abcdef中🙂\t\n".repeat(300), cx);
    redraw(&mut visual);
    let before = visual.read(|cx| editor.read(cx).viewport_layout().unwrap().key().clone());
    let bounds = visual.read(|cx| editor.read(cx).viewport_layout().unwrap().bounds());
    visual.simulate_event(gpui::ScrollEvent {
        position: point(bounds.left() + px(10.), bounds.top() + px(10.)),
        delta: gpui::ScrollDelta::Pixels(point(px(0.), px(-96.))),
        modifiers: Default::default(),
        touch_phase: gpui::TouchPhase::Moved,
        source: gpui::ScrollSource::Pointer,
    });
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(
            editor.read(cx).viewport_layout().unwrap().scroll().y,
            px(96.)
        )
    });
    visual.update(|_, cx| {
        editor.update(cx, |view, cx| {
            view.scroll_by(point(px(14.), px(144.)), cx);
            assert!(view.viewport_layout().is_none());
        })
    });
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        let layout = view.viewport_layout().unwrap();
        assert_eq!(*layout.key(), before);
        assert_eq!(layout.scroll(), point(px(14.), px(240.)));
        let hit = layout
            .hit_test(point(
                layout.bounds().left(),
                layout.bounds().top() + px(8.),
            ))
            .unwrap();
        assert!(hit.0 > 0);
        let rect = layout
            .bounds_for_range(hit..hit, view.wrap_index().unwrap())
            .unwrap();
        assert_eq!(rect.top(), layout.bounds().top());
        assert!(layout.visible_row_count() < 20);
    });
}

#[gpui::test]
fn resizing_and_font_changes_discard_incompatible_layouts_and_stale_jobs(cx: &mut TestAppContext) {
    let (mut visual, editor) = open(&"中🙂abcdefghijklmno\n".repeat(3000), cx);
    let first = visual.read(|cx| editor.read(cx).wrap_index().unwrap().key().clone());
    visual.simulate_resize(size(px(250.), px(200.)));
    visual.update(|_, cx| {
        editor.update(cx, |view, cx| {
            view.set_font(font("monospace"), px(20.), px(30.), cx);
            assert!(view.viewport_layout().is_none());
        })
    });
    visual.simulate_resize(size(px(500.), px(260.)));
    redraw(&mut visual);
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        let index = view.wrap_index().unwrap();
        let layout = view.viewport_layout().unwrap();
        assert!(index.is_complete());
        assert_eq!(index.text().len(), view.buffer().text().len());
        assert_ne!(*index.key(), first);
        assert_eq!(index.key(), layout.key());
        assert_eq!(index.key().settings.font_size, px(20.));
        assert_eq!(index.key().settings.line_height, px(30.));
        assert!(index.key().settings.wrap_width > first.settings.wrap_width);
    });
}

#[gpui::test]
fn scratch_has_real_empty_line_and_large_single_line_is_not_truncated(cx: &mut TestAppContext) {
    let (visual, editor) = open("", cx);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.wrap_index().unwrap().row_count(), 1);
        assert_eq!(view.viewport_layout().unwrap().visible_row_count(), 1);
        assert!(
            view.viewport_layout()
                .unwrap()
                .bounds_for_range(ByteOffset(0)..ByteOffset(0), view.wrap_index().unwrap())
                .is_some()
        );
    });
    let (mut visual, editor) = open(&"x".repeat(1024 * 1024), cx);
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        let index = view.wrap_index().unwrap();
        assert!(index.is_complete());
        assert_eq!(index.indexed_bytes(), 1024 * 1024);
        assert!(index.row_count() > 10_000);
        assert!(view.viewport_layout().unwrap().visible_row_count() < 20);
    });
    visual.update(|window, cx| {
        window.blur(cx);
        editor.update(cx, |_, cx| cx.notify());
    });
    redraw(&mut visual);
    visual.update(|window, cx| {
        assert!(!editor.focus_handle(cx).is_focused(window));
    });
}
