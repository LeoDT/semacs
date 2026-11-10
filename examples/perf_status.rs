//! Production-shell status baselines on TestPlatform / TestTextSystem, without desktop access.
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use gpui_kit::{
    App, Entity, Keystroke, PlatformInput, ScrollDelta, ScrollEvent, ScrollSource, TestAppContext,
    TestDispatcher, TouchPhase, VisualTestContext, Window, point, px, size,
};
use semacs::app::{AppShell, GitWritePhase};
use semacs::buffer::Buffer;
use semacs::files::{DecodedFile, FileFormat};
use semacs::mode::ModeId;
use semacs::status::index::{Operation, Outcome};
use semacs::text::ByteOffset;
use semacs::view::BufferView;

#[path = "support/headless.rs"]
mod headless;

const ACTIONS: &[&str] = &[
    "open",
    "navigate",
    "file-toggle",
    "scroll",
    "split",
    "collapse",
    "refresh",
    "expand",
    "stage",
    "unstage",
    "unsplit",
];
const TARGET: &str = "unstaged-0000.txt";
const SOURCE: &str = "unsaved editor text\n";

fn key(window: &mut Window, cx: &mut App, key: &'static str) {
    window.dispatch_keystroke(Keystroke::parse(key).unwrap(), cx);
}

fn git(repo: &Path, args: &[&str]) -> Vec<u8> {
    let result = Command::new("git")
        .args([
            "--no-optional-locks",
            "--literal-pathspecs",
            "-c",
            "core.fsmonitor=false",
        ])
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .unwrap();
    assert!(result.status.success(), "fixture oracle: {result:?}");
    result.stdout
}

fn line_start(text: &str, marker: &str) -> ByteOffset {
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        if line.trim_end() == marker
            || line.starts_with(marker) && (marker.ends_with('(') || marker == "Status: ")
        {
            return ByteOffset(offset);
        }
        offset += line.len();
    }
    panic!("missing oracle line: {marker}");
}

fn projection(text: &str, collapsed: bool, file_open: bool) -> Vec<Range<ByteOffset>> {
    let mut group_hidden = false;
    let mut body_hidden = true;
    let mut unstaged = false;
    let mut offset = 0;
    let mut result = Vec::new();
    for line in text.split_inclusive('\n') {
        let group = line.starts_with("Untracked files (")
            || line.starts_with("Unstaged changes (")
            || line.starts_with("Staged changes (")
            || line.starts_with("Unmerged paths (");
        let file = [" ?? ", " MM ", " .M ", " M. "]
            .iter()
            .any(|prefix| line.starts_with(prefix));
        let visible = if group {
            unstaged = line.starts_with("Unstaged changes (");
            group_hidden = collapsed && unstaged;
            body_hidden = true;
            true
        } else if file {
            body_hidden = !(file_open && unstaged && line == " MM both-0000.txt\n");
            !group_hidden
        } else if line.starts_with("@@") || line.starts_with('-') || line.starts_with('+') {
            !group_hidden && !body_hidden
        } else {
            true
        };
        if visible {
            result.push(ByteOffset(offset)..ByteOffset(offset + line.len()));
        }
        offset += line.len();
    }
    result.push(ByteOffset(offset)..ByteOffset(offset));
    result
}

struct Workload {
    shell: Entity<AppShell>,
    views: Vec<Entity<BufferView>>,
    source: Entity<BufferView>,
    fixture: PathBuf,
    base: String,
    staged: String,
    output: File,
}

impl Workload {
    fn new(shell: Entity<AppShell>, fixture: PathBuf, report: &Path, cx: &App) -> Self {
        let head = std::fs::read_to_string(fixture.join("head")).unwrap();
        let title = format!(
            "Status: {}\nBranch: main\nHEAD: {}\n",
            fixture.join("repo").to_string_lossy().escape_debug(),
            head.trim_end()
        );
        let base = title.clone() + &std::fs::read_to_string(fixture.join("base.expected")).unwrap();
        let staged = title + &std::fs::read_to_string(fixture.join("stage.expected")).unwrap();
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(report)
            .unwrap();
        writeln!(output, "# TestPlatform / TestTextSystem; release={}; Lilex 16px/24px; requested=960x640; no soft wrap; no explicit font warmup; setup source painted before open; OS cache uncontrolled", !cfg!(debug_assertions)).unwrap();
        writeln!(output, "# first/ready observed after run_until_parked + draw; scheduler can coalesce readiness; command/point preparation outside timers; open uses temporary display, then single status window before remaining actions; probe draw excludes scheduler-internal paints; no native draw/idle measurement").unwrap();
        writeln!(output, "phase,views,action_cpu_ms,first_ms,selected_ready_ms,all_ready_ms,probe_draw_max_ms,frames,min_visible_rows,bytes,projection_rows,write_records,verified").unwrap();
        output.flush().unwrap();
        let source = shell.read(cx).active_view().clone();
        Self {
            shell,
            views: vec![source.clone()],
            source,
            fixture,
            base,
            staged,
            output,
        }
    }

    fn locate(&self, index: usize, marker: &str, window: &mut Window, cx: &mut App) {
        let view = &self.views[index];
        let point = line_start(&view.read(cx).buffer().text().to_string(), marker);
        let switch = self.shell.read(cx).active_view() != view;
        if switch {
            key(window, cx, "ctrl-x");
            key(window, cx, "o");
        }
        assert_eq!(self.shell.read(cx).active_view(), view);
        key(window, cx, "alt-<");
        let limit = view.read(cx).buffer().sections().unwrap().nodes().len();
        for _ in 0..limit {
            if view.read(cx).buffer().point() == point {
                break;
            }
            key(window, cx, "n");
        }
        assert_eq!(view.read(cx).buffer().point(), point);
        if switch {
            key(window, cx, "ctrl-x");
            key(window, cx, "o");
        }
    }

    fn prepare(&self, phase: &str, window: &mut Window, cx: &mut App) {
        match phase {
            "open" => {
                key(window, cx, "alt-x");
                for ch in "magit-status".chars() {
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
            "navigate" => {
                key(window, cx, "ctrl-x");
                key(window, cx, "1");
                self.locate(0, "Unstaged changes (", window, cx);
            }
            "scroll" => self.locate(0, "Status: ", window, cx),
            "collapse" => {
                self.locate(0, " MM both-0001.txt", window, cx);
                self.locate(1, "Unstaged changes (", window, cx);
            }
            "stage" => {
                self.locate(0, " MM both-0001.txt", window, cx);
                self.locate(1, &format!(" .M {TARGET}"), window, cx);
            }
            "unstage" => self.locate(1, &format!(" M. {TARGET}"), window, cx),
            _ => {}
        }
    }

    fn begin(&mut self, phase: &str, window: &mut Window, cx: &mut App) {
        match phase {
            "open" => key(window, cx, "enter"),
            "navigate" => key(window, cx, "n"),
            "file-toggle" | "collapse" | "expand" => key(window, cx, "tab"),
            "refresh" => key(window, cx, "g"),
            "stage" => key(window, cx, "s"),
            "unstage" => key(window, cx, "u"),
            "scroll" => {
                let view = self.shell.read(cx).active_view().read(cx);
                let bounds = view.paint_layout().unwrap().bounds();
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
                assert_ne!(self.views[0], second);
                self.views.push(second);
            }
            "unsplit" => {
                for k in ["ctrl-x", "1"] {
                    key(window, cx, k);
                }
                self.views = vec![self.shell.read(cx).active_view().clone()];
            }
            _ => unreachable!(),
        }
    }

    fn expected(&self, phase: &str) -> &str {
        if phase == "stage" {
            &self.staged
        } else {
            &self.base
        }
    }

    fn verify(&self, phase: &str, cx: &App) {
        let collapsed = matches!(phase, "collapse" | "refresh");
        let expected = self.expected(phase);
        let rows = projection(expected, collapsed, !matches!(phase, "open" | "navigate"));
        let selected = self.shell.read(cx).active_view();
        let snapshot = selected.read(cx).buffer().snapshot_id();
        for entity in &self.views {
            let view = entity.read(cx);
            let buffer = view.buffer();
            assert_eq!(buffer.mode(), ModeId::MAGIT_STATUS);
            assert!(buffer.is_read_only() && !buffer.is_modified() && !buffer.git_is_stale());
            assert_eq!(buffer.snapshot_id(), snapshot);
            assert_eq!(buffer.text().to_string(), expected);
            let sections = buffer.sections().unwrap();
            assert_eq!(sections.visible_lines().as_ref(), rows);
            let index = view.wrap_index().unwrap();
            assert_eq!(
                index.key().settings.visible_lines.as_deref(),
                Some(rows.as_slice())
            );
            assert_eq!(index.row_count(), rows.len());
            let marker = match phase {
                "navigate" | "file-toggle" => Some(" MM both-0000.txt"),
                "collapse" | "refresh" | "expand" => Some("Unstaged changes ("),
                "stage" if entity == selected => Some("Unstaged changes ("),
                "unstage" if entity == selected => Some("Staged changes ("),
                "stage" | "unstage" => Some(" MM both-0001.txt"),
                _ => None,
            };
            if let Some(marker) = marker {
                assert_eq!(buffer.point(), line_start(expected, marker));
            }
        }
        if matches!(phase, "stage" | "unstage") {
            let writes: Vec<_> = self.shell.read(cx).git_write_records().collect();
            assert_eq!(writes.len(), if phase == "stage" { 1 } else { 2 });
            for (i, record) in writes.iter().enumerate() {
                assert_eq!(record.phase, GitWritePhase::Complete);
                assert_eq!(record.outcome, Some(Outcome::Succeeded));
                assert_eq!(record.verified, Some(true));
                assert!(record.installed);
                assert_eq!(record.path, Path::new(TARGET));
                assert_eq!(
                    record.operation,
                    if i == 0 {
                        Operation::Stage
                    } else {
                        Operation::Unstage
                    }
                );
            }
            let state = if phase == "stage" { "stage" } else { "base" };
            assert_eq!(
                git(&self.fixture.join("repo"), &["ls-files", "--stage", "-z"]),
                std::fs::read(self.fixture.join(format!("{state}.index"))).unwrap()
            );
        }
        assert_eq!(self.source.read(cx).buffer().text().to_string(), SOURCE);
    }

    fn measure(&mut self, phase: &str, cx: &mut VisualTestContext) -> Result<(), String> {
        cx.update(|window, cx| self.prepare(phase, window, cx));
        headless::draw(cx, false);
        let before = cx.read(|cx| {
            self.shell
                .read(cx)
                .active_view()
                .read(cx)
                .buffer()
                .snapshot_id()
        });
        let start = Instant::now();
        cx.update(|window, cx| self.begin(phase, window, cx));
        let action_ms = headless::ms(start);
        let mut first = None;
        let mut selected_ready = None;
        let mut all_ready = None;
        let mut max_draw: f64 = 0.;
        let mut frames = 0;
        let (bytes, rows, visible) = loop {
            cx.run_until_parked();
            let draw_start = Instant::now();
            headless::draw(cx, false);
            max_draw = max_draw.max(headless::ms(draw_start));
            frames += 1;
            let (front, ready, all, bytes, rows, visible) = cx.read(|cx| {
                if phase == "open" {
                    self.views = vec![self.shell.read(cx).active_view().clone()];
                }
                let selected = self.shell.read(cx).active_view();
                let wanted_writes = if phase == "stage" {
                    1
                } else if phase == "unstage" {
                    2
                } else {
                    0
                };
                let writes_ready = wanted_writes == 0 || {
                    let writes: Vec<_> = self.shell.read(cx).git_write_records().collect();
                    writes.len() == wanted_writes
                        && writes.iter().all(|r| r.phase == GitWritePhase::Complete)
                };
                let sample = |entity: &Entity<BufferView>| {
                    let view = entity.read(cx);
                    let buffer = view.buffer();
                    let display = view.display_snapshot();
                    let index = view.wrap_index().unwrap();
                    let layout = view.paint_layout().unwrap();
                    let exact = buffer.mode() == ModeId::MAGIT_STATUS
                        && !buffer.git_is_stale()
                        && writes_ready
                        && (!matches!(phase, "open" | "refresh" | "stage" | "unstage")
                            || buffer.snapshot_id() != before)
                        && layout.key() == index.key()
                        && index.key().display == display.id()
                        && display.id().committed == buffer.snapshot_id()
                        && display.marked_range().is_none()
                        && buffer.sections().is_some_and(|s| {
                            index.key().settings.visible_lines.as_deref()
                                == Some(s.visible_lines().as_ref())
                                && s.nodes()
                                    .iter()
                                    .find(|n| n.key == "unstaged")
                                    .unwrap()
                                    .collapsed
                                    == matches!(phase, "collapse" | "refresh")
                        })
                        && (phase != "scroll" || f32::from(layout.scroll().y) > 0.);
                    (
                        exact,
                        exact
                            && view.layout_ready()
                            && index.indexed_bytes() == display.text().len(),
                        display.text().len(),
                        index.row_count(),
                        layout.visible_row_count(),
                    )
                };
                let (front, ready, bytes, rows, _) = sample(selected);
                let samples: Vec<_> = self.views.iter().map(sample).collect();
                let visible = samples.iter().map(|s| s.4).min().unwrap();
                assert!(visible > 0, "blank body frame");
                (
                    front,
                    ready,
                    samples.iter().all(|s| s.1),
                    bytes,
                    rows,
                    visible,
                )
            });
            let now = headless::ms(start);
            if front {
                first.get_or_insert(now);
            }
            if ready {
                selected_ready.get_or_insert(now);
            }
            if all {
                all_ready.get_or_insert(now);
                break (bytes, rows, visible);
            }
            if start.elapsed() > Duration::from_secs(30) {
                writeln!(
                    self.output,
                    "# failed_phase={phase}; safety timeout, not a latency budget"
                )
                .unwrap();
                self.output.flush().unwrap();
                return Err(format!("{phase}: observation timeout"));
            }
            std::thread::sleep(Duration::from_millis(1));
        };
        cx.read(|cx| self.verify(phase, cx));
        let writes = cx.read(|cx| self.shell.read(cx).git_write_records().count());
        writeln!(self.output, "{phase},{},{action_ms:.6},{:.6},{:.6},{:.6},{max_draw:.6},{frames},{visible},{bytes},{rows},{writes},true",
                 self.views.len(), first.unwrap(), selected_ready.unwrap(), all_ready.unwrap()).unwrap();
        self.output.flush().unwrap();
        Ok(())
    }
}

fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    assert_eq!(args.len(), 2, "perf_status FIXTURE NEW_REPORT");
    let fixture = PathBuf::from(&args[0]).canonicalize().unwrap();
    let report = PathBuf::from(&args[1]);
    let dispatcher = TestDispatcher::new(0);
    let mut app = TestAppContext::build(dispatcher.clone(), Some("perf_status"));
    app.background_executor.allow_parking();
    app.update(semacs::app::init);
    let buffer = Buffer::new(
        Some(fixture.join("repo").join(TARGET)),
        DecodedFile {
            text: SOURCE.into(),
            format: FileFormat::default(),
        },
    );
    let window = app.open_window(size(px(960.), px(640.)), |window, cx| {
        AppShell::new(buffer, window, cx)
    });
    let shell = window.entity(&app).unwrap();
    let mut cx = VisualTestContext::from_window(window.into(), &app);
    headless::draw(&mut cx, false);
    let source = cx.read(|cx| shell.read(cx).active_view().clone());
    headless::ready(&mut cx, &source);
    let mut work = cx.read(|cx| Workload::new(shell, fixture, &report, cx));
    let result = ACTIONS
        .iter()
        .try_for_each(|phase| work.measure(phase, &mut cx));
    writeln!(
        work.output,
        "# completed={}; baseline_only=true",
        result.is_ok()
    )
    .unwrap();
    work.output.flush().unwrap();
    cx.run_until_parked();
    cx.update(|_, cx| {
        cx.background_executor().forbid_parking();
        cx.quit();
    });
    cx.run_until_parked();
    drop(work);
    drop(cx);
    drop(app);
    dispatcher.drain_tasks();
    dispatcher.scheduler().end_test();
    if let Err(error) = result {
        eprintln!("STOP: {error}");
        std::process::exit(2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projection_oracle_hides_default_files_but_preserves_expansion_through_group_toggle() {
        let text = "Unstaged changes (2)\n MM both-0000.txt\n@@ -1 +1 @@\n-INDEX intermediate\n+WORKTREE current\n .M unstaged-0000.txt\n@@ -1 +1 @@\n-HEAD base\n+WORKTREE current\nStaged changes (1)\n MM both-0000.txt\n@@ -1 +1 @@\n-HEAD base\n+INDEX intermediate\nUnmerged paths (0)\n";
        let rows = |collapsed, file_open| {
            projection(text, collapsed, file_open)
                .into_iter()
                .map(|row| &text[row.start.0..row.end.0])
                .collect::<Vec<_>>()
        };
        assert_eq!(rows(false, false).len(), 7);
        assert_eq!(rows(false, true).len(), 10);
        assert_eq!(rows(true, true).len(), 5);
        assert!(rows(false, true).contains(&"-INDEX intermediate\n"));
        assert!(!rows(false, true).contains(&"-HEAD base\n"));
    }

    #[test]
    fn projection_oracle_preserves_headings_empty_groups_and_final_empty_row() {
        let text = "Status: /tmp\nBranch: main\nHEAD: abc\nUntracked files (1)\n ?? a\nUnstaged changes (2)\n .M b\n MM c\nStaged changes (1)\n MM c\nUnmerged paths (0)\n";
        let expanded = projection(text, false, false);
        let collapsed = projection(text, true, false);
        assert_eq!(expanded.len(), 12);
        assert_eq!(collapsed.len(), 10);
        for row in collapsed {
            assert!(expanded.contains(&row));
            assert!(
                !matches!(&text[row.start.0..row.end.0], " .M b\n" | " MM c\n")
                    || row.start > line_start(text, "Staged changes (")
            );
        }
        assert_eq!(
            expanded.last().unwrap(),
            &(ByteOffset(text.len())..ByteOffset(text.len()))
        );
    }
}
