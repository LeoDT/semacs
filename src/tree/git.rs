//! Bounded, cancellable Git status projection for the folder tree.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use crate::git::ReadEnvironment;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum GitNodeState {
    Ignored,
    Untracked,
    Added,
    Modified,
    Conflict,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeGitStatus {
    tree_root: PathBuf,
    root_inherited: Option<GitNodeState>,
    files: HashMap<PathBuf, GitNodeState>,
    directories: HashMap<PathBuf, GitNodeState>,
    ignored_dirs: HashSet<PathBuf>,
    untracked_dirs: HashSet<PathBuf>,
}

impl TreeGitStatus {
    /// Loads repository status for `tree_root` on a background thread.
    /// Returns `None` outside Git repositories, on cancellation, or on errors
    /// so tree browsing never blocks or fails when Git status is unavailable.
    pub fn load(tree_root: &Path, cancelled: &AtomicBool) -> Option<Self> {
        let canonical_tree_root = tree_root.canonicalize().ok()?;
        let repo = crate::git::discover(&canonical_tree_root, cancelled).ok()?;
        let bytes = ReadEnvironment::Projection
            .read(
                &repo,
                &[
                    "status".as_ref(),
                    "--porcelain=v1".as_ref(),
                    "--ignored=matching".as_ref(),
                    "--untracked-files=normal".as_ref(),
                    "-z".as_ref(),
                ],
                cancelled,
            )
            .ok()?;
        Some(Self::parse(
            tree_root.to_path_buf(),
            &canonical_tree_root,
            &repo.root,
            &bytes,
        ))
    }

    pub(crate) fn parse(
        tree_root: PathBuf,
        canonical_tree_root: &Path,
        repo_root: &Path,
        bytes: &[u8],
    ) -> Self {
        let mut status = Self {
            tree_root,
            root_inherited: None,
            files: HashMap::new(),
            directories: HashMap::new(),
            ignored_dirs: HashSet::new(),
            untracked_dirs: HashSet::new(),
        };
        let repo_prefix = canonical_tree_root.strip_prefix(repo_root).ok();
        let tree_prefix = if repo_prefix.is_none() {
            repo_root.strip_prefix(canonical_tree_root).ok()
        } else {
            None
        };
        if repo_prefix.is_none() && tree_prefix.is_none() {
            return status;
        }
        let records: Vec<&[u8]> = bytes
            .strip_suffix(&[0])
            .unwrap_or(bytes)
            .split(|byte| *byte == 0)
            .filter(|record| !record.is_empty())
            .collect();
        let mut index = 0;
        while index < records.len() {
            let record = records[index];
            index += 1;
            if record.len() < 4 || record[2] != b' ' {
                continue;
            }
            let x = record[0];
            let y = record[1];
            if matches!(x, b'R' | b'C') || matches!(y, b'R' | b'C') {
                // Porcelain v1 `-z` emits `<new>\0<orig>\0` for renames/copies.
                if index < records.len() {
                    index += 1;
                }
            }
            let raw_path = &record[3..];
            let is_dir = raw_path.ends_with(b"/");
            let trimmed = if is_dir {
                &raw_path[..raw_path.len() - 1]
            } else {
                raw_path
            };
            let Ok(repo_rel) = crate::git::relative_path(trimmed) else {
                continue;
            };
            let Some(state) = classify_xy(x, y) else {
                continue;
            };
            if let Some(prefix) = repo_prefix
                && !prefix.as_os_str().is_empty()
                && is_dir
                && prefix.starts_with(&repo_rel)
                && matches!(state, GitNodeState::Ignored | GitNodeState::Untracked)
            {
                status.root_inherited = Some(
                    status
                        .root_inherited
                        .map_or(state, |current| current.max(state)),
                );
                continue;
            }
            let tree_rel = if let Some(prefix) = repo_prefix {
                if prefix.as_os_str().is_empty() {
                    repo_rel
                } else {
                    let Ok(stripped) = repo_rel.strip_prefix(prefix) else {
                        continue;
                    };
                    stripped.to_path_buf()
                }
            } else if let Some(prefix) = tree_prefix {
                prefix.join(repo_rel)
            } else {
                continue;
            };
            if tree_rel.as_os_str().is_empty() {
                continue;
            }
            if state == GitNodeState::Ignored {
                if is_dir {
                    status.ignored_dirs.insert(tree_rel.clone());
                    merge_state(&mut status.directories, tree_rel, state);
                } else {
                    merge_state(&mut status.files, tree_rel, state);
                }
                continue;
            }
            if is_dir {
                if state == GitNodeState::Untracked {
                    status.untracked_dirs.insert(tree_rel.clone());
                }
                merge_state(&mut status.directories, tree_rel.clone(), state);
            } else {
                merge_state(&mut status.files, tree_rel.clone(), state);
            }
            for ancestor in tree_rel.ancestors().skip(1) {
                if ancestor.as_os_str().is_empty() {
                    break;
                }
                merge_state(&mut status.directories, ancestor.to_path_buf(), state);
            }
        }
        status
    }

    pub fn state_for(&self, path: &Path, target: &Path, directory: bool) -> Option<GitNodeState> {
        let rel_path = path.strip_prefix(&self.tree_root).ok()?;
        if directory {
            let rel_target = target.strip_prefix(&self.tree_root).unwrap_or(rel_path);
            if let Some(&state) = self.directories.get(rel_target) {
                return Some(state);
            }
            for ancestor in rel_target.ancestors() {
                if ancestor.as_os_str().is_empty() {
                    break;
                }
                if self.ignored_dirs.contains(ancestor) {
                    return Some(GitNodeState::Ignored);
                }
                if self.untracked_dirs.contains(ancestor) {
                    return Some(GitNodeState::Untracked);
                }
            }
            return self.root_inherited;
        }
        if let Some(&state) = self.files.get(rel_path) {
            return Some(state);
        }
        if let Some(parent) = rel_path.parent() {
            for ancestor in parent.ancestors() {
                if ancestor.as_os_str().is_empty() {
                    break;
                }
                if self.ignored_dirs.contains(ancestor) {
                    return Some(GitNodeState::Ignored);
                }
                if self.untracked_dirs.contains(ancestor) {
                    return Some(GitNodeState::Untracked);
                }
            }
        }
        self.root_inherited
    }
}

fn merge_state(map: &mut HashMap<PathBuf, GitNodeState>, path: PathBuf, state: GitNodeState) {
    map.entry(path)
        .and_modify(|current| *current = (*current).max(state))
        .or_insert(state);
}

fn classify_xy(x: u8, y: u8) -> Option<GitNodeState> {
    if matches!(
        [x, y],
        [b'D', b'D']
            | [b'A', b'U']
            | [b'U', b'D']
            | [b'U', b'A']
            | [b'D', b'U']
            | [b'A', b'A']
            | [b'U', b'U']
    ) {
        return Some(GitNodeState::Conflict);
    }
    match [x, y] {
        [b'!', b'!'] => Some(GitNodeState::Ignored),
        [b'?', b'?'] => Some(GitNodeState::Untracked),
        [b' ', b' '] => None,
        [b'A' | b'R' | b'C', b' '] => Some(GitNodeState::Added),
        _ => Some(GitNodeState::Modified),
    }
}
