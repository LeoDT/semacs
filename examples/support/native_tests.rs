use std::time::{Duration, Instant};

use gpui_kit::WindowId;
use gpui_kit::profiler::journal::PresentedFrame;
use gpui_kit::profiler::{DrawPhases, FrameTiming, PresentTiming};

use super::{EditStamp, Measurement, PaintState, Step};

pub(super) fn stamp() -> EditStamp {
    let buffer = semacs::buffer::Buffer::new(
        None,
        semacs::files::DecodedFile {
            text: "abc".into(),
            format: semacs::files::FileFormat::default(),
        },
    );
    let target = semacs::editing::EditTarget::new(buffer);
    EditStamp {
        display: target.display_snapshot().id(),
        point: 0,
        mark: None,
        mark_active: false,
    }
}

pub(super) fn frame(start: Instant) -> PresentedFrame {
    PresentedFrame {
        frame: FrameTiming {
            window_id: WindowId::from(1),
            dirty_at: Some(start),
            invalidations: 1,
            draw_start: start,
            draw_end: start + Duration::from_millis(1),
            refresh_interval: None,
            signal_at: None,
            phases: DrawPhases::default(),
        },
        presentation: PresentTiming {
            window_id: WindowId::from(1),
            present_start: start + Duration::from_millis(1),
            present_end: start + Duration::from_millis(2),
            animation_interval: None,
        },
    }
}

pub(super) fn paint(at: Instant) -> PaintState {
    PaintState {
        stamp: stamp(),
        at,
        ready: true,
        compatible: true,
        bytes: 3,
        indexed: 3,
        rows: 1,
        visible: 1,
        scanned: 3,
        width: 944.,
        height: 576.,
        scroll: 0.,
        marked: false,
    }
}

#[test]
fn paint_pairing_discards_stale_observations_and_preserves_future_ones() {
    let start = Instant::now();
    let stale = start - Duration::from_millis(1);
    let current = start + Duration::from_micros(500);
    let future = start + Duration::from_millis(3);
    let mut paints = std::collections::VecDeque::from([stale, current, future]);
    assert_eq!(
        super::take_paint(&mut paints, &frame(start), |at| *at),
        Some(current)
    );
    assert_eq!(
        super::take_paint(&mut paints, &frame(start), |at| *at),
        None
    );
    assert_eq!(paints.front(), Some(&future));
    assert_eq!(
        super::take_paint(&mut paints, &frame(future), |at| *at),
        Some(future)
    );
    assert!(paints.is_empty());
    assert_eq!(
        super::take_paint(&mut paints, &frame(future), |at| *at),
        None
    );
}

#[test]
fn previous_frames_do_not_complete_a_new_action() {
    let mut measurement = Measurement::new(Step::Insert, "abc".into(), false);
    let earlier = measurement.start - Duration::from_secs(1);
    assert!(!measurement.accept(frame(earlier), Some(paint(earlier))));
    assert_eq!(measurement.frames, 0);
    assert!(measurement.first_frame.is_none());
}

#[test]
fn blank_preview_and_complete_body_have_distinct_boundaries() {
    let mut measurement = Measurement::new(Step::Open, "abc".into(), false);
    let start = measurement.start;
    let mut blank = paint(start);
    blank.visible = 0;
    blank.ready = false;
    assert!(!measurement.accept(frame(start), Some(blank)));
    assert!(measurement.first.is_none());
    assert_eq!(measurement.blank_frames, 1);

    let mut preview = paint(start + Duration::from_millis(3));
    preview.ready = false;
    preview.indexed = 1;
    assert!(!measurement.accept(frame(preview.at), Some(preview)));
    assert!(measurement.first.is_some());

    let complete = paint(start + Duration::from_millis(6));
    assert!(measurement.accept(frame(complete.at), Some(complete)));
    assert_eq!(measurement.frames, 3);
    assert_eq!(measurement.draw_ms, 3.);
    assert_eq!(measurement.submit_ms, 3.);
    assert!(
        measurement.first_frame.unwrap().frame.draw_start
            < measurement.first.unwrap().frame.draw_start
    );
    assert!(
        measurement.first.unwrap().frame.draw_start < measurement.last.unwrap().frame.draw_start
    );
}

#[test]
fn cached_frames_are_counted_without_inventing_body_observations() {
    let mut measurement = Measurement::new(Step::Idle, "abc".into(), false);
    let start = measurement.start;
    assert!(!measurement.accept(frame(start), None));
    assert_eq!(measurement.frames, 1);
    assert_eq!(measurement.draw_ms, 1.);
    assert!(measurement.first_paint.is_none());
    assert!(measurement.first.is_none());
}

#[test]
fn retained_text_is_not_a_blank_frame_or_current_layout_completion() {
    let mut measurement = Measurement::new(Step::Font, "abc".into(), false);
    let start = measurement.start;
    let mut retained = paint(start);
    retained.compatible = false;
    retained.ready = false;
    assert!(!measurement.accept(frame(start), Some(retained)));
    assert_eq!(measurement.blank_frames, 0);
    assert!(measurement.first.is_none());
    assert!(measurement.accept(frame(start), Some(paint(start))));
}

#[test]
fn external_input_waits_for_target_preedit_state() {
    let mut measurement = Measurement::new(Step::Preedit, "abc".into(), true);
    let start = measurement.start;
    assert!(!measurement.accept(frame(start), Some(paint(start))));
    let mut marked = paint(start);
    marked.marked = true;
    marked.bytes = 2;
    assert!(!measurement.accept(frame(start), Some(marked)));
    marked.bytes = 3;
    assert!(measurement.accept(frame(start), Some(marked)));
}

#[test]
fn resize_waits_for_an_actual_width_change() {
    let mut measurement = Measurement::new(Step::Resize, "abc".into(), false);
    measurement.old_width = 944.;
    let start = measurement.start;
    assert!(!measurement.accept(frame(start), Some(paint(start))));
    let mut resized = paint(start);
    resized.width = 704.;
    assert!(measurement.accept(frame(start), Some(resized)));
}
