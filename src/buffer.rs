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
mod repository_history;
mod search;
mod selection;
mod status;
pub(crate) mod tree;
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

#[derive(Clone, Debug)]
pub(crate) struct BatchEdit<'a> {
    pub range: Range<ByteOffset>,
    pub text: &'a str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AppliedBatchEdit {
    pub summary: AppliedEdit,
    pub new_ranges: Vec<Range<ByteOffset>>,
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
    repository_history: Option<repository_history::HistorySession>,
    search: Option<search::SearchSession>,
    tree: Option<tree::TreeSession>,
}

impl Document {
    fn touch(&mut self) {
        self.revision = self
            .revision
            .checked_add(1)
            .expect("document revision exhausted");
    }
}

pub(crate) struct PointMarker(Rc<TrackedPoint>);

impl PointMarker {
    pub(crate) fn position(&self) -> ByteOffset {
        ByteOffset(self.0.get())
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
            repository_history: None,
            search: None,
            tree: None,
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

    fn track_point_at(&self, offset: ByteOffset) -> Rc<TrackedPoint> {
        let point = Rc::new(TrackedPoint::new(offset.0));
        let mut doc = self.document.borrow_mut();
        doc.points.retain(|point| point.strong_count() > 0);
        doc.points.push(Rc::downgrade(&point));
        point
    }

    fn track_point(&self) -> Rc<TrackedPoint> {
        self.track_point_at(self.point())
    }

    pub(crate) fn point_marker(&self) -> PointMarker {
        PointMarker(self.track_point())
    }

    pub(crate) fn point_marker_at(&self, offset: ByteOffset) -> Result<PointMarker, EditError> {
        if !self.text().is_char_boundary(offset.0) {
            return Err(EditError::InvalidRange);
        }
        Ok(PointMarker(self.track_point_at(offset)))
    }

    pub(crate) fn set_marker(
        &self,
        marker: &PointMarker,
        offset: ByteOffset,
    ) -> Result<(), EditError> {
        if !self.text().is_char_boundary(offset.0) {
            return Err(EditError::InvalidRange);
        }
        marker.0.set(offset.0);
        Ok(())
    }

    pub fn new_view(&self) -> Buffer {
        Buffer {
            document: self.document.clone(),
            point: self.track_point(),
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
            .or_else(|| self.history_query().map(|query| query.repository().clone()))
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
        let pointer_mark = self.document.borrow().mark.is_pointer();
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
        if let Some(mark) = mark.filter(|_| pointer_mark) {
            doc.mark.set_pointer(mark);
        } else {
            doc.mark.set(mark, active);
        }
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

    pub(crate) fn syntax_language(&self) -> Option<crate::syntax::LanguageId> {
        self.document.borrow().syntax.language()
    }

    pub(crate) fn syntax_tree(
        &self,
    ) -> Option<(crate::syntax::LanguageId, BufferSnapshot, tree_sitter::Tree)> {
        self.document.borrow().syntax.current_tree()
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
    /// Sets mark at `point` without activating the region when no region is
    /// already active (Emacs `push-mark` on `isearch-done`).
    pub(crate) fn save_mark(&mut self, point: ByteOffset) -> bool {
        if !self.text().is_char_boundary(point.0) {
            return false;
        }
        let mut doc = self.document.borrow_mut();
        if doc.mark.is_active() {
            return false;
        }
        doc.mark.set(Some(point), false);
        doc.touch();
        true
    }
    pub(crate) fn set_pointer_mark(&mut self, point: ByteOffset) -> Result<(), EditError> {
        if !self.text().is_char_boundary(point.0) {
            return Err(EditError::InvalidRange);
        }
        let mut doc = self.document.borrow_mut();
        doc.mark.set_pointer(point);
        doc.touch();
        Ok(())
    }
    pub(crate) fn deactivate_pointer_mark(&mut self) {
        if self.document.borrow().mark.is_pointer() {
            self.deactivate_mark();
        }
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

    /// Reverts a file-visiting buffer to `loaded` using a minimal diff replacement
    /// while preserving point, undo history, and read-only state.
    pub(crate) fn revert_visited(
        &mut self,
        loaded: crate::files::LoadedFile,
        preserve_eob: bool,
    ) -> crate::revert::RevertOutcome {
        self.break_history_group();
        let old_text = self.text();
        let old_len = old_text.len();
        let new_len = loaded.file.text.len();

        let Some((old_range, new_mid)) = crate::revert::diff_range(&old_text, &loaded.file.text)
        else {
            let mut doc = self.document.borrow_mut();
            let format_changed = doc.format != loaded.file.format;
            let was_modified = doc.modified;
            doc.format = loaded.file.format;
            doc.disk_stamp = loaded.stamp;
            doc.identity = loaded.stamp.and_then(FileStamp::identity);
            if was_modified {
                doc.history.mark_only_current_clean();
                doc.modified = false;
            }
            if format_changed || was_modified {
                doc.touch();
            }
            return crate::revert::RevertOutcome::Unchanged;
        };

        let delta = new_mid.len() as isize - (old_range.end.0 - old_range.start.0) as isize;
        let (points, point_before, removed, undo_mark) = {
            let mut doc = self.document.borrow_mut();
            doc.points.retain(|point| point.strong_count() > 0);
            let points: Vec<_> = doc
                .points
                .iter()
                .filter_map(Weak::upgrade)
                .map(|point| {
                    let old_offset = point.get();
                    (point, old_offset, old_offset == old_len)
                })
                .collect();
            let point_before = self.point();
            let removed = old_text.slice(old_range.start.0..old_range.end.0);
            let undo_mark = doc.mark.position().and_then(|m| {
                (m > old_range.start && m <= old_range.end).then(|| (0, m.0 - old_range.start.0))
            });
            (points, point_before, removed, undo_mark)
        };

        let inserted = Rope::from(new_mid);
        self.apply_validated_batch(
            &[BatchEdit {
                range: old_range.clone(),
                text: new_mid,
            }],
            EditSource::Command,
            None,
        );

        for (point, old_offset, was_eob) in points {
            let next_offset = if preserve_eob && was_eob {
                new_len
            } else if old_offset <= old_range.start.0 {
                old_offset
            } else if old_offset >= old_range.end.0 {
                (old_offset as isize + delta) as usize
            } else {
                loaded
                    .file
                    .text
                    .floor_char_boundary(old_offset.min(new_len))
            };
            point.set(next_offset);
        }

        let point_after = self.point();
        let mut doc = self.document.borrow_mut();
        doc.history.record_revert(
            old_range.start,
            removed,
            inserted,
            point_before,
            point_after,
            undo_mark,
            self.view_id,
        );
        doc.modified = false;
        doc.format = loaded.file.format;
        doc.disk_stamp = loaded.stamp;
        doc.identity = loaded.stamp.and_then(FileStamp::identity);
        doc.touch();
        crate::revert::RevertOutcome::Reverted
    }

    pub fn format(&self) -> FileFormat {
        self.document.borrow().format
    }
    pub(crate) fn needs_exit_save(&self) -> bool {
        let document = self.document.borrow();
        document.path.is_some() && document.modified
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
    fn validate_batch(
        &self,
        snapshot: SnapshotId,
        edits: &[BatchEdit<'_>],
    ) -> Result<(), EditError> {
        self.validate_snapshot(snapshot)?;
        let mut previous_end = None;
        for edit in edits {
            self.validate_range(&edit.range)?;
            if let Some(end) = previous_end
                && edit.range.start < end
            {
                return Err(EditError::InvalidRange);
            }
            if !(edit.range.is_empty() && edit.text.is_empty()) {
                self.check_writable_range(edit.range.clone())?;
            }
            previous_end = Some(edit.range.end);
        }
        Ok(())
    }

    fn apply_batch(
        &mut self,
        snapshot: SnapshotId,
        edits: &[BatchEdit<'_>],
        source: EditSource,
        primary_index: Option<usize>,
    ) -> Result<Option<AppliedBatchEdit>, EditError> {
        self.validate_batch(snapshot, edits)?;
        if edits
            .iter()
            .all(|edit| edit.range.is_empty() && edit.text.is_empty())
        {
            return Ok(None);
        }
        Ok(Some(self.apply_validated_batch(
            edits,
            source,
            primary_index,
        )))
    }

    fn apply_validated_edit(&mut self, edit: Edit<'_>) -> AppliedEdit {
        self.apply_validated_batch(
            &[BatchEdit {
                range: edit.range,
                text: edit.text,
            }],
            edit.source,
            Some(0),
        )
        .summary
    }

    fn apply_validated_batch(
        &mut self,
        edits: &[BatchEdit<'_>],
        source: EditSource,
        primary_index: Option<usize>,
    ) -> AppliedBatchEdit {
        let mut delta: isize = 0;
        let mut new_ranges = Vec::with_capacity(edits.len());
        let mut first_active = None;
        let mut last_active = None;
        for (index, edit) in edits.iter().enumerate() {
            let new_start = (edit.range.start.0 as isize + delta) as usize;
            let new_end = new_start + edit.text.len();
            new_ranges.push(ByteOffset(new_start)..ByteOffset(new_end));
            let removed = edit.range.end.0 - edit.range.start.0;
            delta += edit.text.len() as isize - removed as isize;
            if !edit.range.is_empty() || !edit.text.is_empty() {
                if first_active.is_none() {
                    first_active = Some(index);
                }
                last_active = Some(index);
            }
        }
        let first_active = first_active.expect("non-empty batch edit");
        let last_active = last_active.expect("non-empty batch edit");

        let mut doc = self.document.borrow_mut();
        let before = doc.snapshot.id;
        let version = TextVersion(
            before
                .version
                .0
                .checked_add(1)
                .expect("text version exhausted"),
        );
        doc.snapshot.id.version = version;
        doc.annotations = None;

        for edit in edits.iter().rev() {
            if edit.range.is_empty() && edit.text.is_empty() {
                continue;
            }
            let start = edit.range.start.0;
            let old_end = edit.range.end.0;
            let inserted = edit.text.len();
            doc.snapshot.text.replace(start..old_end, edit.text);
            doc.syntax
                .edited(doc.snapshot.clone(), start..old_end, start + inserted);
        }
        let map_offset = |p: usize| -> usize {
            let mut delta: isize = 0;
            for (edit, new_range) in edits.iter().zip(&new_ranges) {
                if edit.range.is_empty() && edit.text.is_empty() {
                    continue;
                }
                let start = edit.range.start.0;
                let old_end = edit.range.end.0;
                if p <= start {
                    return (p as isize + delta) as usize;
                }
                if p <= old_end {
                    return new_range.start.0;
                }
                delta += edit.text.len() as isize - (old_end - start) as isize;
            }
            (p as isize + delta) as usize
        };
        doc.points.retain(|weak| {
            if let Some(point) = weak.upgrade() {
                if primary_index.is_none() || !Rc::ptr_eq(&point, &self.point) {
                    point.set(map_offset(point.get()));
                }
                true
            } else {
                false
            }
        });
        let mark = doc
            .mark
            .position()
            .map(|mark| ByteOffset(map_offset(mark.0)));
        doc.mark.set(mark, false);
        if let Some(index) = primary_index {
            self.point.set(new_ranges[index].end.0);
        }
        doc.modified = true;
        let after = doc.snapshot.id;
        let summary = AppliedEdit {
            before,
            after,
            old_range: edits[first_active].range.start..edits[last_active].range.end,
            new_range: new_ranges[first_active].start..new_ranges[last_active].end,
            source,
        };
        doc.last_edit = Some(summary.clone());
        doc.touch();
        AppliedBatchEdit {
            summary,
            new_ranges,
        }
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
    fn point_markers_follow_shared_edits_but_not_explicit_point_motion() {
        let mut first = Buffer::new(
            None,
            DecodedFile {
                text: "abcdef".into(),
                format: FileFormat::default(),
            },
        );
        let mut second = first.new_view();
        second.set_point(ByteOffset(4)).unwrap();
        let marker = second.point_marker();
        replace(&mut first, 0..0, "中");
        assert_eq!(marker.position(), second.point());
        assert_eq!(marker.position(), ByteOffset(7));
        second.set_point(ByteOffset(5)).unwrap();
        assert_eq!(marker.position(), ByteOffset(7));
        let typed = second.point_marker();
        replace(&mut second, 5..5, "x");
        assert_eq!(typed.position(), ByteOffset(5));
        assert_eq!(second.point(), ByteOffset(6));
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

    #[test]
    fn revert_visited_preserves_prefix_suffix_and_eob_points_and_updates_undo() {
        let loaded = |text: &str, format: FileFormat| crate::files::LoadedFile {
            file: DecodedFile {
                text: text.to_owned(),
                format,
            },
            stamp: None,
        };
        let mut prefix_view = Buffer::visiting(
            PathBuf::from("file.txt"),
            DecodedFile {
                text: "alpha\nbeta\ngamma\n".into(),
                format: FileFormat::default(),
            },
            None,
        );
        let mut suffix_view = prefix_view.new_view();
        let mut interior_view = prefix_view.new_view();
        let mut eob_view = prefix_view.new_view();

        prefix_view.set_point(ByteOffset(2)).unwrap();
        suffix_view.set_point(ByteOffset(13)).unwrap();
        interior_view.set_point(ByteOffset(9)).unwrap();
        eob_view.set_point(ByteOffset(17)).unwrap();

        assert_eq!(
            prefix_view.revert_visited(loaded("alpha\nB\ngamma\n", FileFormat::default()), true),
            crate::revert::RevertOutcome::Reverted
        );
        assert_eq!(prefix_view.text().to_string(), "alpha\nB\ngamma\n");
        assert!(!prefix_view.is_modified());
        assert_eq!(prefix_view.point(), ByteOffset(2));
        assert_eq!(suffix_view.point(), ByteOffset(10));
        // Interior offset 9 lands in the middle of multi-byte or past replacement -> valid boundary.
        assert!(prefix_view.text().is_char_boundary(interior_view.point().0));
        assert_eq!(eob_view.point(), ByteOffset(14));

        // Undo restores pre-revert text and marks buffer modified relative to disk.
        prefix_view.undo().unwrap().unwrap();
        assert_eq!(prefix_view.text().to_string(), "alpha\nbeta\ngamma\n");
        assert!(prefix_view.is_modified());
        assert_eq!(prefix_view.point(), ByteOffset(2));

        // Redo restores reverted text and returns to clean.
        prefix_view.redo().unwrap().unwrap();
        assert_eq!(prefix_view.text().to_string(), "alpha\nB\ngamma\n");
        assert!(!prefix_view.is_modified());
    }

    #[test]
    fn revert_visited_handles_unchanged_text_saved_history_and_read_only() {
        let loaded = |text: &str, format: FileFormat| crate::files::LoadedFile {
            file: DecodedFile {
                text: text.to_owned(),
                format,
            },
            stamp: None,
        };
        let mut buffer = Buffer::visiting(
            PathBuf::from("file.txt"),
            DecodedFile {
                text: "v1\n".into(),
                format: FileFormat::default(),
            },
            None,
        );
        let initial_version = buffer.snapshot_id().version;

        // Identical clean revert leaves text version untouched.
        assert_eq!(
            buffer.revert_visited(loaded("v1\n", FileFormat::default()), false),
            crate::revert::RevertOutcome::Unchanged
        );
        assert_eq!(buffer.snapshot_id().version, initial_version);

        // User edit -> save -> edit -> revert to external v2.
        replace(&mut buffer, 3..3, "saved\n");
        let point = buffer.begin_save();
        buffer.mark_saved_at(point).unwrap();
        assert!(!buffer.is_modified());
        replace(&mut buffer, 9..9, "draft\n");
        assert!(buffer.is_modified());

        let crlf = FileFormat {
            bom: true,
            line_ending: crate::files::LineEnding::Crlf,
        };
        assert_eq!(
            buffer.revert_visited(loaded("external\n", crlf), false),
            crate::revert::RevertOutcome::Reverted
        );
        assert!(!buffer.is_modified());
        assert_eq!(buffer.format(), crlf);

        // Undoing back through earlier saved state keeps modified = true because disk has "external\n".
        buffer.undo().unwrap().unwrap();
        assert!(buffer.is_modified());
        buffer.undo().unwrap().unwrap();
        assert!(buffer.is_modified());
        buffer.redo().unwrap().unwrap();
        buffer.redo().unwrap().unwrap();
        assert!(!buffer.is_modified());

        // Read-only buffer can still be reverted while staying read-only.
        buffer.set_read_only(true);
        assert_eq!(
            buffer.revert_visited(loaded("external2\n", FileFormat::default()), false),
            crate::revert::RevertOutcome::Reverted
        );
        assert!(buffer.is_read_only());
        assert!(!buffer.is_modified());
        assert_eq!(buffer.text().to_string(), "external2\n");
    }
}
