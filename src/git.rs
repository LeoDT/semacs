//! Fixed-repository Git access with bounded process ownership.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

mod process;

pub(crate) use process::{Access, execute, write_environment};

#[cfg(test)]
use process::{OUTPUT_LIMIT, collect};
#[cfg(test)]
use std::sync::atomic::Ordering;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Repository {
    pub root: PathBuf,
    pub git_dir: PathBuf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadEnvironment {
    Projection,
    Commit,
}

impl ReadEnvironment {
    pub(crate) fn discover(
        self,
        directory: &Path,
        cancelled: &AtomicBool,
    ) -> Result<Repository, String> {
        discover_environment(self, directory, cancelled)
    }

    pub(crate) fn read(
        self,
        repository: &Repository,
        args: &[&std::ffi::OsStr],
        cancelled: &AtomicBool,
    ) -> Result<Vec<u8>, String> {
        successful(self.execute(repository, args, cancelled)?)
    }

    pub(crate) fn execute(
        self,
        repository: &Repository,
        args: &[&std::ffi::OsStr],
        cancelled: &AtomicBool,
    ) -> Result<process::Output, String> {
        let access = match self {
            Self::Projection => Access::Read(cancelled),
            Self::Commit => Access::CommitRead(cancelled),
        };
        execute(
            &repository.root,
            Some(&repository.git_dir),
            args,
            None,
            access,
        )
        .map_err(|failure| failure.diagnostic)
    }
}

fn successful(output: process::Output) -> Result<Vec<u8>, String> {
    if !output.status.success() {
        return Err(format!(
            "Git failed ({}): {}",
            output.status, output.diagnostic
        ));
    }
    Ok(output.bytes)
}

pub(crate) fn read(
    directory: &Path,
    git_dir: Option<&Path>,
    args: &[&std::ffi::OsStr],
    cancelled: &AtomicBool,
) -> Result<Vec<u8>, String> {
    read_input(directory, git_dir, args, None, cancelled)
}

pub(crate) fn read_input(
    directory: &Path,
    git_dir: Option<&Path>,
    args: &[&std::ffi::OsStr],
    input: Option<&[u8]>,
    cancelled: &AtomicBool,
) -> Result<Vec<u8>, String> {
    let output = execute(directory, git_dir, args, input, Access::Read(cancelled))
        .map_err(|failure| failure.diagnostic)?;
    successful(output)
}

pub(crate) fn discover(directory: &Path, cancelled: &AtomicBool) -> Result<Repository, String> {
    ReadEnvironment::Projection.discover(directory, cancelled)
}

fn discover_environment(
    environment: ReadEnvironment,
    directory: &Path,
    cancelled: &AtomicBool,
) -> Result<Repository, String> {
    let read = |args: &[&std::ffi::OsStr]| {
        let access = match environment {
            ReadEnvironment::Projection => Access::Read(cancelled),
            ReadEnvironment::Commit => Access::CommitRead(cancelled),
        };
        successful(
            execute(directory, None, args, None, access).map_err(|failure| failure.diagnostic)?,
        )
    };
    let root = output_path(read(&["rev-parse".as_ref(), "--show-toplevel".as_ref()])?)?;
    let git_dir = output_path(read(&[
        "rev-parse".as_ref(),
        "--absolute-git-dir".as_ref(),
    ])?)?;
    Ok(Repository {
        root: root
            .canonicalize()
            .map_err(|error| format!("Cannot resolve worktree: {error}"))?,
        git_dir: git_dir
            .canonicalize()
            .map_err(|error| format!("Cannot resolve git-dir: {error}"))?,
    })
}

pub(crate) fn relative_path(bytes: &[u8]) -> Result<PathBuf, String> {
    if bytes.is_empty()
        || bytes.contains(&0)
        || bytes
            .split(|byte| *byte == b'/')
            .any(|part| part.is_empty() || part == b"." || part == b"..")
    {
        return Err("Invalid repository-relative Git path".into());
    }
    let path = path_bytes(bytes.to_vec())?;
    if path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        return Err("Invalid repository-relative Git path".into());
    }
    Ok(path)
}

fn output_path(mut bytes: Vec<u8>) -> Result<PathBuf, String> {
    if bytes.last() == Some(&b'\n') {
        bytes.pop();
    }
    path_bytes(bytes)
}

fn path_bytes(bytes: Vec<u8>) -> Result<PathBuf, String> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        Ok(std::ffi::OsString::from_vec(bytes).into())
    }
    #[cfg(not(unix))]
    {
        Ok(String::from_utf8(bytes)
            .map_err(|_| "Repository path is not UTF-8")?
            .into())
    }
}

pub(crate) fn blob(
    root: &Path,
    git_dir: &Path,
    oid: &str,
    environment: ReadEnvironment,
    cancelled: &AtomicBool,
) -> Result<String, String> {
    if !valid_oid(oid) || oid.bytes().all(|byte| byte == b'0') {
        return Err("No file version exists on this side".into());
    }
    let bytes = environment.read(
        &Repository {
            root: root.to_owned(),
            git_dir: git_dir.to_owned(),
        },
        &["cat-file".as_ref(), "blob".as_ref(), oid.as_ref()],
        cancelled,
    )?;
    if bytes.contains(&0) {
        return Err("Binary file versions are not supported".into());
    }
    String::from_utf8(bytes).map_err(|_| "File version is not UTF-8".into())
}

pub(crate) fn valid_oid(oid: &str) -> bool {
    matches!(oid.len(), 40 | 64) && oid.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(crate) fn current_branch(
    repository: &Repository,
    environment: ReadEnvironment,
    cancelled: &AtomicBool,
) -> Result<Option<String>, String> {
    let branch = environment.execute(
        repository,
        &[
            "symbolic-ref".as_ref(),
            "--quiet".as_ref(),
            "--short".as_ref(),
            "HEAD".as_ref(),
        ],
        cancelled,
    )?;
    match branch.status.code() {
        Some(0) => Ok(String::from_utf8(branch.bytes)
            .ok()
            .map(|name| name.trim_end_matches('\n').to_owned())
            .filter(|name| !name.is_empty())),
        Some(1) => Ok(None),
        _ => Err(format!("Cannot read current branch: {}", branch.diagnostic)),
    }
}

pub(crate) fn resolve_commit(
    repository: &Repository,
    rev: &str,
    environment: ReadEnvironment,
    cancelled: &AtomicBool,
) -> Result<String, String> {
    if rev.is_empty() {
        return Err("Empty revision".into());
    }
    let deref = if rev.starts_with(":/") {
        rev.to_owned()
    } else {
        format!("{rev}^{{commit}}")
    };
    let output = environment.execute(
        repository,
        &[
            "rev-parse".as_ref(),
            "--verify".as_ref(),
            "--quiet".as_ref(),
            "--end-of-options".as_ref(),
            deref.as_ref(),
        ],
        cancelled,
    )?;
    if !output.status.success() {
        return Err(format!("{rev} cannot be dereferenced as a commit"));
    }
    let oid = String::from_utf8(output.bytes)
        .map_err(|_| "Git commit identity is not UTF-8")?
        .trim_end_matches('\n')
        .to_owned();
    if !valid_oid(&oid) {
        return Err(format!("{rev} cannot be dereferenced as a commit"));
    }
    Ok(oid)
}

const SPECIAL_REFNAMES: &[&str] = &[
    "HEAD",
    "ORIG_HEAD",
    "FETCH_HEAD",
    "MERGE_HEAD",
    "CHERRY_PICK_HEAD",
];

pub(crate) fn list_refnames(
    repository: &Repository,
    include_special: bool,
    environment: ReadEnvironment,
    cancelled: &AtomicBool,
) -> Result<Vec<String>, String> {
    let mut refs = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for namespace in ["refs/heads", "refs/remotes", "refs/tags"] {
        let mut args: Vec<&std::ffi::OsStr> = vec![
            "for-each-ref".as_ref(),
            "--format=%(symref)%00%(refname:short)".as_ref(),
        ];
        if namespace == "refs/tags" {
            args.push("--sort=-version:refname".as_ref());
        }
        args.push(namespace.as_ref());
        let bytes = environment.read(repository, &args, cancelled)?;
        for line in bytes.split(|byte| *byte == b'\n') {
            if line.is_empty() {
                continue;
            }
            let Some(sep) = line.iter().position(|byte| *byte == 0) else {
                continue;
            };
            if sep > 0 {
                // Skip symbolic refs (e.g. origin/HEAD), matching upstream `magit-list-refs`.
                continue;
            }
            if let Ok(name) = std::str::from_utf8(&line[sep + 1..])
                && !name.is_empty()
                && seen.insert(name.to_owned())
            {
                refs.push(name.to_owned());
            }
        }
    }
    if include_special {
        for &name in SPECIAL_REFNAMES {
            if repository.git_dir.join(name).exists() && seen.insert(name.to_owned()) {
                refs.push(name.to_owned());
            }
        }
    }
    Ok(refs)
}

#[cfg(all(test, unix))]
#[path = "git_process_tests.rs"]
mod process_tests;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stream_limits_fail_without_retaining_unbounded_output() {
        let exceeded = AtomicBool::new(false);
        assert_eq!(
            collect(std::io::Cursor::new(b"four"), 4, &exceeded).unwrap(),
            b"four"
        );
        assert!(!exceeded.load(Ordering::Relaxed));
        assert!(collect(std::io::Cursor::new(b"too much data"), 4, &exceeded).is_err());
        assert!(exceeded.load(Ordering::Relaxed));
    }
}
