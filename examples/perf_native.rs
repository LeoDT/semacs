//! Opt-in native-font / platform-submission measurements over the production shell.
//! Platform submission is not GPU execution or compositor presentation.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::fs::OpenOptions;
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use gpui_kit::profiler::journal::{
    ForegroundJournalCollector, ForegroundJournalEntry, IntervalBoundary, PresentedFrame,
};
use gpui_kit::*;
use semacs::app::AppShell;
use semacs::buffer::Buffer;
use semacs::files::{self, DecodedFile, FileFormat};
use semacs::input::DisplayId;
use semacs::view::BufferView;

#[path = "support/samples.rs"]
mod samples;

#[path = "support/native_diagnostic.rs"]
mod diagnostic;
#[path = "support/native_m1.rs"]
mod m1;
#[path = "support/native_m2.rs"]
mod m2;
#[path = "support/native_split.rs"]
mod split;
#[path = "support/native_syntax.rs"]
mod syntax;
#[path = "support/m1_workload.rs"]
mod workload;

#[cfg(test)]
#[path = "support/native_tests.rs"]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct EditStamp {
    display: DisplayId,
    point: usize,
    mark: Option<usize>,
    mark_active: bool,
}

impl EditStamp {
    fn capture(view: &BufferView) -> Self {
        Self {
            display: view.display_snapshot().id(),
            point: view.buffer().point().0,
            mark: view.buffer().mark().map(|mark| mark.0),
            mark_active: view.buffer().mark_active(),
        }
    }
}

#[derive(Clone, Copy)]
struct PaintState {
    stamp: EditStamp,
    at: Instant,
    ready: bool,
    compatible: bool,
    bytes: usize,
    indexed: usize,
    rows: usize,
    visible: usize,
    scanned: usize,
    width: f32,
    height: f32,
    scroll: f32,
    marked: bool,
}

fn capture(view: &BufferView) -> PaintState {
    let display = view.display_snapshot();
    let index = view.wrap_index().expect("paint must have a wrap index");
    assert_eq!(index.key().display, display.id());
    let layout = view.paint_layout().expect("paint must have a body layout");
    PaintState {
        stamp: EditStamp::capture(view),
        at: Instant::now(),
        ready: view.layout_ready(),
        compatible: layout.key() == index.key(),
        bytes: display.text().len(),
        indexed: index.indexed_bytes(),
        rows: index.row_count(),
        visible: layout.visible_row_count(),
        scanned: index.scanned_bytes(),
        width: f32::from(layout.bounds().size.width),
        height: f32::from(layout.bounds().size.height),
        scroll: f32::from(layout.scroll().y),
        marked: display.marked_range().is_some(),
    }
}

fn collect_frames(
    collector: &mut ForegroundJournalCollector,
    paints: &RefCell<VecDeque<PaintState>>,
    window: &Window,
) -> Vec<(PresentedFrame, Option<PaintState>)> {
    collect_paints(collector, paints, window, |paint| paint.at)
}

fn collect_paints<T>(
    collector: &mut ForegroundJournalCollector,
    paints: &RefCell<VecDeque<T>>,
    window: &Window,
    at: impl Fn(&T) -> Instant,
) -> Vec<(PresentedFrame, Option<T>)> {
    let drained = collector.collect_unseen();
    assert_eq!(drained.lost, 0, "profiler overflow invalidates the run");
    let mut result = Vec::new();
    for entry in drained.entries {
        if let ForegroundJournalEntry::Boundary(IntervalBoundary::Presented(frame)) = entry
            && frame.frame.window_id == window.window_handle().window_id()
        {
            let paint = take_paint(&mut paints.borrow_mut(), &frame, &at);
            result.push((frame, paint));
        }
    }
    result
}

fn take_paint<T>(
    paints: &mut VecDeque<T>,
    frame: &PresentedFrame,
    at: impl Fn(&T) -> Instant,
) -> Option<T> {
    while paints
        .front()
        .is_some_and(|paint| at(paint) < frame.frame.draw_start)
    {
        paints.pop_front();
    }
    if paints
        .front()
        .is_some_and(|paint| at(paint) <= frame.frame.draw_end)
    {
        paints.pop_front()
    } else {
        None
    }
}

struct Probe {
    shell: Entity<AppShell>,
    paints: Rc<RefCell<VecDeque<PaintState>>>,
    _subscription: Subscription,
}

impl Render for Probe {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let editor = self.shell.read(cx).active_view().clone();
        let paints = self.paints.clone();
        div().size_full().child(self.shell.clone()).child(
            canvas(
                |_, _, _| (),
                move |_, _, _, cx| {
                    paints.borrow_mut().push_back(capture(editor.read(cx)));
                },
            )
            .absolute()
            .size_full(),
        )
    }
}

#[derive(Clone, Copy, Debug)]
enum Step {
    Open,
    Scroll,
    Resize,
    Font,
    Forced,
    Idle,
    Click,
    Insert,
    Delete,
    Newline,
    Join,
    BurstInsert,
    BurstDelete,
    Preedit,
    Cancel,
    Commit,
}

impl Step {
    fn name(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Scroll => "scroll",
            Self::Resize => "resize",
            Self::Font => "font",
            Self::Forced => "forced",
            Self::Idle => "idle",
            Self::Click => "click",
            Self::Insert => "insert",
            Self::Delete => "delete",
            Self::Newline => "newline",
            Self::Join => "join",
            Self::BurstInsert => "burst-insert",
            Self::BurstDelete => "burst-delete",
            Self::Preedit => "preedit",
            Self::Cancel => "cancel",
            Self::Commit => "commit",
        }
    }
}

fn display_actions() -> VecDeque<Step> {
    let mut actions = VecDeque::new();
    actions.extend(std::iter::repeat_n(Step::Scroll, 30));
    actions.extend([Step::Resize, Step::Font]);
    actions.extend(std::iter::repeat_n(Step::Forced, 30));
    actions.extend([
        Step::Idle,
        Step::Click,
        Step::Insert,
        Step::Delete,
        Step::Newline,
        Step::Join,
        Step::BurstInsert,
        Step::BurstDelete,
    ]);
    actions
}

struct Measurement {
    step: Step,
    start: Instant,
    expected: String,
    marked: bool,
    first_frame: Option<PresentedFrame>,
    first_paint: Option<PaintState>,
    first: Option<PresentedFrame>,
    ready: Option<PresentedFrame>,
    paint: Option<PaintState>,
    frames: usize,
    draw_ms: f64,
    submit_ms: f64,
    max_draw_ms: f64,
    idle_ticks: usize,
    blank_frames: usize,
    action_cpu_ms: f64,
    old_scroll: f32,
    old_width: f32,
}

impl Measurement {
    fn new(step: Step, expected: String, marked: bool) -> Self {
        Self {
            step,
            start: Instant::now(),
            expected,
            marked,
            first_frame: None,
            first_paint: None,
            first: None,
            ready: None,
            paint: None,
            frames: 0,
            draw_ms: 0.,
            submit_ms: 0.,
            max_draw_ms: 0.,
            idle_ticks: 0,
            blank_frames: 0,
            action_cpu_ms: 0.,
            old_scroll: 0.,
            old_width: 0.,
        }
    }

    fn accept(&mut self, frame: PresentedFrame, paint: Option<PaintState>) -> bool {
        if frame.frame.draw_start < self.start {
            return false;
        }
        self.frames += 1;
        if self.first_frame.is_none() {
            self.first_frame = Some(frame);
            self.first_paint = paint;
        }
        let draw_ms = millis(frame.frame.draw_duration());
        self.draw_ms += draw_ms;
        self.submit_ms += millis(frame.presentation.present_duration());
        self.max_draw_ms = self.max_draw_ms.max(draw_ms);
        let Some(paint) = paint else {
            return false;
        };
        self.blank_frames += usize::from(paint.visible == 0);
        if !paint.compatible
            || paint.bytes != self.expected.len()
            || paint.marked != self.marked
            || paint.visible == 0
        {
            return false;
        }
        if matches!(self.step, Step::Resize) && paint.width >= self.old_width - 1. {
            return false;
        }
        self.first.get_or_insert(frame);
        self.paint = Some(paint);
        if paint.ready && paint.indexed == paint.bytes {
            self.ready.get_or_insert(frame);
        }
        self.ready.is_some()
    }

    fn hard_error(&self, now: Instant, diagnostic: bool) -> Option<String> {
        let label = self.step.name();
        if self.blank_frames > 0 {
            return Some(format!("{label}: blank body frame"));
        }
        if matches!(self.step, Step::Open)
            && self.first_frame.is_some()
            && !self.first_paint.is_some_and(|paint| paint.visible > 0)
        {
            return Some("open: first frame has no body observation".into());
        }
        let timeout = Duration::from_secs(if diagnostic { 2 } else { 30 });
        if now.duration_since(self.start) > timeout {
            return Some(format!(
                "{label}: observation timeout ({} ms)",
                timeout.as_millis()
            ));
        }
        None
    }

    fn performance_errors(&self, now: Instant, hardware: bool, external: bool) -> Vec<String> {
        let label = self.step.name();
        // Submitted timestamps survive late observation; only missing boundaries use the clock.
        let elapsed = |frame: Option<PresentedFrame>| {
            millis(frame.map_or(now, |frame| frame.presentation.present_end) - self.start)
        };
        let mut errors = Vec::new();
        if matches!(self.step, Step::Idle) && self.frames > 0 {
            errors.push("idle: unexpected draw".into());
        }
        if hardware {
            let mut limits = vec![("draw_cpu_max_ms", self.max_draw_ms, 16.)];
            match self.step {
                Step::Open => limits.extend([
                    ("first_submit_ms", elapsed(self.first), 250.),
                    ("ready_submit_ms", elapsed(self.ready), 500.),
                ]),
                Step::Resize | Step::Font => limits.extend([
                    ("first_frame_submit_ms", elapsed(self.first_frame), 50.),
                    ("ready_submit_ms", elapsed(self.ready), 300.),
                ]),
                Step::Insert | Step::Delete | Step::Newline | Step::Join => limits.extend([
                    ("action_cpu_ms", self.action_cpu_ms, 2.),
                    ("first_submit_ms", elapsed(self.first), 16.),
                    ("ready_submit_ms", elapsed(self.ready), 300.),
                ]),
                Step::BurstInsert | Step::BurstDelete => limits.extend([
                    ("action_cpu_ms", self.action_cpu_ms, 50.),
                    ("first_submit_ms", elapsed(self.first), 50.),
                    ("ready_submit_ms", elapsed(self.ready), 300.),
                ]),
                Step::Scroll => limits.push(("first_submit_ms", elapsed(self.first), 4.)),
                Step::Forced => limits.push(("first_submit_ms", elapsed(self.first), 2.)),
                Step::Preedit | Step::Cancel | Step::Commit if external => limits.extend([
                    ("first_submit_ms", elapsed(self.first), 75.),
                    ("ready_submit_ms", elapsed(self.ready), 300.),
                ]),
                _ => {}
            }
            for (metric, value, limit) in limits {
                if value > limit {
                    errors.push(format!("{label}: {metric}={value:.6} ms > {limit:.3} ms"));
                }
            }
        }
        errors
    }

    fn budget_error(&self, now: Instant, hardware: bool, external: bool) -> Option<String> {
        self.hard_error(now, false).or_else(|| {
            self.performance_errors(now, hardware, external)
                .into_iter()
                .next()
        })
    }

    fn csv_row(&self, fallback: Option<PaintState>, external: bool, status: &str) -> String {
        let paint = self.paint.or(fallback);
        let cell = |value: fn(PaintState) -> String| paint.map_or_else(String::new, value);
        let first_ms = self.first.map_or(String::new(), |frame| {
            format!("{:.3}", millis(frame.presentation.present_end - self.start))
        });
        let ready_ms = self.ready.map_or(String::new(), |frame| {
            format!("{:.3}", millis(frame.presentation.present_end - self.start))
        });
        let first_frame_ms = self.first_frame.map_or(String::new(), |frame| {
            format!("{:.3}", millis(frame.presentation.present_end - self.start))
        });
        let first_scan = self
            .first_paint
            .map_or(String::new(), |paint| paint.scanned.to_string());
        let first_visible = self
            .first_paint
            .map_or(String::new(), |paint| paint.visible.to_string());
        format!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{:.3},{:.3},{:.3},{},{},{},{},{:.3},{:.3},{}",
            self.step.name(),
            cell(|p| p.bytes.to_string()),
            cell(|p| p.rows.to_string()),
            cell(|p| p.visible.to_string()),
            cell(|p| p.scanned.to_string()),
            self.frames,
            cell(|p| format!("{:.1}", p.width)),
            cell(|p| format!("{:.1}", p.height)),
            cell(|p| format!("{:.1}", p.scroll)),
            first_ms,
            ready_ms,
            if external && matches!(self.step, Step::Preedit | Step::Cancel | Step::Commit) {
                "arm-to-submit"
            } else {
                "action-to-submit"
            },
            self.draw_ms,
            self.submit_ms,
            self.max_draw_ms,
            first_frame_ms,
            first_scan,
            first_visible,
            self.blank_frames,
            self.action_cpu_ms,
            millis(self.start.elapsed()),
            status,
        )
    }
}

fn create_report(path: &std::path::Path, header: &str, diagnostic: bool) -> std::fs::File {
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .expect("create new report; refusing to overwrite");
    writeln!(output, "{header}").unwrap();
    if diagnostic {
        writeln!(
            output,
            "# mode=display-diagnostic; acceptance=false; stage_timeout_ms=2000"
        )
        .unwrap();
    }
    writeln!(output, "phase,bytes,wrap_rows,visible_rows,scan_bytes,frames,width_px,height_px,scroll_y,first_submit_ms,ready_submit_ms,boundary,draw_cpu_total_ms,submit_cpu_total_ms,draw_cpu_max_ms,first_frame_submit_ms,first_scan_bytes,first_visible_rows,blank_frames,action_cpu_ms,observed_elapsed_ms,status").unwrap();
    output.flush().unwrap();
    output
}

fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.
}

struct Run {
    editor: Entity<BufferView>,
    paints: Rc<RefCell<VecDeque<PaintState>>>,
    collector: ForegroundJournalCollector,
    actions: VecDeque<Step>,
    current: Option<Measurement>,
    output: std::fs::File,
    row_count: usize,
    budget_violations: usize,
    diagnostic: bool,
    report: PathBuf,
    commands: Option<mpsc::Receiver<String>>,
    external: bool,
    waiting_since: Instant,
    last_paint: Option<PaintState>,
    hardware: bool,
    succeeded: Rc<Cell<bool>>,
}

impl Run {
    fn write_row(&mut self, measurement: &Measurement, status: &str) {
        self.row_count += 1;
        writeln!(
            self.output,
            "{}",
            measurement.csv_row(self.last_paint, self.external, status)
        )
        .unwrap();
        self.output.flush().unwrap();
    }

    fn record(&mut self, measurement: &Measurement, cx: &App) -> Result<(), String> {
        let view = self.editor.read(cx);
        let check = |valid, reason| {
            if valid {
                Ok(())
            } else {
                Err(format!("{}: {reason}", measurement.step.name()))
            }
        };
        check(measurement.blank_frames == 0, "body must remain visible")?;
        check(
            view.display_snapshot().text().to_string() == measurement.expected,
            "display text mismatch",
        )?;
        check(
            view.display_snapshot().marked_range().is_some() == measurement.marked,
            "marked state mismatch",
        )?;
        if measurement.marked {
            check(
                !view.buffer().is_modified(),
                "ordinary preedit must remain clean",
            )?;
        }
        let paint = measurement
            .paint
            .or(self.last_paint)
            .ok_or_else(|| "body must have painted".to_string())?;
        check(
            paint.indexed == paint.bytes,
            "final index must cover the whole display",
        )?;
        if matches!(measurement.step, Step::Scroll) {
            check(
                paint.scroll > measurement.old_scroll,
                "scroll must actually move",
            )?;
        }
        if matches!(measurement.step, Step::Open) {
            check(
                measurement
                    .first_paint
                    .is_some_and(|paint| paint.visible > 0),
                "cold first frame must contain body text",
            )?;
        }
        if matches!(measurement.step, Step::Font) {
            check(
                view.wrap_index().unwrap().key().settings.font_size == px(18.),
                "font size mismatch",
            )?;
        }
        if matches!(
            measurement.step,
            Step::Insert
                | Step::Delete
                | Step::Newline
                | Step::Join
                | Step::BurstInsert
                | Step::BurstDelete
                | Step::Commit
        ) {
            check(
                view.buffer().is_modified(),
                "committed edit must mark buffer modified",
            )?;
        }
        if matches!(measurement.step, Step::Cancel) {
            check(
                !view.buffer().is_modified(),
                "cancel must leave buffer clean",
            )?;
        }
        let errors = measurement.performance_errors(Instant::now(), self.hardware, self.external);
        for reason in &errors {
            writeln!(
                self.output,
                "# budget_violation=row {}: {reason}",
                self.row_count + 1
            )
            .unwrap();
        }
        self.budget_violations += errors.len();
        self.write_row(
            measurement,
            if errors.is_empty() {
                "ok"
            } else {
                "over-budget"
            },
        );
        if self.diagnostic {
            println!("STAGE_DONE {}", measurement.step.name());
            std::io::stdout().flush().unwrap();
        }
        if self.external {
            println!("DONE {}", measurement.step.name());
            std::io::stdout().flush().unwrap();
        }
        Ok(())
    }

    fn begin(&mut self, step: Step, window: &mut Window, cx: &mut App) {
        let view = self.editor.read(cx);
        let point = view.buffer().point().0;
        let mut expected = view.buffer().text().to_string();
        let marked = matches!(step, Step::Preedit);
        match step {
            Step::Insert => expected.insert(point, 'x'),
            Step::Delete => {
                expected.remove(point - 1);
            }
            Step::Newline => expected.insert(point, '\n'),
            Step::Join => {
                assert_eq!(expected.remove(point - 1), '\n');
            }
            Step::BurstInsert => expected.insert_str(point, &"x".repeat(32)),
            Step::BurstDelete => {
                expected.replace_range(point - 32..point, "");
            }
            Step::Preedit => expected.insert_str(point, "ni"),
            Step::Commit => expected.insert(point, '你'),
            _ => {}
        }
        let mut measurement = Measurement::new(step, expected, marked);
        if let Some(paint) = self.last_paint {
            measurement.old_scroll = paint.scroll;
            measurement.old_width = paint.width;
        }
        if self.diagnostic {
            println!("STAGE {}", step.name());
            std::io::stdout().flush().unwrap();
        }
        // Expected text construction and previous-stage checks are outside the interval.
        measurement.start = Instant::now();
        match step {
            Step::Scroll => {
                let bounds = view.viewport_layout().unwrap().bounds();
                window.dispatch_event(
                    PlatformInput::Scroll(ScrollEvent {
                        position: point_fn(bounds.left() + px(8.), bounds.top() + px(8.)),
                        delta: ScrollDelta::Pixels(point_fn(px(0.), px(-120.))),
                        modifiers: Default::default(),
                        touch_phase: TouchPhase::Moved,
                        source: ScrollSource::Pointer,
                    }),
                    cx,
                );
            }
            Step::Resize => {
                let size = window.viewport_size();
                window.resize(size_fn((size.width * 0.75).max(px(320.)), size.height));
            }
            Step::Font => self.editor.update(cx, |view, cx| {
                view.set_font(
                    semacs::appearance::FontSettings::default().font().bold(),
                    px(18.),
                    px(27.),
                    cx,
                )
            }),
            Step::Forced => window.refresh(),
            Step::Click => {
                let bounds = view.viewport_layout().unwrap().bounds();
                let position = point_fn(bounds.left() + px(1.), bounds.top() + px(4.));
                window.dispatch_event(
                    PlatformInput::MouseDown(MouseDownEvent {
                        button: MouseButton::Left,
                        position,
                        modifiers: Default::default(),
                        click_count: 1,
                        first_mouse: false,
                    }),
                    cx,
                );
                window.dispatch_event(
                    PlatformInput::MouseUp(MouseUpEvent {
                        button: MouseButton::Left,
                        position,
                        modifiers: Default::default(),
                        click_count: 1,
                    }),
                    cx,
                );
            }
            Step::Insert
            | Step::Delete
            | Step::Newline
            | Step::Join
            | Step::BurstInsert
            | Step::BurstDelete => {
                let key = match step {
                    Step::Insert | Step::BurstInsert => "x",
                    Step::Newline => "enter",
                    _ => "backspace",
                };
                let count = if matches!(step, Step::BurstInsert | Step::BurstDelete) {
                    32
                } else {
                    1
                };
                for _ in 0..count {
                    window.dispatch_keystroke(Keystroke::parse(key).unwrap(), cx);
                }
            }
            _ => {}
        }
        measurement.action_cpu_ms = millis(measurement.start.elapsed());
        self.current = Some(measurement);
        if self.external {
            println!("ARMED {}", step.name());
            std::io::stdout().flush().unwrap();
        }
    }

    fn finish(&mut self, error: Option<String>, cx: &mut App) -> bool {
        if error.is_some()
            && let Some(measurement) = self.current.take()
        {
            self.write_row(&measurement, "failed");
        }
        if let Some(reason) = &error {
            writeln!(self.output, "# stop_reason={reason}").unwrap();
        }
        let success = error.is_none();
        let verdict = if !self.hardware {
            "not-applicable"
        } else if success && self.budget_violations == 0 {
            "true"
        } else {
            "false"
        };
        writeln!(self.output, "# completed={success}; budget_passed={verdict}; acceptance={}; budget_violation_count={}; gpu_execution_ms=unmeasured; compositor_presentation_ms=unmeasured",
            self.hardware && !self.diagnostic, self.budget_violations).unwrap();
        self.output.flush().unwrap();
        self.succeeded.set(error.is_none());
        if let Some(reason) = error {
            println!("STOP {reason}; report={}", self.report.display());
        } else if self.diagnostic {
            println!(
                "DIAGNOSTIC_COMPLETE {}; budget_violation_count={}; acceptance=false",
                self.report.display(),
                self.budget_violations
            );
        } else {
            println!("COMPLETED {}", self.report.display());
        }
        std::io::stdout().flush().unwrap();
        cx.quit();
        false
    }

    fn tick(&mut self, window: &mut Window, cx: &mut App) -> bool {
        let mut completed = self
            .current
            .as_ref()
            .is_some_and(|reading| reading.ready.is_some());
        for (frame, paint) in collect_frames(&mut self.collector, &self.paints, window) {
            if let Some(paint) = paint {
                self.last_paint = Some(paint);
            }
            if let Some(measurement) = &mut self.current {
                completed |= measurement.accept(frame, paint);
            }
        }
        if let Some(measurement) = &self.current
            && let Some(reason) = if self.diagnostic {
                measurement.hard_error(Instant::now(), true)
            } else {
                measurement.budget_error(Instant::now(), self.hardware, self.external)
            }
        {
            return self.finish(Some(reason), cx);
        }
        if let Some(measurement) = &mut self.current
            && matches!(measurement.step, Step::Idle)
        {
            measurement.idle_ticks += 1;
            completed = measurement.idle_ticks >= 30;
        }
        if completed {
            let measurement = self.current.take().unwrap();
            if let Err(reason) = self.record(&measurement, cx) {
                self.current = Some(measurement);
                return self.finish(Some(reason), cx);
            }
            self.waiting_since = Instant::now();
            if self.external {
                println!("READY");
                std::io::stdout().flush().unwrap();
            }
        }
        if self.current.is_none() {
            if let Some(step) = self.actions.pop_front() {
                self.begin(step, window, cx);
            } else if let Some(commands) = &self.commands {
                assert!(
                    self.waiting_since.elapsed() < Duration::from_secs(60),
                    "external driver timeout"
                );
                match commands.try_recv() {
                    Ok(command) => {
                        let step = match command.trim() {
                            "preedit" => Step::Preedit,
                            "cancel" => Step::Cancel,
                            "commit" => Step::Commit,
                            "scroll" => Step::Scroll,
                            "resize" => Step::Resize,
                            "font" => Step::Font,
                            "click" => Step::Click,
                            "done" => {
                                return self.finish(None, cx);
                            }
                            other => panic!("unknown external command: {other}"),
                        };
                        self.begin(step, window, cx);
                    }
                    Err(mpsc::TryRecvError::Empty) => {}
                    Err(mpsc::TryRecvError::Disconnected) => panic!("external driver disconnected"),
                }
            } else {
                return self.finish(None, cx);
            }
        }
        true
    }
}

use gpui_kit::{point as point_fn, size as size_fn};

fn schedule(run: Rc<RefCell<Run>>, window: &Window) {
    window.on_next_frame(move |window, cx| {
        let keep_running = run.borrow_mut().tick(window, cx);
        if keep_running {
            schedule(run, window);
        }
    });
}

fn main() {
    let mut args = std::env::args_os().skip(1);
    let input =
        PathBuf::from(args.next().expect(
            "expected MIXED_SAMPLE SAMPLE NEW_REPORT [--allow-software] [--display-diagnostic | --external | --m1 | --m2-directory NEW_DIRECTORY | --split right|below | --syntax]",
        ));
    let sample = args.next().expect("expected SAMPLE").into_string().unwrap();

    let report = PathBuf::from(args.next().expect("expected NEW_REPORT"));
    assert!(!report.exists(), "report must not already exist");
    let mut allow_software = false;
    let mut external = false;
    let mut m1_mode = false;
    let mut display_diagnostic = false;
    let mut syntax_mode = false;
    let mut split_direction = None;
    let mut m2_directory = None;
    let mut m2_open_diagnostic = false;
    let mut config = m1::Config::default();
    let mut pacing_options = false;
    while let Some(arg) = args.next() {
        match arg.to_str().unwrap() {
            "--allow-software" => allow_software = true,
            "--external" => external = true,
            "--m1" => m1_mode = true,
            "--display-diagnostic" => display_diagnostic = true,
            "--syntax" => syntax_mode = true,
            "--split" => {
                let direction = args.next().expect("split direction: right or below");
                let direction = direction.into_string().unwrap();
                assert!(matches!(direction.as_str(), "right" | "below"));
                split_direction = Some(direction);
            }
            "--m2-open-diagnostic" => m2_open_diagnostic = true,
            "--m2-directory" => {
                m2_directory = Some(PathBuf::from(args.next().expect("new M2 work directory")));
            }
            "--duration-seconds" => {
                config.seconds = args
                    .next()
                    .expect("duration value")
                    .to_str()
                    .unwrap()
                    .parse()
                    .expect("integer duration");
                pacing_options = true;
            }
            "--interval-ms" => {
                config.interval_ms = args
                    .next()
                    .expect("interval value")
                    .to_str()
                    .unwrap()
                    .parse()
                    .expect("integer interval");
                pacing_options = true;
            }
            other => panic!("unknown argument: {other}"),
        }
    }
    assert!(
        usize::from(external)
            + usize::from(m1_mode)
            + usize::from(m2_directory.is_some())
            + usize::from(split_direction.is_some())
            + usize::from(syntax_mode)
            <= 1,
        "external, M1, M2, split and syntax are separate measurements"
    );
    assert!(
        !display_diagnostic
            || (!external
                && !m1_mode
                && !syntax_mode
                && m2_directory.is_none()
                && split_direction.is_none()
                && sample == "mixed"),
        "display diagnostic requires the ordinary mixed workload"
    );
    assert!(!pacing_options || m1_mode, "pacing options require --m1");
    assert!(
        !m2_open_diagnostic || m2_directory.is_some(),
        "open diagnostic requires --m2-directory"
    );
    config.validate().expect("valid M1 settings");
    let mixed = files::read(&input)
        .expect("read deterministic mixed sample")
        .text;
    if syntax_mode {
        syntax::run(input, sample, mixed, report, allow_software);
        return;
    }
    assert!(samples::NAMES.contains(&sample.as_str()));
    let text = samples::build(&sample, &mixed);
    if let Some(direction) = split_direction {
        split::run(text, sample, report, direction, allow_software);
        return;
    }
    let m2_directory = m2_directory.map(|directory| {
        m2::prepare(&directory, &text);
        directory
            .canonicalize()
            .expect("absolute M2 work directory")
    });
    let model_start = Instant::now();
    let buffer = if m2_directory.is_some() {
        Buffer::scratch()
    } else {
        Buffer::new(
            None,
            DecodedFile {
                text: text.clone(),
                format: FileFormat::default(),
            },
        )
    };
    let model_ms = millis(model_start.elapsed());
    let commands = external.then(|| {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for line in std::io::stdin().lock().lines() {
                if tx.send(line.expect("read external command")).is_err() {
                    break;
                }
            }
        });
        rx
    });
    let succeeded = Rc::new(Cell::new(false));
    let outcome = succeeded.clone();
    gpui_kit::application().run(move |cx| {
        semacs::app::init(cx);
        profiler::set_trace_enabled(true);
        let collector = cx.foreground_journal().collector();
        let paints = Rc::new(RefCell::new(VecDeque::new()));
        let probe_paints = paints.clone();
        if display_diagnostic {
            println!("STAGE open");
            std::io::stdout().flush().unwrap();
        }
        let open = Measurement::new(Step::Open, text, false);
        let (window, probe) = gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    Point::default(), size(px(960.), px(640.)),
                ))),
                app_id: Some(format!("semacs-perf-native-{}", std::env::var("SEMACS_PERF_TOKEN").unwrap_or_default())),
                titlebar: Some(TitlebarOptions { title: Some("semacs native performance".into()), ..Default::default() }),
                ..Default::default()
            },
            cx,
            move |window, cx| cx.new(|cx| {
                let shell = cx.new(|cx| AppShell::new(buffer, window, cx));
                let subscription = cx.observe(&shell, |_, _, cx| cx.notify());
                Probe { shell, paints: probe_paints, _subscription: subscription }
            }),
        ).expect("open native performance window");
        let editor = probe.read(cx).shell.read(cx).active_view().clone();
        window.update(cx, move |_, window, cx| {
            let specs = window.gpu_specs().expect("renderer must identify itself");
            assert!(allow_software || !specs.is_software_emulated,
                "hardware GPU required; --allow-software is diagnostic only");
            let header = format!("# sample={sample}; model_ms={model_ms:.3}; external={external}; graphics={specs:?}\n# release={}; native {}; soft wrap enabled\n# first/ready end at CPU platform submission, NOT screen presentation; external includes driver/IPC/IME overhead",
                !cfg!(debug_assertions), if m1_mode || m2_directory.is_some() { "Lilex 16px/24px, fixed dimensions" } else { "Lilex 16px/24px -> bold 18px/27px" });
            if let Some(directory) = m2_directory {
                m2::start(m2::Session {
                    shell: probe.read(cx).shell.clone(), paints, collector,
                    directory, original: open.expected, report, header, open_diagnostic: m2_open_diagnostic,
                    succeeded: outcome, hardware: !specs.is_software_emulated,
                }, window, cx);
                return;
            }
            if m1_mode {
                m1::start(config, m1::Session {
                    editor, paints, collector, original: open.expected, opened_at: open.start,
                    report, header, succeeded: outcome, hardware: !specs.is_software_emulated,
                }, window, cx);
                return;
            }
            let actions = if external { VecDeque::new() } else { display_actions() };
            let run = Run {
                editor, paints, collector, actions, current: Some(open),
                output: create_report(&report, &header, display_diagnostic),
                row_count: 0, budget_violations: 0, diagnostic: display_diagnostic,
                report, commands, external, waiting_since: Instant::now(), last_paint: None,
                hardware: !specs.is_software_emulated, succeeded: outcome,
            };
            schedule(Rc::new(RefCell::new(run)), window);
        }).unwrap();
    });
    if !succeeded.get() {
        std::process::exit(2);
    }
}
