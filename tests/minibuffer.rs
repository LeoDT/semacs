use gpui_kit::{KeyDownEvent, Keystroke};
use proptest::prelude::*;
use semacs::buffer::{Buffer, Edit, EditError, EditSource};
use semacs::command::{Command, Scope};
use semacs::editing::{CommandOutcome, EditTarget, KillRing, RouteError};
use semacs::files::{DecodedFile, FileFormat};
use semacs::input::InputError;
use semacs::keymap::Keymap;
use semacs::text::{ByteOffset, OffsetUtf16};

fn target(prompt: &str, initial: &str) -> EditTarget {
    EditTarget::new(Buffer::minibuffer(prompt, initial))
        .with_field_boundary(ByteOffset(prompt.len()))
        .unwrap()
}

fn run(target: &mut EditTarget, command: Command) -> CommandOutcome {
    target.execute(target.token(), command).unwrap()
}

fn at(target: &mut EditTarget, offset: usize) {
    target
        .set_point_from_pointer(target.token(), ByteOffset(offset))
        .unwrap();
}

fn readonly() -> RouteError {
    RouteError::Input(InputError::Edit(EditError::TextReadOnly))
}

fn assert_text(target: &EditTarget, text: &str, point: usize) {
    assert_eq!(target.buffer().text().to_string(), text);
    assert_eq!(target.buffer().point(), ByteOffset(point));
}

#[test]
fn prompt_is_committed_text_but_not_a_user_history_entry() {
    let mut target = target("输入🙂: ", "字");
    assert_text(&target, "输入🙂: 字", 15);
    assert_eq!(target.buffer().read_only_prefix(), ByteOffset(12));
    assert!(target.buffer().is_modified());
    assert_eq!(
        target.execute(target.token(), Command::Undo).unwrap_err(),
        RouteError::NoUndoInformation
    );
    target.replace_text(target.token(), None, "X").unwrap();
    run(&mut target, Command::Undo);
    assert_text(&target, "输入🙂: 字", 15);
    run(&mut target, Command::UndoRedo);
    assert_text(&target, "输入🙂: 字X", 15);
    assert!(target.buffer().is_modified());
}

#[test]
fn field_motion_and_readonly_protection_are_independent() {
    let mut readonly_only = EditTarget::new(Buffer::minibuffer("Input: ", "abc"));
    run(&mut readonly_only, Command::MoveBeginningOfLine);
    assert_eq!(readonly_only.buffer().point(), ByteOffset(0));
    assert_eq!(
        readonly_only
            .execute(readonly_only.token(), Command::DeleteChar)
            .unwrap_err(),
        readonly()
    );

    let mut field = target("Input: ", "abc");
    run(&mut field, Command::MoveBeginningOfLine);
    assert_eq!(field.buffer().point(), ByteOffset(7));
    run(&mut field, Command::BackwardChar);
    assert_eq!(field.buffer().point(), ByteOffset(6));
    run(&mut field, Command::MoveEndOfLine);
    assert_eq!(field.buffer().point(), ByteOffset(10));
    at(&mut field, 6);
    run(&mut field, Command::MoveBeginningOfLine);
    assert_eq!(field.buffer().point(), ByteOffset(0));
    run(&mut field, Command::MoveEndOfLine);
    assert_eq!(field.buffer().point(), ByteOffset(10));
    run(&mut field, Command::MoveBeginningOfLine);
    assert_eq!(field.buffer().point(), ByteOffset(7));
    run(&mut field, Command::EndOfBuffer);
    assert_eq!(field.buffer().point(), ByteOffset(10));
    run(&mut field, Command::BeginningOfBuffer);
    assert_eq!(field.buffer().point(), ByteOffset(0));
}

#[test]
fn only_fixed_boundaries_inside_immutable_prefixes_are_accepted() {
    let buffer = Buffer::minibuffer("中: ", "tail");
    assert!(matches!(
        EditTarget::new(buffer).with_field_boundary(ByteOffset(1)),
        Err(EditError::InvalidRange)
    ));
    assert!(matches!(
        EditTarget::new(Buffer::minibuffer("P: ", "tail")).with_field_boundary(ByteOffset(4)),
        Err(EditError::InvalidRange)
    ));
    assert!(matches!(
        Buffer::scratch().with_read_only_prefix(ByteOffset(1)),
        Err(EditError::InvalidRange)
    ));
}

#[test]
fn field_line_motion_respects_later_lines_and_multiline_unicode_prompts() {
    let mut target = target("提示\n中: ", "abc\ndef");
    run(&mut target, Command::MoveBeginningOfLine);
    assert_eq!(target.buffer().point(), ByteOffset(16));
    at(&mut target, 14);
    run(&mut target, Command::MoveBeginningOfLine);
    assert_eq!(target.buffer().point(), ByteOffset(12));
    run(&mut target, Command::MoveEndOfLine);
    assert_eq!(target.buffer().point(), ByteOffset(15));
    at(&mut target, 7);
    run(&mut target, Command::MoveEndOfLine);
    assert_eq!(target.buffer().point(), ByteOffset(15));
    at(&mut target, 11);
    run(&mut target, Command::MoveBeginningOfLine);
    assert_eq!(target.buffer().point(), ByteOffset(7));
}

#[test]
fn command_insert_delete_and_region_edits_cannot_change_prompt() {
    for command in [
        Command::DeleteChar,
        Command::DeleteForwardChar,
        Command::DeleteBackwardChar,
        Command::Newline,
        Command::ElectricNewline,
        Command::Indent,
    ] {
        let mut target = target("P: ", "abc");
        at(
            &mut target,
            if command == Command::DeleteBackwardChar {
                3
            } else {
                1
            },
        );
        let before = target.display_snapshot().id();
        assert_eq!(
            target.execute(target.token(), command).unwrap_err(),
            readonly(),
            "{command:?}"
        );
        assert_eq!(target.buffer().text().to_string(), "P: abc");
        assert_eq!(target.display_snapshot().id(), before);
        assert_eq!(
            target.execute(target.token(), Command::Undo).unwrap_err(),
            RouteError::NoUndoInformation
        );
    }
    let mut target = target("P: ", "abc");
    at(&mut target, 1);
    run(&mut target, Command::SetMark);
    at(&mut target, 5);
    assert_eq!(
        target
            .execute(target.token(), Command::DeleteForwardChar)
            .unwrap_err(),
        readonly()
    );
    assert!(target.buffer().mark_active());
    assert_text(&target, "P: abc", 5);
}

#[test]
fn direct_edits_validate_every_source_and_allow_insertion_at_the_prefix_end() {
    for source in [
        EditSource::Command,
        EditSource::PlatformInput,
        EditSource::Undo,
        EditSource::Redo,
    ] {
        let mut buffer = Buffer::minibuffer("中: ", "abc");
        let snapshot = buffer.snapshot_id();
        for range in [0..0, 0..3, 3..5, 3..7] {
            assert_eq!(
                buffer.edit(Edit {
                    snapshot,
                    range: ByteOffset(range.start)..ByteOffset(range.end),
                    text: "X",
                    source
                }),
                Err(EditError::TextReadOnly)
            );
            assert_eq!(buffer.snapshot_id(), snapshot);
        }
        buffer
            .edit(Edit {
                snapshot,
                range: ByteOffset(5)..ByteOffset(5),
                text: "X",
                source,
            })
            .unwrap();
        assert_eq!(buffer.text().to_string(), "中: Xabc");
    }
}

#[test]
fn adding_protection_also_guards_history_replay_without_losing_the_stack() {
    let mut buffer = Buffer::scratch();
    buffer
        .edit(Edit {
            snapshot: buffer.snapshot_id(),
            range: ByteOffset(0)..ByteOffset(0),
            text: "abc",
            source: EditSource::Command,
        })
        .unwrap();
    let mut buffer = buffer.with_read_only_prefix(ByteOffset(3)).unwrap();
    let id = buffer.snapshot_id();
    assert_eq!(buffer.undo(), Err(EditError::TextReadOnly));
    assert_eq!(buffer.snapshot_id(), id);
    let mut buffer = buffer.with_read_only_prefix(ByteOffset(0)).unwrap();
    buffer.undo().unwrap().unwrap();
    assert!(buffer.text().is_empty());
    buffer.redo().unwrap().unwrap();
    assert_eq!(buffer.text().to_string(), "abc");
}

#[test]
fn platform_replacements_and_pure_preedit_are_transactionally_protected() {
    let mut target = target("中: ", "abc");
    let before = target.display_snapshot().id();
    for range in [
        Some(OffsetUtf16(0)..OffsetUtf16(0)),
        Some(OffsetUtf16(2)..OffsetUtf16(4)),
    ] {
        assert_eq!(
            target
                .replace_text(target.token(), range.clone(), "X")
                .unwrap_err(),
            readonly()
        );
        assert_eq!(
            target
                .replace_and_mark(target.token(), range, "ni", None)
                .unwrap_err(),
            readonly()
        );
        assert!(!target.input().is_composing());
        assert_eq!(target.display_snapshot().id(), before);
    }
    at(&mut target, 3);
    assert!(!target.allows_text_input(target.token()));
    assert_eq!(
        target
            .replace_and_mark(target.token(), None, "ni", None)
            .unwrap_err(),
        readonly()
    );
    assert_eq!(
        target.replace_text(target.token(), None, "X").unwrap_err(),
        readonly()
    );
    target
        .replace_text(target.token(), Some(OffsetUtf16(3)..OffsetUtf16(4)), "字")
        .unwrap();
    assert_text(&target, "中: 字bc", 8);
}

#[test]
fn invalid_preedit_replacement_keeps_the_valid_existing_composition() {
    let mut target = target("P: ", "abc");
    at(&mut target, 3);
    target
        .replace_and_mark(target.token(), None, "ni", None)
        .unwrap();
    let before = target.display_snapshot();
    assert_eq!(
        target
            .replace_and_mark(
                target.token(),
                Some(OffsetUtf16(1)..OffsetUtf16(4)),
                "bad",
                None
            )
            .unwrap_err(),
        readonly()
    );
    assert_eq!(
        target
            .replace_text(target.token(), Some(OffsetUtf16(0)..OffsetUtf16(4)), "bad")
            .unwrap_err(),
        readonly()
    );
    assert_eq!(target.display_snapshot().id(), before.id());
    assert_eq!(target.display_snapshot().text().to_string(), "P: niabc");
    target.commit_preedit(target.token()).unwrap().unwrap();
    assert_text(&target, "P: niabc", 5);
    run(&mut target, Command::Undo);
    assert_text(&target, "P: abc", 3);
}

#[test]
fn newline_and_indent_do_not_consume_trailing_prompt_whitespace() {
    for command in [Command::Newline, Command::ElectricNewline, Command::Indent] {
        let mut target = target("  P: \t ", "");
        let prefix = target.buffer().read_only_prefix();
        run(&mut target, command);
        let text = target.buffer().text().to_string();
        assert!(text.starts_with("  P: \t "));
        assert!(text.len() > prefix.0);
        run(&mut target, Command::Undo);
        assert_text(&target, "  P: \t ", prefix.0);
    }
}

#[test]
fn kill_inside_prompt_copies_only_its_field_then_errors_without_history() {
    let mut target = target("Input: ", "abc");
    at(&mut target, 0);
    let id = target.buffer().snapshot_id();
    assert_eq!(
        target
            .execute(target.token(), Command::KillLine)
            .unwrap_err(),
        readonly()
    );
    assert_text(&target, "Input: abc", 7);
    assert_eq!(target.kill_ring().latest().unwrap().to_string(), "Input: ");
    assert_eq!(target.buffer().snapshot_id(), id);
    run(&mut target, Command::KillLine);
    assert_text(&target, "Input: ", 7);
    assert_eq!(
        target.kill_ring().latest().unwrap().to_string(),
        "Input: abc"
    );
    run(&mut target, Command::Undo);
    assert_text(&target, "Input: abc", 7);
    assert_eq!(
        target.execute(target.token(), Command::Undo).unwrap_err(),
        RouteError::NoUndoInformation
    );
}

#[test]
fn copy_and_kill_can_include_prompt_without_modifying_it() {
    for command in [Command::KillRegion, Command::KillRingSave] {
        let mut target = target("P: ", "abc");
        at(&mut target, 1);
        run(&mut target, Command::SetMark);
        at(&mut target, 5);
        let result = target.execute(target.token(), command);
        if command == Command::KillRegion {
            assert_eq!(result.unwrap_err(), readonly());
        } else {
            result.unwrap();
        }
        assert_eq!(target.kill_ring().latest().unwrap().to_string(), ": ab");
        assert_text(&target, "P: abc", 5);
        assert!(!target.buffer().mark_active());
    }
}

#[test]
fn body_and_minibuffer_keep_independent_histories_with_a_shared_ring() {
    let ring = KillRing::default();
    let body = Buffer::new(
        None,
        DecodedFile {
            text: "body".into(),
            format: FileFormat::default(),
        },
    );
    let mut body = EditTarget::with_kill_ring(body, ring.clone());
    let mut mini = EditTarget::with_kill_ring(Buffer::minibuffer("Input: ", ""), ring)
        .with_field_boundary(ByteOffset(7))
        .unwrap();
    run(&mut body, Command::KillLine);
    body.replace_text(body.token(), None, "A").unwrap();
    body.end_input();
    run(&mut mini, Command::Yank);
    run(&mut mini, Command::MoveBeginningOfLine);
    run(&mut mini, Command::SetMark);
    run(&mut mini, Command::ForwardChar);
    run(&mut mini, Command::ForwardChar);
    run(&mut mini, Command::KillRegion);
    assert_eq!(mini.kill_ring().latest().unwrap().to_string(), "bo");
    run(&mut mini, Command::Undo);
    assert_text(&mini, "Input: body", 9);
    run(&mut mini, Command::Undo);
    assert_text(&mini, "Input: ", 7);
    body.resume_input();
    run(&mut body, Command::Undo);
    assert_text(&body, "", 0);
    run(&mut body, Command::Undo);
    assert_text(&body, "body", 0);
    assert!(mini.buffer().is_modified());
    assert!(!body.buffer().is_modified());
}

#[test]
fn yank_into_protected_text_keeps_its_mark_side_effect_without_editing_prompt() {
    for entry in [Some("X"), Some(""), None] {
        let mut target = target("P: ", entry.unwrap_or(""));
        if entry.is_some() {
            run(&mut target, Command::MoveBeginningOfLine);
            run(&mut target, Command::SetMark);
            run(&mut target, Command::EndOfBuffer);
            run(&mut target, Command::KillRingSave);
        }
        let text = target.buffer().text().to_string();
        at(&mut target, 1);
        let result = target.execute(target.token(), Command::Yank);
        match entry {
            Some("X") => assert_eq!(result.unwrap_err(), readonly()),
            Some("") => {
                result.unwrap();
            }
            None => assert_eq!(result.unwrap_err(), RouteError::EmptyKillRing),
            _ => unreachable!(),
        }
        assert_eq!(target.buffer().mark(), Some(ByteOffset(1)));
        assert_text(&target, &text, 1);
        assert_eq!(
            target.execute(target.token(), Command::Undo).unwrap_err(),
            RouteError::NoUndoInformation
        );
    }
}

#[test]
fn self_insert_keys_inside_prompt_are_rejected_before_platform_input() {
    let map = Keymap::for_scope(Scope::Edit);
    let mut target = target("P: ", "");
    at(&mut target, 1);
    let event = KeyDownEvent {
        keystroke: Keystroke::parse("a").unwrap().with_simulated_ime(),
        is_held: false,
        prefer_character_input: false,
    };
    assert_eq!(
        target
            .key_down(target.token(), &event, &[&map])
            .unwrap_err(),
        readonly()
    );
    assert_text(&target, "P: ", 1);
}

proptest! {
    #[test]
    fn arbitrary_attempts_to_edit_a_unicode_prefix_never_change_it_or_its_history(
        prompt in prop::collection::vec(prop::sample::select(vec!['中', '🙂', ' ', '\n', '\t']), 1..16),
        input in prop::collection::vec(any::<char>(), 0..16),
        a in any::<usize>(), b in any::<usize>(),
        inserted in prop::collection::vec(any::<char>(), 0..12),
    ) {
        let prompt: String = prompt.into_iter().collect();
        let input: String = input.into_iter().collect();
        let inserted: String = inserted.into_iter().collect();
        let mut buffer = Buffer::minibuffer(&prompt, &input);
        let text = buffer.text().to_string();
        let bounds: Vec<_> = text.char_indices().map(|(i, _)| i).chain([text.len()]).collect();
        let a = bounds[a % bounds.len()];
        let b = bounds[b % bounds.len()];
        let start = a.min(b);
        let range = ByteOffset(start)..ByteOffset(a.max(b));
        let snapshot = buffer.snapshot_id();
        let result = buffer.edit(Edit { snapshot, range: range.clone(), text: &inserted, source: EditSource::PlatformInput });
        if start < prompt.len() && !(range.is_empty() && inserted.is_empty()) {
            prop_assert_eq!(result, Err(EditError::TextReadOnly));
            prop_assert_eq!(buffer.snapshot_id(), snapshot);
            prop_assert_eq!(buffer.text().to_string(), text);
            prop_assert!(buffer.undo().unwrap().is_none());
        } else {
            result.unwrap();
            prop_assert!(buffer.text().to_string().starts_with(&prompt));
            if buffer.undo().unwrap().is_some() {
                prop_assert_eq!(&buffer.text().to_string(), &text);
                buffer.redo().unwrap().unwrap();
                prop_assert!(buffer.text().to_string().starts_with(&prompt));
            }
        }
    }
}
