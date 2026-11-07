//! Shared committed documents and view-local points. Preedit lives in input.

use std::cell::RefCell;
use std::ops::Range;
use std::rc::{Rc, Weak};

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::files::{DecodedFile, FileFormat, FileIdentity, FileStamp};
use crate::text::{ByteOffset, Rope};

#[cfg(test)]
mod annotation_tests;
mod annotations;
pub(crate) use annotations::Annotations;
pub use annotations::{LineBackground, StyledRange};
mod diff;
mod generated;
mod history;
mod process;
mod selection;
mod status;
pub(crate) use history::EditGroup;
use history::HistoryState;
pub(crate) use selection::RegionId;
use selection::{TrackedMark, TrackedPoint};

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

/// Editable documents track saves through undo states; generated buffers,
/// which have no undo history, track the committed text version instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SavePoint {
    buffer: BufferId,
    state: SaveBoundary,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SaveBoundary {
    History(HistoryState),
    Generated(TextVersion),
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
    EditingInhibited(&'static str),
}

struct Document {
    snapshot: BufferSnapshot,
    revision: u64,
    last_edit: Option<AppliedEdit>,
    points: Vec<Weak<TrackedPoint>>,
    mark: TrackedMark,
    read_only: bool,
    edit_inhibition: Option<&'static str>,
    message_log: bool,
    mode: crate::mode::ModeId,
    read_only_prefix: ByteOffset,
    path: Option<PathBuf>,
    identity: Option<FileIdentity>,
    base_name: String,
    display_name: String,
    disk_stamp: Option<FileStamp>,
    format: FileFormat,
    modified: bool,
    history: history::History,
    syntax: crate::syntax::Syntax,
    annotations: Option<std::sync::Arc<Annotations>>,
    sections: Option<std::sync::Arc<crate::section::Sections>>,
    named_styles: std::sync::Arc<[(Range<ByteOffset>, crate::theme::StyleRole)]>,
    diff: Option<diff::DiffSession>,
    status: Option<status::StatusSession>,
    process: Option<process::ProcessSession>,
}

impl Document {
    fn touch(&mut self) {
        self.revision = self
            .revision
            .checked_add(1)
            .expect("document revision exhausted");
    }
}

pub struct Buffer {
    document: Rc<RefCell<Document>>,
    point: Rc<TrackedPoint>,
    view_id: u64,
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
        Self::from_text(path, Rope::from(file.text.as_str()), file.format)
    }

    pub(crate) fn from_text(path: Option<PathBuf>, text: Rope, format: FileFormat) -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        let id = NEXT_ID
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .expect("buffer identity exhausted");
        let view_id = next_view_id();
        let base_name = path
            .as_deref()
            .and_then(Path::file_name)
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "*scratch*".to_owned());
        let point = Rc::new(TrackedPoint::new(0));
        let points = vec![Rc::downgrade(&point)];
        let snapshot = BufferSnapshot {
            id: SnapshotId {
                buffer: BufferId(id),
                version: TextVersion::default(),
            },
            text,
        };
        let syntax = crate::syntax::Syntax::new(snapshot.clone(), path.as_deref());
        let document = Document {
            snapshot,
            syntax,
            annotations: None,
            sections: None,
            named_styles: std::sync::Arc::from([]),
            diff: None,
            status: None,
            process: None,
            revision: 0,
            last_edit: None,
            points,
            mark: TrackedMark::default(),
            read_only: false,
            edit_inhibition: None,
            message_log: false,
            mode: crate::mode::ModeId::default(),
            read_only_prefix: ByteOffset(0),
            path,
            identity: None,
            base_name: base_name.clone(),
            display_name: base_name,
            disk_stamp: None,
            format,
            modified: false,
            history: history::History::default(),
        };
        Self {
            document: Rc::new(RefCell::new(document)),
            point,
            view_id,
        }
    }

    pub fn new_view(&self) -> Buffer {
        let point = Rc::new(TrackedPoint::new(self.point().0));
        let mut doc = self.document.borrow_mut();
        doc.points.retain(|point| point.strong_count() > 0);
        doc.points.push(Rc::downgrade(&point));
        Buffer {
            document: self.document.clone(),
            point,
            view_id: next_view_id(),
        }
    }

    pub fn visiting(path: PathBuf, file: DecodedFile, stamp: Option<FileStamp>) -> Self {
        let mut buffer = Self::new(Some(path), file);
        buffer.set_disk_stamp(stamp);
        buffer
    }

    pub fn with_disk_stamp(mut self, stamp: Option<FileStamp>) -> Self {
        self.set_disk_stamp(stamp);
        self
    }

    pub fn named(name: &str) -> Self {
        let buffer = Self::new(
            None,
            DecodedFile {
                text: String::new(),
                format: FileFormat::default(),
            },
        );
        {
            let mut doc = buffer.document.borrow_mut();
            doc.base_name = name.to_owned();
            doc.display_name = name.to_owned();
        }
        buffer
    }

    pub(crate) fn messages() -> Self {
        let buffer = Self::named("*Messages*");
        {
            let mut doc = buffer.document.borrow_mut();
            doc.message_log = true;
            doc.read_only = true;
        }
        buffer
    }

    pub(crate) fn repository(&self) -> Option<crate::git::Repository> {
        self.process_repository()
            .or_else(|| self.status_repository())
            .or_else(|| self.comparison().map(|comparison| comparison.repository))
    }

    pub fn sections(&self) -> Option<std::sync::Arc<crate::section::Sections>> {
        self.document.borrow().sections.clone()
    }

    pub(crate) fn named_styles(
        &self,
    ) -> std::sync::Arc<[(Range<ByteOffset>, crate::theme::StyleRole)]> {
        self.document.borrow().named_styles.clone()
    }

    pub fn mode(&self) -> crate::mode::ModeId {
        self.document.borrow().mode
    }

    pub(crate) fn set_mode(&mut self, mode: crate::mode::ModeId) {
        let mut doc = self.document.borrow_mut();
        doc.mode = mode;
        doc.touch();
    }

    pub fn is_message_log(&self) -> bool {
        self.document.borrow().message_log
    }

    pub(crate) fn detach_message_log(&mut self) {
        let mut doc = self.document.borrow_mut();
        doc.message_log = false;
        doc.touch();
    }

    pub(crate) fn update_message_log(&mut self, range: Range<ByteOffset>, text: &str) {
        assert!(self.is_message_log());
        self.validate_range(&range)
            .expect("valid message log range");
        let active = self.mark_active();
        self.apply_validated_edit(Edit {
            snapshot: self.snapshot_id(),
            range,
            text,
            source: EditSource::Command,
        });
        let mut doc = self.document.borrow_mut();
        doc.modified = false;
        doc.history = history::History::default();
        let mark = doc.mark.position();
        doc.mark.set(mark, active);
    }

    pub fn minibuffer(prompt: &str, initial: &str) -> Self {
        let buffer = Self::new(
            None,
            DecodedFile {
                text: format!("{prompt}{initial}"),
                format: FileFormat::default(),
            },
        )
        .with_read_only_prefix(ByteOffset(prompt.len()))
        .expect("valid prompt boundary");
        buffer.point.set(buffer.text().len());
        buffer.document.borrow_mut().modified = true;
        buffer
    }

    pub fn with_read_only_prefix(mut self, end: ByteOffset) -> Result<Self, EditError> {
        self.validate_range(&(ByteOffset(0)..end))?;
        self.document.borrow_mut().read_only_prefix = end;
        self.break_history_group();
        Ok(self)
    }
    pub fn read_only_prefix(&self) -> ByteOffset {
        self.document.borrow().read_only_prefix
    }
    pub fn check_writable_range(&self, range: Range<ByteOffset>) -> Result<(), EditError> {
        self.validate_range(&range)?;
        self.check_editable()?;
        let doc = self.document.borrow();
        if range.start < doc.read_only_prefix {
            return Err(EditError::TextReadOnly);
        }
        Ok(())
    }
    pub(crate) fn prepare_syntax(&self, observer: gpui_kit::AnyWeakEntity, cx: &mut gpui_kit::App) {
        self.document.borrow().syntax.prepare(observer, cx);
    }

    pub(crate) fn syntax_highlights(&self) -> Option<std::sync::Arc<crate::syntax::Highlights>> {
        self.document.borrow().syntax.highlights()
    }

    pub(crate) fn syntax_display(&self) -> Option<crate::syntax::DisplayHighlightSnapshot> {
        self.document.borrow().syntax.display_highlights()
    }

    pub(crate) fn syntax_ready(&self) -> bool {
        self.document.borrow().syntax.ready()
    }

    pub(crate) fn revision(&self) -> u64 {
        self.document.borrow().revision
    }

    pub(crate) fn last_edit(&self) -> Option<AppliedEdit> {
        self.document.borrow().last_edit.clone()
    }

    pub fn snapshot(&self) -> BufferSnapshot {
        self.document.borrow().snapshot.clone()
    }
    pub fn snapshot_id(&self) -> SnapshotId {
        self.document.borrow().snapshot.id
    }
    pub(crate) fn region_id(&self) -> RegionId {
        let doc = self.document.borrow();
        RegionId {
            snapshot: doc.snapshot.id,
            view: self.view_id,
            point_revision: self.point.revision(),
            mark_revision: doc.mark.revision(),
        }
    }
    pub fn text(&self) -> Rope {
        self.document.borrow().snapshot.text.clone()
    }
    pub fn point(&self) -> ByteOffset {
        ByteOffset(self.point.get())
    }
    pub fn set_point(&mut self, point: ByteOffset) -> Result<(), EditError> {
        if !self.text().is_char_boundary(point.0) {
            return Err(EditError::InvalidRange);
        }
        let point = self.sections().map_or(point, |sections| {
            sections.motion_point(point, point >= self.point())
        });
        self.point.set(point.0);
        Ok(())
    }
    pub fn mark(&self) -> Option<ByteOffset> {
        self.document.borrow().mark.position()
    }
    pub fn mark_active(&self) -> bool {
        self.document.borrow().mark.is_active()
    }
    pub fn region(&self) -> Option<Range<ByteOffset>> {
        let mark = self
            .mark()
            .filter(|mark| self.mark_active() && *mark != self.point())?;
        Some(self.point().min(mark)..self.point().max(mark))
    }
    pub fn set_mark(&mut self, point: ByteOffset) -> Result<(), EditError> {
        if !self.text().is_char_boundary(point.0) {
            return Err(EditError::InvalidRange);
        }
        let mut doc = self.document.borrow_mut();
        doc.mark.set(Some(point), true);
        doc.touch();
        Ok(())
    }
    pub fn deactivate_mark(&mut self) {
        let mut doc = self.document.borrow_mut();
        if doc.mark.is_active() {
            doc.mark.deactivate();
            doc.touch();
        }
    }
    pub fn edit_inhibition(&self) -> Option<&'static str> {
        self.document.borrow().edit_inhibition
    }
    pub(crate) fn set_edit_inhibition(&mut self, reason: Option<&'static str>) {
        let mut doc = self.document.borrow_mut();
        doc.edit_inhibition = reason;
        doc.touch();
    }
    pub(crate) fn check_editable(&self) -> Result<(), EditError> {
        if let Some(reason) = self.edit_inhibition() {
            return Err(EditError::EditingInhibited(reason));
        }
        if self.is_read_only() {
            return Err(EditError::ReadOnly);
        }
        Ok(())
    }
    pub fn is_read_only(&self) -> bool {
        self.document.borrow().read_only
    }
    pub fn set_read_only(&mut self, value: bool) {
        let mut doc = self.document.borrow_mut();
        doc.read_only = value;
        doc.touch();
    }
    pub fn line_column(&self) -> (u32, usize) {
        let text = self.text();
        let point = text.offset_to_point(self.point().0);
        let start = text.point_to_offset(crate::text::Point::new(point.row, 0));
        let summary = text
            .cursor(start)
            .summary::<crate::text::rope::TextSummary>(self.point().0);
        (point.row + 1, summary.chars)
    }
    pub fn path(&self) -> Option<PathBuf> {
        self.document.borrow().path.clone()
    }
    pub fn display_name(&self) -> String {
        self.document.borrow().display_name.clone()
    }
    pub fn base_name(&self) -> String {
        self.document.borrow().base_name.clone()
    }
    pub(crate) fn set_display_name(&mut self, name: String) {
        let mut doc = self.document.borrow_mut();
        doc.display_name = name;
        doc.touch();
    }
    pub fn disk_stamp(&self) -> Option<FileStamp> {
        self.document.borrow().disk_stamp
    }
    pub fn identity(&self) -> Option<FileIdentity> {
        self.document.borrow().identity
    }
    pub(crate) fn set_disk_stamp(&mut self, stamp: Option<FileStamp>) {
        let mut d = self.document.borrow_mut();
        d.identity = stamp.and_then(FileStamp::identity);
        d.disk_stamp = stamp;
        d.touch();
    }
    pub(crate) fn set_path(&mut self, path: PathBuf) {
        let mut d = self.document.borrow_mut();
        d.base_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "*scratch*".to_owned());
        d.display_name = d.base_name.clone();
        d.syntax.set_path(&path);
        d.path = Some(path);
        d.touch();
    }
    #[cfg(unix)]
    pub(crate) fn reload_visited(&mut self, loaded: crate::files::LoadedFile) {
        let points: Vec<_> = self
            .document
            .borrow()
            .points
            .iter()
            .filter_map(Weak::upgrade)
            .map(|point| {
                let offset = loaded
                    .file
                    .text
                    .floor_char_boundary(point.get().min(loaded.file.text.len()));
                (point, offset)
            })
            .collect();
        self.apply_validated_edit(Edit {
            snapshot: self.snapshot_id(),
            range: ByteOffset(0)..ByteOffset(self.text().len()),
            text: &loaded.file.text,
            source: EditSource::Command,
        });
        self.set_disk_stamp(loaded.stamp);
        let mut doc = self.document.borrow_mut();
        doc.format = loaded.file.format;
        doc.modified = false;
        doc.history = Default::default();
        doc.mark.set(None, false);
        for (point, offset) in points {
            point.set(offset);
        }
    }

    pub fn format(&self) -> FileFormat {
        self.document.borrow().format
    }
    pub fn is_modified(&self) -> bool {
        self.document.borrow().modified
    }
    pub fn edit(&mut self, edit: Edit<'_>) -> Result<Option<AppliedEdit>, EditError> {
        self.edit_grouped(edit, EditGroup::Isolated)
    }

    fn validate_snapshot(&self, snapshot: SnapshotId) -> Result<(), EditError> {
        let current = self.snapshot_id();
        if snapshot.buffer != current.buffer {
            return Err(EditError::WrongTarget);
        }
        if snapshot != current {
            return Err(EditError::StaleSnapshot);
        }
        Ok(())
    }
    fn validate_range(&self, range: &Range<ByteOffset>) -> Result<(), EditError> {
        let text = self.text();
        if range.start > range.end
            || !text.is_char_boundary(range.start.0)
            || !text.is_char_boundary(range.end.0)
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
        Ok(Some(self.apply_validated_edit(edit)))
    }

    fn apply_validated_edit(&mut self, edit: Edit<'_>) -> AppliedEdit {
        let mut doc = self.document.borrow_mut();
        let before = doc.snapshot.id;
        let old_end = edit.range.end.0;
        let start = edit.range.start.0;
        let removed = old_end - start;
        let inserted = edit.text.len();
        let version = TextVersion(
            before
                .version
                .0
                .checked_add(1)
                .expect("text version exhausted"),
        );
        doc.snapshot.text.replace(start..old_end, edit.text);
        doc.snapshot.id.version = version;
        doc.annotations = None;
        doc.syntax
            .edited(doc.snapshot.clone(), start..old_end, start + inserted);
        doc.points.retain(|weak| {
            if let Some(point) = weak.upgrade() {
                if !Rc::ptr_eq(&point, &self.point) {
                    let p = point.get();
                    let adjusted = if p <= start {
                        p
                    } else if p <= old_end {
                        start
                    } else {
                        p - removed + inserted
                    };
                    point.set(adjusted);
                }
                true
            } else {
                false
            }
        });
        self.point.set(start + inserted);
        let mark = doc.mark.position().map(|mut mark| {
            if mark.0 > old_end {
                mark.0 = mark.0 - removed + inserted;
            } else if mark.0 > start {
                mark = ByteOffset(start);
            }
            mark
        });
        doc.mark.set(mark, false);
        doc.modified = true;
        let after = doc.snapshot.id;
        let applied = AppliedEdit {
            before,
            after,
            new_range: ByteOffset(start)..ByteOffset(start + inserted),
            old_range: edit.range,
            source: edit.source,
        };
        doc.last_edit = Some(applied.clone());
        doc.touch();
        applied
    }
}

fn next_view_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
        .expect("view identity exhausted")
}

#[cfg(test)]
mod shared_tests {
    use super::*;

    fn replace(buffer: &mut Buffer, range: Range<usize>, value: &str) {
        buffer
            .edit(Edit {
                snapshot: buffer.snapshot_id(),
                range: ByteOffset(range.start)..ByteOffset(range.end),
                text: value,
                source: EditSource::Command,
            })
            .unwrap();
    }

    #[test]
    fn views_share_text_and_adjust_independent_points() {
        let mut first = Buffer::new(
            None,
            DecodedFile {
                text: "abcd".into(),
                format: FileFormat::default(),
            },
        );
        first.set_point(ByteOffset(2)).unwrap();
        let mut second = first.new_view();
        second.set_point(ByteOffset(3)).unwrap();
        replace(&mut first, 2..2, "XY");
        assert_eq!(first.text().to_string(), "abXYcd");
        assert_eq!(first.point(), ByteOffset(4));
        assert_eq!(second.point(), ByteOffset(5));
        let third = first.new_view();
        assert_eq!(third.point(), ByteOffset(4));
    }

    #[test]
    fn deletion_clamps_other_points_and_view_lifetime_is_independent() {
        let mut first = Buffer::new(
            None,
            DecodedFile {
                text: "abcdef".into(),
                format: FileFormat::default(),
            },
        );
        let mut second = first.new_view();
        second.set_point(ByteOffset(4)).unwrap();
        replace(&mut first, 2..5, "");
        assert_eq!(second.point(), ByteOffset(2));
        drop(second);
        replace(&mut first, 0..1, "z");
        assert_eq!(first.text().to_string(), "zbf");
    }

    #[test]
    fn undo_and_redo_restore_only_the_calling_views_point() {
        let mut first = Buffer::new(
            None,
            DecodedFile {
                text: "abc".into(),
                format: FileFormat::default(),
            },
        );
        let mut second = first.new_view();
        first.set_point(ByteOffset(1)).unwrap();
        replace(&mut first, 1..1, "X");
        first.set_point(ByteOffset(4)).unwrap();
        second.undo().unwrap();
        assert_eq!(second.point(), ByteOffset(1));
        assert_eq!(first.point(), ByteOffset(3));
        second.redo().unwrap();
        assert_eq!(second.point(), ByteOffset(1));
        assert_eq!(first.point(), ByteOffset(4));
    }

    #[test]
    fn insertion_at_another_views_point_keeps_it_before_inserted_text() {
        let mut first = Buffer::new(
            None,
            DecodedFile {
                text: "abc".into(),
                format: FileFormat::default(),
            },
        );
        let mut second = first.new_view();
        second.set_point(ByteOffset(1)).unwrap();
        replace(&mut first, 1..1, "中");
        assert_eq!(first.point(), ByteOffset(4));
        assert_eq!(second.point(), ByteOffset(1));
        drop(second);
        replace(&mut first, 1..4, "");
        assert_eq!(first.document.borrow().points.len(), 1);
    }

    #[test]
    fn document_metadata_mark_and_save_state_are_shared() {
        let mut first = Buffer::new_file(PathBuf::from("before.txt"));
        let second = first.new_view();
        first.set_mark(ByteOffset(0)).unwrap();
        first.set_path(PathBuf::from("after.txt"));
        first.set_disk_stamp(None);
        assert_eq!(second.path(), Some(PathBuf::from("after.txt")));
        assert_eq!(second.mark(), Some(ByteOffset(0)));
        let save = first.begin_save();
        replace(&mut first, 0..0, "x");
        assert!(second.is_modified());
        first.mark_saved_at(save).unwrap();
        assert!(second.is_modified());
        first.undo().unwrap();
        assert!(!second.is_modified());
    }
}
