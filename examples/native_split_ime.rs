//! Disposable native IME integration probe. Not a performance measurement.
//! Driven only by scripts/split_ime.py after explicit desktop consent.

use std::cell::{Cell, RefCell};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use gpui_kit::component::ActiveTheme;
use gpui_kit::*;
use semacs::app::AppShell;
use semacs::buffer::{Buffer, SnapshotId};
use semacs::files;
use semacs::view::BufferView;

const INITIAL: &str = "alpha\nsecond\n";
const ONCE: &str = "你alpha\nsecond\n";
const TWICE: &str = "你你alpha\nsecond\n";
const PHASES: [&str; 18] = [
    "open",
    "split",
    "right-preedit",
    "right-cancel",
    "right-preedit",
    "right-commit",
    "left",
    "left-preedit",
    "left-cancel",
    "left-preedit",
    "left-commit",
    "minibuffer",
    "mini-preedit",
    "mini-cancel",
    "mini-preedit",
    "mini-commit",
    "restore",
    "save",
];

struct ViewState {
    snapshot: SnapshotId,
    text: String,
    point: usize,
    marked: bool,
    marked_text: String,
    ready: bool,
    focused: bool,
    caret_in_target: bool,
    bounds: Option<Bounds<Pixels>>,
}

impl ViewState {
    fn capture(view: &BufferView, window: &Window, cx: &App) -> Self {
        let display = view.display_snapshot();
        let layout = view.viewport_layout();
        let caret = layout.and_then(|layout| {
            let point = display.selected_range().end;
            layout.bounds_for_range(point..point, view.wrap_index()?)
        });
        Self {
            snapshot: view.buffer().snapshot_id(),
            text: view.buffer().text().to_string(),
            point: view.buffer().point().0,
            marked: display.marked_range().is_some(),
            marked_text: display.marked_range().map_or_else(String::new, |range| {
                display
                    .text()
                    .chunks_in_range(range.start.0..range.end.0)
                    .collect()
            }),
            ready: view.layout_ready(),
            focused: view.focus_handle(cx).is_focused(window),
            caret_in_target: layout
                .zip(caret)
                .is_some_and(|(layout, caret)| layout.bounds().contains(&caret.origin)),
            bounds: layout.map(|layout| layout.bounds()),
        }
    }
}

struct State {
    first: ViewState,
    second: Option<ViewState>,
    mini: Option<(ViewState, String)>,
    selected_first: bool,
}

impl State {
    fn matches(&self, step: usize) -> bool {
        let expected = if step < 5 {
            INITIAL
        } else if step < 10 {
            ONCE
        } else {
            TWICE
        };
        if !self.first.ready || self.first.text != expected {
            return false;
        }
        if self.first.marked && PHASES[step] != "left-preedit" {
            return false;
        }
        if step == 0 {
            return self.second.is_none() && self.first.focused && self.first.point == 0;
        }
        let Some(second) = &self.second else {
            return false;
        };
        if second.marked && PHASES[step] != "right-preedit" {
            return false;
        }
        if self.mini.is_some() && !(11..=15).contains(&step) {
            return false;
        }
        if !second.ready || second.text != expected || second.snapshot != self.first.snapshot {
            return false;
        }
        let Some((left, right)) = self.first.bounds.zip(second.bounds) else {
            return false;
        };
        if left.right() >= right.left() {
            return false;
        }
        if (step <= 5 && self.selected_first) || (step >= 6 && !self.selected_first) {
            return false;
        }
        match PHASES[step] {
            "split" => self.first.point == 0 && second.point == 0 && second.focused,
            "right-preedit" => {
                second.focused
                    && second.marked
                    && second.marked_text == "ni"
                    && second.caret_in_target
                    && !self.first.marked
                    && self.first.point == 0
            }
            "right-cancel" => second.focused && !second.marked && second.point == 0,
            "right-commit" => {
                second.focused && !second.marked && second.point == 3 && self.first.point == 0
            }
            "left" => self.first.focused && self.first.point == 0 && second.point == 3,
            "left-preedit" => {
                self.first.focused
                    && self.first.marked
                    && self.first.marked_text == "ni"
                    && self.first.caret_in_target
                    && !second.marked
                    && second.point == 3
            }
            "left-cancel" => self.first.focused && !self.first.marked && self.first.point == 0,
            "left-commit" => {
                self.first.focused
                    && !self.first.marked
                    && self.first.point == 3
                    && second.point == 6
            }
            "minibuffer" | "mini-preedit" | "mini-cancel" | "mini-commit" => {
                let Some((mini, content)) = &self.mini else {
                    return false;
                };
                if !mini.ready
                    || !mini.focused
                    || self.first.focused
                    || second.focused
                    || self.first.marked
                    || second.marked
                    || self.first.point != 3
                    || second.point != 6
                {
                    return false;
                }
                match PHASES[step] {
                    "mini-preedit" => {
                        mini.marked
                            && mini.marked_text == "ni"
                            && mini.caret_in_target
                            && content.is_empty()
                    }
                    "mini-commit" => !mini.marked && content == "你",
                    _ => !mini.marked && content.is_empty(),
                }
            }
            "restore" | "save" => {
                self.mini.is_none()
                    && self.first.focused
                    && !self.first.marked
                    && !second.marked
                    && self.first.point == 3
                    && second.point == 6
            }
            _ => false,
        }
    }
}

fn hex(text: &str) -> String {
    use std::fmt::Write;
    let mut encoded = String::with_capacity(text.len() * 2);
    for byte in text.bytes() {
        write!(&mut encoded, "{byte:02x}").unwrap();
    }
    encoded
}

fn keys(window: &mut Window, cx: &mut App, sequence: &[&'static str]) {
    for key in sequence {
        window.dispatch_keystroke(Keystroke::parse(key).unwrap(), cx);
    }
}

fn instruction(phase: &str) -> &'static str {
    match phase {
        "right-preedit" => "右侧正文：启用中文拼音，输入 ni，等待下一条提示（暂不提交）",
        "left-preedit" => "左侧正文：启用中文拼音，输入 ni，等待下一条提示（暂不提交）",
        "mini-preedit" => "minibuffer：启用中文拼音，输入 ni，等待下一条提示（暂不提交）",
        "right-cancel" | "left-cancel" | "mini-cancel" => {
            "请按输入法的 Escape 取消预编辑，然后等待下一条提示"
        }
        "right-commit" | "left-commit" | "mini-commit" => {
            "请明确选择并提交“你”（通常按空格），然后等待下一条提示"
        }
        _ => "正在自动检查分屏 / 输入目标 / 保存，请勿额外编辑或关闭窗口",
    }
}

struct Probe {
    shell: Entity<AppShell>,
    instruction: &'static str,
    _subscription: Subscription,
}

impl Render for Probe {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(56.))
                    .flex_shrink_0()
                    .px_4()
                    .flex()
                    .items_center()
                    .bg(cx.theme().accent)
                    .text_color(cx.theme().accent_foreground)
                    .child(self.instruction),
            )
            .child(div().flex_1().min_h_0().child(self.shell.clone()))
    }
}

struct Run {
    probe: Entity<Probe>,
    shell: Entity<AppShell>,
    first: Entity<BufferView>,
    second: Option<Entity<BufferView>>,
    commands: mpsc::Receiver<String>,
    report: File,
    step: usize,
    started: Option<Instant>,
    waiting: Instant,
    succeeded: Rc<Cell<bool>>,
}

impl Run {
    fn capture(&self, window: &Window, cx: &App) -> State {
        let mini = self.shell.read(cx).minibuffer().map(|mini| {
            let mini = mini.read(cx);
            (
                ViewState::capture(mini.editor().read(cx), window, cx),
                mini.contents(cx),
            )
        });
        State {
            first: ViewState::capture(self.first.read(cx), window, cx),
            second: self
                .second
                .as_ref()
                .map(|view| ViewState::capture(view.read(cx), window, cx)),
            mini,
            selected_first: self.shell.read(cx).active_view() == &self.first,
        }
    }

    fn begin(&mut self, phase: &str, window: &mut Window, cx: &mut App) {
        assert!(self.step < PHASES.len(), "expected done");
        assert_eq!(phase, PHASES[self.step], "out-of-order external command");
        self.probe.update(cx, |probe, cx| {
            probe.instruction = instruction(phase);
            cx.notify();
        });
        match phase {
            "split" => {
                keys(window, cx, &["ctrl-x", "3", "ctrl-x", "o"]);
                self.second = Some(self.shell.read(cx).active_view().clone());
                assert_ne!(self.second.as_ref().unwrap(), &self.first);
            }
            "left" => keys(window, cx, &["ctrl-x", "o"]),
            "minibuffer" => keys(window, cx, &["ctrl-x", "b"]),
            "restore" => keys(window, cx, &["ctrl-g"]),
            "save" => keys(window, cx, &["ctrl-x", "ctrl-s"]),
            _ => {}
        }
        self.started = Some(Instant::now());
        println!("ARMED {phase}");
        std::io::stdout().flush().unwrap();
    }

    fn tick(&mut self, window: &mut Window, cx: &mut App) -> bool {
        if let Some(start) = self.started {
            assert!(
                start.elapsed() < Duration::from_secs(30),
                "timeout in {}: expected target state was not observed",
                PHASES[self.step]
            );
            let state = self.capture(window, cx);
            let saved = PHASES[self.step] != "save" || !self.first.read(cx).buffer().is_modified();
            if state.matches(self.step) && saved {
                let second_point = state.second.as_ref().map_or(0, |view| view.point);
                let mini_content = state
                    .mini
                    .as_ref()
                    .map_or("", |(_, content)| content.as_str());
                writeln!(
                    self.report,
                    "{},{},{},{},{},{},true",
                    self.step,
                    PHASES[self.step],
                    state.first.point,
                    second_point,
                    hex(&state.first.text),
                    hex(mini_content)
                )
                .unwrap();
                self.report.flush().unwrap();
                println!("DONE {}", PHASES[self.step]);
                println!("READY");
                std::io::stdout().flush().unwrap();
                self.step += 1;
                self.started = None;
                self.waiting = Instant::now();
            }
            return true;
        }
        assert!(
            self.waiting.elapsed() < Duration::from_secs(60),
            "driver timeout"
        );
        match self.commands.try_recv() {
            Ok(command) if command == "done" => {
                assert_eq!(self.step, PHASES.len(), "cannot finish an incomplete run");
                writeln!(
                    self.report,
                    "# completed=true; performance=unmeasured; candidate_visual=unreviewed"
                )
                .unwrap();
                self.report.flush().unwrap();
                self.succeeded.set(true);
                println!("COMPLETED");
                std::io::stdout().flush().unwrap();
                cx.quit();
                false
            }
            Ok(command) => {
                self.begin(&command, window, cx);
                true
            }
            Err(mpsc::TryRecvError::Empty) => true,
            Err(mpsc::TryRecvError::Disconnected) => panic!("driver disconnected"),
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

fn main() {
    let mut args = std::env::args_os().skip(1);
    let input = PathBuf::from(args.next().expect("expected disposable INPUT"));
    let report = PathBuf::from(args.next().expect("expected NEW_REPORT"));
    assert!(args.next().is_none(), "expected INPUT NEW_REPORT");
    let token = std::env::var("SEMACS_PERF_TOKEN").expect("use scripts/split_ime.py");
    assert!(!token.is_empty());
    let loaded = files::load(&input).expect("load disposable fixture");
    assert_eq!(loaded.file.text, INITIAL, "unexpected fixture");
    let buffer = Buffer::visiting(input, loaded.file, loaded.stamp);
    let mut report = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(report)
        .expect("refusing to overwrite report");
    writeln!(
        report,
        "step,phase,first_point,second_point,body_hex,mini_hex,assertions_passed"
    )
    .unwrap();
    let (tx, commands) = mpsc::channel();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            if tx.send(line.expect("read command")).is_err() {
                break;
            }
        }
    });
    let succeeded = Rc::new(Cell::new(false));
    let outcome = succeeded.clone();
    gpui_kit::application().run(move |cx| {
        semacs::app::init(cx);
        let (window, probe) = gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    Point::default(),
                    size(px(960.), px(640.)),
                ))),
                window_min_size: Some(size(px(480.), px(320.))),
                app_id: Some(format!("semacs-perf-native-{token}")),
                titlebar: Some(TitlebarOptions {
                    title: Some("semacs split IME check".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            cx,
            move |window, cx| {
                cx.new(|cx| {
                    let shell = cx.new(|cx| AppShell::new(buffer, window, cx));
                    let subscription = cx.observe(&shell, |_, _, cx| cx.notify());
                    Probe {
                        shell,
                        instruction: instruction("open"),
                        _subscription: subscription,
                    }
                })
            },
        )
        .expect("open integration probe");
        let shell = probe.read(cx).shell.clone();
        let first = shell.read(cx).active_view().clone();
        window
            .update(cx, move |_, window, _| {
                let graphics = window.gpu_specs().expect("renderer identity");
                assert!(
                    !graphics.is_software_emulated,
                    "hardware desktop required; no software fallback"
                );
                writeln!(
                    report,
                    "# hardware=true; graphics={graphics:?}; performance=unmeasured"
                )
                .unwrap();
                let run = Rc::new(RefCell::new(Run {
                    probe,
                    shell,
                    first,
                    second: None,
                    commands,
                    report,
                    step: 0,
                    started: Some(Instant::now()),
                    waiting: Instant::now(),
                    succeeded: outcome,
                }));
                schedule(run, window);
            })
            .expect("start integration probe");
    });
    assert!(
        succeeded.get(),
        "probe closed before completing integration checks"
    );
}

#[cfg(test)]
#[path = "support/split_ime_tests.rs"]
mod tests;
