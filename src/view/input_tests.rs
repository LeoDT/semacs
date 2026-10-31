use super::BufferView;
use crate::app::AppShell;
use crate::buffer::Buffer;
use crate::display::{FontMeasure, WrapJob};
use crate::files::{DecodedFile, FileFormat};
use crate::platform::BufferInputHandler;
use crate::text::{ByteOffset, range_to_utf16};
use gpui_kit as gpui;
use gpui_kit::{
    Entity, Focusable, InputHandler, TestAppContext, VisualTestContext, font, point, px, size,
};
use std::sync::atomic::AtomicBool;

fn open(text: &str, cx: &mut TestAppContext) -> (VisualTestContext, Entity<BufferView>) {
    cx.update(crate::app::init);
    let buffer = Buffer::new(
        None,
        DecodedFile {
            text: text.into(),
            format: FileFormat::default(),
        },
    );
    let window = cx.open_window(size(px(400.), px(240.)), |window, cx| {
        AppShell::new(buffer, window, cx)
    });
    let editor = window
        .read_with(cx, |shell, _| shell.editor().clone())
        .unwrap();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.update(|window, _| window.activate_window());
    redraw(&mut visual);
    (visual, editor)
}

fn redraw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn handler(editor: &Entity<BufferView>, cx: &VisualTestContext) -> BufferInputHandler {
    cx.read(|cx| {
        let view = editor.read(cx);
        BufferInputHandler {
            view: editor.downgrade(),
            token: view.input.token(),
            registration: view.registration,
        }
    })
}

#[gpui::test]
fn registered_handler_receives_typing_newline_and_scalar_deletion(cx: &mut TestAppContext) {
    let (mut visual, editor) = open("", cx);
    visual.simulate_input("中🙂e\u{301}");
    redraw(&mut visual);
    visual.read(|cx| assert_eq!(editor.read(cx).buffer.text().to_string(), "中🙂e\u{301}"));
    visual.simulate_keystrokes("backspace enter");
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.buffer.text().to_string(), "中🙂e\n");
        assert!(view.buffer.is_modified());
        assert!(view.layout_ready());
    });
    visual.simulate_keystrokes("ctrl-v ctrl-c ctrl-x");
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.buffer.text().to_string(), "中🙂e\n");
        assert!(view.message().unwrap().contains("undefined"));
    });
}

#[gpui::test]
fn same_frame_callbacks_advance_only_their_own_token_and_geometry_waits_for_layout(
    cx: &mut TestAppContext,
) {
    let (mut visual, editor) = open("tail", cx);
    let mut input = handler(&editor, &visual);
    visual.update(|window, cx| {
        input.replace_and_mark_text_in_range(None, "汉🙂", Some(1..1), window, cx);
        assert_eq!(
            input.selected_text_range(false, window, cx).unwrap().range,
            1..1
        );
        assert_eq!(input.marked_text_range(window, cx), Some(0..3));
        assert!(input.bounds_for_range(0..0, window, cx).is_none());
        let mut adjusted = None;
        assert_eq!(
            input.text_for_range(2..3, &mut adjusted, window, cx),
            Some("🙂".into())
        );
        assert_eq!(adjusted, Some(1..3));
        input.replace_and_mark_text_in_range(None, "字🙂", None, window, cx);
        input.replace_text_in_range(None, "终", window, cx);
        input.replace_text_in_range(None, "!", window, cx);
        assert_eq!(
            input.selected_text_range(false, window, cx).unwrap().range,
            2..2
        );
        assert!(input.bounds_for_range(2..2, window, cx).is_none());
    });
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.buffer.text().to_string(), "终!tail");
        assert_eq!(view.buffer.point(), ByteOffset(4));
        assert!(!view.input.is_composing());
        assert!(view.layout_ready());
    });
    visual.update(|window, cx| {
        // A new paint supersedes the old callback registration.
        input.replace_text_in_range(None, "late", window, cx);
        assert!(input.selected_text_range(false, window, cx).is_none());
    });
    let mut current = handler(&editor, &visual);
    visual.update(|window, cx| {
        assert!(current.bounds_for_range(2..2, window, cx).is_some());
        current.replace_and_mark_text_in_range(None, "保留", None, window, cx);
        current.unmark_text(window, cx);
        current.replace_text_in_range(None, "后", window, cx);
    });
    redraw(&mut visual);
    visual.read(|cx| assert_eq!(editor.read(cx).buffer.text().to_string(), "终!保留后tail"));
}

#[gpui::test]
fn platform_geometry_roundtrips_utf16_and_same_length_updates_invalidate_it(
    cx: &mut TestAppContext,
) {
    let (mut visual, editor) = open("a\t中🙂\n\ne\u{301}\n", cx);
    let mut input = handler(&editor, &visual);
    visual.update(|window, cx| {
        let text = editor.read(cx).display_snapshot().text().to_string();
        for byte in text.char_indices().map(|(i, _)| i).chain([text.len()]) {
            let unit = text[..byte].encode_utf16().count();
            let rect = input.bounds_for_range(unit..unit, window, cx).unwrap();
            assert_eq!(
                input.character_index_for_point(
                    point(rect.left(), rect.top() + px(4.)),
                    window,
                    cx
                ),
                Some(unit)
            );
        }
        input.replace_text_in_range(Some(2..3), "文", window, cx);
        assert!(input.bounds_for_range(2..2, window, cx).is_none());
        assert!(
            input
                .character_index_for_point(point(px(8.), px(8.)), window, cx)
                .is_none()
        );
    });
    redraw(&mut visual);
    let mut input = handler(&editor, &visual);
    visual.update(|window, cx| assert!(input.bounds_for_range(2..2, window, cx).is_some()));
    let count = visual.read(|cx| editor.read(cx).geometry_refreshes);
    for _ in 0..3 {
        visual.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
    }
    visual.read(|cx| assert_eq!(editor.read(cx).geometry_refreshes, count));
}

#[gpui::test]
fn composition_barrier_blocks_commands_but_not_platform_composition(cx: &mut TestAppContext) {
    let (mut visual, editor) = open("body", cx);
    let mut input = handler(&editor, &visual);
    visual
        .update(|window, cx| input.replace_and_mark_text_in_range(None, "拼音", None, window, cx));
    redraw(&mut visual);
    visual.simulate_keystrokes("ctrl-x backspace enter");
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.buffer.text().to_string(), "body");
        assert_eq!(view.display_snapshot().text().to_string(), "拼音body");
        assert_eq!(
            view.message(),
            Some("Input method composition is active; finish it first")
        );
        assert!(!view.buffer.is_modified());
    });
    let mut input = handler(&editor, &visual);
    visual.update(|window, cx| {
        input.replace_and_mark_text_in_range(None, "", None, window, cx);
        assert_eq!(input.marked_text_range(window, cx), None);
        input.replace_text_in_range(None, "完", window, cx);
    });
    redraw(&mut visual);
    visual.read(|cx| assert_eq!(editor.read(cx).buffer.text().to_string(), "完body"));
}

#[gpui::test]
fn blur_cancels_marked_text_and_reopened_focus_rejects_old_handlers(cx: &mut TestAppContext) {
    let (mut visual, editor) = open("body", cx);
    let mut old = handler(&editor, &visual);
    visual.update(|window, cx| {
        old.replace_and_mark_text_in_range(None, "未提交", None, window, cx);
        window.blur(cx);
        old.replace_text_in_range(None, "late", window, cx);
    });
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.display_snapshot().text().to_string(), "body");
        assert!(!view.buffer.is_modified());
        assert!(view.viewport_layout().is_some());
    });
    visual.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    redraw(&mut visual);
    visual.update(|window, cx| old.replace_text_in_range(None, "late", window, cx));
    visual.simulate_input("new");
    redraw(&mut visual);
    visual.read(|cx| assert_eq!(editor.read(cx).buffer.text().to_string(), "newbody"));
}

#[gpui::test]
fn window_deactivation_ends_session_even_when_view_focus_handle_is_unchanged(
    cx: &mut TestAppContext,
) {
    let (mut visual, editor) = open("body", cx);
    let mut old = handler(&editor, &visual);
    visual.update(|window, cx| old.replace_and_mark_text_in_range(None, "临时", None, window, cx));
    visual.deactivate_window();
    visual.update(|window, cx| {
        old.replace_text_in_range(None, "late", window, cx);
        assert!(old.selected_text_range(false, window, cx).is_none());
    });
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(
            editor.read(cx).display_snapshot().text().to_string(),
            "body"
        );
        assert!(!editor.read(cx).buffer.is_modified());
    });
    visual.update(|window, _| window.activate_window());
    redraw(&mut visual);
    visual.update(|window, cx| old.replace_text_in_range(None, "late", window, cx));
    visual.simulate_input("new");
    redraw(&mut visual);
    visual.read(|cx| assert_eq!(editor.read(cx).buffer.text().to_string(), "newbody"));
}

#[gpui::test]
fn scrolling_replaces_platform_geometry_and_bounds_match_the_drawn_region(cx: &mut TestAppContext) {
    let (mut visual, editor) = open(&"a\t中🙂\n".repeat(100), cx);
    visual.update(|_, cx| {
        editor.update(cx, |view, cx| {
            view.scroll_by(point(px(12.), px(96.)), cx);
        })
    });
    redraw(&mut visual);
    let mut input = handler(&editor, &visual);
    visual.update(|window, cx| {
        let layout = editor.read(cx).viewport_layout().unwrap();
        let bounds = layout.bounds();
        assert!(bounds.left() > px(0.) && bounds.top() > px(0.));
        let position = point(bounds.left(), bounds.top() + px(4.));
        let offset = input
            .character_index_for_point(position, window, cx)
            .unwrap();
        assert!(offset > 0);
        let rect = input.bounds_for_range(offset..offset, window, cx).unwrap();
        assert_eq!(rect.top(), bounds.top());
        assert_eq!(input.element_bounds(window, cx), Some(bounds));
        assert!(
            input
                .character_index_for_point(point(bounds.left() - px(1.), bounds.top()), window, cx)
                .is_none()
        );
    });
}

#[gpui::test]
fn malformed_platform_callbacks_leave_projection_and_committed_versions_unchanged(
    cx: &mut TestAppContext,
) {
    let (mut visual, editor) = open("🙂body", cx);
    let mut input = handler(&editor, &visual);
    visual.update(|window, cx| {
        input.replace_and_mark_text_in_range(None, "汉", None, window, cx);
        let before = editor.read(cx).display_snapshot();
        let committed = editor.read(cx).buffer.snapshot_id();
        let reversed = std::ops::Range { start: 3, end: 1 };
        input.replace_and_mark_text_in_range(Some(0..2), "bad", Some(reversed.clone()), window, cx);
        input.replace_text_in_range(Some(reversed.clone()), "bad", window, cx);
        assert_eq!(editor.read(cx).display_snapshot().id(), before.id());
        assert_eq!(editor.read(cx).buffer.snapshot_id(), committed);
        let mut adjusted = Some(0..0);
        assert!(
            input
                .text_for_range(reversed, &mut adjusted, window, cx)
                .is_none()
        );
        assert_eq!(adjusted, None);
    });
}

#[gpui::test]
fn dirty_close_is_confirmed_cancel_restores_focus_and_discard_closes(cx: &mut TestAppContext) {
    let (mut visual, editor) = open("a🙂中z", cx);
    let mut input = handler(&editor, &visual);
    visual.update(|window, cx| {
        input.replace_and_mark_text_in_range(Some(1..4), "ni", None, window, cx);
        assert_eq!(editor.read(cx).buffer.text().to_string(), "az");
        let marked = input.marked_text_range(window, cx).unwrap();
        input.replace_text_in_range(Some(marked), "", window, cx);
    });
    redraw(&mut visual);
    let mut old = handler(&editor, &visual);
    assert!(!visual.simulate_close());
    assert!(visual.update(|window, _| window.has_active_prompt()));
    assert!(!visual.simulate_close());
    visual.update(|window, cx| old.replace_text_in_range(None, "late", window, cx));
    redraw(&mut visual);
    visual.simulate_keystrokes("escape");
    redraw(&mut visual);
    visual.update(|window, cx| {
        assert!(editor.focus_handle(cx).is_focused(window));
        old.replace_text_in_range(None, "late", window, cx);
    });
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.buffer.text().to_string(), "az");
        assert!(view.buffer.is_modified());
    });
    visual.simulate_input("x");
    redraw(&mut visual);
    assert!(!visual.simulate_close());
    redraw(&mut visual);
    visual.simulate_keystrokes("tab enter");
    visual.run_until_parked();
    assert!(visual.windows().is_empty());
}

#[gpui::test]
fn closing_only_a_preedit_is_clean_but_preserves_prior_commits(cx: &mut TestAppContext) {
    let (mut visual, editor) = open("body", cx);
    let mut input = handler(&editor, &visual);
    visual
        .update(|window, cx| input.replace_and_mark_text_in_range(None, "临时", None, window, cx));
    assert!(visual.simulate_close());
    assert!(!visual.update(|window, _| window.has_active_prompt()));
    visual.read(|cx| {
        assert_eq!(
            editor.read(cx).display_snapshot().text().to_string(),
            "body"
        );
        assert!(!editor.read(cx).buffer.is_modified());
    });

    let (mut visual, editor) = open("body", cx);
    let mut input = handler(&editor, &visual);
    visual.update(|window, cx| {
        input.replace_text_in_range(None, "提交", window, cx);
        input.replace_and_mark_text_in_range(None, "临时", None, window, cx);
    });
    assert!(!visual.simulate_close());
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.display_snapshot().text().to_string(), "提交body");
        assert!(view.buffer.is_modified());
    });
    redraw(&mut visual);
    visual.simulate_keystrokes("escape");
    redraw(&mut visual);
}

#[gpui::test]
fn close_confirmation_defaults_to_cancel_and_accepts_keyboard_quit(cx: &mut TestAppContext) {
    let (mut visual, editor) = open("", cx);
    visual.simulate_input("dirty");
    for key in ["enter", "ctrl-g", "escape"] {
        assert!(!visual.simulate_close());
        redraw(&mut visual);
        assert!(visual.update(|window, _| window.has_active_prompt()));
        visual.simulate_keystrokes(key);
        redraw(&mut visual);
        visual.update(|window, cx| {
            assert!(!window.has_active_prompt());
            assert!(editor.focus_handle(cx).is_focused(window));
        });
        visual.simulate_input("x");
    }
    visual.read(|cx| {
        assert_eq!(editor.read(cx).buffer.text().to_string(), "dirtyxxx");
        assert!(editor.read(cx).buffer.is_modified());
    });
}

#[gpui::test]
fn actual_input_deltas_reuse_wrap_index_across_multiple_callbacks(cx: &mut TestAppContext) {
    let (mut visual, editor) = open(&"abcdefghijk\t中🙂\n".repeat(10_000), cx);
    let mut input = handler(&editor, &visual);
    visual.update(|window, cx| {
        input.replace_text_in_range(None, "头\n", window, cx);
        input.replace_and_mark_text_in_range(None, "候选🙂", None, window, cx);
        input.replace_and_mark_text_in_range(None, "更新🙂", None, window, cx);
        input.replace_text_in_range(None, "完成", window, cx);
    });
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        let index = view.wrap_index().unwrap();
        assert!(view.layout_ready());
        assert!(
            index.scanned_bytes() < 8192,
            "must not silently rebuild the entire document"
        );
        let mut metrics = FontMeasure::new(cx.text_system().clone(), view.settings.clone());
        let full = WrapJob::new(view.display_snapshot(), view.settings.clone(), None, None)
            .finish(&mut metrics, &AtomicBool::new(false))
            .unwrap();
        assert_eq!(index.text().to_string(), full.text().to_string());
        let rows = |index: &crate::display::WrapIndex| {
            index
                .visible_rows(0..index.row_count())
                .into_iter()
                .map(|row| (row.start, row.row))
                .collect::<Vec<_>>()
        };
        assert_eq!(rows(index), rows(&full));
        let point = view.buffer.point();
        assert!(range_to_utf16(view.buffer.text(), point..point).is_some());
    });
}

#[gpui::test]
fn preview_and_final_geometry_refresh_once_each_and_not_on_stable_frames(cx: &mut TestAppContext) {
    let (mut visual, editor) = open(&"abcdef\n".repeat(10_000), cx);
    visual.update(|window, cx| {
        editor.update(cx, |view, cx| {
            view.set_font(font("monospace"), px(18.), px(27.), cx);
        });
        let before = editor.read(cx).geometry_refreshes;
        window.draw(cx).clear(cx);
        let view = editor.read(cx);
        assert!(!view.wrap_index().unwrap().is_complete());
        assert_eq!(view.geometry_refreshes, before + 1);
    });
    let preview = visual.read(|cx| editor.read(cx).geometry_refreshes);
    redraw(&mut visual);
    let final_count = visual.read(|cx| {
        let view = editor.read(cx);
        assert!(view.layout_ready());
        assert_eq!(view.geometry_refreshes, preview + 1);
        view.geometry_refreshes
    });
    visual.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
    visual.read(|cx| assert_eq!(editor.read(cx).geometry_refreshes, final_count));
}
