use std::time::{Duration, Instant};

use super::{WheelState, WheelStep, vertical_motion};
use crate::buffer::Buffer;
use crate::display::{FontMeasure, WrapJob};
use crate::editing::RouteError;
use crate::text::ByteOffset;
use crate::view::BufferView;
use crate::view::input_tests::{handler, open, open_buffer, redraw};
use gpui_kit::{
    Entity, InputHandler, MouseButton, ScrollDelta, ScrollEvent, TestAppContext, VisualTestContext,
    font, point, px, size,
};

fn event(delta: ScrollDelta) -> ScrollEvent {
    ScrollEvent {
        position: point(px(20.), px(20.)),
        delta,
        ..Default::default()
    }
}

fn lines(y: f32) -> ScrollEvent {
    event(ScrollDelta::Lines(point(0., -y)))
}

#[test]
fn progressive_speed_uses_repeat_count_and_the_platform_line_amount() {
    let mut state = WheelState::default();
    let start = Instant::now();
    for (i, expected) in [2, 4, 6, 8].into_iter().enumerate() {
        let step = state.step(
            &lines(2.),
            px(24.),
            px(12.),
            start + Duration::from_millis(i as u64 * 100),
        );
        assert_eq!(
            step,
            WheelStep {
                rows: expected,
                columns: 0
            }
        );
    }
    assert_eq!(
        state
            .step(
                &lines(-1.),
                px(24.),
                px(12.),
                start + Duration::from_millis(400)
            )
            .rows,
        -1
    );
    assert_eq!(
        state
            .step(
                &lines(-1.),
                px(24.),
                px(12.),
                start + Duration::from_millis(900)
            )
            .rows,
        -1
    );
    let mut moved = lines(-1.);
    moved.position.x += px(4.);
    assert_eq!(
        state
            .step(&moved, px(24.), px(12.), start + Duration::from_millis(901))
            .rows,
        -1
    );
    state.reset();
    assert_eq!(
        state
            .step(&moved, px(24.), px(12.), start + Duration::from_millis(902))
            .rows,
        -1
    );
}

#[test]
fn fractional_lines_and_pixels_accumulate_before_acceleration() {
    let mut state = WheelState::default();
    let now = Instant::now();
    assert_eq!(
        state.step(&lines(0.25), px(24.), px(12.), now),
        WheelStep::default()
    );
    let pixel = event(ScrollDelta::Pixels(point(px(0.), px(-6.))));
    for _ in 0..2 {
        assert_eq!(
            state.step(&pixel, px(24.), px(12.), now),
            WheelStep::default()
        );
    }
    assert_eq!(state.step(&pixel, px(24.), px(12.), now).rows, 1);
    for _ in 0..3 {
        assert_eq!(
            state.step(&pixel, px(24.), px(12.), now),
            WheelStep::default()
        );
    }
    assert_eq!(state.step(&pixel, px(24.), px(12.), now).rows, 2);
}

#[test]
fn horizontal_input_uses_columns_without_a_second_shift_transform_or_acceleration() {
    let mut state = WheelState::default();
    let now = Instant::now();
    let mut shifted = event(ScrollDelta::Lines(point(-1., 0.)));
    shifted.modifiers.shift = true;
    for _ in 0..3 {
        assert_eq!(
            state.step(&shifted, px(24.), px(12.), now),
            WheelStep {
                rows: 0,
                columns: 1
            }
        );
    }
    let pixels = event(ScrollDelta::Pixels(point(px(-6.), px(0.))));
    assert_eq!(
        state.step(&pixels, px(24.), px(12.), now),
        WheelStep::default()
    );
    assert_eq!(
        state.step(&pixels, px(24.), px(12.), now),
        WheelStep {
            rows: 0,
            columns: 1
        }
    );
    assert_eq!(state.step(&lines(1.), px(24.), px(12.), now).rows, 1);
}

#[gpui_kit::test]
fn window_scroll_preserves_point_until_an_edge_pushes_it_to_a_row_start(cx: &mut TestAppContext) {
    let (visual, editor) = open(&"abcdef\n".repeat(100), cx);
    visual.read(|cx| {
        let index = editor.read(cx).wrap_index().unwrap();
        let run = |point, first: f32, rows| {
            vertical_motion(
                index,
                ByteOffset(point),
                px(first * 24.),
                px(120.),
                px(24.),
                rows,
            )
            .unwrap()
        };
        let kept = run(3 * 7 + 4, 0., 2);
        assert_eq!(kept.scroll, px(48.));
        assert_eq!(kept.point, ByteOffset(3 * 7 + 4));
        let pushed = run(3 * 7 + 4, 0., 4);
        assert_eq!(pushed.scroll, px(96.));
        assert_eq!(pushed.point, ByteOffset(4 * 7));
        let kept = run(12 * 7 + 4, 10., -1);
        assert_eq!(kept.point, ByteOffset(12 * 7 + 4));
        let pushed = run(14 * 7 + 4, 10., -1);
        assert_eq!(pushed.scroll, px(9. * 24.));
        assert_eq!(pushed.point, ByteOffset(13 * 7));
        let partial = vertical_motion(
            index,
            ByteOffset(14 * 7 + 4),
            px(10. * 24.),
            px(5.5 * 24.),
            px(24.),
            -1,
        )
        .unwrap();
        assert_eq!(partial.point, ByteOffset(13 * 7));
    });
}

#[gpui_kit::test]
fn a_clipped_bottom_point_is_handled_before_the_wheel_amount(cx: &mut TestAppContext) {
    let (visual, editor) = open(&"abcdef\n".repeat(100), cx);
    visual.read(|cx| {
        let index = editor.read(cx).wrap_index().unwrap();
        for rows in [1, 1000] {
            let motion = vertical_motion(
                index,
                ByteOffset(5 * 7 + 4),
                px(0.),
                px(5.5 * 24.),
                px(24.),
                rows,
            )
            .unwrap();
            assert_eq!(motion.scroll, px(5. * 24.));
            assert_eq!(motion.point, ByteOffset(5 * 7 + 4));
            assert_eq!(motion.boundary, None);
            assert_eq!(motion.redisplay_point, None);
        }
        let motion = vertical_motion(index, ByteOffset(4), px(0.), px(12.), px(24.), 1000).unwrap();
        assert_eq!(motion.scroll, px(12.));
        assert_eq!(motion.point, ByteOffset(4));
        let restored =
            vertical_motion(index, motion.point, motion.scroll, px(12.), px(24.), -1000).unwrap();
        assert_eq!(restored.scroll, px(0.));
        assert_eq!(restored.point, ByteOffset(4));
    });
}

#[gpui_kit::test]
fn a_top_clipped_point_consumes_the_vscroll_before_applying_another_step(cx: &mut TestAppContext) {
    let (visual, editor) = open(&"abcdef\n".repeat(100), cx);
    visual.read(|cx| {
        let index = editor.read(cx).wrap_index().unwrap();
        for rows in [1, 1000] {
            let motion =
                vertical_motion(index, ByteOffset(4), px(12.), px(5.5 * 24.), px(24.), rows)
                    .unwrap();
            assert_eq!(motion.scroll, px(24.));
            assert_eq!(motion.point, ByteOffset(4));
            assert_eq!(motion.redisplay_point, Some(ByteOffset(3 * 7)));
            assert_eq!(motion.boundary, None);
        }
    });
    let (visual, editor) = open("abc", cx);
    visual.read(|cx| {
        let index = editor.read(cx).wrap_index().unwrap();
        let motion = vertical_motion(index, ByteOffset(2), px(12.), px(120.), px(24.), 1).unwrap();
        assert_eq!(motion.redisplay_point, Some(ByteOffset(3)));
        assert_eq!(motion.boundary, None);
    });
}

#[gpui_kit::test]
fn clipped_wrapped_rows_follow_the_logical_line_start_and_forced_redisplay(
    cx: &mut TestAppContext,
) {
    let (visual, editor) = open(&"中🙂 ".repeat(1000), cx);
    visual.read(|cx| {
        let index = editor.read(cx).wrap_index().unwrap();
        let point = index.row(5).unwrap().start;
        let motion = vertical_motion(index, point, px(0.), px(5.5 * 24.), px(24.), 1000).unwrap();
        assert_eq!(motion.point, point);
        assert_eq!(motion.scroll, px(2.75 * 24.));
        assert_eq!(motion.redisplay_point, None);
        let point = index.row(55).unwrap().start;
        let motion =
            vertical_motion(index, point, px(50. * 24.), px(5.5 * 24.), px(24.), 1000).unwrap();
        assert_eq!(motion.scroll, px(0.));
        assert_eq!(motion.point, point);
        assert_eq!(motion.redisplay_point, Some(index.row(2).unwrap().start));
    });
}

#[gpui_kit::test]
fn bringing_a_clipped_point_to_the_top_keeps_its_temporary_region(cx: &mut TestAppContext) {
    let (mut visual, editor) = open(&"abcdef\n".repeat(100), cx);
    visual.update(|window, cx| {
        editor.update(cx, |view, cx| {
            view.target
                .press_pointer(view.target.token(), ByteOffset(0))
                .unwrap();
            view.target
                .drag_pointer(view.target.token(), ByteOffset(5 * 7 + 4))
                .unwrap();
            view.target.release_pointer(false);
            view.content_bounds.as_mut().unwrap().size.height = view.settings.line_height * 5.5;
            view.scroll_wheel(&lines(1000.), window, cx);
            assert_eq!(view.scroll.y, view.settings.line_height * 5.);
            assert_eq!(view.buffer().point(), ByteOffset(5 * 7 + 4));
            assert!(view.buffer().mark_active());
            assert_eq!(view.message(), None);
        })
    });
}

#[gpui_kit::test]
fn an_offscreen_point_is_recentered_before_scrolling(cx: &mut TestAppContext) {
    let (visual, editor) = open(&"abcdef\n".repeat(100), cx);
    visual.read(|cx| {
        let index = editor.read(cx).wrap_index().unwrap();
        for (rows, first) in [(1, 9.), (-1, 7.)] {
            let motion = vertical_motion(
                index,
                ByteOffset(10 * 7 + 4),
                px(50. * 24.),
                px(120.),
                px(24.),
                rows,
            )
            .unwrap();
            assert_eq!(motion.point, ByteOffset(10 * 7 + 4));
            assert_eq!(motion.scroll, px(first * 24.));
        }
    });
}

#[gpui_kit::test]
fn wheel_limits_follow_mwheel_whole_screen_retry_instead_of_clamping(cx: &mut TestAppContext) {
    let (visual, editor) = open(&"abcdef\n".repeat(100), cx);
    visual.read(|cx| {
        let index = editor.read(cx).wrap_index().unwrap();
        let motion =
            vertical_motion(index, ByteOffset(0), px(0.), px(7. * 24.), px(24.), 1000).unwrap();
        assert_eq!(motion.scroll, px(95. * 24.));
        assert_eq!(motion.point, ByteOffset(95 * 7));
        assert_eq!(motion.boundary, Some(RouteError::EndOfBuffer));
        let motion = vertical_motion(
            index,
            ByteOffset(99 * 7 + 4),
            px(99. * 24.),
            px(7. * 24.),
            px(24.),
            1,
        )
        .unwrap();
        assert_eq!(motion.scroll, px(99. * 24.));
        assert_eq!(motion.point, ByteOffset(99 * 7 + 4));
        assert_eq!(motion.boundary, Some(RouteError::EndOfBuffer));
        let motion = vertical_motion(
            index,
            ByteOffset(5 * 7 + 4),
            px(5. * 24.),
            px(7. * 24.),
            px(24.),
            -1000,
        )
        .unwrap();
        assert_eq!(motion.scroll, px(0.));
        assert_eq!(motion.point, ByteOffset(5 * 7 + 4));
        assert_eq!(motion.boundary, None);
        let motion =
            vertical_motion(index, ByteOffset(4), px(0.), px(7. * 24.), px(24.), -1).unwrap();
        assert_eq!(motion.boundary, Some(RouteError::BeginningOfBuffer));
        assert_eq!(motion.point, ByteOffset(4));
    });
}

#[gpui_kit::test]
fn empty_buffers_and_unterminated_last_rows_have_distinct_scroll_limits(cx: &mut TestAppContext) {
    for text in ["", "abc", "abc\ndef", "abc\ndef\n"] {
        let (visual, editor) = open(text, cx);
        visual.read(|cx| {
            let index = editor.read(cx).wrap_index().unwrap();
            let motion =
                vertical_motion(index, ByteOffset(0), px(0.), px(120.), px(24.), 1).unwrap();
            if text.contains('\n') {
                assert_eq!(motion.scroll, px(24.));
                assert_eq!(motion.point, ByteOffset(4));
                assert_eq!(motion.boundary, None);
            } else {
                assert_eq!(motion.scroll, px(0.));
                assert_eq!(motion.point, ByteOffset(0));
                assert_eq!(motion.boundary, Some(RouteError::EndOfBuffer));
            }
        });
    }
}

fn send(visual: &mut VisualTestContext, editor: &Entity<BufferView>, delta: ScrollDelta) {
    visual.update(|window, cx| {
        editor.update(cx, |view, cx| {
            let bounds = view.content_bounds.unwrap();
            let mut event = event(delta);
            event.position = bounds.origin + point(px(4.), px(4.));
            view.scroll_wheel(&event, window, cx);
        });
    });
}

#[gpui_kit::test]
fn repeated_events_work_without_redraw_and_do_not_reveal_or_edit(cx: &mut TestAppContext) {
    let (mut visual, editor) = open(&"abcdef\n".repeat(100), cx);
    let mut old = handler(&editor, &visual);
    let before = visual.read(|cx| editor.read(cx).display_snapshot().id());
    for rows in [1., 1., 1.] {
        send(&mut visual, &editor, lines(rows).delta);
    }
    visual.update(|window, cx| old.replace_text_in_range(None, "late", window, cx));
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.scroll.y, view.settings.line_height * 6.);
        assert_eq!(view.buffer().point(), ByteOffset(6 * 7));
        assert_eq!(view.display_snapshot().id(), before);
        assert!(!view.reveal_caret);
        assert!(!view.buffer().is_modified());
        assert_eq!(view.buffer().mark(), None);
        assert!(view.target.kill_ring().latest().is_none());
    });
}

#[gpui_kit::test]
fn pixel_scroll_waits_for_a_full_line_then_moves_point(cx: &mut TestAppContext) {
    let (mut visual, editor) = open(&"abcdef\n".repeat(100), cx);
    let half = visual.read(|cx| editor.read(cx).settings.line_height / 2.);
    send(
        &mut visual,
        &editor,
        ScrollDelta::Pixels(point(px(0.), -half)),
    );
    visual.read(|cx| {
        assert_eq!(editor.read(cx).scroll.y, px(0.));
        assert_eq!(editor.read(cx).buffer().point(), ByteOffset(0));
    });
    send(
        &mut visual,
        &editor,
        ScrollDelta::Pixels(point(px(0.), -half)),
    );
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.scroll.y, view.settings.line_height);
        assert_eq!(view.buffer().point(), ByteOffset(7));
    });
}

#[gpui_kit::test]
fn scrolling_a_nonselected_pane_changes_its_point_without_stealing_focus(cx: &mut TestAppContext) {
    let (mut visual, first) = open(&"abcdef\n".repeat(100), cx);
    visual.simulate_resize(size(px(960.), px(640.)));
    redraw(&mut visual);
    visual.simulate_keystrokes("ctrl-x 3 ctrl-x o");
    redraw(&mut visual);
    let second = visual.update(|window, cx| {
        crate::view::input_tests::app_shell(window, cx)
            .read(cx)
            .active_view()
            .clone()
    });
    second.update(&mut visual, |view, _| {
        view.target
            .press_pointer(view.target.token(), ByteOffset(0))
            .unwrap();
        view.target
            .drag_pointer(view.target.token(), ByteOffset(4))
            .unwrap();
        view.target.release_pointer(false);
    });
    let before = visual.read(|cx| second.read(cx).buffer().point());
    send(&mut visual, &first, lines(1.).delta);
    redraw(&mut visual);
    visual.update(|window, cx| {
        assert_eq!(first.read(cx).buffer().point(), ByteOffset(7));
        assert_eq!(second.read(cx).buffer().point(), before);
        assert!(second.read(cx).buffer().mark_active());
        assert!(second.read(cx).focus.is_focused(window));
        assert!(!first.read(cx).focus.is_focused(window));
        assert!(!first.read(cx).buffer().is_modified());
    });
}

#[gpui_kit::test]
fn scrolling_deactivates_only_pointer_regions_when_point_moves_in_the_selected_view(
    cx: &mut TestAppContext,
) {
    let (mut visual, editor) = open(&"abcdef\n".repeat(100), cx);
    editor.update(&mut visual, |view, _| {
        view.target
            .press_pointer(view.target.token(), ByteOffset(0))
            .unwrap();
        view.target
            .drag_pointer(view.target.token(), ByteOffset(4))
            .unwrap();
        view.target.release_pointer(false);
    });
    send(&mut visual, &editor, ScrollDelta::Lines(point(-1., 0.)));
    visual.read(|cx| assert!(editor.read(cx).buffer().mark_active()));
    send(&mut visual, &editor, lines(1.).delta);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.buffer().point(), ByteOffset(7));
        assert_eq!(view.buffer().mark(), Some(ByteOffset(0)));
        assert!(!view.buffer().mark_active());
    });
    redraw(&mut visual);
    visual.simulate_keystrokes("ctrl-t ctrl-f");
    redraw(&mut visual);
    send(&mut visual, &editor, lines(1.).delta);
    visual.read(|cx| assert!(editor.read(cx).buffer().mark_active()));
}

#[gpui_kit::test]
fn readonly_text_scrolls_but_composition_and_suspended_input_do_not(cx: &mut TestAppContext) {
    let (mut visual, editor) = open(&"abcdef\n".repeat(100), cx);
    editor.update(&mut visual, |view, cx| view.set_read_only(true, cx));
    send(&mut visual, &editor, lines(1.).delta);
    redraw(&mut visual);
    visual.read(|cx| assert_eq!(editor.read(cx).buffer().point(), ByteOffset(7)));
    editor.update(&mut visual, |view, cx| view.set_read_only(false, cx));
    redraw(&mut visual);
    let mut input = handler(&editor, &visual);
    visual.update(|window, cx| input.replace_and_mark_text_in_range(None, "ni", None, window, cx));
    redraw(&mut visual);
    let before = visual.read(|cx| editor.read(cx).scroll);
    send(&mut visual, &editor, lines(1.).delta);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.scroll, before);
        assert_eq!(view.buffer().point(), ByteOffset(7));
        assert!(view.target.input().is_composing());
        assert_eq!(
            view.message(),
            Some("Input method composition is active; finish it first")
        );
    });
    editor.update(&mut visual, |view, cx| view.suspend_input(cx));
    send(&mut visual, &editor, lines(1.).delta);
    visual.read(|cx| assert_eq!(editor.read(cx).scroll, before));
}

#[gpui_kit::test]
fn soft_wrapped_rows_and_folded_sections_use_the_visible_projection(cx: &mut TestAppContext) {
    let (mut visual, editor) = open(&"中🙂 ".repeat(500), cx);
    let next = visual.read(|cx| editor.read(cx).wrap_index().unwrap().row(1).unwrap().start);
    send(&mut visual, &editor, lines(1.).delta);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.buffer().point(), next);
        assert_eq!(view.buffer().text().max_point().row, 0);
        assert!(view.buffer().text().is_char_boundary(next.0));
    });
    let repo = crate::diff::tests::Repository::new();
    repo.write("a", "old\n");
    repo.write("b", "old\n");
    repo.commit();
    repo.write("a", "new\n");
    repo.write("b", "new\n");
    let data = repo.load(crate::diff::DiffKind::Unstaged);
    let mut buffer = Buffer::diff(data.comparison.clone());
    let request = buffer.begin_diff_request().unwrap().1;
    buffer
        .install_diff(request, data.clone(), data.render().unwrap())
        .unwrap();
    let (mut visual, editor) = open_buffer(buffer, cx);
    visual.update(|window, cx| {
        editor.update(cx, |view, cx| {
            let hunk = view
                .buffer()
                .sections()
                .unwrap()
                .nodes()
                .iter()
                .find(|node| node.hunk)
                .unwrap()
                .range
                .start;
            view.show_location(hunk, window, cx);
        });
    });
    visual.simulate_keystrokes("tab");
    redraw(&mut visual);
    visual.update(|window, cx| {
        editor.update(cx, |view, cx| view.show_location(ByteOffset(0), window, cx))
    });
    redraw(&mut visual);
    let (target, expected) = visual.read(|cx| {
        let view = editor.read(cx);
        let index = view.wrap_index().unwrap();
        assert!(index.is_projected());
        assert!(index.row_count() < view.buffer().text().max_point().row as usize + 1);
        let hunk = view
            .buffer()
            .sections()
            .unwrap()
            .nodes()
            .iter()
            .find(|node| node.hunk)
            .unwrap()
            .range
            .start;
        let heading = index.row_for_offset(hunk).unwrap().index;
        let clipped = vertical_motion(
            index,
            hunk,
            view.settings.line_height * (heading as f32 + 0.5),
            view.settings.line_height * 5.5,
            view.settings.line_height,
            1,
        )
        .unwrap();
        assert_eq!(
            clipped.scroll,
            view.settings.line_height * (heading + 1) as f32
        );
        let middle = (heading + 3).min(index.row_count() - 1);
        assert_eq!(
            clipped.redisplay_point,
            Some(index.row(middle).unwrap().start)
        );
        let target = index.row_count() - 2;
        let expected = index.row(target).unwrap().start;
        assert!(view.buffer().text().offset_to_point(expected.0).row as usize > target);
        (target, expected)
    });
    send(&mut visual, &editor, lines(target as f32).delta);
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.buffer().point(), expected);
        assert_eq!(view.scroll.y, view.settings.line_height * target as f32);
    });
}

#[gpui_kit::test]
fn horizontal_events_keep_point_and_use_columns_without_progressive_speed(cx: &mut TestAppContext) {
    let (mut visual, editor) = open(&"abcdef\n".repeat(100), cx);
    let column = editor.update(&mut visual, |view, _| {
        view.metrics.as_mut().unwrap().width(" ")
    });
    visual.read(|cx| assert!(column < editor.read(cx).settings.line_height));
    for _ in 0..3 {
        visual.update(|window, cx| {
            editor.update(cx, |view, cx| {
                let mut shifted = event(ScrollDelta::Lines(point(-1., 0.)));
                shifted.modifiers.shift = true;
                view.scroll_wheel(&shifted, window, cx);
            })
        });
    }
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.scroll.x, column * 3.);
        assert_eq!(view.scroll.y, px(0.));
        assert_eq!(view.buffer().point(), ByteOffset(0));
    });
    send(&mut visual, &editor, ScrollDelta::Lines(point(10., 0.)));
    visual.read(|cx| assert_eq!(editor.read(cx).scroll.x, px(0.)));
}

#[gpui_kit::test]
fn a_mouse_button_resets_progressive_speed(cx: &mut TestAppContext) {
    let (mut visual, editor) = open(&"abcdef\n".repeat(100), cx);
    send(&mut visual, &editor, lines(1.).delta);
    send(&mut visual, &editor, lines(1.).delta);
    redraw(&mut visual);
    let at =
        visual.read(|cx| editor.read(cx).content_bounds.unwrap().origin + point(px(4.), px(4.)));
    visual.simulate_mouse_down(at, MouseButton::Right, Default::default());
    send(&mut visual, &editor, lines(1.).delta);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.scroll.y, view.settings.line_height * 4.);
    });
}

#[gpui_kit::test]
fn a_partial_or_incompatible_index_never_guesses_the_target_row(cx: &mut TestAppContext) {
    let (mut visual, editor) = open(&"abcdef\n".repeat(100), cx);
    editor.update(&mut visual, |view, cx| {
        let mut metrics = FontMeasure::new(cx.text_system().clone(), view.settings.clone());
        view.index = Some(
            WrapJob::new(view.display_snapshot(), view.settings.clone(), None, None).preview(
                &mut metrics,
                2,
                8192,
            ),
        );
        assert!(!view.index.as_ref().unwrap().is_complete());
    });
    send(&mut visual, &editor, lines(20.).delta);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.message(), Some("Text layout is not ready; try again"));
        assert_eq!(view.scroll.y, px(0.));
        assert_eq!(view.buffer().point(), ByteOffset(0));
    });
    redraw(&mut visual);
    visual.update(|window, cx| {
        editor.update(cx, |view, cx| {
            view.set_font(font("Other font"), px(15.), px(30.), cx);
            view.scroll_wheel(&lines(1.), window, cx);
            assert_eq!(view.scroll.y, px(0.));
            assert_eq!(view.buffer().point(), ByteOffset(0));
            assert_eq!(view.message(), Some("Text layout is not ready; try again"));
        })
    });
}
