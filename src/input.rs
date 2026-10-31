//! Target-bound platform input, with a disposable projection for preedit text.

use std::ops::Range;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::buffer::{AppliedEdit, Buffer, BufferId, Edit, EditError, EditSource, SnapshotId};
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

    /// The committed projection, available even after `end` so the body still renders.
    pub fn committed_snapshot(&self, buffer: &Buffer) -> DisplaySnapshot {
        let point = buffer.point();
        DisplaySnapshot {
            id: self.display_id(buffer),
            text: buffer.text().clone(),
            selected: point..point,
            marked: None,
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
        self.validate(buffer, token)?;
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
        self.validate(buffer, token)?;
        let replacement = self.prepare_replacement(buffer, range, text)?;
        let end =
            replacement.display.len() - (buffer.text().len() - replacement.committed_range.end.0);
        let inserted: String = replacement
            .display
            .chunks_in_range(replacement.committed_range.start.0..end)
            .collect();
        let edit = buffer.edit(Edit {
            snapshot: buffer.snapshot_id(),
            range: replacement.committed_range,
            text: &inserted,
            source: EditSource::PlatformInput,
        })?;
        buffer
            .set_point(replacement.inserted.end)
            .expect("valid projected caret");
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

    /// Finish composition without a platform replacement: commit the visible projection once.
    pub fn commit_preedit(
        &mut self,
        buffer: &mut Buffer,
    ) -> Result<Option<InputUpdate>, InputError> {
        self.validate(buffer, self.token)?;
        let Some(preedit) = self.preedit.take() else {
            return Ok(None);
        };
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
        buffer
            .set_point(preedit.selected.end)
            .expect("valid committed caret");
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
