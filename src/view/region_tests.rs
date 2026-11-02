use gpui_kit as gpui;
use gpui_kit::{InputHandler, TestAppContext, font, point, px};

use super::input_tests::{handler, open, press_listener, redraw};
use crate::text::ByteOffset;

#[gpui::test]
fn active_region_paints_unicode_tabs_newlines_and_blank_lines_without_reflow(
    cx: &mut TestAppContext,
) {
    let (mut visual, editor) = open("a\t中🙂\n\nxyz", cx);
    let (id, cache) = visual.read(|cx| {
        let view = editor.read(cx);
        (
            view.display_snapshot().id(),
            view.cache.values().cloned().collect::<Vec<_>>(),
        )
    });
    visual.simulate_keystrokes("ctrl-f ctrl-t ctrl-n ctrl-n");
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.buffer().point(), ByteOffset(12));
        assert_eq!(view.buffer().region(), Some(ByteOffset(1)..ByteOffset(12)));
        let layout = view.viewport_layout().unwrap();
        let index = view.wrap_index().unwrap();
        let bounds = view.region_bounds();
        assert_eq!(bounds.len(), 3);
        let left = layout
            .bounds_for_range(ByteOffset(1)..ByteOffset(1), index)
            .unwrap()
            .left();
        let right = layout
            .bounds_for_range(ByteOffset(12)..ByteOffset(12), index)
            .unwrap()
            .left();
        assert_eq!(bounds[0].left(), left);
        assert_eq!(bounds[0].right(), layout.bounds.right());
        assert_eq!(bounds[1].left(), layout.bounds.left());
        assert_eq!(bounds[1].right(), layout.bounds.right());
        assert_eq!(bounds[2].right(), right);
        assert_eq!(view.display_snapshot().id(), id);
        assert!(!view.buffer().is_modified());
        for row in cache {
            assert!(
                view.cache
                    .values()
                    .any(|current| std::sync::Arc::ptr_eq(&row, current))
            );
        }
    });
    visual.simulate_keystrokes("ctrl-g");
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert!(view.region_bounds().is_empty());
        assert_eq!(view.buffer().mark(), Some(ByteOffset(1)));
    });
}

#[gpui::test]
fn reverse_regions_normalize_and_soft_wrap_geometry_is_clipped_to_the_viewport(
    cx: &mut TestAppContext,
) {
    let (mut visual, editor) = open(&"中🙂 ".repeat(300), cx);
    visual.simulate_keystrokes("alt-> ctrl-t alt-<");
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(
            view.buffer().region(),
            Some(ByteOffset(0)..ByteOffset(view.buffer().text().len()))
        );
        assert!(!view.region_bounds().is_empty());
        assert!(view.region_bounds().len() <= view.viewport_layout().unwrap().visible_row_count());
    });
    editor.update(&mut visual, |view, cx| {
        view.scroll_by(point(px(5.), px(12.)), cx)
    });
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        let layout = view.viewport_layout().unwrap();
        let bounds = view.region_bounds();
        assert!(!bounds.is_empty());
        assert_eq!(bounds[0].top(), layout.bounds.top());
        assert!(bounds[0].size.height < view.settings.line_height);
        for rect in bounds {
            assert!(rect.left() >= layout.bounds.left());
            assert!(rect.right() <= layout.bounds.right());
            assert!(rect.top() >= layout.bounds.top());
            assert!(rect.bottom() <= layout.bounds.bottom());
        }
    });
}

#[gpui::test]
fn region_uses_retained_paint_geometry_but_never_survives_an_incompatible_text_edit(
    cx: &mut TestAppContext,
) {
    let (mut visual, editor) = open("a\t中🙂\nxyz", cx);
    visual.simulate_keystrokes("ctrl-t ctrl-f ctrl-f ctrl-f");
    redraw(&mut visual);
    let before = visual.read(|cx| editor.read(cx).region_bounds());
    editor.update(&mut visual, |view, cx| {
        view.set_font(font("monospace"), px(20.), px(30.), cx);
        assert!(view.viewport_layout().is_none());
        assert_eq!(view.region_bounds(), before);
    });
    redraw(&mut visual);
    visual.simulate_input("X");
    visual.read(|cx| {
        let view = editor.read(cx);
        assert!(!view.buffer().mark_active());
        assert!(view.region_bounds().is_empty());
    });
    redraw(&mut visual);
    visual.read(|cx| assert!(editor.read(cx).region_bounds().is_empty()));
}

#[gpui::test]
fn region_projection_leaves_a_gap_for_uncommitted_preedit_and_restores_on_discard(
    cx: &mut TestAppContext,
) {
    let (mut visual, editor) = open("abcd", cx);
    visual.simulate_keystrokes("ctrl-t ctrl-e");
    redraw(&mut visual);
    let original = visual.read(|cx| editor.read(cx).region_bounds());
    let mut input = handler(&editor, &visual);
    visual.update(|window, cx| {
        input.replace_and_mark_text_in_range(Some(2..2), "拼", None, window, cx)
    });
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        let layout = view.viewport_layout().unwrap();
        let marked = layout
            .bounds_for_range(ByteOffset(2)..ByteOffset(5), view.wrap_index().unwrap())
            .unwrap();
        let bounds = view.region_bounds();
        assert_eq!(bounds.len(), 2);
        assert_eq!(bounds[0].right(), marked.left());
        assert_eq!(bounds[1].left(), marked.right());
        assert_eq!(view.buffer().mark(), Some(ByteOffset(0)));
        assert_eq!(view.buffer().point(), ByteOffset(4));
        assert!(view.buffer().mark_active());
        assert!(!view.buffer().is_modified());
    });
    editor.update(&mut visual, |view, cx| view.end_input(cx));
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.region_bounds(), original);
        assert_eq!(view.buffer().text().to_string(), "abcd");
        assert!(view.buffer().mark_active());
    });
}

#[gpui::test]
fn formal_copy_kill_and_yank_chain_preserves_ring_text_and_mark_semantics(cx: &mut TestAppContext) {
    let (mut visual, editor) = open("ab\ncd", cx);
    visual.simulate_keystrokes("ctrl-t ctrl-f ctrl-f alt-w");
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.target.kill_ring().latest().unwrap().to_string(), "ab");
        assert!(view.region_bounds().is_empty());
        assert!(!view.buffer().is_modified());
    });
    visual.simulate_keystrokes("ctrl-k ctrl-k ctrl-y");
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.buffer().text().to_string(), "ab\ncd");
        assert_eq!(view.buffer().point(), ByteOffset(5));
        assert_eq!(view.buffer().mark(), Some(ByteOffset(2)));
        assert!(!view.buffer().mark_active());
        assert_eq!(view.message(), Some("Mark set"));
        assert_eq!(
            view.target
                .kill_ring()
                .entries()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            ["\ncd", "ab"]
        );
    });
    visual.simulate_keystrokes("ctrl-y");
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.buffer().text().to_string(), "ab\ncd\ncd");
        assert_eq!(view.buffer().mark(), Some(ByteOffset(5)));
        assert_eq!(view.target.kill_ring().entries().len(), 2);
    });
}

#[gpui::test]
fn escape_copy_can_extend_a_kill_but_the_following_copy_starts_a_new_entry(
    cx: &mut TestAppContext,
) {
    let (mut visual, editor) = open("abc\ndef", cx);
    visual.simulate_keystrokes("ctrl-t ctrl-f ctrl-f ctrl-k escape w alt-w");
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.buffer().text().to_string(), "ab\ndef");
        assert_eq!(view.buffer().point(), ByteOffset(2));
        assert_eq!(view.buffer().mark(), Some(ByteOffset(0)));
        assert!(!view.buffer().mark_active());
        assert_eq!(
            view.target
                .kill_ring()
                .entries()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            ["ab", "cab"]
        );
    });
}

#[gpui::test]
fn readonly_kill_copies_and_clears_region_while_kill_line_moves_the_caret(cx: &mut TestAppContext) {
    let (mut visual, editor) = open("abc\ndef", cx);
    visual.simulate_keystrokes("ctrl-t ctrl-f ctrl-f");
    redraw(&mut visual);
    editor.update(&mut visual, |view, _| view.target.set_read_only(true));
    visual.simulate_keystrokes("ctrl-w");
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.message(), Some("Buffer is read-only"));
        assert_eq!(view.target.kill_ring().latest().unwrap().to_string(), "ab");
        assert_eq!(view.buffer().point(), ByteOffset(2));
        assert!(view.region_bounds().is_empty());
        assert!(!view.buffer().is_modified());
    });
    visual.simulate_keystrokes("ctrl-a ctrl-k");
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.message(), Some("Buffer is read-only"));
        assert_eq!(view.buffer().point(), ByteOffset(3));
        assert_eq!(view.target.kill_ring().latest().unwrap().to_string(), "abc");
        let layout = view.viewport_layout().unwrap();
        assert!(
            layout
                .bounds_for_range(ByteOffset(3)..ByteOffset(3), view.wrap_index().unwrap())
                .is_some()
        );
        assert_eq!(view.buffer().text().to_string(), "abc\ndef");
    });
    visual.simulate_keystrokes("ctrl-k");
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.buffer().point(), ByteOffset(4));
        assert_eq!(
            view.target.kill_ring().latest().unwrap().to_string(),
            "abc\n"
        );
        assert_eq!(view.message(), Some("Buffer is read-only"));
        assert!(!view.buffer().is_modified());
    });
}

#[gpui::test]
fn kill_commands_preserve_composition_registration_but_revoke_callbacks_after_committing(
    cx: &mut TestAppContext,
) {
    let (mut visual, editor) = open("abc", cx);
    let mut input = handler(&editor, &visual);
    visual.update(|window, cx| {
        input.replace_and_mark_text_in_range(None, "拼", None, window, cx);
        for key in ["ctrl-k", "ctrl-w", "alt-w", "ctrl-y"] {
            press_listener(&editor, key, window, cx);
            assert_eq!(
                editor.read(cx).message(),
                Some("Input method composition is active; finish it first")
            );
            assert!(editor.read(cx).target.kill_ring().latest().is_none());
        }
        input.replace_text_in_range(None, "汉", window, cx);
        assert_eq!(editor.read(cx).buffer().text().to_string(), "汉abc");
        press_listener(&editor, "ctrl-k", window, cx);
        input.replace_text_in_range(None, "late", window, cx);
        assert_eq!(editor.read(cx).buffer().text().to_string(), "汉");
        assert_eq!(
            editor
                .read(cx)
                .target
                .kill_ring()
                .latest()
                .unwrap()
                .to_string(),
            "abc"
        );
    });
    redraw(&mut visual);
    visual.simulate_keystrokes("ctrl-x ctrl-k");
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.message(), Some("C-x C-k is undefined"));
        assert_eq!(view.target.kill_ring().entries().len(), 1);
    });
}

#[gpui::test]
fn application_ring_is_shared_between_windows_and_survives_repeated_initialization(
    cx: &mut TestAppContext,
) {
    let (mut first_window, first) = open("中🙂", cx);
    first_window.simulate_keystrokes("ctrl-k");
    redraw(&mut first_window);
    let (mut second_window, second) = open("tail", cx);
    second_window.simulate_keystrokes("ctrl-y");
    redraw(&mut second_window);
    second_window.read(|cx| {
        let view = second.read(cx);
        assert_eq!(view.buffer().text().to_string(), "中🙂tail");
        assert_eq!(view.buffer().point(), ByteOffset(7));
        assert_eq!(view.buffer().mark(), Some(ByteOffset(0)));
        assert_eq!(first.read(cx).buffer().text().to_string(), "");
        assert_eq!(
            first
                .read(cx)
                .target
                .kill_ring()
                .latest()
                .unwrap()
                .to_string(),
            "中🙂"
        );
    });
}
