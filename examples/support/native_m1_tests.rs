use super::*;
use crate::tests::{frame, paint, stamp};

fn reading() -> Reading {
    Reading::new(Phase::Chain(0), Instant::now(), stamp(), 3)
}

fn matched(reading: &Reading, at: Instant) -> PaintState {
    PaintState {
        stamp: reading.stamp,
        ..paint(at)
    }
}

#[test]
fn pacing_is_anchored_not_shifted_by_slow_layout_or_observation() {
    let config = Config::default();
    let origin = Instant::now();
    assert_eq!(config.keys(), 1200);
    assert_eq!(config.due(origin, 4) - origin, Duration::from_millis(200));
    assert!(
        Config {
            seconds: 0,
            ..config
        }
        .validate()
        .is_err()
    );
    assert!(
        Config {
            interval_ms: 0,
            ..config
        }
        .validate()
        .is_err()
    );
    assert!(
        Config {
            seconds: u64::MAX,
            ..config
        }
        .validate()
        .is_err()
    );
    for sent in 0..128 {
        let expected = if sent % 64 <= 32 {
            sent % 64
        } else {
            64 - sent % 64
        };
        assert_eq!(workload::pending_bytes(sent), expected);
    }
}

#[test]
fn same_length_old_identity_and_old_selection_do_not_complete_actions() {
    let mut reading = reading();
    let start = reading.start;
    let mut old = matched(&reading, start);
    old.stamp.display.revision += 1;
    reading.accept(frame(start), Some(old));
    assert!(reading.first.is_none());
    old = matched(&reading, start);
    old.stamp.point += 1;
    reading.accept(frame(start), Some(old));
    assert!(reading.first.is_none());
    old = matched(&reading, start);
    old.stamp.mark = Some(0);
    old.stamp.mark_active = true;
    reading.accept(frame(start), Some(old));
    assert!(reading.first.is_none());
    reading.accept(frame(start), Some(matched(&reading, start)));
    assert!(reading.ready.is_some());
}

#[test]
fn superseded_ready_remains_missing_and_first_completion_is_not_overwritten() {
    let mut reading = reading();
    let start = reading.start;
    let mut preview = matched(&reading, start);
    preview.ready = false;
    preview.indexed = 1;
    reading.accept(frame(start), Some(preview));
    assert_eq!(reading.status(), "superseded-before-ready");
    assert!(elapsed(reading.ready, start).is_none());
    assert!(
        reading
            .budget_error(start + Duration::from_millis(50), true)
            .is_none()
    );
    let later = start + Duration::from_millis(100);
    reading.accept(frame(later), Some(matched(&reading, later)));
    let later_again = start + Duration::from_millis(200);
    reading.accept(frame(later_again), Some(matched(&reading, later_again)));
    assert_eq!(elapsed(reading.first, start), Some(2.));
    assert_eq!(elapsed(reading.ready, start), Some(102.));
    assert_eq!(reading.status(), "ready");
}

#[test]
fn previous_retained_incomplete_and_preedit_frames_never_fake_full_ready() {
    let mut reading = reading();
    let start = reading.start;
    let old = start - Duration::from_millis(1);
    reading.accept(frame(old), Some(matched(&reading, old)));
    assert_eq!(reading.frames, 0);
    let mut candidate = matched(&reading, start);
    candidate.compatible = false;
    reading.accept(frame(start), Some(candidate));
    assert!(reading.first.is_none());
    candidate.compatible = true;
    candidate.marked = true;
    reading.accept(frame(start), Some(candidate));
    assert!(reading.first.is_none());
    candidate.marked = false;
    candidate.indexed = 2;
    reading.accept(frame(start), Some(candidate));
    assert!(reading.first.is_some());
    assert!(reading.ready.is_none());
}

#[test]
fn budgets_fail_on_each_actual_metric_without_retry_or_software_claims() {
    let mut reading = reading();
    let start = reading.start;
    reading.action_ms = 2.1;
    assert!(
        reading
            .budget_error(start, true)
            .unwrap()
            .contains("action_cpu_ms")
    );
    assert!(reading.budget_error(start, false).is_none());
    reading.action_ms = 0.;
    reading.max_draw_ms = 16.1;
    assert!(
        reading
            .budget_error(start, true)
            .unwrap()
            .contains("draw_cpu_max_ms")
    );
    reading.max_draw_ms = 0.;
    assert!(
        reading
            .budget_error(start + Duration::from_millis(17), true)
            .unwrap()
            .contains("first_submit_ms")
    );
    let mut preview = matched(&reading, start);
    preview.ready = false;
    reading.accept(frame(start), Some(preview));
    assert!(
        reading
            .budget_error(start + Duration::from_millis(301), true)
            .unwrap()
            .contains("ready_submit_ms")
    );
    reading.blank_frames = 1;
    assert!(
        reading
            .budget_error(start, false)
            .unwrap()
            .contains("blank")
    );
}

#[test]
fn late_observed_submission_is_not_excused_by_late_collection() {
    let mut reading = reading();
    let start = reading.start;
    let late = start + Duration::from_millis(20);
    reading.accept(frame(late), Some(matched(&reading, late)));
    assert!(
        reading
            .budget_error(late + Duration::from_secs(1), true)
            .unwrap()
            .contains("first_submit_ms=22.000")
    );
}
