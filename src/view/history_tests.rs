use gpui_kit as gpui;
use gpui_kit::{InputHandler, TestAppContext};

use super::input_tests::{handler, open, press_listener, redraw};
use crate::text::ByteOffset;

#[gpui::test]
fn registered_undo_redo_bindings_replay_typing_and_refresh_layout(cx: &mut TestAppContext) {
    let (mut visual, editor) = open("", cx);
    for (undo, redo) in [
        ("ctrl-/", "ctrl-?"),
        ("ctrl-_", "ctrl-alt-_"),
        ("ctrl-x u", "escape ctrl-_"),
    ] {
        visual.simulate_input("中🙂e\u{301}");
        redraw(&mut visual);
        let before = visual.read(|cx| editor.read(cx).display_snapshot().id());
        visual.simulate_keystrokes(undo);
        redraw(&mut visual);
        visual.read(|cx| {
            let view = editor.read(cx);
            assert_eq!(view.buffer().text().to_string(), "");
            assert_eq!(view.buffer().point(), ByteOffset(0));
            assert!(!view.buffer().is_modified());
            assert_eq!(view.message(), Some("Undo"));
            assert!(view.layout_ready());
            assert_ne!(view.display_snapshot().id(), before);
        });
        visual.simulate_keystrokes(redo);
        redraw(&mut visual);
        visual.read(|cx| {
            let view = editor.read(cx);
            assert_eq!(view.buffer().text().to_string(), "中🙂e\u{301}");
            assert_eq!(view.buffer().point(), ByteOffset(0));
            assert_eq!(view.message(), Some("Redo"));
            assert!(view.buffer().is_modified());
            assert!(view.layout_ready());
            assert_ne!(view.display_snapshot().id(), before);
        });
        visual.simulate_keystrokes("ctrl-/");
        redraw(&mut visual);
    }
}

#[gpui::test]
fn listener_deletion_chain_and_history_need_no_intermediate_paint(cx: &mut TestAppContext) {
    let (mut visual, editor) = open("a中🙂e\u{301}Z", cx);
    visual.update(|window, cx| {
        for key in [
            "ctrl-f",
            "ctrl-space",
            "ctrl-f",
            "ctrl-f",
            "ctrl-f",
            "ctrl-f",
            "backspace",
            "backspace",
            "backspace",
            "backspace",
            "ctrl-/",
            "ctrl-?",
            "ctrl-/",
        ] {
            press_listener(&editor, key, window, cx);
        }
        let view = editor.read(cx);
        assert_eq!(view.buffer().text().to_string(), "a中🙂e\u{301}Z");
        assert_eq!(view.buffer().point(), ByteOffset(11));
        assert_eq!(view.buffer().mark(), Some(ByteOffset(1)));
        assert!(!view.buffer().mark_active());
        assert!(!view.buffer().is_modified());
        assert!(view.viewport_layout().is_none());
    });
    redraw(&mut visual);
    visual.read(|cx| assert!(editor.read(cx).layout_ready()));
}

#[gpui::test]
fn undo_revokes_callbacks_and_same_length_replay_never_reuses_old_input_geometry(
    cx: &mut TestAppContext,
) {
    let (mut visual, editor) = open("abc", cx);
    let mut input = handler(&editor, &visual);
    visual.update(|window, cx| input.replace_text_in_range(Some(0..1), "X", window, cx));
    redraw(&mut visual);
    let mut old = handler(&editor, &visual);
    let before = visual.read(|cx| editor.read(cx).display_snapshot().id());
    visual.update(|window, cx| {
        press_listener(&editor, "ctrl-/", window, cx);
        assert!(!old.accepts_text_input(window, cx));
        old.replace_text_in_range(None, "late", window, cx);
        old.replace_and_mark_text_in_range(None, "late", None, window, cx);
        old.unmark_text(window, cx);
        let view = editor.read(cx);
        assert_eq!(view.buffer().text().to_string(), "abc");
        assert_ne!(view.display_snapshot().id(), before);
        assert!(view.viewport_layout().is_none());
        assert_ne!(
            view.input_geometry.as_ref().unwrap().key.display,
            view.display_snapshot().id()
        );
        assert!(old.bounds_for_range(0..0, window, cx).is_none());
    });
    redraw(&mut visual);
    visual.simulate_keystrokes("ctrl-?");
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.buffer().text().to_string(), "Xbc");
        assert_eq!(view.buffer().point(), ByteOffset(0));
        assert!(view.layout_ready());
        assert_eq!(
            view.input_geometry.as_ref().unwrap().key.display,
            view.display_snapshot().id()
        );
    });
}

#[gpui::test]
fn composition_barrier_keeps_its_callback_until_commit_then_undoes_only_committed_text(
    cx: &mut TestAppContext,
) {
    let (mut visual, editor) = open("tail", cx);
    visual.simulate_input("a");
    redraw(&mut visual);
    let mut input = handler(&editor, &visual);
    visual.update(|window, cx| {
        input.replace_and_mark_text_in_range(None, "字", None, window, cx);
        press_listener(&editor, "ctrl-/", window, cx);
        assert!(input.accepts_text_input(window, cx));
        assert_eq!(
            editor.read(cx).message(),
            Some("Input method composition is active; finish it first")
        );
        assert_eq!(editor.read(cx).buffer().text().to_string(), "atail");
        input.unmark_text(window, cx);
    });
    redraw(&mut visual);
    visual.simulate_keystrokes("ctrl-/");
    redraw(&mut visual);
    visual.read(|cx| assert_eq!(editor.read(cx).buffer().text().to_string(), "atail"));
    visual.simulate_keystrokes("ctrl-/");
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.buffer().text().to_string(), "tail");
        assert!(!view.buffer().is_modified());
    });
}

#[gpui::test]
fn region_kill_yank_and_history_have_independent_ring_and_saved_boundaries(
    cx: &mut TestAppContext,
) {
    let (mut visual, editor) = open("abc\ndef", cx);
    visual.simulate_keystrokes("ctrl-f ctrl-f ctrl-space ctrl-a ctrl-k ctrl-k ctrl-y ctrl-y");
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.buffer().text().to_string(), "abc\nabc\ndef");
        assert_eq!(
            view.target.kill_ring().latest().unwrap().to_string(),
            "abc\n"
        );
    });
    editor.update(&mut visual, |view, _| {
        let point = view.target.begin_save();
        view.target.mark_saved_at(point).unwrap();
    });
    visual.simulate_keystrokes("ctrl-/ ctrl-/ ctrl-/ ctrl-/");
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.buffer().text().to_string(), "abc\ndef");
        assert_eq!(view.buffer().point(), ByteOffset(0));
        assert_eq!(view.buffer().mark(), Some(ByteOffset(2)));
        assert!(!view.buffer().mark_active());
        assert!(!view.buffer().is_modified());
        assert_eq!(
            view.target.kill_ring().latest().unwrap().to_string(),
            "abc\n"
        );
        assert!(view.layout_ready());
    });
    visual.simulate_keystrokes("ctrl-? ctrl-? ctrl-? ctrl-?");
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.buffer().text().to_string(), "abc\nabc\ndef");
        assert_eq!(view.buffer().point(), ByteOffset(4));
        assert!(!view.buffer().is_modified());
    });
}

#[gpui::test]
fn readonly_and_empty_history_errors_do_not_mutate_the_visible_region(cx: &mut TestAppContext) {
    let (mut visual, editor) = open("abc", cx);
    visual.simulate_keystrokes("ctrl-space ctrl-f ctrl-/");
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.message(), Some("No further undo information"));
        assert!(view.buffer().mark_active());
    });
    visual.simulate_input("X");
    redraw(&mut visual);
    visual.simulate_keystrokes("ctrl-space ctrl-f");
    editor.update(&mut visual, |view, _| view.target.set_read_only(true));
    let before = visual.read(|cx| editor.read(cx).display_snapshot().id());
    visual.simulate_keystrokes("ctrl-/ ctrl-?");
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.message(), Some("Buffer is read-only"));
        assert_eq!(view.display_snapshot().id(), before);
        assert!(view.buffer().mark_active());
    });
    editor.update(&mut visual, |view, _| view.target.set_read_only(false));
    visual.simulate_keystrokes("ctrl-/");
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.buffer().text().to_string(), "abc");
        assert_eq!(view.buffer().point(), ByteOffset(1));
        assert!(!view.buffer().mark_active());
        assert!(view.region_bounds().is_empty());
    });
}
