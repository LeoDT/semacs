//! Project discovery, known-project list, and project file listing following
//! GNU Emacs `project.el`.

use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use gpui_kit::Global;

use crate::completion::MatchedCandidate;
use crate::files;
use crate::persisted::{self, List};

/// Choice appended to the known-project selector, matching
/// `project-prompt-project-dir` upstream.
pub const CHOOSE_DIR_LABEL: &str = "... (choose a dir)";

const PROJECT_LIST_CAPACITY: usize = 1024;
const MAX_ENTRY_BYTES: usize = 4096;
const MAX_PROJECT_FILES: usize = 200_000;
const MAX_TRANSIENT_DIRS: usize = 50_000;
const MAX_SUBMODULE_DEPTH: usize = 8;
const MAX_GITMODULES_BYTES: u64 = 1024 * 1024;

/// Upstream `vc-directory-exclusion-list` used when listing transient projects.
const VC_EXCLUSION_DIRS: &[&str] = &[
    "SCCS",
    "RCS",
    "CVS",
    "MCVS",
    ".src",
    ".svn",
    ".git",
    ".hg",
    ".bzr",
    "_MTN",
    "_darcs",
    "{arch}",
    ".arch-ids",
    ".fslckout",
    ".got",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectKind {
    /// Recognized Git project (`.git` directory or file).
    Git,
    /// User-selected directory without a recognized VCS; not persisted.
    Transient,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Project {
    pub root: PathBuf,
    pub kind: ProjectKind,
}

impl Project {
    pub fn git(root: PathBuf) -> Self {
        Self {
            root,
            kind: ProjectKind::Git,
        }
    }

    pub fn transient(root: PathBuf) -> Self {
        Self {
            root,
            kind: ProjectKind::Transient,
        }
    }

    /// Human-readable project name, matching `project-name`.
    pub fn name(&self) -> String {
        self.root
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| self.root.to_string_lossy().into_owned())
    }

    /// Whether `directory` resides inside this project root, matching default
    /// `project-buffers` directory prefix semantics (falling back to canonical
    /// paths when symlinks differ, e.g. between a symlinked project root and a
    /// canonicalized Git worktree).
    pub fn contains_directory(&self, directory: &Path) -> bool {
        self.contains_directory_with_canonical(
            directory,
            std::fs::canonicalize(&self.root).ok().as_deref(),
        )
    }

    pub(crate) fn contains_directory_with_canonical(
        &self,
        directory: &Path,
        canonical_root: Option<&Path>,
    ) -> bool {
        if directory.starts_with(&self.root) {
            return true;
        }
        let Some(root) = canonical_root else {
            return false;
        };
        std::fs::canonicalize(directory)
            .map(|dir| dir.starts_with(root))
            .unwrap_or(false)
    }
}

/// Finds the nearest `.git` directory or file, falling back to the buffer's
/// directory. Relative file names are resolved against `working_directory`.
pub fn root_for(file: Option<&Path>, working_directory: &Path) -> io::Result<PathBuf> {
    let directory = match file {
        Some(file) => {
            let path = files::expand(file, working_directory)?;
            path.parent()
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "File has no parent"))?
                .to_path_buf()
        }
        None => files::expand(Path::new("."), working_directory)?,
    };
    Ok(find_git_root(&directory).unwrap_or(directory))
}

/// Finds a recognized VCS project containing `directory`, returning `None` when
/// no `.git` marker is found (matching `project--find-in-directory`).
pub fn find_in_directory(directory: &Path) -> Option<Project> {
    let normalized = files::expand(Path::new("."), directory).ok()?;
    find_git_root(&normalized).map(Project::git)
}

fn find_git_root(directory: &Path) -> Option<PathBuf> {
    directory
        .ancestors()
        .find(|candidate| {
            std::fs::metadata(candidate.join(".git"))
                .is_ok_and(|metadata| metadata.is_file() || metadata.is_dir())
        })
        .map(Path::to_path_buf)
}

/// Checks the fixed root itself; this does not rediscover a different project.
pub fn is_available(root: &Path) -> bool {
    root.is_dir()
}

/// Known project roots ordered most-recently-used first and mirrored to
/// `$XDG_STATE_HOME/semacs/projects`.
pub struct ProjectList {
    list: List,
}

impl Default for ProjectList {
    fn default() -> Self {
        Self {
            list: List::new(PROJECT_LIST_CAPACITY),
        }
    }
}

impl Global for ProjectList {}

impl ProjectList {
    pub fn entries(&self) -> &[String] {
        self.list.entries()
    }

    pub fn roots(&self) -> Vec<PathBuf> {
        self.list.entries().iter().map(PathBuf::from).collect()
    }

    pub fn contains(&self, root: &Path) -> bool {
        let Some(key) = canonical_key(root) else {
            return false;
        };
        self.list.entries().iter().any(|entry| entry == &key)
    }

    /// Moves `root` to the front of the known project list and persists if
    /// changed.
    pub fn remember(&mut self, root: &Path) -> bool {
        let Some(key) = canonical_key(root) else {
            return false;
        };
        self.list.promote(&key)
    }

    /// Removes `root` from the known project list. Returns whether it was
    /// present.
    pub fn forget(&mut self, root: &Path) -> bool {
        let Some(key) = canonical_key(root) else {
            return false;
        };
        self.list.remove(&key)
    }

    /// Removes entries whose directories no longer exist on disk, returning how
    /// many were forgotten.
    pub fn prune_zombies(&mut self) -> usize {
        self.list.retain(|entry| Path::new(entry).is_dir())
    }
}

/// Opens the known-project list stored at `path`, starting empty when missing
/// or unreadable.
pub fn open_list(path: PathBuf) -> io::Result<(ProjectList, persisted::Persistence)> {
    let (mut list, persistence) = persisted::open(path, PROJECT_LIST_CAPACITY, valid_entry)?;
    list.dedup_in_place();
    Ok((ProjectList { list }, persistence))
}

fn valid_entry(entry: &str) -> bool {
    valid_raw_entry(entry) && canonical_key(Path::new(entry)).as_deref() == Some(entry)
}

fn valid_raw_entry(entry: &str) -> bool {
    !entry.is_empty()
        && entry.len() <= MAX_ENTRY_BYTES
        && !entry.contains(['\n', '\r'])
        && Path::new(entry).is_absolute()
}

fn canonical_key(root: &Path) -> Option<String> {
    let expanded = files::expand(root, Path::new("/")).ok()?;
    let text = expanded.to_str()?;
    valid_raw_entry(text).then(|| text.to_owned())
}

/// Lists project files as `/`-separated paths relative to `project.root`.
///
/// Git projects mirror Emacs `vc-git-project-list-files` (tracked + untracked
/// respecting `.gitignore`, merging initialized submodules). Transient projects
/// walk the directory while skipping standard VCS directories.
pub fn list_files(project: &Project, cancelled: &AtomicBool) -> Result<Vec<String>, String> {
    let mut files = match project.kind {
        ProjectKind::Git => {
            let mut out = Vec::new();
            let mut visited = HashSet::new();
            if let Ok(canon) = std::fs::canonicalize(&project.root) {
                visited.insert(canon);
            }
            list_git_files(&project.root, "", 0, &mut visited, &mut out, cancelled)?;
            out
        }
        ProjectKind::Transient => {
            let mut out = Vec::new();
            list_transient_files(&project.root, &mut out, cancelled)?;
            out
        }
    };
    if cancelled.load(Ordering::Relaxed) {
        return Err("File listing cancelled".into());
    }
    files.sort();
    files.dedup();
    Ok(files)
}

fn list_git_files(
    dir: &Path,
    prefix: &str,
    depth: usize,
    visited: &mut HashSet<PathBuf>,
    out: &mut Vec<String>,
    cancelled: &AtomicBool,
) -> Result<(), String> {
    if cancelled.load(Ordering::Relaxed) {
        return Err("File listing cancelled".into());
    }
    let bytes = crate::git::read(
        dir,
        None,
        &[
            "ls-files".as_ref(),
            "-z".as_ref(),
            "-c".as_ref(),
            "--exclude-standard".as_ref(),
            "-o".as_ref(),
        ],
        cancelled,
    )?;
    let submodules = git_submodules(dir);
    let submodule_set: HashSet<&str> = submodules.iter().map(String::as_str).collect();
    for entry in bytes.split(|byte| *byte == 0) {
        if cancelled.load(Ordering::Relaxed) {
            return Err("File listing cancelled".into());
        }
        if entry.is_empty() {
            continue;
        }
        let Ok(relative) = std::str::from_utf8(entry) else {
            continue;
        };
        if relative.ends_with('/') || submodule_set.contains(relative) {
            continue;
        }
        if out.len() >= MAX_PROJECT_FILES {
            return Err(format!(
                "Project file list exceeds {MAX_PROJECT_FILES} entries"
            ));
        }
        if prefix.is_empty() {
            out.push(relative.to_owned());
        } else {
            out.push(format!("{prefix}{relative}"));
        }
    }
    if depth < MAX_SUBMODULE_DEPTH {
        for module in submodules {
            if cancelled.load(Ordering::Relaxed) {
                return Err("File listing cancelled".into());
            }
            if !is_real_subdirectory(dir, Path::new(&module)) {
                continue;
            }
            let sub_dir = dir.join(&module);
            if std::fs::symlink_metadata(sub_dir.join(".git")).is_err() {
                continue;
            }
            let Ok(canon) = std::fs::canonicalize(&sub_dir) else {
                continue;
            };
            if !visited.insert(canon) {
                continue;
            }
            let sub_prefix = if prefix.is_empty() {
                format!("{module}/")
            } else {
                format!("{prefix}{module}/")
            };
            list_git_files(&sub_dir, &sub_prefix, depth + 1, visited, out, cancelled)?;
        }
    }
    Ok(())
}

/// Checks every path component of `relative` under `base` with
/// `symlink_metadata` so a symlink cannot escape `base`.
fn is_real_subdirectory(base: &Path, relative: &Path) -> bool {
    let mut current = base.to_path_buf();
    for component in relative.components() {
        let std::path::Component::Normal(part) = component else {
            return false;
        };
        current.push(part);
        let Ok(metadata) = std::fs::symlink_metadata(&current) else {
            return false;
        };
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return false;
        }
    }
    true
}

/// Parses `.gitmodules` `path = ...` lines, matching `project--git-submodules`,
/// with a bounded read and deduplication.
fn git_submodules(dir: &Path) -> Vec<String> {
    use std::io::Read;
    let Ok(file) = std::fs::File::open(dir.join(".gitmodules")) else {
        return Vec::new();
    };
    let mut text = String::new();
    if file
        .take(MAX_GITMODULES_BYTES + 1)
        .read_to_string(&mut text)
        .is_err()
        || text.len() as u64 > MAX_GITMODULES_BYTES
    {
        return Vec::new();
    }
    let mut seen = HashSet::new();
    let mut modules = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix("path") else {
            continue;
        };
        let rest = rest.trim_start();
        let Some(value) = rest.strip_prefix('=') else {
            continue;
        };
        let value = value.trim().trim_end_matches('/');
        if !value.is_empty()
            && crate::git::relative_path(value.as_bytes()).is_ok()
            && seen.insert(value.to_owned())
        {
            modules.push(value.to_owned());
        }
    }
    modules
}

fn list_transient_files(
    root: &Path,
    out: &mut Vec<String>,
    cancelled: &AtomicBool,
) -> Result<(), String> {
    let mut stack = vec![root.to_path_buf()];
    let mut visited_dirs = 0usize;
    while let Some(dir) = stack.pop() {
        if cancelled.load(Ordering::Relaxed) {
            return Err("File listing cancelled".into());
        }
        visited_dirs += 1;
        if visited_dirs > MAX_TRANSIENT_DIRS {
            return Err(format!(
                "Project directory traversal exceeds {MAX_TRANSIENT_DIRS} directories"
            ));
        }
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if dir == root => {
                return Err(format!("File listing failed: {}: {error}", dir.display()));
            }
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            if cancelled.load(Ordering::Relaxed) {
                return Err("File listing cancelled".into());
            }
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if file_type.is_dir() {
                if VC_EXCLUSION_DIRS.contains(&name.as_str()) {
                    continue;
                }
                stack.push(path);
            } else if file_type.is_file() || (file_type.is_symlink() && path.is_file()) {
                if out.len() >= MAX_PROJECT_FILES {
                    return Err(format!(
                        "Project file list exceeds {MAX_PROJECT_FILES} entries"
                    ));
                }
                if let Ok(rel) = path.strip_prefix(root) {
                    let mut text = String::new();
                    for (index, component) in rel.components().enumerate() {
                        if index > 0 {
                            text.push('/');
                        }
                        text.push_str(&component.as_os_str().to_string_lossy());
                    }
                    if !text.is_empty() {
                        out.push(text);
                    }
                }
            }
        }
    }
    Ok(())
}

/// Ranks project file candidates with Vertico's `history -> length -> alpha`
/// rule, translating `file-name-history` entries under `root` to project-relative
/// paths and skipping the current buffer's file.
pub fn rank_project_files(
    items: &mut [MatchedCandidate],
    history: &[String],
    root: &Path,
    current: Option<&Path>,
) {
    let relative_history: Vec<String> = history
        .iter()
        .filter_map(|entry| {
            let expanded = files::expand(Path::new(entry), root).ok()?;
            if current.is_some_and(|current| current == expanded) {
                return None;
            }
            let rel = expanded.strip_prefix(root).ok()?;
            let rel_str = rel.to_str()?;
            (!rel_str.is_empty()).then(|| rel_str.replace(std::path::MAIN_SEPARATOR, "/"))
        })
        .collect();
    let ranks = crate::completion::history_ranks(&relative_history, Some);
    items.sort_by(|a, b| {
        let rank = |item: &MatchedCandidate| {
            ranks
                .get(item.candidate.label.as_str())
                .copied()
                .unwrap_or(i64::MAX)
        };
        rank(a)
            .cmp(&rank(b))
            .then_with(|| {
                a.candidate
                    .label
                    .chars()
                    .count()
                    .cmp(&b.candidate.label.chars().count())
            })
            .then_with(|| a.candidate.label.cmp(&b.candidate.label))
    });
}

/// Discovers Git project roots located directly under `directory` (or
/// recursively when `recursive` is true), matching `project-remember-projects-under`.
pub fn projects_under(
    directory: &Path,
    recursive: bool,
    cancelled: &AtomicBool,
) -> Result<Vec<PathBuf>, String> {
    let base = files::expand(Path::new("."), directory)
        .map_err(|error| format!("{}: {error}", directory.display()))?;
    let mut found = Vec::new();
    let mut seen = HashSet::new();
    scan_projects_under(&base, recursive, &mut seen, &mut found, cancelled)?;
    found.sort();
    Ok(found)
}

fn scan_projects_under(
    root: &Path,
    recursive: bool,
    seen: &mut HashSet<PathBuf>,
    found: &mut Vec<PathBuf>,
    cancelled: &AtomicBool,
) -> Result<(), String> {
    let mut stack = vec![root.to_path_buf()];
    let mut visited_dirs = 0usize;
    while let Some(dir) = stack.pop() {
        if cancelled.load(Ordering::Relaxed) {
            return Err("Project scan cancelled".into());
        }
        visited_dirs += 1;
        if visited_dirs > MAX_TRANSIENT_DIRS {
            return Err(format!(
                "Project scan exceeds {MAX_TRANSIENT_DIRS} directories"
            ));
        }
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if dir == root => return Err(format!("{}: {error}", dir.display())),
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            if cancelled.load(Ordering::Relaxed) {
                return Err("Project scan cancelled".into());
            }
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            let is_dir = if recursive {
                file_type.is_dir()
            } else {
                file_type.is_dir() || (file_type.is_symlink() && path.is_dir())
            };
            if !is_dir {
                continue;
            }
            if let Some(name) = entry.file_name().to_str()
                && VC_EXCLUSION_DIRS.contains(&name)
            {
                continue;
            }
            if let Some(project) = find_in_directory(&path)
                && seen.insert(project.root.clone())
            {
                found.push(project.root);
            }
            if recursive && file_type.is_dir() {
                stack.push(path);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "project_tests.rs"]
mod tests;
