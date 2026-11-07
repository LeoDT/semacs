use std::sync::atomic::AtomicBool;

use super::Repository;
use crate::git;

pub(crate) enum Preparation {
    Staged,
    All,
    Merge,
}

pub(crate) fn prepare(
    repository: &Repository,
    cancelled: &AtomicBool,
) -> Result<Preparation, String> {
    if changed(repository, true, cancelled)? {
        return Ok(Preparation::Staged);
    }
    let unstaged = changed(repository, false, cancelled)?;
    if repository.git_dir.join("MERGE_MSG").is_file() {
        let unmerged = git::ReadEnvironment::Commit.read(
            repository,
            &["ls-files".as_ref(), "--unmerged".as_ref()],
            cancelled,
        )?;
        if !unmerged.is_empty() {
            return Err("Unresolved conflicts".into());
        }
        return Ok(if unstaged {
            Preparation::Merge
        } else {
            Preparation::Staged
        });
    }
    if unstaged {
        Ok(Preparation::All)
    } else {
        Err("Nothing staged (or unstaged)".into())
    }
}

fn changed(repository: &Repository, cached: bool, cancelled: &AtomicBool) -> Result<bool, String> {
    let mut args = vec![
        "diff".as_ref(),
        "--quiet".as_ref(),
        "--submodule=short".as_ref(),
    ];
    if cached {
        args.push("--cached".as_ref());
    }
    args.push("--".as_ref());
    let output = git::ReadEnvironment::Commit.execute(repository, &args, cancelled)?;
    match output.status.code() {
        Some(0) => Ok(false),
        Some(1) => Ok(true),
        _ => Err(format!(
            "Git diff failed ({}): {}",
            output.status, output.diagnostic
        )),
    }
}
