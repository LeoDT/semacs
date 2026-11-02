use std::ops::Range;
use std::path::PathBuf;

use proptest::prelude::*;
use semacs::buffer::{Buffer, Edit, EditError, EditSource};
use semacs::files::{DecodedFile, FileFormat, LineEnding};
use semacs::input::{DisplayChange, DisplaySnapshot, InputError, InputSession};
use semacs::text::{ByteOffset, OffsetUtf16, Point, Rope, clip_utf16_range, range_to_utf16};

fn bytes(start: usize, end: usize) -> Range<ByteOffset> {
    ByteOffset(start)..ByteOffset(end)
}

fn assert_display_change(
    before: &DisplaySnapshot,
    after: &DisplaySnapshot,
    change: &DisplayChange,
) {
    assert_eq!(change.before, before.id());
    assert_eq!(change.after, after.id());
    assert_eq!(change.old_range.start, change.new_range.start);
    let mut patched = before.text().to_string();
    let new = after.text().to_string();
    patched.replace_range(
        change.old_range.start.0..change.old_range.end.0,
        &new[change.new_range.start.0..change.new_range.end.0],
    );
    assert_eq!(patched, new);
}

fn utf16(start: usize, end: usize) -> Range<OffsetUtf16> {
    OffsetUtf16(start)..OffsetUtf16(end)
}

fn buffer(text: &str) -> Buffer {
    Buffer::new(
        None,
        DecodedFile {
            text: text.into(),
            format: FileFormat::default(),
        },
    )
}

fn command(buffer: &mut Buffer, range: Range<ByteOffset>, text: &str) {
    buffer
        .edit(Edit {
            snapshot: buffer.snapshot_id(),
            range,
            text,
            source: EditSource::Command,
        })
        .unwrap();
}

#[test]
fn startup_buffers_are_clean_and_preserve_new_file_identity() {
    for buffer in [
        Buffer::scratch(),
        Buffer::new_file(PathBuf::from("notes.txt")),
    ] {
        assert!(buffer.text().is_empty());
        assert_eq!(buffer.point(), ByteOffset(0));
        assert_eq!(buffer.format(), FileFormat::default());
        assert!(!buffer.is_modified());
    }
    assert_eq!(Buffer::scratch().path(), None);
    assert_eq!(Buffer::scratch().display_name(), "*scratch*");
    let new = Buffer::new_file(PathBuf::from("notes.txt"));
    assert_eq!(
        new.path().as_deref(),
        Some(PathBuf::from("notes.txt").as_path())
    );
    assert_eq!(new.display_name(), "notes.txt");
}

#[test]
fn persistent_snapshots_version_every_replacement_and_preserve_format() {
    let format = FileFormat {
        bom: true,
        line_ending: LineEnding::Crlf,
    };
    let mut buffer = Buffer::new(
        Some(PathBuf::from("notes.txt")),
        DecodedFile {
            text: "中🙂\ne\u{301}\t\n".into(),
            format,
        },
    );
    let before = buffer.snapshot();
    assert!(!buffer.is_modified());
    assert_eq!(buffer.display_name(), "notes.txt");
    assert_eq!(
        buffer.path().as_deref(),
        Some(PathBuf::from("notes.txt").as_path())
    );
    assert_eq!(buffer.format(), format);
    assert_eq!(buffer.text().max_point(), Point::new(2, 0));
    let change = buffer
        .edit(Edit {
            snapshot: before.id(),
            range: bytes(0, 3),
            text: "文",
            source: EditSource::PlatformInput,
        })
        .unwrap()
        .unwrap();
    assert_eq!(change.before, before.id());
    assert_eq!(change.after, buffer.snapshot_id());
    assert_eq!(change.old_range, bytes(0, 3));
    assert_eq!(change.new_range, bytes(0, 3));
    assert_eq!(change.source, EditSource::PlatformInput);
    assert_ne!(change.before, change.after);
    assert!(buffer.is_modified());
    assert_eq!(buffer.point(), ByteOffset(3));
    assert_eq!(buffer.format(), format);
    assert_eq!(before.text().to_string(), "中🙂\ne\u{301}\t\n");
    assert_eq!(buffer.text().to_string(), "文🙂\ne\u{301}\t\n");
    let version = buffer.snapshot_id();
    command(&mut buffer, bytes(0, 3), "文");
    assert_ne!(
        buffer.snapshot_id(),
        version,
        "no content comparison as a save-point substitute"
    );
}

#[test]
fn invalid_stale_and_wrong_target_edits_are_atomic() {
    let mut buffer = buffer("中🙂a");
    let original = buffer.snapshot_id();
    let other = self::buffer("");
    for (id, range, expected) in [
        (other.snapshot_id(), bytes(0, 0), EditError::WrongTarget),
        (original, bytes(1, 3), EditError::InvalidRange),
        (original, bytes(0, 4), EditError::InvalidRange),
        (original, bytes(8, 0), EditError::InvalidRange),
        (original, bytes(0, 9), EditError::InvalidRange),
    ] {
        assert_eq!(
            buffer.edit(Edit {
                snapshot: id,
                range,
                text: "x",
                source: EditSource::Command
            }),
            Err(expected)
        );
        assert_eq!(buffer.snapshot_id(), original);
        assert_eq!(buffer.point(), ByteOffset(0));
        assert!(!buffer.is_modified());
        assert_eq!(buffer.text().to_string(), "中🙂a");
    }
    assert_eq!(
        buffer.set_point(ByteOffset(1)),
        Err(EditError::InvalidRange)
    );
    assert_eq!(
        buffer
            .edit(Edit {
                snapshot: original,
                range: bytes(0, 0),
                text: "",
                source: EditSource::Command
            })
            .unwrap(),
        None
    );
    assert_eq!(buffer.snapshot_id(), original);
    assert!(!buffer.is_modified());
    command(&mut buffer, bytes(0, 0), "x");
    assert_eq!(
        buffer.edit(Edit {
            snapshot: original,
            range: bytes(0, 0),
            text: "late",
            source: EditSource::Command
        }),
        Err(EditError::StaleSnapshot)
    );
    assert_eq!(buffer.text().to_string(), "x中🙂a");
}

#[test]
fn coordinates_are_scalar_safe_across_chunks_newlines_and_eof() {
    let text = "中🙂e\u{301}\t\n\n".repeat(300);
    let rope = Rope::from(text.as_str());
    let mut units = 0;
    for (byte, ch) in text.char_indices() {
        assert_eq!(
            clip_utf16_range(&rope, utf16(units, units)),
            Some(bytes(byte, byte))
        );
        assert_eq!(
            range_to_utf16(&rope, bytes(byte, byte)),
            Some(utf16(units, units))
        );
        assert_eq!(rope.point_to_offset(rope.offset_to_point(byte)), byte);
        units += ch.len_utf16();
    }
    assert_eq!(
        clip_utf16_range(&rope, utf16(units, units)),
        Some(bytes(text.len(), text.len()))
    );
    assert_eq!(
        clip_utf16_range(&rope, utf16(usize::MAX, usize::MAX)),
        Some(bytes(text.len(), text.len()))
    );
    assert_eq!(
        clip_utf16_range(&rope, utf16(2, 2)),
        Some(bytes(3, 3)),
        "inside surrogate pair remains collapsed"
    );
    assert_eq!(clip_utf16_range(&rope, utf16(2, 3)), Some(bytes(3, 7)));
    assert_eq!(clip_utf16_range(&rope, utf16(3, 2)), None);
    assert_eq!(range_to_utf16(&rope, bytes(1, 3)), None);
}

#[test]
fn ordinary_preedit_updates_cancel_without_editing_committed_text_or_point() {
    let mut buffer = buffer("a🙂中z");
    buffer.set_point(ByteOffset(1)).unwrap();
    let original = buffer.snapshot();
    let mut input = InputSession::new(&buffer);
    let token = input.token();
    for text in ["ni", "候选", "汉🙂"] {
        let before = input.display_snapshot(&buffer).unwrap();
        let update = input
            .replace_and_mark(&mut buffer, token, None, text, None)
            .unwrap();
        assert_eq!(update.edit, None);
        assert_eq!(buffer.snapshot_id(), original.id());
        assert_eq!(buffer.point(), ByteOffset(1));
        assert!(!buffer.is_modified());
        let after = input.display_snapshot(&buffer).unwrap();
        assert_display_change(&before, &after, &update.change);
        assert_ne!(before.id(), after.id());
        assert!(input.is_composing());
    }
    assert_eq!(
        input.guard_command(&buffer, token),
        Err(InputError::CompositionActive)
    );
    let before = input.display_snapshot(&buffer).unwrap();
    let (next, change) = input.cancel(&buffer, token).unwrap();
    let after = input.display_snapshot(&buffer).unwrap();
    assert_display_change(&before, &after, &change);
    assert_ne!(next, token);
    assert_eq!(input.guard_command(&buffer, next), Ok(()));
    assert_eq!(buffer.text().to_string(), original.text().to_string());
    assert!(!buffer.is_modified());
}

#[test]
fn explicit_nonempty_replacement_is_committed_and_cancellation_does_not_restore_it() {
    let mut buffer = buffer("a🙂中z");
    let original = buffer.snapshot();
    let mut input = InputSession::new(&buffer);
    let token = input.token();
    let before = input.display_snapshot(&buffer).unwrap();
    let update = input
        .replace_and_mark(
            &mut buffer,
            token,
            Some(utf16(1, 4)),
            "汉e\u{301}",
            Some(utf16(1, 3)),
        )
        .unwrap();
    let edit = update.edit.unwrap();
    assert_eq!(edit.before, original.id());
    assert_eq!(edit.old_range, bytes(1, 8));
    assert_eq!(edit.new_range, bytes(1, 1));
    assert_eq!(buffer.text().to_string(), "az");
    assert!(buffer.is_modified());
    let after = input.display_snapshot(&buffer).unwrap();
    assert_display_change(&before, &after, &update.change);
    assert_eq!(after.text().to_string(), "a汉e\u{301}z");
    assert_eq!(after.selected_range_utf16(), utf16(2, 4));
    assert_eq!(after.marked_range_utf16(), Some(utf16(1, 4)));
    assert_eq!(
        after.text_for_range(utf16(1, 99)),
        Some(("汉e\u{301}z".into(), utf16(1, 5)))
    );
    let version = buffer.snapshot_id();
    let (_, change) = input.cancel(&buffer, token).unwrap();
    let restored = input.display_snapshot(&buffer).unwrap();
    assert_display_change(&after, &restored, &change);
    assert_eq!(change.old_range, bytes(1, 7));
    assert_eq!(change.new_range, bytes(1, 1));
    assert_eq!(buffer.text().to_string(), "az");
    assert_eq!(buffer.snapshot_id(), version);
    assert_eq!(original.text().to_string(), "a🙂中z");
}

#[test]
fn ordinary_composition_commit_is_one_edit_and_rejects_old_token() {
    let mut buffer = buffer("az");
    buffer.set_point(ByteOffset(1)).unwrap();
    let original = buffer.snapshot_id();
    let mut input = InputSession::new(&buffer);
    let token = input.token();
    input
        .replace_and_mark(&mut buffer, token, None, "拼音", None)
        .unwrap();
    input
        .replace_and_mark(&mut buffer, token, None, "候选", None)
        .unwrap();
    let before = input.display_snapshot(&buffer).unwrap();
    let update = input
        .replace_text(&mut buffer, token, None, "汉🙂")
        .unwrap();
    let edit = update.edit.unwrap();
    assert_eq!(edit.before, original);
    assert_eq!(edit.old_range, bytes(1, 1));
    assert_eq!(edit.new_range, bytes(1, 8));
    assert_eq!(buffer.text().to_string(), "a汉🙂z");
    assert_eq!(buffer.point(), ByteOffset(8));
    assert!(buffer.is_modified());
    let after = input.display_snapshot(&buffer).unwrap();
    assert_display_change(&before, &after, &update.change);
    assert_eq!(after.marked_range(), None);
    assert_eq!(after.selected_range_utf16(), utf16(4, 4));
    assert_eq!(
        input.replace_text(&mut buffer, token, None, "late"),
        Err(InputError::StaleSession)
    );
}

#[test]
fn partial_and_external_replacements_preserve_unmarked_text_as_committed() {
    let mut buffer = buffer("0123456789");
    let mut input = InputSession::new(&buffer);
    let token = input.token();
    input
        .replace_and_mark(&mut buffer, token, Some(utf16(3, 6)), "ABCD", None)
        .unwrap();
    input
        .replace_and_mark(&mut buffer, token, Some(utf16(4, 6)), "中", None)
        .unwrap();
    assert_eq!(buffer.text().to_string(), "012AD6789");
    assert_eq!(
        input.display_snapshot(&buffer).unwrap().text().to_string(),
        "012A中D6789"
    );
    input
        .replace_and_mark(&mut buffer, token, Some(utf16(0, 1)), "x", None)
        .unwrap();
    assert_eq!(buffer.text().to_string(), "12A中D6789");
    let before = input.display_snapshot(&buffer).unwrap();
    let (_, change) = input.cancel(&buffer, token).unwrap();
    assert_display_change(&before, &input.display_snapshot(&buffer).unwrap(), &change);
    assert_eq!(buffer.text().to_string(), "12A中D6789");
    assert!(buffer.is_modified());
}

#[test]
fn empty_preedit_and_platform_deletion_of_marked_text_do_not_dirty_ordinary_input() {
    let mut buffer = buffer("中");
    let mut input = InputSession::new(&buffer);
    let token = input.token();
    input
        .replace_and_mark(&mut buffer, token, None, "ni", None)
        .unwrap();
    let marked = input
        .display_snapshot(&buffer)
        .unwrap()
        .marked_range_utf16()
        .unwrap();
    let update = input
        .replace_text(&mut buffer, token, Some(marked), "")
        .unwrap();
    assert_eq!(update.edit, None);
    assert_eq!(buffer.text().to_string(), "中");
    assert!(!buffer.is_modified());
    let token = input.token();
    input
        .replace_and_mark(&mut buffer, token, None, "", None)
        .unwrap();
    assert!(!input.is_composing());
    assert_eq!(input.guard_command(&buffer, token), Ok(()));
    input.cancel(&buffer, token).unwrap();
    assert!(!buffer.is_modified());
    let token = input.token();
    input
        .replace_text(&mut buffer, token, Some(utf16(0, 1)), "")
        .unwrap();
    assert_eq!(buffer.text().to_string(), "");
    assert!(buffer.is_modified());
}

#[test]
fn ended_sessions_and_new_sessions_never_reinterpret_old_target_events() {
    let mut a = buffer("a");
    let mut b = buffer("b");
    let mut input = InputSession::new(&a);
    let old = input.token();
    input
        .replace_and_mark(&mut a, old, None, "预编辑", None)
        .unwrap();
    input.end();
    assert!(!input.is_composing());
    assert_eq!(
        input.replace_text(&mut a, old, None, "late"),
        Err(InputError::InactiveSession)
    );
    assert_eq!(
        input.replace_text(&mut b, old, None, "late"),
        Err(InputError::InactiveSession)
    );
    assert_eq!(input.committed_snapshot(&a).text().to_string(), "a");
    let mut reopened = InputSession::new(&a);
    assert_eq!(
        reopened.replace_text(&mut a, old, None, "late"),
        Err(InputError::StaleSession)
    );
    let new = reopened.token();
    assert_eq!(
        reopened.replace_text(&mut b, new, None, "late"),
        Err(InputError::Edit(EditError::WrongTarget))
    );
    assert_eq!(a.text().to_string(), "a");
    assert_eq!(b.text().to_string(), "b");
    assert!(!a.is_modified());
}

#[test]
fn independently_edited_buffer_requires_a_new_session_even_without_preedit() {
    for compose in [false, true] {
        let mut buffer = buffer("abc");
        let mut input = InputSession::new(&buffer);
        let token = input.token();
        if compose {
            input
                .replace_and_mark(&mut buffer, token, None, "x", None)
                .unwrap();
        }
        command(&mut buffer, bytes(0, 1), "z");
        assert_eq!(
            input.replace_text(&mut buffer, token, None, "late"),
            Err(InputError::Edit(EditError::StaleSnapshot))
        );
        assert_eq!(
            input.replace_and_mark(&mut buffer, token, None, "late", None),
            Err(InputError::Edit(EditError::StaleSnapshot))
        );
        assert!(input.display_snapshot(&buffer).is_err());
        input.end();
        assert_eq!(input.committed_snapshot(&buffer).text().to_string(), "zbc");
        let mut rebound = InputSession::new(&buffer);
        assert_eq!(
            rebound.replace_text(&mut buffer, token, None, "late"),
            Err(InputError::StaleSession)
        );
    }
}

#[test]
fn invalid_platform_ranges_do_not_disturb_active_preedit_or_commit_deletions() {
    let mut buffer = buffer("🙂\n");
    let mut input = InputSession::new(&buffer);
    let token = input.token();
    input
        .replace_and_mark(&mut buffer, token, Some(utf16(0, 2)), "汉", None)
        .unwrap();
    let before = input.display_snapshot(&buffer).unwrap();
    let version = buffer.snapshot_id();
    for (range, selected) in [
        (Some(utf16(2, 1)), None),
        (Some(utf16(0, 1)), Some(utf16(2, 1))),
    ] {
        assert_eq!(
            input.replace_and_mark(&mut buffer, token, range, "文", selected),
            Err(InputError::Edit(EditError::InvalidRange))
        );
        let after = input.display_snapshot(&buffer).unwrap();
        assert_eq!(before.id(), after.id());
        assert_eq!(before.text().to_string(), after.text().to_string());
        assert_eq!(buffer.snapshot_id(), version);
    }
}

#[test]
fn line_column_counts_unicode_scalars_using_rope_summaries() {
    let text = "中🙂e\u{301}\t\n\n".repeat(1000);
    let mut buffer = buffer(&text);
    for byte in [0, 3, 7, 8, 10, text.len()] {
        buffer.set_point(ByteOffset(byte)).unwrap();
        let prefix = &text[..byte];
        assert_eq!(
            buffer.line_column(),
            (
                prefix.bytes().filter(|byte| *byte == b'\n').count() as u32 + 1,
                prefix.rsplit('\n').next().unwrap().chars().count(),
            )
        );
    }
}

#[test]
fn direct_input_covers_empty_lines_tab_combining_scalar_and_eof() {
    let mut buffer = buffer("");
    let mut input = InputSession::new(&buffer);
    let token = input.token();
    for text in ["中", "🙂", "e", "\u{301}", "\t", "\n", "\n"] {
        input.replace_text(&mut buffer, token, None, text).unwrap();
    }
    assert_eq!(buffer.text().to_string(), "中🙂e\u{301}\t\n\n");
    assert_eq!(buffer.text().max_point(), Point::new(2, 0));
    let snapshot = input.display_snapshot(&buffer).unwrap();
    assert_eq!(snapshot.selected_range_utf16(), utf16(8, 8));
    assert_eq!(
        snapshot.text_for_range(utf16(2, 3)),
        Some(("🙂".into(), utf16(1, 3)))
    );
    input
        .replace_text(&mut buffer, token, Some(utf16(0, 1)), "文")
        .unwrap();
    assert_ne!(input.display_snapshot(&buffer).unwrap().id(), snapshot.id());
}

#[test]
fn large_single_line_and_mixed_text_keep_exact_content_through_preedit_and_commit() {
    let mixed = "ab中🙂e\u{301}\t\n\n";
    for text in [
        "x".repeat(8 * 1024 * 1024),
        mixed.repeat((8_usize * 1024 * 1024).div_ceil(mixed.len())),
    ] {
        let mut buffer = buffer(&text);
        let original = buffer.snapshot();
        let start = text.floor_char_boundary(text.len() / 2);
        buffer.set_point(ByteOffset(start)).unwrap();
        let mut input = InputSession::new(&buffer);
        let token = input.token();
        input
            .replace_and_mark(&mut buffer, token, None, "候选🙂", None)
            .unwrap();
        let mut expected = text.clone();
        expected.insert_str(start, "候选🙂");
        let first = input.display_snapshot(&buffer).unwrap();
        assert_eq!(first.text().to_string(), expected);
        input
            .replace_and_mark(&mut buffer, token, None, "汉字🙂", None)
            .unwrap();
        assert_ne!(first.id(), input.display_snapshot(&buffer).unwrap().id());
        assert_eq!(buffer.snapshot_id(), original.id());
        assert!(!buffer.is_modified());
        input
            .replace_text(&mut buffer, token, None, "终🙂\n")
            .unwrap();
        expected.replace_range(start..start + "候选🙂".len(), "终🙂\n");
        assert_eq!(buffer.text().to_string(), expected);
        assert_eq!(original.text().to_string(), text);
        assert_eq!(buffer.point(), ByteOffset(start + "终🙂\n".len()));
    }
}

#[test]
fn partial_commit_uses_projection_delta_and_places_caret_at_inserted_text() {
    let mut buffer = buffer("0123456789");
    let mut input = InputSession::new(&buffer);
    let token = input.token();
    input
        .replace_and_mark(&mut buffer, token, Some(utf16(3, 6)), "ABCD", None)
        .unwrap();
    let before = input.display_snapshot(&buffer).unwrap();
    let update = input
        .replace_text(&mut buffer, token, Some(utf16(4, 5)), "中")
        .unwrap();
    assert_eq!(buffer.text().to_string(), "012A中CD6789");
    assert_eq!(buffer.point(), ByteOffset("012A中".len()));
    assert_eq!(update.edit.as_ref().unwrap().old_range, bytes(3, 3));
    assert_eq!(update.change.old_range, bytes(4, 5));
    assert_eq!(update.change.new_range, bytes(4, 7));
    assert_display_change(
        &before,
        &input.display_snapshot(&buffer).unwrap(),
        &update.change,
    );
}

#[test]
fn composition_unmark_commits_once_and_preserves_projected_caret() {
    let mut buffer = buffer("az");
    buffer.set_point(ByteOffset(1)).unwrap();
    let mut input = InputSession::new(&buffer);
    let token = input.token();
    input
        .replace_and_mark(&mut buffer, token, None, "终🙂选", Some(utf16(1, 1)))
        .unwrap();
    let before = input.display_snapshot(&buffer).unwrap();
    let update = input.commit_preedit(&mut buffer).unwrap().unwrap();
    assert_display_change(
        &before,
        &input.display_snapshot(&buffer).unwrap(),
        &update.change,
    );
    assert!(update.change.old_range.is_empty());
    assert_eq!(buffer.text().to_string(), "a终🙂选z");
    assert_eq!(buffer.point(), ByteOffset(4));
    assert_eq!(update.edit.unwrap().old_range, bytes(1, 1));
    let version = buffer.snapshot_id();
    assert_eq!(input.commit_preedit(&mut buffer).unwrap(), None);
    assert_eq!(version, buffer.snapshot_id());
    assert_eq!(
        input.replace_text(&mut buffer, token, None, "late"),
        Err(InputError::StaleSession)
    );
}

#[test]
fn composed_changes_cover_crossing_deletion_and_earlier_expansion() {
    for (replacement, expected_old, expected_new) in [
        ("", bytes(2, 5), bytes(2, 2)),
        ("12345678", bytes(2, 3), bytes(2, 13)),
    ] {
        let mut buffer = buffer("abcdefghij");
        let mut input = InputSession::new(&buffer);
        let token = input.token();
        let before = input.display_snapshot(&buffer).unwrap();
        let first = input
            .replace_and_mark(&mut buffer, token, Some(utf16(3, 3)), "XYZ", None)
            .unwrap();
        let end = if replacement.is_empty() { 8 } else { 3 };
        let second = input
            .replace_and_mark(&mut buffer, token, Some(utf16(2, end)), replacement, None)
            .unwrap();
        let change = first.change.compose(&second.change);
        assert_eq!(change.old_range, expected_old);
        assert_eq!(change.new_range, expected_new);
        assert_display_change(&before, &input.display_snapshot(&buffer).unwrap(), &change);
    }
}

proptest! {
    #[test]
    fn projected_edits_match_string_and_commit_once(
        initial in "[a-z中🙂\\n\\t]{0,80}",
        operations in prop::collection::vec((any::<usize>(), any::<usize>(), "[a-z字🙂\\n\\t]{0,15}"), 1..25),
        commit in any::<bool>(),
        explicit_commit in any::<bool>(),
        final_start in any::<usize>(),
        final_end in any::<usize>(),
    ) {
        let mut buffer = buffer(&initial);
        let mut input = InputSession::new(&buffer);
        let token = input.token();
        let initial_display = input.display_snapshot(&buffer).unwrap();
        let mut token = token;
        let mut cumulative: Option<DisplayChange> = None;
        let mut expected = initial.clone();
        for (a, b, replacement) in operations {
            let boundaries: Vec<_> = expected.char_indices().map(|(i, _)| i).chain([expected.len()]).collect();
            let (a, b) = (a % boundaries.len(), b % boundaries.len());
            let start = boundaries[a.min(b)];
            let end = boundaries[a.max(b)];
            let range = utf16(expected[..start].encode_utf16().count(), expected[..end].encode_utf16().count());
            let before = input.display_snapshot(&buffer).unwrap();
            let update = input.replace_and_mark(&mut buffer, token, Some(range), &replacement, None).unwrap();
            expected.replace_range(start..end, &replacement);
            let actual = input.display_snapshot(&buffer).unwrap();
            assert_display_change(&before, &actual, &update.change);
            let composed = cumulative.as_ref().map_or_else(|| update.change.clone(), |c| c.compose(&update.change));
            assert_display_change(&initial_display, &actual, &composed);
            cumulative = Some(composed);
            prop_assert_eq!(actual.text().to_string(), expected.as_str());
            prop_assert_eq!(actual.marked_range(), if replacement.is_empty() { None } else { Some(bytes(start, start + replacement.len())) });
            token = input.token();
            let mut committed = expected.clone();
            committed.replace_range(start..start + replacement.len(), "");
            prop_assert_eq!(buffer.text().to_string(), committed);
        }
        let before = input.display_snapshot(&buffer).unwrap();
        let was_composing = input.is_composing();
        let change = if commit {
            let marked = before.marked_range().unwrap_or(before.selected_range());
            let (range, start, end) = if explicit_commit {
                let boundaries: Vec<_> = expected.char_indices().map(|(i, _)| i).chain([expected.len()]).collect();
                let (a, b) = (final_start % boundaries.len(), final_end % boundaries.len());
                let start = boundaries[a.min(b)];
                let end = boundaries[a.max(b)];
                (Some(utf16(expected[..start].encode_utf16().count(), expected[..end].encode_utf16().count())), start, end)
            } else {
                (None, marked.start.0, marked.end.0)
            };
            expected.replace_range(start..end, "终");
            let update = input.replace_text(&mut buffer, token, range, "终").unwrap();
            prop_assert_eq!(buffer.point(), ByteOffset(start + "终".len()));
            prop_assert!(buffer.is_modified());
            update.change
        } else {
            let marked = before.marked_range().unwrap_or(before.selected_range());
            expected.replace_range(marked.start.0..marked.end.0, "");
            input.cancel(&buffer, token).unwrap().1
        };
        let after = input.display_snapshot(&buffer).unwrap();
        assert_display_change(&before, &after, &change);
        assert_display_change(&initial_display, &after, &cumulative.unwrap().compose(&change));
        prop_assert_eq!(buffer.text().to_string(), expected);
        if was_composing || !commit {
            prop_assert_eq!(input.replace_text(&mut buffer, token, None, "late"), Err(InputError::StaleSession));
        }
    }
}
