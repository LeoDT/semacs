use gpui_kit::{KeyDownEvent, Keystroke};
use proptest::prelude::*;
use semacs::buffer::{Buffer, Edit, EditError, EditSource};
use semacs::command::{Command, Scope};
use semacs::editing::{CommandOutcome, EditTarget, KillRing, RouteError};
use semacs::files::{DecodedFile, FileFormat};
use semacs::input::InputError;
use semacs::keymap::Keymap;
use semacs::text::{ByteOffset, OffsetUtf16};

fn target(text: &str) -> EditTarget {
    EditTarget::new(Buffer::new(
        None,
        DecodedFile {
            text: text.into(),
            format: FileFormat::default(),
        },
    ))
}

fn run(target: &mut EditTarget, command: Command) -> CommandOutcome {
    target.execute(target.token(), command).unwrap()
}

fn insert(target: &mut EditTarget, text: &str) {
    target.replace_text(target.token(), None, text).unwrap();
}

fn at(target: &mut EditTarget, offset: usize) {
    target
        .set_point_from_pointer(target.token(), ByteOffset(offset))
        .unwrap();
}

fn press(target: &mut EditTarget, key: &'static str) -> Result<CommandOutcome, RouteError> {
    target
        .key_down(
            target.token(),
            &KeyDownEvent {
                keystroke: Keystroke::parse(key).unwrap().with_simulated_ime(),
                is_held: false,
                prefer_character_input: false,
            },
            &[&Keymap::for_scope(Scope::Edit)],
        )
        .map(|route| route.outcome)
}

fn assert_state(target: &EditTarget, text: &str, point: usize) {
    assert_eq!(target.buffer().text().to_string(), text);
    assert_eq!(target.buffer().point(), ByteOffset(point));
    assert_eq!(target.display_snapshot().text().to_string(), text);
}

#[test]
fn unicode_typing_is_one_group_and_replay_has_fresh_monotonic_identities() {
    let mut target = target("tail");
    for text in ["中", "🙂", "e", "\u{301}"] {
        insert(&mut target, text);
    }
    let before = target.display_snapshot().id();
    let snapshot = target.buffer().snapshot();
    let undo = run(&mut target, Command::Undo);
    assert_eq!(undo.message.as_deref(), Some("Undo"));
    let update = undo.update.unwrap();
    assert_eq!(update.change.before, before);
    assert_eq!(update.change.old_range, ByteOffset(0)..ByteOffset(10));
    assert_eq!(update.change.new_range, ByteOffset(0)..ByteOffset(0));
    assert_eq!(update.edit.unwrap().source, EditSource::Undo);
    assert_state(&target, "tail", 0);
    assert!(!target.buffer().is_modified());
    assert_eq!(snapshot.text().to_string(), "中🙂e\u{301}tail");
    let undone = target.display_snapshot().id();
    assert_ne!(before, undone);
    let redo = run(&mut target, Command::UndoRedo);
    assert_eq!(redo.message.as_deref(), Some("Redo"));
    let update = redo.update.unwrap();
    assert_eq!(update.change.before, undone);
    assert_eq!(update.edit.unwrap().source, EditSource::Redo);
    assert_state(&target, "中🙂e\u{301}tail", 0);
    assert!(target.buffer().is_modified());
    assert_ne!(target.display_snapshot().id(), before);
    assert_eq!(
        target
            .execute(target.token(), Command::UndoRedo)
            .unwrap_err(),
        RouteError::NoRedoInformation
    );
    run(&mut target, Command::Undo);
    assert_eq!(
        target.execute(target.token(), Command::Undo).unwrap_err(),
        RouteError::NoUndoInformation
    );
}

#[test]
fn contiguous_unicode_deletes_merge_and_restore_the_original_point_and_marker() {
    for backward in [false, true] {
        for mark in [0, 1, 4, 8, 9, 11] {
            let mut target = target("a中🙂e\u{301}Z");
            at(&mut target, mark);
            run(&mut target, Command::SetMark);
            at(&mut target, if backward { 11 } else { 1 });
            let point = target.buffer().point();
            for _ in 0..4 {
                run(
                    &mut target,
                    if backward {
                        Command::DeleteBackwardChar
                    } else {
                        Command::DeleteChar
                    },
                );
            }
            assert_state(&target, "aZ", 1);
            run(&mut target, Command::Undo);
            assert_state(&target, "a中🙂e\u{301}Z", point.0);
            assert_eq!(target.buffer().mark(), Some(ByteOffset(mark)));
            assert!(!target.buffer().mark_active());
            run(&mut target, Command::UndoRedo);
            assert_state(&target, "aZ", 1);
            assert_eq!(
                target.buffer().mark(),
                Some(ByteOffset(if mark > 1 { 1 } else { mark }))
            );
            run(&mut target, Command::Undo);
            assert_eq!(target.buffer().mark(), Some(ByteOffset(mark)));
            assert_eq!(
                target.execute(target.token(), Command::Undo).unwrap_err(),
                RouteError::NoUndoInformation
            );
        }
    }
}

#[test]
fn motions_commands_undefined_pointer_and_lifecycle_break_typing_groups() {
    for interruption in 0..6 {
        let mut target = target("");
        insert(&mut target, "ab");
        match interruption {
            0 => {
                run(&mut target, Command::MoveEndOfLine);
            }
            1 => {
                run(&mut target, Command::KeyboardQuit);
            }
            2 => {
                press(&mut target, "ctrl-z").unwrap();
            }
            3 => {
                at(&mut target, 2);
            }
            4 => {
                target.end_input();
                target.resume_input();
            }
            _ => {
                run(&mut target, Command::SetMark);
            }
        }
        insert(&mut target, "cd");
        run(&mut target, Command::Undo);
        assert_state(&target, "ab", 2);
        run(&mut target, Command::Undo);
        assert_state(&target, "", 0);
    }
}

#[test]
fn newline_and_delete_directions_are_separate_from_typing_and_each_other() {
    let mut target = target("tail");
    insert(&mut target, "ab");
    run(&mut target, Command::Newline);
    insert(&mut target, "cd");
    run(&mut target, Command::DeleteBackwardChar);
    run(&mut target, Command::DeleteBackwardChar);
    run(&mut target, Command::DeleteChar);
    run(&mut target, Command::DeleteForwardChar);
    assert_state(&target, "ab\nil", 3);
    for (text, point) in [
        ("ab\ntail", 3),
        ("ab\ncdtail", 5),
        ("ab\ntail", 3),
        ("abtail", 2),
        ("tail", 0),
    ] {
        run(&mut target, Command::Undo);
        assert_state(&target, text, point);
    }
}

#[test]
fn platform_newlines_and_explicit_replacements_do_not_merge_with_typing() {
    let mut target = target("");
    insert(&mut target, "a");
    insert(&mut target, "\n");
    insert(&mut target, "b");
    target
        .replace_text(target.token(), Some(OffsetUtf16(2)..OffsetUtf16(3)), "b")
        .unwrap();
    let snapshot = target.buffer().snapshot_id();
    run(&mut target, Command::Undo);
    assert_state(&target, "a\nb", 3);
    assert_ne!(target.buffer().snapshot_id(), snapshot);
    run(&mut target, Command::Undo);
    assert_state(&target, "a\n", 2);
    run(&mut target, Command::Undo);
    assert_state(&target, "a", 1);
    run(&mut target, Command::Undo);
    assert_state(&target, "", 0);
}

#[test]
fn redo_survives_motion_but_new_input_discards_the_redo_branch() {
    let mut target = target("tail");
    insert(&mut target, "ab");
    run(&mut target, Command::Undo);
    run(&mut target, Command::ForwardChar);
    run(&mut target, Command::UndoRedo);
    assert_state(&target, "abtail", 0);
    run(&mut target, Command::Undo);
    insert(&mut target, "X");
    assert_eq!(
        target
            .execute(target.token(), Command::UndoRedo)
            .unwrap_err(),
        RouteError::NoRedoInformation
    );
    run(&mut target, Command::Undo);
    assert_state(&target, "tail", 0);
    assert_eq!(
        target.execute(target.token(), Command::Undo).unwrap_err(),
        RouteError::NoUndoInformation
    );
}

#[test]
fn kill_ring_append_and_repeated_yank_do_not_merge_history() {
    let mut target = target("ab\ncd");
    run(&mut target, Command::KillLine);
    run(&mut target, Command::KillLine);
    assert_eq!(target.kill_ring().latest().unwrap().to_string(), "ab\n");
    run(&mut target, Command::Yank);
    run(&mut target, Command::Yank);
    for (text, point) in [("ab\ncd", 3), ("cd", 0), ("\ncd", 0), ("ab\ncd", 0)] {
        run(&mut target, Command::Undo);
        assert_state(&target, text, point);
    }
    assert_eq!(target.kill_ring().latest().unwrap().to_string(), "ab\n");
}

#[test]
fn deleted_marker_restores_only_if_still_collapsed_and_active_region_undo_is_global() {
    for moved in [false, true] {
        let mut target = target("abc\ndef");
        at(&mut target, 2);
        run(&mut target, Command::SetMark);
        at(&mut target, 0);
        run(&mut target, Command::KillLine);
        assert_eq!(target.buffer().mark(), Some(ByteOffset(0)));
        if moved {
            at(&mut target, 4);
            run(&mut target, Command::SetMark);
        }
        run(&mut target, Command::Undo);
        assert_state(&target, "abc\ndef", 0);
        assert_eq!(
            target.buffer().mark(),
            Some(ByteOffset(if moved { 7 } else { 2 }))
        );
        assert!(!target.buffer().mark_active());
        run(&mut target, Command::UndoRedo);
        assert_state(&target, "\ndef", 0);
        assert_eq!(
            target.buffer().mark(),
            Some(ByteOffset(if moved { 4 } else { 0 }))
        );
    }
    let mut target = target("abcdef");
    at(&mut target, 3);
    run(&mut target, Command::SetMark);
    at(&mut target, 0);
    run(&mut target, Command::KillRegion);
    run(&mut target, Command::Undo);
    assert_state(&target, "abcdef", 0);
    assert_eq!(target.buffer().mark(), Some(ByteOffset(3)));
}

#[test]
fn markers_created_inside_inserted_text_survive_undo_redo_cycles() {
    let mut target = target("");
    insert(&mut target, "中🙂Z");
    at(&mut target, 7);
    run(&mut target, Command::SetMark);
    run(&mut target, Command::Undo);
    assert_state(&target, "", 0);
    assert_eq!(target.buffer().mark(), Some(ByteOffset(0)));
    for _ in 0..3 {
        run(&mut target, Command::UndoRedo);
        assert_state(&target, "中🙂Z", 0);
        assert_eq!(target.buffer().mark(), Some(ByteOffset(7)));
        run(&mut target, Command::Undo);
        assert_eq!(target.buffer().mark(), Some(ByteOffset(0)));
    }
}

#[test]
fn successful_save_marks_the_current_boundary_and_the_fixed_past_save_case() {
    let mut target = target("");
    insert(&mut target, "abc");
    let point = target.begin_save();
    target.mark_saved_at(point).unwrap();
    assert!(!target.buffer().is_modified());
    insert(&mut target, "d");
    run(&mut target, Command::Undo);
    assert_state(&target, "abc", 3);
    assert!(!target.buffer().is_modified());
    run(&mut target, Command::Undo);
    assert_state(&target, "", 0);
    assert!(!target.buffer().is_modified());
    insert(&mut target, "X");
    assert!(target.buffer().is_modified());

    let mut target = self::target("");
    insert(&mut target, "ab");
    run(&mut target, Command::Newline);
    insert(&mut target, "cd");
    let point = target.begin_save();
    target.mark_saved_at(point).unwrap();
    run(&mut target, Command::Undo);
    assert_state(&target, "ab\n", 3);
    assert!(target.buffer().is_modified());
    run(&mut target, Command::Undo);
    assert_state(&target, "ab", 2);
    assert!(target.buffer().is_modified());
    run(&mut target, Command::UndoRedo);
    assert!(target.buffer().is_modified());
    run(&mut target, Command::UndoRedo);
    assert_state(&target, "ab\ncd", 3);
    assert!(!target.buffer().is_modified());
}

#[test]
fn save_after_undo_and_repeated_save_boundaries_preserve_modified_markers() {
    let mut target = target("");
    insert(&mut target, "ab");
    let point = target.begin_save();
    target.mark_saved_at(point).unwrap();
    insert(&mut target, "cd");
    let point = target.begin_save();
    target.mark_saved_at(point).unwrap();
    run(&mut target, Command::Undo);
    assert!(!target.buffer().is_modified());
    run(&mut target, Command::UndoRedo);
    assert!(!target.buffer().is_modified());
    run(&mut target, Command::Undo);
    run(&mut target, Command::Undo);
    let point = target.begin_save();
    target.mark_saved_at(point).unwrap();
    run(&mut target, Command::UndoRedo);
    run(&mut target, Command::Undo);
    assert!(!target.buffer().is_modified());
}

#[test]
fn async_save_of_an_earlier_boundary_keeps_later_edits_dirty_until_undo_reaches_it() {
    let mut target = target("");
    insert(&mut target, "ab");
    let saved = target.begin_save();
    insert(&mut target, "cd");
    assert!(target.buffer().is_modified());
    target.mark_saved_at(saved).unwrap();
    assert!(
        target.buffer().is_modified(),
        "the disk snapshot is the older state, not the current text"
    );
    assert_state(&target, "abcd", 4);
    run(&mut target, Command::Undo);
    assert_state(&target, "ab", 2);
    assert!(!target.buffer().is_modified());
    run(&mut target, Command::UndoRedo);
    assert_state(&target, "abcd", 2);
    assert!(target.buffer().is_modified());
}

#[test]
fn a_save_completed_while_undone_marks_the_saved_state_and_the_different_current_state() {
    let mut target = target("");
    insert(&mut target, "ab");
    let saved = target.begin_save();
    run(&mut target, Command::Undo);
    assert!(!target.buffer().is_modified());
    target.mark_saved_at(saved).unwrap();
    assert!(
        target.buffer().is_modified(),
        "the current empty text no longer matches the saved disk state"
    );
    assert_state(&target, "", 0);
    run(&mut target, Command::UndoRedo);
    assert_state(&target, "ab", 0);
    assert!(!target.buffer().is_modified());
    run(&mut target, Command::Undo);
    assert_state(&target, "", 0);
    assert!(target.buffer().is_modified());
}

#[test]
fn an_unreachable_save_point_cannot_clear_dirty() {
    let mut target = target("");
    insert(&mut target, "ab");
    insert(&mut target, "cd");
    let saved = target.begin_save();
    run(&mut target, Command::Undo);
    insert(&mut target, "X");
    assert!(target.buffer().is_modified());
    assert_eq!(
        target.mark_saved_at(saved),
        Err(EditError::StaleSnapshot),
        "a new branch discarded the saved history state"
    );
    assert!(target.buffer().is_modified());
    let other = self::target("").begin_save();
    assert_eq!(target.mark_saved_at(other), Err(EditError::WrongTarget));
}

#[test]
fn stale_and_wrong_target_save_completion_cannot_clear_dirty_or_break_a_group() {
    let mut target = target("");
    insert(&mut target, "discarded");
    let old = target.begin_save();
    let other = self::target("").begin_save();
    run(&mut target, Command::Undo);
    insert(&mut target, "ab");
    assert_eq!(target.mark_saved_at(old), Err(EditError::StaleSnapshot));
    assert_eq!(target.mark_saved_at(other), Err(EditError::WrongTarget));
    assert!(target.buffer().is_modified());
    insert(&mut target, "cd");
    run(&mut target, Command::Undo);
    assert_state(&target, "", 0);
}

#[test]
fn preedit_updates_and_discard_have_no_history_and_final_commit_is_isolated() {
    let mut target = target("");
    insert(&mut target, "a");
    target
        .replace_and_mark(target.token(), None, "ni", None)
        .unwrap();
    target
        .replace_and_mark(target.token(), None, "字", None)
        .unwrap();
    target.end_input();
    target.resume_input();
    assert_state(&target, "a", 1);
    run(&mut target, Command::Undo);
    assert_state(&target, "", 0);
    assert_eq!(
        target.execute(target.token(), Command::Undo).unwrap_err(),
        RouteError::NoUndoInformation
    );

    insert(&mut target, "a");
    target
        .replace_and_mark(target.token(), None, "字", None)
        .unwrap();
    target.commit_preedit(target.token()).unwrap().unwrap();
    insert(&mut target, "b");
    run(&mut target, Command::Undo);
    assert_state(&target, "a字", 4);
    run(&mut target, Command::Undo);
    assert_state(&target, "a", 1);
    run(&mut target, Command::Undo);
    assert_state(&target, "", 0);
}

#[test]
fn partial_composition_replacements_and_final_commit_replay_once_each() {
    let mut target = target("abc");
    at(&mut target, 2);
    run(&mut target, Command::SetMark);
    at(&mut target, 0);
    target
        .replace_and_mark(
            target.token(),
            Some(OffsetUtf16(0)..OffsetUtf16(2)),
            "ni",
            None,
        )
        .unwrap();
    assert_eq!(target.buffer().text().to_string(), "c");
    assert_eq!(target.buffer().mark(), Some(ByteOffset(0)));
    target.replace_text(target.token(), None, "字").unwrap();
    assert_state(&target, "字c", 3);
    run(&mut target, Command::Undo);
    assert_state(&target, "c", 0);
    run(&mut target, Command::Undo);
    assert_state(&target, "abc", 0);
    assert_eq!(target.buffer().mark(), Some(ByteOffset(2)));
    run(&mut target, Command::UndoRedo);
    run(&mut target, Command::UndoRedo);
    assert_state(&target, "字c", 0);
    assert_eq!(target.buffer().mark(), Some(ByteOffset(0)));
}

#[test]
fn history_guards_preserve_stacks_text_and_region_until_replay_is_allowed() {
    let mut target = target("tail");
    insert(&mut target, "a");
    run(&mut target, Command::SetMark);
    let before = target.display_snapshot().id();
    target.set_read_only(true);
    for command in [Command::Undo, Command::UndoRedo] {
        assert_eq!(
            target.execute(target.token(), command).unwrap_err(),
            RouteError::Input(InputError::Edit(EditError::ReadOnly))
        );
    }
    assert_eq!(target.display_snapshot().id(), before);
    assert!(target.buffer().mark_active());
    target.set_read_only(false);
    press(&mut target, "ctrl-x").unwrap();
    assert_eq!(
        target.execute(target.token(), Command::Undo).unwrap_err(),
        RouteError::PrefixPending
    );
    assert_eq!(
        target
            .replace_text(target.token(), None, "late")
            .unwrap_err(),
        RouteError::PrefixPending
    );
    press(&mut target, "ctrl-g").unwrap();
    target
        .replace_and_mark(target.token(), None, "ni", None)
        .unwrap();
    assert_eq!(
        target.execute(target.token(), Command::Undo).unwrap_err(),
        RouteError::Input(InputError::CompositionActive)
    );
    let old = target.token();
    target.end_input();
    assert_eq!(
        target.execute(old, Command::Undo).unwrap_err(),
        RouteError::Input(InputError::InactiveSession)
    );
    target.resume_input();
    assert_eq!(
        target.execute(old, Command::Undo).unwrap_err(),
        RouteError::Input(InputError::StaleSession)
    );
    run(&mut target, Command::Undo);
    assert_state(&target, "tail", 0);
    assert!(!target.buffer().mark_active());
}

#[test]
fn history_is_buffer_local_while_kill_ring_is_shared() {
    let ring = KillRing::default();
    let mut a = EditTarget::with_kill_ring(Buffer::scratch(), ring.clone());
    let mut b = EditTarget::with_kill_ring(Buffer::scratch(), ring);
    insert(&mut a, "ab");
    at(&mut a, 0);
    run(&mut a, Command::KillLine);
    run(&mut b, Command::Yank);
    run(&mut a, Command::Undo);
    assert_state(&a, "ab", 0);
    assert_state(&b, "ab", 2);
    run(&mut b, Command::Undo);
    assert_state(&b, "", 0);
    assert_state(&a, "ab", 0);
    assert_eq!(a.kill_ring().latest().unwrap().to_string(), "ab");
}

#[test]
fn all_registered_undo_redo_sequences_reach_the_same_history() {
    for undo in ["ctrl-/", "ctrl-_", "ctrl-x u"] {
        for redo in ["ctrl-?", "ctrl-alt-_", "escape ctrl-_"] {
            let mut target = target("");
            insert(&mut target, "ab");
            for key in undo.split(' ') {
                press(&mut target, key).unwrap();
            }
            assert_state(&target, "", 0);
            for key in redo.split(' ') {
                press(&mut target, key).unwrap();
            }
            assert_state(&target, "ab", 0);
        }
    }
}

proptest! {
    #[test]
    fn arbitrary_committed_replacements_roundtrip_text_point_marker_and_dirty(
        initial in prop::collection::vec(prop::sample::select(vec!['a', '中', '🙂', '\n', '\u{301}']), 0..24),
        marker in any::<usize>(),
        edits in prop::collection::vec((any::<usize>(), any::<usize>(), any::<usize>(),
            prop::collection::vec(prop::sample::select(vec!['X', '文', '🙂', '\n', '\u{301}']), 0..8)), 1..40),
    ) {
        let mut buffer = Buffer::new(None, DecodedFile { text: initial.into_iter().collect(), format: FileFormat::default() });
        let boundaries = |text: &str| text.char_indices().map(|(index, _)| index).chain([text.len()]).collect::<Vec<_>>();
        let original = buffer.text().to_string();
        let mark = boundaries(&original)[marker % boundaries(&original).len()];
        buffer.set_mark(ByteOffset(mark)).unwrap();
        let mut states = Vec::new();
        for (a, b, point, inserted) in edits {
            let text = buffer.text().to_string();
            let bounds = boundaries(&text);
            let a = bounds[a % bounds.len()];
            let b = bounds[b % bounds.len()];
            let inserted: String = inserted.into_iter().collect();
            if a == b && inserted.is_empty() { continue; }
            buffer.set_point(ByteOffset(bounds[point % bounds.len()])).unwrap();
            let before = (text, buffer.point(), buffer.mark(), buffer.is_modified());
            let start = a.min(b);
            buffer.edit(Edit { snapshot: buffer.snapshot_id(), range: ByteOffset(start)..ByteOffset(a.max(b)), text: &inserted, source: EditSource::Command }).unwrap();
            states.push((before, buffer.text().to_string(), buffer.mark(), start));
        }
        for (before, _, _, _) in states.iter().rev() {
            buffer.undo().unwrap().unwrap();
            prop_assert_eq!(&buffer.text().to_string(), &before.0);
            prop_assert_eq!(buffer.point(), before.1);
            prop_assert_eq!(buffer.mark(), before.2);
            prop_assert_eq!(buffer.is_modified(), before.3);
            prop_assert!(!buffer.mark_active());
        }
        prop_assert!(buffer.undo().unwrap().is_none());
        for (_, text, mark, start) in states {
            buffer.redo().unwrap().unwrap();
            prop_assert_eq!(buffer.text().to_string(), text);
            prop_assert_eq!(buffer.mark(), mark);
            prop_assert_eq!(buffer.point(), ByteOffset(start));
            prop_assert!(buffer.is_modified());
        }
        prop_assert!(buffer.redo().unwrap().is_none());
    }
}
