use gpui_kit::{KeyDownEvent, Keystroke};
use semacs::buffer::{Buffer, EditError, EditSource};
use semacs::command::Command;
use semacs::editing::{CommandOutcome, EditTarget, KillRing, RouteError};
use semacs::files::{DecodedFile, FileFormat};
use semacs::input::InputError;
use semacs::keymap::Keymap;
use semacs::text::{ByteOffset, OffsetUtf16};

fn target(text: &str, ring: &KillRing) -> EditTarget {
    EditTarget::with_kill_ring(
        Buffer::new(
            None,
            DecodedFile {
                text: text.into(),
                format: FileFormat::default(),
            },
        ),
        ring.clone(),
    )
}

fn run(target: &mut EditTarget, command: Command) -> CommandOutcome {
    target.execute(target.token(), command).unwrap()
}

fn at(target: &mut EditTarget, point: usize) {
    target
        .set_point_from_pointer(target.token(), ByteOffset(point))
        .unwrap();
}

fn region(target: &mut EditTarget, mark: usize, point: usize) {
    at(target, mark);
    run(target, Command::SetMark);
    at(target, point);
}

fn entries(ring: &KillRing) -> Vec<String> {
    ring.entries().iter().map(ToString::to_string).collect()
}

fn press(target: &mut EditTarget, map: &Keymap, key: &'static str) {
    target
        .key_down(
            target.token(),
            &KeyDownEvent {
                keystroke: Keystroke::parse(key).unwrap().with_simulated_ime(),
                is_held: false,
                prefer_character_input: false,
            },
            &[map],
        )
        .unwrap();
}

#[test]
fn kill_line_handles_text_whitespace_newline_and_unicode() {
    for (text, point, remaining, killed) in [
        ("abc\ndef", 0, "\ndef", "abc"),
        ("abc\ndef", 3, "abcdef", "\n"),
        ("  \t\ndef", 0, "def", "  \t\n"),
        ("abc  \ndef", 3, "abcdef", "  \n"),
        ("abc   ", 3, "abc", "   "),
        ("\n\ndef", 0, "\ndef", "\n"),
        ("中🙂\ntail", 3, "中\ntail", "🙂"),
        ("abc\u{a0}\ndef", 3, "abc\ndef", "\u{a0}"),
    ] {
        let ring = KillRing::default();
        let mut target = target(text, &ring);
        at(&mut target, point);
        let update = run(&mut target, Command::KillLine).update.unwrap();
        assert_eq!(target.buffer().text().to_string(), remaining);
        assert_eq!(target.buffer().point(), ByteOffset(point));
        assert_eq!(entries(&ring), [killed]);
        assert_eq!(update.edit.unwrap().source, EditSource::Command);
    }
}

#[test]
fn consecutive_kills_append_but_eob_does_not_add_an_entry_or_edit() {
    let ring = KillRing::default();
    let mut target = target("abc\ndef", &ring);
    run(&mut target, Command::KillLine);
    let first = ring.latest().unwrap();
    run(&mut target, Command::KillLine);
    assert_eq!(entries(&ring), ["abc\n"]);
    assert_eq!(first.to_string(), "abc");
    run(&mut target, Command::KillLine);
    let id = target.buffer().snapshot_id();
    assert_eq!(entries(&ring), ["abc\ndef"]);
    assert_eq!(
        target
            .execute(target.token(), Command::KillLine)
            .unwrap_err(),
        RouteError::EndOfBuffer
    );
    assert_eq!(target.buffer().snapshot_id(), id);
    assert_eq!(entries(&ring), ["abc\ndef"]);
}

#[test]
fn region_direction_controls_prepend_or_append_across_the_shared_command_chain() {
    for (mark, point, expected) in [(0, 2, "oldab"), (2, 0, "abold")] {
        let ring = KillRing::default();
        let mut first = target("old", &ring);
        let mut second = target("abcd", &ring);
        region(&mut second, mark, point);
        run(&mut first, Command::KillLine);
        run(&mut second, Command::KillRegion);
        assert_eq!(entries(&ring), [expected]);
        assert_eq!(second.buffer().text().to_string(), "cd");
        assert_eq!(second.buffer().point(), ByteOffset(0));
        assert_eq!(second.buffer().mark(), Some(ByteOffset(0)));
        assert!(!second.buffer().mark_active());
    }
}

#[test]
fn copy_can_extend_a_kill_but_is_not_itself_a_kill_command() {
    for (mark, point, expected) in [(0, 2, "oldab"), (2, 0, "abold")] {
        let ring = KillRing::default();
        let mut first = target("old", &ring);
        let mut second = target("abcd", &ring);
        region(&mut second, mark, point);
        let id = second.display_snapshot().id();
        run(&mut first, Command::KillLine);
        run(&mut second, Command::KillRingSave);
        assert_eq!(entries(&ring), [expected]);
        assert_eq!(second.display_snapshot().id(), id);
        assert!(!second.buffer().is_modified());
        assert!(!second.buffer().mark_active());
        run(&mut second, Command::KillRingSave);
        assert_eq!(entries(&ring), ["ab", expected]);
        run(&mut second, Command::KillRegion);
        assert_eq!(entries(&ring), ["ab", "ab", expected]);
    }
}

#[test]
fn inactive_marks_still_define_kill_and_copy_regions() {
    for command in [Command::KillRegion, Command::KillRingSave] {
        let ring = KillRing::default();
        let mut target = target("中🙂xyz", &ring);
        region(&mut target, 0, 7);
        run(&mut target, Command::KeyboardQuit);
        run(&mut target, command);
        assert_eq!(entries(&ring), ["中🙂"]);
        assert_eq!(
            target.buffer().text().to_string(),
            if command == Command::KillRegion {
                "xyz"
            } else {
                "中🙂xyz"
            }
        );
    }
}

#[test]
fn empty_regions_store_empty_entries_without_editing_even_when_readonly() {
    for command in [Command::KillRegion, Command::KillRingSave] {
        for readonly in [false, true] {
            let ring = KillRing::default();
            let mut target = target("abc", &ring);
            region(&mut target, 1, 1);
            target.set_read_only(readonly);
            let id = target.buffer().snapshot_id();
            run(&mut target, command);
            assert_eq!(entries(&ring), [""]);
            assert_eq!(target.buffer().snapshot_id(), id);
            assert!(!target.buffer().is_modified());
            assert!(!target.buffer().mark_active());
        }
    }
}

#[test]
fn empty_kill_after_kill_extends_the_existing_entry() {
    let ring = KillRing::default();
    let mut target = target("abc", &ring);
    region(&mut target, 0, 3);
    run(&mut target, Command::KillRegion);
    run(&mut target, Command::KillRegion);
    assert_eq!(entries(&ring), ["abc"]);
}

#[test]
fn readonly_kills_copy_deactivate_region_and_report_readonly_without_editing() {
    for command in [
        Command::KillRegion,
        Command::KillLine,
        Command::KillRingSave,
    ] {
        let ring = KillRing::default();
        let mut target = target("abcdef\nx", &ring);
        region(&mut target, 4, 1);
        target.set_read_only(true);
        let id = target.buffer().snapshot_id();
        let result = target.execute(target.token(), command);
        if command != Command::KillRingSave {
            assert_eq!(
                result.unwrap_err(),
                RouteError::Input(InputError::Edit(EditError::ReadOnly))
            );
        } else {
            result.unwrap();
        }
        assert_eq!(
            entries(&ring),
            [if command == Command::KillLine {
                "bcdef"
            } else {
                "bcd"
            }]
        );
        assert_eq!(
            target.buffer().point(),
            ByteOffset(if command == Command::KillLine { 6 } else { 1 })
        );
        assert_eq!(target.buffer().mark(), Some(ByteOffset(4)));
        assert!(!target.buffer().mark_active());
        assert!(!target.buffer().is_modified());
        assert_eq!(target.buffer().snapshot_id(), id);
    }
}

#[test]
fn rejected_readonly_typing_breaks_a_preceding_kill_chain() {
    let ring = KillRing::default();
    let mut target = target("abc\ndef", &ring);
    target.set_read_only(true);
    target
        .execute(target.token(), Command::KillLine)
        .unwrap_err();
    let event = KeyDownEvent {
        keystroke: Keystroke::parse("z").unwrap().with_simulated_ime(),
        is_held: false,
        prefer_character_input: false,
    };
    assert_eq!(
        target
            .key_down(target.token(), &event, &[&Keymap::m1_defaults()])
            .unwrap_err(),
        RouteError::Input(InputError::Edit(EditError::ReadOnly))
    );
    target
        .execute(target.token(), Command::KillLine)
        .unwrap_err();
    assert_eq!(entries(&ring), ["\n", "abc"]);
}

#[test]
fn yank_inserts_at_point_without_replacing_region_and_sets_an_inactive_mark() {
    let ring = KillRing::default();
    let mut source = target("中🙂", &ring);
    let mut destination = target("abcdef", &ring);
    region(&mut destination, 1, 4);
    run(&mut source, Command::KillLine);
    let outcome = run(&mut destination, Command::Yank);
    assert_eq!(outcome.message.as_deref(), Some("Mark set"));
    assert_eq!(destination.buffer().text().to_string(), "abcd中🙂ef");
    assert_eq!(destination.buffer().point(), ByteOffset(11));
    assert_eq!(destination.buffer().mark(), Some(ByteOffset(4)));
    assert!(!destination.buffer().mark_active());
    run(&mut destination, Command::Yank);
    assert_eq!(destination.buffer().text().to_string(), "abcd中🙂中🙂ef");
    assert_eq!(destination.buffer().mark(), Some(ByteOffset(11)));
    assert_eq!(entries(&ring), ["中🙂"]);
    assert!(source.buffer().text().is_empty());
}

#[test]
fn yank_readonly_and_empty_ring_have_the_reference_mark_side_effects() {
    let ring = KillRing::default();
    let mut target = target("abc", &ring);
    region(&mut target, 2, 1);
    target.set_read_only(true);
    assert_eq!(
        target.execute(target.token(), Command::Yank).unwrap_err(),
        RouteError::Input(InputError::Edit(EditError::ReadOnly))
    );
    assert_eq!(target.buffer().mark(), Some(ByteOffset(2)));
    assert!(target.buffer().mark_active());
    target.set_read_only(false);
    assert_eq!(
        target.execute(target.token(), Command::Yank).unwrap_err(),
        RouteError::EmptyKillRing
    );
    assert_eq!(target.buffer().mark(), Some(ByteOffset(1)));
    assert!(target.buffer().mark_active());
    assert!(!target.buffer().is_modified());
    run(&mut target, Command::KillRingSave);
    run(&mut target, Command::SetMark);
    let id = target.buffer().snapshot_id();
    run(&mut target, Command::Yank);
    assert!(target.buffer().mark_active());
    assert_eq!(target.buffer().snapshot_id(), id);
    assert!(!target.buffer().is_modified());
}

#[test]
fn region_commands_without_a_mark_fail_without_editing_or_recording() {
    let ring = KillRing::default();
    let mut target = target("abc", &ring);
    for (command, error) in [
        (Command::KillRegion, RouteError::NoRegion),
        (Command::KillRingSave, RouteError::MarkNotSet),
    ] {
        assert_eq!(target.execute(target.token(), command).unwrap_err(), error);
    }
    assert!(entries(&ring).is_empty());
    assert!(!target.buffer().is_modified());
}

#[test]
fn global_command_chain_is_broken_by_other_targets_commands_and_input() {
    for interruption in 0..7 {
        let ring = KillRing::default();
        let mut first = target("abc\ndef", &ring);
        let mut second = target("x", &ring);
        run(&mut first, Command::KillLine);
        match interruption {
            0 => {
                run(&mut second, Command::ForwardChar);
            }
            1 => {
                second.replace_text(second.token(), None, "z").unwrap();
            }
            2 => {
                run(&mut first, Command::KeyboardQuit);
            }
            3 => {
                at(&mut first, 0);
            }
            4 => {
                first.end_input();
                first.resume_input();
            }
            5 => {
                press(&mut first, &Keymap::m1_defaults(), "ctrl-z");
            }
            _ => {
                run(&mut first, Command::Undo);
                run(&mut first, Command::UndoRedo);
            }
        }
        run(&mut first, Command::KillLine);
        assert_eq!(entries(&ring), ["\n", "abc"], "interruption {interruption}");
    }
}

#[test]
fn prefixes_are_not_commands_and_do_not_break_a_contiguous_kill() {
    let ring = KillRing::default();
    let mut target = target("abc\ndef", &ring);
    let mut map = Keymap::m1_defaults();
    map.bind(
        &semacs::keymap::parse_sequence("ctrl-x k").unwrap(),
        Command::KillLine,
    )
    .unwrap();
    run(&mut target, Command::KillLine);
    press(&mut target, &map, "ctrl-x");
    assert!(target.is_pending());
    press(&mut target, &map, "k");
    assert_eq!(entries(&ring), ["abc\n"]);
}

#[test]
fn prefix_composition_and_stale_tokens_cannot_write_the_ring() {
    let ring = KillRing::default();
    let mut target = target("abc", &ring);
    let map = Keymap::m1_defaults();
    press(&mut target, &map, "ctrl-x");
    assert_eq!(
        target
            .execute(target.token(), Command::KillLine)
            .unwrap_err(),
        RouteError::PrefixPending
    );
    press(&mut target, &map, "ctrl-g");
    target
        .replace_and_mark(target.token(), None, "拼", None)
        .unwrap();
    for command in [
        Command::KillLine,
        Command::KillRegion,
        Command::KillRingSave,
        Command::Yank,
    ] {
        assert_eq!(
            target.execute(target.token(), command).unwrap_err(),
            RouteError::Input(InputError::CompositionActive)
        );
    }
    let old = target.token();
    target.end_input();
    target.resume_input();
    assert_eq!(
        target.execute(old, Command::KillLine).unwrap_err(),
        RouteError::Input(InputError::StaleSession)
    );
    assert!(entries(&ring).is_empty());
    assert!(!target.buffer().is_modified());
}

#[test]
fn ring_retains_the_latest_120_entries_and_does_not_deduplicate() {
    let ring = KillRing::default();
    let mut target = target("abc", &ring);
    region(&mut target, 0, 3);
    for _ in 0..125 {
        run(&mut target, Command::KillRingSave);
    }
    assert_eq!(entries(&ring), vec!["abc"; 120]);
    let isolated = EditTarget::new(Buffer::scratch());
    assert!(isolated.kill_ring().latest().is_none());
}

#[test]
fn committed_region_projection_excludes_marked_text_and_survives_discard() {
    let ring = KillRing::default();
    let mut target = target("ab中🙂cd", &ring);
    region(&mut target, 11, 0);
    let snapshot = target.display_snapshot();
    assert_eq!(
        snapshot.region_ranges().collect::<Vec<_>>(),
        [ByteOffset(0)..ByteOffset(11)]
    );
    target
        .replace_and_mark(
            target.token(),
            Some(OffsetUtf16(2)..OffsetUtf16(2)),
            "拼",
            None,
        )
        .unwrap();
    let snapshot = target.display_snapshot();
    assert_eq!(snapshot.text().to_string(), "ab拼中🙂cd");
    assert_eq!(
        snapshot.region_ranges().collect::<Vec<_>>(),
        [ByteOffset(0)..ByteOffset(2), ByteOffset(5)..ByteOffset(14)]
    );
    target
        .replace_and_mark(target.token(), None, "🙂", None)
        .unwrap();
    assert_eq!(
        target
            .display_snapshot()
            .region_ranges()
            .collect::<Vec<_>>(),
        [ByteOffset(0)..ByteOffset(2), ByteOffset(6)..ByteOffset(15)]
    );
    target.end_input();
    assert_eq!(
        target
            .display_snapshot()
            .region_ranges()
            .collect::<Vec<_>>(),
        [ByteOffset(0)..ByteOffset(11)]
    );
    assert_eq!(target.buffer().mark(), Some(ByteOffset(11)));
    assert!(target.buffer().mark_active());
    assert!(!target.buffer().is_modified());
}

#[test]
fn region_projection_handles_endpoints_at_and_after_the_preedit_anchor() {
    for (mark, point, expected) in [
        (0, 2, vec![ByteOffset(0)..ByteOffset(2)]),
        (2, 4, vec![ByteOffset(5)..ByteOffset(7)]),
        (3, 4, vec![ByteOffset(6)..ByteOffset(7)]),
    ] {
        let ring = KillRing::default();
        let mut target = target("abcd", &ring);
        region(&mut target, mark, point);
        target
            .replace_and_mark(
                target.token(),
                Some(OffsetUtf16(2)..OffsetUtf16(2)),
                "拼",
                None,
            )
            .unwrap();
        assert_eq!(
            target
                .display_snapshot()
                .region_ranges()
                .collect::<Vec<_>>(),
            expected
        );
        assert!(target.buffer().mark_active());
    }
}
