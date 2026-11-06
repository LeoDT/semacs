use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use super::{MAX_BYTES, Repository, Status};

const MAX_REPOSITORIES: usize = 4096;

fn read(repo: &Repository, args: &[&OsStr], cancelled: &AtomicBool) -> Result<Vec<u8>, String> {
    crate::git::read(&repo.root, Some(&repo.git_dir), args, cancelled)
}

fn records(bytes: &[u8]) -> Result<impl Iterator<Item = &[u8]>, String> {
    if !bytes.is_empty() && !bytes.ends_with(&[0]) {
        return Err("Truncated Git preflight output".into());
    }
    Ok(bytes
        .strip_suffix(&[0])
        .unwrap_or(bytes)
        .split(|byte| *byte == 0)
        .filter(move |_| !bytes.is_empty()))
}

pub(super) fn external_filters(bytes: &[u8]) -> Result<HashSet<Vec<u8>>, String> {
    let mut settings = HashMap::new();
    for record in records(bytes)? {
        let Some(end) = record.iter().position(|byte| *byte == b'\n') else {
            continue;
        };
        let (key, value) = (&record[..end], &record[end + 1..]);
        if let Some(driver) = key.strip_prefix(b"filter.") {
            for suffix in [b".clean".as_slice(), b".process"] {
                if let Some(driver) = driver.strip_suffix(suffix) {
                    settings.insert((driver.to_vec(), suffix), !value.is_empty());
                }
            }
        }
    }
    Ok(settings
        .into_iter()
        .filter_map(|((driver, _), enabled)| enabled.then_some(driver))
        .collect())
}

fn index_paths(bytes: &[u8]) -> Result<(Vec<u8>, Vec<PathBuf>), String> {
    let mut paths = Vec::new();
    let mut modules = Vec::new();
    let mut seen = HashSet::new();
    let mut module_paths = HashSet::new();
    for record in records(bytes)? {
        let tab = record
            .iter()
            .position(|byte| *byte == b'\t')
            .ok_or("Malformed Git index record")?;
        let metadata: Vec<_> = record[..tab].split(|byte| *byte == b' ').collect();
        if metadata.len() != 3
            || metadata[0].len() != 6
            || !metadata[0].iter().all(|byte| (b'0'..=b'7').contains(byte))
            || !std::str::from_utf8(metadata[1]).is_ok_and(crate::git::valid_oid)
            || !matches!(metadata[2], b"0" | b"1" | b"2" | b"3")
        {
            return Err("Malformed Git index fields".into());
        }
        let raw_path = &record[tab + 1..];
        let path = crate::git::relative_path(raw_path)?;
        if seen.insert(path.clone()) {
            paths.extend_from_slice(raw_path);
            paths.push(0);
        }
        if metadata[0] == b"160000" && module_paths.insert(path.clone()) {
            modules.push(path);
        }
    }
    Ok((paths, modules))
}

pub(super) fn check_filters(
    repo: &Repository,
    paths: &[u8],
    filters: &HashSet<Vec<u8>>,
    cancelled: &AtomicBool,
) -> Result<(), String> {
    if filters.is_empty() || paths.is_empty() {
        return Ok(());
    }
    let bytes = crate::git::read_input(
        &repo.root,
        Some(&repo.git_dir),
        &[
            "check-attr".as_ref(),
            "-z".as_ref(),
            "--stdin".as_ref(),
            "filter".as_ref(),
        ],
        Some(paths),
        cancelled,
    )?;
    let values: Vec<_> = records(&bytes)?.collect();
    let (triplets, remainder) = values.as_chunks::<3>();
    if !remainder.is_empty() {
        return Err("Malformed Git attribute output".into());
    }
    let mut expected = records(paths)?;
    for triplet in triplets {
        if expected.next() != Some(triplet[0]) || triplet[1] != b"filter" {
            return Err("Git attribute path mismatch".into());
        }
        if filters.contains(triplet[2]) {
            return Err(format!(
                "Status refused: external clean/process filter for {:?} in {:?}",
                crate::git::relative_path(triplet[0])?,
                repo.root
            ));
        }
    }
    if expected.next().is_some() {
        return Err("Missing Git attributes".into());
    }
    Ok(())
}

fn initialized_module(
    repo: &Repository,
    path: &Path,
    cancelled: &AtomicBool,
) -> Result<Option<Repository>, String> {
    let mut root = repo.root.clone();
    for (index, part) in path.components().enumerate() {
        root.push(part);
        match std::fs::symlink_metadata(&root) {
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) if index + 1 == path.components().count() => return Ok(None),
            Ok(_) => {
                return Err(
                    "Submodule parent is not a directory or contains a symbolic link".into(),
                );
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("Cannot inspect submodule: {error}")),
        }
    }
    match std::fs::symlink_metadata(root.join(".git")) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("Cannot inspect submodule git-dir: {error}")),
    }
    let module = crate::git::discover(&root, cancelled)?;
    if module.root != root.canonicalize().map_err(|error| error.to_string())? {
        return Err("Submodule repository identity mismatch".into());
    }
    Ok(Some(module))
}

pub(super) fn load(repository: Repository, cancelled: &AtomicBool) -> Result<Status, String> {
    let mut pending = vec![repository.clone()];
    let mut visited = HashSet::new();
    let mut budget = MAX_BYTES;
    while let Some(repo) = pending.pop() {
        if cancelled.load(Ordering::Relaxed) {
            return Err("Git request cancelled".into());
        }
        if !visited.insert(repo.clone()) {
            return Err("Cyclic submodule repository identity".into());
        }
        if visited.len() > MAX_REPOSITORIES {
            return Err("Status preflight exceeds its repository limit".into());
        }
        let index = read(
            &repo,
            &["ls-files".as_ref(), "--stage".as_ref(), "-z".as_ref()],
            cancelled,
        )?;
        budget = budget
            .checked_sub(index.len())
            .ok_or("Status preflight exceeds its 16 MiB index limit")?;
        let (paths, modules) = index_paths(&index)?;
        let config = read(
            &repo,
            &["config".as_ref(), "--null".as_ref(), "--list".as_ref()],
            cancelled,
        )?;
        check_filters(&repo, &paths, &external_filters(&config)?, cancelled)?;
        for path in modules {
            if let Some(module) = initialized_module(&repo, &path, cancelled)? {
                pending.push(module);
                if pending.len() + visited.len() > MAX_REPOSITORIES {
                    return Err("Status preflight exceeds its repository limit".into());
                }
            }
        }
    }
    let bytes = read(
        &repository,
        &[
            "status".as_ref(),
            "--porcelain=v2".as_ref(),
            "--branch".as_ref(),
            "--no-ahead-behind".as_ref(),
            "-z".as_ref(),
            "--untracked-files=normal".as_ref(),
            "--ignore-submodules=none".as_ref(),
            "--find-renames=50%".as_ref(),
        ],
        cancelled,
    )?;
    Status::parse(repository, &bytes)
}
