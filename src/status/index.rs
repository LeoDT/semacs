//! File, hunk, and group index requests captured from a status projection, never from script paths.

use super::{Entry, Group, Head, Record, Repository, Status, StatusTarget};
use crate::git;
use std::collections::HashSet;
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WriteTarget {
    File(PathBuf),
    Group { group: Group, count: usize },
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

enum RequestTarget {
    File(Entry),
    Hunk {
        entry: Entry,
        file: std::sync::Arc<crate::diff::FileDiff>,
        hunk: usize,
    },
    Group(Vec<Entry>),
}

pub struct Request {
    repository: Repository,
    head: Head,
    group: Group,
    operation: Operation,
    target: RequestTarget,
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
            Record::Untracked { .. } => true,
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

fn has_nested_git_dir(repo: &Repository, path: &Path) -> Result<bool, String> {
    let git_entry = repo.root.join(path).join(".git");
    match std::fs::symlink_metadata(&git_entry) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!("Cannot inspect untracked directory: {error}")),
    }
}

fn check_group_operation(group: Group, operation: Operation) -> Result<(), String> {
    match (group, operation) {
        (Group::Untracked | Group::Unstaged, Operation::Stage) => Ok(()),
        (Group::Staged, Operation::Unstage) => Ok(()),
        (Group::Staged, Operation::Stage) => Err("Staged changes are already staged".into()),
        (Group::Untracked | Group::Unstaged, Operation::Unstage) => {
            Err("Only staged changes can be unstaged".into())
        }
        (Group::Unmerged, _) => Err("Unmerged conflict paths cannot be written as a group".into()),
    }
}

impl Status {
    pub fn index_request_at(
        &self,
        location: &super::StatusLocation,
        operation: Operation,
    ) -> Result<Request, String> {
        match location.target {
            StatusTarget::Group => self.index_request_group(location.group, operation),
            StatusTarget::File { entry } => self.index_request(location.group, entry, operation),
            StatusTarget::Hunk { entry, hunk } => {
                let mut request = self.index_request(location.group, entry, operation)?;
                let file = self
                    .file_diff(location.group, entry)
                    .ok_or("No diff for this status file")?;
                if file.summary.is_some()
                    || file.hunks.get(hunk).is_none()
                    || !matches!(file.status, 'A' | 'D' | 'M')
                {
                    return Err("This change has no supported text hunk".into());
                }
                let RequestTarget::File(entry_data) = request.target else {
                    unreachable!();
                };
                request.target = RequestTarget::Hunk {
                    entry: entry_data,
                    file: file.clone(),
                    hunk,
                };
                Ok(request)
            }
        }
    }

    pub fn index_request_group(
        &self,
        group: Group,
        operation: Operation,
    ) -> Result<Request, String> {
        check_group_operation(group, operation)?;
        let entries: Vec<Entry> = self.group(group).map(|(_, entry)| entry.clone()).collect();
        if entries.is_empty() {
            return Err("Status section has no entries".into());
        }
        let unmerged_paths: HashSet<&Path> = self
            .entries()
            .iter()
            .filter(|entry| matches!(entry.record, Record::Unmerged { .. }))
            .map(|entry| entry.path.as_path())
            .collect();
        for entry in &entries {
            if entry.submodule.is_submodule {
                return Err("Submodule entries cannot be written as a group".into());
            }
            if matches!(entry.record, Record::Unmerged { .. })
                || unmerged_paths.contains(entry.path.as_path())
            {
                return Err("Conflict paths cannot be written as a group".into());
            }
            if let Record::Untracked { directory: true } = entry.record
                && has_nested_git_dir(self.repository(), &entry.path)?
            {
                return Err("Nested Git repositories cannot be staged directly".into());
            }
        }
        Ok(Request {
            repository: self.repository().clone(),
            head: self.head().clone(),
            group,
            operation,
            target: RequestTarget::Group(entries),
        })
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
        if let Record::Untracked { directory: true } = entry.record
            && has_nested_git_dir(self.repository(), &entry.path)?
        {
            return Err("Nested Git repositories cannot be staged directly".into());
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
            group,
            operation,
            target: RequestTarget::File(entry.clone()),
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

fn validate_relative_path(raw: &Path) -> Result<PathBuf, String> {
    let path = git::relative_path(raw.as_os_str().as_encoded_bytes())?;
    if path != raw || path.as_os_str().as_encoded_bytes().starts_with(b":(") {
        return Err("Invalid index target or pathspec magic".into());
    }
    Ok(path)
}

fn check_untracked_directory(repo: &Repository, raw: &Path) -> Result<Vec<PathBuf>, String> {
    let path = validate_relative_path(raw)?;
    let mut absolute = repo.root.clone();
    let count = path.components().count();
    for (index, component) in path.components().enumerate() {
        absolute.push(component);
        let last = index + 1 == count;
        match std::fs::symlink_metadata(&absolute) {
            Ok(metadata) if metadata.is_dir() => {}
            Err(error) => return Err(format!("Index target changed; refresh first: {error}")),
            _ if last => {
                return Err(
                    "Index target is no longer an ordinary directory; refresh first".into(),
                );
            }
            _ => {
                return Err(
                    "Index target has a symbolic-link or non-directory parent; refresh first"
                        .into(),
                );
            }
        }
    }
    if has_nested_git_dir(repo, &path)? {
        return Err("Nested Git repositories cannot be staged directly".into());
    }
    let output = read(
        repo,
        &[
            "ls-files".as_ref(),
            "-z".as_ref(),
            "--others".as_ref(),
            "--exclude-standard".as_ref(),
            "--".as_ref(),
            path.as_os_str(),
        ],
    )?;
    let mut files = Vec::new();
    for record in output
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
    {
        if record.ends_with(b"/") {
            return Err("Nested Git repositories cannot be staged directly".into());
        }
        files.push(git::relative_path(record)?);
    }
    if files.is_empty() {
        return Err("Untracked directory has no stageable files; refresh first".into());
    }
    Ok(files)
}

fn check_path(repo: &Repository, entry: &Entry, patch: bool) -> Result<(), String> {
    let path = validate_relative_path(&entry.path)?;
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

fn check_group_entry_path(
    repo: &Repository,
    raw: &Path,
    allow_missing: bool,
) -> Result<(), String> {
    let path = validate_relative_path(raw)?;
    let mut absolute = repo.root.clone();
    let count = path.components().count();
    for (index, component) in path.components().enumerate() {
        absolute.push(component);
        let last = index + 1 == count;
        match std::fs::symlink_metadata(&absolute) {
            Ok(metadata) if !last && metadata.is_dir() => {}
            Ok(metadata) if last && (metadata.is_file() || metadata.file_type().is_symlink()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && allow_missing => {
                return Ok(());
            }
            Err(error) => return Err(format!("Index target changed; refresh first: {error}")),
            _ => {
                return Err(
                    "Index target is not an ordinary file/symlink or has a symbolic-link parent; refresh first"
                        .into(),
                );
            }
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

fn check_sparse_index(repo: &Repository) -> Result<(), String> {
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
    Ok(())
}

fn check_index(repo: &Repository, entry: &Entry, match_identity: bool) -> Result<(), String> {
    check_sparse_index(repo)?;
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

fn push_nul_path(out: &mut Vec<u8>, path: &Path) {
    out.extend_from_slice(path.as_os_str().as_encoded_bytes());
    out.push(0);
}

struct Prepared {
    request: Request,
    input: Option<Vec<u8>>,
}

impl Request {
    pub fn repository(&self) -> &Repository {
        &self.repository
    }

    pub fn target(&self) -> WriteTarget {
        match &self.target {
            RequestTarget::File(entry) | RequestTarget::Hunk { entry, .. } => {
                WriteTarget::File(entry.path.clone())
            }
            RequestTarget::Group(entries) => WriteTarget::Group {
                group: self.group,
                count: entries.len(),
            },
        }
    }

    pub fn operation(&self) -> Operation {
        self.operation
    }

    pub(crate) fn expected_head(&self) -> &Head {
        &self.head
    }

    pub(crate) fn expected_entries(&self) -> Vec<Entry> {
        match &self.target {
            RequestTarget::File(entry) | RequestTarget::Hunk { entry, .. } => vec![entry.clone()],
            RequestTarget::Group(entries) => entries.clone(),
        }
    }

    pub(crate) fn group(&self) -> Group {
        self.group
    }

    fn prepare(self) -> Result<Prepared, String> {
        let config = read(
            &self.repository,
            &["config".as_ref(), "--null".as_ref(), "--list".as_ref()],
        )?;
        check_configuration(&config)?;
        let filters = super::load::external_filters(&config)?;

        match &self.target {
            RequestTarget::Group(entries) => {
                check_sparse_index(&self.repository)?;
                let mut filter_paths = Vec::new();
                let mut write_paths = Vec::new();
                let mut seen_write_paths = HashSet::new();
                let mut seen_filter_paths = HashSet::new();

                for entry in entries {
                    if let Record::Untracked { directory: true } = entry.record {
                        let expanded = check_untracked_directory(&self.repository, &entry.path)?;
                        for file_path in expanded {
                            if seen_filter_paths.insert(file_path.clone()) {
                                push_nul_path(&mut filter_paths, &file_path);
                            }
                        }
                        if seen_write_paths.insert(entry.path.clone()) {
                            push_nul_path(&mut write_paths, &entry.path);
                        }
                    } else {
                        let allow_missing = matches!(&entry.record, Record::Tracked { .. });
                        check_group_entry_path(&self.repository, &entry.path, allow_missing)?;
                        if seen_filter_paths.insert(entry.path.clone()) {
                            push_nul_path(&mut filter_paths, &entry.path);
                        }
                        if seen_write_paths.insert(entry.path.clone()) {
                            push_nul_path(&mut write_paths, &entry.path);
                        }
                        if self.group == Group::Staged
                            && self.operation == Operation::Unstage
                            && let Record::Tracked {
                                rename: Some(rename),
                                ..
                            } = &entry.record
                        {
                            check_group_entry_path(&self.repository, &rename.source, true)?;
                            if seen_filter_paths.insert(rename.source.clone()) {
                                push_nul_path(&mut filter_paths, &rename.source);
                            }
                            if seen_write_paths.insert(rename.source.clone()) {
                                push_nul_path(&mut write_paths, &rename.source);
                            }
                        }
                    }
                }

                super::load::check_filters(
                    &self.repository,
                    &filter_paths,
                    &filters,
                    &AtomicBool::new(false),
                )?;

                let current = super::load::load(self.repository.clone(), &AtomicBool::new(false))?;
                if current.head() != &self.head {
                    return Err("Status, HEAD or index changed; refresh before writing".into());
                }
                let revalidated = current.index_request_group(self.group, self.operation)?;
                let RequestTarget::Group(current_entries) = &revalidated.target else {
                    unreachable!();
                };
                if current_entries != entries {
                    return Err("Status, HEAD or index changed; refresh before writing".into());
                }

                Ok(Prepared {
                    request: self,
                    input: Some(write_paths),
                })
            }
            RequestTarget::File(entry) | RequestTarget::Hunk { entry, .. } => {
                let patch = matches!(self.target, RequestTarget::Hunk { .. });
                let untracked_dir = matches!(entry.record, Record::Untracked { directory: true });
                let mut filter_paths = Vec::new();
                if untracked_dir {
                    for file_path in check_untracked_directory(&self.repository, &entry.path)? {
                        push_nul_path(&mut filter_paths, &file_path);
                    }
                } else {
                    check_path(&self.repository, entry, patch)?;
                    push_nul_path(&mut filter_paths, &entry.path);
                }
                super::load::check_filters(
                    &self.repository,
                    &filter_paths,
                    &filters,
                    &AtomicBool::new(false),
                )?;
                check_index(&self.repository, entry, !patch)?;
                let current = super::load::load(self.repository.clone(), &AtomicBool::new(false))?;
                if let RequestTarget::Hunk { file, hunk, .. } = &self.target {
                    if current.entries().iter().any(|other| {
                        (other.path == entry.path && !other.ordinary_index_file())
                            || matches!(
                                &other.record,
                                Record::Tracked {
                                    rename: Some(rename),
                                    ..
                                } if rename.source == entry.path
                            )
                    }) {
                        return Err(
                            "Special index or file types are not supported for hunk writes".into(),
                        );
                    }
                    let patch_text = file.hunk_patch(*hunk)?;
                    return Ok(Prepared {
                        request: self,
                        input: Some(patch_text.into_bytes()),
                    });
                }
                if current.head() != &self.head
                    || !current
                        .entries()
                        .iter()
                        .any(|other| other == entry && other.in_group(self.group))
                {
                    return Err("Status, HEAD or index changed; refresh before writing".into());
                }
                let index = current
                    .entries()
                    .iter()
                    .position(|other| other == entry)
                    .unwrap();
                current.index_request(self.group, index, self.operation)?;
                if !untracked_dir {
                    check_path(&self.repository, entry, false)?;
                }
                Ok(Prepared {
                    request: self,
                    input: None,
                })
            }
        }
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
        let args: Vec<&OsStr> = match &request.target {
            RequestTarget::Hunk { .. } => {
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
            }
            RequestTarget::File(entry) => match (&request.operation, &request.head) {
                (Operation::Stage, _) => {
                    vec!["add".as_ref(), "--".as_ref(), entry.path.as_os_str()]
                }
                (Operation::Unstage, Head::Unborn { .. }) => vec![
                    "update-index".as_ref(),
                    "--force-remove".as_ref(),
                    "--".as_ref(),
                    entry.path.as_os_str(),
                ],
                (Operation::Unstage, Head::Branch { oid, .. } | Head::Detached { oid }) => vec![
                    "restore".as_ref(),
                    "--staged".as_ref(),
                    "--source".as_ref(),
                    oid.as_ref(),
                    "--".as_ref(),
                    entry.path.as_os_str(),
                ],
            },
            RequestTarget::Group(_) => match (&request.operation, &request.head) {
                (Operation::Stage, _) => vec![
                    "add".as_ref(),
                    "--pathspec-from-file=-".as_ref(),
                    "--pathspec-file-nul".as_ref(),
                ],
                (Operation::Unstage, Head::Unborn { .. }) => vec![
                    "rm".as_ref(),
                    "--cached".as_ref(),
                    "--force".as_ref(),
                    "--pathspec-from-file=-".as_ref(),
                    "--pathspec-file-nul".as_ref(),
                ],
                (Operation::Unstage, Head::Branch { oid, .. } | Head::Detached { oid }) => vec![
                    "restore".as_ref(),
                    "--staged".as_ref(),
                    "--source".as_ref(),
                    oid.as_ref(),
                    "--pathspec-from-file=-".as_ref(),
                    "--pathspec-file-nul".as_ref(),
                ],
            },
        };
        let (outcome, diagnostic) = match git::execute(
            &request.repository.root,
            Some(&request.repository.git_dir),
            &args,
            self.input.as_deref(),
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
