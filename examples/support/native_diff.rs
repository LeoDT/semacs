use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui_kit::profiler::journal::{ForegroundJournalCollector, PresentedFrame};
use gpui_kit::*;

use super::collect_paints;
#[path = "diff_frames.rs"]
mod frames;
#[cfg(test)]
#[path = "native_diff_tests.rs"]
mod tests;
#[path = "diff_workload.rs"]
mod workload;

use frames::{FrameLog, Scope};

struct Probe {
    shell: Entity<semacs::app::AppShell>,
    views: Rc<RefCell<Vec<Entity<semacs::view::BufferView>>>>,
    paints: Rc<RefCell<VecDeque<workload::Scene>>>,
    _subscription: Subscription,
}

impl Render for Probe {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let views = self.views.clone();
        let shell = self.shell.clone();
        let paints = self.paints.clone();
        div().size_full().child(self.shell.clone()).child(
            canvas(
                |_, _, _| (),
                move |_, _, _, cx| {
                    paints
                        .borrow_mut()
                        .push_back(workload::scene(&shell, &views.borrow(), cx));
                },
            )
            .absolute()
            .size_full(),
        )
    }
}

struct Run {
    work: workload::Workload,
    views: Rc<RefCell<Vec<Entity<semacs::view::BufferView>>>>,
    reading: Option<workload::Reading>,
    collector: ForegroundJournalCollector,
    paints: Rc<RefCell<VecDeque<workload::Scene>>>,
    succeeded: Rc<Cell<bool>>,
    boot: bool,
    prepared: bool,
    diagnostic: bool,
    budget_passed: bool,
    frames: FrameLog,
    setup_started: Instant,
}

impl Run {
    fn begin(&mut self, window: &mut Window, cx: &mut App) {
        self.reading = Some(self.work.begin(window, cx));
        self.views.borrow_mut().clone_from(&self.work.views);
    }

    fn stop(&mut self, error: Option<String>, cx: &mut App) -> bool {
        if error.is_some()
            && let Some(reading) = self.reading.take()
        {
            let violations = reading.budget_violations();
            let _ = self.work.record(&reading, false, &violations);
        }
        self.frames.finish(error.is_none());
        self.work.context(&format!(
            "budget_passed={}",
            self.budget_passed && error.is_none()
        ));
        self.work.finish(error.as_deref());
        self.succeeded.set(error.is_none());
        cx.quit();
        false
    }

    fn accept_frame(
        &mut self,
        frame: PresentedFrame,
        scene: Option<workload::Scene>,
    ) -> Result<(), String> {
        let scope = if let Some(reading) = &self.reading {
            Scope {
                step: reading.step as i32,
                kind: reading.kind,
                phase: if frame.frame.draw_start >= reading.start {
                    reading.phase
                } else {
                    "before-action"
                },
                action: frame.frame.draw_start >= reading.start,
                start: (frame.frame.draw_start >= reading.start).then_some(reading.start),
            }
        } else {
            Scope {
                step: if self.boot { self.work.step as i32 } else { -1 },
                kind: if !self.boot {
                    "none"
                } else if self.work.step < workload::ACTIONS.len() {
                    "staged"
                } else {
                    "unstaged"
                },
                phase: if !self.boot {
                    "window-open"
                } else if self.prepared
                    && workload::ACTIONS[self.work.step % workload::ACTIONS.len()] == "open"
                {
                    "command-prepare"
                } else {
                    "between-actions"
                },
                action: false,
                start: None,
            }
        };
        let violations = self.frames.record(scope, frame, scene.as_ref());
        self.budget_passed &= violations.is_empty();
        if scope.action {
            self.reading.as_mut().unwrap().accepts(
                frame.presentation.present_end,
                workload::ms(frame.frame.draw_duration()),
                scene,
            );
        } else {
            let scene = scene.ok_or("missing setup body observation")?;
            if scene.bodies.is_empty() || scene.bodies.iter().any(|b| b.visible == 0) {
                return Err("blank setup body observation".into());
            }
            if scene
                .bodies
                .iter()
                .filter(|b| b.view == scene.selected)
                .count()
                != 1
            {
                return Err("wrong setup selected-view identity".into());
            }
            if scene.bodies.iter().any(|b| !b.exact) {
                return Err("incompatible setup display/projection identity".into());
            }
            self.boot |= scene.bodies.iter().all(|b| b.ready);
        }
        if !self.diagnostic && !violations.is_empty() {
            return Err(format!(
                "{}/{}: {}",
                scope.kind,
                scope.phase,
                violations.join(";")
            ));
        }
        Ok(())
    }

    fn advance(&mut self, window: &mut Window, cx: &mut App) -> bool {
        if self.reading.is_none() && self.setup_started.elapsed() > Duration::from_secs(30) {
            return self.stop(
                Some("setup observation timeout (resource bound, not latency budget)".into()),
                cx,
            );
        }
        if !self.boot {
            return true;
        }
        if let Some(reading) = &mut self.reading {
            if reading.phase == "idle" {
                reading.idle_ticks += 1;
            }
            let error = if self.diagnostic {
                reading.correctness_error()
            } else {
                reading.error(true)
            };
            if let Some(error) = error {
                let reason = format!("{}/{}: {error}", reading.kind, reading.phase);
                return self.stop(Some(reason), cx);
            }
        }
        if self
            .reading
            .as_ref()
            .is_some_and(workload::Reading::complete)
        {
            let reading = self.reading.take().unwrap();
            let result = self.work.verify(&reading, cx);
            self.views.borrow_mut().clone_from(&self.work.views);
            let violations = reading.budget_violations();
            self.budget_passed &= violations.is_empty();
            if let Err(error) = self.work.record(&reading, result.is_ok(), &violations) {
                return self.stop(Some(error), cx);
            }
            if let Err(error) = result {
                return self.stop(Some(error), cx);
            }
            self.work.step += 1;
            self.prepared = false;
            self.setup_started = Instant::now();
        }
        if self.reading.is_none() {
            if self.work.done() {
                return self.stop(None, cx);
            }
            if !self.prepared {
                println!("SETUP step={}", self.work.step);
                use std::io::Write;
                std::io::stdout().flush().unwrap();
                self.work.prepare(window, cx);
                self.prepared = true;
            } else {
                self.begin(window, cx);
            }
        }
        true
    }

    fn tick(&mut self, window: &mut Window, cx: &mut App) -> bool {
        for (frame, scene) in collect_paints(&mut self.collector, &self.paints, window, |s| s.at) {
            if let Err(error) = self.accept_frame(frame, scene) {
                return self.stop(Some(error), cx);
            }
        }
        self.advance(window, cx)
    }
}

fn schedule(run: Rc<RefCell<Run>>, window: &Window) {
    window.on_next_frame(move |window, cx| {
        let more = run.borrow_mut().tick(window, cx);
        if more {
            schedule(run, window);
        }
    });
}

pub fn run(fixture: PathBuf, report: PathBuf, diagnostic: bool) {
    let fixture = fixture.canonicalize().expect("fixture directory");
    let succeeded = Rc::new(Cell::new(false));
    let outcome = succeeded.clone();
    application().run(move |cx| {
        semacs::app::init(cx);
        profiler::set_trace_enabled(true);
        let collector = cx.foreground_journal().collector();
        let paints = Rc::new(RefCell::new(VecDeque::new()));
        let views = Rc::new(RefCell::new(Vec::new()));
        let probe_views = views.clone();
        let probe_paints = paints.clone();
        let buffer = workload::buffer(&fixture);
        println!("SETUP window-open");
        use std::io::Write;
        std::io::stdout().flush().unwrap();
        let opened_at = Instant::now();
        let (window, probe) = open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    Point::default(),
                    size(px(960.), px(640.)),
                ))),
                app_id: Some("semacs-perf-diff".into()),
                titlebar: Some(TitlebarOptions {
                    title: Some("semacs diff performance".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            cx,
            move |window, cx| {
                cx.new(|cx| {
                    let shell = cx.new(|cx| semacs::app::AppShell::new(buffer, window, cx));
                    probe_views
                        .borrow_mut()
                        .push(shell.read(cx).active_view().clone());
                    let subscription = cx.observe(&shell, |_, _, cx| cx.notify());
                    Probe {
                        shell,
                        views: probe_views,
                        paints: probe_paints,
                        _subscription: subscription,
                    }
                })
            },
        )
        .expect("open diff probe");
        let shell = probe.read(cx).shell.clone();
        window
            .update(cx, move |_, window, cx| {
                let specs = window.gpu_specs().expect("renderer identity required");
                println!("GRAPHICS {specs:?}");
                assert!(
                    !specs.is_software_emulated,
                    "hardware GPU required; no software fallback"
                );
                let mut work = workload::Workload::new(shell, fixture, &report, "native", cx);
                work.context(&format!("hardware=true; graphics={specs:?}"));
                work.context(&format!(
                    "diagnostic={diagnostic}; acceptance={}",
                    !diagnostic
                ));
                let frames = FrameLog::new(&report, opened_at);
                let run = Rc::new(RefCell::new(Run {
                    work,
                    views,
                    reading: None,
                    collector,
                    paints,
                    succeeded: outcome,
                    boot: false,
                    prepared: false,
                    diagnostic,
                    budget_passed: true,
                    frames,
                    setup_started: opened_at,
                }));
                schedule(run, window);
            })
            .unwrap();
    });
    if !succeeded.get() {
        std::process::exit(2);
    }
}
