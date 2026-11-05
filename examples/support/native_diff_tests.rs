use super::{Probe, Run, workload};
use gpui_kit::{AppContext, TestAppContext, VisualTestContext, px, size};
use semacs::app::AppShell;
use semacs::buffer::Buffer;
use semacs::files::{DecodedFile, FileFormat};
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::rc::Rc;

#[path = "diff_test_fixture.rs"]
mod fixture;

fn harness(cx: &mut TestAppContext, diagnostic: bool) -> (VisualTestContext, Run, PathBuf) {
    cx.update(semacs::app::init);
    let directory = fixture::create();
    let paints = Rc::new(RefCell::new(VecDeque::new()));
    let views = Rc::new(RefCell::new(Vec::new()));
    let pp = paints.clone();
    let pv = views.clone();
    let buffer = workload::buffer(&directory);
    let handle = cx.open_window(size(px(960.), px(640.)), |window, cx| {
        let shell = cx.new(|cx| AppShell::new(buffer, window, cx));
        pv.borrow_mut().push(shell.read(cx).active_view().clone());
        let subscription = cx.observe(&shell, |_, _, cx| cx.notify());
        Probe {
            shell,
            views: pv,
            paints: pp,
            _subscription: subscription,
        }
    });
    let probe = handle.entity(cx).unwrap();
    let shell = probe.read_with(cx, |probe, _| probe.shell.clone());
    let mut visual = VisualTestContext::from_window(handle.into(), cx);
    visual.update(|window, cx| window.draw(cx).clear(cx));
    let report = directory.join("native.csv");
    let run = visual.update(|_, cx| {
        let mut work = workload::Workload::new(shell, directory.clone(), &report, "native", cx);
        work.context(&format!(
            "diagnostic={diagnostic}; acceptance=false; TestPlatform synthetic journal timings"
        ));
        Run {
            work,
            views,
            paints,
            reading: None,
            collector: cx.foreground_journal().collector(),
            succeeded: Rc::new(Cell::new(false)),
            boot: false,
            prepared: false,
            diagnostic,
            budget_passed: true,
            frames: super::FrameLog::new(
                &report,
                std::time::Instant::now() - std::time::Duration::from_secs(1),
            ),
            setup_started: std::time::Instant::now(),
        }
    });
    (visual, run, directory)
}

fn frame(
    scene: &workload::Scene,
    duration: std::time::Duration,
) -> gpui_kit::profiler::journal::PresentedFrame {
    let mut frame = crate::tests::frame(scene.at - duration);
    frame.frame.draw_end = scene.at;
    frame.presentation.present_start = scene.at;
    frame.presentation.present_end = scene.at;
    frame
}

#[gpui_kit::test]
fn diagnostic_finishes_all_real_actions_after_slow_setup_draw_scroll_and_idle(
    cx: &mut TestAppContext,
) {
    use std::time::Duration;
    let (mut visual, mut run, directory) = harness(cx, true);
    let first = run.paints.borrow().back().unwrap().clone();
    run.accept_frame(frame(&first, Duration::from_millis(20)), Some(first))
        .unwrap();
    assert!(!run.budget_passed);
    let mut idle = std::collections::HashSet::new();
    let mut iterations = 0;
    loop {
        iterations += 1;
        assert!(
            iterations < 1000,
            "controller did not finish; {}",
            directory.display()
        );
        if !visual.update(|window, cx| run.advance(window, cx)) {
            break;
        }
        let phase = run.reading.as_ref().map(|r| (r.step, r.phase));
        if let Some((step, "idle")) = phase
            && !idle.insert(step)
        {
            continue;
        }
        let slow_draw = phase.is_some_and(|(step, phase)| {
            phase == "collapse" && step % workload::ACTIONS.len() == 2
        });
        let duration = if slow_draw {
            Duration::from_millis(20)
        } else {
            Duration::from_micros(1)
        };
        std::thread::sleep(if slow_draw {
            Duration::from_millis(21)
        } else if phase.is_some_and(|(_, phase)| phase == "scroll") {
            Duration::from_millis(6)
        } else {
            Duration::from_millis(1)
        });
        visual.run_until_parked();
        visual.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
        let scene = run.paints.borrow().back().unwrap().clone();
        run.accept_frame(frame(&scene, duration), Some(scene))
            .unwrap();
    }
    assert!(run.work.done() && run.succeeded.get());
    assert!(!run.budget_passed);
    let report = std::fs::read_to_string(directory.join("native.csv")).unwrap();
    assert!(report.contains("# completed=true") && report.contains("# budget_passed=false"));
    assert!(report.contains("diagnostic=true; acceptance=false"));
    assert_eq!(
        report.lines().filter(|line| !line.starts_with('#')).count(),
        27
    );
    assert!(report.matches(",over-budget,").count() >= 6);
    assert!(report.contains("unstaged,unsplit,25"));
    let frames = std::fs::read_to_string(directory.join("native.frames.csv")).unwrap();
    assert!(frames.contains("window-open,setup") && frames.contains("command-prepare,setup"));
    assert!(frames.contains("draw_cpu_ms=20.000000 >16") && frames.contains("idle_draw"));
    assert!(frames.contains("# completed=true"));
    let validation = std::process::Command::new("python3")
        .args([
            "-B",
            "-c",
            r#"
import csv, pathlib, sys
sys.path.insert(0, sys.argv[2])
from perf_diff import read_frames
report = pathlib.Path(sys.argv[1])
with report.open() as source:
    rows = list(csv.DictReader(line for line in source if not line.startswith('#')))
assert read_frames(report.with_suffix('.frames.csv'), rows, True)['budget_passed'] is False
"#,
        ])
        .arg(directory.join("native.csv"))
        .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scripts"))
        .output()
        .unwrap();
    assert!(
        validation.status.success(),
        "{}",
        String::from_utf8_lossy(&validation.stderr)
    );
    drop(run);
    std::fs::remove_dir_all(directory).unwrap();
}

#[gpui_kit::test]
fn diagnostic_blank_setup_is_still_a_hard_stop(cx: &mut TestAppContext) {
    let (mut visual, mut run, directory) = harness(cx, true);
    let mut scene = run.paints.borrow().back().unwrap().clone();
    let frame = frame(&scene, std::time::Duration::from_micros(1));
    scene.bodies[0].visible = 0;
    let error = run.accept_frame(frame, Some(scene)).unwrap_err();
    assert!(error.contains("blank setup"));
    visual.update(|_, cx| run.stop(Some(error), cx));
    assert_eq!(run.work.step, 0);
    assert!(!run.succeeded.get());
    let csv = std::fs::read_to_string(directory.join("native.csv")).unwrap();
    assert!(csv.contains("# completed=false") && !csv.contains("staged,open,"));
    drop(run);
    std::fs::remove_dir_all(directory).unwrap();
}

#[gpui_kit::test]
fn diagnostic_action_timeout_does_not_advance(cx: &mut TestAppContext) {
    let (mut visual, mut run, directory) = harness(cx, true);
    run.boot = true;
    run.work.step = 6;
    visual.update(|window, cx| run.begin(window, cx));
    run.reading.as_mut().unwrap().start -= std::time::Duration::from_secs(31);
    assert!(!visual.update(|window, cx| run.advance(window, cx)));
    assert_eq!(run.work.step, 6);
    assert!(!run.succeeded.get());
    let csv = std::fs::read_to_string(directory.join("native.csv")).unwrap();
    assert!(csv.contains("observation timeout") && csv.contains("# completed=false"));
    drop(run);
    std::fs::remove_dir_all(directory).unwrap();
}

fn reject_bad_setup_after_boot(cx: &mut TestAppContext, prepared: bool) {
    let (mut visual, mut run, directory) = harness(cx, true);
    let scene = run.paints.borrow().back().unwrap().clone();
    run.accept_frame(
        frame(&scene, std::time::Duration::from_micros(1)),
        Some(scene.clone()),
    )
    .unwrap();
    assert!(run.boot);
    run.prepared = prepared;
    let mut pending = scene.clone();
    pending.bodies[0].ready = false;
    run.accept_frame(
        frame(&pending, std::time::Duration::from_micros(1)),
        Some(pending),
    )
    .unwrap();
    let mut bad = scene;
    bad.bodies[0].exact = false;
    let error = run
        .accept_frame(frame(&bad, std::time::Duration::from_micros(1)), Some(bad))
        .unwrap_err();
    assert!(error.contains("setup display/projection identity"));
    visual.update(|_, cx| run.stop(Some(error), cx));
    assert_eq!(run.work.step, 0);
    assert!(!run.succeeded.get());
    let frames = std::fs::read_to_string(directory.join("native.frames.csv")).unwrap();
    assert!(frames.contains(if prepared {
        "command-prepare"
    } else {
        "between-actions"
    }));
    assert!(frames.contains("# completed=false"));
    drop(run);
    std::fs::remove_dir_all(directory).unwrap();
}

#[gpui_kit::test]
fn diagnostic_bad_command_prepare_projection_is_hard_after_boot(cx: &mut TestAppContext) {
    reject_bad_setup_after_boot(cx, true);
}

#[gpui_kit::test]
fn diagnostic_bad_between_actions_projection_is_hard_after_boot(cx: &mut TestAppContext) {
    reject_bad_setup_after_boot(cx, false);
}

#[gpui_kit::test]
fn strict_mode_stops_slow_setup_with_numeric_frame_evidence(cx: &mut TestAppContext) {
    let (mut visual, mut run, directory) = harness(cx, false);
    let scene = run.paints.borrow().back().unwrap().clone();
    let error = run
        .accept_frame(
            frame(&scene, std::time::Duration::from_millis(20)),
            Some(scene),
        )
        .unwrap_err();
    assert!(error.contains("20.000000 >16"));
    visual.update(|_, cx| run.stop(Some(error), cx));
    assert_eq!(run.work.step, 0);
    assert!(!run.succeeded.get());
    let csv = std::fs::read_to_string(directory.join("native.frames.csv")).unwrap();
    assert!(csv.contains("draw_cpu_ms=20.000000 >16") && csv.contains("# completed=false"));
    drop(run);
    std::fs::remove_dir_all(directory).unwrap();
}

#[gpui_kit::test]
fn diagnostic_setup_timeout_stops_without_starting_actions(cx: &mut TestAppContext) {
    let (mut visual, mut run, directory) = harness(cx, true);
    run.setup_started -= std::time::Duration::from_secs(31);
    assert!(!visual.update(|window, cx| run.advance(window, cx)));
    assert_eq!(run.work.step, 0);
    let csv = std::fs::read_to_string(directory.join("native.csv")).unwrap();
    assert!(csv.contains("setup observation timeout") && csv.contains("# completed=false"));
    drop(run);
    std::fs::remove_dir_all(directory).unwrap();
}

#[gpui_kit::test]
fn diagnostic_comparison_identity_failure_stops_before_next_action(cx: &mut TestAppContext) {
    let (mut visual, mut run, directory) = harness(cx, true);
    let scene = run.paints.borrow().back().unwrap().clone();
    run.accept_frame(
        frame(&scene, std::time::Duration::from_micros(1)),
        Some(scene),
    )
    .unwrap();
    let mut complete = false;
    for _ in 0..200 {
        assert!(visual.update(|window, cx| run.advance(window, cx)));
        std::thread::sleep(std::time::Duration::from_millis(1));
        visual.run_until_parked();
        visual.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
        let scene = run.paints.borrow().back().unwrap().clone();
        run.accept_frame(
            frame(&scene, std::time::Duration::from_micros(1)),
            Some(scene),
        )
        .unwrap();
        if run
            .reading
            .as_ref()
            .is_some_and(workload::Reading::complete)
        {
            complete = true;
            break;
        }
    }
    assert!(complete);
    run.work.fixture = directory.join("wrong-comparison");
    assert!(!visual.update(|window, cx| run.advance(window, cx)));
    assert_eq!(run.work.step, 0);
    let csv = std::fs::read_to_string(directory.join("native.csv")).unwrap();
    assert!(csv.contains("wrong comparison root/kind") && csv.contains("# completed=false"));
    drop(run);
    std::fs::remove_dir_all(directory).unwrap();
}

#[gpui_kit::test]
fn first_paint_precedes_driver_setup_and_observes_split_mounts(cx: &mut TestAppContext) {
    cx.update(semacs::app::init);
    let paints = Rc::new(RefCell::new(VecDeque::new()));
    let views = Rc::new(RefCell::new(Vec::new()));
    let probe_paints = paints.clone();
    let probe_views = views.clone();
    let buffer = Buffer::new(
        None,
        DecodedFile {
            text: "source body\n".repeat(100),
            format: FileFormat::default(),
        },
    );
    let window = cx.open_window(size(px(960.), px(640.)), |window, cx| {
        let shell = cx.new(|cx| AppShell::new(buffer, window, cx));
        probe_views
            .borrow_mut()
            .push(shell.read(cx).active_view().clone());
        let subscription = cx.observe(&shell, |_, _, cx| cx.notify());
        Probe {
            shell,
            views: probe_views,
            paints: probe_paints,
            _subscription: subscription,
        }
    });
    let probe = window.entity(cx).unwrap();
    let shell = probe.read_with(cx, |probe, _| probe.shell.clone());
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.update(|window, cx| window.draw(cx).clear(cx));
    let initial = paints
        .borrow()
        .back()
        .expect("capture before any driver exists")
        .clone();
    assert_eq!(initial.bodies.len(), 1);
    assert_eq!(initial.selected, initial.bodies[0].view);
    assert!(initial.bodies[0].exact && initial.bodies[0].visible > 0);

    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
        ".pi/tmp/native-diff-first-paint-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&fixture).unwrap();
    for kind in ["staged", "unstaged"] {
        std::fs::write(
            fixture.join(format!("{kind}.expected")),
            "stat\n@@ first\n-hidden\n@@ next\n+visible\n",
        )
        .unwrap();
        std::fs::write(fixture.join(format!("{kind}.projection")), "2 3\n").unwrap();
    }
    let mut run = visual.update(|_, cx| Run {
        work: workload::Workload::new(
            shell,
            fixture.clone(),
            &fixture.join("probe.csv"),
            "native",
            cx,
        ),
        views,
        reading: None,
        collector: cx.foreground_journal().collector(),
        paints: paints.clone(),
        succeeded: Rc::new(Cell::new(false)),
        boot: false,
        prepared: false,
        diagnostic: false,
        budget_passed: true,
        frames: super::FrameLog::new(&fixture.join("probe.csv"), initial.at),
        setup_started: initial.at,
    });
    for (step, expected_views) in [(6, 2), (12, 1)] {
        run.work.step = step;
        visual.update(|window, cx| {
            run.begin(window, cx);
        });
        assert_eq!(run.views.borrow().len(), expected_views);
        visual.run_until_parked();
        visual.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
        let observed = paints.borrow().back().unwrap().clone();
        assert_eq!(observed.bodies.len(), expected_views);
        assert_eq!(
            observed.selected,
            visual.read(|cx| run.work.shell.read(cx).active_view().entity_id())
        );
        for body in &observed.bodies {
            assert!(body.exact && body.visible > 0);
            assert!(
                run.work
                    .views
                    .iter()
                    .any(|view| view.entity_id() == body.view)
            );
        }
        run.reading.take();
    }
    drop(run);
    std::fs::remove_dir_all(fixture).unwrap();
}
