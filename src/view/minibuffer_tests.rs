use gpui_kit as gpui;
use gpui_kit::{
    Entity, Focusable, InputHandler, TestAppContext, VisualTestContext, font, point, px,
};

use super::BufferView;
use super::input_tests::{app_shell, handler, open, press_listener, redraw};
use crate::app::AppShell;
use crate::minibuffer::Minibuffer;
use crate::text::ByteOffset;

fn show(
    visual: &mut VisualTestContext,
    prompt: &str,
    initial: &str,
) -> (Entity<AppShell>, Entity<Minibuffer>, Entity<BufferView>) {
    let result = visual.update(|window, cx| {
        let shell = app_shell(window, cx);
        let mini = shell.update(cx, |shell, cx| {
            shell.mount_minibuffer(prompt, initial, window, cx).unwrap()
        });
        let editor = mini.read(cx).editor().clone();
        (shell, mini, editor)
    });
    redraw(visual);
    result
}

fn hide(visual: &mut VisualTestContext, shell: &Entity<AppShell>) {
    visual.update(|window, cx| {
        assert!(shell.update(cx, |shell, cx| shell.unmount_minibuffer(window, cx)));
    });
    redraw(visual);
}

#[gpui::test]
fn mark_override_and_fallback_route_to_body_and_active_minibuffer(cx: &mut TestAppContext) {
    let (mut visual, body) = open("body", cx);
    visual.simulate_keystrokes("ctrl-f ctrl-t ctrl-f ctrl-space");
    visual.read(|cx| {
        let view = body.read(cx);
        assert_eq!(view.buffer().point(), ByteOffset(2));
        assert_eq!(view.buffer().mark(), Some(ByteOffset(1)));
        assert!(view.buffer().mark_active());
        assert_eq!(view.message(), Some("C-SPC is undefined"));
        assert_eq!(view.buffer().text().to_string(), "body");
    });
    visual.simulate_keystrokes("ctrl-@");
    assert_eq!(
        visual.read(|cx| body.read(cx).buffer().mark()),
        Some(ByteOffset(2))
    );

    visual.simulate_keystrokes("alt-x");
    redraw(&mut visual);
    let (shell, editor) = visual.update(|window, cx| {
        let shell = app_shell(window, cx);
        let editor = shell
            .read(cx)
            .minibuffer()
            .unwrap()
            .read(cx)
            .editor()
            .clone();
        (shell, editor)
    });
    visual.simulate_input("abc");
    visual.simulate_keystrokes("ctrl-a ctrl-t ctrl-f ctrl-space");
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.buffer().point(), ByteOffset(5));
        assert_eq!(view.buffer().mark(), Some(ByteOffset(4)));
        assert!(view.buffer().mark_active());
        assert_eq!(view.message(), Some("C-SPC is undefined"));
        assert_eq!(view.buffer().text().to_string(), "M-x abc");
    });
    visual.simulate_keystrokes("ctrl-@");
    assert_eq!(
        visual.read(|cx| editor.read(cx).buffer().mark()),
        Some(ByteOffset(5))
    );
    visual.simulate_keystrokes("ctrl-g");
    visual.read(|cx| {
        assert!(shell.read(cx).session().is_none());
        assert_eq!(body.read(cx).buffer().mark(), Some(ByteOffset(2)));
        assert_eq!(body.read(cx).buffer().point(), ByteOffset(2));
    });
}

#[gpui::test]
fn minibuffer_is_a_real_edit_target_with_prompt_in_text_and_field_aware_keys(
    cx: &mut TestAppContext,
) {
    let (mut visual, body) = open("body", cx);
    let (shell, mini, editor) = show(&mut visual, "输入🙂: ", "");
    visual.simulate_input("中🙂");
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.buffer().text().to_string(), "输入🙂: 中🙂");
        assert_eq!(view.buffer().point(), ByteOffset(19));
        assert_eq!(mini.read(cx).prompt(cx), "输入🙂: ");
        assert_eq!(mini.read(cx).contents(cx), "中🙂");
        assert!(view.layout_ready());
        assert_eq!(body.read(cx).buffer().text().to_string(), "body");
    });
    let id = visual.read(|cx| editor.read(cx).display_snapshot().id());
    visual.simulate_keystrokes("ctrl-a ctrl-b");
    visual.simulate_input("bad");
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.buffer().point(), ByteOffset(11));
        assert_eq!(view.display_snapshot().id(), id);
        assert_eq!(view.message(), Some("Text is read-only"));
    });
    visual.update(|window, cx| {
        assert_eq!(shell.read(cx).echo_message(window, cx), "Text is read-only")
    });
    visual.simulate_keystrokes("ctrl-e");
    assert_eq!(
        visual.read(|cx| editor.read(cx).buffer().point()),
        ByteOffset(19)
    );
    visual.simulate_keystrokes("ctrl-a");
    assert_eq!(
        visual.read(|cx| editor.read(cx).buffer().point()),
        ByteOffset(12)
    );
    visual.simulate_input("X");
    redraw(&mut visual);
    assert_eq!(visual.read(|cx| mini.read(cx).contents(cx)), "X中🙂");
}

#[gpui::test]
fn official_body_minibuffer_kill_yank_and_undo_chain_uses_independent_histories(
    cx: &mut TestAppContext,
) {
    let (mut visual, body) = open("body", cx);
    visual.simulate_keystrokes("ctrl-k");
    visual.simulate_input("A");
    redraw(&mut visual);
    let (shell, mini, editor) = show(&mut visual, "Input: ", "");
    visual.simulate_keystrokes("ctrl-y ctrl-a ctrl-t ctrl-f ctrl-f ctrl-w");
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(mini.read(cx).contents(cx), "dy");
        assert_eq!(body.read(cx).buffer().text().to_string(), "A");
        assert_eq!(
            editor
                .read(cx)
                .target
                .kill_ring()
                .latest()
                .unwrap()
                .to_string(),
            "bo"
        );
    });
    visual.simulate_keystrokes("ctrl-/");
    assert_eq!(
        visual.read(|cx| editor.read(cx).buffer().point()),
        ByteOffset(9)
    );
    visual.simulate_keystrokes("ctrl-/");
    redraw(&mut visual);
    assert_eq!(visual.read(|cx| mini.read(cx).contents(cx)), "");
    hide(&mut visual, &shell);
    visual.simulate_keystrokes("ctrl-/ ctrl-/");
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(body.read(cx).buffer().text().to_string(), "body");
        assert!(!body.read(cx).buffer().is_modified());
        assert_eq!(
            editor
                .read(cx)
                .target
                .kill_ring()
                .latest()
                .unwrap()
                .to_string(),
            "bo"
        );
        assert_eq!(mini.read(cx).prompt(cx), "Input: ");
    });
}

#[gpui::test]
fn callbacks_query_full_prompt_text_but_cannot_replace_or_preedit_over_it(cx: &mut TestAppContext) {
    let (mut visual, _) = open("body", cx);
    let (_, mini, editor) = show(&mut visual, "中: ", "abc");
    let mut input = handler(&editor, &visual);
    visual.update(|window, cx| {
        let mut adjusted = None;
        assert_eq!(
            input.text_for_range(0..6, &mut adjusted, window, cx),
            Some("中: abc".into())
        );
        assert_eq!(
            input.selected_text_range(false, window, cx).unwrap().range,
            6..6
        );
        input.replace_text_in_range(Some(0..1), "X", window, cx);
        input.replace_and_mark_text_in_range(Some(1..4), "bad", None, window, cx);
        assert!(!editor.read(cx).target.input().is_composing());
        assert_eq!(mini.read(cx).contents(cx), "abc");
        input.replace_and_mark_text_in_range(Some(3..4), "ni", None, window, cx);
        assert_eq!(input.marked_text_range(window, cx), Some(3..5));
        assert_eq!(mini.read(cx).contents(cx), "bc");
        press_listener(&editor, "ctrl-/", window, cx);
        assert!(input.accepts_text_input(window, cx));
        input.replace_text_in_range(Some(0..4), "bad", window, cx);
        assert_eq!(
            editor.read(cx).display_snapshot().text().to_string(),
            "中: nibc"
        );
        input.unmark_text(window, cx);
    });
    redraw(&mut visual);
    assert_eq!(visual.read(|cx| mini.read(cx).contents(cx)), "nibc");
    visual.simulate_keystrokes("ctrl-/ ctrl-/");
    redraw(&mut visual);
    assert_eq!(visual.read(|cx| mini.read(cx).contents(cx)), "abc");
}

#[gpui::test]
fn hidden_replaced_and_reopened_targets_reject_old_callbacks_even_when_retained(
    cx: &mut TestAppContext,
) {
    let (mut visual, body) = open("body", cx);
    let mut old_body = handler(&body, &visual);
    let (shell, old_mini, old_editor) = show(&mut visual, "Old: ", "old");
    visual.update(|window, cx| {
        old_body.replace_text_in_range(None, "late", window, cx);
        assert!(!old_body.accepts_text_input(window, cx));
    });
    let mut old_input = handler(&old_editor, &visual);
    hide(&mut visual, &shell);
    let (_, new_mini, new_editor) = show(&mut visual, "New: ", "new");
    visual.update(|window, cx| {
        for input in [&mut old_body, &mut old_input] {
            input.replace_text_in_range(None, "late", window, cx);
            input.replace_and_mark_text_in_range(None, "late", None, window, cx);
            input.unmark_text(window, cx);
            assert!(input.selected_text_range(false, window, cx).is_none());
        }
    });
    assert_eq!(visual.read(|cx| old_mini.read(cx).contents(cx)), "old");
    assert_eq!(visual.read(|cx| new_mini.read(cx).contents(cx)), "new");
    assert_ne!(
        visual.read(|cx| old_editor.read(cx).buffer().snapshot_id().buffer),
        visual.read(|cx| new_editor.read(cx).buffer().snapshot_id().buffer)
    );
    let mut replaced = handler(&new_editor, &visual);
    let (_, latest, _) = show(&mut visual, "Latest: ", "");
    visual.update(|window, cx| replaced.replace_text_in_range(None, "late", window, cx));
    assert_eq!(visual.read(|cx| latest.read(cx).contents(cx)), "");
    assert_eq!(
        visual.read(|cx| body.read(cx).buffer().text().to_string()),
        "body"
    );
}

#[gpui::test]
fn target_switch_discards_preedit_clears_prefix_and_keeps_committed_parts(cx: &mut TestAppContext) {
    let (mut visual, body) = open("body", cx);
    let mut input = handler(&body, &visual);
    visual.update(|window, cx| {
        input.replace_and_mark_text_in_range(Some(0..2), "ni", None, window, cx)
    });
    let (shell, mini, editor) = show(&mut visual, "P: ", "abc");
    assert_eq!(
        visual.read(|cx| body.read(cx).buffer().text().to_string()),
        "dy"
    );
    assert!(!visual.read(|cx| body.read(cx).target.input().is_composing()));
    let mut input = handler(&editor, &visual);
    visual.update(|window, cx| input.replace_and_mark_text_in_range(None, "未", None, window, cx));
    hide(&mut visual, &shell);
    assert_eq!(visual.read(|cx| mini.read(cx).contents(cx)), "abc");
    assert!(!visual.read(|cx| editor.read(cx).target.input().is_composing()));
    visual.simulate_keystrokes("ctrl-x");
    let (shell, _, editor) = show(&mut visual, "Next: ", "");
    assert!(!visual.read(|cx| body.read(cx).target.is_pending()));
    visual.simulate_keystrokes("ctrl-x");
    hide(&mut visual, &shell);
    assert!(!visual.read(|cx| editor.read(cx).target.is_pending()));
    visual.simulate_input("X");
    redraw(&mut visual);
    assert_eq!(
        visual.read(|cx| body.read(cx).buffer().text().to_string()),
        "Xdy"
    );
}

#[gpui::test]
fn pointer_can_enter_and_select_prompt_but_forward_delete_cannot_remove_it(
    cx: &mut TestAppContext,
) {
    let (mut visual, _) = open("body", cx);
    let (_, mini, editor) = show(&mut visual, "Input: ", "abc");
    let caret = visual.read(|cx| {
        let view = editor.read(cx);
        view.viewport_layout()
            .unwrap()
            .bounds_for_range(ByteOffset(1)..ByteOffset(1), view.wrap_index().unwrap())
            .unwrap()
    });
    visual.simulate_click(
        point(caret.left(), caret.top() + px(8.)),
        Default::default(),
    );
    visual.simulate_keystrokes("ctrl-t ctrl-f ctrl-f ctrl-f ctrl-f ctrl-f ctrl-f alt-w");
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(
            view.target.kill_ring().latest().unwrap().to_string(),
            "nput: "
        );
        assert_eq!(view.buffer().point(), ByteOffset(7));
    });
    visual.simulate_keystrokes("ctrl-a ctrl-b ctrl-t ctrl-f ctrl-f delete");
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.message(), Some("Text is read-only"));
        assert!(view.buffer().mark_active());
        assert!(!view.region_bounds().is_empty());
        assert_eq!(mini.read(cx).contents(cx), "abc");
        assert_eq!(mini.read(cx).prompt(cx), "Input: ");
    });
}

#[gpui::test]
fn body_and_minibuffer_keep_separate_scroll_font_mark_and_echo(cx: &mut TestAppContext) {
    let (mut visual, body) = open(&"body\n".repeat(100), cx);
    body.update(&mut visual, |view, cx| {
        view.set_font(font("monospace"), px(18.), px(27.), cx);
        view.scroll_by(point(px(0.), px(81.)), cx);
    });
    redraw(&mut visual);
    let (shell, _, editor) = show(&mut visual, "P: ", "one\ntwo\nthree\nfour");
    visual.simulate_keystrokes("ctrl-t ctrl-b ctrl-x");
    visual.update(|window, cx| assert_eq!(shell.read(cx).echo_message(window, cx), "C-x"));
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(body.read(cx).scroll.y, px(81.));
        assert!(editor.read(cx).scroll.y > px(0.));
        assert_eq!(body.read(cx).settings.line_height, px(27.));
        assert_eq!(editor.read(cx).settings.line_height, px(24.));
        assert_ne!(
            body.read(cx).wrap_index().unwrap().key().display,
            editor.read(cx).wrap_index().unwrap().key().display
        );
    });
    visual.simulate_keystrokes("ctrl-g");
    let caret = visual.read(|cx| {
        let view = body.read(cx);
        view.viewport_layout()
            .unwrap()
            .bounds_for_range(ByteOffset(15)..ByteOffset(15), view.wrap_index().unwrap())
            .unwrap()
    });
    visual.simulate_click(
        point(caret.left(), caret.top() + px(8.)),
        Default::default(),
    );
    redraw(&mut visual);
    visual.update(|window, cx| {
        assert!(body.focus_handle(cx).is_focused(window));
        assert_ne!(
            shell.read(cx).echo_message(window, cx),
            "C-x C-g is undefined"
        );
    });
    visual.simulate_keystrokes("ctrl-t ctrl-f");
    visual.read(|cx| {
        assert_ne!(
            body.read(cx).buffer().mark(),
            editor.read(cx).buffer().mark()
        );
        assert!(editor.read(cx).buffer().mark_active());
    });
}

#[gpui::test]
fn refocusing_the_same_minibuffer_preserves_state_but_not_old_callbacks(cx: &mut TestAppContext) {
    let (mut visual, body) = open("body", cx);
    let (_, mini, editor) = show(&mut visual, "P: ", "abc");
    visual.simulate_input("X");
    visual.simulate_keystrokes("ctrl-t ctrl-b");
    redraw(&mut visual);
    let mut old = handler(&editor, &visual);
    visual.update(|window, cx| {
        old.replace_and_mark_text_in_range(None, "未", None, window, cx);
        window.focus(&body.focus_handle(cx), cx);
    });
    redraw(&mut visual);
    assert_eq!(visual.read(|cx| mini.read(cx).contents(cx)), "abcX");
    visual.simulate_input("Y");
    visual.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    redraw(&mut visual);
    visual.update(|window, cx| {
        old.replace_text_in_range(None, "late", window, cx);
        assert!(old.selected_text_range(false, window, cx).is_none());
        let view = editor.read(cx);
        assert_eq!(view.buffer().point(), ByteOffset(6));
        assert_eq!(view.buffer().mark(), Some(ByteOffset(7)));
        assert!(view.buffer().mark_active());
        assert!(!view.target.input().is_composing());
    });
    let mut before_deactivation = handler(&editor, &visual);
    visual.deactivate_window();
    visual.update(|window, cx| {
        before_deactivation.replace_text_in_range(None, "late", window, cx);
        window.activate_window();
    });
    redraw(&mut visual);
    visual.update(|window, cx| before_deactivation.replace_text_in_range(None, "late", window, cx));
    visual.simulate_input("Z");
    redraw(&mut visual);
    assert_eq!(visual.read(|cx| mini.read(cx).contents(cx)), "abcZX");
    visual.simulate_keystrokes("ctrl-/");
    redraw(&mut visual);
    assert_eq!(visual.read(|cx| mini.read(cx).contents(cx)), "abcX");
    visual.simulate_keystrokes("ctrl-/");
    redraw(&mut visual);
    assert_eq!(visual.read(|cx| mini.read(cx).contents(cx)), "abc");
    assert_eq!(
        visual.read(|cx| body.read(cx).buffer().text().to_string()),
        "Ybody"
    );
}

#[gpui::test]
fn whole_buffer_readonly_remains_distinct_from_prompt_protection(cx: &mut TestAppContext) {
    let (mut visual, _) = open("body", cx);
    let (_, mini, editor) = show(&mut visual, "P: ", "abc");
    editor.update(&mut visual, |view, _| view.target.set_read_only(true));
    visual.simulate_keystrokes("ctrl-a ctrl-k ctrl-y");
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.message(), Some("Buffer is read-only"));
        assert_eq!(view.target.kill_ring().latest().unwrap().to_string(), "abc");
        assert_eq!(mini.read(cx).contents(cx), "abc");
    });
    editor.update(&mut visual, |view, _| view.target.set_read_only(false));
    visual.simulate_input("X");
    redraw(&mut visual);
    assert_eq!(visual.read(|cx| mini.read(cx).contents(cx)), "abcX");
}

#[gpui::test]
fn close_cancels_both_compositions_and_cancel_restores_body_focus(cx: &mut TestAppContext) {
    let (mut visual, body) = open("body", cx);
    visual.simulate_input("X");
    let (shell, mini, editor) = show(&mut visual, "P: ", "abc");
    let mut input = handler(&editor, &visual);
    visual.update(|window, cx| input.replace_and_mark_text_in_range(None, "未", None, window, cx));
    assert!(!visual.simulate_close());
    visual.update(|window, cx| {
        assert!(shell.read(cx).session().is_some());
        input.replace_text_in_range(None, "late", window, cx);
        assert!(shell.read(cx).minibuffer().is_some());
        assert!(
            shell
                .update(cx, |shell, cx| shell
                    .mount_minibuffer("blocked", "", window, cx))
                .is_none()
        );
    });
    redraw(&mut visual);
    visual.simulate_keystrokes("ctrl-g");
    redraw(&mut visual);
    visual.update(|window, cx| {
        assert!(shell.read(cx).session().is_none());
        assert!(body.read(cx).focus_handle(cx).is_focused(window));
        input.replace_text_in_range(None, "late", window, cx);
    });
    assert_eq!(visual.read(|cx| mini.read(cx).contents(cx)), "abc");
    visual.simulate_input("Y");
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(body.read(cx).buffer().text().to_string(), "XYbody");
        assert_eq!(mini.read(cx).contents(cx), "abc");
    });
}

#[gpui::test]
fn transient_minibuffer_dirty_state_does_not_make_a_clean_document_unsaved(
    cx: &mut TestAppContext,
) {
    let (mut visual, body) = open("body", cx);
    let (_, _, editor) = show(&mut visual, "P: ", "");
    visual.simulate_input("transient");
    assert!(visual.simulate_close());
    visual.update(|_, cx| {
        assert!(!body.read(cx).buffer().is_modified());
        assert!(editor.read(cx).buffer().is_modified());
    });
}

#[gpui::test]
fn bare_mount_keeps_plain_edit_bindings_without_read_semantics(cx: &mut TestAppContext) {
    let (mut visual, _) = open("body", cx);
    let (shell, mini, _) = show(&mut visual, "P: ", "");
    visual.simulate_input("abc");
    visual.simulate_keystrokes("ctrl-t ctrl-b ctrl-g enter");
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(shell.read(cx).minibuffer(), Some(&mini));
        assert_eq!(mini.read(cx).contents(cx), "ab\nc");
    });
    visual.simulate_keystrokes("ctrl-/");
    redraw(&mut visual);
    assert_eq!(visual.read(|cx| mini.read(cx).contents(cx)), "abc");
}

#[gpui::test]
fn mounting_and_editing_without_paint_ends_old_sessions_before_the_new_kill_chain(
    cx: &mut TestAppContext,
) {
    let (mut visual, body) = open("body", cx);
    visual.simulate_keystrokes("ctrl-k");
    let (mini, editor) = visual.update(|window, cx| {
        let shell = app_shell(window, cx);
        let mini = shell.update(cx, |shell, cx| {
            shell.mount_minibuffer("P: ", "ab\ncd", window, cx).unwrap()
        });
        let editor = mini.read(cx).editor().clone();
        for key in ["alt-<", "ctrl-f", "ctrl-f", "ctrl-f", "ctrl-k", "ctrl-k"] {
            press_listener(&editor, key, window, cx);
        }
        assert_eq!(
            editor
                .read(cx)
                .target
                .kill_ring()
                .latest()
                .unwrap()
                .to_string(),
            "ab\n"
        );
        assert!(
            !body
                .read(cx)
                .target
                .snapshot_for(body.read(cx).target.token())
                .is_ok()
        );
        (mini, editor)
    });
    redraw(&mut visual);
    visual.simulate_keystrokes("ctrl-k");
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(mini.read(cx).contents(cx), "");
        assert_eq!(
            editor
                .read(cx)
                .target
                .kill_ring()
                .latest()
                .unwrap()
                .to_string(),
            "ab\ncd"
        );
    });
}
