use super::*;
use crate::tests::{frame, paint, stamp};
use gpui_kit::{TestAppContext, VisualTestContext};

fn reading(step: Step) -> Reading {
    Reading::new(step, Instant::now(), stamp(), 3, false)
}

fn observed(reading: &Reading, at: Instant, ready: bool) -> SyntaxPaint {
    SyntaxPaint {
        body: PaintState {
            stamp: reading.target,
            ..paint(at)
        },
        syntax: ready.then_some(reading.target.display.committed),
        syntax_ready: ready,
    }
}

#[test]
fn layout_budget_is_not_replaced_with_multilayer_syntax_latency() {
    let mut reading = reading(Step::BurstDelete);
    let start = reading.start;
    reading.accept(frame(start), Some(observed(&reading, start, false)));
    assert!(!reading.complete());
    assert!(reading.layout.is_some());
    assert!(
        reading
            .error(start + Duration::from_secs(2), true)
            .is_none()
    );
    let later = start + Duration::from_secs(2);
    reading.accept(frame(later), Some(observed(&reading, later, true)));
    assert!(reading.complete());
    assert_eq!(elapsed(reading.layout, start, later), 2.);
    assert!((elapsed(reading.syntax, start, later) - 2002.).abs() < 0.000001);
}

#[test]
fn early_layout_may_advance_without_claiming_syntax_was_ready() {
    let mut reading = reading(Step::Insert);
    let start = reading.start;
    reading.accept(frame(start), Some(observed(&reading, start, false)));
    assert!(reading.complete());
    assert_eq!(reading.pending_frames, 1);
    assert!(reading.syntax.is_none());
}

#[test]
fn target_identity_and_same_frame_observations_are_fail_closed() {
    let mut reading = reading(Step::Insert);
    let start = reading.start;
    reading.accept(frame(start), None);
    assert!(reading.error(start, true).unwrap().contains("same-frame"));
    let mut reading = super::tests::reading(Step::Insert);
    let start = reading.start;
    let mut wrong = observed(&reading, start, true);
    wrong.syntax = Some(stamp().display.committed);
    reading.accept(frame(start), Some(wrong));
    assert!(
        reading
            .error(start, true)
            .unwrap()
            .contains("syntax snapshot")
    );
}

#[test]
fn painted_wrong_target_is_distinguished_from_missing_paint() {
    let mut reading = reading(Step::Newline);
    let start = reading.start;
    let mut wrong = observed(&reading, start, false);
    wrong.body.bytes += 1;
    reading.accept(frame(start), Some(wrong));
    assert_eq!(reading.missing, 1);
    assert!(
        reading
            .error(start, false)
            .unwrap()
            .contains("paint observed but target")
    );
}

#[test]
fn deadlines_fail_before_moving_to_a_followup_action() {
    let mut reading = reading(Step::Insert);
    let start = reading.start;
    assert!(
        reading
            .error(start + Duration::from_millis(17), true)
            .unwrap()
            .contains("first_submit_ms")
    );
    assert!(
        reading
            .error(start + Duration::from_millis(17), false)
            .is_none()
    );
    reading.action_ms = 2.001;
    assert!(
        reading
            .error(start, true)
            .unwrap()
            .contains("action_cpu_ms")
    );
    let mut reading = super::tests::reading(Step::Insert);
    let start = reading.start;
    let mut bad = observed(&reading, start, true);
    bad.body.ready = false;
    bad.body.indexed = 1;
    reading.accept(frame(start), Some(bad));
    assert!(
        reading
            .error(start + Duration::from_millis(301), true)
            .unwrap()
            .contains("ready_submit_ms")
    );
}

#[test]
fn idle_and_initial_visibility_remain_strict() {
    let mut idle = reading(Step::Idle);
    let start = idle.start;
    idle.accept(frame(start), Some(observed(&idle, start, true)));
    assert!(idle.error(start, true).unwrap().contains("idle produced"));
    let mut open = reading(Step::Open);
    let start = open.start;
    let mut blank = observed(&open, start, true);
    blank.body.visible = 0;
    open.accept(frame(start), Some(blank));
    assert!(open.error(start, true).unwrap().contains("blank"));
    assert_eq!(phases().len(), 38);
}

#[gpui_kit::test]
fn actual_syntax_fixture_enables_background_parsing_and_tracks_current_snapshots(
    cx: &mut TestAppContext,
) {
    for (sample, path, unit) in [
        (
            "markdown",
            "sample.md",
            "\x60\x60\x60html\n<script>const n = call(42);</script><style>a { width: 12px; }</style>\n\x60\x60\x60\n",
        ),
        (
            "html",
            "sample.html",
            "<script>const n = call(42);</script><style>a { width: 12px; }</style>\n",
        ),
    ] {
        let text = unit.repeat((256_usize * 1024).div_ceil(unit.len()));
        let (document, offset) = buffer(path.into(), sample, &text);
        assert_eq!(offset, text.find("call(42)").unwrap());
        cx.update(semacs::app::init);
        let window = cx.open_window(size(px(960.), px(640.)), |window, cx| {
            AppShell::new(document, window, cx)
        });
        let shell = window.entity(cx).unwrap();
        let view = shell.read_with(cx, |shell, _| shell.active_view().clone());
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        for _ in 0..3 {
            visual.run_until_parked();
            visual.update(|window, cx| window.draw(cx).clear(cx));
        }
        visual.read(|cx| {
            let state = observe(view.read(cx));
            assert!(state.syntax_ready);
            assert_eq!(state.syntax, Some(state.body.stamp.display.committed));
            assert_eq!(state.body.stamp.point, offset);
        });
        visual.update(|window, cx| window.dispatch_keystroke(Keystroke::parse("x").unwrap(), cx));
        visual.read(|cx| {
            assert!(view.read(cx).syntax_snapshot().is_none());
            assert_eq!(view.read(cx).buffer().text().len(), text.len() + 1);
        });
        for _ in 0..3 {
            visual.run_until_parked();
            visual.update(|window, cx| window.draw(cx).clear(cx));
        }
        visual.read(|cx| {
            let state = observe(view.read(cx));
            assert_eq!(state.syntax, Some(state.body.stamp.display.committed));
        });
    }
}

#[gpui_kit::test]
fn complete_real_ret_workload_checks_full_text_point_and_final_whitespace_removal(
    cx: &mut TestAppContext,
) {
    for (sample, path, unit) in [
        (
            "markdown",
            "full.md",
            "# 中🙂\n\n```html\n<script>const n = call(42);</script><style>a { width: 12px; }</style>\n```\n",
        ),
        (
            "html",
            "full.html",
            "<script>const n = call(42);</script><style>a { width: 12px; }</style>\n",
        ),
    ] {
        let text = unit.repeat((256_usize * 1024).div_ceil(unit.len()));
        let (document, offset) = buffer(path.into(), sample, &text);
        cx.update(semacs::app::init);
        let window = cx.open_window(size(px(960.), px(640.)), |window, cx| {
            AppShell::new(document, window, cx)
        });
        let shell = window.entity(cx).unwrap();
        let view = shell.read_with(cx, |shell, _| shell.active_view().clone());
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let mut expected = text.clone();
        let mut point = offset;
        for step in phases().into_iter().filter(|step| is_input(*step)) {
            advance_expected(&mut expected, &mut point, step);
            let (key, count) = match step {
                Step::Insert => ("x", 1),
                Step::Newline => ("enter", 1),
                Step::BurstInsert => ("x", 32),
                Step::BurstDelete => ("backspace", 32),
                _ => ("backspace", 1),
            };
            visual.update(|window, cx| {
                for _ in 0..count {
                    window.dispatch_keystroke(Keystroke::parse(key).unwrap(), cx);
                }
                let view = view.read(cx);
                assert_eq!(
                    view.buffer().text().to_string(),
                    expected,
                    "{}",
                    step.name()
                );
                assert_eq!(view.buffer().point().0, point, "{}", step.name());
                let delta = match step {
                    Step::Insert => 1,
                    Step::Delete | Step::Newline => 0,
                    Step::BurstInsert => 31,
                    _ => -1,
                };
                assert_eq!(
                    expected.len(),
                    text.len().checked_add_signed(delta).unwrap()
                );
                assert_eq!(point, offset.checked_add_signed(delta).unwrap());
            });
        }
        assert!(final_matches(&expected, point, &text, offset));
        assert!(!final_matches(&text, offset, &text, offset));
        let mut corrupted = expected.clone();
        corrupted.replace_range(0..1, "!");
        assert!(!final_matches(&corrupted, point, &text, offset));
    }
}

#[test]
fn embedded_input_offset_ignores_same_named_plain_text_before_the_script() {
    for (sample, extension, context) in [
        (
            "markdown",
            "md",
            "\x60\x60\x60html\n<script>const n = call(42);</script><style>.box { width: 12px; }</style>\n\x60\x60\x60\n",
        ),
        (
            "html",
            "html",
            "<script>const n = call(42);</script><style>.box { width: 12px; }</style>\n",
        ),
    ] {
        let unit = format!("call(42)\n{context}");
        let text = unit.repeat((256_usize * 1024).div_ceil(unit.len()));
        let (buffer, offset) = buffer(format!("sample.{extension}").into(), sample, &text);
        assert!(offset > text.find("call(42)").unwrap());
        assert_eq!(
            &text[offset - "<script>const n = ".len()..offset],
            "<script>const n = "
        );
        assert_eq!(buffer.point().0, offset);
    }
}
