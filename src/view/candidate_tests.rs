use std::sync::Arc;

use gpui_kit as gpui;
use gpui_kit::{InputHandler, Keystroke, TestAppContext};

use super::input_tests::{app_shell, handler, open, redraw};
use crate::text::ByteOffset;

#[gpui::test]
fn candidates_freeze_during_preedit_and_editor_selection_keys_cannot_accept_it(
    cx: &mut TestAppContext,
) {
    let (mut visual, body) = open("body", cx);
    visual.simulate_keystrokes("alt-x");
    visual.simulate_input("forward-");
    redraw(&mut visual);
    let (shell, mini, editor, before) = visual.update(|window, cx| {
        let shell = app_shell(window, cx);
        let mini = shell.read(cx).minibuffer().unwrap().clone();
        let editor = mini.read(cx).editor().clone();
        let before = shell
            .read(cx)
            .session()
            .unwrap()
            .candidates()
            .unwrap()
            .clone();
        (shell, mini, editor, before)
    });
    let mut input = handler(&editor, &visual);
    visual.update(|window, cx| input.replace_and_mark_text_in_range(None, "wor", None, window, cx));
    redraw(&mut visual);
    visual.simulate_keystrokes("ctrl-n tab enter alt-enter ctrl-g");
    redraw(&mut visual);
    visual.read(|cx| {
        let list = shell.read(cx).session().unwrap().candidates().unwrap();
        assert!(Arc::ptr_eq(&list.items, &before.items));
        assert_eq!(list.selected, before.selected);
        assert_eq!(mini.read(cx).contents(cx), "forward-");
        assert_eq!(
            editor.read(cx).message(),
            Some("Input method composition is active; finish it first")
        );
        assert_eq!(body.read(cx).buffer().point(), ByteOffset(0));
    });
    let mut input = handler(&editor, &visual);
    visual.update(|window, cx| input.replace_text_in_range(None, "word", window, cx));
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(mini.read(cx).contents(cx), "forward-word");
        assert_eq!(
            shell
                .read(cx)
                .session()
                .unwrap()
                .candidates()
                .unwrap()
                .selected_candidate()
                .unwrap()
                .value,
            "forward-word"
        );
    });
    visual.simulate_keystrokes("enter");
    assert_eq!(
        visual.read(|cx| body.read(cx).buffer().point()),
        ByteOffset(4)
    );
}

#[gpui::test]
fn preedit_cancels_a_pending_file_insertion_without_accepting_the_delayed_candidate(
    cx: &mut TestAppContext,
) {
    let (mut visual, _) = open("body", cx);
    visual.simulate_keystrokes("ctrl-x ctrl-f");
    redraw(&mut visual);
    let (shell, mini, editor) = visual.update(|window, cx| {
        let shell = app_shell(window, cx);
        let mini = shell.read(cx).minibuffer().unwrap().clone();
        let editor = mini.read(cx).editor().clone();
        (shell, mini, editor)
    });
    visual.update(|window, cx| {
        editor.update(cx, |view, cx| view.replace_input("Cargo", window, cx));
        window.dispatch_keystroke(Keystroke::parse("tab").unwrap(), cx);
        let view = editor.read(cx);
        let mut input = crate::platform::BufferInputHandler {
            view: editor.downgrade(),
            token: view.target.input().token(),
            registration: view.registration,
        };
        input.replace_and_mark_text_in_range(None, "preedit", None, window, cx);
    });
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(mini.read(cx).contents(cx), "Cargo");
        assert!(
            shell
                .read(cx)
                .session()
                .unwrap()
                .candidates()
                .unwrap()
                .loading
        );
    });
    let mut input = handler(&editor, &visual);
    visual.update(|window, cx| input.replace_and_mark_text_in_range(None, "", None, window, cx));
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(mini.read(cx).contents(cx), "Cargo");
        assert!(
            !shell
                .read(cx)
                .session()
                .unwrap()
                .candidates()
                .unwrap()
                .loading
        );
    });
    visual.simulate_keystrokes("ctrl-g");
}
