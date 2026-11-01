use gpui_kit::{KeyDownEvent, Keystroke};
use semacs::buffer::{Buffer, Edit, EditError, EditSource};
use semacs::command::Command;
use semacs::editing::{CommandOutcome, EditTarget, InputDisposition, Route, RouteError};
use semacs::files::{DecodedFile, FileFormat};
use semacs::input::InputError;
use semacs::keymap::Keymap;
use semacs::text::ByteOffset;

fn target(text: &str) -> EditTarget {
    EditTarget::new(Buffer::new(
        None,
        DecodedFile {
            text: text.into(),
            format: FileFormat::default(),
        },
    ))
}

fn execute(target: &mut EditTarget, command: Command) -> CommandOutcome {
    target.execute(target.token(), command).unwrap()
}

fn press(target: &mut EditTarget, key: &'static str, map: &Keymap) -> Result<Route, RouteError> {
    target.key_down(
        target.token(),
        &KeyDownEvent {
            keystroke: Keystroke::parse(key).unwrap().with_simulated_ime(),
            is_held: false,
            prefer_character_input: false,
        },
        &[map],
    )
}

#[test]
fn motion_uses_unicode_scalars_without_replacing_the_display_identity() {
    let mut target = target("中e\u{301}🙂\nxyz");
    let identity = target.display_snapshot().id();
    for expected in [3, 4, 6, 10] {
        execute(&mut target, Command::ForwardChar);
        assert_eq!(target.buffer().point(), ByteOffset(expected));
        assert_eq!(target.display_snapshot().id(), identity);
    }
    execute(&mut target, Command::MoveBeginningOfLine);
    assert_eq!(target.buffer().point(), ByteOffset(0));
    execute(&mut target, Command::MoveEndOfLine);
    assert_eq!(target.buffer().point(), ByteOffset(10));
    execute(&mut target, Command::ForwardChar);
    assert_eq!(target.buffer().point(), ByteOffset(11));
    execute(&mut target, Command::MoveEndOfLine);
    assert_eq!(target.buffer().point(), ByteOffset(14));
    assert_eq!(
        target
            .execute(target.token(), Command::ForwardChar)
            .unwrap_err(),
        RouteError::EndOfBuffer
    );
    execute(&mut target, Command::MoveBeginningOfLine);
    execute(&mut target, Command::BackwardChar);
    assert_eq!(target.buffer().point(), ByteOffset(10));
    assert!(!target.buffer().is_modified());
}

#[test]
fn command_and_platform_edits_use_the_same_buffer_with_distinct_sources() {
    let mut target = target("中🙂");
    let initial = target.buffer().snapshot_id();
    let command = execute(&mut target, Command::DeleteChar).update.unwrap();
    assert_eq!(command.edit.as_ref().unwrap().source, EditSource::Command);
    assert_eq!(command.change.before.committed, initial);
    assert_eq!(target.buffer().text().to_string(), "🙂");
    let input = target.replace_text(target.token(), None, "字").unwrap();
    assert_eq!(
        input.edit.as_ref().unwrap().source,
        EditSource::PlatformInput
    );
    assert_eq!(command.change.after, input.change.before);
    assert_eq!(target.buffer().text().to_string(), "字🙂");
    execute(&mut target, Command::DeleteBackwardChar);
    assert_eq!(target.buffer().text().to_string(), "🙂");
    assert_eq!(
        target
            .execute(target.token(), Command::DeleteBackwardChar)
            .unwrap_err(),
        RouteError::BeginningOfBuffer
    );
}

#[test]
fn undefined_suffix_does_not_invoke_top_level_quit_or_deactivate_the_region() {
    let map = Keymap::m1_defaults();
    let mut target = target("abc");
    execute(&mut target, Command::SetMark);
    execute(&mut target, Command::ForwardChar);
    press(&mut target, "ctrl-x", &map).unwrap();
    assert_eq!(
        target
            .execute(target.token(), Command::KeyboardQuit)
            .unwrap_err(),
        RouteError::PrefixPending
    );
    let route = press(&mut target, "ctrl-g", &map).unwrap();
    assert_eq!(route.disposition, InputDisposition::Handled);
    let outcome = route.outcome;
    assert_eq!(outcome.message.as_deref(), Some("C-x C-g is undefined"));
    assert_eq!(target.buffer().mark(), Some(ByteOffset(0)));
    assert!(target.buffer().mark_active());
    assert!(!target.is_pending());
    let route = press(&mut target, "ctrl-g", &map).unwrap();
    assert_eq!(route.disposition, InputDisposition::Handled);
    let outcome = route.outcome;
    assert_eq!(outcome.message.as_deref(), Some("Quit"));
    assert_eq!(target.buffer().mark(), Some(ByteOffset(0)));
    assert!(!target.buffer().mark_active());
}

#[test]
fn pending_prefix_blocks_every_direct_input_mutation() {
    let map = Keymap::m1_defaults();
    let mut target = target("abc");
    let initial = target.buffer().snapshot_id();
    press(&mut target, "ctrl-x", &map).unwrap();
    assert!(!target.allows_text_input(target.token()));
    assert_eq!(
        target
            .replace_text(target.token(), None, "bad")
            .unwrap_err(),
        RouteError::PrefixPending
    );
    assert_eq!(
        target
            .replace_and_mark(target.token(), None, "bad", None)
            .unwrap_err(),
        RouteError::PrefixPending
    );
    assert_eq!(
        target.commit_preedit(target.token()).unwrap_err(),
        RouteError::PrefixPending
    );
    assert_eq!(target.buffer().snapshot_id(), initial);
    assert!(!target.input().is_composing());
    assert!(target.is_pending());
    assert_eq!(
        press(&mut target, "z", &map).unwrap().disposition,
        InputDisposition::Handled
    );
    assert!(target.allows_text_input(target.token()));
    assert_eq!(
        press(&mut target, "z", &map).unwrap().disposition,
        InputDisposition::TextInput
    );
}

#[test]
fn composition_rejects_commands_without_consuming_prefix_state_or_the_preedit() {
    let map = Keymap::m1_defaults();
    let mut target = target("tail");
    execute(&mut target, Command::SetMark);
    target
        .replace_and_mark(target.token(), None, "拼音", None)
        .unwrap();
    let before = target.display_snapshot();
    for key in ["ctrl-x", "ctrl-g", "backspace", "enter"] {
        assert_eq!(
            press(&mut target, key, &map).unwrap_err(),
            RouteError::Input(InputError::CompositionActive)
        );
        assert!(!target.is_pending());
        assert_eq!(target.display_snapshot().id(), before.id());
        assert_eq!(target.buffer().point(), ByteOffset(0));
        assert!(target.buffer().mark_active());
    }
    assert_eq!(
        press(&mut target, "a", &map).unwrap().disposition,
        InputDisposition::TextInput
    );
    target.replace_text(target.token(), None, "汉").unwrap();
    assert_eq!(target.buffer().text().to_string(), "汉tail");
    assert_eq!(target.buffer().mark(), Some(ByteOffset(0)));
    assert!(!target.buffer().mark_active());
}

#[test]
fn readonly_targets_allow_motion_and_mark_but_reject_input_and_edits() {
    let map = Keymap::m1_defaults();
    let mut target = target("abc");
    target.set_read_only(true);
    let initial = target.buffer().snapshot_id();
    assert_eq!(
        press(&mut target, "a", &map).unwrap_err(),
        RouteError::Input(InputError::Edit(EditError::ReadOnly))
    );
    execute(&mut target, Command::ForwardChar);
    execute(&mut target, Command::SetMark);
    for command in [
        Command::DeleteChar,
        Command::DeleteBackwardChar,
        Command::Newline,
        Command::ElectricNewline,
        Command::Indent,
    ] {
        assert_eq!(
            target.execute(target.token(), command).unwrap_err(),
            RouteError::Input(InputError::Edit(EditError::ReadOnly))
        );
    }
    assert_eq!(
        target.replace_text(target.token(), None, "x").unwrap_err(),
        RouteError::Input(InputError::Edit(EditError::ReadOnly))
    );
    assert_eq!(
        target
            .replace_and_mark(target.token(), None, "x", None)
            .unwrap_err(),
        RouteError::Input(InputError::Edit(EditError::ReadOnly))
    );
    assert_eq!(target.buffer().snapshot_id(), initial);
    assert_eq!(target.buffer().point(), ByteOffset(1));
    assert!(target.buffer().mark_active());
    assert!(!target.buffer().is_modified());
}

#[test]
fn failed_unmark_is_transactional_and_does_not_discard_the_preedit() {
    let mut target = target("tail");
    target
        .replace_and_mark(target.token(), None, "字", None)
        .unwrap();
    let before = target.display_snapshot();
    target.set_read_only(true);
    assert_eq!(
        target.commit_preedit(target.token()).unwrap_err(),
        RouteError::Input(InputError::Edit(EditError::ReadOnly))
    );
    assert_eq!(target.display_snapshot().id(), before.id());
    assert_eq!(target.display_snapshot().text().to_string(), "字tail");
    assert!(target.input().is_composing());
    assert!(!target.buffer().is_modified());
    target.set_read_only(false);
    target.commit_preedit(target.token()).unwrap().unwrap();
    assert_eq!(target.buffer().text().to_string(), "字tail");
}

#[test]
fn lifecycle_reset_clears_prefix_and_rejects_callbacks_and_commands_from_old_sessions() {
    let map = Keymap::m1_defaults();
    let mut first = target("first");
    let mut second = target("second");
    let old = first.token();
    press(&mut first, "ctrl-x", &map).unwrap();
    first.end_input();
    assert!(!first.is_pending());
    assert_eq!(
        first.execute(old, Command::ForwardChar).unwrap_err(),
        RouteError::Input(InputError::InactiveSession)
    );
    first.resume_input().unwrap();
    for target in [&mut first, &mut second] {
        assert_eq!(
            target.replace_text(old, None, "late").unwrap_err(),
            RouteError::Input(InputError::StaleSession)
        );
        assert_eq!(
            target.execute(old, Command::ForwardChar).unwrap_err(),
            RouteError::Input(InputError::StaleSession)
        );
        assert_eq!(target.buffer().point(), ByteOffset(0));
        assert!(!target.buffer().is_modified());
    }
}

#[test]
fn marker_tracks_committed_edits_but_not_marked_updates_or_discard() {
    let mut target = target("abc");
    execute(&mut target, Command::ForwardChar);
    execute(&mut target, Command::ForwardChar);
    execute(&mut target, Command::SetMark);
    execute(&mut target, Command::MoveBeginningOfLine);
    target
        .replace_and_mark(target.token(), None, "ni", None)
        .unwrap();
    assert_eq!(target.buffer().mark(), Some(ByteOffset(2)));
    assert!(target.buffer().mark_active());
    target.end_input();
    assert_eq!(target.buffer().mark(), Some(ByteOffset(2)));
    assert!(target.buffer().mark_active());
    target.resume_input().unwrap();
    target.replace_text(target.token(), None, "中").unwrap();
    assert_eq!(target.buffer().mark(), Some(ByteOffset(5)));
    assert!(!target.buffer().mark_active());
    execute(&mut target, Command::DeleteChar);
    assert_eq!(target.buffer().mark(), Some(ByteOffset(4)));
    execute(&mut target, Command::DeleteChar);
    assert_eq!(target.buffer().mark(), Some(ByteOffset(3)));
}

#[test]
fn repeated_mark_and_escape_quit_do_not_delete_the_mark() {
    let map = Keymap::m1_defaults();
    let mut target = target("abc");
    execute(&mut target, Command::SetMark);
    let outcome = execute(&mut target, Command::SetMark);
    assert_eq!(outcome.message.as_deref(), Some("Mark deactivated"));
    assert!(!target.buffer().mark_active());
    execute(&mut target, Command::SetMark);
    for key in ["escape", "escape"] {
        press(&mut target, key, &map).unwrap();
    }
    let route = press(&mut target, "escape", &map).unwrap();
    assert_eq!(route.disposition, InputDisposition::Handled);
    let outcome = route.outcome;
    assert_eq!(outcome.message, None);
    assert_eq!(target.buffer().mark(), Some(ByteOffset(0)));
    assert!(!target.buffer().mark_active());
}

#[test]
fn newline_results_match_the_fixed_fundamental_mode_reference() {
    for (initial, point, command, expected, expected_point) in [
        ("", 0, Command::Newline, "\n", 1),
        ("    abc", 7, Command::Newline, "    abc\n    ", 12),
        ("    abc", 2, Command::Newline, "\nabc", 1),
        ("    abc\n", 8, Command::Newline, "    abc\n\n", 9),
        ("abc   def", 6, Command::Newline, "abc\ndef", 4),
        ("        abc", 11, Command::Newline, "        abc\n\t", 13),
        ("\tfoo", 2, Command::Newline, "\tf\n\too", 4),
        ("    abc", 7, Command::ElectricNewline, "    abc\n", 8),
        ("    abc", 2, Command::ElectricNewline, "  \n  abc", 3),
        ("", 0, Command::ElectricNewline, "\n", 1),
    ] {
        let mut target = target(initial);
        target
            .set_point_from_pointer(target.token(), ByteOffset(point))
            .unwrap();
        execute(&mut target, command);
        assert_eq!(
            target.buffer().text().to_string(),
            expected,
            "{initial:?} {point} {command:?}"
        );
        assert_eq!(target.buffer().point(), ByteOffset(expected_point));
    }
}

#[test]
fn tab_indents_to_relative_fields_and_falls_back_to_tab_stops() {
    for (initial, point, expected, expected_point) in [
        ("", 0, "\t", 1),
        ("    abc", 7, "    abc\t", 8),
        ("    abc", 2, "\t  abc", 1),
        ("    abc\n", 8, "    abc\n    ", 12),
        ("  foo bar\nxx", 12, "  foo bar\nxx    ", 16),
        ("\tfoo\n", 5, "\tfoo\n\t", 6),
        ("    \n", 5, "    \n    ", 9),
        ("foo bar\n", 8, "foo bar\n    ", 12),
        ("foo bar\n  ", 10, "foo bar\n    ", 12),
        ("foo bar\n      ", 14, "foo bar\n\t", 9),
        ("  foo bar\n\n", 11, "  foo bar\n\n  ", 13),
        ("中 文\n", 8, "中 文\n   ", 11),
        ("abc  ", 5, "abc\t", 4),
        ("abc\ndef", 5, "abc\nd\tef", 6),
        ("foo\tbar\n", 8, "foo\tbar\n\t", 9),
    ] {
        let mut target = target(initial);
        target
            .set_point_from_pointer(target.token(), ByteOffset(point))
            .unwrap();
        execute(&mut target, Command::Indent);
        assert_eq!(
            target.buffer().text().to_string(),
            expected,
            "{initial:?} {point}"
        );
        assert_eq!(target.buffer().point(), ByteOffset(expected_point));
    }
}

#[test]
fn gui_forward_delete_uses_region_but_control_d_deletes_one_character() {
    let mut target = target("abcdef");
    execute(&mut target, Command::ForwardChar);
    execute(&mut target, Command::SetMark);
    for _ in 0..3 {
        execute(&mut target, Command::ForwardChar);
    }
    execute(&mut target, Command::DeleteForwardChar);
    assert_eq!(target.buffer().text().to_string(), "aef");
    assert_eq!(target.buffer().point(), ByteOffset(1));
    assert_eq!(target.buffer().mark(), Some(ByteOffset(1)));
    execute(&mut target, Command::SetMark);
    execute(&mut target, Command::ForwardChar);
    execute(&mut target, Command::DeleteChar);
    assert_eq!(target.buffer().text().to_string(), "ae");
}

#[test]
fn readonly_is_enforced_at_the_authoritative_edit_entry() {
    let mut buffer = Buffer::scratch();
    buffer.set_read_only(true);
    let before = buffer.snapshot_id();
    assert_eq!(
        buffer
            .edit(Edit {
                snapshot: before,
                range: ByteOffset(0)..ByteOffset(0),
                text: "x",
                source: EditSource::Command,
            })
            .unwrap_err(),
        EditError::ReadOnly
    );
    assert_eq!(buffer.snapshot_id(), before);
    assert!(!buffer.is_modified());
}

#[test]
fn word_motion_skips_separators_and_clamps_at_buffer_edges() {
    let map = Keymap::m1_defaults();
    let mut target = target("one_two foo-bar 中中文 🙂last!\n");
    let id = target.display_snapshot().id();
    for expected in [3, 7, 11, 15, 25, 34, 36, 36] {
        press(&mut target, "alt-f", &map).unwrap();
        assert_eq!(target.buffer().point(), ByteOffset(expected));
    }
    for expected in [30, 16, 12, 8, 4, 0, 0] {
        press(&mut target, "alt-b", &map).unwrap();
        assert_eq!(target.buffer().point(), ByteOffset(expected));
    }
    assert_eq!(target.display_snapshot().id(), id);
    assert!(!target.buffer().is_modified());
}

#[test]
fn word_motion_uses_the_registered_f2_combining_mark_rule() {
    let mut target = target("e\u{301}clair");
    execute(&mut target, Command::ForwardWord);
    assert_eq!(target.buffer().point(), ByteOffset(1));
    execute(&mut target, Command::ForwardWord);
    assert_eq!(target.buffer().point(), ByteOffset(8));
    execute(&mut target, Command::BackwardWord);
    assert_eq!(target.buffer().point(), ByteOffset(3));
}

#[test]
fn vertical_motion_retains_columns_across_short_lines_and_direction_changes() {
    let map = Keymap::m1_defaults();
    let mut target = target("abcdef\nx\nabcdef\n");
    target
        .set_point_from_pointer(target.token(), ByteOffset(4))
        .unwrap();
    for (key, expected) in [("ctrl-n", 8), ("ctrl-n", 13), ("ctrl-p", 8), ("ctrl-p", 4)] {
        press(&mut target, key, &map).unwrap();
        assert_eq!(target.buffer().point(), ByteOffset(expected));
    }
    press(&mut target, "ctrl-n", &map).unwrap();
    press(&mut target, "ctrl-a", &map).unwrap();
    press(&mut target, "ctrl-n", &map).unwrap();
    assert_eq!(target.buffer().point(), ByteOffset(9));
    assert!(!target.buffer().is_modified());
}

#[test]
fn vertical_columns_account_for_tabs_wide_and_combining_characters() {
    for (initial, point, expected) in [
        ("中e\u{301}x\nabcdef", 6, 11),
        ("1234\n\tfoo", 4, 6),
        ("a\n中x", 1, 5),
        ("a\ne\u{301}x", 1, 3),
        ("\n\u{301}x", 0, 1),
        ("\tfoo\n1234567890", 1, 13),
    ] {
        let mut target = target(initial);
        target
            .set_point_from_pointer(target.token(), ByteOffset(point))
            .unwrap();
        execute(&mut target, Command::NextLine);
        assert_eq!(target.buffer().point(), ByteOffset(expected), "{initial:?}");
    }
}

#[test]
fn vertical_boundary_errors_still_move_to_the_buffer_edge() {
    let mut target = target("abc\nx\nabcdef");
    target
        .set_point_from_pointer(target.token(), ByteOffset(2))
        .unwrap();
    execute(&mut target, Command::NextLine);
    assert_eq!(target.buffer().point(), ByteOffset(5));
    execute(&mut target, Command::NextLine);
    assert_eq!(target.buffer().point(), ByteOffset(8));
    assert_eq!(
        target
            .execute(target.token(), Command::NextLine)
            .unwrap_err(),
        RouteError::EndOfBuffer
    );
    assert_eq!(target.buffer().point(), ByteOffset(12));
    execute(&mut target, Command::PreviousLine);
    execute(&mut target, Command::PreviousLine);
    assert_eq!(target.buffer().point(), ByteOffset(2));
    assert_eq!(
        target
            .execute(target.token(), Command::PreviousLine)
            .unwrap_err(),
        RouteError::BeginningOfBuffer
    );
    assert_eq!(target.buffer().point(), ByteOffset(0));
}

#[test]
fn platform_input_pointer_and_undefined_keys_interrupt_the_vertical_goal() {
    let map = Keymap::m1_defaults();
    for interruption in 0..3 {
        let mut target = target("abcdef\nx\nabcdef");
        target
            .set_point_from_pointer(target.token(), ByteOffset(4))
            .unwrap();
        execute(&mut target, Command::NextLine);
        match interruption {
            0 => {
                target.replace_text(target.token(), None, "z").unwrap();
            }
            1 => {
                target
                    .set_point_from_pointer(target.token(), ByteOffset(8))
                    .unwrap();
            }
            _ => {
                press(&mut target, "ctrl-z", &map).unwrap();
            }
        }
        execute(&mut target, Command::NextLine);
        assert_eq!(
            target.buffer().point(),
            ByteOffset(if interruption == 0 { 12 } else { 10 })
        );
    }
}

#[test]
fn buffer_jumps_set_an_inactive_mark_but_preserve_an_active_region() {
    let map = Keymap::m1_defaults();
    let mut target = target("中abc");
    execute(&mut target, Command::ForwardChar);
    let outcome = press(&mut target, "alt->", &map).unwrap().outcome;
    assert_eq!(outcome.message.as_deref(), Some("Mark set"));
    assert_eq!(target.buffer().point(), ByteOffset(6));
    assert_eq!(target.buffer().mark(), Some(ByteOffset(3)));
    assert!(!target.buffer().mark_active());
    execute(&mut target, Command::SetMark);
    press(&mut target, "escape", &map).unwrap();
    let outcome = press(&mut target, "<", &map).unwrap().outcome;
    assert_eq!(outcome.message, None);
    assert_eq!(target.buffer().point(), ByteOffset(0));
    assert_eq!(target.buffer().mark(), Some(ByteOffset(6)));
    assert!(target.buffer().mark_active());
    assert!(!target.buffer().is_modified());
}

#[test]
fn recenter_cycles_without_changing_point_mark_or_text() {
    use semacs::editing::{RecenterPosition::*, ViewportRequest};
    let mut target = target("abc");
    execute(&mut target, Command::SetMark);
    let id = target.display_snapshot().id();
    for position in [Center, Top, Bottom, Center] {
        let outcome = execute(&mut target, Command::Recenter);
        assert_eq!(outcome.viewport, Some(ViewportRequest::Recenter(position)));
    }
    execute(&mut target, Command::ForwardChar);
    assert_eq!(
        execute(&mut target, Command::Recenter).viewport,
        Some(ViewportRequest::Recenter(Center))
    );
    assert_eq!(target.buffer().point(), ByteOffset(1));
    assert_eq!(target.buffer().mark(), Some(ByteOffset(0)));
    assert!(target.buffer().mark_active());
    assert_eq!(target.display_snapshot().id(), id);
}

#[test]
fn paging_requires_a_viewport_and_still_obeys_all_target_guards() {
    use semacs::editing::{CommandViewport, PageMotion, ViewportRequest};
    struct Page;
    impl CommandViewport for Page {
        fn page(&self, _: ByteOffset, _: bool) -> Result<PageMotion, RouteError> {
            Ok(PageMotion {
                point: ByteOffset(2),
                first_row: 3,
            })
        }
    }
    let mut target = target("abc");
    assert_eq!(
        target
            .execute(target.token(), Command::ScrollUp)
            .unwrap_err(),
        RouteError::LayoutNotReady
    );
    target.set_read_only(true);
    execute(&mut target, Command::SetMark);
    let outcome = target
        .execute_with_viewport(target.token(), Command::ScrollUp, Some(&Page))
        .unwrap();
    assert_eq!(outcome.viewport, Some(ViewportRequest::ScrollToRow(3)));
    assert_eq!(target.buffer().point(), ByteOffset(2));
    assert!(target.buffer().mark_active());
    assert!(!target.buffer().is_modified());
    press(&mut target, "ctrl-x", &Keymap::m1_defaults()).unwrap();
    assert_eq!(
        target
            .execute_with_viewport(target.token(), Command::ScrollDown, Some(&Page))
            .unwrap_err(),
        RouteError::PrefixPending
    );
    target.end_input();
    target.resume_input();
    target.set_read_only(false);
    target
        .replace_and_mark(target.token(), None, "拼", None)
        .unwrap();
    assert_eq!(
        target
            .execute_with_viewport(target.token(), Command::ScrollDown, Some(&Page))
            .unwrap_err(),
        RouteError::Input(InputError::CompositionActive)
    );
    assert_eq!(target.buffer().point(), ByteOffset(2));
}

#[test]
fn undo_without_history_reports_an_error_without_editing_the_target() {
    let map = Keymap::m1_defaults();
    let mut target = target("abc");
    let before = target.buffer().snapshot_id();
    press(&mut target, "ctrl-x", &map).unwrap();
    assert_eq!(
        press(&mut target, "u", &map).unwrap_err(),
        RouteError::NoUndoInformation
    );
    assert!(!target.is_pending());
    assert_eq!(target.buffer().snapshot_id(), before);
    assert!(!target.buffer().is_modified());
}
