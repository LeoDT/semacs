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
        measurement.first.unwrap().frame.draw_start < measurement.ready.unwrap().frame.draw_start
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
fn budgets_stop_missing_boundaries_without_waiting_for_ready_or_timeout() {
    let mut open = Measurement::new(Step::Open, "abc".into(), false);
    let mut preview = paint(open.start);
    preview.ready = false;
    preview.indexed = 1;
    open.accept(frame(open.start), Some(preview));
    let now = open.start + Duration::from_millis(501);
    assert!(
        open.budget_error(now, true, false)
            .unwrap()
            .contains("ready_submit_ms")
    );
    assert!(open.ready.is_none());
    assert!(open.budget_error(now, false, false).is_none());

    for step in [Step::Resize, Step::Font] {
        let mut measurement = Measurement::new(step, "abc".into(), false);
        let mut retained = paint(measurement.start);
        retained.compatible = false;
        retained.ready = false;
        measurement.accept(frame(measurement.start), Some(retained));
        let now = measurement.start + Duration::from_millis(301);
        assert!(
            measurement
                .budget_error(now, true, false)
                .unwrap()
                .contains("ready_submit_ms")
        );
        let row = measurement.csv_row(None, false, "failed");
        let cells: Vec<_> = row.split(',').collect();
        assert_eq!(cells[10], "");
        assert_eq!(cells.last(), Some(&"failed"));
    }
    let measurement = Measurement::new(Step::Font, "abc".into(), false);
    assert!(
        measurement
            .budget_error(measurement.start + Duration::from_millis(51), true, false)
            .unwrap()
            .contains("first_frame_submit_ms")
    );
    assert!(
        measurement
            .budget_error(measurement.start + Duration::from_secs(31), false, false)
            .unwrap()
            .contains("timeout")
    );
}

#[test]
fn native_budget_limits_match_the_driver_for_each_stage() {
    for (step, action, first, ready, scene, draw, expected) in [
        (Step::Open, 0., 251, 252, 2, 1., "first_submit_ms"),
        (Step::Open, 0., 2, 501, 2, 1., "ready_submit_ms"),
        (Step::Resize, 0., 52, 53, 51, 1., "first_frame_submit_ms"),
        (Step::Font, 0., 2, 301, 2, 1., "ready_submit_ms"),
        (Step::Font, 0., 2, 3, 2, 17., "draw_cpu_max_ms"),
        (Step::Insert, 2.1, 2, 3, 2, 1., "action_cpu_ms"),
        (Step::Delete, 0., 17, 18, 2, 1., "first_submit_ms"),
        (Step::Newline, 0., 2, 301, 2, 1., "ready_submit_ms"),
        (Step::Join, 0., 2, 3, 2, 17., "draw_cpu_max_ms"),
        (Step::BurstInsert, 51., 2, 3, 2, 1., "action_cpu_ms"),
        (Step::BurstDelete, 0., 51, 52, 2, 1., "first_submit_ms"),
        (Step::Scroll, 0., 5, 6, 2, 1., "first_submit_ms"),
        (Step::Forced, 0., 3, 4, 2, 1., "first_submit_ms"),
        (Step::Preedit, 0., 76, 77, 2, 1., "first_submit_ms"),
        (Step::Commit, 0., 2, 301, 2, 1., "ready_submit_ms"),
    ] {
        let mut measurement = Measurement::new(step, "abc".into(), false);
        let submitted = |ms: u64| frame(measurement.start + Duration::from_millis(ms - 2));
        measurement.first_frame = Some(submitted(scene));
        measurement.first_paint = Some(paint(measurement.start));
        measurement.first = Some(submitted(first));
        measurement.ready = Some(submitted(ready));
        measurement.action_cpu_ms = action;
        measurement.max_draw_ms = draw;
        let now = measurement.start + Duration::from_secs(1);
        let error = measurement.budget_error(now, true, true).unwrap();
        assert!(error.contains(expected), "{step:?}: {error}");
        assert!(measurement.budget_error(now, false, true).is_none());
    }
}

#[test]
fn timely_submissions_observed_late_do_not_fail_or_move_ready() {
    let mut measurement = Measurement::new(Step::Open, "abc".into(), false);
    let start = measurement.start;
    measurement.accept(frame(start), Some(paint(start)));
    let ready = measurement.ready.unwrap().presentation.present_end;
    assert!(
        measurement
            .budget_error(start + Duration::from_secs(2), true, false)
            .is_none()
    );
    let later = start + Duration::from_millis(10);
    measurement.accept(frame(later), Some(paint(later)));
    assert_eq!(measurement.ready.unwrap().presentation.present_end, ready);
}

#[test]
fn blank_and_idle_draws_fail_even_in_software_diagnostics() {
    let mut measurement = Measurement::new(Step::Idle, "abc".into(), false);
    measurement.accept(frame(measurement.start), None);
    assert!(
        measurement
            .budget_error(measurement.start, false, false)
            .unwrap()
            .contains("idle")
    );
    let mut measurement = Measurement::new(Step::Open, "abc".into(), false);
    let mut blank = paint(measurement.start);
    blank.visible = 0;
    measurement.accept(frame(measurement.start), Some(blank));
    assert!(
        measurement
            .budget_error(measurement.start, false, false)
            .unwrap()
            .contains("blank")
    );
}

#[gpui_kit::test]
fn failed_open_saves_partial_report_and_does_not_start_resize_or_font(
    cx: &mut gpui_kit::TestAppContext,
) {
    use gpui_kit::{AppContext, px, size};
    use std::{
        cell::{Cell, RefCell},
        collections::VecDeque,
        rc::Rc,
    };
    cx.update(semacs::app::init);
    let window = cx.open_window(size(px(400.), px(240.)), |window, cx| {
        semacs::app::AppShell::new(semacs::buffer::Buffer::scratch(), window, cx)
    });
    let editor = window
        .read_with(cx, |shell, _| shell.active_view().clone())
        .unwrap();
    let collector = cx.update(|cx| cx.foreground_journal().collector());
    let mut current = Measurement::new(Step::Open, "abc".into(), false);
    current.start -= Duration::from_secs(1);
    let mut preview = paint(current.start);
    preview.ready = false;
    preview.indexed = 1;
    current.accept(frame(current.start), Some(preview));
    let report = std::path::PathBuf::from(format!(
        ".pi/tmp/native-stage-stop-{}.csv",
        std::process::id()
    ));
    std::fs::create_dir_all(report.parent().unwrap()).unwrap();
    let succeeded = Rc::new(Cell::new(true));
    let mut run = super::Run {
        editor,
        collector,
        paints: Rc::new(RefCell::new(VecDeque::new())),
        actions: VecDeque::from([Step::Resize, Step::Font]),
        current: Some(current),
        output: super::create_report(&report, "# test", false),
        row_count: 0,
        budget_violations: 0,
        diagnostic: false,
        report: report.clone(),
        commands: None,
        external: false,
        waiting_since: Instant::now(),
        last_paint: None,
        hardware: true,
        succeeded: succeeded.clone(),
    };
    cx.update_window(window.into(), |_, window, cx| {
        assert!(!run.tick(window, cx));
    })
    .unwrap();
    assert_eq!(run.actions.len(), 2);
    assert!(run.current.is_none());
    assert!(!succeeded.get());
    let output = std::fs::read_to_string(&report).unwrap();
    assert!(output.contains("# completed=false; budget_passed=false;"));
    assert!(output.contains("# stop_reason=open: ready_submit_ms="));
    assert!(
        !output
            .lines()
            .any(|line| line.starts_with("resize,") || line.starts_with("font,"))
    );
    let row: Vec<_> = output
        .lines()
        .find(|line| line.starts_with("open,"))
        .unwrap()
        .split(',')
        .collect();
    assert_eq!(row[10], "");
    assert_eq!(row.last(), Some(&"failed"));
    std::fs::remove_file(report).unwrap();
}

fn diagnostic_run(
    cx: &mut gpui_kit::TestAppContext,
    text: &str,
    label: &str,
) -> (gpui_kit::VisualTestContext, super::Run, std::path::PathBuf) {
    use gpui_kit::{px, size};
    use std::{
        cell::{Cell, RefCell},
        collections::VecDeque,
        rc::Rc,
    };
    cx.update(semacs::app::init);
    let buffer = semacs::buffer::Buffer::new(
        None,
        semacs::files::DecodedFile {
            text: text.into(),
            format: semacs::files::FileFormat::default(),
        },
    );
    let window = cx.open_window(size(px(960.), px(640.)), |window, cx| {
        semacs::app::AppShell::new(buffer, window, cx)
    });
    let editor = window
        .read_with(cx, |shell, _| shell.active_view().clone())
        .unwrap();
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    let collector = visual.update(|_, cx| cx.foreground_journal().collector());
    let report = std::path::PathBuf::from(format!(
        ".pi/tmp/native-diagnostic-{label}-{}.csv",
        std::process::id()
    ));
    std::fs::create_dir_all(report.parent().unwrap()).unwrap();
    let run = super::Run {
        editor,
        collector,
        paints: Rc::new(RefCell::new(VecDeque::new())),
        actions: super::display_actions(),
        current: Some(Measurement::new(Step::Open, text.into(), false)),
        output: super::create_report(
            &report,
            "# TestPlatform / synthetic submission boundaries; not native",
            true,
        ),
        row_count: 0,
        budget_violations: 0,
        diagnostic: true,
        report: report.clone(),
        commands: None,
        external: false,
        waiting_since: Instant::now(),
        last_paint: None,
        hardware: true,
        succeeded: Rc::new(Cell::new(false)),
    };
    (visual, run, report)
}

#[gpui_kit::test]
fn diagnostic_continues_after_slow_open_through_real_scroll_font_input_and_idle(
    cx: &mut gpui_kit::TestAppContext,
) {
    use gpui_kit::{px, size};
    let text = "AV e\u{301}🙂 alpha beta gamma\n".repeat(2000);
    let (mut visual, mut run, report) = diagnostic_run(cx, &text, "complete");
    let steps: Vec<_> = std::iter::once(Step::Open)
        .chain(super::display_actions())
        .collect();
    for (index, step) in steps.iter().enumerate() {
        assert_eq!(run.current.as_ref().unwrap().step.name(), step.name());
        if matches!(step, Step::Resize) {
            visual.simulate_resize(size(px(720.), px(640.)));
        }
        if matches!(step, Step::Idle) {
            run.current.as_mut().unwrap().idle_ticks = 29;
        } else {
            for _ in 0..8 {
                visual.run_until_parked();
                visual.update(|window, cx| {
                    window.refresh();
                    window.draw(cx).clear(cx);
                });
                if visual.read(|cx| run.editor.read(cx).layout_ready()) {
                    break;
                }
            }
            let observation = visual.read(|cx| super::capture(run.editor.read(cx)));
            let current = run.current.as_mut().unwrap();
            if index == 0 {
                current.start -= Duration::from_millis(600);
            }
            assert!(
                current.accept(frame(observation.at), Some(observation)),
                "{step:?}"
            );
            run.last_paint = Some(observation);
        }
        let keep_running = visual.update(|window, cx| run.tick(window, cx));
        assert_eq!(keep_running, index + 1 < steps.len(), "{step:?}");
        let saved = std::fs::read_to_string(&report).unwrap();
        assert_eq!(
            saved.lines().filter(|line| !line.starts_with('#')).count(),
            index + 2
        );
    }
    assert!(run.succeeded.get());
    assert!(run.budget_violations >= 2);
    let output = std::fs::read_to_string(&report).unwrap();
    assert!(output.contains("# completed=true; budget_passed=false; acceptance=false;"));
    assert!(output.contains("# budget_violation=row 1: open: first_submit_ms="));
    assert!(output.contains("# budget_violation=row 1: open: ready_submit_ms="));
    assert_eq!(run.row_count, 71);
    assert_eq!(
        visual.read(|cx| run.editor.read(cx).buffer().text().to_string()),
        text
    );
    std::fs::remove_file(report).unwrap();
}

#[gpui_kit::test]
fn diagnostic_timeout_keeps_missing_ready_and_does_not_advance(cx: &mut gpui_kit::TestAppContext) {
    let (mut visual, mut run, report) = diagnostic_run(cx, "abc", "timeout");
    run.current.as_mut().unwrap().start -= Duration::from_millis(2001);
    assert!(!visual.update(|window, cx| run.tick(window, cx)));
    assert_eq!(run.actions.len(), 70);
    assert!(!run.succeeded.get());
    let output = std::fs::read_to_string(&report).unwrap();
    assert!(output.contains("observation timeout (2000 ms)"));
    assert!(output.contains("# completed=false; budget_passed=false; acceptance=false;"));
    let row: Vec<_> = output
        .lines()
        .find(|line| line.starts_with("open,"))
        .unwrap()
        .split(',')
        .collect();
    assert_eq!(row[9], "");
    assert_eq!(row[10], "");
    assert_eq!(row.last(), Some(&"failed"));
    std::fs::remove_file(report).unwrap();
}

#[gpui_kit::test]
fn diagnostic_text_mismatch_is_a_hard_stop_with_a_report(cx: &mut gpui_kit::TestAppContext) {
    let (mut visual, mut run, report) = diagnostic_run(cx, "abc", "mismatch");
    visual.run_until_parked();
    visual.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
    let observation = visual.read(|cx| super::capture(run.editor.read(cx)));
    let current = run.current.as_mut().unwrap();
    current.expected = "xyz".into();
    assert!(current.accept(frame(observation.at), Some(observation)));
    assert!(!visual.update(|window, cx| run.tick(window, cx)));
    assert_eq!(run.actions.len(), 70);
    let output = std::fs::read_to_string(&report).unwrap();
    assert!(output.contains("# stop_reason=open: display text mismatch"));
    assert!(!run.succeeded.get());
    std::fs::remove_file(report).unwrap();
}

#[test]
fn diagnostic_defers_budget_checks_but_not_blank_body_or_deadline() {
    let mut measurement = Measurement::new(Step::Open, "abc".into(), false);
    let mut preview = paint(measurement.start);
    preview.ready = false;
    preview.indexed = 1;
    measurement.accept(frame(measurement.start), Some(preview));
    let now = measurement.start + Duration::from_millis(600);
    assert!(measurement.hard_error(now, true).is_none());
    assert!(
        measurement
            .performance_errors(now, true, false)
            .iter()
            .any(|error| error.contains("ready_submit_ms"))
    );
    measurement.blank_frames = 1;
    assert!(measurement.hard_error(now, true).unwrap().contains("blank"));
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
