use super::*;
use crate::view::input_tests::{app_shell, handler, open, redraw};
use gpui_kit as gpui;
use gpui_kit::{InputHandler, LineLayout, TestAppContext, black, font, px, size};
use std::sync::Arc;

#[gpui::test]
fn cursor_covers_whole_graphemes_without_changing_scalar_motion(cx: &mut TestAppContext) {
    let text = "ae\u{301}中🙂👩\u{200d}💻z";
    let (mut visual, editor) = open(text, cx);
    let clusters: Vec<_> = text.grapheme_indices(true).collect();
    for (start, cluster) in clusters {
        for (inside, _) in cluster.char_indices() {
            let offset = ByteOffset(start + inside);
            editor.update(&mut visual, |view, cx| {
                view.target
                    .set_point_from_pointer(view.target.token(), offset)
                    .unwrap();
                cx.notify();
            });
            redraw(&mut visual);
            visual.read(|cx| {
                let view = editor.read(cx);
                let cursor = view.cursor().unwrap();
                assert_eq!(view.buffer().point(), offset);
                assert_eq!(
                    cursor.range,
                    ByteOffset(start)..ByteOffset(start + cluster.len())
                );
                let layout = view.viewport_layout().unwrap();
                let expected = layout
                    .bounds_for_range(cursor.range.clone(), view.wrap_index().unwrap())
                    .unwrap();
                assert_eq!(cursor.bounds, expected);
            });
        }
    }
    visual.simulate_keystrokes("alt-< ctrl-f ctrl-f");
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.buffer().point(), ByteOffset(2));
        assert_eq!(view.cursor().unwrap().range, ByteOffset(1)..ByteOffset(4));
        assert!(!view.buffer().is_modified());
    });
}

#[gpui::test]
fn tabs_line_ends_and_empty_rows_use_one_space_without_reflow(cx: &mut TestAppContext) {
    let text = "a\t中\n\nz";
    let (mut visual, editor) = open(text, cx);
    let cache = visual.read(|cx| editor.read(cx).geometry.as_ref().unwrap().rows.clone());
    for offset in [0, 1, 2, 5, 6, 7, 8] {
        editor.update(&mut visual, |view, cx| {
            view.target
                .set_point_from_pointer(view.target.token(), ByteOffset(offset))
                .unwrap();
            cx.notify();
        });
        redraw(&mut visual);
        visual.read(|cx| {
            let view = editor.read(cx);
            let cursor = view.cursor().unwrap();
            let layout = view.viewport_layout().unwrap();
            let row = layout
                .rows
                .iter()
                .find(|row| row.source.index == cursor.row)
                .unwrap();
            assert_eq!(cursor.bounds.size.width, row.space_width);
            let caret = layout
                .bounds_for_range(
                    ByteOffset(offset)..ByteOffset(offset),
                    view.wrap_index().unwrap(),
                )
                .unwrap();
            assert_eq!(cursor.bounds.origin, caret.origin);
            assert_eq!(caret.size.width, px(1.));
            if offset == 1 {
                let tab = row.parts.iter().find(|part| part.line.is_none()).unwrap();
                assert!(tab.width > cursor.bounds.size.width);
            }
            assert!(Arc::ptr_eq(row, &cache[cursor.row]));
            assert!(!view.buffer().is_modified());
        });
    }
}

#[gpui::test]
fn soft_wrap_boundary_uses_the_next_display_row(cx: &mut TestAppContext) {
    let (mut visual, editor) = open(&"中🙂abc ".repeat(100), cx);
    visual.simulate_resize(size(px(150.), px(240.)));
    redraw(&mut visual);
    let offset = visual.read(|cx| {
        editor
            .read(cx)
            .index
            .as_ref()
            .unwrap()
            .row(1)
            .unwrap()
            .start
    });
    editor.update(&mut visual, |view, cx| {
        view.target
            .set_point_from_pointer(view.target.token(), offset)
            .unwrap();
        cx.notify();
    });
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        let layout = view.viewport_layout().unwrap();
        let cursor = view.cursor().unwrap();
        assert_eq!(cursor.row, 1);
        assert_eq!(cursor.range.start, offset);
        assert_eq!(cursor.bounds.origin, layout.origin(&layout.rows[1]));
    });
}

#[gpui::test]
fn preedit_cursor_uses_projection_and_leaves_platform_geometry_unchanged(cx: &mut TestAppContext) {
    let (mut visual, editor) = open("original", cx);
    let before = visual.read(|cx| editor.read(cx).buffer().snapshot_id());
    let mut input = handler(&editor, &visual);
    visual.update(|window, cx| {
        input.replace_and_mark_text_in_range(None, "e\u{301}🙂", Some(1..1), window, cx)
    });
    redraw(&mut visual);
    let mut input = handler(&editor, &visual);
    visual.update(|window, cx| {
        let view = editor.read(cx);
        assert_eq!(view.buffer().snapshot_id(), before);
        assert_eq!(view.buffer().text().to_string(), "original");
        let snapshot = view.display_snapshot();
        assert_eq!(snapshot.marked_range(), Some(ByteOffset(0)..ByteOffset(7)));
        let cursor = view.cursor().unwrap();
        assert_eq!(cursor.range, ByteOffset(0)..ByteOffset(3));
        let cursor_width = cursor.bounds.size.width;
        let expected = view
            .viewport_layout()
            .unwrap()
            .bounds_for_range(ByteOffset(1)..ByteOffset(1), view.wrap_index().unwrap())
            .unwrap();
        let platform = input.bounds_for_range(1..1, window, cx).unwrap();
        assert_eq!(platform, expected);
        assert_eq!(platform.size.width, px(1.));
        assert!(cursor_width > platform.size.width);
    });
}

#[gpui::test]
fn changed_fonts_hide_old_cursor_and_platform_coordinates_until_layout_is_compatible(
    cx: &mut TestAppContext,
) {
    let (mut visual, editor) = open(&"abc中🙂\n".repeat(100), cx);
    let mut input = handler(&editor, &visual);
    visual.update(|window, cx| {
        editor.update(cx, |view, cx| {
            view.set_font(font("monospace"), px(20.), px(30.), cx);
        });
        let view = editor.read(cx);
        assert!(view.paint_layout().is_some());
        assert!(view.cursor().is_none());
        assert!(input.bounds_for_range(0..0, window, cx).is_none());
    });
    redraw(&mut visual);
    let mut input = handler(&editor, &visual);
    visual.update(|window, cx| {
        let cursor = editor.read(cx).cursor().unwrap();
        assert_eq!(cursor.bounds.size.height, px(30.));
        assert_eq!(
            input.bounds_for_range(0..0, window, cx).unwrap().size.width,
            px(1.)
        );
    });
}

#[gpui::test]
fn minibuffer_uses_the_same_cursor_geometry(cx: &mut TestAppContext) {
    let (mut visual, _) = open("body", cx);
    let shell = visual.update(|window, cx| app_shell(window, cx));
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.mount_minibuffer("P: ", "中🙂", window, cx)
        });
    });
    redraw(&mut visual);
    visual.read(|cx| {
        let editor = shell
            .read(cx)
            .minibuffer()
            .unwrap()
            .read(cx)
            .editor()
            .read(cx);
        let cursor = editor.cursor().unwrap();
        assert_eq!(cursor.range, ByteOffset(10)..ByteOffset(10));
        assert_eq!(
            cursor.bounds.size.width,
            editor.geometry.as_ref().unwrap().rows[0].space_width
        );
    });
}

#[gpui::test]
fn shaping_clusters_without_a_separate_glyph_are_not_split_by_cursor_stops(
    cx: &mut TestAppContext,
) {
    let mut line = cx.update(|cx| {
        gpui_kit::WindowTextSystem::new(cx.text_system().clone()).shape_line(
            "fi".into(),
            px(16.),
            &[gpui_kit::TextRun {
                len: 2,
                font: font("monospace"),
                color: black(),
                background_color: None,
                underline: None,
                strikethrough: None,
            }],
            None,
        )
    });
    let mut run = line.runs[0].clone();
    run.glyphs.truncate(1);
    *line = Arc::new(LineLayout {
        font_size: line.font_size,
        width: line.width,
        ascent: line.ascent,
        descent: line.descent,
        runs: vec![run],
        len: 2,
    });
    assert_eq!(cursor_stops(&line), vec![0, 2]);
}
