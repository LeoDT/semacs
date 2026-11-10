//! File and hunk index requests captured from a status projection, never from script paths.

use super::{Entry, Group, Head, Record, Repository, Status};
use crate::git;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

#[cfg(all(test, unix))]
mod process_tests;
#[cfg(test)]
mod tests;

const DIAGNOSTIC_LIMIT: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    Stage,
    Unstage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    NotExecuted,
    Succeeded,
    Failed,
    Uncertain,
}

pub struct Report {
    pub outcome: Outcome,
    pub diagnostic: String,
    pub stale: bool,
    pub verification: Option<Result<Status, String>>,
}

pub(crate) fn bounded(mut text: String) -> String {
    const MARKER: &str = "\n[diagnostic truncated]";
    if text.len() > DIAGNOSTIC_LIMIT {
        let mut end = DIAGNOSTIC_LIMIT - MARKER.len();
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push_str(MARKER);
    }
    text
}

impl Report {
    fn refused(diagnostic: String, stale: bool) -> Self {
        Self {
            outcome: Outcome::NotExecuted,
            diagnostic: bounded(diagnostic),
            stale,
            verification: None,
        }
    }
}

pub struct Request {
    repository: Repository,
    head: Head,
    entry: Entry,
    group: Group,
    operation: Operation,
    patch: Option<(std::sync::Arc<crate::diff::FileDiff>, usize)>,
}

fn ordinary(mode: u32) -> bool {
    matches!(mode, 0 | 0o100644 | 0o100755)
}

impl Entry {
    pub fn supports_index_operation(&self, group: Group, operation: Operation) -> bool {
        if !self.in_group(group) || self.submodule.is_submodule {
            return false;
        }
        match operation {
            Operation::Stage if !matches!(group, Group::Untracked | Group::Unstaged) => {
                return false;
            }
            Operation::Unstage if group != Group::Staged => return false,
            _ => {}
        }
        self.ordinary_index_file()
    }

    fn ordinary_index_file(&self) -> bool {
        if self.submodule.is_submodule {
            return false;
        }
        match &self.record {
            Record::Untracked { directory } => !directory,
            Record::Tracked { state, rename } => {
                rename.is_none()
                    && !self
                        .xy
                        .iter()
                        .any(|byte| matches!(byte, b'T' | b'R' | b'C'))
                    && self.xy[1] != b'A'
                    && [state.head_mode, state.index_mode, state.worktree_mode]
                        .into_iter()
                        .all(ordinary)
            }
            Record::Unmerged { .. } => false,
        }
    }
}

impl Status {
    pub fn index_request_at(
        &self,
        location: &super::StatusLocation,
        operation: Operation,
    ) -> Result<Request, String> {
        let mut request = self.index_request(location.group, location.entry, operation)?;
        if let Some(hunk) = location.hunk {
            let file = self
                .file_diff(location.group, location.entry)
                .ok_or("No diff for this status file")?;
            if file.summary.is_some()
                || file.hunks.get(hunk).is_none()
                || !matches!(file.status, 'A' | 'D' | 'M')
            {
                return Err("This change has no supported text hunk".into());
            }
            request.patch = Some((file.clone(), hunk));
        }
        Ok(request)
    }

    pub fn index_request(
        &self,
        group: Group,
        entry: usize,
        operation: Operation,
    ) -> Result<Request, String> {
        let entry = self.entries().get(entry).ok_or("No status file selected")?;
        if !entry.supports_index_operation(group, operation) {
            return Err(
                "Select one supported ordinary file in the appropriate status group".into(),
            );
        }
        if self.entries().iter().any(|other| match &other.record {
            Record::Tracked {
                rename: Some(rename),
                ..
            } => other.path == entry.path || rename.source == entry.path,
            Record::Unmerged { .. } => other.path == entry.path,
            _ => false,
        }) {
            return Err("Rename/copy or conflict paths cannot be written independently".into());
        }
        Ok(Request {
            repository: self.repository().clone(),
            head: self.head().clone(),
            entry: entry.clone(),
            group,
            operation,
            patch: None,
        })
    }
}

fn read(repo: &Repository, args: &[&OsStr]) -> Result<Vec<u8>, String> {
    git::read(
        &repo.root,
        Some(&repo.git_dir),
        args,
        &AtomicBool::new(false),
    )
}

fn check_path(repo: &Repository, entry: &Entry, patch: bool) -> Result<(), String> {
    let path = git::relative_path(entry.path.as_os_str().as_encoded_bytes())?;
    if path != entry.path || path.as_os_str().as_encoded_bytes().starts_with(b":(") {
        return Err("Invalid index target or pathspec magic".into());
    }
    let mut absolute = repo.root.clone();
    let count = path.components().count();
    for (index, component) in path.components().enumerate() {
        absolute.push(component);
        let last = index + 1 == count;
        match std::fs::symlink_metadata(&absolute) {
            Ok(metadata) if last && metadata.is_file() => {}
            Ok(metadata) if !last && metadata.is_dir() => {}
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound
                    && (patch
                        || matches!(&entry.record, Record::Tracked { state, .. } if state.worktree_mode == 0)) =>
            {
                return Ok(());
            }
            Err(error) => return Err(format!("Index target changed; refresh first: {error}")),
            _ => return Err(
                "Index target is not an ordinary file or has a symbolic-link parent; refresh first"
                    .into(),
            ),
        }
    }
    Ok(())
}

fn check_configuration(config: &[u8]) -> Result<(), String> {
    let mut sparse = std::collections::HashMap::new();
    for record in config
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
    {
        let (key, value) = match record.iter().position(|byte| *byte == b'\n') {
            Some(end) => (&record[..end], &record[end + 1..]),
            None => (record, b"true".as_slice()),
        };
        if matches!(key, b"core.sparsecheckout" | b"index.sparse") {
            sparse.insert(key, value);
        }
    }
    if sparse.values().any(|value| {
        ![b"".as_slice(), b"false", b"no", b"off", b"0"]
            .iter()
            .any(|disabled| value.eq_ignore_ascii_case(disabled))
    }) {
        return Err("Sparse checkout/index is not supported for index writes".into());
    }
    Ok(())
}

fn check_index(repo: &Repository, entry: &Entry, match_identity: bool) -> Result<(), String> {
    // Inspect stored sparse entries even if sparse configuration has since been disabled.
    let index = read(
        repo,
        &[
            "-c".as_ref(),
            "core.sparseCheckout=true".as_ref(),
            "-c".as_ref(),
            "index.sparse=true".as_ref(),
            "ls-files".as_ref(),
            "--sparse".as_ref(),
            "--stage".as_ref(),
            "-z".as_ref(),
        ],
    )?;
    if index
        .split(|byte| *byte == 0)
        .any(|record| record.starts_with(b"040000 "))
    {
        return Err("Sparse index is not supported for index writes".into());
    }
    let bytes = read(
        repo,
        &[
            "ls-files".as_ref(),
            "-v".as_ref(),
            "--stage".as_ref(),
            "-z".as_ref(),
            "--".as_ref(),
            entry.path.as_os_str(),
        ],
    )?;
    let expected = match &entry.record {
        Record::Tracked { state, .. } if state.index_mode != 0 => {
            Some((state.index_mode, state.index_oid.as_str()))
        }
        _ => None,
    };
    if bytes.is_empty() {
        return if !match_identity || expected.is_none() {
            Ok(())
        } else {
            Err("Index entry disappeared; refresh first".into())
        };
    }
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .ok_or("Truncated index entry")?;
    let tab = bytes[..end]
        .iter()
        .position(|byte| *byte == b'\t')
        .ok_or("Malformed index entry")?;
    let fields: Vec<_> = bytes[..tab].split(|byte| *byte == b' ').collect();
    if fields.len() != 4
        || fields[0] != b"H"
        || fields[3] != b"0"
        || &bytes[tab + 1..end] != entry.path.as_os_str().as_encoded_bytes()
    {
        return Err("Special or unmerged index entries are not supported".into());
    }
    if end + 1 != bytes.len() {
        return Err("Multiple index entries are not supported".into());
    }
    let mode = std::str::from_utf8(fields[1])
        .ok()
        .and_then(|s| u32::from_str_radix(s, 8).ok());
    let oid = std::str::from_utf8(fields[2]).ok();
    if !mode.is_some_and(ordinary) {
        return Err("Non-ordinary index entries are not supported".into());
    }
    if match_identity && (expected.is_none() || expected != mode.zip(oid)) {
        return Err("Index identity changed; refresh first".into());
    }
    Ok(())
}

struct Prepared {
    request: Request,
    patch: Option<String>,
}

impl Request {
    pub fn repository(&self) -> &Repository {
        &self.repository
    }
    pub fn path(&self) -> &Path {
        &self.entry.path
    }
    pub fn operation(&self) -> Operation {
        self.operation
    }

    pub(crate) fn expected_head(&self) -> &Head {
        &self.head
    }
    pub(crate) fn expected_entry(&self) -> &Entry {
        &self.entry
    }
    pub(crate) fn group(&self) -> Group {
        self.group
    }

    fn prepare(self) -> Result<Prepared, String> {
        let patch = self.patch.is_some();
        check_path(&self.repository, &self.entry, patch)?;
        let config = read(
            &self.repository,
            &["config".as_ref(), "--null".as_ref(), "--list".as_ref()],
        )?;
        check_configuration(&config)?;
        let mut paths = self.entry.path.as_os_str().as_encoded_bytes().to_vec();
        paths.push(0);
        super::load::check_filters(
            &self.repository,
            &paths,
            &super::load::external_filters(&config)?,
            &AtomicBool::new(false),
        )?;
        check_index(&self.repository, &self.entry, !patch)?;
        let current = super::load::load(self.repository.clone(), &AtomicBool::new(false))?;
        if patch {
            if current.entries().iter().any(|entry| {
                (entry.path == self.entry.path && !entry.ordinary_index_file())
                    || matches!(&entry.record, Record::Tracked { rename: Some(rename), .. } if rename.source == self.entry.path)
            }) {
                return Err("Special index or file types are not supported for hunk writes".into());
            }
            let (file, hunk) = self.patch.as_ref().unwrap();
            let patch = file.hunk_patch(*hunk)?;
            return Ok(Prepared {
                request: self,
                patch: Some(patch),
            });
        }
        if current.head() != &self.head
            || !current
                .entries()
                .iter()
                .any(|entry| entry == &self.entry && entry.in_group(self.group))
        {
            return Err("Status, HEAD or index changed; refresh before writing".into());
        }
        // The complete model is checked too: a rename source must not be staged as an untracked file.
        let index = current
            .entries()
            .iter()
            .position(|entry| entry == &self.entry)
            .unwrap();
        current.index_request(self.group, index, self.operation)?;
        check_path(&self.repository, &self.entry, false)?;
        Ok(Prepared {
            request: self,
            patch: None,
        })
    }

    pub fn execute(self) -> Report {
        match self.write_observed(&mut || {}) {
            Ok(executed) => executed.verify(),
            Err(report) => *report,
        }
    }

    pub(crate) fn write_observed(self, started: &mut dyn FnMut()) -> Result<Executed, Box<Report>> {
        if let Err(error) = git::write_environment() {
            return Err(Box::new(Report::refused(error, false)));
        }
        let prepared = self
            .prepare()
            .map_err(|error| Box::new(Report::refused(error, true)))?;
        prepared
            .write_observed(started)
            .map_err(|error| Box::new(Report::refused(error, false)))
    }
}

impl Prepared {
    #[cfg(test)]
    fn execute(self) -> Report {
        match self.write() {
            Ok(executed) => executed.verify(),
            Err(error) => Report::refused(error, false),
        }
    }

    #[cfg(test)]
    fn write(self) -> Result<Executed, String> {
        self.write_observed(&mut || {})
    }

    fn write_observed(self, started: &mut dyn FnMut()) -> Result<Executed, String> {
        let request = self.request;
        let hooks = EmptyHooks::new()?;
        let args: Vec<&OsStr> = if request.patch.is_some() {
            let mut args = vec![
                "apply".as_ref(),
                "--cached".as_ref(),
                "-p0".as_ref(),
                "-C3".as_ref(),
                "--ignore-space-change".as_ref(),
            ];
            if request.operation == Operation::Unstage {
                args.push("--reverse".as_ref());
            }
            args.push("-".as_ref());
            args
        } else {
            match (&request.operation, &request.head) {
                (Operation::Stage, _) => vec![
                    "add".as_ref(),
                    "--".as_ref(),
                    request.entry.path.as_os_str(),
                ],
                (Operation::Unstage, Head::Unborn { .. }) => vec![
                    "update-index".as_ref(),
                    "--force-remove".as_ref(),
                    "--".as_ref(),
                    request.entry.path.as_os_str(),
                ],
                (Operation::Unstage, Head::Branch { oid, .. } | Head::Detached { oid }) => vec![
                    "restore".as_ref(),
                    "--staged".as_ref(),
                    "--source".as_ref(),
                    oid.as_ref(),
                    "--".as_ref(),
                    request.entry.path.as_os_str(),
                ],
            }
        };
        let (outcome, diagnostic) = match git::execute(
            &request.repository.root,
            Some(&request.repository.git_dir),
            &args,
            self.patch.as_deref().map(str::as_bytes),
            git::Access::IndexWrite {
                hooks: &hooks.0,
                started,
            },
        ) {
            Ok(output) if output.status.success() => (Outcome::Succeeded, output.diagnostic),
            Ok(output) => (
                if output.status.code().is_some() {
                    Outcome::Failed
                } else {
                    Outcome::Uncertain
                },
                format!("Git write ({}): {}", output.status, output.diagnostic),
            ),
            Err(failure) if !failure.started => return Err(failure.diagnostic),
            Err(failure) => (Outcome::Uncertain, failure.diagnostic),
        };
        Ok(Executed {
            repository: request.repository,
            outcome,
            diagnostic: bounded(diagnostic),
        })
    }
}

pub(crate) struct Executed {
    repository: Repository,
    pub outcome: Outcome,
    pub diagnostic: String,
}

impl Executed {
    pub fn verify(self) -> Report {
        let verification = Status::load(self.repository, &AtomicBool::new(false)).map_err(bounded);
        Report {
            outcome: self.outcome,
            diagnostic: self.diagnostic,
            stale: true,
            verification: Some(verification),
        }
    }
}

struct EmptyHooks(PathBuf);

impl EmptyHooks {
    fn new() -> Result<Self, String> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        for _ in 0..16 {
            let path = std::env::temp_dir().join(format!(
                "semacs-empty-hooks-{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_err(|error| error.to_string())?
                    .as_nanos(),
                NEXT.fetch_add(1, Ordering::Relaxed),
            ));
            match builder.create(&path) {
                Ok(()) => match path.canonicalize() {
                    Ok(absolute) => return Ok(Self(absolute)),
                    Err(error) => {
                        let _ = std::fs::remove_dir(&path);
                        return Err(format!("Cannot resolve empty hooks directory: {error}"));
                    }
                },
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(format!("Cannot create empty hooks directory: {error}")),
            }
        }
        Err("Cannot allocate empty hooks directory".into())
    }
}

impl Drop for EmptyHooks {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir(&self.0);
    }
}
