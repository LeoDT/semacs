use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui_kit::profiler::journal::{ForegroundJournalCollector, PresentedFrame};
use gpui_kit::{
    App, AppContext, Bounds, Context, Entity, IntoElement, Keystroke, ParentElement, Point, Render,
    Styled, Subscription, TitlebarOptions, Window, WindowBounds, WindowOptions, canvas, div,
    profiler, px, size,
};
use semacs::app::AppShell;
use semacs::buffer::{Buffer, SnapshotId};
use semacs::files::{DecodedFile, FileFormat};
use semacs::text::ByteOffset;
use semacs::view::BufferView;

use super::{EditStamp, PaintState, Step, capture, collect_paints, millis};

#[derive(Clone, Copy)]
struct SyntaxPaint {
    body: PaintState,
    syntax: Option<SnapshotId>,
    syntax_ready: bool,
}

fn observe(view: &BufferView) -> SyntaxPaint {
    SyntaxPaint {
        body: capture(view),
        syntax: view.syntax_snapshot(),
        syntax_ready: view.syntax_ready(),
    }
}

struct Probe {
    shell: Entity<AppShell>,
    paints: Rc<RefCell<VecDeque<SyntaxPaint>>>,
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
                    paints.borrow_mut().push_back(observe(editor.read(cx)));
                },
            )
            .absolute()
            .size_full(),
        )
    }
}

fn phases() -> VecDeque<Step> {
    [
        Step::Open,
        Step::Insert,
        Step::Delete,
        Step::Newline,
        Step::Join,
        Step::BurstInsert,
        Step::BurstDelete,
    ]
    .into_iter()
    .chain(std::iter::repeat_n(Step::Forced, 30))
    .chain([Step::Idle])
    .collect()
}

fn is_input(step: Step) -> bool {
    matches!(
        step,
        Step::Insert
            | Step::Delete
            | Step::Newline
            | Step::Join
            | Step::BurstInsert
            | Step::BurstDelete
    )
}

fn needs_syntax(step: Step) -> bool {
    matches!(step, Step::BurstDelete | Step::Forced | Step::Idle)
}

struct Reading {
    step: Step,
    start: Instant,
    target: EditStamp,
    bytes: usize,
    pending_before_action: bool,
    action_ms: f64,
    first_frame: Option<PresentedFrame>,
    first: Option<PresentedFrame>,
    layout: Option<PresentedFrame>,
    syntax: Option<PresentedFrame>,
    paint: Option<SyntaxPaint>,
    frames: usize,
    missing: usize,
    blank: usize,
    pending_frames: usize,
    first_visible: usize,
    idle_ticks: usize,
    max_draw_ms: f64,
    invalid_syntax: bool,
    mismatched_target: bool,
}

impl Reading {
    fn new(step: Step, start: Instant, target: EditStamp, bytes: usize, pending: bool) -> Self {
        Self {
            step,
            start,
            target,
            bytes,
            pending_before_action: pending,
            action_ms: 0.,
            first_frame: None,
            first: None,
            layout: None,
            syntax: None,
            paint: None,
            frames: 0,
            missing: 0,
            blank: 0,
            pending_frames: 0,
            first_visible: 0,
            idle_ticks: 0,
            max_draw_ms: 0.,
            invalid_syntax: false,
            mismatched_target: false,
        }
    }

    fn accept(&mut self, frame: PresentedFrame, paint: Option<SyntaxPaint>) {
        if frame.frame.draw_start < self.start {
            return;
        }
        self.frames += 1;
        self.first_frame.get_or_insert(frame);
        self.max_draw_ms = self.max_draw_ms.max(millis(frame.frame.draw_duration()));
        let Some(paint) = paint else {
            self.missing += 1;
            return;
        };
        let body = paint.body;
        if self.frames == 1 {
            self.first_visible = body.visible;
        }
        self.blank += usize::from(body.visible == 0);
        if body.stamp != self.target || body.bytes != self.bytes || body.marked {
            self.missing += 1;
            self.mismatched_target = true;
            return;
        }
        self.invalid_syntax |= paint
            .syntax
            .is_some_and(|id| id != self.target.display.committed)
            || paint.syntax_ready != paint.syntax.is_some();
        self.pending_frames += usize::from(!paint.syntax_ready);
        if !body.compatible || body.visible == 0 {
            return;
        }
        self.first.get_or_insert(frame);
        self.paint = Some(paint);
        if body.ready && body.indexed == self.bytes {
            self.layout.get_or_insert(frame);
            if paint.syntax == Some(self.target.display.committed) && paint.syntax_ready {
                self.syntax.get_or_insert(frame);
            }
        }
    }

    fn complete(&self) -> bool {
        if matches!(self.step, Step::Idle) {
            self.idle_ticks >= 30
        } else {
            self.layout.is_some() && (!needs_syntax(self.step) || self.syntax.is_some())
        }
    }

    fn error(&self, now: Instant, hardware: bool) -> Option<String> {
        if self.mismatched_target {
            return Some(
                "same-frame paint observed but target identity / bytes / marked state mismatch"
                    .into(),
            );
        }
        if self.missing > 0 {
            return Some("missing exact same-frame target observation".into());
        }
        if self.blank > 0 {
            return Some("blank body frame".into());
        }
        if self.invalid_syntax {
            return Some("syntax snapshot / readiness does not match committed target".into());
        }
        if matches!(self.step, Step::Idle) && self.frames > 0 {
            return Some("idle produced draw".into());
        }
        if now.duration_since(self.start) >= Duration::from_secs(30) {
            return Some("observation timeout; not a numeric syntax-ready budget".into());
        }
        if !hardware {
            return None;
        }
        let (action, first, layout) = match self.step {
            Step::Open => (None, Some(250.), Some(500.)),
            Step::Insert | Step::Delete | Step::Newline | Step::Join => {
                (Some(2.), Some(16.), Some(300.))
            }
            Step::BurstInsert | Step::BurstDelete => (Some(50.), Some(50.), Some(300.)),
            Step::Forced => (None, Some(2.), None),
            _ => (None, None, None),
        };
        for (key, value, limit) in [
            ("action_cpu_ms", self.action_ms, action),
            (
                "first_submit_ms",
                elapsed(self.first, self.start, now),
                first,
            ),
            (
                "ready_submit_ms",
                elapsed(self.layout, self.start, now),
                layout,
            ),
            ("draw_cpu_max_ms", self.max_draw_ms, Some(16.)),
        ] {
            if let Some(limit) = limit
                && value > limit
            {
                return Some(format!("{key}={value:.6} > {limit:.6} ms"));
            }
        }
        None
    }
}

fn elapsed(frame: Option<PresentedFrame>, start: Instant, now: Instant) -> f64 {
    millis(
        frame
            .map_or(now, |frame| frame.presentation.present_end)
            .saturating_duration_since(start),
    )
}

fn cell(frame: Option<PresentedFrame>, start: Instant) -> String {
    frame.map_or_else(String::new, |frame| {
        format!("{:.6}", elapsed(Some(frame), start, start))
    })
}

struct Run {
    editor: Entity<BufferView>,
    paints: Rc<RefCell<VecDeque<SyntaxPaint>>>,
    collector: ForegroundJournalCollector,
    output: File,
    hardware: bool,
    succeeded: Rc<Cell<bool>>,
    phases: VecDeque<Step>,
    current: Option<Reading>,
    last: Option<SyntaxPaint>,
    expected: String,
    original: String,
    original_point: usize,
    point: usize,
    rows: usize,
    pending_inputs: usize,
}

fn advance_expected(expected: &mut String, point: &mut usize, step: Step) {
    match step {
        Step::Insert => {
            expected.insert(*point, 'x');
            *point += 1;
        }
        Step::Delete => {
            *point -= 1;
            assert_eq!(expected.remove(*point), 'x');
        }
        Step::Newline => {
            assert!(expected[..*point].ends_with("<script>const n = "));
            assert!(expected[*point..].starts_with("call(42)"));
            expected.replace_range(*point - 1..*point, "\n");
        }
        Step::Join => {
            *point -= 1;
            assert_eq!(expected.remove(*point), '\n');
        }
        Step::BurstInsert => {
            expected.insert_str(*point, &"x".repeat(32));
            *point += 32;
        }
        Step::BurstDelete => {
            expected.replace_range(*point - 32..*point, "");
            *point -= 32;
        }
        _ => {}
    }
}

fn final_matches(expected: &str, point: usize, original: &str, original_point: usize) -> bool {
    let (before, after) = original.split_at(original_point);
    before.ends_with(' ')
        && point == original_point - 1
        && expected.len() == original.len() - 1
        && expected.starts_with(&before[..before.len() - 1])
        && expected.ends_with(after)
}

impl Run {
    fn begin(&mut self, step: Step, window: &mut Window, cx: &mut App) {
        let view = self.editor.read(cx);
        let pending = view.syntax_snapshot() != Some(view.buffer().snapshot_id());
        advance_expected(&mut self.expected, &mut self.point, step);
        let start = Instant::now();
        if is_input(step) {
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
        } else if matches!(step, Step::Forced) {
            window.refresh();
        }
        let action_ms = millis(start.elapsed());
        let mut reading = Reading::new(
            step,
            start,
            EditStamp::capture(self.editor.read(cx)),
            self.expected.len(),
            pending,
        );
        reading.action_ms = action_ms;
        self.current = Some(reading);
    }

    fn verify(&self, reading: &Reading, cx: &App) -> Result<(), String> {
        let view = self.editor.read(cx);
        if view.buffer().text().to_string() != self.expected
            || view.buffer().point().0 != self.point
            || EditStamp::capture(view) != reading.target
        {
            return Err("committed text / point / display identity mismatch".into());
        }
        if needs_syntax(reading.step)
            && (view.syntax_snapshot() != Some(view.buffer().snapshot_id()) || !view.syntax_ready())
        {
            return Err("final syntax unavailable or bound to another snapshot".into());
        }
        Ok(())
    }

    fn write_row(&mut self, reading: &Reading, verified: bool) {
        let paint = reading.paint.or(self.last);
        let target = reading.target.display.committed;
        let (paint_buffer, paint_version, syntax_buffer, syntax_version) = paint.map_or_else(
            || (String::new(), String::new(), String::new(), String::new()),
            |paint| {
                (
                    format!("{:?}", paint.body.stamp.display.committed.buffer),
                    format!("{:?}", paint.body.stamp.display.committed.version),
                    paint
                        .syntax
                        .map_or_else(String::new, |id| format!("{:?}", id.buffer)),
                    paint
                        .syntax
                        .map_or_else(String::new, |id| format!("{:?}", id.version)),
                )
            },
        );
        let body = paint.map(|paint| paint.body);
        let status = if matches!(reading.step, Step::Idle) {
            "idle"
        } else if reading.syntax.is_some() {
            "ready"
        } else {
            "not-observed"
        };
        writeln!(self.output,
            "{},{},{},{},{:?},{:?},{},{},{},{},{},{},{},{},{},{},{:.6},{:.6},{},{},{},{},{},{},{:.1},{:.1},{},{},{},{:.6}",
            reading.step.name(), self.rows, reading.bytes, reading.target.point,
            target.buffer, target.version, paint_buffer, paint_version, syntax_buffer, syntax_version,
            reading.pending_before_action, reading.pending_frames,
            cell(reading.first_frame, reading.start), cell(reading.first, reading.start),
            cell(reading.layout, reading.start), cell(reading.syntax, reading.start),
            reading.action_ms, reading.max_draw_ms, reading.frames, reading.missing,
            reading.blank, reading.idle_ticks, body.map_or(0, |body| body.visible),
            body.map_or(0, |body| body.indexed), body.map_or(0., |body| body.width),
            body.map_or(0., |body| body.height), reading.first_visible, verified, status, millis(reading.start.elapsed()),
        ).unwrap();
        self.output.flush().unwrap();
        self.rows += 1;
        self.pending_inputs += usize::from(is_input(reading.step) && reading.pending_before_action);
    }

    fn finish(&mut self, error: Option<String>, cx: &mut App) -> bool {
        let error = error.or_else(|| {
            if !final_matches(
                &self.expected,
                self.point,
                &self.original,
                self.original_point,
            ) {
                Some("final text / point differs from RET whitespace-cleaned fixture".into())
            } else if self.pending_inputs == 0 {
                Some("coverage missing: no input while prior syntax was pending".into())
            } else {
                None
            }
        });
        if let Some(error) = error {
            if let Some(reading) = self.current.take() {
                self.write_row(&reading, false);
            }
            writeln!(
                self.output,
                "# completed=false; STOP {}",
                error.replace(['\n', '\r'], " ")
            )
            .unwrap();
            println!("STOP {error}");
        } else {
            writeln!(self.output, "# completed=true; input_while_syntax_pending={}; layout_budget_separate=true; syntax_ready_budget=not-established; gpu_execution_ms=unmeasured; compositor_presentation_ms=unmeasured", self.pending_inputs).unwrap();
            self.succeeded.set(true);
            println!("COMPLETED syntax");
        }
        self.output.flush().unwrap();
        cx.quit();
        false
    }

    fn tick(&mut self, window: &mut Window, cx: &mut App) -> bool {
        for (frame, paint) in collect_paints(&mut self.collector, &self.paints, window, |paint| {
            paint.body.at
        }) {
            if let Some(paint) = paint {
                self.last = Some(paint);
            }
            if let Some(reading) = &mut self.current {
                reading.accept(frame, paint);
            }
        }
        if let Some(reading) = &mut self.current {
            if matches!(reading.step, Step::Idle) {
                reading.idle_ticks += 1;
            }
            if let Some(error) = reading.error(Instant::now(), self.hardware) {
                let error = format!("{}: {error}", reading.step.name());
                return self.finish(Some(error), cx);
            }
        }
        if self.current.as_ref().is_some_and(Reading::complete) {
            let reading = self.current.take().unwrap();
            if let Err(error) = self.verify(&reading, cx) {
                self.current = Some(reading);
                return self.finish(Some(error), cx);
            }
            self.write_row(&reading, true);
        }
        if self.current.is_none() {
            if let Some(step) = self.phases.pop_front() {
                self.begin(step, window, cx);
            } else {
                return self.finish(None, cx);
            }
        }
        true
    }
}

fn schedule(run: Rc<RefCell<Run>>, window: &Window) {
    window.on_next_frame(move |window, cx| {
        if run.borrow_mut().tick(window, cx) {
            schedule(run, window);
        }
    });
}

fn input_offset(sample: &str, text: &str) -> usize {
    let prefix = if sample == "markdown" {
        "```html\n"
    } else {
        ""
    };
    let marker = format!("{prefix}<script>const n = call(42);</script><style>");
    text.find(&marker).expect("embedded script input marker")
        + prefix.len()
        + "<script>const n = ".len()
}

fn buffer(input: PathBuf, sample: &str, text: &str) -> (Buffer, usize) {
    let extension = match sample {
        "markdown" => "md",
        "html" => "html",
        _ => panic!("unknown syntax sample"),
    };
    assert_eq!(input.extension().and_then(|s| s.to_str()), Some(extension));
    assert!(
        (256 * 1024..257 * 1024).contains(&text.len()),
        "expected representative 256 KiB syntax fixture"
    );
    let offset = input_offset(sample, text);
    let mut buffer = Buffer::new(
        Some(input),
        DecodedFile {
            text: text.into(),
            format: FileFormat::default(),
        },
    );
    buffer.set_point(ByteOffset(offset)).unwrap();
    (buffer, offset)
}

pub fn run(input: PathBuf, sample: String, text: String, report: PathBuf, allow_software: bool) {
    let (buffer, offset) = buffer(input, &sample, &text);
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(report)
        .expect("new syntax report");
    let succeeded = Rc::new(Cell::new(false));
    let outcome = succeeded.clone();
    gpui_kit::application().run(move |cx| {
        semacs::app::init(cx);
        profiler::set_trace_enabled(true);
        let collector = cx.foreground_journal().collector();
        let paints = Rc::new(RefCell::new(VecDeque::new()));
        let probe_paints = paints.clone();
        let opened_at = Instant::now();
        let (window, probe) = gpui_kit::open_window(WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(Point::default(), size(px(960.), px(640.))))),
            app_id: Some(format!("semacs-perf-native-{}", std::env::var("SEMACS_PERF_TOKEN").unwrap_or_default())),
            titlebar: Some(TitlebarOptions { title: Some("semacs syntax CPU performance".into()), ..Default::default() }),
            ..Default::default()
        }, cx, move |window, cx| cx.new(|cx| {
            let shell = cx.new(|cx| AppShell::new(buffer, window, cx));
            let subscription = cx.observe(&shell, |_, _, cx| cx.notify());
            Probe { shell, paints: probe_paints, _subscription: subscription }
        })).expect("open native syntax probe");
        let editor = probe.read(cx).shell.read(cx).active_view().clone();
        window.update(cx, move |_, window, cx| {
            let specs = window.gpu_specs().expect("renderer identity required");
            assert!(allow_software || !specs.is_software_emulated, "hardware GPU required");
            writeln!(output, "# mode=syntax; sample={sample}; release={}; fixture_bytes={}; input_offset={offset}; graphics={specs:?}", !cfg!(debug_assertions), text.len()).unwrap();
            writeln!(output, "# Lilex 16px/24px; 960x640 requested, actual body sizes below; soft wrap enabled; synthetic GPUI input, NOT OS keys or IME; CPU submission NOT screen presentation").unwrap();
            writeln!(output, "phase,step,bytes,point,target_buffer,target_version,painted_buffer,painted_version,syntax_buffer,syntax_version,syntax_pending_before_action,syntax_pending_frames,first_frame_submit_ms,first_submit_ms,ready_submit_ms,syntax_and_layout_submit_ms,action_cpu_ms,draw_cpu_max_ms,frames,missing_observations,blank_frames,idle_ticks,visible_rows,indexed_bytes,width_px,height_px,first_visible_rows,text_verified,syntax_status,observed_elapsed_ms").unwrap();
            output.flush().unwrap();
            let mut steps = phases();
            let first = steps.pop_front().unwrap();
            let reading = Reading::new(first, opened_at, EditStamp::capture(editor.read(cx)), text.len(), true);
            let run = Run {
                editor, paints, collector, output, hardware: !specs.is_software_emulated,
                succeeded: outcome, phases: steps, current: Some(reading), last: None,
                expected: text.clone(), original: text, original_point: offset, point: offset, rows: 0, pending_inputs: 0,
            };
            schedule(Rc::new(RefCell::new(run)), window);
        }).unwrap();
    });
    if !succeeded.get() {
        std::process::exit(2);
    }
}

#[cfg(test)]
#[path = "native_syntax_tests.rs"]
mod tests;
