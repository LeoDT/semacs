//! A unified Git comparison, independent of views and application focus.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

mod parse;
mod render;
#[cfg(test)]
pub(crate) mod tests;
pub(crate) use render::{DiffNavigation, RenderedDiff};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffKind {
    Staged,
    Unstaged,
}
impl DiffKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Staged => "staged",
            Self::Unstaged => "unstaged",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Comparison {
    pub root: PathBuf,
    pub git_dir: PathBuf,
    pub kind: DiffKind,
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
        line: usize,
        column: usize,
    },
    Worktree {
        path: PathBuf,
        line: usize,
        column: usize,
    },
}

impl Diff {
    pub(crate) fn load(comparison: Comparison, cancelled: &AtomicBool) -> Result<Self, String> {
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
        if comparison.kind == DiffKind::Staged {
            args.push("--cached");
        }
        args.push("--");
        let args: Vec<_> = args.into_iter().map(std::ffi::OsStr::new).collect();
        let bytes = {
            #[cfg(feature = "native-profiler")]
            let _span = crate::preparation_trace::Span::new("diff-git", 0);
            crate::git::read(
                &comparison.root,
                Some(&comparison.git_dir),
                &args,
                cancelled,
            )?
        };
        #[cfg(feature = "native-profiler")]
        let _span = crate::preparation_trace::Span::new("diff-parse", bytes.len());
        let files = parse::parse(&bytes)?;
        Ok(Self { comparison, files })
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
        let file = self.files.get(file).ok_or("No file here")?;
        if file.summary.is_some() && file.hunks.is_empty() {
            return Err("This change has no supported text location".into());
        }
        let old = new_line.is_none() && old_line.is_some() || file.status == 'D';
        let line = if old { old_line } else { new_line }.unwrap_or(1).max(1);
        let path = if old { &file.old_path } else { &file.new_path };
        if !old && self.comparison.kind == DiffKind::Unstaged {
            return Ok(Visit::Worktree {
                path: self.comparison.root.join(path),
                line,
                column,
            });
        }
        let oid = if old { &file.old_oid } else { &file.new_oid };
        if oid.bytes().all(|byte| byte == b'0') {
            return Err("No file version exists on this side".into());
        }
        Ok(Visit::Blob {
            root: self.comparison.root.clone(),
            git_dir: self.comparison.git_dir.clone(),
            oid: oid.clone(),
            path: path.clone(),
            line,
            column,
        })
    }
}
