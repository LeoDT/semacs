use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gpui_kit::*;
use semacs::app::AppShell;
use semacs::buffer::{Buffer, SnapshotId};
use semacs::files::{DecodedFile, FileFormat};
use semacs::input::DisplayId;
use semacs::mode::ModeId;
use semacs::preparation_trace::{Capture, Recording};
use semacs::view::BufferView;

#[cfg(test)]
#[path = "diff_workload_tests.rs"]
mod tests;

pub const ACTIONS: &[&str] = &[
    "open", "locate", "collapse", "expand", "navigate", "scroll", "split", "locate", "collapse",
    "refresh", "expand", "idle", "unsplit",
];

pub fn ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.
}

pub fn key(window: &mut Window, cx: &mut App, key: &'static str) {
    window.dispatch_keystroke(Keystroke::parse(key).unwrap(), cx);
}

pub fn buffer(fixture: &Path) -> Buffer {
    let path = fixture.join("repo/file-0000.txt");
    Buffer::new(
        Some(path),
        DecodedFile {
            text: "diff performance source\n".into(),
            format: FileFormat::default(),
        },
    )
}

#[derive(Clone, Debug)]
pub struct Body {
    pub view: EntityId,
    pub display: DisplayId,
    pub snapshot: SnapshotId,
    pub kind: String,
    pub exact: bool,
    pub ready: bool,
    pub visible: usize,
    pub rows: usize,
    pub bytes: usize,
    pub collapsed: bool,
    pub point: usize,
    pub hunk: usize,
    pub scroll: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Debug)]
pub struct Scene {
    pub at: Instant,
    pub selected: EntityId,
    pub bodies: Vec<Body>,
}

pub fn scene(shell: &Entity<AppShell>, views: &[Entity<BufferView>], cx: &App) -> Scene {
    let active = [shell.read(cx).active_view().clone()];
    let views = if views.len() == 1 { &active[..] } else { views };
    let bodies = views
        .iter()
        .map(|entity| {
            let view = entity.read(cx);
            let buffer = view.buffer();
            let display = view.display_snapshot();
            let index = view.wrap_index().expect("paint index");
            let layout = view.paint_layout().expect("paint layout");
            let sections = buffer.sections();
            let hunk = sections
                .as_ref()
                .and_then(|s| s.nodes().iter().find(|n| n.key.contains("/@@")));
            let projection = sections.as_ref().map(|s| s.visible_lines());
            let projected = match (&index.key().settings.visible_lines, &projection) {
                (Some(index), Some(current)) => index == current,
                (None, None) => true,
                _ => false,
            };
            Body {
                view: entity.entity_id(),
                display: display.id(),
                snapshot: buffer.snapshot_id(),
                kind: buffer.display_name(),
                exact: display.marked_range().is_none()
                    && display.id().committed == buffer.snapshot_id()
                    && index.key().display == display.id()
                    && layout.key() == index.key()
                    && projected
                    && projection
                        .as_ref()
                        .is_none_or(|p| index.row_count() == p.len()),
                ready: view.layout_ready() && index.indexed_bytes() == display.text().len(),
                visible: layout.visible_row_count(),
                rows: index.row_count(),
                bytes: display.text().len(),
                collapsed: hunk.is_some_and(|h| h.collapsed),
                point: buffer.point().0,
                hunk: hunk.map_or(0, |h| h.range.start.0),
                scroll: f32::from(layout.scroll().y),
                width: f32::from(layout.bounds().size.width),
                height: f32::from(layout.bounds().size.height),
            }
        })
        .collect();
    Scene {
        at: Instant::now(),
        selected: shell.read(cx).active_view().entity_id(),
        bodies,
    }
}

pub struct Reading {
    pub step: usize,
    pub kind: &'static str,
    pub phase: &'static str,
    pub start: Instant,
    pub action_ms: f64,
    pub first: Option<Instant>,
    pub selected_ready: Option<Instant>,
    pub all_ready: Option<Instant>,
    pub max_draw: f64,
    pub frames: usize,
    pub missing: usize,
    pub blank: usize,
    pub idle_ticks: usize,
    pub last: Option<Scene>,
    observed: Option<Scene>,
    selected: EntityId,
    targets: Vec<EntityId>,
    before: SnapshotId,
    before_point: usize,
    before_scroll: f32,
    collapsed: bool,
    capture: Capture,
    _recording: Recording,
}

impl Reading {
    pub fn accepts(&mut self, at: Instant, draw_ms: f64, scene: Option<Scene>) {
        if at < self.start {
            return;
        }
        self.frames += 1;
        self.max_draw = self.max_draw.max(draw_ms);
        let Some(scene) = scene else {
            self.missing += 1;
            return;
        };
        self.observed = Some(scene.clone());
        if self.phase == "open" && scene.bodies.len() == 1 {
            self.selected = scene.selected;
            self.targets = vec![scene.selected];
        }
        if scene.bodies.len() != self.targets.len()
            || scene.selected != self.selected
            || self
                .targets
                .iter()
                .any(|id| scene.bodies.iter().filter(|b| b.view == *id).count() != 1)
        {
            self.missing += 1;
            return;
        }
        self.blank += usize::from(scene.bodies.iter().any(|b| b.visible == 0));
        if self.phase == "idle" {
            if scene
                .bodies
                .iter()
                .all(|b| b.exact && b.ready && b.snapshot == self.before)
            {
                self.last = Some(scene);
            } else {
                self.missing += 1;
            }
            return;
        }
        let matches = |body: &Body| {
            body.exact
                && body
                    .kind
                    .starts_with(&format!("*magit-diff {}:", self.kind))
                && body.collapsed == self.collapsed
                && (self.phase != "refresh"
                    || (body.snapshot.buffer == self.before.buffer && body.snapshot != self.before))
                && (body.view != self.selected
                    || match self.phase {
                        "navigate" => body.point != self.before_point,
                        "locate" => body.point == body.hunk && body.hunk > 0,
                        "scroll" => body.scroll > self.before_scroll,
                        _ => true,
                    })
        };
        if let Some(body) = scene.bodies.iter().find(|b| b.view == self.selected)
            && matches(body)
        {
            self.first.get_or_insert(at);
            if body.ready {
                self.selected_ready.get_or_insert(at);
            }
        }
        if scene.bodies.iter().all(|b| matches(b) && b.ready)
            && scene
                .bodies
                .iter()
                .all(|b| b.snapshot == scene.bodies[0].snapshot)
        {
            self.all_ready.get_or_insert(at);
            self.last = Some(scene);
        }
    }

    pub fn correctness_error(&self) -> Option<String> {
        if self.missing > 0 {
            return Some("missing same-frame body observation".into());
        }
        if self.blank > 0 {
            return Some("blank body frame".into());
        }
        if self.start.elapsed() > Duration::from_secs(30) {
            return Some(format!(
                "stage observation timeout (resource bound, not latency budget); observed={:?}",
                self.observed
            ));
        }
        None
    }

    pub fn budget_violations(&self) -> Vec<String> {
        let mut violations = Vec::new();
        if self.max_draw > 16. {
            violations.push(format!("draw_cpu_max_ms={:.6} >16", self.max_draw));
        }
        if self.phase == "scroll"
            && let Some(first) = self.first
        {
            let elapsed = ms(first.duration_since(self.start));
            if elapsed > 4. {
                violations.push(format!("first_ms={elapsed:.6} >4"));
            }
        }
        if self.phase == "idle" && self.frames > 0 {
            violations.push(format!("idle_frames={} >0", self.frames));
        }
        violations
    }

    pub fn error(&self, native: bool) -> Option<String> {
        if let Some(error) = self.correctness_error() {
            return Some(error);
        }
        if native {
            if let Some(error) = self.budget_violations().into_iter().next() {
                return Some(error);
            }
            if self.phase == "scroll" && self.first.is_none() && ms(self.start.elapsed()) > 4. {
                return Some("scroll first submit not observed within 4 ms".into());
            }
        } else if self.phase == "idle" && self.frames > 0 {
            return Some("idle produced draw".into());
        }
        None
    }

    pub fn complete(&self) -> bool {
        if self.phase == "idle" {
            self.idle_ticks == 30
        } else {
            self.all_ready.is_some()
        }
    }
}

struct ProjectionOracle {
    expanded: Vec<std::ops::Range<semacs::text::ByteOffset>>,
    collapsed: Vec<std::ops::Range<semacs::text::ByteOffset>>,
}

impl ProjectionOracle {
    fn new(title: &str, body: &str, hidden: std::ops::Range<usize>) -> Self {
        use semacs::text::ByteOffset;
        let text = format!("{title}\n{body}");
        let mut offset = 0;
        let mut expanded: Vec<_> = text
            .split_inclusive('\n')
            .map(|line| {
                let start = offset;
                offset += line.len();
                ByteOffset(start)..ByteOffset(offset)
            })
            .collect();
        if text.ends_with('\n') {
            expanded.push(ByteOffset(offset)..ByteOffset(offset));
        }
        assert!(hidden.start < hidden.end && hidden.end + 1 < expanded.len());
        let collapsed = expanded
            .iter()
            .enumerate()
            .filter(|(i, _)| !hidden.contains(&i.saturating_sub(1)) || *i == 0)
            .map(|(_, range)| range.clone())
            .collect();
        Self {
            expanded,
            collapsed,
        }
    }
}

pub struct Workload {
    pub shell: Entity<AppShell>,
    pub views: Vec<Entity<BufferView>>,
    pub step: usize,
    pub fixture: PathBuf,
    expected: [String; 2],
    projections: [ProjectionOracle; 2],
    output: File,
    spans: File,
}

impl Workload {
    pub fn new(
        shell: Entity<AppShell>,
        fixture: PathBuf,
        report: &Path,
        mode: &str,
        cx: &App,
    ) -> Self {
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(report)
            .unwrap();
        let mut spans = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(report.with_extension("spans.csv"))
            .unwrap();
        writeln!(output, "# mode={mode}-diff; release={}; Lilex 16px/24px; 960x640 requested; section projection, no soft wrap; diff latency baseline only; OS cache uncontrolled", !cfg!(debug_assertions)).unwrap();
        writeln!(output, "kind,phase,step,views,action_cpu_ms,first_ms,selected_ready_ms,all_ready_ms,draw_cpu_max_ms,frames,missing_observations,blank_frames,idle_ticks,min_visible_rows,fully_indexed_views,bytes,projection_rows,width_px,height_px,target_display,text_verified,projection_verified,status,observed_elapsed_ms,selected_view,target_point,before_point,before_scroll_y,budget_violations").unwrap();
        writeln!(
            spans,
            "kind,phase,step,id,parent,lane,span,bytes,start_ms,duration_ms,complete"
        )
        .unwrap();
        output.flush().unwrap();
        let kinds = ["staged", "unstaged"];
        let expected = kinds
            .map(|kind| std::fs::read_to_string(fixture.join(format!("{kind}.expected"))).unwrap());
        let projections = std::array::from_fn(|i| {
            let plan =
                std::fs::read_to_string(fixture.join(format!("{}.projection", kinds[i]))).unwrap();
            let rows: Vec<usize> = plan
                .split_whitespace()
                .map(|n| n.parse().unwrap())
                .collect();
            assert_eq!(rows.len(), 2);
            let title = format!(
                "{}: {}",
                kinds[i],
                fixture.join("repo").to_string_lossy().escape_debug()
            );
            ProjectionOracle::new(&title, &expected[i], rows[0]..rows[1])
        });
        Self {
            views: vec![shell.read(cx).active_view().clone()],
            shell,
            fixture,
            step: 0,
            expected,
            projections,
            output,
            spans,
        }
    }

    pub fn context(&mut self, value: &str) {
        writeln!(self.output, "# {value}").unwrap();
        self.output.flush().unwrap();
    }

    pub fn done(&self) -> bool {
        self.step == ACTIONS.len() * 2
    }

    pub fn prepare(&self, window: &mut Window, cx: &mut App) {
        if ACTIONS[self.step % ACTIONS.len()] != "open" {
            return;
        }
        key(window, cx, "alt-x");
        let kind = if self.step < ACTIONS.len() {
            "staged"
        } else {
            "unstaged"
        };
        for ch in format!("magit-diff-{kind}").chars() {
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

    pub fn begin(&mut self, window: &mut Window, cx: &mut App) -> Reading {
        let phase = ACTIONS[self.step % ACTIONS.len()];
        let kind = if self.step < ACTIONS.len() {
            "staged"
        } else {
            "unstaged"
        };
        let selected = self.shell.read(cx).active_view().clone();
        let before = selected.read(cx).buffer().snapshot_id();
        let before_point = selected.read(cx).buffer().point().0;
        let before_scroll = selected
            .read(cx)
            .paint_layout()
            .map_or(0., |l| f32::from(l.scroll().y));
        let collapsed = matches!(phase, "collapse" | "refresh");
        let capture = Capture::default();
        let recording = capture.enter();
        println!("STAGE {} {kind}/{phase}", self.step);
        std::io::stdout().flush().unwrap();
        let start = Instant::now();
        match phase {
            "open" => key(window, cx, "enter"),
            "locate" => {
                for k in ["alt-<", "n", "n", "j", "n"] {
                    key(window, cx, k);
                }
            }
            "navigate" => {
                for k in ["n", "n", "ctrl-n", "ctrl-n", "ctrl-n", "ctrl-n"] {
                    key(window, cx, k);
                }
            }
            "collapse" | "expand" => key(window, cx, "tab"),
            "refresh" => key(window, cx, "g"),
            "scroll" => {
                let bounds = selected.read(cx).viewport_layout().unwrap().bounds();
                window.dispatch_event(
                    PlatformInput::Scroll(ScrollEvent {
                        position: point(bounds.left() + px(8.), bounds.top() + px(8.)),
                        delta: ScrollDelta::Pixels(point(px(0.), px(-120.))),
                        modifiers: Default::default(),
                        touch_phase: TouchPhase::Moved,
                        source: ScrollSource::Pointer,
                    }),
                    cx,
                );
            }
            "split" => {
                for k in ["ctrl-x", "3", "ctrl-x", "o"] {
                    key(window, cx, k);
                }
                let second = self.shell.read(cx).active_view().clone();
                assert_ne!(selected, second);
                self.views.push(second);
            }
            "unsplit" => {
                for k in ["ctrl-x", "1"] {
                    key(window, cx, k);
                }
                self.views = vec![self.shell.read(cx).active_view().clone()];
            }
            "idle" => {}
            _ => unreachable!(),
        }
        let action_ms = ms(start.elapsed());
        Reading {
            step: self.step,
            kind,
            phase,
            start,
            action_ms,
            first: None,
            selected_ready: None,
            all_ready: None,
            max_draw: 0.,
            frames: 0,
            missing: 0,
            blank: 0,
            idle_ticks: 0,
            last: None,
            observed: None,
            selected: self.shell.read(cx).active_view().entity_id(),
            targets: self.views.iter().map(|v| v.entity_id()).collect(),
            before,
            before_point,
            before_scroll,
            collapsed,
            capture,
            _recording: recording,
        }
    }

    pub fn verify(&mut self, reading: &Reading, cx: &App) -> Result<(), String> {
        if self.views.len() == 1 {
            self.views[0] = self.shell.read(cx).active_view().clone();
        }
        let kind = usize::from(reading.kind == "unstaged");
        let expected = &self.expected[kind];
        let projection = if reading.collapsed {
            &self.projections[kind].collapsed
        } else {
            &self.projections[kind].expanded
        };
        for entity in &self.views {
            let view = entity.read(cx);
            let text = view.buffer().text().to_string();
            if view.buffer().mode() != ModeId::MAGIT_DIFF
                || !view.buffer().is_read_only()
                || text.split_once('\n').map(|(_, body)| body) != Some(expected.as_str())
            {
                return Err("wrong diff mode/read-only state/complete oracle text".into());
            }
            if view
                .buffer()
                .sections()
                .is_none_or(|s| s.visible_lines().as_ref() != projection.as_slice())
                || view.wrap_index().is_none_or(|index| {
                    index.key().settings.visible_lines.as_deref() != Some(projection.as_slice())
                        || index.row_count() != projection.len()
                })
            {
                return Err(
                    "section/layout projection differs from independent fixture oracle".into(),
                );
            }
            if reading.phase == "navigate" {
                let point = view.buffer().point();
                let start = text[..point.0].rfind('\n').map_or(0, |i| i + 1);
                let end = text[point.0..]
                    .find('\n')
                    .map_or(text.len(), |i| point.0 + i);
                if end - start < 4000
                    || view
                        .paint_layout()
                        .and_then(|layout| {
                            layout.bounds_for_range(point..point, view.wrap_index().unwrap())
                        })
                        .is_none()
                {
                    return Err("navigation did not expose and shape the long changed line".into());
                }
            }
            let root = self.fixture.join("repo");
            let title = format!(
                "{}: {}",
                reading.kind,
                root.to_string_lossy().escape_debug()
            );
            if text.lines().next() != Some(title.as_str()) {
                return Err("wrong comparison root/kind".into());
            }
        }
        Ok(())
    }

    pub fn record(
        &mut self,
        reading: &Reading,
        success: bool,
        violations: &[String],
    ) -> Result<(), String> {
        let (samples, lost, _) = reading.capture.finish();
        for (id, sample) in samples.iter().enumerate() {
            writeln!(
                self.spans,
                "{},{},{},{},{},{},{},{},{:.6},{},{}",
                reading.kind,
                reading.phase,
                reading.step,
                id,
                sample.parent.map_or_else(String::new, |p| p.to_string()),
                sample.lane,
                sample.phase,
                sample.bytes,
                ms(sample.start.saturating_duration_since(reading.start)),
                sample.end.map_or_else(String::new, |end| format!(
                    "{:.6}",
                    ms(end.duration_since(sample.start))
                )),
                sample.end.is_some()
            )
            .unwrap();
        }
        self.spans.flush().unwrap();
        let body = reading
            .last
            .as_ref()
            .and_then(|s| s.bodies.iter().find(|b| b.view == reading.selected));
        let cell = |at: Option<Instant>| {
            at.map_or_else(String::new, |at| {
                format!("{:.6}", ms(at.duration_since(reading.start)))
            })
        };
        writeln!(self.output, "{},{},{},{},{:.6},{},{},{},{:.6},{},{},{},{},{},{},{},{},{:.3},{:.3},\"{:?}\",{},{},{},{:.6},{},{},{},{:.6},{}",
            reading.kind, reading.phase, reading.step, reading.targets.len(), reading.action_ms,
            cell(reading.first), cell(reading.selected_ready), cell(reading.all_ready), reading.max_draw,
            reading.frames, reading.missing, reading.blank, reading.idle_ticks,
            reading.last.as_ref().map_or(0, |s| s.bodies.iter().map(|b| b.visible).min().unwrap_or(0)),
            reading.last.as_ref().map_or(0, |s| s.bodies.iter().filter(|b| b.exact && b.ready).count()),
            body.map_or(0, |b| b.bytes), body.map_or(0, |b| b.rows), body.map_or(0., |b| b.width), body.map_or(0., |b| b.height),
            body.map(|b| b.display), success, success, if !success { "failed" } else if violations.is_empty() { "ready" } else { "over-budget" }, ms(reading.start.elapsed()), reading.selected, body.map_or(0, |b| b.point), reading.before_point, reading.before_scroll, violations.join(";")).unwrap();
        self.output.flush().unwrap();
        if lost != 0 {
            return Err(format!("preparation trace overflow: lost={lost}"));
        }
        Ok(())
    }

    pub fn finish(&mut self, error: Option<&str>) {
        if let Some(reason) = error {
            writeln!(self.output, "# stop_reason={reason}").unwrap();
            println!("STOP {reason}");
        }
        writeln!(self.output, "# completed={}; diff_latency_budget=not-established; gpu_execution=unmeasured; screen_presentation=unmeasured", error.is_none()).unwrap();
        self.output.flush().unwrap();
    }
}
