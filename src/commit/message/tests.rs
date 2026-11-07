use super::*;
use crate::buffer::{Buffer, Edit, EditSource};
use proptest::prelude::*;

#[test]
fn extraction_is_for_recovery_not_git_cleanup() {
    for (text, expected) in [
        ("subject", "subject\n"),
        ("\n\nsubject\n\nbody\n\n\n", "\nsubject\n\nbody\n"),
        ("  subject  \n\t\nbody \t\n", "  subject  \n\t\nbody \t\n"),
        ("# instructions\nsubject\n# status\nbody", "subject\nbody\n"),
        ("subject\n # not a comment\n", "subject\n # not a comment\n"),
        ("subject\n# - >8 -\nignored\n", "subject\n"),
        ("subject\n# -------- >8 --------\nignored", "subject\n"),
        ("subject\n# -------- >8 --------", "subject\n"),
        ("subject\n# -- >8 -- \nbody", "subject\nbody\n"),
        ("subject\n# -->8 --\nbody", "subject\nbody\n"),
        ("subject\n#  -- >8 --\nbody", "subject\nbody\n"),
        ("subject\n# -- >8 \nbody", "subject\nbody\n"),
        ("subject\n#  >8 --\nbody", "subject\nbody\n"),
        ("你好\n\n正文\n", "你好\n\n正文\n"),
        ("\u{a0}", "\u{a0}\n"),
    ] {
        assert_eq!(
            saved_message(text, "#").as_deref(),
            Some(expected),
            "{text:?}"
        );
    }
    for text in [
        "",
        " \t\r\n",
        "# instructions\n\n# status",
        "# - >8 -\nignored",
    ] {
        assert_eq!(saved_message(text, "#"), None, "{text:?}");
    }
}

proptest! {
    #[test]
    fn recovery_normalization_is_idempotent(text in "(?s:.{0,256})") {
        if let Some(saved) = saved_message(&text, "#") {
            let normalized = saved_message(&saved, "#");
            prop_assert_eq!(normalized.as_ref(), Some(&saved));
            prop_assert!(saved.ends_with('\n'));
        }
    }

    #[test]
    fn recall_always_returns_a_valid_prefix_edit(text in "(?s:.{0,256})") {
        let mut ring = MessageRing::default();
        ring.save("recovery", "#");
        let recalled = ring.recall(&mut MessagePosition::default(), &text, "#", 1).unwrap();
        let end = recalled.range.end.0;
        prop_assert_eq!(recalled.range.start, ByteOffset(0));
        prop_assert!(end <= text.len() && text.is_char_boundary(end));
        prop_assert!(end == text.len() || text[end..].starts_with("\n#") || text.starts_with('#'));
    }
}

#[test]
fn comment_prefixes_are_literal_and_may_have_multiple_bytes() {
    assert_eq!(
        saved_message("; instructions\nsubject\n; - >8 -\nignored", ";").as_deref(),
        Some("subject\n")
    );
    assert_eq!(
        saved_message("λ instructions\n你好\nλ - >8 -\nignored", "λ").as_deref(),
        Some("你好\n")
    );
    assert_eq!(
        saved_message("// instructions\nsubject\n// - >8 -\nignored", "//").as_deref(),
        Some("subject\n")
    );
    assert_eq!(
        saved_message(". comment\nsubject\n# body", ".").as_deref(),
        Some("subject\n# body\n")
    );
    assert_eq!(
        saved_message("# message", "").as_deref(),
        Some("# message\n")
    );
}

#[test]
fn saves_deduplicate_and_keep_the_most_recent_32_messages() {
    let mut ring = MessageRing::default();
    assert!(!ring.save("\n# instructions", "#"));
    for n in 0..33 {
        assert!(ring.save(&format!("message {n}"), "#"));
    }
    assert_eq!(ring.messages.len(), 32);
    assert_eq!(ring.messages.front().unwrap(), "message 32\n");
    assert_eq!(ring.messages.back().unwrap(), "message 1\n");
    assert!(ring.save("message 4\n# comment", "#"));
    assert_eq!(ring.messages.len(), 32);
    assert_eq!(ring.messages.front().unwrap(), "message 4\n");
    assert_eq!(
        ring.messages
            .iter()
            .filter(|entry| *entry == "message 4\n")
            .count(),
        1
    );
}

fn recall(ring: &mut MessageRing, position: &mut MessagePosition, stride: i64) -> String {
    ring.recall(position, "# instructions\n", "#", stride)
        .unwrap()
        .text
}

fn ring() -> MessageRing {
    let mut ring = MessageRing::default();
    ring.save("oldest", "#");
    ring.save("middle", "#");
    ring.save("newest", "#");
    ring
}

#[test]
fn cycling_wraps_in_both_directions_and_positions_are_session_local() {
    let mut ring = ring();
    let mut previous = MessagePosition::default();
    let mut next = MessagePosition::default();
    assert_eq!(recall(&mut ring, &mut previous, 1), "newest\n");
    assert_eq!(recall(&mut ring, &mut previous, 1), "middle\n");
    assert_eq!(recall(&mut ring, &mut previous, 1), "oldest\n");
    assert_eq!(recall(&mut ring, &mut previous, 1), "newest\n");
    assert_eq!(recall(&mut ring, &mut next, -1), "oldest\n");
    assert_eq!(recall(&mut ring, &mut next, -1), "middle\n");
    assert_eq!(recall(&mut ring, &mut next, -1), "newest\n");
    assert_eq!(recall(&mut ring, &mut next, -1), "oldest\n");
}

#[test]
fn numeric_strides_and_extreme_values_do_not_overflow() {
    let mut ring = ring();
    let mut position = MessagePosition::default();
    assert_eq!(recall(&mut ring, &mut position, 2), "middle\n");
    assert_eq!(recall(&mut ring, &mut position, -3), "middle\n");
    assert_eq!(recall(&mut ring, &mut position, i64::MAX), "oldest\n");
    assert_eq!(recall(&mut ring, &mut position, i64::MIN), "newest\n");
}

#[test]
fn a_new_draft_is_saved_before_recall_but_existing_entries_are_not_reordered() {
    let mut ring = ring();
    let mut position = MessagePosition::default();
    let recalled = ring
        .recall(&mut position, "unfinished draft\n\n# status\n", "#", 1)
        .unwrap();
    assert_eq!(recalled.text, "newest\n");
    assert_eq!(recalled.number, 2);
    assert_eq!(ring.messages.front().unwrap(), "unfinished draft\n");
    assert_eq!(recall(&mut ring, &mut position, -1), "unfinished draft\n");
    let before = ring.messages.clone();
    let recalled = ring
        .recall(&mut MessagePosition::default(), "middle\n", "#", 1)
        .unwrap();
    assert_eq!(recalled.text, "unfinished draft\n");
    assert_eq!(ring.messages, before);
}

#[test]
fn recalling_from_an_empty_ring_does_not_save_or_replace_the_draft() {
    let mut ring = MessageRing::default();
    let mut position = MessagePosition::default();
    assert!(ring.recall(&mut position, "draft", "#", 1).is_none());
    assert!(position.index.is_none());
    assert!(ring.messages.is_empty());
}

#[test]
fn new_drafts_are_preserved_even_when_cycling_forward_or_at_capacity() {
    let mut ring = ring();
    let recalled = ring
        .recall(&mut MessagePosition::default(), "draft", "#", -1)
        .unwrap();
    assert_eq!(recalled.text, "draft\n");
    for n in 0..32 {
        ring.save(&format!("entry {n}"), "#");
    }
    let recalled = ring
        .recall(&mut MessagePosition::default(), "another draft", "#", 1)
        .unwrap();
    assert_eq!(recalled.text, "entry 31\n");
    assert_eq!(ring.messages.len(), 32);
    assert_eq!(ring.messages.front().unwrap(), "another draft\n");
    assert_eq!(ring.messages.back().unwrap(), "entry 1\n");
}

#[test]
fn recall_edits_only_the_message_prefix_and_uses_normal_undo() {
    for (text, expected) in [
        (
            "draft\n\n# instructions\n# status\n",
            "recovered\n\nbody\n\n# instructions\n# status\n",
        ),
        (
            "\n# instructions\n",
            "recovered\n\nbody\n\n# instructions\n",
        ),
        ("# instructions\n", "recovered\n\nbody\n# instructions\n"),
        ("draft", "recovered\n\nbody\n"),
        (
            "draft\n# - >8 -\ndiff --git a/a b/a\n",
            "recovered\n\nbody\n\n# - >8 -\ndiff --git a/a b/a\n",
        ),
    ] {
        let mut ring = MessageRing::default();
        ring.save("recovered\n\nbody", "#");
        let mut buffer = Buffer::scratch();
        buffer
            .edit(Edit {
                snapshot: buffer.snapshot_id(),
                range: ByteOffset(0)..ByteOffset(0),
                text,
                source: EditSource::Command,
            })
            .unwrap();
        buffer.break_history_group();
        let recalled = ring
            .recall(&mut MessagePosition::default(), text, "#", 1)
            .unwrap();
        assert_eq!(recalled.text, "recovered\n\nbody\n");
        buffer
            .edit(Edit {
                snapshot: buffer.snapshot_id(),
                range: recalled.range,
                text: &recalled.text,
                source: EditSource::Command,
            })
            .unwrap();
        assert_eq!(buffer.text().to_string(), expected, "{text:?}");
        buffer.undo().unwrap();
        assert_eq!(buffer.text().to_string(), text);
    }
}

#[test]
fn recall_ranges_remain_byte_aligned_for_unicode_prefixes_and_messages() {
    let mut ring = MessageRing::default();
    ring.save("恢复", "λ");
    let text = "你好\n\nλ instructions\n";
    let recalled = ring
        .recall(&mut MessagePosition::default(), text, "λ", 1)
        .unwrap();
    let mut replaced = text.to_owned();
    replaced.replace_range(recalled.range.start.0..recalled.range.end.0, &recalled.text);
    assert_eq!(replaced, "恢复\n\nλ instructions\n");
}
