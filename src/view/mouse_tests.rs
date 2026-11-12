use gpui_kit as gpui;
use gpui_kit::{
    Entity, Focusable, InputHandler, MouseButton, MouseExitEvent, MouseUpEvent, Point,
    TestAppContext, VisualTestContext, point, px,
};

use super::BufferView;
use super::input_tests::{handler, open, open_buffer, redraw};
use crate::buffer::Buffer;
use crate::text::ByteOffset;

fn caret(
    visual: &mut VisualTestContext,
    view: &Entity<BufferView>,
    offset: usize,
) -> Point<gpui_kit::Pixels> {
    let bounds = visual.read(|cx| {
        let view = view.read(cx);
        view.viewport_layout()
            .unwrap()
            .bounds_for_range(
                ByteOffset(offset)..ByteOffset(offset),
                view.wrap_index().unwrap(),
            )
            .unwrap()
    });
    point(bounds.left(), bounds.top() + px(5.))
}

#[gpui::test]
fn a_plain_click_sets_point_and_mark_and_leaves_no_active_region(cx: &mut TestAppContext) {
    let (mut visual, editor) = open("abcdef", cx);
    let at = caret(&mut visual, &editor, 3);
    visual.simulate_click(at, Default::default());
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.buffer().point(), ByteOffset(3));
        assert_eq!(view.buffer().mark(), Some(ByteOffset(3)));
        assert!(!view.buffer().mark_active());
        assert_eq!(view.buffer().region(), None);
        assert!(!view.buffer().is_modified());
        assert!(view.region_bounds().is_empty());
        assert!(view.target.kill_ring().latest().is_none());
        assert_eq!(view.message(), Some("Mark set"));
    });
    visual.simulate_keystrokes("ctrl-/");
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.buffer().text().to_string(), "abcdef");
        assert_eq!(view.buffer().point(), ByteOffset(3));
        assert!(!view.buffer().is_modified());
    });
}

#[gpui::test]
fn dragging_keeps_the_press_mark_and_an_active_region_until_the_next_command(
    cx: &mut TestAppContext,
) {
    let (mut visual, editor) = open("abcdef", cx);
    let press = caret(&mut visual, &editor, 4);
    visual.simulate_mouse_down(press, MouseButton::Left, Default::default());
    redraw(&mut visual);
    let target = caret(&mut visual, &editor, 1);
    visual.simulate_mouse_move(target, MouseButton::Left, Default::default());
    redraw(&mut visual);
    visual.update(|window, cx| {
        let view = editor.read(cx);
        assert_eq!(view.buffer().point(), ByteOffset(1));
        assert_eq!(view.buffer().mark(), Some(ByteOffset(4)));
        assert!(view.buffer().mark_active());
        assert_eq!(view.buffer().region(), Some(ByteOffset(1)..ByteOffset(4)));
        assert!(!view.buffer().is_modified());
        assert!(!view.region_bounds().is_empty());
        assert!(view.focus_handle(cx).is_focused(window));
    });
    visual.simulate_mouse_up(target, MouseButton::Left, Default::default());
    redraw(&mut visual);
    visual.read(|cx| {
        let buffer = editor.read(cx).buffer();
        assert_eq!(buffer.point(), ByteOffset(1));
        assert_eq!(buffer.mark(), Some(ByteOffset(4)));
        assert!(buffer.mark_active());
    });
}

#[gpui::test]
fn dragging_forward_selects_through_the_pointer_offset(cx: &mut TestAppContext) {
    let (mut visual, editor) = open("abcdef", cx);
    let press = caret(&mut visual, &editor, 1);
    visual.simulate_mouse_down(press, MouseButton::Left, Default::default());
    redraw(&mut visual);
    let target = caret(&mut visual, &editor, 5);
    visual.simulate_mouse_move(target, MouseButton::Left, Default::default());
    redraw(&mut visual);
    visual.simulate_mouse_up(target, MouseButton::Left, Default::default());
    redraw(&mut visual);
    visual.read(|cx| {
        let buffer = editor.read(cx).buffer();
        assert_eq!(buffer.point(), ByteOffset(5));
        assert_eq!(buffer.mark(), Some(ByteOffset(1)));
        assert!(buffer.mark_active());
        assert_eq!(buffer.region(), Some(ByteOffset(1)..ByteOffset(5)));
    });
}

#[gpui::test]
fn a_press_released_on_the_same_offset_is_a_click(cx: &mut TestAppContext) {
    let (mut visual, editor) = open("abcdef", cx);
    let press = caret(&mut visual, &editor, 2);
    visual.simulate_mouse_down(press, MouseButton::Left, Default::default());
    redraw(&mut visual);
    visual.simulate_mouse_up(press, MouseButton::Left, Default::default());
    redraw(&mut visual);
    visual.read(|cx| {
        let buffer = editor.read(cx).buffer();
        assert_eq!(buffer.point(), ByteOffset(2));
        assert_eq!(buffer.mark(), Some(ByteOffset(2)));
        assert!(!buffer.mark_active());
    });
}

#[gpui::test]
fn readonly_text_follows_pointer_but_is_not_modified(cx: &mut TestAppContext) {
    let (mut visual, editor) = open("abcdef", cx);
    editor.update(&mut visual, |view, cx| view.set_read_only(true, cx));
    redraw(&mut visual);
    let press = caret(&mut visual, &editor, 2);
    visual.simulate_mouse_down(press, MouseButton::Left, Default::default());
    redraw(&mut visual);
    let target = caret(&mut visual, &editor, 5);
    visual.simulate_mouse_move(target, MouseButton::Left, Default::default());
    redraw(&mut visual);
    visual.simulate_mouse_up(target, MouseButton::Left, Default::default());
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.buffer().point(), ByteOffset(5));
        assert_eq!(view.buffer().mark(), Some(ByteOffset(2)));
        assert!(view.buffer().mark_active());
        assert_eq!(view.buffer().text().to_string(), "abcdef");
        assert!(!view.buffer().is_modified());
    });
}

#[gpui::test]
fn composition_rejects_the_press_without_moving_point_or_mark(cx: &mut TestAppContext) {
    let (mut visual, editor) = open("abcdef", cx);
    let mut input = handler(&editor, &visual);
    visual.update(|window, cx| input.replace_and_mark_text_in_range(None, "ni", None, window, cx));
    redraw(&mut visual);
    let at = caret(&mut visual, &editor, 4);
    visual.simulate_mouse_down(at, MouseButton::Left, Default::default());
    redraw(&mut visual);
    visual.update(|window, cx| {
        let view = editor.read(cx);
        assert_eq!(
            view.message(),
            Some("Input method composition is active; finish it first")
        );
        assert_eq!(view.buffer().point(), ByteOffset(0));
        assert_eq!(view.buffer().mark(), None);
        assert!(!view.buffer().mark_active());
        let mut input = input;
        input.unmark_text(window, cx);
    });
}

fn split(cx: &mut TestAppContext) -> (VisualTestContext, Entity<BufferView>, Entity<BufferView>) {
    let (mut visual, first) = open("abcdef\n", cx);
    visual.simulate_resize(gpui_kit::size(px(960.), px(640.)));
    redraw(&mut visual);
    visual.simulate_keystrokes("ctrl-x 3 ctrl-x o");
    redraw(&mut visual);
    let second = visual.update(|window, cx| {
        super::input_tests::app_shell(window, cx)
            .read(cx)
            .active_view()
            .clone()
    });
    (visual, first, second)
}

fn below_bottom(
    visual: &mut VisualTestContext,
    view: &Entity<BufferView>,
    distance: gpui_kit::Pixels,
) -> (
    Point<gpui_kit::Pixels>,
    Point<gpui_kit::Pixels>,
    gpui_kit::Pixels,
) {
    visual.read(|cx| {
        let view = view.read(cx);
        let bounds = view.viewport_layout().unwrap().bounds();
        let caret = view
            .viewport_layout()
            .unwrap()
            .bounds_for_range(ByteOffset(0)..ByteOffset(0), view.wrap_index().unwrap())
            .unwrap();
        let press = point(caret.left(), caret.top() + px(2.));
        (
            press,
            point(caret.left(), bounds.bottom() + distance),
            view.settings.line_height,
        )
    })
}

#[gpui::test]
fn dragging_below_the_text_autoscrolls_and_keeps_the_region_until_the_pointer_returns(
    cx: &mut TestAppContext,
) {
    let (mut visual, editor) = open(&"line\n".repeat(200), cx);
    let (press, below, line_height) = below_bottom(&mut visual, &editor, px(40.));
    let rows = (40. / f32::from(line_height)).ceil();
    visual.simulate_mouse_down(press, MouseButton::Left, Default::default());
    redraw(&mut visual);
    visual.simulate_mouse_move(below, MouseButton::Left, Default::default());
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.scroll.y, line_height * rows);
        assert_eq!(view.pointer_drag.as_ref().unwrap().exterior, Some(below));
        assert!(view.buffer().point() > ByteOffset(0));
        assert_eq!(
            view.buffer().region(),
            Some(ByteOffset(0)..view.buffer().point())
        );
        assert!(!view.region_bounds().is_empty());
        assert!(!view.buffer().is_modified());
    });
    visual
        .executor()
        .advance_clock(std::time::Duration::from_millis(250));
    redraw(&mut visual);
    visual.read(|cx| {
        // Each repeat keeps scrolling by the pointer's distance, like
        // `mouse-scroll-subr` reusing its first jump count.
        let view = editor.read(cx);
        assert_eq!(view.scroll.y, line_height * rows * 2.);
        assert!(view.buffer().point() > ByteOffset(0));
    });
    visual
        .executor()
        .advance_clock(std::time::Duration::from_secs(1));
    redraw(&mut visual);
    let scrolled = visual.read(|cx| editor.read(cx).scroll.y);
    let inside = visual.read(|cx| {
        let bounds = editor.read(cx).viewport_layout().unwrap().bounds();
        point(bounds.left() + px(2.), bounds.top() + px(5.))
    });
    visual.simulate_mouse_move(inside, MouseButton::Left, Default::default());
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.pointer_drag.as_ref().unwrap().exterior, None);
        assert_eq!(
            view.buffer().region(),
            Some(ByteOffset(0)..view.buffer().point())
        );
    });
    visual
        .executor()
        .advance_clock(std::time::Duration::from_secs(1));
    redraw(&mut visual);
    visual.read(|cx| assert_eq!(editor.read(cx).scroll.y, scrolled));
}

#[gpui::test]
fn a_longer_overshoot_scrolls_by_more_rows_at_once(cx: &mut TestAppContext) {
    let (mut visual, editor) = open(&"line\n".repeat(200), cx);
    let (press, below, line_height) = below_bottom(&mut visual, &editor, px(100.));
    let rows = (100. / f32::from(line_height)).ceil();
    assert!(rows >= 3.);
    visual.simulate_mouse_down(press, MouseButton::Left, Default::default());
    redraw(&mut visual);
    visual.simulate_mouse_move(below, MouseButton::Left, Default::default());
    redraw(&mut visual);
    visual.read(|cx| assert_eq!(editor.read(cx).scroll.y, line_height * rows));
}

#[gpui::test]
fn dragging_above_the_text_scrolls_up(cx: &mut TestAppContext) {
    let (mut visual, editor) = open(&"line\n".repeat(200), cx);
    visual.update(|window, cx| {
        editor.update(cx, |view, cx| {
            view.scroll_by(point(px(0.), px(240.)), cx);
            let _ = window;
        })
    });
    redraw(&mut visual);
    let (press, above, line_height) = visual.read(|cx| {
        let view = editor.read(cx);
        let layout = view.viewport_layout().unwrap();
        let index = view.wrap_index().unwrap();
        let first =
            (f32::from(view.scroll.y) / f32::from(view.settings.line_height)).floor() as usize;
        let start = index.row(first).unwrap().start;
        let caret = layout.bounds_for_range(start..start, index).unwrap();
        (
            point(caret.left(), layout.bounds().bottom() - px(4.)),
            point(caret.left(), layout.bounds().top() - px(30.)),
            view.settings.line_height,
        )
    });
    visual.simulate_mouse_down(press, MouseButton::Left, Default::default());
    redraw(&mut visual);
    let start = visual.read(|cx| editor.read(cx).scroll.y);
    visual.simulate_mouse_move(above, MouseButton::Left, Default::default());
    redraw(&mut visual);
    let rows = (30. / f32::from(line_height)).ceil();
    visual.read(|cx| assert_eq!(editor.read(cx).scroll.y, start - line_height * rows));
}

#[gpui::test]
fn moving_into_the_neighbouring_pane_stops_the_autoscroll(cx: &mut TestAppContext) {
    let (mut visual, first, second) = split(cx);
    let press = caret(&mut visual, &first, 1);
    let other = visual.read(|cx| {
        let bounds = second.read(cx).viewport_layout().unwrap().bounds();
        point(bounds.left() + px(4.), bounds.top() + px(4.))
    });
    visual.simulate_mouse_down(press, MouseButton::Left, Default::default());
    redraw(&mut visual);
    visual.simulate_mouse_move(other, MouseButton::Left, Default::default());
    redraw(&mut visual);
    visual.read(|cx| {
        let view = first.read(cx);
        assert_eq!(view.scroll.y, px(0.));
        assert_eq!(view.pointer_drag.as_ref().unwrap().exterior, None);
    });
    visual
        .executor()
        .advance_clock(std::time::Duration::from_secs(1));
    redraw(&mut visual);
    visual.read(|cx| assert_eq!(first.read(cx).scroll.y, px(0.)));
}

#[gpui::test]
fn releasing_outside_the_view_ends_the_drag_and_keeps_the_region(cx: &mut TestAppContext) {
    let (mut visual, editor) = open(&"line\n".repeat(200), cx);
    let (press, below, _) = below_bottom(&mut visual, &editor, px(30.));
    visual.simulate_mouse_down(press, MouseButton::Left, Default::default());
    redraw(&mut visual);
    visual.simulate_mouse_move(below, MouseButton::Left, Default::default());
    redraw(&mut visual);
    visual.simulate_mouse_up(below, MouseButton::Left, Default::default());
    redraw(&mut visual);
    let scrolled = visual.read(|cx| {
        let view = editor.read(cx);
        assert!(view.pointer_drag.is_none());
        assert_eq!(view.buffer().mark(), Some(ByteOffset(0)));
        assert!(view.buffer().mark_active());
        view.scroll.y
    });
    visual
        .executor()
        .advance_clock(std::time::Duration::from_secs(1));
    redraw(&mut visual);
    visual.read(|cx| assert_eq!(editor.read(cx).scroll.y, scrolled));
}

#[gpui::test]
fn ending_the_input_lifecycle_cancels_the_autoscroll(cx: &mut TestAppContext) {
    let (mut visual, editor) = open(&"line\n".repeat(200), cx);
    let (press, below, _) = below_bottom(&mut visual, &editor, px(30.));
    visual.simulate_mouse_down(press, MouseButton::Left, Default::default());
    redraw(&mut visual);
    visual.simulate_mouse_move(below, MouseButton::Left, Default::default());
    redraw(&mut visual);
    let scrolled = visual.update(|_, cx| {
        editor.update(cx, |view, cx| {
            view.end_input(cx);
            view.scroll.y
        })
    });
    visual.read(|cx| assert!(editor.read(cx).pointer_drag.is_none()));
    visual
        .executor()
        .advance_clock(std::time::Duration::from_secs(1));
    redraw(&mut visual);
    visual.read(|cx| assert_eq!(editor.read(cx).scroll.y, scrolled));
}

#[gpui::test]
fn releasing_without_a_pressed_button_ends_a_drag_whose_release_was_lost(cx: &mut TestAppContext) {
    let (mut visual, editor) = open("abcdef", cx);
    let press = caret(&mut visual, &editor, 1);
    visual.simulate_mouse_down(press, MouseButton::Left, Default::default());
    redraw(&mut visual);
    let target = caret(&mut visual, &editor, 4);
    visual.simulate_mouse_move(target, MouseButton::Left, Default::default());
    redraw(&mut visual);
    visual.simulate_mouse_move(target, None, Default::default());
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert!(view.pointer_drag.is_none());
        assert_eq!(view.buffer().point(), ByteOffset(4));
        assert_eq!(view.buffer().mark(), Some(ByteOffset(1)));
        assert!(view.buffer().mark_active());
    });
}

#[gpui::test]
fn leaving_the_window_keeps_scrolling_until_the_press_ends(cx: &mut TestAppContext) {
    let (mut visual, editor) = open(&"line\n".repeat(200), cx);
    let (press, below, _) = below_bottom(&mut visual, &editor, px(30.));
    visual.simulate_mouse_down(press, MouseButton::Left, Default::default());
    redraw(&mut visual);
    visual.simulate_mouse_move(below, MouseButton::Left, Default::default());
    redraw(&mut visual);
    let scrolled = visual.read(|cx| editor.read(cx).scroll.y);
    assert!(scrolled > px(0.));
    visual.simulate_event(MouseExitEvent {
        position: below,
        pressed_button: Some(MouseButton::Left),
        modifiers: Default::default(),
    });
    visual
        .executor()
        .advance_clock(std::time::Duration::from_millis(250));
    redraw(&mut visual);
    visual.read(|cx| {
        assert!(editor.read(cx).scroll.y > scrolled);
        assert!(editor.read(cx).pointer_drag.is_some());
    });
    visual.simulate_event(MouseUpEvent {
        position: below,
        button: MouseButton::Left,
        modifiers: Default::default(),
        click_count: 1,
    });
    redraw(&mut visual);
    visual.read(|cx| {
        assert!(editor.read(cx).pointer_drag.is_none());
        assert!(editor.read(cx).buffer().mark_active());
    });
}

#[gpui::test]
fn window_deactivation_ends_the_drag_and_stops_the_autoscroll(cx: &mut TestAppContext) {
    let (mut visual, editor) = open(&"line\n".repeat(200), cx);
    let (press, below, _) = below_bottom(&mut visual, &editor, px(30.));
    visual.simulate_mouse_down(press, MouseButton::Left, Default::default());
    redraw(&mut visual);
    visual.simulate_mouse_move(below, MouseButton::Left, Default::default());
    redraw(&mut visual);
    visual.deactivate_window();
    redraw(&mut visual);
    let scrolled = visual.read(|cx| {
        let view = editor.read(cx);
        assert!(view.pointer_drag.is_none());
        assert!(view.buffer().mark_active());
        view.scroll.y
    });
    visual
        .executor()
        .advance_clock(std::time::Duration::from_secs(1));
    redraw(&mut visual);
    visual.read(|cx| assert_eq!(editor.read(cx).scroll.y, scrolled));
}

#[gpui::test]
fn a_generated_readonly_buffer_follows_pointer_without_editing(cx: &mut TestAppContext) {
    let mut buffer = Buffer::messages();
    buffer.update_message_log(
        ByteOffset(0)..ByteOffset(0),
        "first message\nsecond message\n",
    );
    let (mut visual, editor) = open_buffer(buffer, cx);
    let content = visual.read(|cx| editor.read(cx).buffer().text().to_string());
    let press = caret(&mut visual, &editor, 3);
    visual.simulate_mouse_down(press, MouseButton::Left, Default::default());
    redraw(&mut visual);
    let target = caret(&mut visual, &editor, 20);
    visual.simulate_mouse_move(target, MouseButton::Left, Default::default());
    redraw(&mut visual);
    visual.simulate_mouse_up(target, MouseButton::Left, Default::default());
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.buffer().point(), ByteOffset(20));
        assert_eq!(view.buffer().mark(), Some(ByteOffset(3)));
        assert!(view.buffer().mark_active());
        assert_eq!(view.buffer().text().to_string(), content);
        assert!(!view.buffer().is_modified());
    });
}
