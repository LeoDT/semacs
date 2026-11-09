use super::BufferView;
use crate::app::AppShell;
use crate::buffer::Buffer;
use crate::display::{FontMeasure, WrapJob};
use crate::files::{DecodedFile, FileFormat};
use crate::platform::BufferInputHandler;
use crate::text::{ByteOffset, range_to_utf16};
use gpui_kit as gpui;
use gpui_kit::{
    App, AppContext, Entity, Focusable, InputHandler, TestAppContext, VisualTestContext, Window,
    font, point, px, size,
};
use std::sync::atomic::AtomicBool;

pub(super) fn open(text: &str, cx: &mut TestAppContext) -> (VisualTestContext, Entity<BufferView>) {
    let buffer = Buffer::new(
        None,
        DecodedFile {
            text: text.into(),
            format: FileFormat::default(),
        },
    );
    open_buffer(buffer, cx)
}

pub(super) fn open_file(
    text: &str,
    cx: &mut TestAppContext,
) -> (VisualTestContext, Entity<BufferView>) {
    open_buffer(
        Buffer::new(
            Some(
                std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join(".pi/tmp/close-input.txt"),
            ),
            DecodedFile {
                text: text.into(),
                format: FileFormat::default(),
            },
        ),
        cx,
    )
}

pub(super) fn open_buffer(
    buffer: Buffer,
    cx: &mut TestAppContext,
) -> (VisualTestContext, Entity<BufferView>) {
    cx.update(crate::app::init);
    let window = cx.open_window(size(px(400.), px(240.)), |window, cx| {
        let shell = cx.new(|cx| AppShell::new(buffer, window, cx));
        gpui_kit::base::Root::new(shell, window, cx)
    });
    let editor = window
        .read_with(cx, |root, cx| {
            root.view()
                .clone()
                .downcast::<AppShell>()
                .unwrap()
                .read(cx)
                .active_view()
                .clone()
        })
        .unwrap();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.update(|window, _| window.activate_window());
    redraw(&mut visual);
    (visual, editor)
}

pub(super) fn app_shell(window: &Window, cx: &App) -> Entity<AppShell> {
    window
        .root::<gpui_kit::base::Root>()
        .unwrap()
        .unwrap()
        .read(cx)
        .view()
        .clone()
        .downcast::<AppShell>()
        .unwrap()
}

pub(super) fn redraw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

pub(crate) fn handler(editor: &Entity<BufferView>, cx: &VisualTestContext) -> BufferInputHandler {
    cx.read(|cx| {
        let view = editor.read(cx);
        BufferInputHandler {
            view: editor.downgrade(),
            token: view.target.input().token(),
            registration: view.registration,
        }
    })
}

pub(super) fn press_listener(
    editor: &Entity<super::BufferView>,
    key: &'static str,
    window: &mut gpui_kit::Window,
    cx: &mut gpui_kit::App,
) {
    editor.update(cx, |view, cx| {
        view.key_down(
            &gpui_kit::KeyDownEvent {
                keystroke: gpui_kit::Keystroke::parse(key).unwrap(),
                is_held: false,
                prefer_character_input: false,
            },
            window,
            cx,
        );
    });
}

#[gpui::test]
fn registered_handler_receives_typing_newline_and_scalar_deletion(cx: &mut TestAppContext) {
    let (mut visual, editor) = open("", cx);
    visual.simulate_input("中🙂e\u{301}");
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(
            editor.read(cx).target.buffer().text().to_string(),
            "中🙂e\u{301}"
        )
    });
    visual.simulate_keystrokes("backspace enter");
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.target.buffer().text().to_string(), "中🙂e\n");
        assert!(view.target.buffer().is_modified());
        assert!(view.layout_ready());
    });
    visual.simulate_keystrokes("ctrl-v ctrl-c ctrl-x");
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.target.buffer().text().to_string(), "中🙂e\n");
        assert_eq!(view.message(), Some("C-x"));
        assert!(view.target.is_pending());
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
        assert_eq!(view.target.buffer().text().to_string(), "终!tail");
        assert_eq!(view.target.buffer().point(), ByteOffset(4));
        assert!(!view.target.input().is_composing());
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
    visual.read(|cx| {
        assert_eq!(
            editor.read(cx).target.buffer().text().to_string(),
            "终!保留后tail"
        )
    });
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
        assert_eq!(view.target.buffer().text().to_string(), "body");
        assert_eq!(view.display_snapshot().text().to_string(), "拼音body");
        assert_eq!(
            view.message(),
            Some("Input method composition is active; finish it first")
        );
        assert!(!view.target.buffer().is_modified());
    });
    let mut input = handler(&editor, &visual);
    visual.update(|window, cx| {
        input.replace_and_mark_text_in_range(None, "", None, window, cx);
        assert_eq!(input.marked_text_range(window, cx), None);
        input.replace_text_in_range(None, "完", window, cx);
    });
    redraw(&mut visual);
    visual.read(|cx| assert_eq!(editor.read(cx).target.buffer().text().to_string(), "完body"));
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
        assert!(!view.target.buffer().is_modified());
        assert!(view.viewport_layout().is_some());
    });
    visual.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    redraw(&mut visual);
    visual.update(|window, cx| old.replace_text_in_range(None, "late", window, cx));
    visual.simulate_input("new");
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(
            editor.read(cx).target.buffer().text().to_string(),
            "newbody"
        )
    });
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
        assert!(!editor.read(cx).target.buffer().is_modified());
    });
    visual.update(|window, _| window.activate_window());
    redraw(&mut visual);
    visual.update(|window, cx| old.replace_text_in_range(None, "late", window, cx));
    visual.simulate_input("new");
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(
            editor.read(cx).target.buffer().text().to_string(),
            "newbody"
        )
    });
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
fn scrolled_reflow_retains_paint_but_never_reuses_old_input_geometry(cx: &mut TestAppContext) {
    for change_font in [false, true] {
        let (mut visual, editor) = open(&"x".repeat(1024 * 1024), cx);
        visual.update(|window, cx| {
            editor.update(cx, |view, cx| {
                let bounds = view.viewport_layout().unwrap().bounds().inset(px(-8.));
                view.scroll_by(point(px(0.), px(3600.)), cx);
                view.prepare(bounds, window, cx);
                let old = view.paint_layout().unwrap().clone();
                assert!(!old.rows.is_empty());
                let mut new_bounds = bounds;
                if change_font {
                    view.set_font(font("monospace").bold(), px(18.), px(27.), cx);
                } else {
                    new_bounds.size.width = px(250.);
                }
                view.prepare(new_bounds, window, cx);
                assert!(!view.layout_ready());
                assert!(view.viewport_layout().unwrap().rows.is_empty());
                let retained = view.paint_layout().unwrap();
                assert_eq!(retained.key(), old.key());
                assert_ne!(retained.key(), view.wrap_index().unwrap().key());
                assert!(std::sync::Arc::ptr_eq(&retained.rows[0], &old.rows[0]));
                assert_eq!(retained.bounds(), view.viewport_layout().unwrap().bounds());
            });
            let mut input = BufferInputHandler {
                view: editor.downgrade(),
                token: editor.read(cx).target.input().token(),
                registration: editor.read(cx).registration,
            };
            let position =
                editor.read(cx).viewport_layout().unwrap().bounds().origin + point(px(4.), px(4.));
            assert!(
                input
                    .character_index_for_point(position, window, cx)
                    .is_none()
            );
            input.replace_text_in_range(None, "new", window, cx);
            assert!(editor.read(cx).retained_paint_layout.is_none());
            assert!(editor.read(cx).paint_layout().is_none());
        });
        redraw(&mut visual);
        visual.read(|cx| {
            let view = editor.read(cx);
            assert!(view.layout_ready());
            assert_eq!(
                view.paint_layout().unwrap().key(),
                view.viewport_layout().unwrap().key()
            );
            assert!(view.retained_paint_layout.is_none());
            assert!(view.target.buffer().text().to_string().starts_with("new"));
        });
    }
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
        let committed = editor.read(cx).target.buffer().snapshot_id();
        let reversed = std::ops::Range { start: 3, end: 1 };
        input.replace_and_mark_text_in_range(Some(0..2), "bad", Some(reversed.clone()), window, cx);
        input.replace_text_in_range(Some(reversed.clone()), "bad", window, cx);
        assert_eq!(editor.read(cx).display_snapshot().id(), before.id());
        assert_eq!(editor.read(cx).target.buffer().snapshot_id(), committed);
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
    let (mut visual, editor) = open_file("a🙂中z", cx);
    let mut input = handler(&editor, &visual);
    visual.update(|window, cx| {
        input.replace_and_mark_text_in_range(Some(1..4), "ni", None, window, cx);
        assert_eq!(editor.read(cx).target.buffer().text().to_string(), "az");
        let marked = input.marked_text_range(window, cx).unwrap();
        input.replace_text_in_range(Some(marked), "", window, cx);
    });
    redraw(&mut visual);
    let mut old = handler(&editor, &visual);
    assert!(!visual.simulate_close());
    visual.update(|window, cx| {
        let shell = app_shell(window, cx);
        let shell = shell.read(cx);
        assert!(shell.session().is_some());
        assert!(
            shell
                .minibuffer()
                .unwrap()
                .read(cx)
                .prompt(cx)
                .contains("Save file")
        );
    });
    assert!(!visual.simulate_close());
    visual.update(|window, cx| old.replace_text_in_range(None, "late", window, cx));
    redraw(&mut visual);
    visual.simulate_keystrokes("ctrl-g");
    redraw(&mut visual);
    visual.update(|window, cx| {
        let shell = app_shell(window, cx);
        assert!(shell.read(cx).session().is_none());
        assert!(editor.focus_handle(cx).is_focused(window));
        old.replace_text_in_range(None, "late", window, cx);
    });
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.target.buffer().text().to_string(), "az");
        assert!(view.target.buffer().is_modified());
    });
    visual.simulate_input("x");
    redraw(&mut visual);
    assert!(!visual.simulate_close());
    redraw(&mut visual);
    visual.simulate_keystrokes("n");
    redraw(&mut visual);
    visual.simulate_input("yes");
    visual.simulate_keystrokes("enter");
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
    visual.read(|cx| {
        assert_eq!(
            editor.read(cx).display_snapshot().text().to_string(),
            "body"
        );
        assert!(!editor.read(cx).target.buffer().is_modified());
    });

    let (mut visual, editor) = open_file("body", cx);
    let mut input = handler(&editor, &visual);
    visual.update(|window, cx| {
        input.replace_text_in_range(None, "提交", window, cx);
        input.replace_and_mark_text_in_range(None, "临时", None, window, cx);
    });
    assert!(!visual.simulate_close());
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.display_snapshot().text().to_string(), "提交body");
        assert!(view.target.buffer().is_modified());
    });
    redraw(&mut visual);
    visual.update(|window, cx| {
        let shell = app_shell(window, cx);
        assert!(shell.read(cx).session().is_some());
    });
    visual.simulate_keystrokes("ctrl-g");
    redraw(&mut visual);
    visual.update(|window, cx| {
        let shell = app_shell(window, cx);
        assert!(shell.read(cx).session().is_none());
        assert!(editor.focus_handle(cx).is_focused(window));
    });
}

#[gpui::test]
fn modified_non_file_close_discards_preedit_without_save_confirmation(cx: &mut TestAppContext) {
    let (mut visual, editor) = open_buffer(Buffer::named("notes"), cx);
    visual.simulate_input("draft");
    let mut input = handler(&editor, &visual);
    visual
        .update(|window, cx| input.replace_and_mark_text_in_range(None, "临时", None, window, cx));
    assert!(visual.simulate_close());
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.display_snapshot().text().to_string(), "draft");
        assert!(view.buffer().is_modified());
        assert!(view.buffer().path().is_none());
    });
}

#[gpui::test]
fn close_confirmation_defaults_to_cancel_and_accepts_keyboard_quit(cx: &mut TestAppContext) {
    let (mut visual, editor) = open_file("", cx);
    visual.simulate_input("dirty");
    for keys in ["enter ctrl-g", "ctrl-g"] {
        assert!(!visual.simulate_close());
        redraw(&mut visual);
        visual.update(|window, cx| {
            let shell = app_shell(window, cx);
            assert!(shell.read(cx).session().is_some());
        });
        for key in keys.split_whitespace() {
            visual.simulate_keystrokes(key);
            redraw(&mut visual);
        }
        visual.update(|window, cx| {
            let shell = app_shell(window, cx);
            assert!(shell.read(cx).session().is_none());
            assert!(editor.focus_handle(cx).is_focused(window));
        });
        visual.simulate_input("x");
    }
    visual.read(|cx| {
        assert_eq!(
            editor.read(cx).target.buffer().text().to_string(),
            "dirtyxx"
        );
        assert!(editor.read(cx).target.buffer().is_modified());
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
        let point = view.target.buffer().point();
        assert!(range_to_utf16(&view.target.buffer().text(), point..point).is_some());
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

#[gpui::test]
fn keyboard_commands_preserve_prefix_quit_and_scalar_editing_semantics(cx: &mut TestAppContext) {
    let (mut visual, editor) = open("中e\u{301}🙂", cx);
    visual.simulate_keystrokes("ctrl-f ctrl-t ctrl-f ctrl-x ctrl-g");
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.buffer().point(), ByteOffset(4));
        assert_eq!(view.buffer().mark(), Some(ByteOffset(3)));
        assert!(view.buffer().mark_active());
        assert!(!view.target.is_pending());
        assert_eq!(view.message(), Some("C-x C-g is undefined"));
        assert!(!view.buffer().is_modified());
    });
    visual.simulate_keystrokes("ctrl-g");
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.message(), Some("Quit"));
        assert_eq!(view.buffer().mark(), Some(ByteOffset(3)));
        assert!(!view.buffer().mark_active());
    });
    visual.simulate_keystrokes("ctrl-a ctrl-d enter ctrl-j");
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.buffer().text().to_string(), "\n\ne\u{301}🙂");
        assert_eq!(view.buffer().point(), ByteOffset(2));
        assert!(view.layout_ready());
    });
}

#[gpui::test]
fn pending_prefix_cannot_be_bypassed_by_direct_platform_callbacks(cx: &mut TestAppContext) {
    let (mut visual, editor) = open("abc", cx);
    visual.simulate_keystrokes("ctrl-x");
    redraw(&mut visual);
    let mut input = handler(&editor, &visual);
    visual.update(|window, cx| {
        assert!(!input.accepts_text_input(window, cx));
        input.replace_text_in_range(None, "bad", window, cx);
        input.replace_and_mark_text_in_range(None, "bad", None, window, cx);
        input.unmark_text(window, cx);
        let view = editor.read(cx);
        assert_eq!(view.buffer().text().to_string(), "abc");
        assert!(view.target.is_pending());
        assert!(!view.target.input().is_composing());
        assert!(!view.buffer().is_modified());
    });
    visual.simulate_keystrokes("z");
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.message(), Some("C-x z is undefined"));
        assert_eq!(view.buffer().text().to_string(), "abc");
        assert!(!view.target.is_pending());
    });
    visual.update(|window, cx| input.replace_text_in_range(None, "late", window, cx));
    visual.simulate_input("z");
    redraw(&mut visual);
    visual.read(|cx| assert_eq!(editor.read(cx).buffer().text().to_string(), "zabc"));
}

#[gpui::test]
fn motion_revokes_old_callbacks_without_rebuilding_compatible_text_layout(cx: &mut TestAppContext) {
    let (mut visual, editor) = open("中🙂tail", cx);
    let mut old = handler(&editor, &visual);
    let (display, layout, shaped) = visual.read(|cx| {
        let view = editor.read(cx);
        (
            view.display_snapshot().id(),
            view.wrap_index().unwrap().key().clone(),
            view.cache.values().cloned().collect::<Vec<_>>(),
        )
    });
    visual.update(|window, cx| {
        window.dispatch_keystroke(gpui_kit::Keystroke::parse("ctrl-f").unwrap(), cx);
        old.replace_text_in_range(None, "late", window, cx);
        assert!(old.selected_text_range(false, window, cx).is_none());
        let view = editor.read(cx);
        assert_eq!(view.buffer().point(), ByteOffset(3));
        assert_eq!(view.buffer().text().to_string(), "中🙂tail");
    });
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.display_snapshot().id(), display);
        assert_eq!(*view.wrap_index().unwrap().key(), layout);
        for row in &shaped {
            assert!(
                view.cache
                    .values()
                    .any(|current| std::sync::Arc::ptr_eq(row, current))
            );
        }
        assert!(!view.buffer().is_modified());
    });
    visual.simulate_input("字");
    visual.read(|cx| assert_eq!(editor.read(cx).buffer().text().to_string(), "中字🙂tail"));
}

#[gpui::test]
fn composition_barrier_preserves_the_current_registration_inside_the_key_listener(
    cx: &mut TestAppContext,
) {
    let (mut visual, editor) = open("tail", cx);
    let mut input = handler(&editor, &visual);
    visual
        .update(|window, cx| input.replace_and_mark_text_in_range(None, "拼音", None, window, cx));
    redraw(&mut visual);
    let mut input = handler(&editor, &visual);
    visual.update(|window, cx| {
        editor.update(cx, |view, cx| {
            view.key_down(
                &gpui_kit::KeyDownEvent {
                    keystroke: gpui_kit::Keystroke::parse("ctrl-x").unwrap(),
                    is_held: false,
                    prefer_character_input: false,
                },
                window,
                cx,
            );
        });
        assert_eq!(
            editor.read(cx).message(),
            Some("Input method composition is active; finish it first")
        );
        assert!(!editor.read(cx).target.is_pending());
        assert_eq!(
            input.registration,
            editor.read(cx).registration,
            "the barrier must not revoke the current callback"
        );
        input.replace_text_in_range(None, "汉", window, cx);
        assert_eq!(editor.read(cx).buffer().text().to_string(), "汉tail");
        assert!(!editor.read(cx).target.input().is_composing());
    });
    redraw(&mut visual);
    visual.simulate_keystrokes("ctrl-x");
    visual.read(|cx| assert_eq!(editor.read(cx).message(), Some("C-x")));
}

#[gpui::test]
fn readonly_blocks_typing_commands_and_direct_callbacks_but_allows_motion(cx: &mut TestAppContext) {
    let (mut visual, editor) = open("abc", cx);
    visual.update(|_, cx| {
        editor.update(cx, |view, cx| {
            view.target.set_read_only(true);
            cx.notify();
        })
    });
    redraw(&mut visual);
    let initial = visual.read(|cx| editor.read(cx).buffer().snapshot_id());
    let mut input = handler(&editor, &visual);
    visual.update(|window, cx| {
        assert!(!input.accepts_text_input(window, cx));
        input.replace_text_in_range(None, "bad", window, cx);
        input.replace_and_mark_text_in_range(None, "bad", None, window, cx);
    });
    visual.simulate_input("x");
    visual.simulate_keystrokes("ctrl-f ctrl-d backspace enter");
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.buffer().snapshot_id(), initial);
        assert_eq!(view.buffer().point(), ByteOffset(1));
        assert_eq!(view.buffer().text().to_string(), "abc");
        assert_eq!(view.message(), Some("Buffer is read-only"));
        assert!(!view.target.input().is_composing());
        assert!(!view.buffer().is_modified());
    });
}

#[gpui::test]
fn printable_binding_runs_the_registered_command_instead_of_self_insert(cx: &mut TestAppContext) {
    let (mut visual, editor) = open("abc", cx);
    visual.update(|_, cx| {
        editor.update(cx, |view, _| {
            view.keymap
                .bind(
                    &crate::keymap::parse_sequence("q").unwrap(),
                    crate::command::Command::ForwardChar,
                )
                .unwrap();
        })
    });
    visual.simulate_input("q");
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.buffer().point(), ByteOffset(1));
        assert_eq!(view.buffer().text().to_string(), "abc");
        assert!(!view.buffer().is_modified());
    });
}

#[gpui::test]
fn gui_delete_uses_emacs_region_semantics_and_backspace_remains_scalar(cx: &mut TestAppContext) {
    let (mut visual, editor) = open("abcdef", cx);
    visual.simulate_keystrokes("ctrl-f ctrl-t ctrl-f ctrl-f ctrl-f delete");
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.buffer().text().to_string(), "aef");
        assert_eq!(view.buffer().mark(), Some(ByteOffset(1)));
        assert!(!view.buffer().mark_active());
    });
    visual.simulate_keystrokes("backspace");
    redraw(&mut visual);
    visual.read(|cx| assert_eq!(editor.read(cx).buffer().text().to_string(), "ef"));
}

#[gpui::test]
fn typing_does_not_replace_an_active_region_and_indent_aliases_use_commands(
    cx: &mut TestAppContext,
) {
    let (mut visual, editor) = open("abcdef", cx);
    visual.simulate_keystrokes("ctrl-f ctrl-t ctrl-f ctrl-f");
    visual.simulate_input("X");
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.buffer().text().to_string(), "abcXdef");
        assert_eq!(view.buffer().mark(), Some(ByteOffset(1)));
        assert!(!view.buffer().mark_active());
    });
    visual.simulate_keystrokes("ctrl-a tab ctrl-a ctrl-i");
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.buffer().text().to_string(), "\t\tabcXdef");
        assert_eq!(view.buffer().point(), ByteOffset(1));
        assert!(view.layout_ready());
    });
}
