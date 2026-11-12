use gpui_kit as gpui;
use gpui_kit::{
    Entity, InputHandler, Keystroke, TestAppContext, VisualTestContext, font, point, px,
};

use super::input_tests::{handler, open, press_listener, redraw};
use crate::text::ByteOffset;

#[gpui::test]
fn remaining_motion_bindings_share_the_formal_command_path(cx: &mut TestAppContext) {
    let (mut visual, editor) = open("one_two\nx\nabcdef", cx);
    let id = visual.read(|cx| editor.read(cx).display_snapshot().id());
    visual.simulate_keystrokes("alt-f ctrl-n ctrl-n");
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.buffer().point(), ByteOffset(13));
        assert_eq!(view.message(), None);
    });
    visual.simulate_keystrokes("ctrl-n");
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.buffer().point(), ByteOffset(16));
        assert_eq!(view.message(), Some("End of buffer"));
        assert!(
            view.viewport_layout()
                .unwrap()
                .bounds_for_range(ByteOffset(16)..ByteOffset(16), view.wrap_index().unwrap())
                .is_some()
        );
    });
    visual.simulate_keystrokes("escape < ctrl-t alt-> alt-b ctrl-p");
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.buffer().point(), ByteOffset(8));
        assert_eq!(view.buffer().mark(), Some(ByteOffset(0)));
        assert!(view.buffer().mark_active());
        assert_eq!(view.display_snapshot().id(), id);
        assert!(!view.buffer().is_modified());
    });
}

#[gpui::test]
fn consecutive_pages_work_without_an_intervening_frame_and_revoke_old_callbacks(
    cx: &mut TestAppContext,
) {
    let (mut visual, editor) = open(&"abcdef\n".repeat(100), cx);
    let (step, id) = visual.read(|cx| {
        let view = editor.read(cx);
        let height = f32::from(view.settings.line_height);
        let visible =
            (f32::from(view.content_bounds.unwrap().size.height) / height).floor() as usize;
        (
            visible.saturating_sub(2).max(1),
            view.display_snapshot().id(),
        )
    });
    let mut old = handler(&editor, &visual);
    visual.update(|window, cx| {
        for _ in 0..2 {
            press_listener(&editor, "ctrl-v", window, cx);
            assert!(editor.read(cx).geometry.is_none());
        }
        old.replace_text_in_range(None, "late", window, cx);
        let view = editor.read(cx);
        assert_eq!(view.buffer().point(), ByteOffset(2 * step * 7));
        assert_eq!(view.scroll.y, view.settings.line_height * (2 * step) as f32);
        assert_eq!(view.message(), None);
    });
    redraw(&mut visual);
    visual.simulate_keystrokes("alt-v");
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.scroll.y, view.settings.line_height * step as f32);
        assert_eq!(view.buffer().point(), ByteOffset(2 * step * 7));
        assert_eq!(view.display_snapshot().id(), id);
        assert!(view.layout_ready());
        assert!(!view.buffer().is_modified());
    });
}

#[gpui::test]
fn visual_rows_drive_pages_and_vertical_motion(cx: &mut TestAppContext) {
    let (mut visual, editor) = open(&"中🙂 ".repeat(500), cx);
    visual.simulate_keystrokes("ctrl-v");
    redraw(&mut visual);
    let next = visual.read(|cx| {
        let view = editor.read(cx);
        assert!(view.buffer().point().0 > 0);
        assert!(
            view.buffer()
                .text()
                .is_char_boundary(view.buffer().point().0)
        );
        assert!(view.scroll.y > px(0.));
        assert_eq!(view.buffer().text().max_point().row, 0);
        let index = view.wrap_index().unwrap();
        let row = index.row_for_offset(view.buffer().point()).unwrap();
        assert_eq!(view.buffer().point(), row.start);
        assert_eq!(view.message(), None);
        index.row(row.index + 1).unwrap().start
    });
    visual.simulate_keystrokes("ctrl-n");
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.buffer().point(), next);
        assert_eq!(view.message(), None);
        assert!(!view.buffer().is_modified());
    });
}

#[gpui::test]
fn vertical_motion_keeps_the_column_within_a_visual_row(cx: &mut TestAppContext) {
    let (mut visual, editor) = open(&"0123456789".repeat(200), cx);
    let (second, third) = visual.read(|cx| {
        let index = editor.read(cx).wrap_index().unwrap();
        assert!(index.is_complete());
        assert!(index.row(1).unwrap().start.0 > 3);
        (index.row(1).unwrap().start.0, index.row(2).unwrap().start.0)
    });
    visual.simulate_keystrokes("ctrl-f ctrl-f ctrl-f");
    redraw(&mut visual);
    for (keys, expected) in [
        ("ctrl-n", second + 3),
        ("ctrl-n", third + 3),
        ("ctrl-p", second + 3),
        ("ctrl-p", 3),
    ] {
        visual.simulate_keystrokes(keys);
        redraw(&mut visual);
        visual.read(|cx| {
            let view = editor.read(cx);
            assert_eq!(view.buffer().point(), ByteOffset(expected), "{keys}");
            assert_eq!(view.message(), None);
        });
    }
    // A boundary error keeps the goal column; an interrupting command
    // recomputes it from the row the point now belongs to.
    visual.simulate_keystrokes("ctrl-f");
    redraw(&mut visual);
    visual.simulate_keystrokes("ctrl-p");
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.buffer().point(), ByteOffset(0));
        assert_eq!(view.message(), Some("Beginning of buffer"));
    });
    visual.simulate_keystrokes("ctrl-n");
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(
            editor.read(cx).buffer().point(),
            ByteOffset(second + 4),
            "goal survives a boundary error"
        );
    });
    visual.simulate_keystrokes("ctrl-f ctrl-p");
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.buffer().point(), ByteOffset(5));
        assert_eq!(view.message(), None);
    });
}

#[gpui::test]
fn vertical_motion_clamps_to_a_shorter_visual_row(cx: &mut TestAppContext) {
    let (mut visual, editor) = open(&"0123456789".repeat(200), cx);
    let (start, end) = visual.read(|cx| {
        let index = editor.read(cx).wrap_index().unwrap();
        let last = index.row(index.row_count() - 1).unwrap();
        let previous = index.row(last.index - 1).unwrap();
        assert!(previous.row.content_len > last.row.content_len);
        (
            previous.start.0 + previous.row.content_len - 1,
            last.start.0 + last.row.content_len,
        )
    });
    editor.update(&mut visual, |view, _| {
        view.target
            .set_point_from_pointer(view.target.token(), ByteOffset(start))
            .unwrap();
    });
    visual.simulate_keystrokes("ctrl-n");
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.buffer().point(), ByteOffset(end));
        assert_eq!(view.message(), None);
    });
}

#[gpui::test]
fn visual_motion_reports_boundaries_only_for_a_complete_index(cx: &mut TestAppContext) {
    use crate::display::{FontMeasure, WrapJob};
    use crate::editing::{CommandViewport, RouteError};

    let (visual, editor) = open(&"abcdef\n".repeat(10), cx);
    visual.read(|cx| {
        let view = editor.read(cx);
        let settings = view.settings.clone();
        let mut metrics = FontMeasure::new(cx.text_system().clone(), settings.clone());
        let index = WrapJob::new(view.display_snapshot(), settings.clone(), None, None).preview(
            &mut metrics,
            2,
            8192,
        );
        assert!(!index.is_complete());
        let viewport = super::motion::Viewport {
            index: &index,
            first_row: 0,
            visible_rows: 2,
        };
        let motion = viewport.adjacent_row(ByteOffset(0), true).unwrap();
        assert_eq!(motion.current, ByteOffset(0)..ByteOffset(6));
        assert_eq!(motion.target, Some(ByteOffset(7)..ByteOffset(13)));
        let last = index.row(index.row_count() - 1).unwrap();
        assert_eq!(
            viewport.adjacent_row(last.start, true).err(),
            Some(RouteError::LayoutNotReady)
        );
        assert_eq!(
            viewport.adjacent_row(ByteOffset(0), false).err(),
            Some(RouteError::LayoutNotReady)
        );
    });
    visual.read(|cx| {
        let view = editor.read(cx);
        let index = view.wrap_index().unwrap();
        assert!(index.is_complete());
        let viewport = super::motion::Viewport {
            index,
            first_row: 0,
            visible_rows: 2,
        };
        assert_eq!(
            viewport.adjacent_row(ByteOffset(0), false).unwrap().target,
            None
        );
        let last = index.row(index.row_count() - 1).unwrap();
        assert_eq!(
            viewport.adjacent_row(last.start, true).unwrap().target,
            None
        );
    });
}

#[gpui::test]
fn leaving_the_viewport_recenters_the_point_row(cx: &mut TestAppContext) {
    let (mut visual, editor) = open(&"abcdef\n".repeat(100), cx);
    let (line_height, visible) = visual.read(|cx| {
        let view = editor.read(cx);
        let height = view.content_bounds.unwrap().size.height;
        (
            view.settings.line_height,
            (height / view.settings.line_height).floor() as usize,
        )
    });
    assert!(visible > 4);
    let last_visible = visible - 1;
    let keys = vec!["ctrl-n"; last_visible].join(" ");
    visual.simulate_keystrokes(&keys);
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.buffer().point(), ByteOffset(last_visible * 7));
        assert_eq!(view.scroll.y, px(0.), "a visible point never scrolls");
    });
    visual.simulate_keystrokes("ctrl-n");
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        let row = view
            .wrap_index()
            .unwrap()
            .row_for_offset(view.buffer().point())
            .unwrap()
            .index;
        assert_eq!(row, last_visible + 1);
        assert!(view.pending_scroll.is_none());
        assert_row_in_the_middle(view, row);
    });
    // Scrolling deeper keeps the upward recenter away from the first line.
    visual.simulate_keystrokes(&["ctrl-v"; 2].join(" "));
    redraw(&mut visual);
    let (point_row, first) = visual.read(|cx| {
        let view = editor.read(cx);
        let index = view.wrap_index().unwrap();
        (
            index.row_for_offset(view.buffer().point()).unwrap().index,
            (f32::from(view.scroll.y) / f32::from(line_height)).floor() as usize,
        )
    });
    assert!(point_row >= first && first > 1);
    let keys = vec!["ctrl-p"; point_row - first + 1].join(" ");
    visual.simulate_keystrokes(&keys);
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        let row = view
            .wrap_index()
            .unwrap()
            .row_for_offset(view.buffer().point())
            .unwrap()
            .index;
        assert_eq!(row, first - 1);
        assert!(view.scroll.y > px(0.), "the upward recenter is not clamped");
        assert_row_in_the_middle(view, row);
    });
}

/// The point row sits inside the window with its middle within one line of the
/// window middle; the exact pixel offset is this product's rounding.
fn assert_row_in_the_middle(view: &super::BufferView, row: usize) {
    let line = view.settings.line_height;
    let height = view.content_bounds.unwrap().size.height;
    let top = line * row as f32 - view.scroll.y;
    assert!(
        top > px(0.) && top + line < height,
        "the point row is not fully inside the window"
    );
    assert!(
        (top - height / 2.).abs() < line,
        "the point row is not near the middle"
    );
}

#[gpui::test]
fn buffer_edges_clamp_the_recenter(cx: &mut TestAppContext) {
    let (mut visual, editor) = open(&"abcdef\n".repeat(100), cx);
    visual.simulate_keystrokes("alt->");
    redraw(&mut visual);
    let (height, line) = visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.buffer().point(), ByteOffset(700));
        assert!(view.pending_scroll.is_none());
        (
            view.content_bounds.unwrap().size.height,
            view.settings.line_height,
        )
    });
    // `end-of-buffer` puts the point three lines above the window bottom
    // instead of centering it like redisplay does for other point moves.
    assert_eq!(row_offset(&visual, &editor, 700), height - line * 3.);
    // A partially visible end row is left to redisplay, which centers.
    visual.update(|_, cx| {
        editor.update(cx, |view, cx| {
            let height = view.content_bounds.unwrap().size.height;
            let line = view.settings.line_height;
            let target = line * 100. - (height - line + px(8.));
            view.scroll_by(point(px(0.), target - view.scroll.y), cx);
        })
    });
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        let line = view.settings.line_height;
        let height = view.content_bounds.unwrap().size.height;
        let top = line * 100. - view.scroll.y;
        assert!(
            top < height && top + line > height,
            "the end row is partially visible"
        );
    });
    visual.simulate_keystrokes("alt->");
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        let line = view.settings.line_height;
        let top = line * 100. - view.scroll.y;
        assert_ne!(
            top,
            view.content_bounds.unwrap().size.height - line * 3.,
            "a partially visible end is left to redisplay"
        );
        assert_row_in_the_middle(view, 100);
    });
    visual.simulate_keystrokes("alt-<");
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.buffer().point(), ByteOffset(0));
        assert_eq!(view.scroll.y, px(0.));
    });
}

/// The offset of the row holding `offset` from the top of the window.
fn row_offset(
    visual: &VisualTestContext,
    editor: &Entity<super::BufferView>,
    offset: usize,
) -> gpui::Pixels {
    visual.read(|cx| {
        let view = editor.read(cx);
        let row = view
            .wrap_index()
            .unwrap()
            .row_for_offset(ByteOffset(offset))
            .unwrap()
            .index;
        view.settings.line_height * row as f32 - view.scroll.y
    })
}

#[gpui::test]
fn end_of_buffer_does_not_scroll_when_the_end_is_visible(cx: &mut TestAppContext) {
    let (mut visual, editor) = open("abc\ndef\n", cx);
    visual.simulate_keystrokes("alt->");
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.buffer().point(), ByteOffset(8));
        assert_eq!(view.scroll.y, px(0.));
    });
}

#[gpui::test]
fn page_boundaries_move_to_edges_before_reporting_errors(cx: &mut TestAppContext) {
    let (mut visual, editor) = open("abc\ndef", cx);
    for (keys, expected, message) in [
        ("ctrl-v", 7, None),
        ("ctrl-v", 7, Some("End of buffer")),
        ("alt-v", 0, None),
        ("alt-v", 0, Some("Beginning of buffer")),
    ] {
        visual.simulate_keystrokes(keys);
        redraw(&mut visual);
        visual.read(|cx| {
            let view = editor.read(cx);
            assert_eq!(view.buffer().point(), ByteOffset(expected));
            assert_eq!(view.scroll.y, px(0.));
            assert_eq!(view.message(), message);
            assert!(!view.buffer().is_modified());
        });
    }
}

#[gpui::test]
fn recenter_cycles_center_top_bottom_and_allows_blank_space_after_eob(cx: &mut TestAppContext) {
    let (mut visual, editor) = open(&"abcdef\n".repeat(100), cx);
    visual.simulate_keystrokes("alt->");
    redraw(&mut visual);
    let id = visual.read(|cx| editor.read(cx).display_snapshot().id());
    for cycle in [0, 1, 2, 0] {
        visual.simulate_keystrokes("ctrl-l");
        redraw(&mut visual);
        visual.read(|cx| {
            let view = editor.read(cx);
            let available =
                (view.content_bounds.unwrap().size.height - view.settings.line_height).max(px(0.));
            let offset = match cycle {
                0 => available / 2.,
                1 => px(0.),
                _ => available,
            };
            assert_eq!(view.scroll.y, view.settings.line_height * 100. - offset);
            assert_eq!(view.buffer().point(), ByteOffset(700));
            assert_eq!(view.buffer().mark(), Some(ByteOffset(0)));
            assert!(!view.buffer().mark_active());
            assert_eq!(view.display_snapshot().id(), id);
            assert!(view.pending_scroll.is_none());
            assert!(view.layout_ready());
        });
    }
    visual.simulate_keystrokes("ctrl-b ctrl-l");
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        let available = view.content_bounds.unwrap().size.height - view.settings.line_height;
        assert_eq!(
            view.scroll.y,
            view.settings.line_height * 99. - available / 2.
        );
    });
}

#[gpui::test]
fn paging_rejects_stale_or_incomplete_indexes_without_changing_text_or_point(
    cx: &mut TestAppContext,
) {
    let (mut visual, editor) = open(&"abcdef\n".repeat(10_000), cx);
    visual.update(|window, cx| {
        editor.update(cx, |view, cx| {
            view.set_font(font("monospace"), px(18.), px(27.), cx)
        });
        window.dispatch_keystroke(Keystroke::parse("ctrl-v").unwrap(), cx);
        assert_eq!(
            editor.read(cx).message(),
            Some("Text layout is not ready; try again")
        );
        assert_eq!(editor.read(cx).buffer().point(), ByteOffset(0));
        window.draw(cx).clear(cx);
        assert!(!editor.read(cx).wrap_index().unwrap().is_complete());
        window.dispatch_keystroke(Keystroke::parse("ctrl-v").unwrap(), cx);
        assert_eq!(
            editor.read(cx).message(),
            Some("Text layout is not ready; try again")
        );
        assert_eq!(editor.read(cx).buffer().point(), ByteOffset(0));
    });
    redraw(&mut visual);
    visual.simulate_keystrokes("ctrl-v");
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert!(view.buffer().point().0 > 0);
        assert_eq!(view.message(), None);
        assert!(!view.buffer().is_modified());
    });
}

#[gpui::test]
fn new_motion_commands_preserve_readonly_regions_and_the_composition_barrier(
    cx: &mut TestAppContext,
) {
    let (mut visual, editor) = open(&"abcdef\n".repeat(100), cx);
    editor.update(&mut visual, |view, _| view.target.set_read_only(true));
    visual.simulate_keystrokes("ctrl-t ctrl-v ctrl-l");
    redraw(&mut visual);
    let point = visual.read(|cx| {
        let view = editor.read(cx);
        assert!(view.buffer().point().0 > 0);
        assert_eq!(view.buffer().mark(), Some(ByteOffset(0)));
        assert!(view.buffer().mark_active());
        assert!(!view.buffer().is_modified());
        view.buffer().point()
    });
    editor.update(&mut visual, |view, _| view.target.set_read_only(false));
    let mut input = handler(&editor, &visual);
    visual.update(|window, cx| {
        input.replace_and_mark_text_in_range(None, "拼", None, window, cx);
        for key in [
            "ctrl-v", "alt-v", "ctrl-l", "ctrl-n", "ctrl-p", "alt-f", "alt-b", "alt-<", "alt->",
        ] {
            press_listener(&editor, key, window, cx);
            let view = editor.read(cx);
            assert_eq!(
                view.message(),
                Some("Input method composition is active; finish it first")
            );
            assert_eq!(view.buffer().point(), point);
            assert!(!view.target.is_pending());
            assert!(view.buffer().mark_active());
        }
        input.replace_text_in_range(None, "汉", window, cx);
        assert!(!editor.read(cx).target.input().is_composing());
        assert_eq!(
            editor.read(cx).buffer().point(),
            ByteOffset(point.0 + "汉".len())
        );
    });
    redraw(&mut visual);
    let scroll = visual.read(|cx| editor.read(cx).scroll);
    visual.simulate_keystrokes("ctrl-x ctrl-v");
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.message(), Some("C-x C-v is undefined"));
        assert_eq!(view.scroll, scroll);
        assert!(!view.target.is_pending());
    });
}

#[gpui::test]
fn paging_cannot_use_the_previous_committed_text_index_in_the_same_frame(cx: &mut TestAppContext) {
    let (mut visual, editor) = open(&"abcdef\n".repeat(100), cx);
    let mut input = handler(&editor, &visual);
    visual.update(|window, cx| {
        input.replace_text_in_range(None, "x", window, cx);
        press_listener(&editor, "ctrl-v", window, cx);
        let view = editor.read(cx);
        assert_eq!(view.message(), Some("Text layout is not ready; try again"));
        assert_eq!(view.buffer().point(), ByteOffset(1));
        assert_eq!(view.scroll.y, px(0.));
    });
    redraw(&mut visual);
    visual.simulate_keystrokes("ctrl-v");
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert!(view.buffer().point().0 > 1);
        assert_eq!(view.message(), None);
    });
}

#[gpui::test]
fn one_row_pages_still_make_progress_and_bound_excess_wheel_scroll(cx: &mut TestAppContext) {
    use crate::editing::CommandViewport;
    let (visual, editor) = open("a\nb\nc", cx);
    visual.read(|cx| {
        let index = editor.read(cx).wrap_index().unwrap();
        let mut viewport = super::motion::Viewport {
            index,
            first_row: 0,
            visible_rows: 1,
        };
        let motion = viewport.page(ByteOffset(0), true).unwrap();
        assert_eq!(motion.first_row, 1);
        assert_eq!(motion.point, ByteOffset(2));
        viewport.first_row = usize::MAX;
        let motion = viewport.page(ByteOffset(2), false).unwrap();
        assert_eq!(motion.first_row, 1);
        assert_eq!(motion.point, ByteOffset(2));
    });
}

#[gpui::test]
fn recenter_waits_for_compatible_rows_without_blocking_layout(cx: &mut TestAppContext) {
    let (mut visual, editor) = open(&"abcdef\n".repeat(10_000), cx);
    visual.simulate_keystrokes("alt->");
    redraw(&mut visual);
    visual.update(|window, cx| {
        editor.update(cx, |view, cx| {
            view.set_font(font("monospace"), px(18.), px(27.), cx)
        });
        window.dispatch_keystroke(Keystroke::parse("ctrl-l").unwrap(), cx);
        window.draw(cx).clear(cx);
        let view = editor.read(cx);
        assert!(!view.wrap_index().unwrap().is_complete());
        assert!(view.pending_scroll.is_some());
    });
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        let available = view.content_bounds.unwrap().size.height - view.settings.line_height;
        assert_eq!(
            view.scroll.y,
            view.settings.line_height * 10_000. - available / 2.
        );
        assert!(view.pending_scroll.is_none());
        assert!(view.layout_ready());
    });
}
