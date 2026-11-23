use super::{
    AppliedBatchEdit, AppliedEdit, BatchEdit, Buffer, Edit, EditError, EditSource, SavePoint,
};
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

struct EditEntry {
    /// Start offset in the pre-group document (D0).
    start: ByteOffset,
    removed: Rope,
    inserted: Rope,
}

struct Group {
    id: GroupId,
    kind: EditGroup,
    entries: Vec<EditEntry>,
    primary_entry: Option<usize>,
    point_before: ByteOffset,
    point_after: ByteOffset,
    undo_mark: Option<(usize, usize)>,
    redo_mark: Option<(usize, usize)>,
    modified_before: bool,
    modified_after: bool,
    origin: u64,
}

impl Group {
    /// Returns the post-group (D1) start offset for each entry.
    fn after_starts(&self) -> Vec<ByteOffset> {
        let mut delta: isize = 0;
        let mut starts = Vec::with_capacity(self.entries.len());
        for entry in &self.entries {
            let start = (entry.start.0 as isize + delta) as usize;
            starts.push(ByteOffset(start));
            delta += entry.inserted.len() as isize - entry.removed.len() as isize;
        }
        starts
    }
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
            && previous.entries.len() == group.entries.len()
            && previous.primary_entry == group.primary_entry
        {
            let prev_after_starts = previous.after_starts();
            match group.kind {
                EditGroup::Insert
                    if previous.entries.iter().zip(&group.entries).enumerate().all(
                        |(i, (prev, cur))| {
                            prev.removed.is_empty()
                                && cur.removed.is_empty()
                                && prev_after_starts[i].0 + prev.inserted.len() == cur.start.0
                        },
                    ) =>
                {
                    for (prev, cur) in previous.entries.iter_mut().zip(group.entries) {
                        prev.inserted.append(cur.inserted);
                    }
                    previous.point_after = group.point_after;
                    previous.modified_after = group.modified_after;
                    return;
                }
                EditGroup::BackwardDelete
                    if previous.entries.iter().zip(&group.entries).enumerate().all(
                        |(i, (prev, cur))| {
                            prev.inserted.is_empty()
                                && cur.inserted.is_empty()
                                && cur.start.0 + cur.removed.len() == prev_after_starts[i].0
                        },
                    ) =>
                {
                    let cur_lens: Vec<usize> = group
                        .entries
                        .iter()
                        .map(|entry| entry.removed.len())
                        .collect();
                    previous.undo_mark = previous
                        .undo_mark
                        .map(|(idx, offset)| (idx, offset + cur_lens[idx]))
                        .or(group.undo_mark);
                    for (prev, mut cur) in previous.entries.iter_mut().zip(group.entries) {
                        let removed_len = cur.removed.len();
                        cur.removed.append(prev.removed.clone());
                        prev.removed = cur.removed;
                        prev.start = ByteOffset(prev.start.0 - removed_len);
                    }
                    previous.point_after = group.point_after;
                    previous.modified_after = group.modified_after;
                    return;
                }
                EditGroup::ForwardDelete
                    if previous.entries.iter().zip(&group.entries).enumerate().all(
                        |(i, (prev, cur))| {
                            prev.inserted.is_empty()
                                && cur.inserted.is_empty()
                                && prev_after_starts[i] == cur.start
                        },
                    ) =>
                {
                    let prev_lens: Vec<usize> = previous
                        .entries
                        .iter()
                        .map(|entry| entry.removed.len())
                        .collect();
                    previous.undo_mark = previous.undo_mark.or(group
                        .undo_mark
                        .map(|(idx, offset)| (idx, prev_lens[idx] + offset)));
                    for (prev, cur) in previous.entries.iter_mut().zip(group.entries) {
                        prev.removed.append(cur.removed);
                    }
                    previous.point_after = group.point_after;
                    previous.modified_after = group.modified_after;
                    return;
                }
                _ => {}
            }
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
        let batch = [BatchEdit {
            range: edit.range,
            text: edit.text,
        }];
        self.edit_batch_grouped(edit.snapshot, &batch, edit.source, kind, Some(0))
            .map(|applied| applied.map(|batch| batch.summary))
    }

    pub(crate) fn edit_batch_grouped(
        &mut self,
        snapshot: super::SnapshotId,
        edits: &[BatchEdit<'_>],
        source: EditSource,
        kind: EditGroup,
        primary_index: Option<usize>,
    ) -> Result<Option<AppliedBatchEdit>, EditError> {
        self.validate_batch(snapshot, edits)?;
        let (entries, primary_entry, undo_mark, modified) = {
            let doc = self.document.borrow();
            let mark = doc.mark.position();
            let mut entries = Vec::new();
            let mut primary_entry = None;
            let mut undo_mark = None;
            for (raw_idx, edit) in edits.iter().enumerate() {
                if edit.range.is_empty() && edit.text.is_empty() {
                    continue;
                }
                let idx = entries.len();
                if primary_index == Some(raw_idx) {
                    primary_entry = Some(idx);
                }
                let removed = doc
                    .snapshot
                    .text
                    .slice(edit.range.start.0..edit.range.end.0);
                if undo_mark.is_none()
                    && let Some(m) = mark
                    && m > edit.range.start
                    && m <= edit.range.end
                {
                    undo_mark = Some((idx, m.0 - edit.range.start.0));
                }
                entries.push(EditEntry {
                    start: edit.range.start,
                    removed,
                    inserted: Rope::from(edit.text),
                });
            }
            (entries, primary_entry, undo_mark, doc.modified)
        };
        if entries.is_empty() {
            if kind == EditGroup::Isolated {
                self.document.borrow_mut().history.merging = false;
            }
            return Ok(None);
        }
        let point_before = self.point();
        let applied = self.apply_batch(snapshot, edits, source, primary_index)?;
        if let Some(applied) = applied {
            let group = Group {
                id: GroupId(0),
                kind,
                entries,
                primary_entry,
                point_before,
                point_after: self.point(),
                undo_mark,
                redo_mark: None,
                modified_before: modified,
                modified_after: true,
                origin: self.view_id,
            };
            self.document.borrow_mut().history.record(group);
            return Ok(Some(applied));
        }
        if kind == EditGroup::Isolated {
            self.document.borrow_mut().history.merging = false;
        }
        Ok(None)
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
        let after_starts = group.after_starts();
        let mark = self.mark();
        let (restore_mark, inverse_mark, ranges_and_texts) = if redo {
            let restore = group.redo_mark.and_then(|(idx, rel)| {
                (mark == Some(group.entries[idx].start))
                    .then(|| ByteOffset(after_starts[idx].0 + rel))
            });
            let inverse = mark.and_then(|m| {
                group.entries.iter().enumerate().find_map(|(idx, entry)| {
                    (m > entry.start && m.0 <= entry.start.0 + entry.removed.len())
                        .then(|| (idx, m.0 - entry.start.0))
                })
            });
            let ops: Vec<_> = group
                .entries
                .iter()
                .map(|entry| {
                    (
                        entry.start..ByteOffset(entry.start.0 + entry.removed.len()),
                        entry.inserted.to_string(),
                    )
                })
                .collect();
            (restore, inverse, ops)
        } else {
            let restore = group.undo_mark.and_then(|(idx, rel)| {
                (mark == Some(after_starts[idx]))
                    .then(|| ByteOffset(group.entries[idx].start.0 + rel))
            });
            let inverse = mark.and_then(|m| {
                group.entries.iter().enumerate().find_map(|(idx, entry)| {
                    let start = after_starts[idx];
                    (m > start && m.0 <= start.0 + entry.inserted.len())
                        .then(|| (idx, m.0 - start.0))
                })
            });
            let ops: Vec<_> = group
                .entries
                .iter()
                .enumerate()
                .map(|(idx, entry)| {
                    let start = after_starts[idx];
                    (
                        start..ByteOffset(start.0 + entry.inserted.len()),
                        entry.removed.to_string(),
                    )
                })
                .collect();
            (restore, inverse, ops)
        };
        let batch: Vec<BatchEdit<'_>> = ranges_and_texts
            .iter()
            .map(|(range, text)| BatchEdit {
                range: range.clone(),
                text: text.as_str(),
            })
            .collect();
        let result = self.apply_batch(
            self.snapshot_id(),
            &batch,
            if redo {
                EditSource::Redo
            } else {
                EditSource::Undo
            },
            None,
        );
        let applied = match result {
            Ok(a) => a.map(|batch| batch.summary),
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
        if let Some(position) = restore_mark {
            doc.mark.set(Some(position), false);
        }
        if redo {
            group.undo_mark = inverse_mark;
            let redo_point = group
                .primary_entry
                .map_or(group.point_after.0, |idx| after_starts[idx].0);
            self.point.set(redo_point);
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
            buffer.document.borrow_mut().history.undo[0].entries[0]
                .inserted
                .len(),
            COUNT
        );
        assert!(
            buffer.document.borrow_mut().history.undo[0].entries[0]
                .removed
                .is_empty()
        );
        buffer.break_history_group();
        for i in (0..COUNT).rev() {
            edit(&mut buffer, i..i + 1, "", EditGroup::BackwardDelete);
        }
        assert_eq!(buffer.document.borrow_mut().history.undo.len(), 2);
        assert_eq!(
            buffer.document.borrow_mut().history.undo[1].entries[0]
                .removed
                .len(),
            COUNT
        );
        assert!(
            buffer.document.borrow_mut().history.undo[1].entries[0]
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

    #[test]
    fn batch_edit_is_one_undo_group_merges_sustained_edits_and_tracks_save_point() {
        let mut first = Buffer::new(
            None,
            crate::files::DecodedFile {
                text: "aa bb cc".into(),
                format: crate::files::FileFormat::default(),
            },
        );
        let mut second = first.new_view();
        second.set_point(ByteOffset(6)).unwrap();

        let saved = first.begin_save();
        first.mark_saved_at(saved).unwrap();
        assert!(!first.is_modified());

        // Typing '1' then '2' at offsets 2, 5, 8 should merge into one undo group.
        first
            .edit_batch_grouped(
                first.snapshot_id(),
                &[
                    BatchEdit {
                        range: ByteOffset(2)..ByteOffset(2),
                        text: "1",
                    },
                    BatchEdit {
                        range: ByteOffset(5)..ByteOffset(5),
                        text: "1",
                    },
                    BatchEdit {
                        range: ByteOffset(8)..ByteOffset(8),
                        text: "1",
                    },
                ],
                EditSource::Command,
                EditGroup::Insert,
                Some(0),
            )
            .unwrap()
            .unwrap();
        assert_eq!(first.text().to_string(), "aa1 bb1 cc1");
        assert_eq!(second.point(), ByteOffset(8));

        first
            .edit_batch_grouped(
                first.snapshot_id(),
                &[
                    BatchEdit {
                        range: ByteOffset(3)..ByteOffset(3),
                        text: "2",
                    },
                    BatchEdit {
                        range: ByteOffset(7)..ByteOffset(7),
                        text: "2",
                    },
                    BatchEdit {
                        range: ByteOffset(11)..ByteOffset(11),
                        text: "2",
                    },
                ],
                EditSource::Command,
                EditGroup::Insert,
                Some(0),
            )
            .unwrap()
            .unwrap();
        assert_eq!(first.text().to_string(), "aa12 bb12 cc12");
        assert_eq!(first.document.borrow().history.undo.len(), 1);
        assert!(first.is_modified());

        // One undo restores all three sites and returns to the clean save point.
        first.undo().unwrap().unwrap();
        assert_eq!(first.text().to_string(), "aa bb cc");
        assert!(!first.is_modified());
        assert_eq!(second.point(), ByteOffset(6));

        // Redo reapplies all three sites atomically.
        first.redo().unwrap().unwrap();
        assert_eq!(first.text().to_string(), "aa12 bb12 cc12");
        assert!(first.is_modified());

        // Consecutive multi-site backward deletes also merge into a single group.
        first.break_history_group();
        first
            .edit_batch_grouped(
                first.snapshot_id(),
                &[
                    BatchEdit {
                        range: ByteOffset(3)..ByteOffset(4),
                        text: "",
                    },
                    BatchEdit {
                        range: ByteOffset(8)..ByteOffset(9),
                        text: "",
                    },
                    BatchEdit {
                        range: ByteOffset(13)..ByteOffset(14),
                        text: "",
                    },
                ],
                EditSource::Command,
                EditGroup::BackwardDelete,
                Some(0),
            )
            .unwrap()
            .unwrap();
        first
            .edit_batch_grouped(
                first.snapshot_id(),
                &[
                    BatchEdit {
                        range: ByteOffset(2)..ByteOffset(3),
                        text: "",
                    },
                    BatchEdit {
                        range: ByteOffset(6)..ByteOffset(7),
                        text: "",
                    },
                    BatchEdit {
                        range: ByteOffset(10)..ByteOffset(11),
                        text: "",
                    },
                ],
                EditSource::Command,
                EditGroup::BackwardDelete,
                Some(0),
            )
            .unwrap()
            .unwrap();
        assert_eq!(first.text().to_string(), "aa bb cc");
        assert_eq!(first.document.borrow().history.undo.len(), 2);
        first.undo().unwrap().unwrap();
        assert_eq!(first.text().to_string(), "aa12 bb12 cc12");
    }

    #[test]
    fn touching_batch_edits_map_markers_and_undo_mark_without_double_collapse() {
        let mut first = Buffer::new(
            None,
            crate::files::DecodedFile {
                text: "abcd".into(),
                format: crate::files::FileFormat::default(),
            },
        );
        let mut second = first.new_view();
        second.set_point(ByteOffset(2)).unwrap();
        first.set_mark(ByteOffset(2)).unwrap();

        // Edit 0: 0..1 -> "XXX" (new_range 0..3), Edit 1: 1..2 -> "" (new_range 3..3).
        first
            .edit_batch_grouped(
                first.snapshot_id(),
                &[
                    BatchEdit {
                        range: ByteOffset(0)..ByteOffset(1),
                        text: "XXX",
                    },
                    BatchEdit {
                        range: ByteOffset(1)..ByteOffset(2),
                        text: "",
                    },
                ],
                EditSource::Command,
                EditGroup::Isolated,
                Some(0),
            )
            .unwrap()
            .unwrap();
        assert_eq!(first.text().to_string(), "XXXcd");
        assert_eq!(second.point(), ByteOffset(3));
        assert_eq!(first.mark(), Some(ByteOffset(3)));

        first.undo().unwrap().unwrap();
        assert_eq!(first.text().to_string(), "abcd");
        assert_eq!(first.mark(), Some(ByteOffset(2)));
    }
}
