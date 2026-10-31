//! Authoritative committed text. Temporary platform composition lives in input.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::files::{DecodedFile, FileFormat};
use crate::text::{ByteOffset, Rope};

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
}

pub struct Buffer {
    snapshot: BufferSnapshot,
    point: ByteOffset,
    path: Option<PathBuf>,
    format: FileFormat,
    modified: bool,
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
            path,
            format: file.format,
            modified: false,
        }
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
    /// R1 has no history or save operation; D16 save-point semantics land in M1.
    pub fn edit(&mut self, edit: Edit<'_>) -> Result<Option<AppliedEdit>, EditError> {
        if edit.snapshot.buffer != self.snapshot.id.buffer {
            return Err(EditError::WrongTarget);
        }
        if edit.snapshot != self.snapshot.id {
            return Err(EditError::StaleSnapshot);
        }
        if edit.range.start > edit.range.end
            || !self.text().is_char_boundary(edit.range.start.0)
            || !self.text().is_char_boundary(edit.range.end.0)
        {
            return Err(EditError::InvalidRange);
        }
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
