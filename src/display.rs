//! Versioned, streaming soft-wrap indexes with font-sensitive grapheme advances.

use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::ops::Range;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering as AtomicOrdering},
};

use gpui_kit::{Font, Pixels, TextRun, TextSystem, WindowTextSystem, black, px};
use unicode_segmentation::{GraphemeCursor, GraphemeIncomplete};

#[cfg(test)]
mod style_tests;
mod typography;
pub use typography::{FontSpan, Typography};

pub use crate::input::DisplayChange;
use crate::input::{DisplayId, DisplaySnapshot};
use crate::text::rope::Chunks;
use crate::text::sum_tree::{ContextLessSummary, Item, SeekTarget, SumTree};
use crate::text::{Bias, ByteOffset, Point, Rope};

#[derive(Clone, Debug, PartialEq)]
pub struct LayoutSettings {
    pub font: Font,
    pub font_size: Pixels,
    pub line_height: Pixels,
    pub wrap_width: Pixels,
    pub tab_size: u32,
    pub typography: Typography,
}

impl LayoutSettings {
    fn compatible_after_edit(&self, other: &Self) -> bool {
        self.font == other.font
            && self.font_size == other.font_size
            && self.line_height == other.line_height
            && self.wrap_width == other.wrap_width
            && self.tab_size == other.tab_size
            && self.typography.compatible_after_edit(&other.typography)
    }

    pub fn validate(&self) {
        for value in [self.font_size, self.line_height, self.wrap_width] {
            assert!(f32::from(value).is_finite() && value > px(0.));
        }
        assert!(self.tab_size > 0);
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LayoutKey {
    pub display: DisplayId,
    pub settings: LayoutSettings,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WrapRow {
    pub byte_len: usize,
    pub content_len: usize,
    /// Logical horizontal advance modulo the tab period, preserved across soft wraps.
    pub tab_phase: Pixels,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RowSummary {
    pub bytes: usize,
    pub rows: usize,
}

impl ContextLessSummary for RowSummary {
    fn zero() -> Self {
        Self::default()
    }
    fn add_summary(&mut self, other: &Self) {
        self.bytes += other.bytes;
        self.rows += other.rows;
    }
}

impl Item for WrapRow {
    type Summary = RowSummary;
    fn summary(&self, (): ()) -> RowSummary {
        RowSummary {
            bytes: self.byte_len,
            rows: 1,
        }
    }
}

struct ByByte(usize);
struct ByRow(usize);

impl<'a> SeekTarget<'a, RowSummary, RowSummary> for ByByte {
    fn cmp(&self, location: &RowSummary, (): ()) -> Ordering {
        Ord::cmp(&self.0, &location.bytes)
    }
}
impl<'a> SeekTarget<'a, RowSummary, RowSummary> for ByRow {
    fn cmp(&self, location: &RowSummary, (): ()) -> Ordering {
        Ord::cmp(&self.0, &location.rows)
    }
}

#[derive(Clone)]
pub struct WrapIndex {
    key: LayoutKey,
    text: Rope,
    rows: SumTree<WrapRow>,
    complete: bool,
    scanned_bytes: usize,
}

#[derive(Clone, Debug)]
pub struct IndexedRow {
    pub index: usize,
    pub start: ByteOffset,
    pub row: WrapRow,
}

impl WrapIndex {
    pub fn key(&self) -> &LayoutKey {
        &self.key
    }
    pub fn text(&self) -> &Rope {
        &self.text
    }
    pub fn is_complete(&self) -> bool {
        self.complete
    }
    pub fn row_count(&self) -> usize {
        self.rows.summary().rows
    }
    pub fn indexed_bytes(&self) -> usize {
        self.rows.summary().bytes
    }
    pub fn scanned_bytes(&self) -> usize {
        self.scanned_bytes
    }

    pub fn row(&self, index: usize) -> Option<IndexedRow> {
        let mut cursor = self.rows.cursor::<RowSummary>(());
        cursor.seek(&ByRow(index), Bias::Right);
        Some(IndexedRow {
            index,
            start: ByteOffset(cursor.start().bytes),
            row: cursor.item()?.clone(),
        })
    }

    pub fn row_for_offset(&self, offset: ByteOffset) -> Option<IndexedRow> {
        if offset.0 > self.indexed_bytes() || self.rows.is_empty() {
            return None;
        }
        if offset.0 == self.indexed_bytes() {
            if !self.complete {
                return None;
            }
            return self.row(self.row_count() - 1);
        }
        let mut cursor = self.rows.cursor::<RowSummary>(());
        cursor.seek(&ByByte(offset.0), Bias::Right);
        Some(IndexedRow {
            index: cursor.start().rows,
            start: ByteOffset(cursor.start().bytes),
            row: cursor.item()?.clone(),
        })
    }

    pub fn visible_rows(&self, range: Range<usize>) -> Vec<IndexedRow> {
        let mut cursor = self.rows.cursor::<RowSummary>(());
        cursor.seek(&ByRow(range.start), Bias::Right);
        let mut rows = Vec::new();
        while let Some(row) = cursor.item() {
            let start = *cursor.start();
            if start.rows >= range.end {
                break;
            }
            rows.push(IndexedRow {
                index: start.rows,
                start: ByteOffset(start.bytes),
                row: row.clone(),
            });
            cursor.next();
        }
        rows
    }
}

/// A job owns persistent prefix/suffix trees, so local reflow never copies an entire index.
#[derive(Clone)]
pub struct WrapJob {
    snapshot: DisplaySnapshot,
    key: LayoutKey,
    prefix: SumTree<WrapRow>,
    suffix: SumTree<WrapRow>,
    range: Range<usize>,
    phase: Pixels,
    reuse: Option<(WrapIndex, DisplayChange)>,
}

impl WrapJob {
    pub fn new(
        snapshot: DisplaySnapshot,
        settings: LayoutSettings,
        previous: Option<&WrapIndex>,
        change: Option<&DisplayChange>,
    ) -> Self {
        settings.validate();
        let mut job = Self {
            key: LayoutKey {
                display: snapshot.id(),
                settings,
            },
            range: 0..snapshot.text().len(),
            snapshot,
            prefix: SumTree::new(()),
            suffix: SumTree::new(()),
            phase: px(0.),
            reuse: None,
        };
        if let (Some(previous), Some(change)) = (previous, change)
            && previous.complete
            && previous
                .key
                .settings
                .compatible_after_edit(&job.key.settings)
            && previous.key.display == change.before
            && job.key.display == change.after
            && previous.key.display.committed.buffer == job.key.display.committed.buffer
            && Self::valid_change(previous.text(), job.snapshot.text(), change)
        {
            let start = previous.text.offset_to_point(change.old_range.start.0).row;
            let end = previous.text.offset_to_point(change.old_range.end.0).row;
            let logical_start = previous.text.point_to_offset(Point::new(start, 0));
            let at_edit = previous
                .row_for_offset(change.old_range.start)
                .expect("edited row");
            let checkpoint = if at_edit.start.0 > logical_start {
                previous.row(at_edit.index - 1).expect("preceding soft row")
            } else {
                at_edit
            };
            let start = checkpoint.start.0;
            job.phase = checkpoint.row.tab_phase;
            job.reuse = Some((previous.clone(), change.clone()));
            let end = if end < previous.text.max_point().row {
                previous.text.point_to_offset(Point::new(end + 1, 0))
            } else {
                previous.text.len()
            };
            let mut cursor = previous.rows.cursor::<RowSummary>(());
            let prefix_rows = previous
                .row_for_offset(ByteOffset(start))
                .expect("logical row start")
                .index;
            job.prefix = cursor.slice(&ByRow(prefix_rows), Bias::Right);
            if end < previous.text.len() {
                cursor.seek_forward(&ByByte(end), Bias::Right);
                job.suffix = cursor.suffix();
            }
            job.range = start
                ..(end - (change.old_range.end.0 - change.old_range.start.0)
                    + (change.new_range.end.0 - change.new_range.start.0));
        }
        job
    }

    fn valid_change(old: &Rope, new: &Rope, change: &DisplayChange) -> bool {
        let a = &change.old_range;
        let b = &change.new_range;
        a.start == b.start
            && a.start <= a.end
            && b.start <= b.end
            && old.is_char_boundary(a.start.0)
            && old.is_char_boundary(a.end.0)
            && new.is_char_boundary(b.start.0)
            && new.is_char_boundary(b.end.0)
            && old.len() - (a.end.0 - a.start.0) == new.len() - (b.end.0 - b.start.0)
    }

    pub fn key(&self) -> &LayoutKey {
        &self.key
    }
    pub fn reflow_range(&self) -> Range<ByteOffset> {
        ByteOffset(self.range.start)..ByteOffset(self.range.end)
    }

    pub fn preview(
        &self,
        metrics: &mut FontMeasure,
        max_rows: usize,
        max_bytes: usize,
    ) -> WrapIndex {
        #[cfg(feature = "native-profiler")]
        let _trace = crate::preparation_trace::Span::new("wrap-preview", max_bytes);
        self.build(metrics, max_rows, max_bytes, None)
            .expect("uncancelled preview")
    }

    pub fn finish(&self, metrics: &mut FontMeasure, cancelled: &AtomicBool) -> Option<WrapIndex> {
        self.build(metrics, usize::MAX, usize::MAX, Some(cancelled))
    }

    fn build(
        &self,
        metrics: &mut FontMeasure,
        max_rows: usize,
        max_bytes: usize,
        cancelled: Option<&AtomicBool>,
    ) -> Option<WrapIndex> {
        assert_eq!(metrics.settings, self.key.settings);
        let text = self.snapshot.text();
        let mut scanner =
            RowScanner::new(text, self.range.clone(), self.suffix.is_empty(), self.phase);
        let limit = self.range.start.saturating_add(max_bytes);
        let mut rows = Vec::new();
        let mut converged_suffix = None;
        while rows.len() < max_rows {
            if cancelled.is_some_and(|flag| flag.load(AtomicOrdering::Relaxed)) {
                return None;
            }
            match scanner.next_row(metrics, limit, cancelled) {
                ScanResult::Row(row) => {
                    rows.push(row);
                    if !scanner.done
                        && let Some((previous, change)) = &self.reuse
                        && scanner.row_start >= change.new_range.end.0
                    {
                        let old_offset =
                            scanner.row_start - change.new_range.end.0 + change.old_range.end.0;
                        if let Some(old_row) = previous.row_for_offset(ByteOffset(old_offset))
                            && old_row.start.0 == old_offset
                            && old_row.row.tab_phase == scanner.row_phase
                        {
                            let mut cursor = previous.rows.cursor::<RowSummary>(());
                            cursor.seek(&ByRow(old_row.index), Bias::Right);
                            converged_suffix = Some(cursor.suffix());
                            break;
                        }
                    }
                }
                ScanResult::Done | ScanResult::Pending => break,
            }
        }
        if cancelled.is_some_and(|flag| flag.load(AtomicOrdering::Relaxed)) {
            return None;
        }
        let complete = scanner.done || converged_suffix.is_some();
        let scanned_bytes = scanner.graphemes.cursor.cur_cursor() - self.range.start
            + scanner.graphemes.context_bytes;
        let mut result = self.prefix.clone();
        result.extend(rows, ());
        if complete {
            result.append(converged_suffix.unwrap_or_else(|| self.suffix.clone()), ());
        }
        Some(WrapIndex {
            key: self.key.clone(),
            text: text.clone(),
            rows: result,
            complete,
            scanned_bytes,
        })
    }
}

pub struct FontMeasure {
    settings: LayoutSettings,
    system: WindowTextSystem,
    ascii: [[Option<Pixels>; 128]; 9],
    other: [HashMap<String, Pixels>; 9],
    other_entries: usize,
    tab_period: Pixels,
    measurements: usize,
}

impl FontMeasure {
    pub fn new(system: Arc<TextSystem>, settings: LayoutSettings) -> Self {
        #[cfg(feature = "native-profiler")]
        let _trace = crate::preparation_trace::Span::new("font-init", 0);
        settings.validate();
        let mut result = Self {
            settings,
            system: WindowTextSystem::new(system),
            ascii: [[None; 128]; 9],
            other: std::array::from_fn(|_| HashMap::new()),
            other_entries: 0,
            tab_period: px(1.),
            measurements: 0,
        };
        result.tab_period = result.width(" ").max(px(0.25)) * result.settings.tab_size as f32;
        result
    }

    pub fn settings(&self) -> &LayoutSettings {
        &self.settings
    }

    pub fn width(&mut self, grapheme: &str) -> Pixels {
        self.width_with(grapheme, self.settings.typography.base())
    }

    pub fn width_at(&mut self, grapheme: &str, offset: usize) -> Pixels {
        self.width_with(grapheme, self.settings.typography.font_at(offset))
    }

    fn width_with(&mut self, grapheme: &str, emphasis: crate::theme::FontEmphasis) -> Pixels {
        let index = |value: Option<bool>| match value {
            None => 0,
            Some(false) => 1,
            Some(true) => 2,
        };
        let font_index = index(emphasis.bold) + 3 * index(emphasis.italic);
        if grapheme.len() == 1 && grapheme.is_ascii() {
            let ix = grapheme.as_bytes()[0] as usize;
            if let Some(width) = self.ascii[font_index][ix] {
                return width;
            }
            let width = self.measure(grapheme, emphasis);
            self.ascii[font_index][ix] = Some(width);
            return width;
        }
        if let Some(width) = self.other[font_index].get(grapheme) {
            return *width;
        }
        let width = self.measure(grapheme, emphasis);
        if self.other_entries < 4096 && grapheme.len() <= 256 {
            self.other[font_index].insert(grapheme.to_owned(), width);
            self.other_entries += 1;
        }
        width
    }

    fn measure(&mut self, text: &str, emphasis: crate::theme::FontEmphasis) -> Pixels {
        #[cfg(feature = "native-profiler")]
        let _trace = crate::preparation_trace::Span::detail("measure-grapheme", text.len());
        if self.measurements == 256 {
            self.system = WindowTextSystem::new(self.system.clone());
            self.measurements = 0;
        }
        self.measurements += 1;
        self.system
            .layout_line(
                text,
                self.settings.font_size,
                &[TextRun {
                    font: emphasis.font(&self.settings.font),
                    ..self.run(text.len())
                }],
                None,
            )
            .width
    }

    pub fn run(&self, len: usize) -> TextRun {
        TextRun {
            len,
            font: self.settings.font.clone(),
            color: black(),
            background_color: None,
            underline: None,
            strikethrough: None,
        }
    }

    pub(crate) fn runs(&self, text: &str, offset: usize) -> Vec<TextRun> {
        self.settings
            .typography
            .runs(text, offset, &self.settings.font)
    }

    pub fn tab_width(&self, phase: Pixels) -> Pixels {
        self.tab_period - phase
    }
    pub fn advance_phase(&self, phase: Pixels, width: Pixels) -> Pixels {
        px((f32::from(phase) + f32::from(width)).rem_euclid(f32::from(self.tab_period)))
    }
}

struct Graphemes<'a> {
    rope: &'a Rope,
    chunks: Chunks<'a>,
    fragment: &'a str,
    fragment_start: usize,
    start: usize,
    context_bytes: usize,
    cursor: GraphemeCursor,
}

struct Grapheme<'a> {
    start: usize,
    end: usize,
    text: Cow<'a, str>,
}

enum GraphemeRead<'a> {
    Item(Grapheme<'a>),
    Pending,
    Done,
}

impl<'a> Graphemes<'a> {
    fn new(rope: &'a Rope, start: usize) -> Self {
        let mut chunks = rope.chunks_in_range(start..rope.len());
        let fragment = chunks.next().unwrap_or("");
        Self {
            rope,
            chunks,
            fragment,
            fragment_start: start,
            start,
            context_bytes: 0,
            cursor: GraphemeCursor::new(start, rope.len(), true),
        }
    }

    fn next(&mut self, limit: usize, cancelled: Option<&AtomicBool>) -> GraphemeRead<'a> {
        loop {
            if cancelled.is_some_and(|flag| flag.load(AtomicOrdering::Relaxed)) {
                return GraphemeRead::Pending;
            }
            let work_limit = limit.saturating_sub(self.context_bytes);
            let limit = work_limit.min(self.rope.len());
            if self.cursor.cur_cursor() > limit
                || (self.cursor.cur_cursor() == limit && limit < self.rope.len())
            {
                return GraphemeRead::Pending;
            }
            let available = limit
                .saturating_sub(self.fragment_start)
                .min(self.fragment.len());
            let fragment = &self.fragment[..self.fragment.floor_char_boundary(available)];
            match self.cursor.next_boundary(fragment, self.fragment_start) {
                Ok(Some(end)) => {
                    let text = if self.start >= self.fragment_start
                        && end <= self.fragment_start + self.fragment.len()
                    {
                        Cow::Borrowed(
                            &self.fragment
                                [self.start - self.fragment_start..end - self.fragment_start],
                        )
                    } else {
                        Cow::Owned(self.rope.chunks_in_range(self.start..end).collect())
                    };
                    let start = self.start;
                    self.start = end;
                    return GraphemeRead::Item(Grapheme { start, end, text });
                }
                Ok(None) => return GraphemeRead::Done,
                Err(GraphemeIncomplete::NextChunk) => {
                    if fragment.len() < self.fragment.len() {
                        return GraphemeRead::Pending;
                    }
                    self.fragment_start += self.fragment.len();
                    self.fragment = self.chunks.next().expect("next rope fragment");
                }
                Err(GraphemeIncomplete::PreContext(end)) => {
                    let context = self
                        .rope
                        .reversed_chunks_in_range(0..end)
                        .next()
                        .expect("preceding rope fragment");
                    if context.len() > work_limit.saturating_sub(self.cursor.cur_cursor()) {
                        return GraphemeRead::Pending;
                    }
                    self.context_bytes += context.len();
                    self.cursor.provide_context(context, end - context.len());
                }
                Err(error) => panic!("invalid forward grapheme stream: {error:?}"),
            }
        }
    }
}

struct Atom {
    start: usize,
    end: usize,
    width: Pixels,
    newline: bool,
    tab: bool,
}

enum ScanResult {
    Row(WrapRow),
    Pending,
    Done,
}

struct RowScanner<'a> {
    graphemes: Graphemes<'a>,
    end: usize,
    include_eof: bool,
    row_start: usize,
    row_phase: Pixels,
    phase: Pixels,
    width: Pixels,
    pending: Option<Atom>,
    done: bool,
}

impl<'a> RowScanner<'a> {
    fn new(text: &'a Rope, range: Range<usize>, include_eof: bool, phase: Pixels) -> Self {
        Self {
            graphemes: Graphemes::new(text, range.start),
            end: range.end,
            include_eof,
            row_start: range.start,
            row_phase: phase,
            phase,
            width: px(0.),
            pending: None,
            done: false,
        }
    }

    fn next_row(
        &mut self,
        metrics: &mut FontMeasure,
        limit: usize,
        cancelled: Option<&AtomicBool>,
    ) -> ScanResult {
        if self.done {
            return ScanResult::Done;
        }
        loop {
            if cancelled.is_some_and(|flag| flag.load(AtomicOrdering::Relaxed)) {
                return ScanResult::Pending;
            }
            if self.pending.is_none() && self.graphemes.start >= self.end {
                self.done = true;
                return if self.include_eof {
                    ScanResult::Row(WrapRow {
                        byte_len: self.end - self.row_start,
                        content_len: self.end - self.row_start,
                        tab_phase: self.row_phase,
                    })
                } else {
                    ScanResult::Done
                };
            }
            let atom = if let Some(atom) = self.pending.take() {
                atom
            } else {
                match self.graphemes.next(limit, cancelled) {
                    GraphemeRead::Pending => return ScanResult::Pending,
                    GraphemeRead::Done => unreachable!("EOF handled before scanning"),
                    GraphemeRead::Item(grapheme) => {
                        let newline = grapheme.text == "\n";
                        let tab = grapheme.text == "\t";
                        let width = if newline {
                            px(0.)
                        } else if tab {
                            metrics.tab_width(self.phase)
                        } else {
                            metrics.width_at(&grapheme.text, grapheme.start)
                        };
                        Atom {
                            start: grapheme.start,
                            end: grapheme.end,
                            width,
                            newline,
                            tab,
                        }
                    }
                }
            };
            if atom.newline {
                let row = WrapRow {
                    byte_len: atom.end - self.row_start,
                    content_len: atom.start - self.row_start,
                    tab_phase: self.row_phase,
                };
                self.row_start = atom.end;
                self.phase = px(0.);
                self.row_phase = px(0.);
                self.width = px(0.);
                return ScanResult::Row(row);
            }
            if self.width + atom.width > metrics.settings.wrap_width && atom.start > self.row_start
            {
                let row = WrapRow {
                    byte_len: atom.start - self.row_start,
                    content_len: atom.start - self.row_start,
                    tab_phase: self.row_phase,
                };
                self.row_start = atom.start;
                self.row_phase = self.phase;
                self.width = px(0.);
                self.pending = Some(atom);
                return ScanResult::Row(row);
            }
            self.width += atom.width;
            self.phase = if atom.tab {
                px(0.)
            } else {
                metrics.advance_phase(self.phase, atom.width)
            };
        }
    }
}
