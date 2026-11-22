//! Ripgrep (`rg`) project and directory search, streaming JSON execution,
//! section projection, and `grep-mode` / `rg.el`-aligned match navigation.

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read};
use std::ops::Range;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::files;
use crate::section::{Section, Sections};
use crate::text::{ByteOffset, Rope};
use crate::theme::StyleRole;

mod json;

#[cfg(test)]
#[path = "search_tests.rs"]
mod tests;

const MAX_MATCHES: usize = 20_000;
const MAX_DISPLAY_BYTES: usize = 12 * 1024 * 1024;
const MAX_LINE_DISPLAY_BYTES: usize = 1024;
const MAX_STDERR_BYTES: usize = 64 * 1024;
const STREAM_INTERVAL: Duration = Duration::from_millis(100);
const STREAM_BATCH_MATCHES: usize = 200;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CaseMode {
    /// `--smart-case`: case-insensitive when all-lowercase, case-sensitive otherwise.
    #[default]
    Smart,
    /// `--case-sensitive` (`-s`).
    Sensitive,
    /// `--ignore-case` (`-i`).
    Insensitive,
}

impl CaseMode {
    pub fn next(self) -> Self {
        match self {
            Self::Smart => Self::Sensitive,
            Self::Sensitive => Self::Insensitive,
            Self::Insensitive => Self::Smart,
        }
    }

    pub fn flag(self) -> &'static str {
        match self {
            Self::Smart => "--smart-case",
            Self::Sensitive => "--case-sensitive",
            Self::Insensitive => "--ignore-case",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Smart => "smart",
            Self::Sensitive => "sensitive",
            Self::Insensitive => "insensitive",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchQuery {
    pub pattern: String,
    pub directory: PathBuf,
    pub literal: bool,
    pub case_mode: CaseMode,
    pub files_glob: Option<String>,
    pub respect_ignore: bool,
    pub project: Option<PathBuf>,
}

impl SearchQuery {
    pub fn new(pattern: String, directory: PathBuf, project: Option<PathBuf>) -> Self {
        Self {
            pattern,
            directory,
            literal: false,
            case_mode: CaseMode::Smart,
            files_glob: None,
            respect_ignore: true,
            project,
        }
    }

    pub fn globs(&self) -> Vec<String> {
        self.files_glob
            .as_deref()
            .map(parse_globs)
            .unwrap_or_default()
    }

    pub fn build_args(&self) -> Vec<OsString> {
        let mut args = vec![
            OsString::from("--json"),
            OsString::from("--line-number"),
            OsString::from("--column"),
            OsString::from(self.case_mode.flag()),
        ];
        if self.literal {
            args.push(OsString::from("--fixed-strings"));
        }
        if !self.respect_ignore {
            args.push(OsString::from("--no-ignore"));
        }
        for glob in self.globs() {
            args.push(OsString::from("-g"));
            args.push(OsString::from(glob));
        }
        args.push(OsString::from("--"));
        args.push(OsString::from(&self.pattern));
        args.push(OsString::from("."));
        args
    }

    pub fn command_preview(&self) -> String {
        let mut parts = vec!["rg".to_owned(), self.case_mode.flag().to_owned()];
        if self.literal {
            parts.push("--fixed-strings".to_owned());
        }
        if !self.respect_ignore {
            parts.push("--no-ignore".to_owned());
        }
        for glob in self.globs() {
            parts.push("-g".to_owned());
            parts.push(shell_quote(&glob));
        }
        parts.push("--".to_owned());
        parts.push(shell_quote(&self.pattern));
        parts.push(".".to_owned());
        parts.join(" ")
    }
}

pub fn parse_globs(input: &str) -> Vec<String> {
    let mut globs = Vec::new();
    let mut current = String::new();
    let mut brace_depth = 0usize;
    for ch in input.chars() {
        match ch {
            '{' => {
                brace_depth += 1;
                current.push(ch);
            }
            '}' => {
                brace_depth = brace_depth.saturating_sub(1);
                current.push(ch);
            }
            ',' if brace_depth == 0 => {
                let trimmed = current.trim();
                if !trimmed.is_empty() {
                    globs.push(trimmed.to_owned());
                }
                current.clear();
            }
            c if c.is_whitespace() && brace_depth == 0 => {
                let trimmed = current.trim();
                if !trimmed.is_empty() {
                    globs.push(trimmed.to_owned());
                }
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    let trimmed = current.trim();
    if !trimmed.is_empty() {
        globs.push(trimmed.to_owned());
    }
    globs
}

fn shell_quote(value: &str) -> String {
    if !value.is_empty()
        && value.bytes().all(|b| {
            b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b'/' | b':' | b'*')
        })
    {
        value.to_owned()
    } else {
        format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SearchStatus {
    Running,
    Finished { elapsed_ms: u64, truncated: bool },
    Killed,
    Failed(String),
}

impl SearchStatus {
    pub fn is_running(&self) -> bool {
        matches!(self, Self::Running)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Submatch {
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchLine {
    /// 1-based line number in the target file.
    pub line_number: usize,
    /// 0-based UTF-8 byte offset of the first match within the file's line.
    pub match_byte_column: usize,
    /// 1-based character column for display.
    pub display_column: usize,
    /// Sanitized single-line preview text (without trailing newline).
    pub text: String,
    /// Highlight byte ranges within `text` (sorted, non-overlapping, char boundaries).
    pub submatches: Vec<Range<usize>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileGroup {
    pub relative_display: String,
    pub path: PathBuf,
    pub matches: Vec<MatchLine>,
}

#[derive(Clone, Debug)]
pub struct SearchData {
    pub query: SearchQuery,
    pub files: Vec<FileGroup>,
    pub total_matches: usize,
    pub warnings: Vec<String>,
    pub status: SearchStatus,
}

impl SearchData {
    pub fn running(query: SearchQuery) -> Self {
        Self {
            query,
            files: Vec::new(),
            total_matches: 0,
            warnings: Vec::new(),
            status: SearchStatus::Running,
        }
    }

    pub fn render(&self) -> RenderedSearch {
        let mut text = String::new();
        let mut styles: Vec<(Range<ByteOffset>, StyleRole)> = Vec::new();
        let mut nodes = vec![Section {
            key: "root".into(),
            parent: None,
            range: ByteOffset(0)..ByteOffset(0),
            body: ByteOffset(0),
            collapsed: false,
            hunk: false,
        }];
        let mut match_locations = Vec::with_capacity(self.total_matches);
        let mut file_locations = Vec::with_capacity(self.files.len());

        let header_start = text.len();
        let dir_label = files::directory_input(&self.query.directory);
        text.push_str(&format!(
            "-*- mode: rg; default-directory: {} -*-\n",
            escape_single_line(&dir_label)
        ));
        text.push_str(&format!(
            "{}\n",
            escape_single_line(&self.query.command_preview())
        ));
        let files_label = self
            .query
            .files_glob
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or("all");
        text.push_str(&format!(
            "[mode: {} | case: {} | files: {} | ignore: {}]\n",
            if self.query.literal {
                "literal"
            } else {
                "regexp"
            },
            self.query.case_mode.label(),
            escape_single_line(files_label),
            if self.query.respect_ignore {
                "on"
            } else {
                "off"
            },
        ));
        let header_end = text.len();
        styles.push((
            ByteOffset(header_start)..ByteOffset(header_end),
            StyleRole::SyntaxComment,
        ));
        text.push('\n');
        nodes[0].body = ByteOffset(text.len());

        let mut used_keys = std::collections::HashSet::<String>::new();
        for (file_index, group) in self.files.iter().enumerate() {
            let section_start = ByteOffset(text.len());
            let name_start = text.len();
            text.push_str(&group.relative_display);
            let name_end = text.len();
            if name_end > name_start {
                styles.push((
                    ByteOffset(name_start)..ByteOffset(name_end),
                    StyleRole::SearchFile,
                ));
            }
            let count_start = text.len();
            let noun = if group.matches.len() == 1 {
                "match"
            } else {
                "matches"
            };
            text.push_str(&format!(" ({} {noun})\n", group.matches.len()));
            let count_end = text.len() - 1;
            if count_end > count_start {
                styles.push((
                    ByteOffset(count_start)..ByteOffset(count_end),
                    StyleRole::CompletionAnnotation,
                ));
            }
            let body_start = ByteOffset(text.len());
            let first_match = match_locations.len();
            let line_width = group
                .matches
                .iter()
                .map(|m| digit_count(m.line_number))
                .max()
                .unwrap_or(1)
                .max(2);
            for item in &group.matches {
                let line_start = ByteOffset(text.len());
                let prefix = format!(
                    " {:>width$}:{}: ",
                    item.line_number,
                    item.display_column,
                    width = line_width
                );
                let num_start = text.len();
                text.push_str(&prefix);
                let num_end = text.len() - 1; // exclude trailing space
                if num_end > num_start {
                    styles.push((
                        ByteOffset(num_start)..ByteOffset(num_end),
                        StyleRole::SearchLineNumber,
                    ));
                }
                let content_start = text.len();
                text.push_str(&item.text);
                for range in &item.submatches {
                    if range.start < range.end && range.end <= item.text.len() {
                        styles.push((
                            ByteOffset(content_start + range.start)
                                ..ByteOffset(content_start + range.end),
                            StyleRole::SearchMatch,
                        ));
                    }
                }
                text.push('\n');
                let line_end = ByteOffset(text.len());
                match_locations.push(MatchLocation {
                    file_index,
                    path: group.path.clone(),
                    line: item.line_number,
                    column: item.match_byte_column,
                    range: line_start..line_end,
                    target: line_start,
                });
            }
            let section_end = ByteOffset(text.len());
            let mut key = format!("file:{}", group.relative_display);
            if !used_keys.insert(key.clone()) {
                let mut suffix = 1usize;
                loop {
                    let candidate = format!("file:{}#{suffix}", group.relative_display);
                    if used_keys.insert(candidate.clone()) {
                        key = candidate;
                        break;
                    }
                    suffix += 1;
                }
            }
            nodes.push(Section {
                key,
                parent: Some(0),
                range: section_start..section_end,
                body: body_start,
                collapsed: false,
                hunk: false,
            });
            file_locations.push(FileLocation {
                path: group.path.clone(),
                heading_range: section_start..body_start,
                first_match,
                match_count: group.matches.len(),
            });
            text.push('\n');
        }

        if !self.warnings.is_empty() {
            let warn_start = text.len();
            for warning in &self.warnings {
                text.push_str("rg warning: ");
                text.push_str(&escape_single_line(warning));
                text.push('\n');
            }
            let warn_end = text.len();
            styles.push((
                ByteOffset(warn_start)..ByteOffset(warn_end),
                StyleRole::DiffRemoved,
            ));
        }

        let footer_start = text.len();
        let footer_role = match &self.status {
            SearchStatus::Running => {
                if self.total_matches == 0 {
                    text.push_str("Searching...\n");
                } else {
                    text.push_str(&format!(
                        "Searching... ({} {} in {} {})\n",
                        self.total_matches,
                        plural(self.total_matches, "match", "matches"),
                        self.files.len(),
                        plural(self.files.len(), "file", "files"),
                    ));
                }
                StyleRole::SyntaxComment
            }
            SearchStatus::Finished {
                elapsed_ms,
                truncated,
            } => {
                if self.total_matches == 0 {
                    text.push_str(&format!(
                        "rg finished (no matches found, {:.2}s)\n",
                        *elapsed_ms as f64 / 1000.0
                    ));
                } else {
                    text.push_str(&format!(
                        "rg finished ({} {} in {} {}, {:.2}s){}\n",
                        self.total_matches,
                        plural(self.total_matches, "match", "matches"),
                        self.files.len(),
                        plural(self.files.len(), "file", "files"),
                        *elapsed_ms as f64 / 1000.0,
                        if *truncated {
                            " [truncated at result limit]".to_owned()
                        } else {
                            String::new()
                        },
                    ));
                }
                StyleRole::SyntaxComment
            }
            SearchStatus::Killed => {
                text.push_str(&format!(
                    "rg killed ({} {} in {} {})\n",
                    self.total_matches,
                    plural(self.total_matches, "match", "matches"),
                    self.files.len(),
                    plural(self.files.len(), "file", "files"),
                ));
                StyleRole::DiffRemoved
            }
            SearchStatus::Failed(error) => {
                text.push_str("rg exited abnormally: ");
                text.push_str(&escape_single_line(error));
                text.push('\n');
                StyleRole::DiffRemoved
            }
        };
        let footer_end = text.len();
        styles.push((
            ByteOffset(footer_start)..ByteOffset(footer_end),
            footer_role,
        ));

        nodes[0].range = ByteOffset(0)..ByteOffset(text.len());
        let rope = Rope::from(text.as_str());
        let sections = Sections::new(&rope, nodes).expect("search sections must be valid");
        RenderedSearch {
            text,
            sections,
            styles,
            navigation: SearchNavigation {
                matches: match_locations,
                files: file_locations,
                running: self.status.is_running(),
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchLocation {
    pub file_index: usize,
    pub path: PathBuf,
    /// 1-based line number.
    pub line: usize,
    /// 0-based UTF-8 byte offset within the target file line.
    pub column: usize,
    pub range: Range<ByteOffset>,
    pub target: ByteOffset,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileLocation {
    pub path: PathBuf,
    pub heading_range: Range<ByteOffset>,
    pub first_match: usize,
    pub match_count: usize,
}

#[derive(Clone, Debug, Default)]
pub struct SearchNavigation {
    pub matches: Vec<MatchLocation>,
    pub files: Vec<FileLocation>,
    pub running: bool,
}

impl SearchNavigation {
    pub fn first_match(&self) -> Option<&MatchLocation> {
        self.matches.first()
    }

    /// Match at `point`, or the first match of the file if `point` is on its
    /// heading line (mirroring `compile-goto-error` on file headings).
    pub fn match_at(&self, point: ByteOffset) -> Result<MatchLocation, String> {
        if let Some(found) = self.matches.iter().find(|item| item.range.contains(&point)) {
            return Ok(found.clone());
        }
        if let Some(file) = self
            .files
            .iter()
            .find(|file| file.heading_range.contains(&point))
            && file.match_count > 0
            && let Some(first) = self.matches.get(file.first_match)
        {
            return Ok(first.clone());
        }
        Err("No match here".into())
    }

    /// Moves strictly to the next or previous match relative to `point`.
    pub fn step_match(&self, point: ByteOffset, forward: bool) -> Result<MatchLocation, String> {
        if forward {
            self.matches
                .iter()
                .find(|item| item.range.start > point)
                .cloned()
                .ok_or_else(|| {
                    if self.running {
                        "No more matches yet".into()
                    } else {
                        "Past last match".into()
                    }
                })
        } else {
            self.matches
                .iter()
                .rev()
                .find(|item| item.range.end <= point)
                .cloned()
                .ok_or_else(|| "Moved back before first match".into())
        }
    }

    /// Moves to the first match of the next or previous file relative to `point`
    /// (`compilation-next-file` / `compilation-previous-file`).
    pub fn step_file(&self, point: ByteOffset, forward: bool) -> Result<MatchLocation, String> {
        let current_file = self
            .matches
            .iter()
            .find(|item| item.range.contains(&point))
            .map(|item| item.file_index)
            .or_else(|| {
                self.files
                    .iter()
                    .enumerate()
                    .find(|(_, file)| file.heading_range.contains(&point))
                    .map(|(idx, _)| idx)
            });
        if forward {
            if let Some(current) = current_file {
                self.matches
                    .iter()
                    .find(|item| item.file_index > current)
                    .cloned()
            } else {
                self.matches
                    .iter()
                    .find(|item| item.range.start > point)
                    .cloned()
            }
            .ok_or_else(|| {
                if self.running {
                    "No more matches yet".into()
                } else {
                    "Past last match".into()
                }
            })
        } else {
            let target_file = if let Some(current) = current_file {
                current.checked_sub(1)
            } else {
                self.matches
                    .iter()
                    .rev()
                    .find(|item| item.range.end <= point)
                    .map(|item| item.file_index)
            };
            target_file
                .and_then(|idx| self.files.get(idx))
                .and_then(|file| self.matches.get(file.first_match))
                .cloned()
                .ok_or_else(|| "Moved back before first match".into())
        }
    }
}

pub struct RenderedSearch {
    pub text: String,
    pub sections: Sections,
    pub styles: Vec<(Range<ByteOffset>, StyleRole)>,
    pub navigation: SearchNavigation,
}

/// Cancellation and process-kill handle shared between UI and background worker.
#[derive(Default)]
pub struct SearchController {
    state: Mutex<ControllerState>,
}

#[derive(Default)]
struct ControllerState {
    cancelled: bool,
    #[cfg(unix)]
    pgid: Option<i32>,
}

impl SearchController {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_cancelled(&self) -> bool {
        self.state.lock().unwrap().cancelled
    }

    pub fn cancel(&self) {
        let mut state = self.state.lock().unwrap();
        state.cancelled = true;
        #[cfg(unix)]
        if let Some(pgid) = state.pgid {
            // Terminate the entire rg process group while holding the lock so
            // `clear_pid` cannot race with PID reuse after `wait()`.
            unsafe {
                libc::kill(-pgid, libc::SIGTERM);
            }
        }
    }

    fn set_pid(&self, pid: u32) {
        let mut state = self.state.lock().unwrap();
        #[cfg(unix)]
        if let Ok(pgid) = i32::try_from(pid)
            && pgid > 0
        {
            state.pgid = Some(pgid);
            if state.cancelled {
                unsafe {
                    libc::kill(-pgid, libc::SIGTERM);
                }
            }
        }
        #[cfg(not(unix))]
        let _ = (pid, &mut *state);
    }

    fn clear_pid(&self) {
        let mut state = self.state.lock().unwrap();
        #[cfg(unix)]
        {
            state.pgid = None;
        }
        #[cfg(not(unix))]
        let _ = &mut *state;
    }
}

/// Latest rendered update pushed from the background worker to the UI.
pub struct SearchUpdate {
    pub query: SearchQuery,
    pub rendered: RenderedSearch,
    pub status: SearchStatus,
    pub total_matches: usize,
    pub file_count: usize,
}

/// Lightweight coalescing async notification from worker thread to UI task.
#[derive(Default)]
pub struct SearchSignal {
    state: Mutex<SignalState>,
}

#[derive(Default)]
struct SignalState {
    pending: bool,
    closed: bool,
    waker: Option<std::task::Waker>,
}

impl SearchSignal {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn notify(&self) {
        let waker = {
            let mut state = self.state.lock().unwrap();
            state.pending = true;
            state.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    pub fn close(&self) {
        let waker = {
            let mut state = self.state.lock().unwrap();
            state.closed = true;
            state.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    pub async fn wait(&self) -> bool {
        std::future::poll_fn(|cx| {
            let mut state = self.state.lock().unwrap();
            if state.pending {
                state.pending = false;
                std::task::Poll::Ready(true)
            } else if state.closed {
                std::task::Poll::Ready(false)
            } else {
                state.waker = Some(cx.waker().clone());
                std::task::Poll::Pending
            }
        })
        .await
    }
}

/// Runs `rg --json` in the background, publishing throttled snapshots via
/// `latest` + `signal` until finished, killed, or failed.
pub fn run_streaming(
    query: SearchQuery,
    controller: Arc<SearchController>,
    latest: Arc<Mutex<Option<SearchUpdate>>>,
    signal: Arc<SearchSignal>,
) {
    struct CloseGuard(Arc<SearchSignal>);
    impl Drop for CloseGuard {
        fn drop(&mut self) {
            self.0.close();
        }
    }
    let _close = CloseGuard(signal.clone());
    let start = Instant::now();
    let mut data = SearchData::running(query.clone());

    let publish = |data: &SearchData| {
        let rendered = data.render();
        let update = SearchUpdate {
            query: data.query.clone(),
            rendered,
            status: data.status.clone(),
            total_matches: data.total_matches,
            file_count: data.files.len(),
        };
        if let Ok(mut guard) = latest.lock() {
            *guard = Some(update);
        }
        signal.notify();
    };

    if controller.is_cancelled() {
        data.status = SearchStatus::Killed;
        publish(&data);
        return;
    }

    let mut command = Command::new("rg");
    command
        .current_dir(&query.directory)
        .args(query.build_args())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            data.status = SearchStatus::Failed(format!("Cannot start rg: {error}"));
            publish(&data);
            return;
        }
    };
    controller.set_pid(child.id());

    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");

    let stderr_handle = std::thread::spawn(move || {
        let mut stderr = stderr;
        let mut buf = Vec::new();
        let _ = stderr
            .by_ref()
            .take((MAX_STDERR_BYTES + 1) as u64)
            .read_to_end(&mut buf);
        let truncated = buf.len() > MAX_STDERR_BYTES;
        if truncated {
            buf.truncate(MAX_STDERR_BYTES);
            // Keep draining stderr so rg never hits EPIPE on noisy directories.
            let _ = std::io::copy(&mut stderr, &mut std::io::sink());
        }
        let mut text = String::from_utf8_lossy(&buf).trim().to_owned();
        if truncated {
            text.push_str(" [stderr truncated]");
        }
        text
    });

    let mut reader = BufReader::new(stdout);
    let mut line_buf = Vec::new();
    let mut truncated = false;
    let mut approx_bytes = 0usize;
    let mut last_publish = Instant::now();
    let mut unpublished_matches = 0usize;
    let mut file_indices = std::collections::HashMap::<PathBuf, usize>::new();

    loop {
        if controller.is_cancelled() {
            break;
        }
        line_buf.clear();
        match reader.read_until(b'\n', &mut line_buf) {
            Ok(0) => break,
            Ok(_) => {
                if let Ok(line) = std::str::from_utf8(&line_buf)
                    && let Ok(value) = json::parse(line)
                    && let Some(added_bytes) =
                        apply_json_record(&mut data, &mut file_indices, &value)
                {
                    approx_bytes += added_bytes;
                    unpublished_matches += 1;
                    if data.total_matches >= MAX_MATCHES || approx_bytes >= MAX_DISPLAY_BYTES {
                        truncated = true;
                        break;
                    }
                    if unpublished_matches >= STREAM_BATCH_MATCHES
                        || (unpublished_matches > 0 && last_publish.elapsed() >= STREAM_INTERVAL)
                    {
                        publish(&data);
                        unpublished_matches = 0;
                        last_publish = Instant::now();
                    }
                }
            }
            Err(_) => break,
        }
    }

    // Disarm external signaling before reaping `child` so `cancel()` can never
    // signal a reused PID/PGID after `try_wait()`/`wait()`.
    controller.clear_pid();
    if controller.is_cancelled() || truncated {
        stop_child(&mut child);
    }
    let exit_status = child.wait();
    let stderr_text = stderr_handle.join().unwrap_or_default();

    if controller.is_cancelled() {
        data.status = SearchStatus::Killed;
    } else if truncated {
        if !stderr_text.is_empty() {
            push_warnings(&mut data.warnings, &stderr_text);
        }
        data.status = SearchStatus::Finished {
            elapsed_ms: start.elapsed().as_millis() as u64,
            truncated: true,
        };
    } else {
        match exit_status {
            Ok(status) => match status.code() {
                Some(0) | Some(1) => {
                    if !stderr_text.is_empty() {
                        push_warnings(&mut data.warnings, &stderr_text);
                    }
                    data.status = SearchStatus::Finished {
                        elapsed_ms: start.elapsed().as_millis() as u64,
                        truncated: false,
                    };
                }
                _ => {
                    if data.total_matches > 0 {
                        // Partial matches with permission warnings or similar.
                        if !stderr_text.is_empty() {
                            push_warnings(&mut data.warnings, &stderr_text);
                        }
                        data.status = SearchStatus::Finished {
                            elapsed_ms: start.elapsed().as_millis() as u64,
                            truncated: false,
                        };
                    } else {
                        let detail = if stderr_text.is_empty() {
                            format!("exit status {status}")
                        } else {
                            stderr_text
                        };
                        data.status = SearchStatus::Failed(detail);
                    }
                }
            },
            Err(error) => {
                data.status = SearchStatus::Failed(format!("Failed waiting for rg: {error}"));
            }
        }
    }

    publish(&data);
}

fn push_warnings(warnings: &mut Vec<String>, stderr: &str) {
    for line in stderr.lines().take(8) {
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            warnings.push(trimmed.to_owned());
        }
    }
}

fn stop_child(child: &mut Child) {
    #[cfg(unix)]
    {
        if let Ok(pgid) = i32::try_from(child.id()) {
            unsafe {
                libc::kill(-pgid, libc::SIGTERM);
            }
            let deadline = Instant::now() + Duration::from_millis(200);
            while Instant::now() < deadline {
                if matches!(child.try_wait(), Ok(Some(_))) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            unsafe {
                libc::kill(-pgid, libc::SIGKILL);
            }
        }
    }
    let _ = child.kill();
}

/// Applies one parsed `rg --json` message. Returns the approximate display
/// bytes added when a match was appended.
pub(crate) fn apply_json_record(
    data: &mut SearchData,
    file_indices: &mut std::collections::HashMap<PathBuf, usize>,
    record: &json::Value,
) -> Option<usize> {
    let kind = record.get("type").and_then(json::Value::as_str)?;
    if kind != "match" {
        return None;
    }
    let payload = record.get("data")?;
    let (relative_display, relative_path) = payload.get("path").and_then(extract_path)?;
    let line_number = payload
        .get("line_number")
        .and_then(json::Value::as_u64)
        .and_then(|n| usize::try_from(n).ok())?;
    let raw_line = payload.get("lines").and_then(extract_line_bytes)?;
    let raw_submatches: Vec<Submatch> = payload
        .get("submatches")
        .and_then(json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let start = item.get("start")?.as_u64()? as usize;
                    let end = item.get("end")?.as_u64()? as usize;
                    Some(Submatch { start, end })
                })
                .collect()
        })
        .unwrap_or_default();

    let match_line = build_match_line(line_number, &raw_line, &raw_submatches);
    let mut added_bytes = match_line.text.len() + 24;
    let abs_path = data.query.directory.join(&relative_path);
    let file_idx = match file_indices.get(&abs_path).copied() {
        Some(idx) => idx,
        None => {
            added_bytes += relative_display.len() + 24;
            let idx = data.files.len();
            data.files.push(FileGroup {
                relative_display,
                path: abs_path.clone(),
                matches: Vec::new(),
            });
            file_indices.insert(abs_path, idx);
            idx
        }
    };
    data.files[file_idx].matches.push(match_line);
    data.total_matches += 1;
    Some(added_bytes)
}

fn extract_path(value: &json::Value) -> Option<(String, PathBuf)> {
    if let Some(text) = value.get("text").and_then(json::Value::as_str) {
        let stripped = text.strip_prefix("./").unwrap_or(text);
        let display = escape_single_line(stripped);
        return Some((display, PathBuf::from(stripped)));
    }
    if let Some(encoded) = value.get("bytes").and_then(json::Value::as_str)
        && let Ok(mut bytes) = json::decode_base64(encoded)
    {
        if bytes.starts_with(b"./") {
            bytes.drain(..2);
        }
        let lossy = String::from_utf8_lossy(&bytes);
        let display = escape_single_line(&lossy);
        #[cfg(unix)]
        let path = {
            use std::os::unix::ffi::OsStringExt;
            PathBuf::from(OsString::from_vec(bytes))
        };
        #[cfg(not(unix))]
        let path = PathBuf::from(lossy.into_owned());
        return Some((display, path));
    }
    None
}

fn extract_line_bytes(value: &json::Value) -> Option<Vec<u8>> {
    if let Some(text) = value.get("text").and_then(json::Value::as_str) {
        return Some(text.as_bytes().to_vec());
    }
    if let Some(encoded) = value.get("bytes").and_then(json::Value::as_str) {
        return json::decode_base64(encoded).ok();
    }
    None
}

fn build_match_line(
    line_number: usize,
    raw_bytes: &[u8],
    raw_submatches: &[Submatch],
) -> MatchLine {
    let mut trimmed = raw_bytes;
    if trimmed.ends_with(b"\n") {
        trimmed = &trimmed[..trimmed.len() - 1];
    }
    if trimmed.ends_with(b"\r") {
        trimmed = &trimmed[..trimmed.len() - 1];
    }
    let first_raw_start = raw_submatches.first().map(|s| s.start).unwrap_or(0);

    // Work on valid UTF-8 (lossy fallback only when file contains non-UTF-8 bytes).
    let owned_lossy;
    let (utf8, exact_utf8) = match std::str::from_utf8(trimmed) {
        Ok(s) => (s, true),
        Err(_) => {
            owned_lossy = String::from_utf8_lossy(trimmed).into_owned();
            (owned_lossy.as_str(), false)
        }
    };

    let match_byte_column = if exact_utf8 {
        clamp_char_boundary(utf8, first_raw_start)
    } else {
        0
    };
    let display_column = utf8[..match_byte_column].chars().count() + 1;

    // Sanitize control characters (keeping TAB as space or literal? keeping TAB/printable,
    // replacing CR/LF/NUL so every match occupies exactly one line) and cap display width.
    let truncated_limit = clamp_char_boundary(utf8, MAX_LINE_DISPLAY_BYTES);
    let was_truncated = truncated_limit < utf8.len();
    let visible_src = &utf8[..truncated_limit];

    let mut display = String::with_capacity(visible_src.len() + if was_truncated { 3 } else { 0 });
    // Map source byte offset -> display byte offset so submatch highlights stay exact.
    let mut offset_map = vec![0usize; visible_src.len() + 1];
    for (src_idx, ch) in visible_src.char_indices() {
        let dst_idx = display.len();
        for slot in &mut offset_map[src_idx..src_idx + ch.len_utf8()] {
            *slot = dst_idx;
        }
        if matches!(ch, '\r' | '\n' | '\0') {
            display.push(' ');
        } else {
            display.push(ch);
        }
    }
    offset_map[visible_src.len()] = display.len();
    let content_end = display.len();
    if was_truncated {
        display.push_str("...");
    }

    let mut submatches = Vec::new();
    if exact_utf8 {
        let mut last_end = 0usize;
        let mut sorted: Vec<_> = raw_submatches.iter().collect();
        sorted.sort_by_key(|s| (s.start, s.end));
        for sub in sorted {
            if sub.start >= truncated_limit || sub.start >= sub.end {
                continue;
            }
            let raw_s = clamp_char_boundary(visible_src, sub.start);
            let raw_e = clamp_char_boundary(visible_src, sub.end.min(truncated_limit));
            let s = offset_map[raw_s].max(last_end);
            let e = offset_map[raw_e].min(content_end);
            if s < e {
                submatches.push(s..e);
                last_end = e;
            }
        }
    }

    MatchLine {
        line_number,
        match_byte_column,
        display_column,
        text: display,
        submatches,
    }
}

fn clamp_char_boundary(s: &str, byte_index: usize) -> usize {
    let mut idx = byte_index.min(s.len());
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

fn escape_single_line(text: &str) -> String {
    text.replace(['\r', '\n', '\0'], " ")
}

fn digit_count(mut n: usize) -> usize {
    let mut count = 1;
    while n >= 10 {
        n /= 10;
        count += 1;
    }
    count
}

fn plural(n: usize, singular: &'static str, plural: &'static str) -> &'static str {
    if n == 1 { singular } else { plural }
}
