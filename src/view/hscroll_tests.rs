use std::sync::Arc;

use gpui_kit as gpui;
use gpui_kit::{
    AppContext, BorrowAppContext, Context, Entity, InputHandler, ScrollDelta, ScrollEvent,
    TestAppContext, VisualTestContext, Window, font, point, px, size,
};

use super::BufferView;
use super::input_tests::{app_shell, handler, open, open_buffer, press_listener, redraw};
use crate::buffer::Buffer;
use crate::display::{FontMeasure, WrapJob};
use crate::text::ByteOffset;

fn text() -> String {
    format!("{}\n", "0123456789".repeat(30)).repeat(30)
}

fn wheel(view: &mut BufferView, columns: f32, window: &mut Window, cx: &mut Context<BufferView>) {
    view.scroll_wheel(
        &ScrollEvent {
            position: view.content_bounds.unwrap().origin + point(px(4.), px(4.)),
            delta: ScrollDelta::Lines(point(-columns, 0.)),
            modifiers: gpui::Modifiers {
                shift: true,
                ..Default::default()
            },
            ..Default::default()
        },
        window,
        cx,
    );
}

fn horizontal(visual: &mut VisualTestContext, view: &Entity<BufferView>, columns: f32) {
    visual.update(|window, cx| view.update(cx, |view, cx| wheel(view, columns, window, cx)));
}

#[gpui::test]
fn horizontal_offset_temporarily_truncates_and_zero_restores_wrapping(cx: &mut TestAppContext) {
    let (mut visual, editor) = open(&text(), cx);
    visual.simulate_keystrokes("ctrl-t ctrl-f ctrl-f ctrl-f");
    redraw(&mut visual);
    let (rows, id, before) = visual.read(|cx| {
        let view = editor.read(cx);
        assert!(view.settings.soft_wrap);
        (
            view.wrap_index().unwrap().row_count(),
            view.display_snapshot().id(),
            view.buffer().point(),
        )
    });
    horizontal(&mut visual, &editor, 5.);
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert!(view.scroll.x > px(0.));
        assert!(!view.settings.soft_wrap);
        assert_eq!(view.wrap_index().unwrap().row_count(), 31);
        assert!(view.wrap_index().unwrap().row_count() < rows);
        assert_eq!(view.buffer().point(), before);
        assert_eq!(view.display_snapshot().id(), id);
        assert!(view.buffer().mark_active());
        assert!(!view.buffer().is_modified());
        let layout = view.viewport_layout().unwrap();
        let at = layout
            .hit_test(layout.bounds().origin + point(px(1.), px(4.)))
            .unwrap();
        assert!(at.0 > before.0);
        assert_eq!(view.buffer().text().offset_to_point(at.0).row, 0);
    });
    horizontal(&mut visual, &editor, -100.);
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.scroll.x, px(0.));
        assert!(view.settings.soft_wrap);
        assert_eq!(view.wrap_index().unwrap().row_count(), rows);
        assert_eq!(view.display_snapshot().id(), id);
        assert_eq!(view.buffer().point(), before);
        assert!(view.buffer().mark_active());
    });
}

#[gpui::test]
fn horizontal_events_can_cross_zero_repeatedly_without_a_frame(cx: &mut TestAppContext) {
    let (mut visual, editor) = open(&text(), cx);
    let original = visual.read(|cx| editor.read(cx).wrap_index().unwrap().key().clone());
    visual.update(|window, cx| {
        editor.update(cx, |view, cx| {
            for _ in 0..3 {
                wheel(view, 1., window, cx);
            }
        });
        press_listener(&editor, "ctrl-v", window, cx);
        let view = editor.read(cx);
        assert_eq!(view.message(), Some("Text layout is not ready; try again"));
        assert_eq!(view.buffer().point(), ByteOffset(0));
        assert!(!view.settings.soft_wrap);
        editor.update(cx, |view, cx| {
            wheel(view, -3., window, cx);
            assert!(view.settings.soft_wrap);
            assert_eq!(view.message(), None);
        });
    });
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.wrap_index().unwrap().key(), &original);
        assert!(view.layout_ready());
    });
}

#[gpui::test]
fn whole_column_steps_return_to_exact_zero_without_pixel_roundoff(cx: &mut TestAppContext) {
    let (mut visual, editor) = open(&text(), cx);
    for count in [3, 7, 11, 23] {
        visual.update(|window, cx| {
            editor.update(cx, |view, cx| {
                for _ in 0..count {
                    wheel(view, 1., window, cx);
                }
                wheel(view, -(count as f32), window, cx);
                assert_eq!(view.scroll.x, px(0.));
                assert!(view.settings.soft_wrap);
            })
        });
        redraw(&mut visual);
        visual.read(|cx| assert!(editor.read(cx).layout_ready()));
    }
}

#[gpui::test]
fn returning_to_zero_does_not_cancel_an_unfinished_matching_wrap_index(cx: &mut TestAppContext) {
    let (mut visual, editor) = open(&text(), cx);
    visual.update(|window, cx| {
        editor.update(cx, |view, cx| {
            let mut metrics = FontMeasure::new(cx.text_system().clone(), view.settings.clone());
            let job = WrapJob::new(view.display_snapshot(), view.settings.clone(), None, None);
            view.index = Some(job.preview(&mut metrics, 2, 8192));
            assert!(!view.index.as_ref().unwrap().is_complete());
            view.finish_index(job, cx);
            wheel(view, 1., window, cx);
            wheel(view, -1., window, cx);
            assert!(view.layout_work.is_some());
        })
    });
    redraw(&mut visual);
    visual.read(|cx| assert!(editor.read(cx).layout_ready()));
}

#[gpui::test]
fn a_rejected_background_index_is_restarted_when_the_wrap_mode_returns(cx: &mut TestAppContext) {
    cx.update(crate::app::init);
    let editor = cx.new(|cx| {
        let buffer = Buffer::new(
            None,
            crate::files::DecodedFile {
                text: text(),
                format: Default::default(),
            },
        );
        BufferView::new(buffer, cx)
    });
    let bounds = gpui::Bounds::new(point(px(0.), px(0.)), size(px(400.), px(200.)));
    editor.update(cx, |view, cx| {
        view.settings.wrap_width = bounds.size.width;
        let mut metrics = FontMeasure::new(cx.text_system().clone(), view.settings.clone());
        let job = WrapJob::new(view.display_snapshot(), view.settings.clone(), None, None);
        view.index = Some(job.preview(&mut metrics, 2, 8192));
        view.metrics = Some(metrics);
        view.finish_index(job, cx);
        view.scroll_by(point(px(10.), px(0.)), cx);
        assert!(!view.settings.soft_wrap);
    });
    cx.run_until_parked();
    editor.read_with(cx, |view, _| {
        assert!(!view.index.as_ref().unwrap().is_complete());
        assert!(view.index.as_ref().unwrap().key().settings.soft_wrap);
        assert!(view.layout_work.is_none());
    });
    editor.update(cx, |view, cx| {
        view.scroll_by(point(px(-10.), px(0.)), cx);
        view.ensure_index(bounds, cx);
        assert!(view.layout_work.is_some());
    });
    cx.run_until_parked();
    editor.read_with(cx, |view, _| {
        assert!(view.index.as_ref().unwrap().is_complete())
    });
}

#[gpui::test]
fn extra_left_scroll_at_zero_preserves_the_pending_text_anchor_and_reveal(cx: &mut TestAppContext) {
    let (mut visual, editor) = open(&text(), cx);
    visual.update(|window, cx| {
        editor.update(cx, |view, cx| {
            view.show_location(ByteOffset(10 * 301 + 50), window, cx)
        })
    });
    redraw(&mut visual);
    horizontal(&mut visual, &editor, 1.);
    redraw(&mut visual);
    visual.update(|window, cx| {
        editor.update(cx, |view, cx| {
            wheel(view, -1., window, cx);
            let anchor = view.scroll_anchor;
            assert!(anchor.is_some());
            assert!(view.reveal_caret);
            wheel(view, -1., window, cx);
            assert_eq!(view.scroll_anchor, anchor);
            assert!(view.reveal_caret);
        })
    });
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        let row = view
            .wrap_index()
            .unwrap()
            .row_for_offset(view.buffer().point())
            .unwrap()
            .index;
        assert!(view.settings.soft_wrap);
        assert!(view.row_fully_visible(row, view.content_bounds.unwrap().size.height));
        assert!(row > 20);
    });
}

#[gpui::test]
fn shared_edits_do_not_resume_horizontal_follow_in_another_view(cx: &mut TestAppContext) {
    let (mut visual, first) = open(&text(), cx);
    visual.simulate_resize(size(px(960.), px(640.)));
    redraw(&mut visual);
    visual.simulate_keystrokes("ctrl-x 3 ctrl-x o");
    redraw(&mut visual);
    let second = visual.update(|window, cx| app_shell(window, cx).read(cx).active_view().clone());
    visual.update(|window, cx| {
        second.update(cx, |view, cx| {
            view.show_location(ByteOffset(10 * 301), window, cx)
        })
    });
    redraw(&mut visual);
    horizontal(&mut visual, &second, 20.);
    redraw(&mut visual);
    let before = visual.read(|cx| second.read(cx).scroll.x);
    visual.simulate_keystrokes("ctrl-x o");
    redraw(&mut visual);
    visual.simulate_input("prefix");
    visual.simulate_keystrokes("enter");
    redraw(&mut visual);
    visual.update(|_, cx| {
        cx.update_global::<crate::appearance::FontSettings, _>(|font, _| {
            font.set_size(px(18.)).unwrap()
        })
    });
    redraw(&mut visual);
    visual.read(|cx| {
        let view = second.read(cx);
        assert_eq!(
            view.buffer().point(),
            ByteOffset(10 * 301 + "prefix\n".len())
        );
        assert_eq!(view.scroll.x, before);
        assert!(!view.settings.soft_wrap);
        assert!(view.suspended_hscroll_point.is_some());
        assert_eq!(
            first.read(cx).buffer().snapshot_id(),
            view.buffer().snapshot_id()
        );
    });
}

#[gpui::test]
fn only_crossing_zero_changes_the_index_and_shaped_rows(cx: &mut TestAppContext) {
    let (mut visual, editor) = open(&text(), cx);
    horizontal(&mut visual, &editor, 1.);
    redraw(&mut visual);
    let (key, shaped) = visual.read(|cx| {
        let view = editor.read(cx);
        (
            view.wrap_index().unwrap().key().clone(),
            view.cache.get(&0).unwrap().clone(),
        )
    });
    horizontal(&mut visual, &editor, 1.);
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.wrap_index().unwrap().key(), &key);
        assert!(Arc::ptr_eq(view.cache.get(&0).unwrap(), &shaped));
    });
}

#[gpui::test]
fn layout_switches_revoke_old_input_coordinates_and_keep_commands_current(cx: &mut TestAppContext) {
    let (mut visual, editor) = open(&text(), cx);
    let mut old = handler(&editor, &visual);
    horizontal(&mut visual, &editor, 1.);
    visual.update(|window, cx| old.replace_text_in_range(None, "late", window, cx));
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert!(!view.buffer().is_modified());
        assert_eq!(view.buffer().point(), ByteOffset(0));
    });
    visual.simulate_keystrokes("ctrl-f ctrl-f ctrl-f ctrl-n");
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.buffer().point(), ByteOffset(301 + 3));
        assert!(!view.settings.soft_wrap);
    });
    visual.simulate_input("X");
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert!(view.buffer().is_modified());
        assert_eq!(view.wrap_index().unwrap().row_count(), 31);
        assert!(view.wrap_index().unwrap().is_complete());
        assert_eq!(
            view.wrap_index().unwrap().indexed_bytes(),
            view.buffer().text().len()
        );
    });
}

#[gpui::test]
fn explicit_point_motion_resumes_horizontal_follow_and_zero_restores_wrapping(
    cx: &mut TestAppContext,
) {
    let (mut visual, editor) = open(&text(), cx);
    horizontal(&mut visual, &editor, 20.);
    redraw(&mut visual);
    let scrolled = visual.read(|cx| editor.read(cx).scroll.x);
    visual.simulate_keystrokes("ctrl-l");
    redraw(&mut visual);
    visual.read(|cx| assert_eq!(editor.read(cx).scroll.x, scrolled));
    visual.simulate_keystrokes("ctrl-f");
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert!(view.scroll.x < scrolled);
        assert!(view.scroll.x > px(0.));
        assert!(!view.settings.soft_wrap);
    });
    visual.simulate_keystrokes("ctrl-a");
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.buffer().point(), ByteOffset(0));
        assert_eq!(view.scroll.x, px(0.));
        assert!(view.settings.soft_wrap);
        assert!(view.layout_ready());
    });
}

#[gpui::test]
fn switching_modes_maps_the_current_text_anchor_instead_of_reusing_visual_row_numbers(
    cx: &mut TestAppContext,
) {
    let text = text();
    let (mut visual, editor) = open(&text, cx);
    visual.update(|window, cx| {
        editor.update(cx, |view, cx| {
            view.show_location(ByteOffset(10 * 301 + 50), window, cx)
        })
    });
    redraw(&mut visual);
    let (logical_first, before) = visual.read(|cx| {
        let view = editor.read(cx);
        let first = (view.scroll.y / view.settings.line_height).floor() as usize;
        let offset = view.wrap_index().unwrap().row(first).unwrap().start;
        (
            view.buffer().text().offset_to_point(offset.0).row as usize,
            view.buffer().point(),
        )
    });
    assert!(logical_first > 0);
    horizontal(&mut visual, &editor, 1.);
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(
            (view.scroll.y / view.settings.line_height).floor() as usize,
            logical_first
        );
        assert_eq!(view.buffer().point(), before);
    });
    horizontal(&mut visual, &editor, -1.);
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert!(view.settings.soft_wrap);
        assert_eq!(view.buffer().point(), before);
        let row = view
            .wrap_index()
            .unwrap()
            .row_for_offset(before)
            .unwrap()
            .index;
        assert!(view.row_fully_visible(row, view.content_bounds.unwrap().size.height));
    });
}

#[gpui::test]
fn wrapping_is_view_local_and_split_copies_the_effective_horizontal_state(cx: &mut TestAppContext) {
    let (mut visual, first) = open(&text(), cx);
    visual.simulate_resize(size(px(960.), px(640.)));
    redraw(&mut visual);
    horizontal(&mut visual, &first, 3.);
    redraw(&mut visual);
    visual.simulate_keystrokes("ctrl-x 3 ctrl-x o");
    redraw(&mut visual);
    let second = visual.update(|window, cx| app_shell(window, cx).read(cx).active_view().clone());
    visual.read(|cx| {
        assert!(!first.read(cx).settings.soft_wrap);
        assert!(!second.read(cx).settings.soft_wrap);
        assert_eq!(first.read(cx).scroll.x, second.read(cx).scroll.x);
    });
    horizontal(&mut visual, &second, -3.);
    redraw(&mut visual);
    visual.read(|cx| {
        assert!(!first.read(cx).settings.soft_wrap);
        assert!(second.read(cx).settings.soft_wrap);
        assert_eq!(
            first.read(cx).buffer().snapshot_id(),
            second.read(cx).buffer().snapshot_id()
        );
    });
}

#[gpui::test]
fn resize_font_changes_and_late_shaping_do_not_restore_an_obsolete_wrap_mode(
    cx: &mut TestAppContext,
) {
    let (mut visual, editor) = open(&format!("{}\nend", "中🙂".repeat(10_000)), cx);
    horizontal(&mut visual, &editor, 1.);
    visual.simulate_resize(size(px(550.), px(280.)));
    editor.update(&mut visual, |view, cx| {
        view.set_font(font("monospace"), px(18.), px(27.), cx)
    });
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert!(!view.settings.soft_wrap);
        assert_eq!(view.wrap_index().unwrap().row_count(), 2);
        assert!(view.layout_ready());
    });
    horizontal(&mut visual, &editor, -100.);
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert!(view.settings.soft_wrap);
        assert!(view.wrap_index().unwrap().row_count() > 2);
        assert_eq!(view.wrap_index().unwrap().key().settings, view.settings);
        assert!(view.layout_ready());
    });
}

#[gpui::test]
fn section_buffers_remain_truncated_after_returning_to_zero(cx: &mut TestAppContext) {
    let repo = crate::diff::tests::Repository::new();
    repo.write("a", "old\n");
    repo.commit();
    repo.write("a", "new\n");
    let data = repo.load(crate::diff::DiffKind::Unstaged);
    let mut buffer = Buffer::diff(data.comparison.clone());
    let request = buffer.begin_diff_request().unwrap().1;
    buffer
        .install_diff(request, data.clone(), data.render().unwrap())
        .unwrap();
    let (mut visual, editor) = open_buffer(buffer, cx);
    let rows = visual.read(|cx| editor.read(cx).wrap_index().unwrap().row_count());
    horizontal(&mut visual, &editor, 2.);
    horizontal(&mut visual, &editor, -2.);
    redraw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.scroll.x, px(0.));
        assert!(!view.settings.soft_wrap);
        assert!(view.wrap_index().unwrap().is_projected());
        assert_eq!(view.wrap_index().unwrap().row_count(), rows);
    });
}
