use super::{Body, Phase, Probe, Reading, Run, Scene, Target};
use crate::tests::{frame, paint};
use gpui_kit::{AppContext, EntityId, TestAppContext, VisualTestContext, px, size};
use semacs::app::AppShell;
use semacs::buffer::Buffer;
use semacs::files::{DecodedFile, FileFormat};
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::fs::File;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

fn scene(at: Instant) -> Scene {
    let p = paint(at);
    Scene {
        at,
        selected: EntityId::from(1),
        bodies: vec![
            Body {
                id: EntityId::from(1),
                paint: p,
                font_size: 16.,
            },
            Body {
                id: EntityId::from(2),
                paint: p,
                font_size: 16.,
            },
        ],
    }
}

fn reading() -> Reading {
    let now = Instant::now();
    let scene = scene(now);
    Reading::new(
        Phase::Insert,
        now,
        scene.selected,
        scene
            .bodies
            .iter()
            .map(|b| Target {
                id: b.id,
                stamp: b.paint.stamp,
                bytes: b.paint.bytes,
                max_width: None,
                max_height: None,
                font_size: 16.,
                scroll: None,
            })
            .collect(),
    )
}

#[test]
fn selected_ready_does_not_imply_same_frame_all_ready() {
    let mut r = reading();
    let mut s = scene(r.start);
    for body in &mut s.bodies {
        body.paint.stamp = r.targets[0].stamp;
    }
    s.bodies[1].paint.ready = false;
    r.accept(frame(r.start), Some(s.clone()));
    assert!(r.ready.is_some());
    assert!(r.all_ready.is_none());
    let mut s2 = s.clone();
    s2.bodies[0].paint.ready = false;
    s2.bodies[1].paint.ready = true;
    r.accept(frame(r.start + Duration::from_millis(3)), Some(s2));
    assert!(
        r.all_ready.is_none(),
        "must not combine readiness from different frames"
    );
    s.bodies[1].paint.ready = true;
    r.accept(frame(r.start + Duration::from_millis(6)), Some(s));
    assert!(r.complete());
    assert!(
        r.ready.unwrap().presentation.present_end < r.all_ready.unwrap().presentation.present_end
    );
}

#[test]
fn stale_display_point_and_incompatible_layout_cannot_complete_peer() {
    for mutate in 0..5 {
        let mut r = reading();
        let mut s = scene(r.start);
        for body in &mut s.bodies {
            body.paint.stamp = r.targets[0].stamp;
        }
        let b = &mut s.bodies[1];
        match mutate {
            0 => b.paint.stamp = crate::tests::stamp(),
            1 => b.paint.stamp.point += 1,
            2 => b.paint.compatible = false,
            3 => b.paint.indexed = 1,
            _ => b.paint.marked = true,
        }
        r.accept(frame(r.start), Some(s));
        assert!(r.ready.is_some());
        assert!(r.all_ready.is_none());
    }
}

#[test]
fn missing_duplicate_wrong_selection_and_blank_observations_fail_even_in_software() {
    for mutate in 0..5 {
        let mut r = reading();
        let mut s = scene(r.start);
        match mutate {
            0 => {
                s.bodies.pop();
            }
            1 => s.bodies[1].id = s.bodies[0].id,
            2 => s.selected = EntityId::from(2),
            3 => s.bodies[1].paint.visible = 0,
            _ => {}
        }
        r.accept(frame(r.start), if mutate == 4 { None } else { Some(s) });
        assert!(r.error(r.start, false).is_some());
    }
}

#[test]
fn old_frames_are_ignored_and_actual_geometry_font_scroll_are_required() {
    let mut r = reading();
    r.accept(frame(r.start - Duration::from_secs(1)), None);
    assert_eq!(r.frames, 0);
    let mut s = scene(r.start);
    for body in &mut s.bodies {
        body.paint.stamp = r.targets[0].stamp;
    }
    r.targets[0].max_width = Some(944.);
    r.targets[0].max_height = Some(576.);
    r.targets[0].font_size = 18.;
    r.targets[0].scroll = Some(0.);
    for step in 0..5 {
        if step == 1 {
            s.bodies[0].paint.width = 700.;
        }
        if step == 2 {
            s.bodies[0].paint.height = 400.;
        }
        if step == 3 {
            s.bodies[0].font_size = 18.;
        }
        if step == 4 {
            s.bodies[0].paint.scroll = 120.;
        }
        r.accept(frame(r.start), Some(s.clone()));
        assert_eq!(r.complete(), step == 4);
    }
}

#[test]
fn selected_limits_are_preserved_without_an_all_ready_threshold() {
    let mut r = reading();
    let mut s = scene(r.start);
    for body in &mut s.bodies {
        body.paint.stamp = r.targets[0].stamp;
    }
    s.bodies[1].paint.ready = false;
    r.accept(frame(r.start), Some(s.clone()));
    let now = r.start + Duration::from_millis(350);
    assert!(
        r.error(now, true).is_none(),
        "peer delay is an un-gated baseline"
    );
    s.bodies[1].paint.ready = true;
    r.accept(frame(now), Some(s));
    assert!(r.complete());
    r.action_ms = 2.001;
    assert!(r.error(now, true).unwrap().contains("action_cpu_ms"));
    assert!(r.error(now, false).is_none());

    let r = reading();
    assert!(
        r.error(r.start + Duration::from_millis(17), true)
            .unwrap()
            .contains("first_submit_ms")
    );
    assert!(
        r.error(r.start + Duration::from_secs(31), false)
            .unwrap()
            .contains("timeout")
    );
}

#[test]
fn idle_requires_thirty_ticks_and_no_draw() {
    let mut r = reading();
    r.phase = Phase::Idle;
    r.idle_ticks = 29;
    assert!(!r.complete());
    r.idle_ticks = 30;
    assert!(r.complete());
    r.accept(frame(r.start), None);
    assert!(r.error(r.start, false).is_some());
}

#[gpui_kit::test]
fn production_probe_actions_and_capture_work_for_both_split_directions(cx: &mut TestAppContext) {
    for direction in ["right", "below"] {
        cx.update(semacs::app::init);
        let text = "e\u{301}🙂 alpha beta gamma\n".repeat(2000);
        let buffer = Buffer::new(
            None,
            DecodedFile {
                text: text.clone(),
                format: FileFormat::default(),
            },
        );
        let paints = Rc::new(RefCell::new(VecDeque::new()));
        let views = Rc::new(RefCell::new(Vec::new()));
        let pp = paints.clone();
        let pv = views.clone();
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
        let collector = visual.update(|_, cx| cx.foreground_journal().collector());
        let path = PathBuf::from(format!(
            ".pi/tmp/c-native-probe-test-{direction}-{}.csv",
            std::process::id()
        ));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let output = File::create(&path).unwrap();
        let mut run = Run {
            shell,
            views,
            paints,
            collector,
            output,
            report: path.clone(),
            hardware: false,
            succeeded: Rc::new(Cell::new(false)),
            direction: direction.into(),
            current: None,
            phases: VecDeque::new(),
            original_bytes: text.len(),
            peer_origin: text.chars().next().unwrap().len_utf8(),
            expected: text,
            peer_scroll: None,
            last_scene: None,
        };
        for phase in std::iter::once(Phase::Boot)
            .chain(super::phases())
            .filter(|phase| *phase != Phase::Idle)
        {
            visual.update(|window, cx| run.begin(phase, window, cx));
            if phase == Phase::Resize {
                visual.simulate_resize(size(px(720.), px(640.)));
            }
            for _ in 0..8 {
                visual.run_until_parked();
                visual.update(|window, cx| {
                    window.refresh();
                    window.draw(cx).clear(cx);
                });
                if visual.read(|cx| {
                    run.views
                        .borrow()
                        .iter()
                        .all(|view| view.read(cx).layout_ready())
                }) {
                    break;
                }
            }
            let at = run
                .paints
                .borrow()
                .back()
                .expect("probe canvas must capture")
                .at;
            let mut r = run.current.take().unwrap();
            let f = frame(at);
            let scene = crate::take_paint(&mut run.paints.borrow_mut(), &f, |scene| scene.at);
            r.accept(f, scene);
            assert!(
                r.complete(),
                "{direction}: {phase:?} did not observe exact ready views"
            );
            visual
                .read(|cx| run.verify(&r, cx))
                .unwrap_or_else(|error| {
                    panic!(
                        "{direction} {phase:?}: {error}; points={:?}, origin={}",
                        visual.read(|cx| run
                            .views
                            .borrow()
                            .iter()
                            .map(|v| v.read(cx).buffer().point())
                            .collect::<Vec<_>>()),
                        run.peer_origin
                    )
                });
            run.write_row(&r, "ready");
            run.last_scene = r.scene;
        }
        visual.update(|window, _| window.remove_window());
        drop(run);
        std::fs::remove_file(path).unwrap();
    }
}
