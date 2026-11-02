use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui_kit::profiler::journal::{ForegroundJournalCollector, PresentedFrame};
use gpui_kit::*;
use semacs::app::AppShell;
use semacs::buffer::Buffer;
use semacs::files::{DecodedFile, FileFormat};
use semacs::view::BufferView;

use super::{EditStamp, PaintState, capture, collect_paints, millis};

#[cfg(test)]
#[path = "native_split_tests.rs"]
mod tests;

#[derive(Clone)]
struct Body {
    id: EntityId,
    paint: PaintState,
    font_size: f32,
}

#[derive(Clone)]
struct Scene {
    at: Instant,
    selected: EntityId,
    bodies: Vec<Body>,
}

struct Probe {
    shell: Entity<AppShell>,
    views: Rc<RefCell<Vec<Entity<BufferView>>>>,
    paints: Rc<RefCell<VecDeque<Scene>>>,
    _subscription: Subscription,
}

impl Render for Probe {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let shell = self.shell.clone();
        let views = self.views.clone();
        let paints = self.paints.clone();
        div().size_full().child(self.shell.clone()).child(
            canvas(
                |_, _, _| (),
                move |_, _, _, cx| {
                    let bodies = views
                        .borrow()
                        .iter()
                        .map(|entity| {
                            let view = entity.read(cx);
                            Body {
                                id: entity.entity_id(),
                                paint: capture(view),
                                font_size: f32::from(
                                    view.wrap_index().unwrap().key().settings.font_size,
                                ),
                            }
                        })
                        .collect();
                    paints.borrow_mut().push_back(Scene {
                        at: Instant::now(),
                        selected: shell.read(cx).active_view().entity_id(),
                        bodies,
                    });
                },
            )
            .absolute()
            .size_full(),
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Boot,
    Split,
    PeerScroll,
    Select,
    Insert,
    Delete,
    Newline,
    Join,
    BurstInsert,
    BurstDelete,
    Scroll,
    Resize,
    Font,
    Forced,
    Idle,
}

impl Phase {
    fn name(self) -> &'static str {
        match self {
            Self::Boot => "boot",
            Self::Split => "split-and-select",
            Self::PeerScroll => "peer-scroll",
            Self::Select => "select-first",
            Self::Insert => "insert",
            Self::Delete => "delete",
            Self::Newline => "newline",
            Self::Join => "join",
            Self::BurstInsert => "burst-insert",
            Self::BurstDelete => "burst-delete",
            Self::Scroll => "scroll",
            Self::Resize => "resize",
            Self::Font => "font",
            Self::Forced => "forced",
            Self::Idle => "idle",
        }
    }

    fn limits(self) -> (Option<f64>, Option<f64>, Option<f64>, Option<f64>) {
        match self {
            Self::Boot => (None, Some(250.), Some(500.), None),
            Self::Insert | Self::Delete | Self::Newline | Self::Join => {
                (Some(2.), Some(16.), Some(300.), None)
            }
            Self::BurstInsert | Self::BurstDelete => (Some(50.), Some(50.), Some(300.), None),
            Self::PeerScroll | Self::Scroll => (None, Some(4.), None, None),
            Self::Forced => (None, Some(2.), None, None),
            Self::Resize | Self::Font => (None, None, Some(300.), Some(50.)),
            _ => (None, None, None, None),
        }
    }
}

fn phases() -> VecDeque<Phase> {
    let mut phases = VecDeque::from([
        Phase::Split,
        Phase::PeerScroll,
        Phase::Select,
        Phase::Insert,
        Phase::Delete,
        Phase::Newline,
        Phase::Join,
        Phase::BurstInsert,
        Phase::BurstDelete,
    ]);
    phases.extend(std::iter::repeat_n(Phase::Scroll, 30));
    phases.extend([Phase::Resize, Phase::Font]);
    phases.extend(std::iter::repeat_n(Phase::Forced, 30));
    phases.push_back(Phase::Idle);
    phases
}

#[derive(Clone)]
struct Target {
    id: EntityId,
    stamp: EditStamp,
    bytes: usize,
    max_width: Option<f32>,
    max_height: Option<f32>,
    font_size: f32,
    scroll: Option<f32>,
}

impl Target {
    fn matches(&self, body: &Body) -> bool {
        let p = body.paint;
        // Peer input sessions renew when deferred shared-buffer observers run.
        body.id == self.id
            && p.stamp.display.committed == self.stamp.display.committed
            && p.stamp.point == self.stamp.point
            && p.stamp.mark == self.stamp.mark
            && p.stamp.mark_active == self.stamp.mark_active
            && p.bytes == self.bytes
            && p.compatible
            && !p.marked
            && p.visible > 0
            && self.max_width.is_none_or(|width| p.width < width - 1.)
            && self.max_height.is_none_or(|height| p.height < height - 1.)
            && (body.font_size - self.font_size).abs() < 0.01
            && self.scroll.is_none_or(|scroll| p.scroll > scroll)
    }
}

struct Reading {
    phase: Phase,
    start: Instant,
    selected: EntityId,
    targets: Vec<Target>,
    action_ms: f64,
    first_frame: Option<PresentedFrame>,
    first: Option<PresentedFrame>,
    ready: Option<PresentedFrame>,
    all_ready: Option<PresentedFrame>,
    scene: Option<Scene>,
    frames: usize,
    missing: usize,
    blank: usize,
    max_draw_ms: f64,
    idle_ticks: usize,
}

impl Reading {
    fn new(phase: Phase, start: Instant, selected: EntityId, targets: Vec<Target>) -> Self {
        Self {
            phase,
            start,
            selected,
            targets,
            action_ms: 0.,
            first_frame: None,
            first: None,
            ready: None,
            all_ready: None,
            scene: None,
            frames: 0,
            missing: 0,
            blank: 0,
            max_draw_ms: 0.,
            idle_ticks: 0,
        }
    }

    fn accept(&mut self, frame: PresentedFrame, scene: Option<Scene>) {
        if frame.frame.draw_start < self.start {
            return;
        }
        self.frames += 1;
        self.max_draw_ms = self.max_draw_ms.max(millis(frame.frame.draw_duration()));
        self.first_frame.get_or_insert(frame);
        let Some(scene) = scene else {
            self.missing += 1;
            return;
        };
        let identities_match = scene.selected == self.selected
            && scene.bodies.len() == self.targets.len()
            && self
                .targets
                .iter()
                .all(|t| scene.bodies.iter().filter(|b| b.id == t.id).count() == 1);
        if !identities_match {
            self.missing += 1;
            return;
        }
        self.blank += usize::from(scene.bodies.iter().any(|b| b.paint.visible == 0));
        let matches = |t: &Target| {
            scene
                .bodies
                .iter()
                .find(|b| b.id == t.id)
                .filter(|b| t.matches(b))
        };
        if let Some(body) = self
            .targets
            .iter()
            .find(|t| t.id == self.selected)
            .and_then(matches)
        {
            self.first.get_or_insert(frame);
            if body.paint.ready && body.paint.indexed == body.paint.bytes {
                self.ready.get_or_insert(frame);
            }
        }
        if self
            .targets
            .iter()
            .all(|t| matches(t).is_some_and(|b| b.paint.ready && b.paint.indexed == b.paint.bytes))
        {
            self.all_ready.get_or_insert(frame);
            self.scene = Some(scene);
        }
    }

    fn error(&self, now: Instant, hardware: bool) -> Option<String> {
        if self.missing > 0 {
            return Some("frame lacks exact same-frame view observations".into());
        }
        if self.blank > 0 {
            return Some("blank body frame".into());
        }
        if self.phase == Phase::Idle && self.frames > 0 {
            return Some("idle produced draw".into());
        }
        if now.duration_since(self.start) > Duration::from_secs(30) {
            return Some("observation timeout (not a numeric all-view budget)".into());
        }
        if !hardware {
            return None;
        }
        let (action, first, ready, first_frame) = self.phase.limits();
        for (key, value, limit) in [
            ("action_cpu_ms", self.action_ms, action),
            (
                "first_submit_ms",
                elapsed_or_now(self.first, self.start, now),
                first,
            ),
            (
                "ready_submit_ms",
                elapsed_or_now(self.ready, self.start, now),
                ready,
            ),
            (
                "first_frame_submit_ms",
                elapsed_or_now(self.first_frame, self.start, now),
                first_frame,
            ),
            ("draw_cpu_max_ms", self.max_draw_ms, Some(16.)),
        ] {
            if limit.is_some_and(|limit| value > limit) {
                return Some(format!("{key}={value:.6} > {:.6} ms", limit.unwrap()));
            }
        }
        None
    }

    fn complete(&self) -> bool {
        if self.phase == Phase::Idle {
            self.idle_ticks >= 30
        } else {
            self.all_ready.is_some()
        }
    }
}

fn elapsed_or_now(frame: Option<PresentedFrame>, start: Instant, now: Instant) -> f64 {
    millis(frame.map_or(now, |frame| frame.presentation.present_end) - start)
}

fn cell(frame: Option<PresentedFrame>, start: Instant) -> String {
    frame.map_or_else(String::new, |frame| {
        format!("{:.6}", millis(frame.presentation.present_end - start))
    })
}

fn keys(window: &mut Window, cx: &mut App, sequence: &[&'static str]) {
    for key in sequence {
        window.dispatch_keystroke(Keystroke::parse(key).unwrap(), cx);
    }
}

struct Run {
    shell: Entity<AppShell>,
    views: Rc<RefCell<Vec<Entity<BufferView>>>>,
    paints: Rc<RefCell<VecDeque<Scene>>>,
    collector: ForegroundJournalCollector,
    output: File,
    report: PathBuf,
    hardware: bool,
    succeeded: Rc<Cell<bool>>,
    direction: String,
    current: Option<Reading>,
    phases: VecDeque<Phase>,
    expected: String,
    peer_scroll: Option<f32>,
    original_bytes: usize,
    peer_origin: usize,
    last_scene: Option<Scene>,
}

impl Run {
    fn targets(&self, phase: Phase, cx: &App) -> Vec<Target> {
        let selected = self.shell.read(cx).active_view().entity_id();
        self.views
            .borrow()
            .iter()
            .map(|entity| {
                let view = entity.read(cx);
                let old = self
                    .last_scene
                    .as_ref()
                    .and_then(|s| s.bodies.iter().find(|b| b.id == entity.entity_id()));
                let split_old = self.last_scene.as_ref().and_then(|s| s.bodies.first());
                Target {
                    id: entity.entity_id(),
                    stamp: EditStamp::capture(view),
                    bytes: view.display_snapshot().text().len(),
                    max_width: if phase == Phase::Resize {
                        old.map(|b| b.paint.width)
                    } else if phase == Phase::Split && self.direction == "right" {
                        split_old.map(|b| b.paint.width)
                    } else {
                        None
                    },
                    max_height: if phase == Phase::Split && self.direction == "below" {
                        split_old.map(|b| b.paint.height)
                    } else {
                        None
                    },
                    font_size: if phase == Phase::Font {
                        18.
                    } else {
                        f32::from(
                            view.wrap_index()
                                .map_or(px(16.), |index| index.key().settings.font_size),
                        )
                    },
                    scroll: if matches!(phase, Phase::Scroll | Phase::PeerScroll)
                        && entity.entity_id() == selected
                    {
                        old.map(|b| b.paint.scroll)
                    } else {
                        None
                    },
                }
            })
            .collect()
    }

    fn begin(&mut self, phase: Phase, window: &mut Window, cx: &mut App) {
        let selected = self.shell.read(cx).active_view().clone();
        let point = selected.read(cx).buffer().point().0;
        match phase {
            Phase::Insert => self.expected.insert(point, 'x'),
            Phase::Delete => {
                assert_eq!(self.expected.remove(point - 1), 'x');
            }
            Phase::Newline => self.expected.insert(point, '\n'),
            Phase::Join => {
                assert_eq!(self.expected.remove(point - 1), '\n');
            }
            Phase::BurstInsert => self.expected.insert_str(point, &"x".repeat(32)),
            Phase::BurstDelete => {
                self.expected.replace_range(point - 32..point, "");
            }
            _ => {}
        }
        let start = Instant::now();
        match phase {
            Phase::Split => {
                keys(
                    window,
                    cx,
                    &[
                        "ctrl-x",
                        if self.direction == "right" { "3" } else { "2" },
                        "ctrl-x",
                        "o",
                        "ctrl-f",
                    ],
                );
            }
            Phase::Select => keys(window, cx, &["ctrl-x", "o"]),
            Phase::Insert => keys(window, cx, &["x"]),
            Phase::Delete | Phase::Join => keys(window, cx, &["backspace"]),
            Phase::Newline => keys(window, cx, &["enter"]),
            Phase::BurstInsert | Phase::BurstDelete => {
                for _ in 0..32 {
                    keys(
                        window,
                        cx,
                        &[if phase == Phase::BurstInsert {
                            "x"
                        } else {
                            "backspace"
                        }],
                    );
                }
            }
            Phase::Scroll | Phase::PeerScroll => {
                let bounds = selected.read(cx).viewport_layout().unwrap().bounds();
                window.dispatch_event(
                    PlatformInput::Scroll(ScrollEvent {
                        position: gpui_kit::point(bounds.left() + px(8.), bounds.top() + px(8.)),
                        delta: ScrollDelta::Pixels(gpui_kit::point(px(0.), px(-120.))),
                        modifiers: Default::default(),
                        touch_phase: TouchPhase::Moved,
                        source: ScrollSource::Pointer,
                    }),
                    cx,
                );
            }
            Phase::Resize => {
                let old = window.viewport_size();
                window.resize(size((old.width * 0.75).max(px(320.)), old.height));
            }
            Phase::Font => {
                for view in self.views.borrow().iter() {
                    view.update(cx, |view, cx| {
                        view.set_font(font("monospace").bold(), px(18.), px(27.), cx)
                    });
                }
            }
            Phase::Forced => window.refresh(),
            _ => {}
        }
        let action_ms = millis(start.elapsed());
        if phase == Phase::Split {
            let second = self.shell.read(cx).active_view().clone();
            assert_ne!(selected, second);
            self.views.borrow_mut().push(second);
        }
        let active = self.shell.read(cx).active_view().entity_id();
        let mut reading = Reading::new(phase, start, active, self.targets(phase, cx));
        reading.action_ms = action_ms;
        self.current = Some(reading);
    }

    fn verify(&mut self, reading: &Reading, cx: &App) -> Result<(), String> {
        let views = self.views.borrow();
        let snapshot = views[0].read(cx).buffer().snapshot_id();
        for entity in views.iter() {
            let view = entity.read(cx);
            if view.buffer().snapshot_id() != snapshot
                || view.buffer().text().to_string() != self.expected
            {
                return Err("shared committed text mismatch".into());
            }
        }
        let delta = self.expected.len() - self.original_bytes;
        if views[0].read(cx).buffer().point().0 != delta
            || (views.len() == 2
                && views[1].read(cx).buffer().point().0 != self.peer_origin + delta)
        {
            return Err("independent points did not match the prescribed actions".into());
        }
        let selected = if matches!(reading.phase, Phase::Split | Phase::PeerScroll) {
            1
        } else {
            0
        };
        if reading.selected != views[selected].entity_id() {
            return Err("wrong selected view".into());
        }
        if reading.phase == Phase::PeerScroll {
            self.peer_scroll = reading.scene.as_ref().and_then(|s| {
                s.bodies
                    .iter()
                    .find(|b| b.id == views[1].entity_id())
                    .map(|b| b.paint.scroll)
            });
        }
        if matches!(
            reading.phase,
            Phase::Insert
                | Phase::Delete
                | Phase::Newline
                | Phase::Join
                | Phase::BurstInsert
                | Phase::BurstDelete
        ) {
            let peer = views[1].read(cx);
            if self.peer_scroll
                != peer
                    .viewport_layout()
                    .map(|layout| f32::from(layout.scroll().y))
            {
                return Err("editing unexpectedly scrolled peer view".into());
            }
        }
        Ok(())
    }

    fn write_row(&mut self, reading: &Reading, status: &str) {
        let scene = reading.scene.as_ref().or(self.last_scene.as_ref());
        let min_visible = scene.map_or(0, |s| {
            s.bodies.iter().map(|b| b.paint.visible).min().unwrap_or(0)
        });
        let fully_indexed = scene.map_or(0, |s| {
            s.bodies
                .iter()
                .filter(|b| b.paint.ready && b.paint.compatible && b.paint.indexed == b.paint.bytes)
                .count()
        });
        let selected = scene.and_then(|s| s.bodies.iter().find(|b| b.id == reading.selected));
        writeln!(
            self.output,
            "{},{},{},{:.6},{},{},{},{},{:.6},{},{},{},{},{},{},{},{},{},{},{},{:.6}",
            reading.phase.name(),
            reading.targets.len(),
            reading.selected,
            reading.action_ms,
            cell(reading.first_frame, reading.start),
            cell(reading.first, reading.start),
            cell(reading.ready, reading.start),
            cell(reading.all_ready, reading.start),
            reading.max_draw_ms,
            reading.frames,
            reading.missing,
            reading.blank,
            reading.idle_ticks,
            min_visible,
            fully_indexed,
            self.expected.len(),
            selected.map_or(0., |b| b.paint.width),
            selected.map_or(0., |b| b.paint.height),
            status,
            if status == "ready" || status == "idle" {
                "true"
            } else {
                "false"
            },
            millis(reading.start.elapsed()),
        )
        .unwrap();
        self.output.flush().unwrap();
    }

    fn finish(&mut self, error: Option<String>, cx: &mut App) -> bool {
        let success = error.is_none();
        if let Some(reason) = error {
            if let Some(reading) = self.current.take() {
                self.write_row(&reading, "failed");
            }
            writeln!(self.output, "# stop_reason={reason}").unwrap();
            println!("STOP {reason}; report={}", self.report.display());
        }
        writeln!(self.output, "# completed={success}; selected_budget_passed={}; all_view_budget=not-established; gpu_execution_ms=unmeasured; compositor_presentation_ms=unmeasured",
            if self.hardware { if success { "true" } else { "false" } } else { "not-applicable" }).unwrap();
        self.output.flush().unwrap();
        self.succeeded.set(success);
        if success {
            println!("COMPLETED {}", self.report.display());
        }
        cx.quit();
        false
    }

    fn tick(&mut self, window: &mut Window, cx: &mut App) -> bool {
        let frames = collect_paints(&mut self.collector, &self.paints, window, |scene| scene.at);
        for (frame, scene) in frames {
            if let Some(reading) = &mut self.current {
                reading.accept(frame, scene);
            }
        }
        if let Some(reading) = &mut self.current {
            if reading.phase == Phase::Idle {
                reading.idle_ticks += 1;
            }
            if let Some(reason) = reading.error(Instant::now(), self.hardware) {
                let reason = format!("{}: {reason}", reading.phase.name());
                return self.finish(Some(reason), cx);
            }
        }
        if self.current.as_ref().is_some_and(Reading::complete) {
            let reading = self.current.take().unwrap();
            if let Err(error) = self.verify(&reading, cx) {
                self.current = Some(reading);
                return self.finish(Some(error), cx);
            }
            self.write_row(
                &reading,
                if reading.phase == Phase::Idle {
                    "idle"
                } else {
                    "ready"
                },
            );
            if let Some(scene) = reading.scene {
                self.last_scene = Some(scene);
            }
        }
        if self.current.is_none() {
            if let Some(phase) = self.phases.pop_front() {
                self.begin(phase, window, cx);
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

pub fn run(text: String, sample: String, report: PathBuf, direction: String, allow_software: bool) {
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&report)
        .expect("new split report");
    let buffer = Buffer::new(
        None,
        DecodedFile {
            text: text.clone(),
            format: FileFormat::default(),
        },
    );
    let succeeded = Rc::new(Cell::new(false));
    let outcome = succeeded.clone();
    gpui_kit::application().run(move |cx| {
        semacs::app::init(cx);
        profiler::set_trace_enabled(true);
        let collector = cx.foreground_journal().collector();
        let paints = Rc::new(RefCell::new(VecDeque::new()));
        let views = Rc::new(RefCell::new(Vec::new()));
        let probe_paints = paints.clone();
        let probe_views = views.clone();
        let opened_at = Instant::now();
        let (window, probe) = gpui_kit::open_window(WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(Point::default(), size(px(960.), px(640.))))),
            app_id: Some(format!("semacs-perf-native-{}", std::env::var("SEMACS_PERF_TOKEN").unwrap_or_default())),
            titlebar: Some(TitlebarOptions { title: Some("semacs split performance".into()), ..Default::default() }),
            ..Default::default()
        }, cx, move |window, cx| cx.new(|cx| {
            let shell = cx.new(|cx| AppShell::new(buffer, window, cx));
            probe_views.borrow_mut().push(shell.read(cx).active_view().clone());
            let subscription = cx.observe(&shell, |_, _, cx| cx.notify());
            Probe { shell, views: probe_views, paints: probe_paints, _subscription: subscription }
        })).expect("open native split probe");
        let shell = probe.read(cx).shell.clone();
        window.update(cx, move |_, window, cx| {
            let specs = window.gpu_specs().expect("renderer identity required");
            assert!(allow_software || !specs.is_software_emulated, "hardware GPU required");
            writeln!(output, "# mode=split; direction={direction}; sample={sample}; release={}; graphics={specs:?}", !cfg!(debug_assertions)).unwrap();
            writeln!(output, "# monospace 16px/24px -> bold 18px/27px; 960x640 requested, actual body sizes below; soft wrap enabled; CPU submission NOT screen presentation").unwrap();
            writeln!(output, "phase,views,selected_view,action_cpu_ms,first_frame_submit_ms,first_submit_ms,ready_submit_ms,all_ready_submit_ms,draw_cpu_max_ms,frames,missing_observations,blank_frames,idle_ticks,min_visible_rows,fully_indexed_views,bytes,width_px,height_px,status,shared_text_verified,observed_elapsed_ms").unwrap();
            output.flush().unwrap();
            let mut run = Run {
                shell, views, paints, collector, output, report,
                hardware: !specs.is_software_emulated, succeeded: outcome, direction,
                current: None, phases: phases(), original_bytes: text.len(), peer_origin: text.chars().next().expect("nonempty sample").len_utf8(), expected: text, peer_scroll: None, last_scene: None,
            };
            run.current = Some(Reading::new(Phase::Boot, opened_at,
                run.shell.read(cx).active_view().entity_id(), run.targets(Phase::Boot, cx)));
            schedule(Rc::new(RefCell::new(run)), window);
        }).unwrap();
    });
    if !succeeded.get() {
        std::process::exit(2);
    }
}
