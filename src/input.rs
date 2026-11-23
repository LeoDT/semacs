//! Target-bound platform input, with a disposable projection for preedit text.

use std::ops::Range;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::buffer::{
    AppliedEdit, BatchEdit, Buffer, BufferId, Edit, EditError, EditGroup, EditSource, SnapshotId,
};
use crate::text::{ByteOffset, OffsetUtf16, Rope, clip_utf16_range, range_to_utf16};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SessionToken {
    target: BufferId,
    serial: u64,
}

impl SessionToken {
    fn new(target: BufferId) -> Self {
        static NEXT_SERIAL: AtomicU64 = AtomicU64::new(1);
        Self {
            target,
            serial: NEXT_SERIAL
                .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
                .expect("input session identity exhausted"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DisplayId {
    pub committed: SnapshotId,
    pub session: SessionToken,
    pub revision: u64,
}

/// A byte range replacement between two display identities.
///
/// Both ranges are projection coordinates, not committed-buffer coordinates.
/// Moving text out of the marked overlay may edit the buffer without changing
/// the corresponding visible text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisplayChange {
    pub before: DisplayId,
    pub after: DisplayId,
    pub old_range: Range<ByteOffset>,
    pub new_range: Range<ByteOffset>,
}

impl DisplayChange {
    /// Compose two adjacent changes so a reflow can reuse the index that predates both.
    pub fn compose(&self, next: &Self) -> Self {
        assert_eq!(
            self.after, next.before,
            "display changes must be contiguous"
        );
        let old_start = self.old_range.start.0;
        let old_end = self.old_range.end.0;
        let new_end = self.new_range.end.0;
        let next_old_start = next.old_range.start.0;
        let next_old_end = next.old_range.end.0;
        let next_new_end = next.new_range.end.0;
        let self_delta = new_end as isize - old_end as isize;
        let next_delta = next_new_end as isize - next_old_end as isize;

        let start = old_start.min(next_old_start);
        let next_old_end_in_self = if next_old_end <= old_start {
            next_old_end
        } else if next_old_end >= new_end {
            (next_old_end as isize - self_delta) as usize
        } else {
            old_start
        };
        let old_end = old_end.max(next_old_end_in_self);
        let self_new_end_in_next = if new_end <= next_old_start {
            new_end
        } else if new_end >= next_old_end {
            (new_end as isize + next_delta) as usize
        } else {
            next_old_start
        };
        let new_end = self_new_end_in_next.max(next_new_end);

        Self {
            before: self.before,
            after: next.after,
            old_range: ByteOffset(start)..ByteOffset(old_end),
            new_range: ByteOffset(start)..ByteOffset(new_end),
        }
    }
}

#[derive(Clone)]
pub struct DisplaySnapshot {
    id: DisplayId,
    text: Rope,
    selected: Range<ByteOffset>,
    marked: Option<Range<ByteOffset>>,
    region: Option<Range<ByteOffset>>,
}

impl DisplaySnapshot {
    pub fn id(&self) -> DisplayId {
        self.id
    }

    pub fn text(&self) -> &Rope {
        &self.text
    }

    pub fn selected_range(&self) -> Range<ByteOffset> {
        self.selected.clone()
    }

    pub fn marked_range(&self) -> Option<Range<ByteOffset>> {
        self.marked.clone()
    }

    pub(crate) fn project_committed_offset(&self, offset: ByteOffset) -> ByteOffset {
        match &self.marked {
            Some(marked) if offset > marked.start => {
                ByteOffset(offset.0 + (marked.end.0 - marked.start.0))
            }
            _ => offset,
        }
    }

    pub(crate) fn project_committed_range(
        &self,
        range: Range<ByteOffset>,
    ) -> [Option<Range<ByteOffset>>; 2] {
        let projected =
            self.project_committed_offset(range.start)..self.project_committed_offset(range.end);
        match &self.marked {
            Some(marked) => [
                Some(projected.start..projected.end.min(marked.start))
                    .filter(|range| !range.is_empty()),
                Some(projected.start.max(marked.end)..projected.end)
                    .filter(|range| !range.is_empty()),
            ],
            None => [Some(projected).filter(|range| !range.is_empty()), None],
        }
    }

    pub fn region_ranges(&self) -> impl Iterator<Item = Range<ByteOffset>> {
        let ranges = match (&self.region, &self.marked) {
            (Some(region), Some(marked)) => [
                Some(region.start..region.end.min(marked.start)),
                Some(region.start.max(marked.end)..region.end),
            ],
            (region, None) => [region.clone(), None],
            (None, _) => [None, None],
        };
        ranges
            .into_iter()
            .flatten()
            .filter(|range| !range.is_empty())
    }

    pub fn selected_range_utf16(&self) -> Range<OffsetUtf16> {
        range_to_utf16(&self.text, self.selected.clone()).expect("valid display selection")
    }

    pub fn marked_range_utf16(&self) -> Option<Range<OffsetUtf16>> {
        self.marked
            .clone()
            .and_then(|range| range_to_utf16(&self.text, range))
    }

    pub fn text_for_range(
        &self,
        range: Range<OffsetUtf16>,
    ) -> Option<(String, Range<OffsetUtf16>)> {
        let bytes = clip_utf16_range(&self.text, range)?;
        let adjusted = range_to_utf16(&self.text, bytes.clone())?;
        let text = self
            .text
            .chunks_in_range(bytes.start.0..bytes.end.0)
            .collect();
        Some((text, adjusted))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputError {
    InactiveSession,
    StaleSession,
    CompositionActive,
    Edit(EditError),
}

impl From<EditError> for InputError {
    fn from(error: EditError) -> Self {
        Self::Edit(error)
    }
}

/// One platform input mutation, carrying both the committed edit and the
/// projection delta that a wrap index needs for an incremental reflow.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputUpdate {
    pub edit: Option<AppliedEdit>,
    pub change: DisplayChange,
}

pub(crate) struct BatchInputUpdate {
    pub update: InputUpdate,
    pub ranges: Vec<(Option<usize>, Range<ByteOffset>)>,
}

struct Preedit {
    anchor: ByteOffset,
    display: Rope,
    marked: Range<ByteOffset>,
    selected: Range<ByteOffset>,
}

struct Replacement {
    before: DisplayId,
    committed_range: Range<ByteOffset>,
    display: Rope,
    replaced: Range<ByteOffset>,
    inserted: Range<ByteOffset>,
}

pub struct InputSession {
    token: SessionToken,
    base: SnapshotId,
    revision: u64,
    active: bool,
    preedit: Option<Preedit>,
}

impl InputSession {
    pub fn new(buffer: &Buffer) -> Self {
        Self {
            token: SessionToken::new(buffer.snapshot_id().buffer),
            base: buffer.snapshot_id(),
            revision: 0,
            active: true,
            preedit: None,
        }
    }

    pub(crate) fn synchronize_buffer(
        &mut self,
        buffer: &Buffer,
        old_len: usize,
    ) -> Option<DisplayChange> {
        if self.base == buffer.snapshot_id() {
            return None;
        }
        let before = DisplayId {
            committed: self.base,
            session: self.token,
            revision: self.revision,
        };
        let edit = buffer.last_edit().filter(|edit| edit.before == self.base);
        let active = self.active;
        *self = Self::new(buffer);
        self.active = active;
        Some(DisplayChange {
            before,
            after: self.display_id(buffer),
            old_range: edit
                .as_ref()
                .map_or(ByteOffset(0)..ByteOffset(old_len), |edit| {
                    edit.old_range.clone()
                }),
            new_range: edit.map_or(ByteOffset(0)..ByteOffset(buffer.text().len()), |edit| {
                edit.new_range
            }),
        })
    }

    pub fn token(&self) -> SessionToken {
        self.token
    }

    pub fn is_composing(&self) -> bool {
        self.preedit.is_some()
    }

    fn validate(&self, buffer: &Buffer, token: SessionToken) -> Result<(), InputError> {
        if !self.active {
            return Err(InputError::InactiveSession);
        }
        if token != self.token {
            return Err(InputError::StaleSession);
        }
        if buffer.snapshot_id().buffer != self.token.target {
            return Err(EditError::WrongTarget.into());
        }
        if self.base != buffer.snapshot_id() {
            return Err(EditError::StaleSnapshot.into());
        }
        Ok(())
    }

    fn invalidate_display(&mut self) {
        self.revision = self
            .revision
            .checked_add(1)
            .expect("display revision exhausted");
    }

    fn display_id(&self, buffer: &Buffer) -> DisplayId {
        DisplayId {
            committed: buffer.snapshot_id(),
            session: self.token,
            revision: self.revision,
        }
    }

    pub fn guard_command(&self, buffer: &Buffer, token: SessionToken) -> Result<(), InputError> {
        self.validate(buffer, token)?;
        if self.is_composing() {
            return Err(InputError::CompositionActive);
        }
        Ok(())
    }

    pub fn command_edit(
        &mut self,
        buffer: &mut Buffer,
        token: SessionToken,
        range: Range<ByteOffset>,
        text: &str,
    ) -> Result<InputUpdate, InputError> {
        self.command_edit_grouped(buffer, token, range, text, EditGroup::Isolated)
    }

    pub(crate) fn command_edit_grouped(
        &mut self,
        buffer: &mut Buffer,
        token: SessionToken,
        range: Range<ByteOffset>,
        text: &str,
        group: EditGroup,
    ) -> Result<InputUpdate, InputError> {
        self.guard_command(buffer, token)?;
        let before = self.display_id(buffer);
        let edit = buffer.edit_grouped(
            Edit {
                snapshot: buffer.snapshot_id(),
                range: range.clone(),
                text,
                source: EditSource::Command,
            },
            group,
        )?;
        self.base = buffer.snapshot_id();
        if edit.is_some() {
            self.invalidate_display();
        }
        Ok(InputUpdate {
            edit,
            change: DisplayChange {
                before,
                after: self.display_id(buffer),
                old_range: range.clone(),
                new_range: range.start..ByteOffset(range.start.0 + text.len()),
            },
        })
    }

    pub(crate) fn command_edit_batch_grouped(
        &mut self,
        buffer: &mut Buffer,
        token: SessionToken,
        edits: &[BatchEdit<'_>],
        group: EditGroup,
        primary_index: Option<usize>,
    ) -> Result<(InputUpdate, Vec<Range<ByteOffset>>), InputError> {
        self.guard_command(buffer, token)?;
        let before = self.display_id(buffer);
        let applied = buffer.edit_batch_grouped(
            buffer.snapshot_id(),
            edits,
            EditSource::Command,
            group,
            primary_index,
        )?;
        self.base = buffer.snapshot_id();
        if let Some(applied) = applied {
            self.invalidate_display();
            let old_range = applied.summary.old_range.clone();
            let new_range = applied.summary.new_range.clone();
            let new_ranges = applied.new_ranges;
            Ok((
                InputUpdate {
                    edit: Some(applied.summary),
                    change: DisplayChange {
                        before,
                        after: self.display_id(buffer),
                        old_range,
                        new_range,
                    },
                },
                new_ranges,
            ))
        } else {
            let point = buffer.point();
            let ranges = edits.iter().map(|edit| edit.range.clone()).collect();
            Ok((
                InputUpdate {
                    edit: None,
                    change: DisplayChange {
                        before,
                        after: self.display_id(buffer),
                        old_range: point..point,
                        new_range: point..point,
                    },
                },
                ranges,
            ))
        }
    }

    pub fn replay_history(
        &mut self,
        buffer: &mut Buffer,
        token: SessionToken,
        redo: bool,
    ) -> Result<Option<InputUpdate>, InputError> {
        self.guard_command(buffer, token)?;
        let before = self.display_id(buffer);
        let edit = if redo { buffer.redo()? } else { buffer.undo()? };
        let Some(edit) = edit else {
            return Ok(None);
        };
        self.base = buffer.snapshot_id();
        self.invalidate_display();
        Ok(Some(InputUpdate {
            change: DisplayChange {
                before,
                after: self.display_id(buffer),
                old_range: edit.old_range.clone(),
                new_range: edit.new_range.clone(),
            },
            edit: Some(edit),
        }))
    }

    fn guard_writable(&self, buffer: &Buffer, token: SessionToken) -> Result<(), InputError> {
        self.validate(buffer, token)?;
        buffer.check_editable()?;
        Ok(())
    }

    /// The committed projection, available even after `end` so the body still renders.
    pub fn committed_snapshot(&self, buffer: &Buffer) -> DisplaySnapshot {
        let point = buffer.point();
        DisplaySnapshot {
            id: self.display_id(buffer),
            text: buffer.text().clone(),
            selected: point..point,
            marked: None,
            region: buffer.region(),
        }
    }

    pub fn snapshot_for(
        &self,
        buffer: &Buffer,
        token: SessionToken,
    ) -> Result<DisplaySnapshot, InputError> {
        self.validate(buffer, token)?;
        let mut snapshot = self.committed_snapshot(buffer);
        if let Some(preedit) = &self.preedit {
            snapshot.text = preedit.display.clone();
            snapshot.selected = preedit.selected.clone();
            snapshot.marked = Some(preedit.marked.clone());
            snapshot.region = snapshot.region.map(|region| {
                let project = |offset: ByteOffset| {
                    ByteOffset(
                        offset.0
                            + if offset > preedit.anchor {
                                preedit.marked.end.0 - preedit.marked.start.0
                            } else {
                                0
                            },
                    )
                };
                project(region.start)..project(region.end)
            });
        }
        Ok(snapshot)
    }

    pub fn display_snapshot(&self, buffer: &Buffer) -> Result<DisplaySnapshot, InputError> {
        self.snapshot_for(buffer, self.token)
    }

    fn prepare_replacement(
        &self,
        buffer: &Buffer,
        range: Option<Range<OffsetUtf16>>,
        text: &str,
    ) -> Result<Replacement, InputError> {
        let snapshot = self.display_snapshot(buffer)?;
        let before = snapshot.id;
        let range = match range {
            Some(range) => {
                clip_utf16_range(&snapshot.text, range).ok_or(EditError::InvalidRange)?
            }
            None => snapshot.marked.unwrap_or(snapshot.selected),
        };
        let committed_range = match &self.preedit {
            Some(preedit) => {
                preedit.anchor.min(range.start)
                    ..ByteOffset(
                        preedit.anchor.0 + range.end.0.saturating_sub(preedit.marked.end.0),
                    )
            }
            None => range.clone(),
        };
        buffer.check_writable_range(committed_range.clone())?;
        let mut display = snapshot.text;
        display.replace(range.start.0..range.end.0, text);
        Ok(Replacement {
            before,
            committed_range,
            display,
            replaced: range.clone(),
            inserted: range.start..ByteOffset(range.start.0 + text.len()),
        })
    }

    pub fn replace_and_mark(
        &mut self,
        buffer: &mut Buffer,
        token: SessionToken,
        range: Option<Range<OffsetUtf16>>,
        text: &str,
        selected: Option<Range<OffsetUtf16>>,
    ) -> Result<InputUpdate, InputError> {
        self.guard_writable(buffer, token)?;
        let selected = match selected {
            Some(range) => {
                clip_utf16_range(&Rope::from(text), range).ok_or(EditError::InvalidRange)?
            }
            None => ByteOffset(text.len())..ByteOffset(text.len()),
        };
        let replacement = self.prepare_replacement(buffer, range, text)?;
        let start = replacement.inserted.start.0;
        let end =
            replacement.display.len() - (buffer.text().len() - replacement.committed_range.end.0);
        let retained: String = replacement
            .display
            .chunks_in_range(replacement.committed_range.start.0..start)
            .chain(
                replacement
                    .display
                    .chunks_in_range(replacement.inserted.end.0..end),
            )
            .collect();
        let edit = buffer.edit(Edit {
            snapshot: buffer.snapshot_id(),
            range: replacement.committed_range,
            text: &retained,
            source: EditSource::PlatformInput,
        })?;
        if edit.is_some() || text.is_empty() {
            buffer
                .set_point(replacement.inserted.start)
                .expect("valid preedit anchor");
        }
        self.base = buffer.snapshot_id();
        let inserted = replacement.inserted;
        if text.is_empty() {
            if self.preedit.take().is_some() {
                self.token = SessionToken::new(self.token.target);
            }
        } else {
            self.preedit = Some(Preedit {
                anchor: inserted.start,
                display: replacement.display,
                marked: inserted.clone(),
                selected: ByteOffset(start + selected.start.0)..ByteOffset(start + selected.end.0),
            });
        }
        self.invalidate_display();
        let change = DisplayChange {
            before: replacement.before,
            after: self.display_id(buffer),
            old_range: replacement.replaced,
            new_range: inserted,
        };
        Ok(InputUpdate { edit, change })
    }

    pub fn replace_text(
        &mut self,
        buffer: &mut Buffer,
        token: SessionToken,
        range: Option<Range<OffsetUtf16>>,
        text: &str,
    ) -> Result<InputUpdate, InputError> {
        self.guard_writable(buffer, token)?;
        let implicit_range = range.is_none();
        let replacement = self.prepare_replacement(buffer, range, text)?;
        let end =
            replacement.display.len() - (buffer.text().len() - replacement.committed_range.end.0);
        let inserted: String = replacement
            .display
            .chunks_in_range(replacement.committed_range.start.0..end)
            .collect();
        let group = if implicit_range
            && !self.is_composing()
            && replacement.committed_range.is_empty()
            && !text.contains(['\n', '\r'])
        {
            EditGroup::Insert
        } else {
            EditGroup::Isolated
        };
        let edit = buffer.edit_grouped(
            Edit {
                snapshot: buffer.snapshot_id(),
                range: replacement.committed_range,
                text: &inserted,
                source: EditSource::PlatformInput,
            },
            group,
        )?;
        buffer
            .set_point(replacement.inserted.end)
            .expect("valid projected caret");
        buffer.deactivate_mark();
        self.base = buffer.snapshot_id();
        if self.preedit.take().is_some() {
            self.token = SessionToken::new(self.token.target);
        }
        self.invalidate_display();
        let change = DisplayChange {
            before: replacement.before,
            after: DisplayId {
                committed: buffer.snapshot_id(),
                session: self.token,
                revision: self.revision,
            },
            old_range: replacement.replaced,
            new_range: replacement.inserted,
        };
        Ok(InputUpdate { edit, change })
    }

    pub(crate) fn replace_text_batch(
        &mut self,
        buffer: &mut Buffer,
        token: SessionToken,
        range: Option<Range<OffsetUtf16>>,
        text: &str,
        extra_offsets: &[ByteOffset],
    ) -> Result<BatchInputUpdate, InputError> {
        if range.is_some() {
            let update = self.replace_text(buffer, token, range, text)?;
            return Ok(BatchInputUpdate {
                update,
                ranges: Vec::new(),
            });
        }
        self.guard_writable(buffer, token)?;
        let replacement = self.prepare_replacement(buffer, None, text)?;
        let end =
            replacement.display.len() - (buffer.text().len() - replacement.committed_range.end.0);
        let inserted: String = replacement
            .display
            .chunks_in_range(replacement.committed_range.start.0..end)
            .collect();
        let group = if !self.is_composing()
            && replacement.committed_range.is_empty()
            && !text.contains(['\n', '\r'])
        {
            EditGroup::Insert
        } else {
            EditGroup::Isolated
        };

        let mut items: Vec<(Option<usize>, Range<ByteOffset>)> = Vec::new();
        let mut skipped_to_primary: Vec<usize> = Vec::new();
        let mut seen_offsets: Vec<ByteOffset> = Vec::new();
        items.push((None, replacement.committed_range.clone()));
        for (idx, &offset) in extra_offsets.iter().enumerate() {
            if offset >= replacement.committed_range.start
                && offset <= replacement.committed_range.end
            {
                skipped_to_primary.push(idx);
                continue;
            }
            if seen_offsets.contains(&offset) {
                continue;
            }
            seen_offsets.push(offset);
            items.push((Some(idx), offset..offset));
        }
        items.sort_by_key(|(_, r)| (r.start, r.end));
        let primary_pos = items
            .iter()
            .position(|(id, _)| id.is_none())
            .expect("primary edit present");
        let batch: Vec<BatchEdit<'_>> = items
            .iter()
            .map(|(_, r)| BatchEdit {
                range: r.clone(),
                text: inserted.as_str(),
            })
            .collect();

        let preedit_info = self
            .preedit
            .as_ref()
            .map(|p| (p.anchor, p.marked.end.0 - p.marked.start.0));
        let applied = buffer.edit_batch_grouped(
            buffer.snapshot_id(),
            &batch,
            EditSource::PlatformInput,
            group,
            Some(primary_pos),
        )?;
        let (edit, new_ranges) = match applied {
            Some(applied) => (Some(applied.summary), applied.new_ranges),
            None => (None, items.iter().map(|(_, r)| r.clone()).collect()),
        };
        let primary_new_range = new_ranges[primary_pos].clone();
        buffer
            .set_point(primary_new_range.end)
            .expect("valid projected caret");
        buffer.deactivate_mark();
        self.base = buffer.snapshot_id();
        if self.preedit.take().is_some() {
            self.token = SessionToken::new(self.token.target);
        }
        self.invalidate_display();

        let (old_range, new_range) = if let Some(ref summary) = edit {
            let old_start = summary.old_range.start.min(replacement.replaced.start);
            let old_end = match preedit_info {
                Some((anchor, marked_len)) => {
                    let projected_end = if summary.old_range.end >= anchor {
                        ByteOffset(summary.old_range.end.0 + marked_len)
                    } else {
                        summary.old_range.end
                    };
                    projected_end.max(replacement.replaced.end)
                }
                None => summary.old_range.end.max(replacement.replaced.end),
            };
            (old_start..old_end, summary.new_range.clone())
        } else {
            (replacement.replaced, replacement.inserted)
        };

        let change = DisplayChange {
            before: replacement.before,
            after: DisplayId {
                committed: buffer.snapshot_id(),
                session: self.token,
                revision: self.revision,
            },
            old_range,
            new_range,
        };
        let mut mapped: Vec<(Option<usize>, Range<ByteOffset>)> = items
            .into_iter()
            .map(|(id, _)| id)
            .zip(new_ranges)
            .collect();
        for idx in skipped_to_primary {
            mapped.push((Some(idx), primary_new_range.clone()));
        }
        Ok(BatchInputUpdate {
            update: InputUpdate { edit, change },
            ranges: mapped,
        })
    }

    pub(crate) fn commit_preedit_batch(
        &mut self,
        buffer: &mut Buffer,
        extra_offsets: &[ByteOffset],
    ) -> Result<Option<BatchInputUpdate>, InputError> {
        self.validate(buffer, self.token)?;
        let Some(preedit) = self.preedit.as_ref() else {
            return Ok(None);
        };
        self.guard_writable(buffer, self.token)?;
        let before = self.display_id(buffer);
        let text: String = preedit
            .display
            .chunks_in_range(preedit.marked.start.0..preedit.marked.end.0)
            .collect();
        let anchor = preedit.anchor;
        let marked_len = preedit.marked.end.0 - preedit.marked.start.0;
        let mut items: Vec<(Option<usize>, Range<ByteOffset>)> = Vec::new();
        let mut skipped_to_primary: Vec<usize> = Vec::new();
        let mut seen_offsets: Vec<ByteOffset> = Vec::new();
        items.push((None, anchor..anchor));
        for (idx, &offset) in extra_offsets.iter().enumerate() {
            if offset == anchor {
                skipped_to_primary.push(idx);
                continue;
            }
            if seen_offsets.contains(&offset) {
                continue;
            }
            seen_offsets.push(offset);
            items.push((Some(idx), offset..offset));
        }
        items.sort_by_key(|(_, r)| r.start);
        let primary_pos = items
            .iter()
            .position(|(id, _)| id.is_none())
            .expect("primary edit present");
        let batch: Vec<BatchEdit<'_>> = items
            .iter()
            .map(|(_, r)| BatchEdit {
                range: r.clone(),
                text: text.as_str(),
            })
            .collect();
        let applied = buffer.edit_batch_grouped(
            buffer.snapshot_id(),
            &batch,
            EditSource::PlatformInput,
            EditGroup::Isolated,
            Some(primary_pos),
        )?;
        let preedit = self.preedit.take().expect("validated preedit");
        let (edit, new_ranges) = match applied {
            Some(applied) => (Some(applied.summary), applied.new_ranges),
            None => (None, items.iter().map(|(_, r)| r.clone()).collect()),
        };
        let primary_new_range = new_ranges[primary_pos].clone();
        buffer
            .set_point(primary_new_range.end)
            .expect("valid committed caret");
        buffer.deactivate_mark();
        self.base = buffer.snapshot_id();
        self.token = SessionToken::new(self.token.target);
        self.invalidate_display();
        let (old_range, new_range) = if let Some(ref summary) = edit {
            let old_start = summary.old_range.start.min(preedit.marked.start);
            let old_end = if summary.old_range.end >= anchor {
                ByteOffset(summary.old_range.end.0 + marked_len)
            } else {
                summary.old_range.end
            }
            .max(preedit.marked.end);
            (old_start..old_end, summary.new_range.clone())
        } else {
            (anchor..anchor, anchor..anchor)
        };
        let mut mapped: Vec<(Option<usize>, Range<ByteOffset>)> = items
            .into_iter()
            .map(|(id, _)| id)
            .zip(new_ranges)
            .collect();
        for idx in skipped_to_primary {
            mapped.push((Some(idx), primary_new_range.clone()));
        }
        Ok(Some(BatchInputUpdate {
            update: InputUpdate {
                edit,
                change: DisplayChange {
                    before,
                    after: DisplayId {
                        committed: buffer.snapshot_id(),
                        session: self.token,
                        revision: self.revision,
                    },
                    old_range,
                    new_range,
                },
            },
            ranges: mapped,
        }))
    }

    /// Finish composition without a platform replacement: commit the visible projection once.
    pub fn commit_preedit(
        &mut self,
        buffer: &mut Buffer,
    ) -> Result<Option<InputUpdate>, InputError> {
        self.validate(buffer, self.token)?;
        let Some(preedit) = self.preedit.as_ref() else {
            return Ok(None);
        };
        self.guard_writable(buffer, self.token)?;
        let before = self.display_id(buffer);
        let text: String = preedit
            .display
            .chunks_in_range(preedit.marked.start.0..preedit.marked.end.0)
            .collect();
        let edit = buffer.edit(Edit {
            snapshot: buffer.snapshot_id(),
            range: preedit.anchor..preedit.anchor,
            text: &text,
            source: EditSource::PlatformInput,
        })?;
        let preedit = self.preedit.take().expect("validated preedit");
        buffer
            .set_point(preedit.selected.end)
            .expect("valid committed caret");
        buffer.deactivate_mark();
        self.base = buffer.snapshot_id();
        self.token = SessionToken::new(self.token.target);
        self.invalidate_display();
        let start = preedit.anchor;
        Ok(Some(InputUpdate {
            edit,
            change: DisplayChange {
                before,
                after: DisplayId {
                    committed: buffer.snapshot_id(),
                    session: self.token,
                    revision: self.revision,
                },
                old_range: start..start,
                new_range: start..start,
            },
        }))
    }

    /// Discard only marked text; already applied replacements remain committed.
    pub fn cancel(
        &mut self,
        buffer: &Buffer,
        token: SessionToken,
    ) -> Result<(SessionToken, DisplayChange), InputError> {
        self.validate(buffer, token)?;
        let before = self.display_id(buffer);
        let (removed, restored) = self.preedit.take().map_or_else(
            || {
                let point = buffer.point();
                (point..point, point..point)
            },
            |preedit| (preedit.marked, preedit.anchor..preedit.anchor),
        );
        self.token = SessionToken::new(self.token.target);
        self.invalidate_display();
        Ok((
            self.token,
            DisplayChange {
                before,
                after: DisplayId {
                    committed: buffer.snapshot_id(),
                    session: self.token,
                    revision: self.revision,
                },
                new_range: restored,
                old_range: removed,
            },
        ))
    }

    /// End the target's input lifetime without committing its remaining marked text.
    pub fn end(&mut self) {
        self.preedit = None;
        self.active = false;
        self.invalidate_display();
    }
}
