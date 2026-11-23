use crate::buffer::{Buffer, EditError};
use crate::command::Command;
use crate::editing::{EditTarget, RouteError};
use crate::files::{DecodedFile, FileFormat};
use crate::input::InputError;
use crate::text::ByteOffset;

fn target(text: &str) -> EditTarget {
    EditTarget::new(Buffer::new(
        None,
        DecodedFile {
            text: text.into(),
            format: FileFormat::default(),
        },
    ))
}

fn run(target: &mut EditTarget, command: Command) -> Result<Option<String>, RouteError> {
    let token = target.token();
    target
        .execute(token, command)
        .map(|outcome| outcome.message)
}

fn type_str(target: &mut EditTarget, text: &str) {
    for ch in text.chars() {
        let mut buf = [0u8; 4];
        let s = ch.encode_utf8(&mut buf);
        let token = target.token();
        target.replace_text(token, None, s).unwrap();
    }
}

#[test]
fn mark_next_and_previous_like_this_handles_forward_backward_and_adjacent_matches() {
    let mut t = target("foo bar foo baz foo");
    run(&mut t, Command::SetMark).unwrap();
    run(&mut t, Command::ForwardWord).unwrap();
    assert_eq!(t.buffer().region(), Some(ByteOffset(0)..ByteOffset(3)));

    run(&mut t, Command::McMarkNextLikeThis).unwrap();
    assert_eq!(t.cursor_count(), 2);
    assert_eq!(t.secondary_cursors(), vec![ByteOffset(11)]);
    assert_eq!(t.secondary_regions(), vec![ByteOffset(8)..ByteOffset(11)]);

    run(&mut t, Command::McMarkNextLikeThis).unwrap();
    assert_eq!(t.cursor_count(), 3);
    assert_eq!(t.secondary_cursors(), vec![ByteOffset(11), ByteOffset(19)]);

    assert_eq!(
        run(&mut t, Command::McMarkNextLikeThis).unwrap().as_deref(),
        Some("No more matches")
    );

    // Backward selection direction is preserved on created fake cursors.
    let mut t = target("foofoo");
    run(&mut t, Command::EndOfBuffer).unwrap();
    run(&mut t, Command::SetMark).unwrap();
    for _ in 0..3 {
        run(&mut t, Command::BackwardChar).unwrap();
    }
    assert_eq!(t.buffer().point(), ByteOffset(3));
    assert_eq!(t.buffer().region(), Some(ByteOffset(3)..ByteOffset(6)));
    run(&mut t, Command::McMarkPreviousLikeThis).unwrap();
    assert_eq!(t.cursor_count(), 2);
    assert_eq!(t.secondary_cursors(), vec![ByteOffset(0)]);
    assert_eq!(t.secondary_regions(), vec![ByteOffset(0)..ByteOffset(3)]);
}

#[test]
fn mark_next_and_previous_without_region_uses_lines_and_preserves_goal_column() {
    let mut t = target("0123456789\nab\n0123456789\n0123456789");
    for _ in 0..6 {
        run(&mut t, Command::ForwardChar).unwrap();
    }
    run(&mut t, Command::McMarkNextLikeThis).unwrap();
    run(&mut t, Command::McMarkNextLikeThis).unwrap();
    assert_eq!(t.cursor_count(), 3);
    // Line 1 col 6 -> line 2 clamped to col 2 ("ab") -> line 3 back to col 6!
    assert_eq!(t.secondary_cursors(), vec![ByteOffset(13), ByteOffset(20)]);

    type_str(&mut t, "!");
    assert_eq!(
        t.buffer().text().to_string(),
        "012345!6789\nab!\n012345!6789\n0123456789"
    );
}

#[test]
fn skip_unmark_mark_all_and_edit_lines() {
    let mut t = target("foo 1 foo 2 foo 3 foo");
    run(&mut t, Command::SetMark).unwrap();
    run(&mut t, Command::ForwardWord).unwrap();

    run(&mut t, Command::McMarkNextLikeThis).unwrap();
    assert_eq!(t.secondary_cursors(), vec![ByteOffset(9)]);
    run(&mut t, Command::McSkipToNextLikeThis).unwrap();
    assert_eq!(t.secondary_cursors(), vec![ByteOffset(15)]);
    run(&mut t, Command::McMarkNextLikeThis).unwrap();
    assert_eq!(t.secondary_cursors(), vec![ByteOffset(15), ByteOffset(21)]);
    run(&mut t, Command::McUnmarkNextLikeThis).unwrap();
    assert_eq!(t.secondary_cursors(), vec![ByteOffset(15)]);
    run(&mut t, Command::McUnmarkNextLikeThis).unwrap();
    assert!(!t.has_multiple_cursors());

    // mark-all-like-this marks all 4 occurrences.
    run(&mut t, Command::McMarkAllLikeThis).unwrap();
    assert_eq!(t.cursor_count(), 4);
    run(&mut t, Command::DeleteBackwardChar).unwrap();
    type_str(&mut t, "bar");
    assert_eq!(t.buffer().text().to_string(), "bar 1 bar 2 bar 3 bar");

    // mc/edit-lines excludes trailing empty column-0 line when region ends at line start.
    let mut t = target("one\ntwo\nthree\nfour");
    run(&mut t, Command::SetMark).unwrap();
    run(&mut t, Command::NextLine).unwrap();
    run(&mut t, Command::NextLine).unwrap();
    run(&mut t, Command::NextLine).unwrap();
    // Point is now at line 4 col 0 ("four"), so region covers lines 1..=3.
    run(&mut t, Command::McEditLines).unwrap();
    assert_eq!(t.cursor_count(), 3);
    assert!(!t.buffer().mark_active());
    type_str(&mut t, "- ");
    assert_eq!(t.buffer().text().to_string(), "- one\n- two\n- three\nfour");
}

#[test]
fn emacs_edit_semantics_no_delete_selection_and_single_undo_group() {
    // 1. Typing with active regions does NOT replace them (no delete-selection-mode).
    let mut t = target("foo foo");
    run(&mut t, Command::SetMark).unwrap();
    run(&mut t, Command::ForwardWord).unwrap();
    run(&mut t, Command::McMarkNextLikeThis).unwrap();
    type_str(&mut t, "X");
    assert_eq!(t.buffer().text().to_string(), "fooX fooX");

    // 2. Backspace deletes all active regions, then typing merges into a single undo group.
    let mut t = target("foo bar foo baz foo");
    run(&mut t, Command::SetMark).unwrap();
    run(&mut t, Command::ForwardWord).unwrap();
    run(&mut t, Command::McMarkAllLikeThis).unwrap();
    run(&mut t, Command::DeleteBackwardChar).unwrap();
    assert_eq!(t.buffer().text().to_string(), " bar  baz ");
    type_str(&mut t, "qux");
    assert_eq!(t.buffer().text().to_string(), "qux bar qux baz qux");

    // Single undo removes all three "qux", second undo restores all three "foo".
    run(&mut t, Command::Undo).unwrap();
    assert_eq!(t.buffer().text().to_string(), " bar  baz ");
    run(&mut t, Command::Undo).unwrap();
    assert_eq!(t.buffer().text().to_string(), "foo bar foo baz foo");
    run(&mut t, Command::UndoRedo).unwrap();
    run(&mut t, Command::UndoRedo).unwrap();
    assert_eq!(t.buffer().text().to_string(), "qux bar qux baz qux");

    // 3. C-d always deletes one char ignoring active region; Delete deletes active regions.
    let mut t = target("ab cd ab");
    run(&mut t, Command::SetMark).unwrap();
    run(&mut t, Command::ForwardWord).unwrap();
    run(&mut t, Command::McMarkNextLikeThis).unwrap();
    run(&mut t, Command::MoveBeginningOfLine).unwrap();
    // Recreate regions on "ab" at 0..2 and 6..8, with point at 0 and 6 (backward).
    let mut t = target("ab cd ab");
    run(&mut t, Command::ForwardWord).unwrap();
    run(&mut t, Command::SetMark).unwrap();
    run(&mut t, Command::BackwardWord).unwrap();
    run(&mut t, Command::McMarkNextLikeThis).unwrap();
    run(&mut t, Command::DeleteChar).unwrap();
    assert_eq!(t.buffer().text().to_string(), "b cd b");

    let mut t = target("ab cd ab");
    run(&mut t, Command::SetMark).unwrap();
    run(&mut t, Command::ForwardWord).unwrap();
    run(&mut t, Command::McMarkNextLikeThis).unwrap();
    run(&mut t, Command::DeleteForwardChar).unwrap();
    assert_eq!(t.buffer().text().to_string(), " cd ");
}

#[test]
fn ret_exits_mc_while_ctrl_j_inserts_newlines_and_ctrl_g_is_two_stage() {
    let mut t = target("alpha\nbeta");
    run(&mut t, Command::SetMark).unwrap();
    run(&mut t, Command::ForwardWord).unwrap();
    run(&mut t, Command::McMarkNextLikeThis).unwrap(); // No match -> still 1 cursor
    assert!(!t.has_multiple_cursors());

    let mut t = target("aa\naa");
    run(&mut t, Command::SetMark).unwrap();
    run(&mut t, Command::ForwardWord).unwrap();
    run(&mut t, Command::McMarkNextLikeThis).unwrap();
    assert_eq!(t.cursor_count(), 2);

    // First C-g deactivates regions but stays in multiple-cursors mode.
    run(&mut t, Command::KeyboardQuit).unwrap();
    assert_eq!(t.cursor_count(), 2);
    assert!(!t.buffer().mark_active());
    assert!(t.secondary_regions().is_empty());

    // C-j inserts newline at every cursor.
    run(&mut t, Command::ElectricNewline).unwrap();
    assert_eq!(t.buffer().text().to_string(), "aa\n\naa\n");

    // Second C-g (no active region) exits multiple-cursors mode.
    run(&mut t, Command::KeyboardQuit).unwrap();
    assert_eq!(t.cursor_count(), 1);

    // Upstream default: RET exits multiple-cursors mode without inserting newline.
    let mut t = target("one\ntwo");
    run(&mut t, Command::McMarkNextLikeThis).unwrap();
    assert_eq!(t.cursor_count(), 2);
    run(&mut t, Command::Newline).unwrap();
    assert_eq!(t.cursor_count(), 1);
    assert_eq!(t.buffer().text().to_string(), "one\ntwo");
}

#[test]
fn per_cursor_kill_ring_motion_and_dedup_on_collision() {
    let mut t = target("alpha\nbeta\ngamma");
    run(&mut t, Command::McMarkNextLikeThis).unwrap();
    run(&mut t, Command::McMarkNextLikeThis).unwrap();
    assert_eq!(t.cursor_count(), 3);

    // Select each line's word independently, copy, move to EOL, type space, yank each cursor's own text.
    run(&mut t, Command::SetMark).unwrap();
    run(&mut t, Command::ForwardWord).unwrap();
    run(&mut t, Command::KillRingSave).unwrap();
    run(&mut t, Command::MoveEndOfLine).unwrap();
    type_str(&mut t, " ");
    run(&mut t, Command::Yank).unwrap();
    assert_eq!(
        t.buffer().text().to_string(),
        "alpha alpha\nbeta beta\ngamma gamma"
    );

    // Moving all cursors to beginning of buffer collapses them into one cursor.
    run(&mut t, Command::BeginningOfBuffer).unwrap();
    assert_eq!(t.cursor_count(), 1);
    assert_eq!(t.buffer().point(), ByteOffset(0));
}

#[test]
fn read_only_and_minibuffer_reject_multiple_cursors() {
    let mut t = target("hello\nhello");
    t.set_read_only(true);
    assert_eq!(
        run(&mut t, Command::McMarkNextLikeThis),
        Err(RouteError::Input(InputError::Edit(EditError::ReadOnly)))
    );

    let mut mini = EditTarget::new(Buffer::minibuffer("Find file: ", "abc"))
        .with_field_boundary(ByteOffset(11))
        .unwrap();
    assert_eq!(
        run(&mut mini, Command::McMarkNextLikeThis),
        Err(RouteError::MultipleCursorsUnsupported)
    );

    // Making an active multi-cursor buffer read-only exits multiple-cursors mode.
    let mut t = target("a\nb");
    run(&mut t, Command::McMarkNextLikeThis).unwrap();
    assert_eq!(t.cursor_count(), 2);
    t.set_read_only(true);
    assert_eq!(t.cursor_count(), 1);
}

#[test]
fn review_regressions_kill_collapse_skip_preserve_mark_all_overlap_redo_and_sibling_readonly() {
    // #1: Collapsing two cursors on the same line during C-k must not panic or corrupt kill rings.
    let mut t = target("abc def\nxyz");
    let token = t.token();
    t.toggle_cursor_at_pointer(token, ByteOffset(4)).unwrap();
    assert_eq!(t.cursor_count(), 2);
    run(&mut t, Command::KillLine).unwrap();
    assert_eq!(t.buffer().text().to_string(), "\nxyz");
    assert_eq!(t.cursor_count(), 1);

    // #3: Redo when primary cursor is NOT the lowest-offset entry keeps all cursors intact.
    let mut t = target("foo bar foo");
    run(&mut t, Command::EndOfBuffer).unwrap();
    run(&mut t, Command::SetMark).unwrap();
    run(&mut t, Command::BackwardWord).unwrap();
    run(&mut t, Command::McMarkPreviousLikeThis).unwrap();
    assert_eq!(t.cursor_count(), 2);
    type_str(&mut t, "!");
    assert_eq!(t.buffer().text().to_string(), "!foo bar !foo");
    run(&mut t, Command::Undo).unwrap();
    assert_eq!(t.buffer().text().to_string(), "foo bar foo");
    assert_eq!(t.cursor_count(), 2);
    run(&mut t, Command::UndoRedo).unwrap();
    assert_eq!(t.buffer().text().to_string(), "!foo bar !foo");
    assert_eq!(t.cursor_count(), 2);

    // #5: mc/skip-to-next-like-this preserves the furthest cursor when no further match exists.
    let mut t = target("foo bar foo");
    run(&mut t, Command::SetMark).unwrap();
    run(&mut t, Command::ForwardWord).unwrap();
    run(&mut t, Command::McMarkNextLikeThis).unwrap();
    assert_eq!(t.cursor_count(), 2);
    assert_eq!(
        run(&mut t, Command::McSkipToNextLikeThis)
            .unwrap()
            .as_deref(),
        Some("No more matches")
    );
    assert_eq!(t.cursor_count(), 2);
    assert_eq!(t.secondary_cursors(), vec![ByteOffset(11)]);

    // #6: mc/mark-all-like-this finds non-overlapping match right after primary region in "aaaaa".
    let mut t = target("aaaaa");
    run(&mut t, Command::ForwardChar).unwrap();
    run(&mut t, Command::SetMark).unwrap();
    run(&mut t, Command::ForwardChar).unwrap();
    run(&mut t, Command::ForwardChar).unwrap();
    // Primary selected 1..3 ("aa"); 0..2 overlaps, 3..5 is a valid non-overlapping match.
    run(&mut t, Command::McMarkAllLikeThis).unwrap();
    assert_eq!(t.cursor_count(), 2);
    assert_eq!(t.secondary_regions(), vec![ByteOffset(3)..ByteOffset(5)]);

    // #7: Sibling view toggling read-only exits multiple-cursors on synchronize_buffer.
    let first_buf = Buffer::new(
        None,
        DecodedFile {
            text: "a\nb".into(),
            format: FileFormat::default(),
        },
    );
    let mut second_buf = first_buf.new_view();
    let mut first = EditTarget::new(first_buf);
    run(&mut first, Command::McMarkNextLikeThis).unwrap();
    assert_eq!(first.cursor_count(), 2);
    second_buf.set_read_only(true);
    let _ = first.synchronize_buffer(3);
    assert_eq!(first.cursor_count(), 1);
}
