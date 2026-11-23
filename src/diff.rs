//! A unified Git comparison, independent of views and application focus.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

pub(crate) mod parse;
mod patch;
pub(crate) mod render;
#[cfg(test)]
pub(crate) mod tests;
pub use crate::git::{ReadEnvironment, Repository};
pub(crate) use render::{DiffNavigation, RenderedDiff};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiffKind {
    Staged,
    Unstaged,
    Worktree,
    Amend,
    RevisionToWorktree {
        spec: String,
        base_oid: String,
    },
    Range {
        spec: String,
        left_oid: String,
        right_oid: String,
        three_dot: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ParsedRange<'a> {
    Single(&'a str),
    Range {
        left: Option<&'a str>,
        right: Option<&'a str>,
        three_dot: bool,
    },
}

pub(crate) fn parse_range(input: &str) -> Result<ParsedRange<'_>, String> {
    if input.is_empty() {
        return Err("Empty revision or range".into());
    }
    if input.starts_with(":/") {
        return Ok(ParsedRange::Single(input));
    }
    if let Some(pos) = input.find("..") {
        let rest = &input[pos..];
        let three_dot = rest.starts_with("...");
        let sep_len = if three_dot { 3 } else { 2 };
        let left = &input[..pos];
        let right = &input[pos + sep_len..];
        if (left.is_empty() && right.is_empty())
            || left.ends_with('.')
            || right.starts_with('.')
            || right.contains("..")
            || left.contains([' ', '\t', '\n', '\r'])
            || right.contains([' ', '\t', '\n', '\r'])
        {
            return Err(format!("Invalid range: {input}"));
        }
        return Ok(ParsedRange::Range {
            left: (!left.is_empty()).then_some(left),
            right: (!right.is_empty()).then_some(right),
            three_dot,
        });
    }
    Ok(ParsedRange::Single(input))
}

impl DiffKind {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Staged => "staged",
            Self::Unstaged => "unstaged",
            Self::Worktree => "worktree",
            Self::Amend => "amend",
            Self::RevisionToWorktree { .. } | Self::Range { .. } => "range",
        }
    }

    pub fn is_range_or_revision(&self) -> bool {
        matches!(self, Self::RevisionToWorktree { .. } | Self::Range { .. })
    }

    pub fn resolve_range(
        repository: &Repository,
        input: &str,
        environment: ReadEnvironment,
        cancelled: &AtomicBool,
    ) -> Result<Self, String> {
        match parse_range(input)? {
            ParsedRange::Single(rev) => {
                let base_oid = crate::git::resolve_commit(repository, rev, environment, cancelled)?;
                Ok(Self::RevisionToWorktree {
                    spec: rev.to_owned(),
                    base_oid,
                })
            }
            ParsedRange::Range {
                left,
                right,
                three_dot,
            } => {
                let left_rev = left.unwrap_or("HEAD");
                let right_rev = right.unwrap_or("HEAD");
                let left_oid =
                    crate::git::resolve_commit(repository, left_rev, environment, cancelled)?;
                let right_oid =
                    crate::git::resolve_commit(repository, right_rev, environment, cancelled)?;
                let sep = if three_dot { "..." } else { ".." };
                Ok(Self::Range {
                    spec: format!("{left_rev}{sep}{right_rev}"),
                    left_oid,
                    right_oid,
                    three_dot,
                })
            }
        }
    }

    pub fn switch_range_type(&self) -> Result<Self, String> {
        let Self::Range {
            spec,
            left_oid,
            right_oid,
            three_dot,
        } = self
        else {
            return Err("No range to change".into());
        };
        let next_three_dot = !three_dot;
        let next_spec = match parse_range(spec) {
            Ok(ParsedRange::Range { left, right, .. }) => format!(
                "{}{}{}",
                left.unwrap_or("HEAD"),
                if next_three_dot { "..." } else { ".." },
                right.unwrap_or("HEAD")
            ),
            _ => format!(
                "{}{}{}",
                &left_oid[..12.min(left_oid.len())],
                if next_three_dot { "..." } else { ".." },
                &right_oid[..12.min(right_oid.len())]
            ),
        };
        Ok(Self::Range {
            spec: next_spec,
            left_oid: left_oid.clone(),
            right_oid: right_oid.clone(),
            three_dot: next_three_dot,
        })
    }

    pub fn flip_revs(&self) -> Result<Self, String> {
        let Self::Range {
            spec,
            left_oid,
            right_oid,
            three_dot,
        } = self
        else {
            return Err("No range to swap".into());
        };
        let sep = if *three_dot { "..." } else { ".." };
        let next_spec = match parse_range(spec) {
            Ok(ParsedRange::Range { left, right, .. }) => {
                format!("{}{sep}{}", right.unwrap_or("HEAD"), left.unwrap_or("HEAD"))
            }
            _ => format!(
                "{}{sep}{}",
                &right_oid[..12.min(right_oid.len())],
                &left_oid[..12.min(left_oid.len())]
            ),
        };
        Ok(Self::Range {
            spec: next_spec,
            left_oid: right_oid.clone(),
            right_oid: left_oid.clone(),
            three_dot: *three_dot,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Comparison {
    pub repository: Repository,
    pub kind: DiffKind,
    pub environment: ReadEnvironment,
}

impl Comparison {
    pub fn same_buffer(&self, other: &Self) -> bool {
        self.repository == other.repository
            && self.environment == other.environment
            && matches!(
                (&self.kind, &other.kind),
                (DiffKind::Staged, DiffKind::Staged)
                    | (DiffKind::Unstaged, DiffKind::Unstaged)
                    | (DiffKind::Worktree, DiffKind::Worktree)
                    | (DiffKind::Amend, DiffKind::Amend)
                    | (
                        DiffKind::Range { .. } | DiffKind::RevisionToWorktree { .. },
                        DiffKind::Range { .. } | DiffKind::RevisionToWorktree { .. },
                    )
            )
    }
}

#[derive(Clone, Debug)]
pub struct FileDiff {
    pub old_path: PathBuf,
    pub new_path: PathBuf,
    pub old_oid: String,
    pub new_oid: String,
    pub status: char,
    pub old_mode: String,
    pub new_mode: String,
    pub added: Option<usize>,
    pub deleted: Option<usize>,
    pub hunks: Vec<Hunk>,
    pub summary: Option<String>,
    pub(crate) patch_header: String,
}
#[derive(Clone, Debug)]
pub struct Hunk {
    pub header: String,
    pub old_start: usize,
    pub new_start: usize,
    pub lines: Vec<DiffLine>,
}
#[derive(Clone, Debug)]
pub struct DiffLine {
    pub text: String,
    pub old_line: Option<usize>,
    pub new_line: Option<usize>,
}
#[derive(Clone, Debug)]
pub struct Diff {
    pub comparison: Comparison,
    pub files: Vec<FileDiff>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Visit {
    Blob {
        root: PathBuf,
        git_dir: PathBuf,
        oid: String,
        path: PathBuf,
        environment: ReadEnvironment,
        line: usize,
        column: usize,
    },
    Worktree {
        path: PathBuf,
        line: usize,
        column: usize,
    },
}

fn worktree_base(
    repository: &Repository,
    cancelled: &AtomicBool,
    environment: crate::git::ReadEnvironment,
    revision: &str,
) -> Result<String, String> {
    let head = environment.execute(
        repository,
        &[
            "rev-parse".as_ref(),
            "--verify".as_ref(),
            "--quiet".as_ref(),
            revision.as_ref(),
        ],
        cancelled,
    )?;
    let bytes = match head.status.code() {
        Some(0) => head.bytes,
        Some(1) => environment.read(
            repository,
            &[
                "hash-object".as_ref(),
                "-t".as_ref(),
                "tree".as_ref(),
                "--stdin".as_ref(),
            ],
            cancelled,
        )?,
        _ => {
            return Err(format!(
                "Cannot read HEAD ({}): {}",
                head.status, head.diagnostic
            ));
        }
    };
    String::from_utf8(bytes).map_err(|error| error.to_string())
}

impl Diff {
    pub(crate) fn load(comparison: Comparison, cancelled: &AtomicBool) -> Result<Self, String> {
        Self::load_paths(comparison, &[], false, cancelled).map(|(diff, _)| diff)
    }

    pub(crate) fn load_status(
        comparison: Comparison,
        paths: &[PathBuf],
        cancelled: &AtomicBool,
    ) -> Result<(Self, usize), String> {
        Self::load_paths(comparison, paths, true, cancelled)
    }

    fn load_paths(
        comparison: Comparison,
        paths: &[PathBuf],
        status: bool,
        cancelled: &AtomicBool,
    ) -> Result<(Self, usize), String> {
        let environment = comparison.environment;
        let mut args = vec![
            "diff",
            "--raw",
            "--numstat",
            "--patch",
            "-z",
            "--no-abbrev",
            "--full-index",
            "--find-renames",
            "--no-ext-diff",
            "--no-textconv",
            "--no-color",
            "--no-relative",
            "--no-prefix",
            "--submodule=short",
            "--ignore-submodules=none",
            "--diff-algorithm=myers",
            "--no-indent-heuristic",
            "--unified=3",
            "--word-diff=none",
            "--output-indicator-new=+",
            "--output-indicator-old=-",
            "--output-indicator-context= ",
        ];
        let base = match &comparison.kind {
            DiffKind::Worktree => Some(worktree_base(
                &comparison.repository,
                cancelled,
                environment,
                "HEAD",
            )?),
            DiffKind::Amend => Some(worktree_base(
                &comparison.repository,
                cancelled,
                environment,
                "HEAD^",
            )?),
            DiffKind::RevisionToWorktree { base_oid, .. } => {
                if !crate::git::valid_oid(base_oid) {
                    return Err("Expected a complete commit identity".into());
                }
                Some(base_oid.clone())
            }
            DiffKind::Range {
                left_oid,
                right_oid,
                three_dot,
                ..
            } => {
                if !crate::git::valid_oid(left_oid) || !crate::git::valid_oid(right_oid) {
                    return Err("Expected a complete commit identity".into());
                }
                Some(format!(
                    "{left_oid}{}{right_oid}",
                    if *three_dot { "..." } else { ".." }
                ))
            }
            DiffKind::Staged | DiffKind::Unstaged => None,
        };
        if status {
            args.push("--diff-filter=ACDMRT");
        }
        if matches!(comparison.kind, DiffKind::Staged | DiffKind::Amend) {
            args.push("--cached");
        }
        if let Some(base) = &base {
            args.push(base.trim());
        }
        args.push("--");
        let mut args: Vec<_> = args.into_iter().map(std::ffi::OsStr::new).collect();
        args.extend(paths.iter().map(|path| path.as_os_str()));
        let bytes = {
            #[cfg(feature = "native-profiler")]
            let _span = crate::preparation_trace::Span::new("diff-git", 0);
            environment.read(&comparison.repository, &args, cancelled)?
        };
        #[cfg(feature = "native-profiler")]
        let _span = crate::preparation_trace::Span::new("diff-parse", bytes.len());
        let summarize_non_utf8 = status || comparison.kind.is_range_or_revision();
        let files = parse::parse_text(&bytes, summarize_non_utf8)?;
        Ok((Self { comparison, files }, bytes.len()))
    }

    pub(crate) fn render(&self) -> Result<RenderedDiff, String> {
        #[cfg(feature = "native-profiler")]
        let _span = crate::preparation_trace::Span::new("diff-render", 0);
        render::render(self)
    }

    pub(crate) fn visit(
        &self,
        file: usize,
        old_line: Option<usize>,
        new_line: Option<usize>,
        column: usize,
    ) -> Result<Visit, String> {
        self.files.get(file).ok_or("No file here")?.visit(
            &self.comparison.repository,
            old_line,
            new_line,
            column,
            matches!(
                self.comparison.kind,
                DiffKind::Unstaged | DiffKind::Worktree | DiffKind::RevisionToWorktree { .. }
            ),
            self.comparison.environment,
        )
    }
}

impl FileDiff {
    pub(crate) fn visit(
        &self,
        repository: &Repository,
        old_line: Option<usize>,
        new_line: Option<usize>,
        column: usize,
        worktree: bool,
        environment: ReadEnvironment,
    ) -> Result<Visit, String> {
        let file = self;
        if file.summary.is_some() && file.hunks.is_empty() {
            return Err("This change has no supported text location".into());
        }
        let old = new_line.is_none() && old_line.is_some() || file.status == 'D';
        let line = if old { old_line } else { new_line }.unwrap_or(1).max(1);
        let path = if old { &file.old_path } else { &file.new_path };
        if !old && worktree {
            return Ok(Visit::Worktree {
                path: repository.root.join(path),
                line,
                column,
            });
        }
        let oid = if old { &file.old_oid } else { &file.new_oid };
        if oid.bytes().all(|byte| byte == b'0') {
            return Err("No file version exists on this side".into());
        }
        Ok(Visit::Blob {
            root: repository.root.clone(),
            git_dir: repository.git_dir.clone(),
            oid: oid.clone(),
            path: path.clone(),
            environment,
            line,
            column,
        })
    }
}
