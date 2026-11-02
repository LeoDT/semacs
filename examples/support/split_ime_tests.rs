use super::{INITIAL, ONCE, PHASES, State, TWICE, ViewState};
use gpui_kit::{Bounds, point, px, size};
use semacs::buffer::Buffer;

fn state(step: usize) -> State {
    let snapshot = Buffer::scratch().snapshot_id();
    let text = if step < 5 {
        INITIAL
    } else if step < 10 {
        ONCE
    } else {
        TWICE
    };
    let first = ViewState {
        snapshot,
        text: text.into(),
        point: if step < 10 { 0 } else { 3 },
        marked: PHASES[step] == "left-preedit",
        marked_text: "ni".into(),
        ready: true,
        focused: step == 0 || (6..=10).contains(&step) || step >= 16,
        caret_in_target: true,
        bounds: Some(Bounds::new(point(px(8.), px(8.)), size(px(200.), px(200.)))),
    };
    let second = (step > 0).then(|| ViewState {
        snapshot,
        text: text.into(),
        point: if step < 5 {
            0
        } else if step < 10 {
            3
        } else {
            6
        },
        marked: PHASES[step] == "right-preedit",
        marked_text: "ni".into(),
        ready: true,
        focused: (1..=5).contains(&step),
        caret_in_target: true,
        bounds: Some(Bounds::new(
            point(px(225.), px(8.)),
            size(px(200.), px(200.)),
        )),
    });
    let mini = (11..=15).contains(&step).then(|| {
        (
            ViewState {
                snapshot: Buffer::scratch().snapshot_id(),
                text: String::new(),
                point: 0,
                marked: PHASES[step] == "mini-preedit",
                marked_text: "ni".into(),
                ready: true,
                focused: true,
                caret_in_target: true,
                bounds: Some(Bounds::new(
                    point(px(8.), px(240.)),
                    size(px(400.), px(24.)),
                )),
            },
            if step == 15 {
                "你".into()
            } else {
                String::new()
            },
        )
    });
    State {
        first,
        second,
        mini,
        selected_first: step == 0 || step >= 6,
    }
}

#[test]
fn each_phase_requires_its_own_target_state() {
    for step in 0..PHASES.len() {
        assert!(state(step).matches(step), "{}", PHASES[step]);
        let mut invalid = state(step);
        invalid.first.text.push('!');
        assert!(!invalid.matches(step), "wrong committed body must fail");
        let mut invalid = state(step);
        invalid.first.ready = false;
        assert!(!invalid.matches(step), "unready layouts must fail");
    }
}

#[test]
fn wrong_target_preedit_or_geometry_does_not_pass() {
    let mut right = state(2);
    right.first.marked = true;
    assert!(!right.matches(2));
    right.first.marked = false;
    right.second.as_mut().unwrap().caret_in_target = false;
    assert!(!right.matches(2));
    let mut mini = state(12);
    mini.mini.as_mut().unwrap().0.focused = false;
    assert!(!mini.matches(12));
    mini.mini.as_mut().unwrap().0.focused = true;
    mini.mini.as_mut().unwrap().0.caret_in_target = false;
    assert!(!mini.matches(12));
}

#[test]
fn split_views_must_share_identity_and_restore_the_original_point() {
    let mut split = state(1);
    split.second.as_mut().unwrap().snapshot = Buffer::scratch().snapshot_id();
    assert!(!split.matches(1));
    let mut restored = state(16);
    restored.first.point = 0;
    assert!(!restored.matches(16));
    let mut saved = state(17);
    saved.mini = state(15).mini;
    assert!(!saved.matches(17));
}
