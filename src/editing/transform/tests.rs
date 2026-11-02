use crate::buffer::{Buffer, Edit, EditSource};
use crate::command::{Command, Scope};
use crate::editing::EditTarget;
use crate::files::{DecodedFile, FileFormat};
use crate::keymap::Keymap;
use crate::text::ByteOffset;
use gpui_kit::Keystroke;

fn target() -> EditTarget {
    let mut buffer = Buffer::new(
        None,
        DecodedFile {
            text: "abc def".into(),
            format: FileFormat::default(),
        },
    );
    buffer.set_mark(ByteOffset(0)).unwrap();
    buffer.set_point(ByteOffset(3)).unwrap();
    EditTarget::new(buffer)
}

#[test]
fn transform_uses_isolated_history_and_existing_point_mark_rules() {
    let mut target = target();
    let (request, input) = target.capture_region(100).unwrap();
    assert_eq!(input, "abc");
    target.apply_region_transform(request, "中文🙂").unwrap();
    assert_eq!(target.buffer().text().to_string(), "中文🙂 def");
    assert_eq!(target.buffer().point().0, "中文🙂".len());
    assert!(!target.buffer().mark_active());
    assert!(target.buffer().is_modified());
    target.execute(target.token(), Command::Undo).unwrap();
    assert_eq!(target.buffer().text().to_string(), "abc def");
    assert_eq!(target.buffer().point(), ByteOffset(3));
    assert!(!target.buffer().is_modified());
    target.execute(target.token(), Command::UndoRedo).unwrap();
    assert_eq!(target.buffer().text().to_string(), "中文🙂 def");
}

#[test]
fn capture_rejects_no_region_read_only_and_oversized_input() {
    let mut target = target();
    assert!(target.capture_region(2).is_err());
    target.buffer.deactivate_mark();
    assert!(target.capture_region(100).is_err());
    target.buffer.set_mark(ByteOffset(0)).unwrap();
    target.set_read_only(true);
    assert!(target.capture_region(100).is_err());
    assert_eq!(target.buffer().text().to_string(), "abc def");
}

#[test]
fn composition_is_a_barrier_at_dispatch_and_commit() {
    let mut target = target();
    let (request, _) = target.capture_region(100).unwrap();
    target
        .replace_and_mark(target.token(), None, "ni", None)
        .unwrap();
    assert!(target.capture_region(100).is_err());
    assert!(target.apply_region_transform(request, "ABC").is_err());
    assert_eq!(target.buffer().text().to_string(), "abc def");
    assert!(target.input().is_composing());
}

#[test]
fn changing_read_only_after_capture_prevents_commit() {
    let mut target = target();
    let (request, _) = target.capture_region(100).unwrap();
    target.set_read_only(true);
    assert!(target.apply_region_transform(request, "ABC").is_err());
    assert_eq!(target.buffer().text().to_string(), "abc def");
}

#[test]
fn shared_edit_and_undo_cannot_resurrect_an_old_result() {
    let mut target = target();
    let (request, _) = target.capture_region(100).unwrap();
    let mut other = EditTarget::new(target.buffer().new_view());
    other.execute(other.token(), Command::DeleteChar).unwrap();
    other.execute(other.token(), Command::Undo).unwrap();
    assert_eq!(target.buffer().text().to_string(), "abc def");
    assert!(target.apply_region_transform(request, "ABC").is_err());
}

#[test]
fn region_motion_input_lifetime_and_wrong_buffer_invalidate_results() {
    let mut target = target();
    let (request, _) = target.capture_region(100).unwrap();
    target
        .execute(target.token(), Command::ForwardChar)
        .unwrap();
    assert!(target.apply_region_transform(request, "ABC").is_err());
    target.buffer.set_point(ByteOffset(3)).unwrap();
    let (request, _) = target.capture_region(100).unwrap();
    target.end_input();
    target.resume_input();
    assert!(target.apply_region_transform(request, "ABC").is_err());
    let (request, _) = target.capture_region(100).unwrap();
    let mut other = EditTarget::new(Buffer::named("other"));
    assert!(other.apply_region_transform(request, "ABC").is_err());
}

#[test]
fn pending_prefix_blocks_capture_and_commit_until_cleared() {
    let mut target = target();
    let (request, _) = target.capture_region(100).unwrap();
    let map = Keymap::for_scope(Scope::Edit);
    target
        .dispatch_keystroke(
            target.token(),
            &Keystroke::parse("ctrl-x").unwrap(),
            false,
            &[&map],
            None,
        )
        .unwrap();
    assert!(target.capture_region(100).is_err());
    assert!(target.apply_region_transform(request, "ABC").is_err());
    target.clear_pending();
    let (request, _) = target.capture_region(100).unwrap();
    target.apply_region_transform(request, "ABC").unwrap();
    assert_eq!(target.buffer().text().to_string(), "ABC def");
}

#[test]
fn restoring_point_or_shared_mark_does_not_resurrect_a_result() {
    let mut target = target();
    let (request, _) = target.capture_region(100).unwrap();
    target
        .execute(target.token(), Command::BackwardChar)
        .unwrap();
    target
        .execute(target.token(), Command::ForwardChar)
        .unwrap();
    assert_eq!(target.buffer().point(), ByteOffset(3));
    assert!(target.apply_region_transform(request, "ABC").is_err());

    let (request, _) = target.capture_region(100).unwrap();
    target
        .set_point_from_pointer(target.token(), ByteOffset(1))
        .unwrap();
    target
        .set_point_from_pointer(target.token(), ByteOffset(3))
        .unwrap();
    assert!(target.apply_region_transform(request, "ABC").is_err());

    let (request, _) = target.capture_region(100).unwrap();
    let mut other = target.buffer().new_view();
    other.set_mark(ByteOffset(1)).unwrap();
    other.set_mark(ByteOffset(0)).unwrap();
    assert!(target.apply_region_transform(request, "ABC").is_err());

    let (request, _) = target.capture_region(100).unwrap();
    other.deactivate_mark();
    other.set_mark(ByteOffset(0)).unwrap();
    assert!(target.apply_region_transform(request, "ABC").is_err());
    assert_eq!(target.buffer().text().to_string(), "abc def");
}

#[test]
fn unrelated_view_motion_and_save_metadata_do_not_invalidate_region() {
    let mut target = target();
    let (request, _) = target.capture_region(100).unwrap();
    let mut other = target.buffer().new_view();
    other.set_point(ByteOffset(4)).unwrap();
    let saved = other.begin_save();
    other.mark_saved_at(saved).unwrap();
    target.apply_region_transform(request, "ABC").unwrap();
    assert_eq!(target.buffer().text().to_string(), "ABC def");
}

#[test]
fn read_only_prefix_is_not_bypassed_by_region_transform() {
    let mut target = target();
    target.buffer = target.buffer.with_read_only_prefix(ByteOffset(1)).unwrap();
    assert!(target.capture_region(100).is_err());
    let mut other = target.buffer.new_view();
    assert!(
        other
            .edit(Edit {
                snapshot: other.snapshot_id(),
                range: ByteOffset(0)..ByteOffset(3),
                text: "ABC",
                source: EditSource::Command,
            })
            .is_err()
    );
}
