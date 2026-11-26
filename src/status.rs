//! Git status data and section projection, independent of buffers and script lifetimes.

use std::path::PathBuf;

pub use crate::git::Repository;

#[cfg(test)]
mod hunk_tests;
mod hunks;
pub mod index;
mod load;
#[cfg(test)]
mod load_tests;
mod parse;
mod render;
#[cfg(test)]
mod tests;

pub use render::{RenderedStatus, StatusLocation, StatusTarget};
pub(crate) use render::{StatusAction, StatusPreviewTarget, StatusVisitSpan};

pub const MAX_RECORDS: usize = 4096;
pub const MAX_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Head {
    Branch { name: Vec<u8>, oid: String },
    Detached { oid: String },
    Unborn { name: Vec<u8> },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Group {
    Untracked,
    Unstaged,
    Staged,
    Unmerged,
}

impl Group {
    pub const ALL: [Self; 4] = [
        Self::Untracked,
        Self::Unstaged,
        Self::Staged,
        Self::Unmerged,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Untracked => "Untracked files",
            Self::Unstaged => "Unstaged changes",
            Self::Staged => "Staged changes",
            Self::Unmerged => "Unmerged paths",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Untracked => "untracked",
            Self::Unstaged => "unstaged",
            Self::Staged => "staged",
            Self::Unmerged => "unmerged",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Submodule {
    pub is_submodule: bool,
    pub commit_changed: bool,
    pub tracked_dirty: bool,
    pub untracked_dirty: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tracked {
    pub head_mode: u32,
    pub index_mode: u32,
    pub worktree_mode: u32,
    pub head_oid: String,
    pub index_oid: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rename {
    pub source: PathBuf,
    pub copy: bool,
    pub score: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Record {
    Tracked {
        state: Tracked,
        rename: Option<Rename>,
    },
    Unmerged {
        modes: [u32; 4],
        oids: [String; 3],
    },
    Untracked {
        directory: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub path: PathBuf,
    pub xy: [u8; 2],
    pub submodule: Submodule,
    pub record: Record,
}

impl Entry {
    pub fn in_group(&self, group: Group) -> bool {
        match self.record {
            Record::Untracked { .. } => group == Group::Untracked,
            Record::Unmerged { .. } => group == Group::Unmerged,
            Record::Tracked { .. } => match group {
                Group::Unstaged => self.xy[1] != b'.',
                Group::Staged => self.xy[0] != b'.',
                _ => false,
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefHeader {
    pub label: String,
    pub target: String,
    pub oid: Option<String>,
    pub summary: Option<String>,
    pub warning: Option<String>,
    pub is_remote: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TagInfo {
    pub name: String,
    pub count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitSummary {
    pub oid: String,
    pub short: String,
    pub summary: String,
    pub decorations: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitSection {
    pub key: String,
    pub title: String,
    pub range: Option<String>,
    pub commits: Vec<CommitSummary>,
    pub show_count: bool,
    pub collapsed_by_default: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StashSummary {
    pub ref_name: String,
    pub oid: String,
    pub summary: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StatusMeta {
    pub head_short: Option<String>,
    pub head_summary: Option<String>,
    pub upstream: Option<RefHeader>,
    pub push: Option<RefHeader>,
    pub current_tag: Option<TagInfo>,
    pub next_tag: Option<TagInfo>,
    pub merge_section: Option<CommitSection>,
    pub stashes: Vec<StashSummary>,
    pub log_sections: Vec<CommitSection>,
}

#[derive(Clone, Debug)]
pub struct Status {
    repository: Repository,
    head: Head,
    entries: Vec<Entry>,
    diffs: std::collections::HashMap<
        Group,
        std::collections::HashMap<PathBuf, std::sync::Arc<crate::diff::FileDiff>>,
    >,
    meta: Box<StatusMeta>,
}

impl Status {
    pub fn load(
        repository: Repository,
        cancelled: &std::sync::atomic::AtomicBool,
    ) -> Result<Self, String> {
        let mut status = load::load(repository, cancelled)?;
        status.load_diffs(cancelled)?;
        Ok(status)
    }

    pub fn parse(repository: Repository, bytes: &[u8]) -> Result<Self, String> {
        parse::parse(repository, bytes)
    }

    pub fn repository(&self) -> &Repository {
        &self.repository
    }

    pub fn head(&self) -> &Head {
        &self.head
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn meta(&self) -> &StatusMeta {
        &self.meta
    }

    pub fn group(&self, group: Group) -> impl Iterator<Item = (usize, &Entry)> {
        self.entries
            .iter()
            .enumerate()
            .filter(move |(_, entry)| entry.in_group(group))
    }

    pub(crate) fn file_diff(
        &self,
        group: Group,
        entry: usize,
    ) -> Option<&std::sync::Arc<crate::diff::FileDiff>> {
        self.diffs.get(&group)?.get(&self.entries.get(entry)?.path)
    }

    pub fn render(&self) -> Result<RenderedStatus, String> {
        render::render(self)
    }
}
