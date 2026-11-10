//! Repository history and immutable commit revisions, separate from running commits.

use std::ffi::OsStr;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use crate::diff::{FileDiff, Visit};
pub use crate::git::Repository;
use crate::git::{ReadEnvironment, valid_oid};

mod combined;
mod log;
mod render;
#[cfg(test)]
pub(crate) mod tests;
pub(crate) use render::{Action, Location, RenderedHistory};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Query {
    Log {
        repository: Repository,
        revision: String,
        limit: usize,
        select: bool,
        environment: ReadEnvironment,
    },
    Revision {
        repository: Repository,
        oid: String,
        environment: ReadEnvironment,
    },
}

impl Query {
    pub fn repository(&self) -> &Repository {
        match self {
            Self::Log { repository, .. } | Self::Revision { repository, .. } => repository,
        }
    }

    pub fn environment(&self) -> ReadEnvironment {
        match self {
            Self::Log { environment, .. } | Self::Revision { environment, .. } => *environment,
        }
    }

    pub fn same_buffer(&self, other: &Self) -> bool {
        self.repository() == other.repository()
            && self.environment() == other.environment()
            && match (self, other) {
                (Self::Log { select: left, .. }, Self::Log { select: right, .. }) => left == right,
                (Self::Revision { .. }, Self::Revision { .. }) => true,
                _ => false,
            }
    }

    pub fn current(
        repository: Repository,
        select: bool,
        cancelled: &AtomicBool,
    ) -> Result<Self, String> {
        let environment = if select {
            ReadEnvironment::Commit
        } else {
            ReadEnvironment::Projection
        };
        let branch = environment.execute(
            &repository,
            &[
                "symbolic-ref".as_ref(),
                "--quiet".as_ref(),
                "--short".as_ref(),
                "HEAD".as_ref(),
            ],
            cancelled,
        )?;
        let revision = match branch.status.code() {
            Some(0) => String::from_utf8(branch.bytes)
                .map_err(|_| "Branch name is not UTF-8")?
                .trim_end_matches('\n')
                .to_owned(),
            Some(1) => "HEAD".into(),
            _ => return Err(format!("Cannot read current branch: {}", branch.diagnostic)),
        };
        Ok(Self::Log {
            repository,
            revision,
            limit: 256,
            select,
            environment,
        })
    }

    pub fn load(&self, cancelled: &AtomicBool) -> Result<Data, String> {
        match self {
            Self::Log {
                repository,
                revision,
                limit,
                environment,
                ..
            } => log::load(repository, revision, *limit, *environment, cancelled).map(Data::Log),
            Self::Revision {
                repository,
                oid,
                environment,
            } => Revision::load(repository, oid, *environment, cancelled).map(Data::Revision),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CommitInfo {
    pub oid: String,
    pub parents: Vec<String>,
    pub author: String,
    pub date: String,
    pub message: String,
    pub decorations: String,
}

#[derive(Clone, Debug)]
pub(crate) struct LogEntry {
    pub graph: String,
    pub commit: Option<CommitInfo>,
}

#[derive(Clone, Debug)]
pub(crate) struct Log {
    pub revision: String,
    pub entries: Vec<LogEntry>,
    pub more: bool,
}

#[derive(Clone, Debug)]
pub(crate) enum Patch {
    Unified(Vec<FileDiff>),
    Combined(Vec<combined::File>),
}

#[derive(Clone, Debug)]
pub(crate) struct Revision {
    pub info: CommitInfo,
    pub patch: Patch,
}

#[derive(Clone, Debug)]
pub(crate) enum Data {
    Log(Log),
    Revision(Revision),
}

impl Data {
    pub fn render(&self, query: &Query) -> Result<RenderedHistory, String> {
        render::render(self, query)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileVersion {
    pub(crate) name: String,
    pub(crate) visit: Visit,
}

#[derive(Clone, Debug)]
pub(crate) struct Blob {
    pub path: PathBuf,
    pub oid: String,
    pub mode: String,
}

impl Blob {
    fn visit(
        &self,
        repository: &Repository,
        line: usize,
        column: usize,
        environment: ReadEnvironment,
    ) -> Result<Visit, String> {
        if self.oid.bytes().all(|byte| byte == b'0') {
            return Err("No file version exists on this side".into());
        }
        if !matches!(self.mode.as_str(), "100644" | "100755") {
            return Err("This change has no supported text location".into());
        }
        Ok(Visit::Blob {
            root: repository.root.clone(),
            git_dir: repository.git_dir.clone(),
            oid: self.oid.clone(),
            path: self.path.clone(),
            environment,
            line: line.max(1),
            column,
        })
    }
}

impl Revision {
    fn load(
        repository: &Repository,
        oid: &str,
        environment: ReadEnvironment,
        cancelled: &AtomicBool,
    ) -> Result<Self, String> {
        if !valid_oid(oid) {
            return Err("Expected a complete commit identity".into());
        }
        let bytes = read_environment(
            environment,
            repository,
            &[
                "show",
                "-s",
                "--no-show-signature",
                "--encoding=UTF-8",
                "--format=%H%x00%P%x00%an <%ae>%x00%aI%x00%B%x00",
                "--end-of-options",
                oid,
                "--",
            ],
            cancelled,
        )?;
        let mut fields = bytes.split(|byte| *byte == 0);
        let mut next = || {
            fields
                .next()
                .ok_or_else(|| "Truncated commit metadata".to_owned())
                .and_then(utf8)
        };
        let actual = next()?.to_owned();
        if actual != oid {
            return Err("Commit identity mismatch".into());
        }
        let parents = next()?
            .split_whitespace()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if parents.iter().any(|oid| !valid_oid(oid)) {
            return Err("Invalid parent identity".into());
        }
        let author = next()?.to_owned();
        let date = next()?.to_owned();
        let message = next()?.to_owned();
        if fields.next() != Some(b"\n".as_slice()) || fields.next().is_some() {
            return Err("Malformed commit metadata".into());
        }
        let mut args = vec![
            "show",
            "--raw",
            "--patch",
            "-z",
            "--no-abbrev",
            "--full-index",
            "--format=",
            "--no-show-signature",
            "--no-prefix",
            "--find-renames",
            "--no-ext-diff",
            "--no-textconv",
            "--no-color",
            "--no-relative",
            "--submodule=short",
            "--unified=3",
            "--diff-algorithm=myers",
            "--no-indent-heuristic",
            "--word-diff=none",
            "--output-indicator-new=+",
            "--output-indicator-old=-",
            "--output-indicator-context= ",
        ];
        if parents.len() > 1 {
            args.extend(["--cc", "--combined-all-paths"]);
        }
        args.extend(["--end-of-options", oid, "--"]);
        let bytes = read_environment(environment, repository, &args, cancelled)?;
        let patch = if parents.len() > 1 {
            Patch::Combined(combined::parse(&bytes, parents.len())?)
        } else {
            Patch::Unified(crate::diff::parse::parse_revision(&bytes)?)
        };
        Ok(Self {
            info: CommitInfo {
                oid: actual,
                parents,
                author,
                date,
                message,
                decorations: String::new(),
            },
            patch,
        })
    }
}

fn read_environment(
    environment: ReadEnvironment,
    repository: &Repository,
    args: &[&str],
    cancelled: &AtomicBool,
) -> Result<Vec<u8>, String> {
    let args = args.iter().map(OsStr::new).collect::<Vec<_>>();
    environment.read(repository, &args, cancelled)
}
fn utf8(bytes: &[u8]) -> Result<&str, String> {
    std::str::from_utf8(bytes).map_err(|_| "Git history metadata is not UTF-8".into())
}
