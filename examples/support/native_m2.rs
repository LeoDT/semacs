use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui_kit::profiler::journal::ForegroundJournalCollector;
use gpui_kit::{App, Entity, Keystroke, WeakEntity, Window};
use semacs::app::AppShell;
use semacs::buffer::BufferId;
use semacs::session::ReadKind;
use semacs::view::BufferView;

use super::{EditStamp, PaintState, collect_frames, millis};

const ENTRIES: usize = 2000;
const CYCLES: usize = 3;

pub fn prepare(directory: &Path, text: &str) {
    fs::create_dir(directory).expect("M2 work directory must be new");
    fs::write(directory.join("original.txt"), text).unwrap();
    for index in 0..3 {
        fs::write(directory.join(format!("input-{index}.txt")), text).unwrap();
    }
    let completion = directory.join("completion");
    fs::create_dir(&completion).unwrap();
    for index in 0..ENTRIES {
        File::create_new(completion.join(format!("entry-{index:04}.txt"))).unwrap();
    }
}

pub struct Session {
    pub shell: Entity<AppShell>,
    pub paints: Rc<RefCell<VecDeque<PaintState>>>,
    pub collector: ForegroundJournalCollector,
    pub directory: PathBuf,
    pub original: String,
    pub report: PathBuf,
    pub header: String,
    pub succeeded: Rc<Cell<bool>>,
    pub hardware: bool,
    pub open_diagnostic: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Boot,
    Open(usize),
    Switch(usize),
    Insert,
    SaveEdit,
    Save,
    Complete,
    CancelCompletion,
    Kill,
}

impl Step {
    fn name(self) -> &'static str {
        match self {
            Self::Boot => "boot",
            Self::Open(_) => "open",
            Self::Switch(_) => "switch",
            Self::Insert => "insert",
            Self::SaveEdit => "save-edit",
            Self::Save => "save",
            Self::Complete => "complete",
            Self::CancelCompletion => "cancel-completion",
            Self::Kill => "kill",
        }
    }

    fn needs_prompt(self) -> bool {
        matches!(
            self,
            Self::Open(_) | Self::Switch(_) | Self::Complete | Self::CancelCompletion | Self::Kill
        )
    }

    fn input_budget(self) -> bool {
        matches!(self, Self::Insert | Self::SaveEdit)
    }

    fn body_submission(self) -> bool {
        !matches!(self, Self::Complete | Self::CancelCompletion | Self::Save)
    }
}

fn plan(open_diagnostic: bool) -> VecDeque<Step> {
    if open_diagnostic {
        return VecDeque::from([Step::Boot, Step::Open(0)]);
    }
    let mut steps = VecDeque::from([
        Step::Boot,
        Step::Open(0),
        Step::Open(1),
        Step::Open(2),
        Step::Switch(0),
        Step::Insert,
        Step::SaveEdit,
        Step::Save,
        Step::Switch(1),
        Step::Switch(2),
        Step::Switch(0),
        Step::Switch(1),
        Step::Switch(2),
        Step::Switch(0),
        Step::Complete,
        Step::CancelCompletion,
        Step::Kill,
        Step::Kill,
        Step::Kill,
    ]);
    for _ in 0..CYCLES {
        steps.extend([Step::Open(0), Step::Insert, Step::Save, Step::Kill]);
    }
    steps
}

fn key(window: &mut Window, cx: &mut App, key: &'static str) {
    window.dispatch_keystroke(Keystroke::parse(key).unwrap(), cx);
}

fn text(window: &mut Window, cx: &mut App, text: &str) {
    for ch in text.chars() {
        let value = ch.to_string();
        window.dispatch_keystroke(
            Keystroke {
                key: value.clone(),
                key_char: Some(value),
                modifiers: Default::default(),
            },
            cx,
        );
    }
}

#[derive(Default)]
struct Reading {
    start: Option<Instant>,
    stamp: Option<EditStamp>,
    bytes: usize,
    action_ms: f64,
    save_action_ms: Option<f64>,
    save_start: Option<Instant>,
    save_observed_ms: Option<f64>,
    save_ack_before_edit: Option<bool>,
    first_ms: Option<f64>,
    ready_ms: Option<f64>,
    result_ms: Option<f64>,
    frames: usize,
    blank: usize,
    draw_max: f64,
    width: f32,
    height: f32,
}

impl Reading {
    fn accept(
        &mut self,
        paint: Option<PaintState>,
        frame: gpui_kit::profiler::journal::PresentedFrame,
    ) {
        let start = self.start.unwrap();
        if frame.frame.draw_start < start {
            return;
        }
        self.frames += 1;
        self.draw_max = self.draw_max.max(millis(frame.frame.draw_duration()));
        let Some(paint) = paint else { return };
        self.blank += usize::from(paint.visible == 0);
        self.width = paint.width;
        self.height = paint.height;
        if Some(paint.stamp) != self.stamp
            || paint.bytes != self.bytes
            || !paint.compatible
            || paint.visible == 0
            || paint.marked
        {
            return;
        }
        let elapsed = millis(frame.presentation.present_end - start);
        self.first_ms.get_or_insert(elapsed);
        if paint.ready && paint.indexed == self.bytes {
            self.ready_ms.get_or_insert(elapsed);
        }
    }

    fn budget_error(&self, step: Step, hardware: bool) -> Option<String> {
        if self.blank > 0 {
            return Some("blank body frame".into());
        }
        if self.start.unwrap().elapsed() > Duration::from_secs(30) {
            return Some("observation timeout (not a performance budget)".into());
        }
        if !hardware {
            return None;
        }
        let mut checks = vec![("draw_cpu_max_ms", self.draw_max, 16.)];
        if step.input_budget() {
            checks.extend([
                ("action_cpu_ms", self.action_ms, 2.),
                (
                    "first_submit_ms",
                    self.first_ms
                        .unwrap_or_else(|| millis(self.start.unwrap().elapsed())),
                    16.,
                ),
                (
                    "ready_submit_ms",
                    self.ready_ms
                        .unwrap_or_else(|| millis(self.start.unwrap().elapsed())),
                    300.,
                ),
            ]);
        }
        checks.into_iter().find_map(|(metric, value, limit)| {
            (value > limit).then(|| format!("{}: {metric}={value:.3} > {limit:.3}", step.name()))
        })
    }
}

#[derive(Default)]
struct BufferCounts {
    scratch: usize,
    files: usize,
    messages: usize,
}

struct Lifecycle {
    scratch: BufferId,
    files: [Option<BufferId>; 3],
    messages: Option<BufferId>,
    messages_required: bool,
}

impl Lifecycle {
    fn new(scratch: BufferId) -> Self {
        Self {
            scratch,
            files: [None; 3],
            messages: None,
            messages_required: false,
        }
    }

    fn opened(&mut self, index: usize, id: BufferId) -> Result<(), String> {
        if self.files[index].is_some_and(|previous| previous != id) {
            return Err("open replaced a live fixture buffer identity".into());
        }
        self.files[index] = Some(id);
        Ok(())
    }

    fn killed(&mut self, index: usize, id: BufferId) -> Result<(), String> {
        if self.files[index] != Some(id) {
            return Err("kill target is not the expected live fixture".into());
        }
        self.files[index] = None;
        Ok(())
    }

    fn counts(&self, shell: &AppShell) -> BufferCounts {
        let mut counts = BufferCounts::default();
        for buffer in shell.buffers() {
            let id = buffer.snapshot_id().buffer;
            if id == self.scratch {
                counts.scratch += 1;
            } else if self.files.contains(&Some(id)) {
                counts.files += 1;
            } else if buffer.is_message_log() {
                counts.messages += 1;
            }
        }
        counts
    }

    fn verify(&mut self, shell: &AppShell, directory: &Path) -> Result<(), String> {
        let mut scratch = 0;
        let mut files = [false; 3];
        let mut messages = 0;
        for buffer in shell.buffers() {
            let id = buffer.snapshot_id().buffer;
            if id == self.scratch {
                if buffer.path().is_some()
                    || buffer.display_name() != "*scratch*"
                    || buffer.is_read_only()
                    || buffer.is_modified()
                    || !buffer.text().is_empty()
                {
                    return Err("original scratch identity or state changed".into());
                }
                scratch += 1;
            } else if let Some(index) = self.files.iter().position(|known| *known == Some(id)) {
                if buffer.path().as_deref()
                    != Some(directory.join(format!("input-{index}.txt")).as_path())
                {
                    return Err("fixture path does not match its buffer identity".into());
                }
                if files[index] {
                    return Err("duplicate fixture buffer identity".into());
                }
                files[index] = true;
            } else if buffer.is_message_log() {
                if !self.messages_required
                    || self.messages.is_some_and(|known| known != id)
                    || messages != 0
                    || buffer.display_name() != "*Messages*"
                    || buffer.path().is_some()
                    || !buffer.is_read_only()
                    || buffer.is_modified()
                {
                    return Err("unexpected message history identity or state".into());
                }
                self.messages = Some(id);
                messages += 1;
            } else {
                return Err(format!(
                    "unexpected retained buffer: id={id:?}, name={:?}, path={:?}",
                    buffer.display_name(),
                    buffer.path()
                ));
            }
        }
        if scratch != 1
            || files != self.files.map(|id| id.is_some())
            || messages != usize::from(self.messages_required)
        {
            return Err("missing expected scratch, fixture or message history buffer".into());
        }
        Ok(())
    }

    fn verify_finished(&self, open_diagnostic: bool) -> Result<(), String> {
        let files = self.files.map(|id| id.is_some());
        if files != [open_diagnostic, false, false] || self.messages_required == open_diagnostic {
            return Err("lifecycle did not finish with the expected scratch / fixture / message history set".into());
        }
        Ok(())
    }
}

struct Run {
    session: Session,
    output: File,
    trace: Option<crate::diagnostic::Trace>,
    steps: VecDeque<Step>,
    step: Step,
    index: usize,
    prompt_typed: bool,
    prepared_at: Instant,
    reading: Reading,
    expected: [String; 3],
    expected_points: [usize; 3],
    disk_expected: Option<String>,
    retired: Vec<WeakEntity<BufferView>>,
    lifecycle: Lifecycle,
    settle_ticks: usize,
}

impl Run {
    fn path(&self, index: usize) -> PathBuf {
        self.session.directory.join(format!("input-{index}.txt"))
    }

    fn active(&self, cx: &App) -> Entity<BufferView> {
        self.session.shell.read(cx).active_view().clone()
    }

    fn active_file(&self, cx: &App) -> Option<usize> {
        let view = self.active(cx);
        let path = view.read(cx).buffer().path();
        (0..3).find(|&index| path.as_deref() == Some(self.path(index).as_path()))
    }

    fn minibuffer_ready(&self, cx: &App) -> bool {
        let shell = self.session.shell.read(cx);
        shell
            .minibuffer()
            .is_some_and(|mini| mini.read(cx).editor().read(cx).layout_ready())
            && shell
                .session()
                .and_then(|session| session.candidates())
                .is_none_or(|list| !list.loading)
    }

    fn setup(&mut self, window: &mut Window, cx: &mut App) {
        self.prompt_typed = false;
        self.prepared_at = Instant::now();
        self.reading = Reading::default();
        match self.step {
            Step::Open(_) | Step::Complete | Step::CancelCompletion => {
                key(window, cx, "ctrl-x");
                key(window, cx, "ctrl-f");
            }
            Step::Switch(_) => {
                key(window, cx, "ctrl-x");
                key(window, cx, "b");
            }
            Step::Kill => {
                self.retired.push(self.active(cx).downgrade());
                key(window, cx, "ctrl-x");
                key(window, cx, "k");
            }
            _ => {}
        }
    }

    fn begin(&mut self, window: &mut Window, cx: &mut App) -> Result<(), String> {
        let file = self.active_file(cx);
        self.disk_expected = if matches!(self.step, Step::Save | Step::SaveEdit) {
            Some(self.expected[file.ok_or("save target is not a fixture")?].clone())
        } else {
            None
        };
        if self.step == Step::Kill {
            let id = self.active(cx).read(cx).buffer().snapshot_id().buffer;
            self.lifecycle
                .killed(file.ok_or("kill target is not a fixture")?, id)?;
        }
        let mut reading = std::mem::take(&mut self.reading);
        let start = Instant::now();
        match self.step {
            Step::Boot => window.refresh(),
            Step::Open(_) | Step::Switch(_) | Step::Kill => key(window, cx, "enter"),
            Step::Insert => key(window, cx, "x"),
            Step::SaveEdit => key(window, cx, "y"),
            Step::Save => {
                reading.save_start = Some(start);
                key(window, cx, "ctrl-x");
                key(window, cx, "ctrl-s");
            }
            Step::Complete => key(window, cx, "tab"),
            Step::CancelCompletion => {
                key(window, cx, "tab");
                text(window, cx, "z");
                key(window, cx, "ctrl-g");
                key(window, cx, "alt-x");
            }
        }
        reading.action_ms = millis(start.elapsed());
        reading.start = Some(start);
        let active = self.active(cx);
        let view = active.read(cx);
        reading.stamp = if matches!(self.step, Step::Open(_)) {
            None
        } else {
            Some(EditStamp::capture(view))
        };
        reading.bytes = view.buffer().text().len();
        if let Some(layout) = view.paint_layout() {
            reading.width = f32::from(layout.bounds().size.width);
            reading.height = f32::from(layout.bounds().size.height);
        }
        self.reading = reading;
        if matches!(self.step, Step::Insert | Step::SaveEdit) {
            let index = file.ok_or("input target is not a fixture")?;
            let ch = if self.step == Step::Insert { 'x' } else { 'y' };
            let point = view.buffer().point().0;
            if point != self.expected_points[index] + 1
                || view.buffer().text().len() != self.expected[index].len() + 1
            {
                return Err("dropped input or incorrect target".into());
            }
            self.expected[index].insert(self.expected_points[index], ch);
            self.expected_points[index] = point;
        }
        Ok(())
    }

    fn observe_result(&mut self, cx: &App) -> Result<bool, String> {
        let active = self.active(cx);
        let view = active.read(cx);
        match self.step {
            Step::Open(index) => {
                if self.active_file(cx) != Some(index) {
                    if view.message().is_some_and(|message| {
                        message.contains("error") || message.contains("denied")
                    }) {
                        return Err(format!("open failed: {:?}", view.message()));
                    }
                    return Ok(false);
                }
                self.lifecycle
                    .opened(index, view.buffer().snapshot_id().buffer)?;
                self.expected_points[index] = 0;
                self.reading.stamp = Some(EditStamp::capture(view));
                self.reading.bytes = view.buffer().text().len();
            }
            Step::Save | Step::SaveEdit => {
                if view
                    .message()
                    .is_some_and(|message| message.starts_with("Save failed:"))
                {
                    return Err(view.message().unwrap().into());
                }
                let done = if self.step == Step::Save {
                    !view.buffer().is_modified()
                } else {
                    self.reading.save_observed_ms.is_some()
                        || view
                            .message()
                            .is_some_and(|message| message.starts_with("Wrote "))
                };
                if done && self.reading.save_observed_ms.is_none() {
                    self.reading.save_observed_ms =
                        Some(millis(self.reading.save_start.unwrap().elapsed()));
                }
                if !done {
                    return Ok(false);
                }
                if self.step == Step::Save {
                    return Ok(true);
                }
            }
            Step::Complete => {
                let Some(mini) = self.session.shell.read(cx).minibuffer() else {
                    return Err("completion lost its read session".into());
                };
                if !mini.read(cx).editor().read(cx).layout_ready()
                    || !self
                        .session
                        .shell
                        .read(cx)
                        .session()
                        .and_then(|session| session.candidates())
                        .is_some_and(|list| {
                            !list.loading
                                && list.selected_candidate().is_some_and(|candidate| {
                                    candidate.value.ends_with("/entry-0000.txt")
                                })
                        })
                {
                    return Ok(false);
                }
                self.reading.result_ms = Some(millis(self.reading.start.unwrap().elapsed()));
                return Ok(true);
            }
            Step::CancelCompletion => {
                let shell = self.session.shell.read(cx);
                if !shell
                    .session()
                    .is_some_and(|session| matches!(session.kind(), ReadKind::Command))
                    || !self.minibuffer_ready(cx)
                {
                    return Ok(false);
                }
                if !shell.minibuffer().unwrap().read(cx).contents(cx).is_empty() {
                    return Err("old completion contaminated the replacement session".into());
                }
                self.reading.result_ms = Some(millis(self.reading.start.unwrap().elapsed()));
                return Ok(true);
            }
            _ => {}
        }
        Ok(self.reading.ready_ms.is_some())
    }

    fn verify(&mut self, cx: &App) -> Result<(), String> {
        let active = self.active(cx);
        let view = active.read(cx);
        if let Some(index) = self.active_file(cx)
            && (view.buffer().text().to_string() != self.expected[index]
                || view.buffer().point().0 != self.expected_points[index])
        {
            return Err("fixture text mismatch".into());
        }
        if let Some(expected) = &self.disk_expected {
            let index = self.active_file(cx).ok_or("saved fixture not active")?;
            if fs::read(self.path(index)).map_err(|e| e.to_string())? != expected.as_bytes() {
                return Err("saved bytes do not match the requested snapshot".into());
            }
            if self.step == Step::SaveEdit && !view.buffer().is_modified() {
                return Err("concurrent edit incorrectly marked clean".into());
            }
        }
        if let Step::Switch(index) = self.step
            && self.active_file(cx) != Some(index)
        {
            return Err("buffer switch did not select its target".into());
        }
        if matches!(self.step, Step::Save | Step::SaveEdit) {
            self.lifecycle.messages_required = true;
        }
        self.lifecycle
            .verify(self.session.shell.read(cx), &self.session.directory)
    }

    fn write_row(&mut self, status: &str, cx: &App) {
        let r = &self.reading;
        let cell =
            |value: Option<f64>| value.map_or_else(String::new, |value| format!("{value:.3}"));
        let rss = fs::read_to_string("/proc/self/status")
            .ok()
            .and_then(|text| {
                text.lines().find_map(|line| {
                    line.strip_prefix("VmRSS:")?
                        .split_whitespace()
                        .next()?
                        .parse::<usize>()
                        .ok()
                })
            });
        let counts = self.lifecycle.counts(self.session.shell.read(cx));
        writeln!(
            self.output,
            "{},{},{},{:.3},{},{},{},{},{},{},{:.3},{},{},{:.1},{:.1},{},{},{},{},{},{},{}",
            self.index,
            self.step.name(),
            status,
            r.action_ms,
            cell(r.save_action_ms),
            r.save_ack_before_edit
                .map_or_else(String::new, |value| value.to_string()),
            cell(r.first_ms.filter(|_| self.step.body_submission())),
            cell(r.ready_ms.filter(|_| self.step.body_submission())),
            cell(r.save_observed_ms),
            cell(r.result_ms),
            r.draw_max,
            r.frames,
            r.blank,
            r.width,
            r.height,
            self.session.shell.read(cx).buffer_count(),
            counts.scratch,
            counts.files,
            counts.messages,
            self.retired
                .iter()
                .filter(|view| view.upgrade().is_some())
                .count(),
            rss.map_or_else(String::new, |value| value.to_string()),
            cell(r.start.map(|start| millis(start.elapsed()))),
        )
        .unwrap();
        self.output.flush().unwrap();
    }

    fn finish(&mut self, error: Option<String>, cx: &mut App) -> bool {
        let success = error.is_none();
        if let Some(error) = error {
            if self.reading.start.is_some() {
                self.write_row("failed", cx);
            }
            writeln!(self.output, "# stop_reason={error}").unwrap();
            println!("STOP {error}; report={}", self.session.report.display());
        }
        writeln!(self.output, "# completed={success}; existing_budget_passed={}; new_m2_latency_budgets=undefined; gpu_execution_ms=unmeasured; compositor_presentation_ms=unmeasured",
            if !self.session.hardware { "not-applicable" } else if success { "true" } else { "false" }).unwrap();
        self.output.flush().unwrap();
        if let Some(trace) = &mut self.trace {
            trace.finish(success);
        }
        self.session.succeeded.set(success);
        cx.quit();
        false
    }

    fn advance(&mut self, window: &mut Window, cx: &mut App) -> Result<bool, String> {
        if let Some(trace) = &mut self.trace {
            trace.poll()?;
        }
        if self.reading.start.is_none() {
            for (frame, paint) in
                collect_frames(&mut self.session.collector, &self.session.paints, window)
            {
                if let Some(trace) = &mut self.trace {
                    trace.record(self.index, self.step.name(), None, None, frame, paint);
                }
            }
            if self.prepared_at.elapsed() > Duration::from_secs(30) {
                return Err("prompt setup timeout".into());
            }
            if self.step.needs_prompt() {
                if !self.minibuffer_ready(cx) {
                    return Ok(true);
                }
                if !self.prompt_typed {
                    let input = match self.step {
                        Step::Open(index) => self.path(index).to_string_lossy().into_owned(),
                        Step::Switch(index) => format!("input-{index}.txt"),
                        Step::Complete | Step::CancelCompletion => format!(
                            "{}/entry-",
                            self.session.directory.join("completion").display()
                        ),
                        _ => String::new(),
                    };
                    text(window, cx, &input);
                    self.prompt_typed = true;
                    return Ok(true);
                }
            }
            if self.step == Step::SaveEdit {
                if self.reading.save_start.is_none() {
                    let start = Instant::now();
                    key(window, cx, "ctrl-x");
                    key(window, cx, "ctrl-s");
                    self.reading.save_action_ms = Some(millis(start.elapsed()));
                    self.reading.save_start = Some(start);
                    return Ok(true);
                }
                let acknowledged = self
                    .active(cx)
                    .read(cx)
                    .message()
                    .is_some_and(|message| message.starts_with("Wrote "));
                self.reading.save_ack_before_edit = Some(acknowledged);
                if acknowledged {
                    self.reading.save_observed_ms =
                        Some(millis(self.reading.save_start.unwrap().elapsed()));
                }
            }
            self.begin(window, cx)?;
            return Ok(true);
        }

        // Resolve an asynchronously activated view before matching submitted paints.
        let mut done = self.observe_result(cx)?;
        for (frame, paint) in
            collect_frames(&mut self.session.collector, &self.session.paints, window)
        {
            if let Some(trace) = &mut self.trace {
                trace.record(
                    self.index,
                    self.step.name(),
                    self.reading.start,
                    self.reading.stamp,
                    frame,
                    paint,
                );
            }
            self.reading.accept(paint, frame);
        }
        if let Some(error) = self.reading.budget_error(self.step, self.session.hardware) {
            return Err(error);
        }
        if self.step.body_submission() {
            done = self.observe_result(cx)?;
        }
        if !done {
            return Ok(true);
        }
        self.verify(cx)?;
        if self.step == Step::Kill && self.retired.iter().any(|view| view.upgrade().is_some()) {
            self.settle_ticks += 1;
            if self.settle_ticks < 10 {
                return Ok(true);
            }
            return Err("closed buffer view still retained after ten frame-demand ticks".into());
        }
        self.write_row("complete", cx);
        if matches!(self.step, Step::Complete | Step::CancelCompletion) {
            key(window, cx, "ctrl-g");
        }
        let Some(step) = self.steps.pop_front() else {
            self.lifecycle
                .verify_finished(self.session.open_diagnostic)?;
            return Ok(false);
        };
        self.step = step;
        self.index += 1;
        self.settle_ticks = 0;
        self.setup(window, cx);
        Ok(true)
    }

    fn tick(&mut self, window: &mut Window, cx: &mut App) -> bool {
        match self.advance(window, cx) {
            Ok(true) => true,
            Ok(false) => self.finish(None, cx),
            Err(error) => self.finish(Some(error), cx),
        }
    }
}

fn schedule(run: Rc<RefCell<Run>>, window: &Window) {
    window.on_next_frame(move |window, cx| {
        if run.borrow_mut().tick(window, cx) {
            schedule(run, window);
        }
    });
}

pub fn start(session: Session, window: &Window, cx: &App) {
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&session.report)
        .expect("new M2 report");
    writeln!(output, "{}", session.header).unwrap();
    writeln!(output, "# mode=m2; entries={ENTRIES}; completion_action=insert-selected; cycles={}; steps={}; synthetic GPUI dispatch, NOT OS keyboard / IME", if session.open_diagnostic { 0 } else { CYCLES }, plan(session.open_diagnostic).len()).unwrap();
    writeln!(
        output,
        "# open_diagnostic={}; acceptance={}",
        session.open_diagnostic, !session.open_diagnostic
    )
    .unwrap();
    writeln!(output, "# confirm/TAB-to-submit for body operations; completion/cancel are UI observation only, NOT submission; save-edit first/ready start at next-tick y, save completion starts at C-x C-s; save_ack_before_edit reports whether saving was already acknowledged (worker overlap is not proven)").unwrap();
    writeln!(output, "# lifecycle counts include D1 message history; scratch/fixture/message identities are validated separately; closed fixture views must be released").unwrap();
    writeln!(output, "# prompt typing, assertions and fixture setup excluded; filesystem cache uncontrolled; RSS is observational, not a leak verdict; timeout is not a latency budget").unwrap();
    writeln!(output, "step,phase,status,action_cpu_ms,save_action_cpu_ms,save_ack_before_edit,first_submit_ms,ready_submit_ms,save_observed_ms,result_observed_ms,draw_cpu_max_ms,frames,blank_frames,width_px,height_px,buffers,scratch_buffers,file_buffers,message_buffers,retained_closed_views,rss_kib,observed_elapsed_ms").unwrap();
    output.flush().unwrap();
    let lifecycle = Lifecycle::new(
        session
            .shell
            .read(cx)
            .active_view()
            .read(cx)
            .buffer()
            .snapshot_id()
            .buffer,
    );
    let expected = std::array::from_fn(|_| session.original.clone());
    let mut steps = plan(session.open_diagnostic);
    let step = steps.pop_front().unwrap();
    let trace = session.open_diagnostic.then(|| {
        crate::diagnostic::Trace::new(&session.report, cx.foreground_journal().collector())
    });
    schedule(
        Rc::new(RefCell::new(Run {
            session,
            output,
            trace,
            steps,
            step,
            index: 0,
            prompt_typed: false,
            prepared_at: Instant::now(),
            reading: Reading::default(),
            expected,
            lifecycle,
            expected_points: [0; 3],
            disk_expected: None,
            retired: Vec::new(),
            settle_ticks: 0,
        })),
        window,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{frame, paint};

    fn exercise_workload(cx: &mut gpui_kit::TestAppContext, open_diagnostic: bool) {
        use gpui_kit::{AppContext, VisualTestContext, px, size};

        struct Directory(PathBuf);
        impl Drop for Directory {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".pi/tmp");
        fs::create_dir_all(&root).unwrap();
        let directory = Directory(root.join(
            format!("m2-workload-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()),
        ));
        prepare(&directory.0, "base\n");
        cx.update(semacs::app::init);
        let paints = Rc::new(RefCell::new(VecDeque::new()));
        let probe_paints = paints.clone();
        let window = cx.open_window(size(px(960.), px(640.)), |window, cx| {
            let probe = cx.new(|cx| {
                let shell =
                    cx.new(|cx| AppShell::new(semacs::buffer::Buffer::scratch(), window, cx));
                let subscription = cx.observe(&shell, |_, _, cx| cx.notify());
                crate::Probe {
                    shell,
                    paints: probe_paints,
                    _subscription: subscription,
                }
            });
            gpui_kit::base::Root::new(probe, window, cx)
        });
        let shell = window
            .read_with(cx, |root, cx| {
                root.view()
                    .clone()
                    .downcast::<crate::Probe>()
                    .unwrap()
                    .read(cx)
                    .shell
                    .clone()
            })
            .unwrap();
        let collector = cx.update(|cx| cx.foreground_journal().collector());
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        visual.update(|window, _| window.activate_window());
        let mut steps = plan(open_diagnostic);
        let step = steps.pop_front().unwrap();
        let trace = open_diagnostic.then(|| {
            crate::diagnostic::Trace::new(
                &directory.0.join("report.csv"),
                cx.update(|cx| cx.foreground_journal().collector()),
            )
        });
        let lifecycle = visual.read(|cx| {
            Lifecycle::new(
                shell
                    .read(cx)
                    .active_view()
                    .read(cx)
                    .buffer()
                    .snapshot_id()
                    .buffer,
            )
        });
        let mut run = Run {
            session: Session {
                shell,
                paints: paints.clone(),
                collector,
                directory: directory.0.clone(),
                original: "base\n".into(),
                report: directory.0.join("report.csv"),
                header: String::new(),
                succeeded: Rc::new(Cell::new(false)),
                hardware: false,
                open_diagnostic,
            },
            output: File::create_new(directory.0.join("report.csv")).unwrap(),
            trace,
            steps,
            step,
            index: 0,
            prompt_typed: false,
            prepared_at: Instant::now(),
            reading: Reading::default(),
            expected: std::array::from_fn(|_| "base\n".into()),
            lifecycle,
            expected_points: [0; 3],
            disk_expected: None,
            retired: Vec::new(),
            settle_ticks: 0,
        };
        let mut finished = false;
        for _ in 0..1000 {
            visual.run_until_parked();
            visual.update(|window, cx| window.draw(cx).clear(cx));
            let keep_running = visual.update(|window, cx| {
                let keep_running = run
                    .advance(window, cx)
                    .unwrap_or_else(|error| panic!("step {} {:?}: {error}", run.index, run.step));
                if run.reading.start.is_some() {
                    let paint = paints.borrow_mut().pop_back();
                    if paint.is_some() {
                        // Synthetic profiler boundaries validate the driver only, never timings.
                        let frame = frame(Instant::now());
                        if let Some(trace) = &mut run.trace {
                            trace.record(
                                run.index,
                                run.step.name(),
                                run.reading.start,
                                run.reading.stamp,
                                frame,
                                paint,
                            );
                        }
                        run.reading.accept(paint, frame);
                        if matches!(run.step, Step::Open(_)) && run.reading.stamp.is_none() {
                            assert!(
                                run.reading.first_ms.is_none() && run.reading.ready_ms.is_none()
                            );
                        }
                    }
                }
                paints.borrow_mut().clear();
                keep_running
            });
            if !keep_running {
                finished = true;
                break;
            }
        }
        assert!(
            finished,
            "workload stuck at step {} {:?}",
            run.index, run.step
        );
        assert_eq!(run.index, if open_diagnostic { 1 } else { 30 });
        if let Some(trace) = &mut run.trace {
            trace.finish(true);
            let text = fs::read_to_string(directory.0.join("report.frames.csv")).unwrap();
            assert!(text.contains("completed_open=true; lost_entries=0; acceptance=false"));
            let preparation =
                fs::read_to_string(directory.0.join("report.preparation.csv")).unwrap();
            for phase in [
                "prepare",
                "ensure-index",
                "font-init",
                "wrap-preview",
                "shape-visible",
                "shape-row",
                "shape-line",
                "measure-grapheme",
            ] {
                assert!(
                    preparation.contains(&format!(",foreground,{phase},")),
                    "missing {phase}"
                );
            }
            assert!(preparation.contains(",initial-worker,initial-worker,5,"));
            assert!(preparation.contains("completed_open=true; lost_entries=0; acceptance=false"));
        }
        assert!(run.retired.iter().all(|view| view.upgrade().is_none()));
        let report = fs::read_to_string(directory.0.join("report.csv")).unwrap();
        let last: Vec<_> = report.lines().last().unwrap().split(',').collect();
        assert_eq!(last.len(), 22);
        assert_eq!(
            &last[15..20],
            if open_diagnostic {
                &["2", "1", "1", "0", "0"]
            } else {
                &["2", "1", "0", "1", "0"]
            }
        );
        assert_eq!(
            fs::read(directory.0.join("input-0.txt")).unwrap(),
            if open_diagnostic {
                b"base\n".as_slice()
            } else {
                b"xxxxybase\n".as_slice()
            }
        );
        assert_eq!(
            fs::read(directory.0.join("input-1.txt")).unwrap(),
            b"base\n"
        );
        assert_eq!(
            fs::read(directory.0.join("input-2.txt")).unwrap(),
            b"base\n"
        );
    }

    #[gpui_kit::test]
    fn native_workload_runs_through_the_production_shell_headlessly(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        exercise_workload(cx, false);
    }

    #[gpui_kit::test]
    fn diagnostic_opens_once_without_running_the_remaining_matrix(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        assert_eq!(plan(true), VecDeque::from([Step::Boot, Step::Open(0)]));
        exercise_workload(cx, true);
    }

    #[gpui_kit::test]
    fn lifecycle_rejects_unknown_buffers_and_recreated_message_history(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        use gpui_kit::{VisualTestContext, px, size};
        use semacs::buffer::Buffer;

        cx.update(semacs::app::init);
        let window = cx.open_window(size(px(960.), px(640.)), |window, cx| {
            AppShell::new(Buffer::scratch(), window, cx)
        });
        let shell = window.entity(cx).unwrap();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let mut lifecycle = visual.read(|cx| {
            Lifecycle::new(
                shell
                    .read(cx)
                    .active_view()
                    .read(cx)
                    .buffer()
                    .snapshot_id()
                    .buffer,
            )
        });
        let redraw = |visual: &mut VisualTestContext| {
            visual.run_until_parked();
            visual.update(|window, cx| window.draw(cx).clear(cx));
        };
        let check = |lifecycle: &mut Lifecycle, visual: &VisualTestContext| {
            visual.read(|cx| lifecycle.verify(shell.read(cx), Path::new(".pi/tmp")))
        };
        redraw(&mut visual);
        visual.simulate_keystrokes("ctrl-g");
        redraw(&mut visual);
        lifecycle.messages_required = true;
        check(&mut lifecycle, &visual).unwrap();
        let original_message = lifecycle.messages.unwrap();
        let mut fake_message = Buffer::named("*Messages*");
        fake_message.set_read_only(true);
        for unexpected in [
            Buffer::named("unexpected"),
            fake_message,
            Buffer::new_file(".pi/tmp/leaked.txt".into()),
        ] {
            visual.update(|window, cx| {
                shell.update(cx, |shell, cx| {
                    let index = shell.create_buffer(unexpected, window, cx);
                    assert!(shell.activate(index, window, cx));
                })
            });
            assert!(
                check(&mut lifecycle, &visual)
                    .unwrap_err()
                    .contains("unexpected retained buffer")
            );
            visual.simulate_keystrokes("ctrl-x k");
            redraw(&mut visual);
            visual.simulate_keystrokes("enter");
            redraw(&mut visual);
            check(&mut lifecycle, &visual).unwrap();
        }
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                let index = shell.buffers().position(Buffer::is_message_log).unwrap();
                assert!(shell.activate(index, window, cx));
            })
        });
        visual.simulate_keystrokes("ctrl-x k");
        redraw(&mut visual);
        visual.simulate_keystrokes("enter");
        redraw(&mut visual);
        assert!(
            check(&mut lifecycle, &visual)
                .unwrap_err()
                .contains("missing expected")
        );
        visual.simulate_keystrokes("ctrl-g");
        redraw(&mut visual);
        assert!(
            check(&mut lifecycle, &visual)
                .unwrap_err()
                .contains("message history identity")
        );
        assert_eq!(lifecycle.messages, Some(original_message));
    }

    #[test]
    fn fixture_identity_cannot_be_replaced_or_killed_by_another_buffer() {
        let scratch = semacs::buffer::Buffer::scratch().snapshot_id().buffer;
        let first = semacs::buffer::Buffer::scratch().snapshot_id().buffer;
        let other = semacs::buffer::Buffer::scratch().snapshot_id().buffer;
        let mut lifecycle = Lifecycle::new(scratch);
        lifecycle.opened(0, first).unwrap();
        assert!(lifecycle.opened(0, other).is_err());
        assert!(lifecycle.killed(0, other).is_err());
        assert_eq!(lifecycle.files[0], Some(first));
        lifecycle.killed(0, first).unwrap();
        assert!(lifecycle.verify_finished(false).is_err());
        lifecycle.messages_required = true;
        assert!(lifecycle.verify_finished(false).is_ok());
        assert!(lifecycle.verify_finished(true).is_err());
    }

    #[test]
    fn plan_covers_three_buffers_and_repeated_lifecycle() {
        let steps = plan(false);
        assert_eq!(steps.len(), 31);
        assert_eq!(
            steps
                .iter()
                .filter(|step| matches!(step, Step::Open(_)))
                .count(),
            6
        );
        assert_eq!(steps.iter().filter(|step| **step == Step::Kill).count(), 6);
    }

    #[test]
    fn previous_or_other_target_paints_cannot_complete_a_reading() {
        let start = Instant::now();
        let target = paint(start);
        let mut r = Reading {
            start: Some(start),
            stamp: Some(target.stamp),
            bytes: target.bytes,
            ..Default::default()
        };
        r.accept(Some(paint(start)), frame(start));
        assert!(r.ready_ms.is_none());
        r.accept(Some(target), frame(start - Duration::from_millis(10)));
        assert!(r.ready_ms.is_none());
        r.accept(Some(target), frame(start));
        assert!(r.ready_ms.is_some());
    }

    #[test]
    fn only_existing_budgets_are_applied_to_new_m2_operations() {
        let start = Instant::now();
        let r = Reading {
            start: Some(start),
            action_ms: 3.,
            first_ms: Some(20.),
            ready_ms: Some(400.),
            ..Default::default()
        };
        assert!(r.budget_error(Step::Open(0), true).is_none());
        assert!(r.budget_error(Step::Switch(0), true).is_none());
        assert!(r.budget_error(Step::Insert, true).is_some());
        assert!(r.budget_error(Step::SaveEdit, true).is_some());
        assert!(r.budget_error(Step::Insert, false).is_none());
    }
}
