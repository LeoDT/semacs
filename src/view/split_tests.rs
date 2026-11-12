use gpui_kit as gpui;
use gpui_kit::{
    Entity, Focusable, InputHandler, TestAppContext, VisualTestContext, point, px, size,
};

use super::BufferView;
use super::input_tests::{app_shell, handler, open, redraw};
use crate::text::ByteOffset;

fn split(cx: &mut TestAppContext) -> (VisualTestContext, Entity<BufferView>, Entity<BufferView>) {
    let (mut visual, first) = open("abcdef", cx);
    visual.simulate_resize(size(px(960.), px(640.)));
    redraw(&mut visual);
    visual.simulate_keystrokes("ctrl-x 3 ctrl-x o");
    redraw(&mut visual);
    let second = visual.update(|window, cx| app_shell(window, cx).read(cx).active_view().clone());
    (visual, first, second)
}

#[gpui::test]
fn switching_views_revokes_old_callbacks_even_after_returning_to_the_same_buffer(
    cx: &mut TestAppContext,
) {
    let (mut visual, first, second) = split(cx);
    let mut old = handler(&second, &visual);
    visual.simulate_keystrokes("ctrl-x o");
    visual.simulate_input("X");
    visual.simulate_keystrokes("ctrl-x o");
    redraw(&mut visual);
    visual.update(|window, cx| {
        assert!(second.focus_handle(cx).is_focused(window));
        old.replace_text_in_range(None, "late", window, cx);
        old.replace_and_mark_text_in_range(None, "late", None, window, cx);
        old.unmark_text(window, cx);
        assert!(!old.accepts_text_input(window, cx));
        assert_eq!(first.read(cx).buffer().text().to_string(), "Xabcdef");
        assert!(!second.read(cx).target.input().is_composing());
    });
    visual.simulate_input("Y");
    visual.read(|cx| assert_eq!(first.read(cx).buffer().text().to_string(), "YXabcdef"));
}

#[gpui::test]
fn composition_blocks_window_prefix_without_leaking_pending_keys(cx: &mut TestAppContext) {
    let (mut visual, first, second) = split(cx);
    let mut input = handler(&second, &visual);
    visual.update(|window, cx| input.replace_and_mark_text_in_range(None, "ni", None, window, cx));
    for keys in ["ctrl-x", "alt-x"] {
        visual.simulate_keystrokes(keys);
        redraw(&mut visual);
        visual.update(|window, cx| {
            assert!(second.focus_handle(cx).is_focused(window));
            assert_eq!(app_shell(window, cx).read(cx).active_view(), &second);
            assert!(second.read(cx).target.input().is_composing());
            assert!(!second.read(cx).target.is_pending());
            assert_eq!(first.read(cx).buffer().text().to_string(), "abcdef");
        });
    }
    let mut input = handler(&second, &visual);
    visual.update(|window, cx| input.replace_text_in_range(None, "你", window, cx));
    redraw(&mut visual);
    visual.read(|cx| assert_eq!(first.read(cx).buffer().text().to_string(), "你abcdef"));
    visual.simulate_keystrokes("ctrl-x o");
    visual.update(|window, cx| assert!(first.focus_handle(cx).is_focused(window)));
}

#[gpui::test]
fn clicking_another_view_discards_only_marked_text_and_routes_the_next_input_there(
    cx: &mut TestAppContext,
) {
    let (mut visual, first, second) = split(cx);
    let mut input = handler(&second, &visual);
    visual.update(|window, cx| {
        input.replace_and_mark_text_in_range(Some(0..2), "ni", None, window, cx)
    });
    redraw(&mut visual);
    let location = visual.read(|cx| first.read(cx).viewport_layout().unwrap().bounds().origin);
    visual.simulate_click(
        point(location.x + px(1.), location.y + px(8.)),
        Default::default(),
    );
    visual.simulate_input("X");
    redraw(&mut visual);
    visual.update(|window, cx| {
        assert!(first.focus_handle(cx).is_focused(window));
        assert!(!second.read(cx).target.input().is_composing());
        assert_eq!(first.read(cx).buffer().text().to_string(), "Xcdef");
        assert_eq!(second.read(cx).buffer().text().to_string(), "Xcdef");
        assert!(first.read(cx).buffer().is_modified());
        input.replace_text_in_range(None, "late", window, cx);
        assert_eq!(first.read(cx).buffer().text().to_string(), "Xcdef");
    });
}

#[gpui::test]
fn pointer_switch_clears_old_pending_prefix_and_moves_the_mark_to_the_click(
    cx: &mut TestAppContext,
) {
    let (mut visual, first, second) = split(cx);
    visual.simulate_keystrokes("ctrl-f ctrl-t ctrl-f ctrl-x");
    redraw(&mut visual);
    assert!(visual.read(|cx| second.read(cx).target.is_pending()));
    let location = visual.read(|cx| first.read(cx).viewport_layout().unwrap().bounds().origin);
    visual.simulate_click(
        point(location.x + px(1.), location.y + px(8.)),
        Default::default(),
    );
    redraw(&mut visual);
    visual.read(|cx| {
        assert!(!second.read(cx).target.is_pending());
        assert_eq!(first.read(cx).buffer().point(), ByteOffset(0));
        assert_eq!(first.read(cx).buffer().mark(), Some(ByteOffset(0)));
        assert!(!first.read(cx).buffer().mark_active());
        assert!(first.read(cx).selected);
        assert!(!second.read(cx).selected);
    });
    visual.simulate_input("X");
    visual.read(|cx| assert_eq!(second.read(cx).buffer().text().to_string(), "Xabcdef"));
}

#[gpui::test]
fn platform_ime_matrix_covers_nested_body_views_and_minibuffer_without_manual_steps(
    cx: &mut TestAppContext,
) {
    let (mut visual, first, second) = split(cx);
    visual.simulate_keystrokes("ctrl-x 2 ctrl-x o");
    redraw(&mut visual);
    let third = visual.update(|window, cx| app_shell(window, cx).read(cx).active_view().clone());
    let views = [third, first, second];
    let mut expected = "abcdef".to_owned();
    for target in &views {
        visual.simulate_keystrokes("alt-< ctrl-g");
        redraw(&mut visual);
        let mut input = handler(target, &visual);
        visual.update(|window, cx| {
            assert!(target.focus_handle(cx).is_focused(window));
            input.replace_and_mark_text_in_range(None, "ni", None, window, cx);
        });
        redraw(&mut visual);
        let mut input = handler(target, &visual);
        visual.update(|window, cx| {
            let selection = input.selected_text_range(false, window, cx).unwrap().range;
            let caret = input
                .bounds_for_range(selection.end..selection.end, window, cx)
                .unwrap();
            assert!(
                input
                    .element_bounds(window, cx)
                    .unwrap()
                    .contains(&caret.origin)
            );
            for view in &views {
                assert_eq!(view.read(cx).buffer().text().to_string(), expected);
                assert_eq!(
                    view.read(cx).display_snapshot().marked_range().is_some(),
                    view == target
                );
            }
            input.replace_and_mark_text_in_range(None, "", None, window, cx);
            assert!(input.marked_text_range(window, cx).is_none());
            input.replace_and_mark_text_in_range(None, "ni", None, window, cx);
            input.replace_text_in_range(None, "你", window, cx);
        });
        expected.insert(0, '你');
        redraw(&mut visual);
        visual.read(|cx| {
            for view in &views {
                assert_eq!(view.read(cx).buffer().text().to_string(), expected);
                assert!(view.read(cx).layout_ready());
            }
        });
        visual.simulate_keystrokes("ctrl-x o");
        redraw(&mut visual);
    }
    let (shell, origin, original) = visual.update(|window, cx| {
        let shell = app_shell(window, cx);
        let origin = shell.read(cx).active_view().clone();
        let view = origin.read(cx);
        let original = (
            view.buffer().point(),
            view.buffer().mark(),
            view.viewport_layout().unwrap().scroll(),
        );
        (shell, origin, original)
    });
    visual.simulate_keystrokes("ctrl-x b");
    redraw(&mut visual);
    let editor = visual.read(|cx| {
        shell
            .read(cx)
            .minibuffer()
            .unwrap()
            .read(cx)
            .editor()
            .clone()
    });
    let mut input = handler(&editor, &visual);
    visual.update(|window, cx| input.replace_and_mark_text_in_range(None, "ni", None, window, cx));
    redraw(&mut visual);
    let mut input = handler(&editor, &visual);
    visual.update(|window, cx| {
        assert!(input.marked_text_range(window, cx).is_some());
        assert_eq!(
            shell.read(cx).minibuffer().unwrap().read(cx).contents(cx),
            ""
        );
        let selection = input.selected_text_range(false, window, cx).unwrap().range;
        let caret = input
            .bounds_for_range(selection.end..selection.end, window, cx)
            .unwrap();
        assert!(
            input
                .element_bounds(window, cx)
                .unwrap()
                .contains(&caret.origin)
        );
        for view in &views {
            assert!(view.read(cx).display_snapshot().marked_range().is_none());
            assert_eq!(view.read(cx).buffer().text().to_string(), expected);
        }
        input.replace_and_mark_text_in_range(None, "", None, window, cx);
        assert!(input.marked_text_range(window, cx).is_none());
        assert_eq!(
            shell.read(cx).minibuffer().unwrap().read(cx).contents(cx),
            ""
        );
        input.replace_and_mark_text_in_range(None, "ni", None, window, cx);
        assert!(input.marked_text_range(window, cx).is_some());
        assert_eq!(
            shell.read(cx).minibuffer().unwrap().read(cx).contents(cx),
            ""
        );
        input.replace_text_in_range(None, "你", window, cx);
    });
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(
            shell.read(cx).minibuffer().unwrap().read(cx).contents(cx),
            "你"
        )
    });
    visual.simulate_keystrokes("ctrl-g");
    redraw(&mut visual);
    visual.update(|window, cx| {
        assert_eq!(shell.read(cx).active_view(), &origin);
        assert!(origin.focus_handle(cx).is_focused(window));
        let view = origin.read(cx);
        assert_eq!(
            (
                view.buffer().point(),
                view.buffer().mark(),
                view.viewport_layout().unwrap().scroll()
            ),
            original
        );
        input.replace_text_in_range(None, "late", window, cx);
        for view in &views {
            assert_eq!(view.read(cx).buffer().text().to_string(), expected);
        }
    });
}

#[gpui::test]
fn closing_a_retained_view_revokes_its_platform_callbacks(cx: &mut TestAppContext) {
    let (mut visual, first, second) = split(cx);
    let mut input = handler(&second, &visual);
    visual.simulate_keystrokes("ctrl-x 0");
    redraw(&mut visual);
    visual.update(|window, cx| {
        assert!(first.focus_handle(cx).is_focused(window));
        input.replace_text_in_range(None, "late", window, cx);
        input.replace_and_mark_text_in_range(None, "late", None, window, cx);
        assert!(!input.accepts_text_input(window, cx));
        assert!(!second.read(cx).accepting_input);
        assert_eq!(first.read(cx).buffer().text().to_string(), "abcdef");
    });
}
