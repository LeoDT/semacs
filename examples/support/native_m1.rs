use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui_kit::profiler::journal::{ForegroundJournalCollector, PresentedFrame};
use gpui_kit::{App, Entity, Keystroke, Window};
use semacs::view::BufferView;

use super::{EditStamp, PaintState, collect_frames, millis, workload};

#[derive(Clone, Copy)]
pub struct Config {
    pub seconds: u64,
    pub interval_ms: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            seconds: 60,
            interval_ms: 50,
        }
    }
}

impl Config {
    pub fn validate(self) -> Result<(), String> {
        if !(1..=600).contains(&self.seconds) || !(20..=1000).contains(&self.interval_ms) {
            return Err("M1 requires duration 1..600 seconds and interval 20..1000 ms".into());
        }
        Ok(())
    }

    pub fn keys(self) -> usize {
        (self.seconds * 1000 / self.interval_ms) as usize
    }

    fn due(self, origin: Instant, sent: usize) -> Instant {
        origin + Duration::from_millis(self.interval_ms * sent as u64)
    }
}

pub struct Session {
    pub editor: Entity<BufferView>,
    pub paints: Rc<RefCell<VecDeque<PaintState>>>,
    pub collector: ForegroundJournalCollector,
    pub original: String,
    pub opened_at: Instant,
    pub report: PathBuf,
    pub header: String,
    pub succeeded: Rc<Cell<bool>>,
    pub hardware: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Open,
    Chain(usize),
    Sustained(usize),
}

impl Phase {
    fn name(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Chain(_) => "chain",
            Self::Sustained(_) => "sustained",
        }
    }

    fn index(self) -> usize {
        match self {
            Self::Open => 0,
            Self::Chain(index) | Self::Sustained(index) => index,
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::Open => "",
            Self::Chain(index) => workload::CHAIN[index].0,
            Self::Sustained(index) => workload::stream_key(index),
        }
    }
}

struct Reading {
    phase: Phase,
    start: Instant,
    stamp: EditStamp,
    bytes: usize,
    action_ms: f64,
    lag_ms: f64,
    first_frame: Option<PresentedFrame>,
    first_paint: Option<PaintState>,
    first: Option<PresentedFrame>,
    ready: Option<PresentedFrame>,
    paint: Option<PaintState>,
    frames: usize,
    max_draw_ms: f64,
    blank_frames: usize,
}

impl Reading {
    fn new(phase: Phase, start: Instant, stamp: EditStamp, bytes: usize) -> Self {
        Self {
            phase,
            start,
            stamp,
            bytes,
            action_ms: 0.,
            lag_ms: 0.,
            first_frame: None,
            first_paint: None,
            first: None,
            ready: None,
            paint: None,
            frames: 0,
            max_draw_ms: 0.,
            blank_frames: 0,
        }
    }

    fn accept(&mut self, frame: PresentedFrame, paint: Option<PaintState>) {
        if frame.frame.draw_start < self.start {
            return;
        }
        self.frames += 1;
        self.max_draw_ms = self.max_draw_ms.max(millis(frame.frame.draw_duration()));
        if self.first_frame.is_none() {
            self.first_frame = Some(frame);
            self.first_paint = paint;
        }
        let Some(paint) = paint else { return };
        self.blank_frames += usize::from(paint.visible == 0);
        if paint.stamp != self.stamp
            || paint.bytes != self.bytes
            || !paint.compatible
            || paint.marked
            || paint.visible == 0
        {
            return;
        }
        self.paint = Some(paint);
        self.first.get_or_insert(frame);
        if paint.ready && paint.indexed == self.bytes {
            self.ready.get_or_insert(frame);
        }
    }

    fn budget_error(&self, now: Instant, hardware: bool) -> Option<String> {
        let label = format!("{} step {}", self.phase.name(), self.phase.index());
        if self.blank_frames > 0 {
            return Some(format!("{label}: blank body frame"));
        }
        if self.phase == Phase::Open && self.frames > 0 && self.first_paint.is_none() {
            return Some("open: first frame has no body observation".into());
        }
        if now.duration_since(self.start) > Duration::from_secs(30) {
            return Some(format!("{label}: observation timeout"));
        }
        if !hardware {
            return None;
        }
        let (first_limit, ready_limit) = if self.phase == Phase::Open {
            (250., 500.)
        } else {
            (16., 300.)
        };
        for (metric, value, limit) in [
            ("draw_cpu_max_ms", self.max_draw_ms, 16.),
            ("action_cpu_ms", self.action_ms, 2.),
            (
                "first_submit_ms",
                elapsed(self.first, self.start).unwrap_or_else(|| millis(now - self.start)),
                first_limit,
            ),
            (
                "ready_submit_ms",
                elapsed(self.ready, self.start).unwrap_or_else(|| millis(now - self.start)),
                ready_limit,
            ),
        ] {
            if value > limit {
                return Some(format!(
                    "{label}: {metric}={value:.3} ms > {limit:.3} ms{}",
                    if (metric == "first_submit_ms" && self.first.is_none())
                        || (metric == "ready_submit_ms" && self.ready.is_none())
                    {
                        " (target submission not observed)"
                    } else {
                        ""
                    }
                ));
            }
        }
        None
    }

    fn status(&self) -> &'static str {
        if self.ready.is_some() {
            "ready"
        } else if self.first.is_some() {
            "superseded-before-ready"
        } else {
            "superseded-before-first"
        }
    }
}

fn elapsed(frame: Option<PresentedFrame>, start: Instant) -> Option<f64> {
    frame.map(|frame| millis(frame.presentation.present_end - start))
}

fn cell(value: Option<f64>) -> String {
    value.map_or_else(String::new, |value| format!("{value:.3}"))
}

struct Run {
    config: Config,
    session: Session,
    output: File,
    current: Option<Reading>,
    next_chain: usize,
    origin: Option<Instant>,
    sent: usize,
    ready_keys: usize,
    superseded_ready: usize,
    superseded_first: usize,
    last_paint: Option<PaintState>,
}

impl Run {
    fn write_row(&mut self, reading: Reading, failed: bool) {
        let status = if failed { "failed" } else { reading.status() };
        if matches!(reading.phase, Phase::Sustained(_)) {
            match status {
                "ready" => self.ready_keys += 1,
                "superseded-before-ready" => self.superseded_ready += 1,
                "superseded-before-first" => self.superseded_first += 1,
                _ => {}
            }
        }
        let paint = reading.paint.or(self.last_paint);
        writeln!(
            self.output,
            "{},{},{},{},{},{},{},{:.3},{},{},{},{:.3},{},{},{:.3},{:.3},{},{},{}",
            reading.phase.name(),
            reading.phase.index(),
            reading.phase.key(),
            reading.bytes,
            reading.stamp.point,
            reading
                .stamp
                .mark
                .map_or_else(String::new, |mark| mark.to_string()),
            reading.stamp.mark_active,
            reading.action_ms,
            cell(elapsed(reading.first, reading.start)),
            cell(elapsed(reading.ready, reading.start)),
            cell(elapsed(reading.first_frame, reading.start)),
            reading.max_draw_ms,
            reading.frames,
            reading.blank_frames,
            reading.lag_ms,
            millis(reading.start.elapsed()),
            status,
            paint.map_or_else(String::new, |paint| format!("{:.1}", paint.width)),
            paint.map_or_else(String::new, |paint| format!("{:.1}", paint.height)),
        )
        .unwrap();
        self.output.flush().unwrap();
    }

    fn finish(&mut self, error: Option<String>, cx: &mut App) -> bool {
        let success = error.is_none();
        if let Some(reason) = error {
            if let Some(reading) = self.current.take() {
                self.write_row(reading, true);
            }
            writeln!(self.output, "# stop_reason={reason}").unwrap();
            println!("STOP {reason}; report={}", self.session.report.display());
        }
        writeln!(self.output,
            "# stream_keys={}; ready_keys={}; superseded_before_ready={}; superseded_before_first={}; stream_elapsed_ms={}",
            self.sent, self.ready_keys, self.superseded_ready, self.superseded_first,
            self.origin.map_or_else(String::new, |origin| format!("{:.3}", millis(origin.elapsed()))),
        ).unwrap();
        writeln!(self.output,
            "# completed={success}; budget_passed={}; gpu_execution_ms=unmeasured; compositor_presentation_ms=unmeasured",
            if self.session.hardware { if success { "true" } else { "false" } } else { "not-applicable" },
        ).unwrap();
        self.output.flush().unwrap();
        self.session.succeeded.set(success);
        if success {
            println!("COMPLETED {}", self.session.report.display());
        }
        cx.quit();
        false
    }

    fn check_text(&self, prefix: &str, point: usize, cx: &App) -> Result<(), String> {
        let view = self.session.editor.read(cx);
        if view.buffer().point().0 != point
            || view.buffer().text().to_string() != format!("{prefix}{}", self.session.original)
        {
            return Err(format!(
                "text / point mismatch, expected prefix {prefix:?} and point {point}"
            ));
        }
        Ok(())
    }

    fn begin(
        &mut self,
        phase: Phase,
        due: Instant,
        window: &mut Window,
        cx: &mut App,
    ) -> Result<(), String> {
        let key = Keystroke::parse(phase.key()).unwrap();
        let (bytes, point) = match phase {
            Phase::Chain(index) => {
                let (_, prefix, point) = workload::CHAIN[index];
                (self.session.original.len() + prefix.len(), point)
            }
            Phase::Sustained(index) => {
                let count = workload::pending_bytes(index + 1);
                (self.session.original.len() + count, count)
            }
            Phase::Open => unreachable!(),
        };
        let start = Instant::now();
        window.dispatch_keystroke(key, cx);
        let action_ms = millis(start.elapsed());
        let view = self.session.editor.read(cx);
        let stamp = EditStamp::capture(view);
        let mut reading = Reading::new(phase, start, stamp, bytes);
        reading.action_ms = action_ms;
        reading.lag_ms = millis(start.saturating_duration_since(due));
        self.current = Some(reading);
        if stamp.point != point || view.buffer().text().len() != bytes {
            return Err(format!(
                "{} step {}: dropped or incorrect input",
                phase.name(),
                phase.index()
            ));
        }
        Ok(())
    }

    fn advance(&mut self, window: &mut Window, cx: &mut App) -> Result<bool, String> {
        for (frame, paint) in
            collect_frames(&mut self.session.collector, &self.session.paints, window)
        {
            if let Some(paint) = paint {
                self.last_paint = Some(paint);
            }
            if let Some(reading) = &mut self.current {
                reading.accept(frame, paint);
            }
            if self.session.hardware && millis(frame.frame.draw_duration()) > 16. {
                return Err(format!(
                    "draw_cpu_ms={:.3} ms > 16 ms",
                    millis(frame.frame.draw_duration())
                ));
            }
        }
        let now = Instant::now();
        if let Some(reading) = &self.current {
            if let Some(error) = reading.budget_error(now, self.session.hardware) {
                return Err(error);
            }
            if reading.ready.is_some() {
                match reading.phase {
                    Phase::Open => self.check_text("", 0, cx)?,
                    Phase::Chain(index) => {
                        let (_, prefix, point) = workload::CHAIN[index];
                        self.check_text(prefix, point, cx)?;
                    }
                    Phase::Sustained(_) => {}
                }
                let reading = self.current.take().unwrap();
                self.write_row(reading, false);
            }
        }
        if self.origin.is_none() {
            if self.current.is_none() {
                if self.next_chain < workload::CHAIN.len() {
                    self.begin(Phase::Chain(self.next_chain), Instant::now(), window, cx)?;
                    self.next_chain += 1;
                } else {
                    self.origin = Some(Instant::now());
                    println!(
                        "SUSTAINED duration_seconds={} interval_ms={} keys={}",
                        self.config.seconds,
                        self.config.interval_ms,
                        self.config.keys()
                    );
                }
            }
            return Ok(true);
        }
        let origin = self.origin.unwrap();
        if self.sent < self.config.keys() && now >= self.config.due(origin, self.sent) {
            if let Some(reading) = self.current.take() {
                self.write_row(reading, false);
            }
            self.begin(
                Phase::Sustained(self.sent),
                self.config.due(origin, self.sent),
                window,
                cx,
            )?;
            self.sent += 1;
        }
        if self.sent == self.config.keys() && self.current.is_none() {
            let count = workload::pending_bytes(self.sent);
            self.check_text(&"x".repeat(count), count, cx)?;
            return Ok(false);
        }
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

pub fn start(config: Config, session: Session, window: &Window, cx: &App) {
    config.validate().expect("valid M1 settings");
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&session.report)
        .expect("create new M1 report; refusing to overwrite");
    writeln!(output, "{}", session.header).unwrap();
    writeln!(output, "# mode=m1; duration_seconds={}; interval_ms={}; command_steps={}; synthetic GPUI key dispatch; NOT OS keyboard or IME latency",
        config.seconds, config.interval_ms, workload::CHAIN.len()).unwrap();
    writeln!(output, "# superseded ready is unmeasured, NOT zero; final surviving key must reach full ready; schedule_lag_ms is reported, not a new budget").unwrap();
    writeln!(output, "phase,step,key,bytes,point,mark,mark_active,action_cpu_ms,first_submit_ms,ready_submit_ms,first_frame_submit_ms,draw_cpu_max_ms,frames,blank_frames,schedule_lag_ms,observed_elapsed_ms,status,width_px,height_px").unwrap();
    output.flush().unwrap();
    let stamp = EditStamp::capture(session.editor.read(cx));
    let current = Some(Reading::new(
        Phase::Open,
        session.opened_at,
        stamp,
        session.original.len(),
    ));
    schedule(
        Rc::new(RefCell::new(Run {
            config,
            session,
            output,
            current,
            next_chain: 0,
            origin: None,
            sent: 0,
            ready_keys: 0,
            superseded_ready: 0,
            superseded_first: 0,
            last_paint: None,
        })),
        window,
    );
}

#[cfg(test)]
#[path = "native_m1_tests.rs"]
mod tests;
