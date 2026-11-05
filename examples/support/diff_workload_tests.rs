use super::{Body, ProjectionOracle, Reading, Scene};
use gpui_kit::EntityId;
use semacs::buffer::Buffer;
use semacs::preparation_trace::Capture;
use std::time::{Duration, Instant};

fn body(id: u64, buffer: &Buffer) -> Body {
    let display = semacs::input::InputSession::new(buffer)
        .display_snapshot(buffer)
        .unwrap()
        .id();
    Body {
        view: EntityId::from(id),
        display,
        snapshot: buffer.snapshot_id(),
        kind: "*magit-diff staged: test*".into(),
        exact: true,
        ready: true,
        visible: 10,
        rows: 100,
        bytes: 1000,
        collapsed: false,
        point: 10,
        hunk: 10,
        scroll: 120.,
        width: 480.,
        height: 600.,
    }
}

fn reading(phase: &'static str, buffer: &Buffer) -> Reading {
    let capture = Capture::default();
    let recording = capture.enter();
    Reading {
        step: 0,
        kind: "staged",
        phase,
        start: Instant::now(),
        action_ms: 0.,
        first: None,
        selected_ready: None,
        all_ready: None,
        max_draw: 0.,
        frames: 0,
        missing: 0,
        blank: 0,
        idle_ticks: 0,
        last: None,
        observed: None,
        selected: EntityId::from(1),
        targets: vec![EntityId::from(1), EntityId::from(2)],
        before: buffer.snapshot_id(),
        before_point: 0,
        before_scroll: 0.,
        collapsed: phase == "refresh",
        capture,
        _recording: recording,
    }
}

fn observe(reading: &mut Reading, bodies: Vec<Body>) {
    reading.accepts(
        reading.start + Duration::from_millis(1),
        1.,
        Some(Scene {
            at: reading.start,
            selected: EntityId::from(1),
            bodies,
        }),
    );
}

#[test]
fn independent_projection_oracle_removes_only_prescribed_hunk_body() {
    let oracle = ProjectionOracle::new(
        "title",
        "stat\n@@ first\n+hidden\n@@ next\n+visible\n",
        2..3,
    );
    assert_eq!(oracle.expanded.len(), 7);
    assert_eq!(oracle.collapsed.len(), 6);
    assert!(!oracle.collapsed.contains(&oracle.expanded[3]));
    for i in [0, 1, 2, 4, 5, 6] {
        assert!(oracle.collapsed.contains(&oracle.expanded[i]));
    }
    assert_ne!(oracle.expanded, oracle.collapsed);
}

#[test]
fn selected_and_all_ready_require_the_same_scene() {
    let buffer = Buffer::scratch();
    let mut r = reading("split", &buffer);
    let a = body(1, &buffer);
    let mut b = body(2, &buffer);
    b.ready = false;
    observe(&mut r, vec![a.clone(), b.clone()]);
    assert!(r.selected_ready.is_some());
    assert!(r.all_ready.is_none());
    let mut pending = a.clone();
    pending.ready = false;
    b.ready = true;
    observe(&mut r, vec![pending, b.clone()]);
    assert!(r.all_ready.is_none());
    observe(&mut r, vec![a, b]);
    assert!(r.complete());
}

#[test]
fn stale_projection_or_old_refresh_cannot_complete() {
    let buffer = Buffer::scratch();
    for phase in ["collapse", "refresh", "locate", "navigate", "scroll"] {
        let mut r = reading(phase, &buffer);
        let mut a = body(1, &buffer);
        let mut b = body(2, &buffer);
        match phase {
            "collapse" => a.exact = false,
            "refresh" => {
                a.collapsed = true;
                b.collapsed = true;
            }
            "locate" => a.point += 1,
            "navigate" => a.point = 0,
            "scroll" => a.scroll = 0.,
            _ => unreachable!(),
        }
        observe(&mut r, vec![a, b]);
        assert!(!r.complete(), "{phase}");
    }
}

#[test]
fn wrong_comparison_or_peer_snapshot_cannot_complete() {
    let buffer = Buffer::scratch();
    let other = Buffer::scratch();
    for wrong_kind in [false, true] {
        let mut r = reading("split", &buffer);
        let a = body(1, &buffer);
        let mut b = body(2, if wrong_kind { &buffer } else { &other });
        if wrong_kind {
            b.kind = "*magit-diff unstaged: test*".into();
        }
        observe(&mut r, vec![a, b]);
        assert!(!r.complete());
    }
}

#[test]
fn missing_duplicate_blank_and_idle_draw_fail_closed() {
    let buffer = Buffer::scratch();
    for mutation in 0..4 {
        let mut r = reading(if mutation == 3 { "idle" } else { "split" }, &buffer);
        let a = body(1, &buffer);
        let mut b = body(2, &buffer);
        match mutation {
            0 => b.view = a.view,
            1 => b.visible = 0,
            2 => {
                r.accepts(r.start, 1., None);
            }
            _ => {}
        }
        observe(&mut r, vec![a, b]);
        assert!(r.error(false).is_some());
    }
}

#[test]
fn native_draw_and_scroll_budgets_do_not_apply_to_new_diff_latency() {
    let buffer = Buffer::scratch();
    let mut r = reading("open", &buffer);
    r.start -= Duration::from_secs(1);
    assert!(
        r.error(true).is_none(),
        "diff open has no new latency budget"
    );
    r.max_draw = 16.001;
    assert!(r.error(true).is_some());
    assert!(r.error(false).is_none());
    let mut r = reading("scroll", &buffer);
    r.start -= Duration::from_millis(5);
    assert!(r.error(true).is_some());
}
