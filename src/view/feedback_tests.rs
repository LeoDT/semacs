use super::BufferView;
use crate::view::input_tests::{app_shell, open, redraw};
use gpui_kit as gpui;
use gpui_kit::{Entity, TestAppContext};
use std::sync::Arc;

#[gpui::test]
fn annotation_wraps_without_changing_input_geometry_and_reuses_stable_shapes(
    cx: &mut TestAppContext,
) {
    let (mut visual, _) = open("body", cx);
    let editor: Entity<BufferView> = visual.update(|window, cx| {
        app_shell(window, cx).update(cx, |shell, cx| {
            shell
                .mount_minibuffer("Input: ", "你🙂", window, cx)
                .unwrap()
                .read(cx)
                .editor()
                .clone()
        })
    });
    redraw(&mut visual);
    let before = visual.read(|cx| editor.read(cx).display_snapshot().id());
    editor.update(&mut visual, |view, cx| {
        view.set_hint("details ".repeat(80), cx)
    });
    redraw(&mut visual);
    visual.update(|window, cx| {
        let view = editor.read(cx);
        let first = view.annotation_lines(window, cx);
        let second = view.annotation_lines(window, cx);
        assert!(Arc::ptr_eq(&first, &second));
        assert!(first.len() > 1);
        assert!(first[0].1.text.starts_with(" ["));
        assert_eq!(view.display_snapshot().id(), before);
        assert_eq!(view.buffer().text().to_string(), "Input: 你🙂");
        assert!(view.viewport_layout().is_some());
        assert!(view.minibuffer_height(window, cx) <= window.viewport_size().height * 0.25);
    });
    visual.run_until_parked();
    visual
        .executor()
        .advance_clock(std::time::Duration::from_secs(2));
    redraw(&mut visual);
    visual.read(|cx| assert_eq!(editor.read(cx).message(), None));
}
