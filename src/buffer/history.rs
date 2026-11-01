use super::{AppliedEdit, Buffer, Edit, EditError, EditSource, SnapshotId};
use crate::text::{ByteOffset, Rope};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum EditGroup {
    Insert,
    BackwardDelete,
    ForwardDelete,
    #[default]
    Isolated,
}

struct Group {
    kind: EditGroup,
    start: ByteOffset,
    removed: Rope,
    inserted: Rope,
    point_before: ByteOffset,
    point_after: ByteOffset,
    undo_mark: Option<usize>,
    redo_mark: Option<usize>,
    modified_before: bool,
    modified_after: bool,
}

#[derive(Default)]
pub(super) struct History {
    undo: Vec<Group>,
    redo: Vec<Group>,
    merging: bool,
}

impl History {
    fn record(&mut self, mut group: Group) {
        self.redo.clear();
        if self.merging
            && let Some(previous) = self.undo.last_mut()
            && previous.kind == group.kind
            && previous.point_after == group.point_before
        {
            match group.kind {
                EditGroup::Insert
                    if previous.removed.is_empty()
                        && group.removed.is_empty()
                        && previous.start.0 + previous.inserted.len() == group.start.0 =>
                {
                    previous.inserted.append(group.inserted);
                }
                EditGroup::BackwardDelete
                    if previous.inserted.is_empty()
                        && group.inserted.is_empty()
                        && group.start.0 + group.removed.len() == previous.start.0 =>
                {
                    previous.undo_mark = previous
                        .undo_mark
                        .map(|offset| offset + group.removed.len())
                        .or(group.undo_mark);
                    group.removed.append(previous.removed.clone());
                    previous.removed = group.removed;
                    previous.start = group.start;
                }
                EditGroup::ForwardDelete
                    if previous.inserted.is_empty()
                        && group.inserted.is_empty()
                        && previous.start == group.start =>
                {
                    previous.undo_mark = previous.undo_mark.or(group
                        .undo_mark
                        .map(|offset| previous.removed.len() + offset));
                    previous.removed.append(group.removed);
                }
                _ => {
                    self.merging = group.kind != EditGroup::Isolated;
                    self.undo.push(group);
                    return;
                }
            }
            previous.point_after = group.point_after;
            previous.modified_after = group.modified_after;
            return;
        }
        self.merging = group.kind != EditGroup::Isolated;
        self.undo.push(group);
    }
}

fn collapsed_mark(buffer: &Buffer, start: ByteOffset, len: usize) -> Option<usize> {
    buffer
        .mark
        .filter(|mark| *mark > start && mark.0 <= start.0 + len)
        .map(|mark| mark.0 - start.0)
}

impl Buffer {
    pub fn break_history_group(&mut self) {
        self.history.merging = false;
    }

    /// Called only after a successful save of the current committed snapshot.
    pub fn mark_saved(&mut self, snapshot: SnapshotId) -> Result<(), EditError> {
        self.validate_snapshot(snapshot)?;
        self.break_history_group();
        self.modified = false;
        if let Some(group) = self.history.undo.last_mut() {
            group.modified_after = false;
        }
        if let Some(group) = self.history.redo.last_mut() {
            group.modified_before = false;
        }
        Ok(())
    }

    pub(crate) fn edit_grouped(
        &mut self,
        edit: Edit<'_>,
        kind: EditGroup,
    ) -> Result<Option<AppliedEdit>, EditError> {
        self.validate_edit(&edit)?;
        let group = Group {
            kind,
            start: edit.range.start,
            removed: self.text().slice(edit.range.start.0..edit.range.end.0),
            inserted: Rope::from(edit.text),
            point_before: self.point,
            point_after: ByteOffset(edit.range.start.0 + edit.text.len()),
            undo_mark: collapsed_mark(
                self,
                edit.range.start,
                edit.range.end.0 - edit.range.start.0,
            ),
            redo_mark: None,
            modified_before: self.modified,
            modified_after: true,
        };
        let applied = self.apply_edit(edit)?;
        if applied.is_some() {
            self.history.record(group);
        } else if kind == EditGroup::Isolated {
            self.break_history_group();
        }
        Ok(applied)
    }

    pub fn undo(&mut self) -> Result<Option<AppliedEdit>, EditError> {
        self.replay(false)
    }

    pub fn redo(&mut self) -> Result<Option<AppliedEdit>, EditError> {
        self.replay(true)
    }

    fn replay(&mut self, redo: bool) -> Result<Option<AppliedEdit>, EditError> {
        if self.read_only {
            return Err(EditError::ReadOnly);
        }
        self.break_history_group();
        let stack = if redo {
            &mut self.history.redo
        } else {
            &mut self.history.undo
        };
        let Some(mut group) = stack.pop() else {
            return Ok(None);
        };
        let (removed, inserted, restore_mark) = if redo {
            (&group.removed, &group.inserted, group.redo_mark)
        } else {
            (&group.inserted, &group.removed, group.undo_mark)
        };
        let restore_mark = restore_mark.filter(|_| self.mark == Some(group.start));
        let inverse_mark = collapsed_mark(self, group.start, removed.len());
        let text = inserted.to_string();
        let result = self.apply_edit(Edit {
            snapshot: self.snapshot_id(),
            range: group.start..ByteOffset(group.start.0 + removed.len()),
            text: &text,
            source: if redo {
                EditSource::Redo
            } else {
                EditSource::Undo
            },
        });
        let applied = match result {
            Ok(applied) => applied,
            Err(error) => {
                if redo {
                    self.history.redo.push(group);
                } else {
                    self.history.undo.push(group);
                }
                return Err(error);
            }
        };
        if let Some(offset) = restore_mark {
            self.mark = Some(ByteOffset(group.start.0 + offset));
        }
        if redo {
            group.undo_mark = inverse_mark;
            self.point = group.start;
            self.modified = group.modified_after;
            self.history.undo.push(group);
        } else {
            group.redo_mark = inverse_mark;
            self.point = group.point_before;
            self.modified = group.modified_before;
            self.history.redo.push(group);
        }
        self.deactivate_mark();
        Ok(applied)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(buffer: &mut Buffer, range: std::ops::Range<usize>, text: &str, kind: EditGroup) {
        buffer
            .edit_grouped(
                Edit {
                    snapshot: buffer.snapshot_id(),
                    range: ByteOffset(range.start)..ByteOffset(range.end),
                    text,
                    source: EditSource::Command,
                },
                kind,
            )
            .unwrap();
    }

    #[test]
    fn sustained_typing_and_deletion_keep_one_compact_change_per_group() {
        const COUNT: usize = 2048;
        let mut buffer = Buffer::scratch();
        for i in 0..COUNT {
            edit(&mut buffer, i..i, "x", EditGroup::Insert);
        }
        assert_eq!(buffer.history.undo.len(), 1);
        assert_eq!(buffer.history.undo[0].inserted.len(), COUNT);
        assert!(buffer.history.undo[0].removed.is_empty());
        buffer.break_history_group();
        for i in (0..COUNT).rev() {
            edit(&mut buffer, i..i + 1, "", EditGroup::BackwardDelete);
        }
        assert_eq!(buffer.history.undo.len(), 2);
        assert_eq!(buffer.history.undo[1].removed.len(), COUNT);
        assert!(buffer.history.undo[1].inserted.is_empty());
        buffer.undo().unwrap().unwrap();
        assert_eq!(buffer.text().len(), COUNT);
        buffer.undo().unwrap().unwrap();
        assert!(buffer.text().is_empty());
        assert_eq!(buffer.history.redo.len(), 2);
        edit(&mut buffer, 0..0, "new", EditGroup::Insert);
        assert!(buffer.history.redo.is_empty());
        assert_eq!(buffer.history.undo.len(), 1);
    }

    #[test]
    fn invalid_or_empty_edits_do_not_create_history_or_discard_redo() {
        let mut buffer = Buffer::scratch();
        edit(&mut buffer, 0..0, "中", EditGroup::Insert);
        buffer.undo().unwrap().unwrap();
        let snapshot = buffer.snapshot_id();
        assert_eq!(
            buffer.edit(Edit {
                snapshot,
                range: ByteOffset(1)..ByteOffset(1),
                text: "bad",
                source: EditSource::Command,
            }),
            Err(EditError::InvalidRange)
        );
        edit(&mut buffer, 0..0, "", EditGroup::Isolated);
        assert!(buffer.history.undo.is_empty());
        assert_eq!(buffer.history.redo.len(), 1);
        assert_eq!(buffer.snapshot_id(), snapshot);
        assert!(!buffer.is_modified());
        buffer.redo().unwrap().unwrap();
        assert_eq!(buffer.text().to_string(), "中");
    }
}
