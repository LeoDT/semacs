use super::{AppliedEdit, Buffer, Edit, EditError, EditSource, SavePoint};
use crate::text::{ByteOffset, Rope};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum EditGroup {
    Insert,
    BackwardDelete,
    ForwardDelete,
    #[default]
    Isolated,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct GroupId(u64);

/// A stable position in the linear history: before any group, or after one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HistoryState {
    Base,
    After(GroupId),
}

struct Group {
    id: GroupId,
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
    origin: u64,
}

#[derive(Default)]
pub(super) struct History {
    undo: Vec<Group>,
    redo: Vec<Group>,
    merging: bool,
    next_group_id: u64,
}

impl History {
    fn record(&mut self, mut group: Group) {
        self.redo.clear();
        self.next_group_id = self
            .next_group_id
            .checked_add(1)
            .expect("history group id exhausted");
        group.id = GroupId(self.next_group_id);
        if self.merging
            && let Some(previous) = self.undo.last_mut()
            && previous.origin == group.origin
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

    pub(super) fn current_state(&self) -> HistoryState {
        self.undo
            .last()
            .map_or(HistoryState::Base, |group| HistoryState::After(group.id))
    }

    fn state_index(&self, state: HistoryState) -> Option<usize> {
        match state {
            HistoryState::Base => Some(0),
            HistoryState::After(id) => {
                if let Some(index) = self.undo.iter().position(|group| group.id == id) {
                    Some(index + 1)
                } else {
                    self.redo
                        .iter()
                        .position(|group| group.id == id)
                        .map(|index| self.undo.len() + self.redo.len() - index)
                }
            }
        }
    }

    fn group_mut(&mut self, forward_index: usize) -> Option<&mut Group> {
        if forward_index < self.undo.len() {
            self.undo.get_mut(forward_index)
        } else {
            let reverse_index = forward_index - self.undo.len();
            let index = self.redo.len().checked_sub(1 + reverse_index)?;
            self.redo.get_mut(index)
        }
    }

    /// Marks one history state clean or dirty. A state is represented by the
    /// group that ends it and by the group that starts after it, so both cached
    /// markers must move together.
    fn set_state_modified(&mut self, state: HistoryState, modified: bool) -> bool {
        let Some(index) = self.state_index(state) else {
            return false;
        };
        let total = self.undo.len() + self.redo.len();
        if index > 0
            && let Some(group) = self.group_mut(index - 1)
        {
            group.modified_after = modified;
        }
        if index < total
            && let Some(group) = self.group_mut(index)
        {
            group.modified_before = modified;
        }
        true
    }

    /// Applies a completed save at an arbitrary state. If the buffer has moved
    /// away from that state, its current state becomes dirty because the disk
    /// now holds the saved snapshot, not the current text.
    pub(super) fn mark_saved(&mut self, state: HistoryState) -> bool {
        if !self.set_state_modified(state, false) {
            return false;
        }
        let current = self.current_state();
        if current != state {
            self.set_state_modified(current, true);
        }
        true
    }
}

impl Buffer {
    pub fn break_history_group(&mut self) {
        self.document.borrow_mut().history.merging = false;
    }

    /// The current history state, captured when a save starts.
    pub(crate) fn save_point(&self) -> SavePoint {
        let doc = self.document.borrow();
        SavePoint {
            buffer: doc.snapshot.id.buffer,
            state: if doc.message_log || doc.status.is_some() || doc.process.is_some() {
                super::SaveBoundary::Generated(doc.snapshot.id.version)
            } else {
                super::SaveBoundary::History(doc.history.current_state())
            },
        }
    }

    /// Starts a save: the boundary must not keep merging with later typing, or
    /// the captured group would come to mean a different text.
    pub fn begin_save(&mut self) -> SavePoint {
        self.break_history_group();
        self.save_point()
    }

    /// Applies an asynchronous save result to the state it captured. A result
    /// whose history state is no longer reachable cannot clear dirty. Generated
    /// buffers instead compare versions because their updates have no undo state.
    pub fn mark_saved_at(&mut self, point: SavePoint) -> Result<(), EditError> {
        if point.buffer != self.snapshot_id().buffer {
            return Err(EditError::WrongTarget);
        }
        let mut doc = self.document.borrow_mut();
        match point.state {
            super::SaveBoundary::History(state) => {
                if !doc.history.mark_saved(state) {
                    return Err(EditError::StaleSnapshot);
                }
                doc.modified = doc.history.current_state() != state;
            }
            super::SaveBoundary::Generated(version) => {
                doc.modified = doc.snapshot.id.version != version;
            }
        }
        doc.history.merging = false;
        doc.touch();
        Ok(())
    }

    pub(crate) fn edit_grouped(
        &mut self,
        edit: Edit<'_>,
        kind: EditGroup,
    ) -> Result<Option<AppliedEdit>, EditError> {
        self.validate_edit(&edit)?;
        let (removed, mark, modified) = {
            let doc = self.document.borrow();
            (
                doc.snapshot
                    .text
                    .slice(edit.range.start.0..edit.range.end.0),
                doc.mark.position(),
                doc.modified,
            )
        };
        let removed_len = edit.range.end.0 - edit.range.start.0;
        let group = Group {
            id: GroupId(0),
            kind,
            start: edit.range.start,
            removed,
            inserted: Rope::from(edit.text),
            point_before: self.point(),
            point_after: ByteOffset(edit.range.start.0 + edit.text.len()),
            undo_mark: mark
                .filter(|m| *m > edit.range.start && m.0 <= edit.range.start.0 + removed_len)
                .map(|m| m.0 - edit.range.start.0),
            redo_mark: None,
            modified_before: modified,
            modified_after: true,
            origin: self.view_id,
        };
        let applied = self.apply_edit(edit)?;
        let mut doc = self.document.borrow_mut();
        if applied.is_some() {
            doc.history.record(group);
        } else if kind == EditGroup::Isolated {
            doc.history.merging = false;
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
        self.check_editable()?;
        self.break_history_group();
        let mut group = {
            let mut doc = self.document.borrow_mut();
            let stack = if redo {
                &mut doc.history.redo
            } else {
                &mut doc.history.undo
            };
            let Some(group) = stack.pop() else {
                return Ok(None);
            };
            group
        };
        let (removed, inserted, restore_mark) = if redo {
            (&group.removed, &group.inserted, group.redo_mark)
        } else {
            (&group.inserted, &group.removed, group.undo_mark)
        };
        let mark = self.mark();
        let restore_mark = restore_mark.filter(|_| mark == Some(group.start));
        let inverse_mark = {
            let m = self.mark();
            m.filter(|m| *m > group.start && m.0 <= group.start.0 + removed.len())
                .map(|m| m.0 - group.start.0)
        };
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
            Ok(a) => a,
            Err(e) => {
                let mut d = self.document.borrow_mut();
                if redo {
                    d.history.redo.push(group);
                } else {
                    d.history.undo.push(group);
                }
                return Err(e);
            }
        };
        let mut doc = self.document.borrow_mut();
        if let Some(offset) = restore_mark {
            doc.mark
                .set(Some(ByteOffset(group.start.0 + offset)), false);
        }
        if redo {
            group.undo_mark = inverse_mark;
            self.point.set(group.start.0);
            doc.modified = group.modified_after;
            doc.history.undo.push(group);
        } else {
            group.redo_mark = inverse_mark;
            self.point.set(group.point_before.0);
            doc.modified = group.modified_before;
            doc.history.redo.push(group);
        }
        doc.mark.deactivate();
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
        assert_eq!(buffer.document.borrow_mut().history.undo.len(), 1);
        assert_eq!(
            buffer.document.borrow_mut().history.undo[0].inserted.len(),
            COUNT
        );
        assert!(
            buffer.document.borrow_mut().history.undo[0]
                .removed
                .is_empty()
        );
        buffer.break_history_group();
        for i in (0..COUNT).rev() {
            edit(&mut buffer, i..i + 1, "", EditGroup::BackwardDelete);
        }
        assert_eq!(buffer.document.borrow_mut().history.undo.len(), 2);
        assert_eq!(
            buffer.document.borrow_mut().history.undo[1].removed.len(),
            COUNT
        );
        assert!(
            buffer.document.borrow_mut().history.undo[1]
                .inserted
                .is_empty()
        );
        buffer.undo().unwrap().unwrap();
        assert_eq!(buffer.text().len(), COUNT);
        buffer.undo().unwrap().unwrap();
        assert!(buffer.text().is_empty());
        assert_eq!(buffer.document.borrow_mut().history.redo.len(), 2);
        edit(&mut buffer, 0..0, "new", EditGroup::Insert);
        assert!(buffer.document.borrow_mut().history.redo.is_empty());
        assert_eq!(buffer.document.borrow_mut().history.undo.len(), 1);
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
        assert!(buffer.document.borrow_mut().history.undo.is_empty());
        assert_eq!(buffer.document.borrow_mut().history.redo.len(), 1);
        assert_eq!(buffer.snapshot_id(), snapshot);
        assert!(!buffer.is_modified());
        buffer.redo().unwrap().unwrap();
        assert_eq!(buffer.text().to_string(), "中");
    }

    #[test]
    fn temporary_inhibition_preserves_shared_history_and_saved_state() {
        let mut first = Buffer::scratch();
        edit(&mut first, 0..0, "a", EditGroup::Isolated);
        let saved = first.begin_save();
        first.mark_saved_at(saved).unwrap();
        edit(&mut first, 1..1, "b", EditGroup::Isolated);
        first.undo().unwrap();
        let mut second = first.new_view();
        let snapshot = first.snapshot_id();
        first.set_edit_inhibition(Some("finishing"));
        for buffer in [&mut first, &mut second] {
            assert!(!buffer.is_modified());
            assert!(!buffer.is_read_only());
            assert_eq!(buffer.undo(), Err(EditError::EditingInhibited("finishing")));
            assert_eq!(buffer.redo(), Err(EditError::EditingInhibited("finishing")));
            assert_eq!(
                buffer.edit(Edit {
                    snapshot,
                    range: ByteOffset(0)..ByteOffset(1),
                    text: "bad",
                    source: EditSource::Command,
                }),
                Err(EditError::EditingInhibited("finishing"))
            );
            assert_eq!(buffer.snapshot_id(), snapshot);
        }
        first.set_edit_inhibition(None);
        second.redo().unwrap().unwrap();
        assert_eq!(first.text().to_string(), "ab");
        assert!(first.is_modified());
        first.undo().unwrap().unwrap();
        assert_eq!(second.text().to_string(), "a");
        assert!(!second.is_modified());
    }

    #[test]
    fn history_groups_never_merge_across_view_origins() {
        let mut first = Buffer::scratch();
        edit(&mut first, 0..0, "a", EditGroup::Insert);
        let mut second = first.new_view();
        // Creating a view is inert: the original view can continue its group.
        edit(&mut first, 1..1, "b", EditGroup::Insert);
        assert_eq!(first.document.borrow_mut().history.undo.len(), 1);
        edit(&mut second, 2..2, "c", EditGroup::Insert);
        let doc = first.document.borrow_mut();
        assert_eq!(doc.history.undo.len(), 2);
        assert_ne!(doc.history.undo[0].origin, doc.history.undo[1].origin);
    }
}
