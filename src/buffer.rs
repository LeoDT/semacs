//! Authoritative committed text. Temporary platform composition lives in input.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::files::{DecodedFile, FileFormat};
use crate::text::{ByteOffset, Rope};

mod history;
pub(crate) use history::EditGroup;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BufferId(u64);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct TextVersion(u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SnapshotId {
    pub buffer: BufferId,
    pub version: TextVersion,
}

#[derive(Clone)]
pub struct BufferSnapshot {
    id: SnapshotId,
    text: Rope,
}

impl BufferSnapshot {
    pub fn id(&self) -> SnapshotId {
        self.id
    }

    pub fn text(&self) -> &Rope {
        &self.text
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditSource {
    Command,
    PlatformInput,
    Undo,
    Redo,
}

pub struct Edit<'a> {
    pub snapshot: SnapshotId,
    pub range: Range<ByteOffset>,
    pub text: &'a str,
    pub source: EditSource,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppliedEdit {
    pub before: SnapshotId,
    pub after: SnapshotId,
    pub old_range: Range<ByteOffset>,
    pub new_range: Range<ByteOffset>,
    pub source: EditSource,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditError {
    WrongTarget,
    StaleSnapshot,
    InvalidRange,
    ReadOnly,
    TextReadOnly,
}

pub struct Buffer {
    snapshot: BufferSnapshot,
    point: ByteOffset,
    mark: Option<ByteOffset>,
    mark_active: bool,
    read_only: bool,
    read_only_prefix: ByteOffset,
    path: Option<PathBuf>,
    format: FileFormat,
    modified: bool,
    history: history::History,
}

impl Buffer {
    pub fn scratch() -> Self {
        Self::new(
            None,
            DecodedFile {
                text: String::new(),
                format: FileFormat::default(),
            },
        )
    }

    /// Visiting a path that does not exist starts an empty, unmodified buffer.
    pub fn new_file(path: PathBuf) -> Self {
        Self::new(
            Some(path),
            DecodedFile {
                text: String::new(),
                format: FileFormat::default(),
            },
        )
    }

    pub fn new(path: Option<PathBuf>, file: DecodedFile) -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        let id = NEXT_ID
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .expect("buffer identity exhausted");
        Self {
            snapshot: BufferSnapshot {
                id: SnapshotId {
                    buffer: BufferId(id),
                    version: TextVersion::default(),
                },
                text: Rope::from(file.text.as_str()),
            },
            point: ByteOffset(0),
            mark: None,
            mark_active: false,
            read_only: false,
            read_only_prefix: ByteOffset(0),
            path,
            format: file.format,
            modified: false,
            history: history::History::default(),
        }
    }

    pub fn minibuffer(prompt: &str, initial: &str) -> Self {
        let mut buffer = Self::new(
            None,
            DecodedFile {
                text: format!("{prompt}{initial}"),
                format: FileFormat::default(),
            },
        )
        .with_read_only_prefix(ByteOffset(prompt.len()))
        .expect("valid prompt boundary");
        buffer.point = ByteOffset(buffer.text().len());
        buffer.modified = true;
        buffer
    }

    pub fn with_read_only_prefix(mut self, end: ByteOffset) -> Result<Self, EditError> {
        self.validate_range(&(ByteOffset(0)..end))?;
        self.read_only_prefix = end;
        self.break_history_group();
        Ok(self)
    }

    pub fn read_only_prefix(&self) -> ByteOffset {
        self.read_only_prefix
    }

    pub fn check_writable_range(&self, range: Range<ByteOffset>) -> Result<(), EditError> {
        self.validate_range(&range)?;
        if self.read_only {
            return Err(EditError::ReadOnly);
        }
        if range.start < self.read_only_prefix {
            return Err(EditError::TextReadOnly);
        }
        Ok(())
    }

    pub fn snapshot(&self) -> BufferSnapshot {
        self.snapshot.clone()
    }

    pub fn snapshot_id(&self) -> SnapshotId {
        self.snapshot.id
    }

    pub fn text(&self) -> &Rope {
        &self.snapshot.text
    }

    pub fn point(&self) -> ByteOffset {
        self.point
    }

    pub fn set_point(&mut self, point: ByteOffset) -> Result<(), EditError> {
        if !self.text().is_char_boundary(point.0) {
            return Err(EditError::InvalidRange);
        }
        self.point = point;
        Ok(())
    }

    pub fn mark(&self) -> Option<ByteOffset> {
        self.mark
    }

    pub fn mark_active(&self) -> bool {
        self.mark_active
    }

    pub fn region(&self) -> Option<Range<ByteOffset>> {
        let mark = self
            .mark
            .filter(|mark| self.mark_active && *mark != self.point)?;
        Some(self.point.min(mark)..self.point.max(mark))
    }

    pub fn set_mark(&mut self, point: ByteOffset) -> Result<(), EditError> {
        if !self.text().is_char_boundary(point.0) {
            return Err(EditError::InvalidRange);
        }
        self.mark = Some(point);
        self.mark_active = true;
        Ok(())
    }

    pub fn deactivate_mark(&mut self) {
        self.mark_active = false;
    }

    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    pub fn set_read_only(&mut self, read_only: bool) {
        self.read_only = read_only;
    }

    pub fn line_column(&self) -> (u32, usize) {
        let point = self.text().offset_to_point(self.point.0);
        let start = self
            .text()
            .point_to_offset(crate::text::Point::new(point.row, 0));
        let summary = self
            .text()
            .cursor(start)
            .summary::<crate::text::rope::TextSummary>(self.point.0);
        (point.row + 1, summary.chars)
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn display_name(&self) -> String {
        self.path()
            .and_then(Path::file_name)
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "*scratch*".to_owned())
    }

    pub fn format(&self) -> FileFormat {
        self.format
    }

    pub fn is_modified(&self) -> bool {
        self.modified
    }

    /// Every nonempty replacement is a committed edit, even with identical text.
    pub fn edit(&mut self, edit: Edit<'_>) -> Result<Option<AppliedEdit>, EditError> {
        self.edit_grouped(edit, EditGroup::Isolated)
    }

    fn validate_snapshot(&self, snapshot: SnapshotId) -> Result<(), EditError> {
        if snapshot.buffer != self.snapshot.id.buffer {
            return Err(EditError::WrongTarget);
        }
        if snapshot != self.snapshot.id {
            return Err(EditError::StaleSnapshot);
        }
        Ok(())
    }

    fn validate_range(&self, range: &Range<ByteOffset>) -> Result<(), EditError> {
        if range.start > range.end
            || !self.text().is_char_boundary(range.start.0)
            || !self.text().is_char_boundary(range.end.0)
        {
            return Err(EditError::InvalidRange);
        }
        Ok(())
    }

    fn validate_edit(&self, edit: &Edit<'_>) -> Result<(), EditError> {
        self.validate_snapshot(edit.snapshot)?;
        self.validate_range(&edit.range)?;
        if !(edit.range.is_empty() && edit.text.is_empty()) {
            self.check_writable_range(edit.range.clone())?;
        }
        Ok(())
    }

    fn apply_edit(&mut self, edit: Edit<'_>) -> Result<Option<AppliedEdit>, EditError> {
        self.validate_edit(&edit)?;
        if edit.range.is_empty() && edit.text.is_empty() {
            return Ok(None);
        }
        let before = self.snapshot.id;
        let version = TextVersion(
            before
                .version
                .0
                .checked_add(1)
                .expect("text version exhausted"),
        );
        self.snapshot
            .text
            .replace(edit.range.start.0..edit.range.end.0, edit.text);
        self.snapshot.id.version = version;
        self.point = ByteOffset(edit.range.start.0 + edit.text.len());
        if let Some(mark) = &mut self.mark {
            if *mark > edit.range.end {
                mark.0 = mark.0 - (edit.range.end.0 - edit.range.start.0) + edit.text.len();
            } else if *mark > edit.range.start {
                *mark = edit.range.start;
            }
        }
        self.deactivate_mark();
        self.modified = true;

        Ok(Some(AppliedEdit {
            before,
            after: self.snapshot.id,
            new_range: edit.range.start..self.point,
            old_range: edit.range,
            source: edit.source,
        }))
    }
}
