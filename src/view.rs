//! A viewport over a versioned wrap index; only visible rows are shaped.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use gpui_kit::component::ActiveTheme;
use gpui_kit::*;
use unicode_segmentation::UnicodeSegmentation;

use crate::buffer::{Buffer, EditError, SavePoint};
use crate::command::{Command, Scope};
use crate::display::{FontMeasure, IndexedRow, LayoutKey, LayoutSettings, WrapIndex, WrapJob};
use crate::editing::{
    CommandOutcome, CommandViewport, EditTarget, InputDisposition, KillRing, RecenterPosition,
    RouteError, TAB_WIDTH, ViewportRequest,
};
use crate::input::{DisplayChange, DisplaySnapshot, SessionToken};
use crate::keymap::Keymap;
use crate::platform::{BufferInputHandler, InputOperation};
use crate::text::ByteOffset;

mod motion;

const PREVIEW_BYTES: usize = 8192;
const SYNC_SHAPE_BYTES: usize = 8192;
const PADDING: Pixels = px(8.);

pub(crate) enum KeyLayers<'a> {
    Edit(Option<&'a Keymap>),
    Exclusive(&'a Keymap),
}

/// Result of routing one keystroke through a buffer view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum KeyRoute {
    /// The view consumed the key sequence; stop further propagation.
    Handled,
    /// A command owned by a layer above the edit target; the owner executes it.
    Requested(Command),
    /// Leave the key to platform text input or framework handlers.
    Unhandled,
}

struct BackgroundWork {
    cancelled: Arc<AtomicBool>,
    _task: Task<()>,
}

impl Drop for BackgroundWork {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

struct RowPart {
    range: Range<ByteOffset>,
    x: Pixels,
    width: Pixels,
    line: Option<ShapedLine>,
}

struct ShapedRow {
    source: IndexedRow,
    parts: Vec<RowPart>,
    width: Pixels,
}

impl ShapedRow {
    fn shape(
        row: IndexedRow,
        index: &WrapIndex,
        metrics: &mut FontMeasure,
        system: &WindowTextSystem,
        color: Hsla,
    ) -> Self {
        let text: String = index
            .text()
            .chunks_in_range(row.start.0..row.start.0 + row.row.content_len)
            .collect();
        let mut parts = Vec::new();
        let mut phase = row.row.tab_phase;
        let mut x = px(0.);
        let mut start = 0;
        for (offset, fragment) in text.split_inclusive('\t').scan(0, |offset, fragment| {
            let start = *offset;
            *offset += fragment.len();
            Some((start, fragment))
        }) {
            let has_tab = fragment.ends_with('\t');
            let content = fragment.trim_end_matches('\t');
            if !content.is_empty() {
                let mut run = metrics.run(content.len());
                run.color = color;
                let line = system.shape_line(
                    content.to_owned().into(),
                    metrics.settings().font_size,
                    &[run],
                    None,
                );
                let width = line.width();
                parts.push(RowPart {
                    range: ByteOffset(row.start.0 + offset)
                        ..ByteOffset(row.start.0 + offset + content.len()),
                    x,
                    width,
                    line: Some(line),
                });
                x += width;
                for grapheme in content.graphemes(true) {
                    let width = metrics.width(grapheme);
                    phase = metrics.advance_phase(phase, width);
                }
            }
            if has_tab {
                let width = metrics.tab_width(phase);
                let tab = row.start.0 + offset + content.len();
                parts.push(RowPart {
                    range: ByteOffset(tab)..ByteOffset(tab + 1),
                    x,
                    width,
                    line: None,
                });
                x += width;
                phase = px(0.);
            }
            start = offset + fragment.len();
        }
        debug_assert_eq!(start, text.len());
        Self {
            source: row,
            parts,
            width: x,
        }
    }

    fn x_for_offset(&self, offset: ByteOffset) -> Option<Pixels> {
        if offset < self.source.start
            || offset.0 > self.source.start.0 + self.source.row.content_len
        {
            return None;
        }
        for part in &self.parts {
            if offset >= part.range.start && offset <= part.range.end {
                return Some(
                    part.x
                        + match &part.line {
                            Some(line) => line.x_for_index(offset.0 - part.range.start.0),
                            None if offset == part.range.start => px(0.),
                            None => part.width,
                        },
                );
            }
        }
        Some(self.width)
    }

    fn offset_for_x(&self, x: Pixels) -> ByteOffset {
        for part in &self.parts {
            if x <= part.x + part.width {
                let offset = match &part.line {
                    Some(line) => {
                        let x = (x - part.x).max(px(0.));
                        let before = line.index_for_x(x).unwrap_or(line.len());
                        let after = line.text[before..]
                            .chars()
                            .next()
                            .map_or(before, |ch| before + ch.len_utf8());
                        if x - line.x_for_index(before) > line.x_for_index(after) - x {
                            after
                        } else {
                            before
                        }
                    }
                    None => usize::from(x > part.x + part.width / 2.),
                };
                return ByteOffset(part.range.start.0 + offset);
            }
        }
        ByteOffset(self.source.start.0 + self.source.row.content_len)
    }
}

#[derive(Clone)]
pub struct ViewportLayout {
    key: LayoutKey,
    bounds: Bounds<Pixels>,
    scroll: Point<Pixels>,
    rows: Vec<Arc<ShapedRow>>,
    requested_rows: usize,
}

impl ViewportLayout {
    pub fn key(&self) -> &LayoutKey {
        &self.key
    }
    pub fn bounds(&self) -> Bounds<Pixels> {
        self.bounds
    }
    pub fn scroll(&self) -> Point<Pixels> {
        self.scroll
    }
    pub fn visible_row_count(&self) -> usize {
        self.rows.len()
    }

    pub fn hit_test(&self, position: Point<Pixels>) -> Option<ByteOffset> {
        if !self.bounds.contains(&position) {
            return None;
        }
        let y = position.y - self.bounds.top() + self.scroll.y;
        let row = (f32::from(y) / f32::from(self.key.settings.line_height))
            .floor()
            .max(0.) as usize;
        let line = self.rows.iter().find(|line| line.source.index == row)?;
        Some(line.offset_for_x(position.x - self.bounds.left() + self.scroll.x))
    }

    fn origin(&self, row: &ShapedRow) -> Point<Pixels> {
        point(
            self.bounds.left() - self.scroll.x,
            self.bounds.top() + self.key.settings.line_height * row.source.index as f32
                - self.scroll.y,
        )
    }

    pub fn region_bounds(
        &self,
        ranges: impl IntoIterator<Item = Range<ByteOffset>>,
    ) -> Vec<Bounds<Pixels>> {
        let mut bounds = Vec::new();
        for range in ranges {
            for row in &self.rows {
                let content_end = ByteOffset(row.source.start.0 + row.source.row.content_len);
                let start = range.start.max(row.source.start);
                let end = range
                    .end
                    .min(ByteOffset(row.source.start.0 + row.source.row.byte_len));
                if start >= end {
                    continue;
                }
                let Some(left) = row.x_for_offset(start.min(content_end)) else {
                    continue;
                };
                let right = if end > content_end {
                    self.bounds.size.width + self.scroll.x
                } else if let Some(right) = row.x_for_offset(end) {
                    right
                } else {
                    continue;
                };
                let origin = self.origin(row);
                let right = (origin.x + right.max(left + px(1.))).min(self.bounds.right());
                let left = (origin.x + left).max(self.bounds.left());
                let top = origin.y.max(self.bounds.top());
                let bottom = (origin.y + self.key.settings.line_height).min(self.bounds.bottom());
                if left < right && top < bottom {
                    bounds.push(Bounds::new(
                        point(left, top),
                        size(right - left, bottom - top),
                    ));
                }
            }
        }
        bounds
    }

    pub fn bounds_for_range(
        &self,
        range: Range<ByteOffset>,
        index: &WrapIndex,
    ) -> Option<Bounds<Pixels>> {
        if self.key != *index.key() || range.start > range.end {
            return None;
        }
        let target = index.row_for_offset(range.start)?;
        let row = self
            .rows
            .iter()
            .find(|row| row.source.index == target.index)?;
        let start = row.x_for_offset(range.start)?;
        let end = row.x_for_offset(ByteOffset(
            range.end.0.min(target.start.0 + target.row.content_len),
        ))?;
        let origin = self.origin(row);
        let bounds = Bounds::new(
            point(origin.x + start, origin.y),
            size((end - start).max(px(1.)), self.key.settings.line_height),
        );
        if bounds.bottom() < self.bounds.top() || bounds.top() > self.bounds.bottom() {
            return None;
        }
        Some(bounds)
    }
}

#[derive(PartialEq)]
struct InputGeometry {
    key: LayoutKey,
    bounds: Bounds<Pixels>,
    scroll: Point<Pixels>,
    complete: bool,
    rows: usize,
    selection: Range<ByteOffset>,
}

pub struct BufferView {
    target: EditTarget,
    keymap: Keymap,
    focus: FocusHandle,
    settings: LayoutSettings,
    metrics: Option<FontMeasure>,
    index: Option<WrapIndex>,
    layout_work: Option<BackgroundWork>,
    shape_work: Option<BackgroundWork>,
    shape_range: Option<(LayoutKey, Range<usize>)>,
    cache: HashMap<usize, Arc<ShapedRow>>,
    color: Option<Hsla>,
    geometry: Option<ViewportLayout>,
    content_bounds: Option<Bounds<Pixels>>,
    retained_paint_layout: Option<ViewportLayout>,
    scroll: Point<Pixels>,
    reflow_base: Option<WrapIndex>,
    pending_change: Option<DisplayChange>,
    registration: u64,
    accepting_input: bool,
    reveal_caret: bool,
    pending_recenter: Option<RecenterPosition>,
    input_geometry: Option<InputGeometry>,
    message: Option<SharedString>,
    _input_subscriptions: Vec<Subscription>,
    #[cfg(test)]
    geometry_refreshes: usize,
}

impl BufferView {
    pub fn new(buffer: Buffer, cx: &mut Context<Self>) -> Self {
        let target = EditTarget::with_kill_ring(buffer, cx.default_global::<KillRing>().clone());
        Self::with_target(target, cx)
    }

    pub fn with_target(target: EditTarget, cx: &mut Context<Self>) -> Self {
        Self {
            target,
            keymap: Keymap::for_scope(Scope::Edit),
            focus: cx.focus_handle(),
            settings: LayoutSettings {
                font: font("monospace"),
                font_size: px(16.),
                line_height: px(24.),
                wrap_width: px(1.),
                tab_size: TAB_WIDTH,
            },
            metrics: None,
            index: None,
            layout_work: None,
            shape_work: None,
            shape_range: None,
            cache: HashMap::new(),
            color: None,
            geometry: None,
            content_bounds: None,
            retained_paint_layout: None,
            scroll: Point::default(),
            reflow_base: None,
            pending_change: None,
            registration: 0,
            accepting_input: true,
            reveal_caret: false,
            pending_recenter: None,
            input_geometry: None,
            message: None,
            _input_subscriptions: Vec::new(),
            #[cfg(test)]
            geometry_refreshes: 0,
        }
    }

    pub fn buffer(&self) -> &Buffer {
        self.target.buffer()
    }

    pub(crate) fn mark_saved_at(
        &mut self,
        point: SavePoint,
        cx: &mut Context<Self>,
    ) -> Result<(), EditError> {
        let result = self.target.mark_saved_at(point);
        cx.notify();
        result
    }

    pub(crate) fn set_read_only(&mut self, read_only: bool, cx: &mut Context<Self>) {
        self.target.set_read_only(read_only);
        self.invalidate_input_registration();
        cx.notify();
    }

    pub(crate) fn begin_save(&mut self) -> SavePoint {
        self.target.begin_save()
    }

    pub(crate) fn set_file_path(&mut self, path: std::path::PathBuf, cx: &mut Context<Self>) {
        self.target.set_file_path(path);
        cx.notify();
    }

    pub(crate) fn set_disk_stamp(
        &mut self,
        stamp: Option<crate::files::FileStamp>,
        cx: &mut Context<Self>,
    ) {
        self.target.set_disk_stamp(stamp);
        cx.notify();
    }

    pub(crate) fn set_display_name(&mut self, name: String, cx: &mut Context<Self>) {
        self.target.set_display_name(name);
        cx.notify();
    }
    pub fn display_snapshot(&self) -> DisplaySnapshot {
        self.target.display_snapshot()
    }
    pub fn wrap_index(&self) -> Option<&WrapIndex> {
        self.index.as_ref()
    }
    pub fn viewport_layout(&self) -> Option<&ViewportLayout> {
        self.geometry.as_ref().filter(|geometry| {
            geometry.key.display == self.display_snapshot().id()
                && geometry.key.settings == self.settings
        })
    }

    /// A paint-only fallback may retain the same display at its previous font / width.
    /// Input geometry must use `viewport_layout` instead.
    pub fn paint_layout(&self) -> Option<&ViewportLayout> {
        let current = self.viewport_layout();
        current
            .filter(|layout| !layout.rows.is_empty())
            .or_else(|| {
                self.retained_paint_layout
                    .as_ref()
                    .filter(|layout| layout.key.display == self.display_snapshot().id())
            })
            .or(current)
    }

    pub fn region_bounds(&self) -> Vec<Bounds<Pixels>> {
        self.paint_layout()
            .map(|layout| layout.region_bounds(self.display_snapshot().region_ranges()))
            .unwrap_or_default()
    }

    fn retain_paint_layout(&mut self) {
        if let Some(layout) = self
            .geometry
            .take()
            .filter(|layout| !layout.rows.is_empty())
        {
            self.retained_paint_layout = Some(layout);
        }
    }

    pub fn layout_ready(&self) -> bool {
        self.index.as_ref().is_some_and(WrapIndex::is_complete)
            && self
                .viewport_layout()
                .is_some_and(|layout| layout.rows.len() == layout.requested_rows)
    }

    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    pub(crate) fn set_message(&mut self, message: impl Into<String>, cx: &mut Context<Self>) {
        self.message = Some(SharedString::from(message.into()));
        cx.notify();
    }

    /// Owns the platform input session lifetime. Keyboard routing itself lives
    /// in `AppShell`, which supplies the layers above this target.
    pub(crate) fn install_input_lifecycle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let focus = cx.on_focus(&self.focus, window, |view, _, cx| {
            if view.accepting_input {
                view.resume_input(cx);
            }
        });
        let blur = cx.on_blur(&self.focus, window, |view, _, cx| view.end_input(cx));
        let activation = cx.observe_window_activation(window, |view, window, cx| {
            if !window.is_window_active() {
                view.end_input(cx);
            } else if view.accepting_input && view.focus.is_focused(window) {
                view.resume_input(cx);
            }
        });
        self._input_subscriptions = vec![focus, blur, activation];
    }

    fn queue_change(&mut self, change: DisplayChange, cx: &mut Context<Self>) {
        self.pending_recenter = None;
        if self.reflow_base.is_none() {
            self.reflow_base = self
                .index
                .as_ref()
                .filter(|index| index.is_complete())
                .cloned();
        }
        self.pending_change = Some(match self.pending_change.take() {
            Some(previous) if previous.after == change.before => previous.compose(&change),
            _ => change,
        });
        self.geometry = None;
        self.retained_paint_layout = None;
        self.layout_work = None;
        self.shape_work = None;
        self.shape_range = None;
        cx.notify();
    }

    fn clear_reflow(&mut self, cx: &mut Context<Self>) {
        self.pending_change = None;
        if let Some(previous) = self.reflow_base.take() {
            cx.background_executor()
                .spawn(async move {
                    drop(previous);
                })
                .detach();
        }
    }

    pub(crate) fn end_input(&mut self, cx: &mut Context<Self>) {
        if self.target.snapshot_for(self.target.token()).is_err() {
            return;
        }
        let change = self.target.end_input();
        self.queue_change(change, cx);
        self.input_geometry = None;
        self.invalidate_input_registration();
    }

    fn resume_input(&mut self, cx: &mut Context<Self>) {
        if let Some(change) = self.target.resume_input() {
            self.queue_change(change, cx);
        }
    }

    pub(crate) fn suspend_input(&mut self, cx: &mut Context<Self>) {
        self.accepting_input = false;
        self.end_input(cx);
    }

    pub(crate) fn resume_accepting_input(&mut self, cx: &mut Context<Self>) {
        self.accepting_input = true;
        self.resume_input(cx);
        cx.notify();
    }

    pub(crate) fn restore_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.resume_accepting_input(cx);
        window.focus(&self.focus, cx);
    }

    pub(crate) fn platform_snapshot(
        &self,
        registration: u64,
        token: SessionToken,
        window: &Window,
    ) -> Option<DisplaySnapshot> {
        if !self.accepting_input
            || registration != self.registration
            || !self.focus.is_focused(window)
        {
            return None;
        }
        self.target.snapshot_for(token).ok()
    }

    pub(crate) fn accepts_platform_input(
        &self,
        registration: u64,
        token: SessionToken,
        window: &Window,
    ) -> bool {
        self.platform_snapshot(registration, token, window)
            .is_some()
            && self.target.allows_text_input(token)
    }

    pub(crate) fn platform_update(
        &mut self,
        registration: u64,
        token: SessionToken,
        operation: InputOperation<'_>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<SessionToken> {
        self.platform_snapshot(registration, token, window)?;
        let result = match operation {
            InputOperation::Replace(range, text) => {
                self.target.replace_text(token, range, text).map(Some)
            }
            InputOperation::Mark(range, text, selection) => self
                .target
                .replace_and_mark(token, range, text, selection)
                .map(Some),
            InputOperation::Unmark => self.target.commit_preedit(token),
        };
        let update = match result {
            Ok(update) => update,
            Err(error) => {
                self.message = Some(error.to_string().into());
                cx.notify();
                return None;
            }
        };
        if let Some(update) = update {
            self.queue_change(update.change, cx);
            self.reveal_caret = true;
            self.message = None;
            window.notify_text_input_changed(&self.focus);
        }
        Some(self.target.token())
    }

    fn invalidate_input_registration(&mut self) {
        self.registration = self
            .registration
            .checked_add(1)
            .expect("input registration exhausted");
    }

    fn bind_input(&mut self, cx: &mut Context<Self>) -> BufferInputHandler {
        self.invalidate_input_registration();
        BufferInputHandler {
            view: cx.weak_entity(),
            token: self.target.token(),
            registration: self.registration,
        }
    }

    fn refresh_input_geometry(&mut self, window: &Window) {
        if !self.accepting_input || !self.focus.is_focused(window) {
            return;
        }
        let Some(layout) = self.viewport_layout() else {
            return;
        };
        let Some(index) = &self.index else {
            return;
        };
        let selection = self.display_snapshot().selected_range();
        if layout
            .bounds_for_range(selection.end..selection.end, index)
            .is_none()
        {
            return;
        }
        let geometry = InputGeometry {
            key: layout.key.clone(),
            bounds: layout.bounds,
            scroll: layout.scroll,
            complete: index.is_complete(),
            rows: layout.rows.len(),
            selection,
        };
        if self.input_geometry.as_ref() != Some(&geometry) {
            self.input_geometry = Some(geometry);
            window.invalidate_character_coordinates();
            #[cfg(test)]
            {
                self.geometry_refreshes += 1;
            }
        }
    }

    pub fn set_font(
        &mut self,
        font: Font,
        font_size: Pixels,
        line_height: Pixels,
        cx: &mut Context<Self>,
    ) {
        self.retain_paint_layout();
        self.settings.font = font;
        self.settings.font_size = font_size;
        self.settings.line_height = line_height;
        self.settings.validate();
        self.geometry = None;
        self.layout_work = None;
        self.shape_work = None;
        self.shape_range = None;
        cx.notify();
    }

    pub fn scroll_by(&mut self, delta: Point<Pixels>, cx: &mut Context<Self>) {
        self.reveal_caret = false;
        self.pending_recenter = None;
        self.scroll.x = (self.scroll.x + delta.x).max(px(0.));
        self.scroll.y = (self.scroll.y + delta.y).max(px(0.));
        self.geometry = None;
        self.shape_work = None;
        self.shape_range = None;
        cx.notify();
    }

    fn ensure_index(&mut self, bounds: Bounds<Pixels>, cx: &mut Context<Self>) {
        self.settings.wrap_width = bounds.size.width.max(px(1.));
        let snapshot = self.display_snapshot();
        let key = LayoutKey {
            display: snapshot.id(),
            settings: self.settings.clone(),
        };
        if self.index.as_ref().is_some_and(|index| *index.key() == key) {
            return;
        }
        self.layout_work = None;
        self.shape_work = None;
        self.shape_range = None;
        self.cache.clear();
        self.retain_paint_layout();
        if self
            .metrics
            .as_ref()
            .is_none_or(|metrics| *metrics.settings() != self.settings)
        {
            self.metrics = Some(FontMeasure::new(
                cx.text_system().clone(),
                self.settings.clone(),
            ));
        }
        let job = WrapJob::new(
            snapshot,
            self.settings.clone(),
            self.reflow_base.as_ref().or(self.index.as_ref()),
            self.pending_change.as_ref(),
        );
        let visible_rows = (f32::from(bounds.size.height) / f32::from(self.settings.line_height))
            .ceil() as usize
            + 2;
        let preview = job.preview(self.metrics.as_mut().unwrap(), visible_rows, PREVIEW_BYTES);
        let complete = preview.is_complete();
        if let Some(previous) = self.index.replace(preview) {
            cx.background_executor()
                .spawn(async move {
                    drop(previous);
                })
                .detach();
        }
        if complete {
            self.clear_reflow(cx);
            return;
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancelled.clone();
        let system = cx.text_system().clone();
        let settings = self.settings.clone();
        let worker = cx.background_executor().spawn(async move {
            job.finish(&mut FontMeasure::new(system, settings), &worker_cancel)
        });
        let task = cx.spawn(async move |view, cx| {
            if let Some(result) = worker.await {
                view.update(cx, |view, cx| {
                    if view
                        .index
                        .as_ref()
                        .is_some_and(|index| index.key() == result.key())
                        && result.key().settings == view.settings
                        && result.key().display == view.display_snapshot().id()
                    {
                        if let Some(previous) = view.index.replace(result) {
                            cx.background_executor()
                                .spawn(async move {
                                    drop(previous);
                                })
                                .detach();
                        }
                        view.layout_work = None;
                        view.clear_reflow(cx);
                        cx.notify();
                    }
                })
                .ok();
            }
        });
        self.layout_work = Some(BackgroundWork {
            cancelled,
            _task: task,
        });
    }

    fn prepare(
        &mut self,
        bounds: Bounds<Pixels>,
        color: Hsla,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.focus.is_focused(window) && self.target.snapshot_for(self.target.token()).is_ok() {
            self.end_input(cx);
        }
        let mut bounds = bounds.inset(PADDING);
        bounds.size.width = bounds.size.width.max(px(1.));
        bounds.size.height = bounds.size.height.max(px(1.));
        self.content_bounds = Some(bounds);
        self.ensure_index(bounds, cx);
        let index = self.index.as_ref().unwrap();
        if let Some(position) = self.pending_recenter
            && let Some(row) = index.row_for_offset(self.target.buffer().point())
        {
            let available = (bounds.size.height - self.settings.line_height).max(px(0.));
            let offset = match position {
                RecenterPosition::Center => available / 2.,
                RecenterPosition::Top => px(0.),
                RecenterPosition::Bottom => available,
            };
            self.scroll.y = (self.settings.line_height * row.index as f32 - offset).max(px(0.));
            self.pending_recenter = None;
            self.reveal_caret = false;
        }
        if self.reveal_caret
            && let Some(row) = index.row_for_offset(self.display_snapshot().selected_range().end)
        {
            let top = self.settings.line_height * row.index as f32;
            self.scroll.y = self
                .scroll
                .y
                .min(top)
                .max((top + self.settings.line_height - bounds.size.height).max(px(0.)));
            self.reveal_caret = false;
        }
        if index.is_complete() {
            let last_row = self.settings.line_height * index.row_count().saturating_sub(1) as f32;
            self.scroll.y = self.scroll.y.min(last_row);
        }
        if self.color != Some(color) {
            self.cache.clear();
            self.color = Some(color);
            self.shape_work = None;
            self.shape_range = None;
        }
        let first =
            (f32::from(self.scroll.y) / f32::from(self.settings.line_height)).floor() as usize;
        let count = (f32::from(bounds.size.height) / f32::from(self.settings.line_height)).ceil()
            as usize
            + 1;
        let range = first..first.saturating_add(count);
        self.cache.retain(|row, _| range.contains(row));
        let rows = index.visible_rows(range.clone());
        let mut large = Vec::new();
        for row in &rows {
            if self.cache.contains_key(&row.index) {
                continue;
            }
            if row.row.content_len > SYNC_SHAPE_BYTES {
                large.push(row.clone());
            } else {
                let shaped = ShapedRow::shape(
                    row.clone(),
                    index,
                    self.metrics.as_mut().unwrap(),
                    window.text_system(),
                    color,
                );
                self.cache.insert(row.index, Arc::new(shaped));
            }
        }
        let key = index.key().clone();
        if self
            .shape_range
            .as_ref()
            .is_none_or(|pending| pending != &(key.clone(), range.clone()))
        {
            self.shape_work = None;
            self.shape_range = None;
            if !large.is_empty() {
                let index = index.clone();
                let system = cx.text_system().clone();
                let settings = self.settings.clone();
                let cancelled = Arc::new(AtomicBool::new(false));
                let worker_cancel = cancelled.clone();
                let worker = cx.background_executor().spawn(async move {
                    let shaper = WindowTextSystem::new(system.clone());
                    let mut metrics = FontMeasure::new(system, settings);
                    let mut result = Vec::new();
                    for row in large {
                        if worker_cancel.load(Ordering::Relaxed) {
                            return None;
                        }
                        result.push(Arc::new(ShapedRow::shape(
                            row,
                            &index,
                            &mut metrics,
                            &shaper,
                            color,
                        )));
                    }
                    Some(result)
                });
                self.shape_range = Some((key.clone(), range.clone()));
                let expected = (key.clone(), range.clone());
                let task = cx.spawn(async move |view, cx| {
                    if let Some(rows) = worker.await {
                        view.update(cx, |view, cx| {
                            if view.shape_range.as_ref() == Some(&expected)
                                && view.color == Some(color)
                            {
                                for row in rows {
                                    view.cache.insert(row.source.index, row);
                                }
                                view.shape_work = None;
                                cx.notify();
                            }
                        })
                        .ok();
                    }
                });
                self.shape_work = Some(BackgroundWork {
                    cancelled,
                    _task: task,
                });
            }
        }
        self.geometry = Some(ViewportLayout {
            key,
            bounds,
            scroll: self.scroll,
            requested_rows: rows.len(),
            rows: rows
                .into_iter()
                .filter_map(|row| self.cache.get(&row.index).cloned())
                .collect(),
        });
        if self
            .geometry
            .as_ref()
            .is_some_and(|layout| !layout.rows.is_empty())
        {
            self.retained_paint_layout = None;
        } else if let Some(retained) = &mut self.retained_paint_layout {
            retained.bounds = bounds;
        }
    }

    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.accepting_input {
            return;
        }
        if self.target.input().is_composing() {
            self.message = Some("Input method composition is active; finish it first".into());
            cx.notify();
            return;
        }
        let Some(offset) = self
            .viewport_layout()
            .and_then(|layout| layout.hit_test(event.position))
        else {
            return;
        };
        self.resume_input(cx);
        if let Err(error) = self
            .target
            .set_point_from_pointer(self.target.token(), offset)
        {
            self.message = Some(error.to_string().into());
            cx.notify();
            return;
        }
        self.invalidate_input_registration();
        self.pending_recenter = None;
        self.message = None;
        window.focus(&self.focus, cx);
        window.notify_text_selection_changed(&self.focus);
        cx.notify();
    }

    fn finish_command(
        &mut self,
        outcome: CommandOutcome,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(update) = outcome.update {
            self.queue_change(update.change, cx);
            window.notify_text_input_changed(&self.focus);
        } else if outcome.reveal_point {
            window.notify_text_selection_changed(&self.focus);
        }
        self.pending_recenter = None;
        match outcome.viewport {
            Some(ViewportRequest::ScrollToRow(row)) => {
                let y = self.settings.line_height * row as f32;
                self.scroll_by(point(px(0.), y - self.scroll.y), cx);
            }
            Some(ViewportRequest::Recenter(position)) => {
                self.pending_recenter = Some(position);
                self.reveal_caret = false;
            }
            None => self.reveal_caret |= outcome.reveal_point,
        }
        self.message = outcome.message.map(Into::into);
        cx.notify();
    }

    #[cfg(test)]
    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.route_key(
            &event.keystroke,
            event.prefer_character_input,
            KeyLayers::Edit(None),
            true,
            window,
            cx,
        );
    }

    /// Routes through an edit keymap with an optional outer layer, or an
    /// exclusive outer layer for non-editing queries. Higher-layer commands
    /// are only reported; their owner executes them.
    pub(crate) fn route_key(
        &mut self,
        stroke: &Keystroke,
        prefer_character_input: bool,
        layers: KeyLayers<'_>,
        require_focus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> KeyRoute {
        if !self.accepting_input || (require_focus && !self.focus.is_focused(window)) {
            return KeyRoute::Unhandled;
        }
        let display = self.display_snapshot().id();
        let viewport = motion::Viewport::for_view(
            self.index.as_ref(),
            self.content_bounds,
            self.scroll,
            &self.settings,
            display,
        );
        let before_point = self.target.buffer().point();
        let token = self.target.token();
        let result = match layers {
            KeyLayers::Edit(Some(outer)) => self.target.dispatch_keystroke(
                token,
                stroke,
                prefer_character_input,
                &[outer, &self.keymap],
                viewport
                    .as_ref()
                    .map(|viewport| viewport as &dyn CommandViewport),
            ),
            KeyLayers::Edit(None) => self.target.dispatch_keystroke(
                token,
                stroke,
                prefer_character_input,
                &[&self.keymap],
                viewport
                    .as_ref()
                    .map(|viewport| viewport as &dyn CommandViewport),
            ),
            KeyLayers::Exclusive(layer) => self.target.dispatch_keystroke(
                token,
                stroke,
                prefer_character_input,
                &[layer],
                None,
            ),
        };
        match result {
            Ok(route) => match route.disposition {
                InputDisposition::TextInput => return KeyRoute::Unhandled,
                InputDisposition::Handled => {
                    self.invalidate_input_registration();
                    self.finish_command(route.outcome, window, cx);
                }
                InputDisposition::Requested(command) => {
                    // The owner revokes input when it changes the target. An
                    // asynchronous request alone must not drop ongoing typing.
                    return KeyRoute::Requested(command);
                }
                InputDisposition::Ignored => return KeyRoute::Unhandled,
            },
            Err(error) => self.fail(error, before_point, window, cx),
        }
        KeyRoute::Handled
    }

    /// Runs a command the owner selected for this target (for example after a
    /// read session confirmed a command name). The target must already own a
    /// fresh input session.
    pub(crate) fn execute_command(
        &mut self,
        command: Command,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let display = self.display_snapshot().id();
        let viewport = motion::Viewport::for_view(
            self.index.as_ref(),
            self.content_bounds,
            self.scroll,
            &self.settings,
            display,
        );
        let before_point = self.target.buffer().point();
        let result = self.target.execute_with_viewport(
            self.target.token(),
            command,
            viewport
                .as_ref()
                .map(|viewport| viewport as &dyn CommandViewport),
        );
        match result {
            Ok(outcome) => {
                self.invalidate_input_registration();
                self.finish_command(outcome, window, cx);
            }
            Err(error) => self.fail(error, before_point, window, cx),
        }
    }

    /// Replaces the editable input area, used by the owner of a read session.
    pub(crate) fn replace_input(
        &mut self,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let buffer = self.target.buffer();
        let range = buffer.read_only_prefix()..ByteOffset(buffer.text().len());
        let token = self.target.token();
        match self.target.replace_text_range(token, range, text) {
            Ok(update) => self.finish_command(
                CommandOutcome {
                    update: Some(update),
                    reveal_point: true,
                    ..CommandOutcome::default()
                },
                window,
                cx,
            ),
            Err(error) => {
                let point = self.target.buffer().point();
                self.fail(error, point, window, cx);
            }
        }
    }

    fn fail(
        &mut self,
        error: RouteError,
        before_point: ByteOffset,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.target.input().is_composing() {
            self.invalidate_input_registration();
        }
        if self.target.buffer().point() != before_point {
            self.pending_recenter = None;
            self.reveal_caret = true;
            window.notify_text_selection_changed(&self.focus);
        }
        self.message = Some(error.to_string().into());
        cx.notify();
    }

    fn scroll_wheel(&mut self, event: &ScrollEvent, _: &mut Window, cx: &mut Context<Self>) {
        let delta = event.delta.pixel_delta(self.settings.line_height);
        self.scroll_by(point(-delta.x, -delta.y), cx);
        cx.stop_propagation();
    }
}

impl Focusable for BufferView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for BufferView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("buffer-view")
            .size_full()
            .overflow_hidden()
            .track_focus(&self.focus)
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_scroll(cx.listener(Self::scroll_wheel))
            .child(TextCanvas { view: cx.entity() })
    }
}

struct TextCanvas {
    view: Entity<BufferView>,
}

impl IntoElement for TextCanvas {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for TextCanvas {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let color = cx.theme().foreground;
        self.view
            .update(cx, |view, cx| view.prepare(bounds, color, window, cx));
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.view.update(cx, |view, cx| {
            let handler = view.bind_input(cx);
            if view.accepting_input {
                window.handle_input(&view.focus, handler, cx);
                view.refresh_input_geometry(window);
            }
        });
        let (layout, focus, caret, marked, region) = {
            let view = self.view.read(cx);
            let Some(layout) = view.paint_layout().cloned() else {
                return;
            };
            let focus = view.focus.is_focused(window);
            let snapshot = view.display_snapshot();
            let point = snapshot.selected_range().end;
            let caret = view
                .index
                .as_ref()
                .and_then(|index| layout.bounds_for_range(point..point, index));
            (
                layout,
                focus,
                caret,
                snapshot.marked_range(),
                view.region_bounds(),
            )
        };
        window.with_content_mask(
            Some(ContentMask {
                bounds: layout.bounds,
            }),
            |window| {
                for bounds in region {
                    window.paint_quad(fill(bounds, cx.theme().selection));
                }
                for row in &layout.rows {
                    let origin = layout.origin(row);
                    for part in &row.parts {
                        if let Some(line) = &part.line {
                            line.paint(
                                point(origin.x + part.x, origin.y),
                                layout.key.settings.line_height,
                                TextAlign::Left,
                                None,
                                window,
                                cx,
                            )
                            .unwrap();
                        }
                    }
                }
                if let Some(marked) = marked {
                    for row in &layout.rows {
                        let start = marked.start.max(row.source.start);
                        let end = marked
                            .end
                            .min(ByteOffset(row.source.start.0 + row.source.row.content_len));
                        if start < end
                            && let (Some(left), Some(right)) =
                                (row.x_for_offset(start), row.x_for_offset(end))
                        {
                            let origin = layout.origin(row);
                            window.paint_quad(fill(
                                Bounds::new(
                                    point(
                                        origin.x + left,
                                        origin.y + layout.key.settings.line_height - px(2.),
                                    ),
                                    size((right - left).max(px(1.)), px(1.)),
                                ),
                                cx.theme().foreground,
                            ));
                        }
                    }
                }
                if focus && let Some(caret) = caret {
                    window.paint_quad(fill(caret, cx.theme().foreground));
                }
            },
        );
    }
}

#[cfg(test)]
mod history_tests;
#[cfg(test)]
mod input_tests;
#[cfg(test)]
mod minibuffer_tests;
#[cfg(test)]
mod motion_tests;
#[cfg(test)]
mod region_tests;
#[cfg(test)]
mod routing_tests;

#[cfg(test)]
mod tests {
    use super::BufferView;
    use crate::app::AppShell;
    use crate::buffer::Buffer;
    use crate::files::{DecodedFile, FileFormat};
    use gpui_kit as gpui;
    use gpui_kit::{AppContext, Bounds, TestAppContext, font, point, px, size};
    use std::sync::{Arc, atomic::Ordering};

    fn buffer() -> Buffer {
        Buffer::new(
            None,
            DecodedFile {
                text: "x".repeat(1024 * 1024),
                format: FileFormat::default(),
            },
        )
    }

    #[gpui::test]
    fn obsolete_background_generation_is_cancelled_before_it_can_publish(cx: &mut TestAppContext) {
        let view = cx.update(|cx| {
            cx.new(|cx| {
                let mut view = BufferView::new(buffer(), cx);
                view.ensure_index(
                    Bounds::new(point(px(0.), px(0.)), size(px(400.), px(100.))),
                    cx,
                );
                assert!(!view.index.as_ref().unwrap().is_complete());
                let old = view.layout_work.as_ref().unwrap().cancelled.clone();
                view.set_font(font("monospace"), px(20.), px(30.), cx);
                assert!(old.load(Ordering::Relaxed));
                assert!(view.layout_work.is_none());
                view.ensure_index(
                    Bounds::new(point(px(0.), px(0.)), size(px(500.), px(100.))),
                    cx,
                );
                view
            })
        });
        cx.run_until_parked();
        view.read_with(cx, |view, _| {
            let index = view.index.as_ref().unwrap();
            assert!(index.is_complete());
            assert_eq!(index.key().settings, view.settings);
            assert_eq!(index.key().settings.font_size, px(20.));
            assert_eq!(index.key().settings.wrap_width, px(500.));
            assert!(view.layout_work.is_none());
        });
    }

    #[gpui::test]
    fn stable_forced_frames_reuse_visible_shaped_rows(cx: &mut TestAppContext) {
        cx.update(crate::app::init);
        let window = cx.open_window(size(px(400.), px(240.)), |window, cx| {
            AppShell::new(buffer(), window, cx)
        });
        cx.run_until_parked();
        cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();
        let editor = window
            .read_with(cx, |shell, _| shell.active_view().clone())
            .unwrap();
        let before = editor.read_with(cx, |view, _| view.geometry.as_ref().unwrap().rows.clone());
        cx.update_window(window.into(), |_, window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        })
        .unwrap();
        editor.read_with(cx, |view, _| {
            let after = &view.geometry.as_ref().unwrap().rows;
            assert!(!before.is_empty());
            assert_eq!(before.len(), after.len());
            for (before, after) in before.iter().zip(after) {
                assert!(Arc::ptr_eq(before, after));
            }
        });
    }

    #[gpui::test]
    fn dropping_a_target_cancels_its_pending_layout(cx: &mut TestAppContext) {
        let view = cx.update(|cx| {
            cx.new(|cx| {
                let mut view = BufferView::new(buffer(), cx);
                view.ensure_index(
                    Bounds::new(point(px(0.), px(0.)), size(px(400.), px(100.))),
                    cx,
                );
                view
            })
        });
        let cancelled = view.read_with(cx, |view, _| {
            view.layout_work.as_ref().unwrap().cancelled.clone()
        });
        let weak = view.downgrade();
        drop(view);
        cx.update(|_| ());
        cx.run_until_parked();
        assert!(weak.upgrade().is_none());
        assert!(cancelled.load(Ordering::Relaxed));
    }
}
