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
    let mut root_config: Option<Vec<u8>> = None;
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
        if root_config.is_none() && repo == repository {
            root_config = Some(config);
        }
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
    let mut status = Status::parse(repository, &bytes)?;
    load_meta(&mut status, root_config.as_deref().unwrap_or(&[]), cancelled)?;
    Ok(status)
}

#[derive(Default)]
struct BranchConfig {
    remote: Option<String>,
    merge: Option<String>,
    rebase: Option<bool>,
    pull_rebase: Option<bool>,
    push_remote: Option<String>,
    push_default: Option<String>,
}

fn parse_bool(value: &[u8]) -> Option<bool> {
    if value.is_empty() {
        return Some(true);
    }
    let s = std::str::from_utf8(value).ok()?.trim();
    if s.eq_ignore_ascii_case("true") || s.eq_ignore_ascii_case("yes") || s.eq_ignore_ascii_case("on") || s == "1" {
        Some(true)
    } else if s.eq_ignore_ascii_case("false") || s.eq_ignore_ascii_case("no") || s.eq_ignore_ascii_case("off") || s == "0" {
        Some(false)
    } else {
        None
    }
}

fn parse_branch_config(bytes: &[u8], branch: Option<&[u8]>) -> BranchConfig {
    let mut cfg = BranchConfig::default();
    let Ok(iter) = records(bytes) else {
        return cfg;
    };
    for record in iter {
        let (key, value) = match record.iter().position(|b| *b == b'\n') {
            Some(end) => (&record[..end], &record[end + 1..]),
            None => (record, b"".as_slice()),
        };
        if key == b"pull.rebase" {
            cfg.pull_rebase = parse_bool(value);
        } else if key == b"remote.pushdefault" {
            if let Ok(s) = std::str::from_utf8(value)
                && !s.is_empty()
            {
                cfg.push_default = Some(s.to_owned());
            }
        } else if let Some(branch) = branch
            && let Some(after_branch_prefix) = key.strip_prefix(b"branch.")
            && let Some(suffix) = after_branch_prefix.strip_prefix(branch)
        {
            match suffix {
                b".remote" => {
                    if let Ok(s) = std::str::from_utf8(value)
                        && !s.is_empty()
                    {
                        cfg.remote = Some(s.to_owned());
                    }
                }
                b".merge" => {
                    if let Ok(s) = std::str::from_utf8(value)
                        && !s.is_empty()
                    {
                        cfg.merge = Some(s.to_owned());
                    }
                }
                b".rebase" => {
                    cfg.rebase = parse_bool(value);
                }
                b".pushremote" => {
                    if let Ok(s) = std::str::from_utf8(value)
                        && !s.is_empty()
                    {
                        cfg.push_remote = Some(s.to_owned());
                    }
                }
                _ => {}
            }
        }
    }
    cfg
}

fn run_optional(
    repo: &Repository,
    args: &[&str],
    cancelled: &AtomicBool,
) -> Result<Option<Vec<u8>>, String> {
    if cancelled.load(Ordering::Relaxed) {
        return Err("Git request cancelled".into());
    }
    let os_args: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
    let out = crate::git::ReadEnvironment::Projection.execute(repo, &os_args, cancelled)?;
    if cancelled.load(Ordering::Relaxed) {
        return Err("Git request cancelled".into());
    }
    Ok(out.status.success().then_some(out.bytes))
}

fn load_single_commit(
    repo: &Repository,
    spec: &str,
    cancelled: &AtomicBool,
) -> Result<Option<(String, String, String)>, String> {
    let Some(bytes) = run_optional(
        repo,
        &[
            "log",
            "-n1",
            "--no-show-signature",
            "--encoding=UTF-8",
            "--format=%H%x00%h%x00%s",
            "--end-of-options",
            spec,
            "--",
        ],
        cancelled,
    )?
    else {
        return Ok(None);
    };
    let text = String::from_utf8_lossy(&bytes);
    let trimmed = text.trim_end_matches(&['\r', '\n'][..]);
    let mut parts = trimmed.splitn(3, '\0');
    let (Some(oid), Some(short), Some(summary)) = (parts.next(), parts.next(), parts.next()) else {
        return Ok(None);
    };
    let summary = if summary.trim().is_empty() {
        "(no commit message)".to_owned()
    } else {
        summary.to_owned()
    };
    Ok(Some((oid.to_owned(), short.to_owned(), summary)))
}

fn load_commits(
    repo: &Repository,
    spec: &str,
    limit: usize,
    cancelled: &AtomicBool,
) -> Result<Vec<super::CommitSummary>, String> {
    let limit_arg = format!("-n{limit}");
    let Some(bytes) = run_optional(
        repo,
        &[
            "log",
            "--no-show-signature",
            "--decorate=short",
            "--encoding=UTF-8",
            &limit_arg,
            "--format=%H%x00%h%x00%s%x00%D%x00",
            "--end-of-options",
            spec,
            "--",
        ],
        cancelled,
    )?
    else {
        return Ok(Vec::new());
    };
    let mut commits = Vec::new();
    let mut slice = bytes.as_slice();
    while !slice.is_empty() {
        while slice.first() == Some(&b'\n') || slice.first() == Some(&b'\r') {
            slice = &slice[1..];
        }
        if slice.is_empty() {
            break;
        }
        let mut fields = [None, None, None, None];
        let mut valid = true;
        for slot in &mut fields {
            if let Some(pos) = slice.iter().position(|b| *b == 0) {
                *slot = Some(String::from_utf8_lossy(&slice[..pos]).into_owned());
                slice = &slice[pos + 1..];
            } else {
                valid = false;
                break;
            }
        }
        if !valid {
            break;
        }
        let oid = fields[0].take().unwrap();
        let short = fields[1].take().unwrap();
        let raw_summary = fields[2].take().unwrap();
        let summary = if raw_summary.trim().is_empty() {
            "(no commit message)".to_owned()
        } else {
            raw_summary
        };
        let decorations = fields[3].take().unwrap().trim().to_owned();
        commits.push(super::CommitSummary {
            oid,
            short,
            summary,
            decorations,
        });
    }
    Ok(commits)
}

fn load_tags(
    repo: &Repository,
    cancelled: &AtomicBool,
) -> Result<(Option<super::TagInfo>, Option<super::TagInfo>), String> {
    let current = if let Some(bytes) =
        run_optional(repo, &["describe", "--tags", "--long", "HEAD"], cancelled)?
    {
        let text = String::from_utf8_lossy(&bytes);
        let trimmed = text.trim_end_matches(&['\r', '\n'][..]);
        trimmed
            .rsplit_once("-g")
            .and_then(|(prefix, _)| prefix.rsplit_once('-'))
            .and_then(|(name, count)| {
                let count = count.parse::<usize>().ok()?;
                Some(super::TagInfo {
                    name: name.to_owned(),
                    count,
                })
            })
    } else {
        None
    };
    let mut next = if let Some(bytes) =
        run_optional(repo, &["describe", "--tags", "--contains", "HEAD"], cancelled)?
    {
        let text = String::from_utf8_lossy(&bytes);
        let trimmed = text.trim_end_matches(&['\r', '\n'][..]);
        if let Some((name, rest)) = trimmed.split_once('~') {
            let count = rest
                .split('^')
                .next()
                .and_then(|digits| digits.parse::<usize>().ok())
                .unwrap_or(0);
            Some(super::TagInfo {
                name: name.to_owned(),
                count,
            })
        } else if let Some((name, _)) = trimmed.split_once('^') {
            Some(super::TagInfo {
                name: name.to_owned(),
                count: 0,
            })
        } else if !trimmed.is_empty() {
            Some(super::TagInfo {
                name: trimmed.to_owned(),
                count: 0,
            })
        } else {
            None
        }
    } else {
        None
    };
    if let (Some(curr), Some(nxt)) = (&current, &next)
        && curr.count == 0
        && curr.name == nxt.name
    {
        next = None;
    }
    Ok((current, next))
}

fn load_meta(
    status: &mut Status,
    root_config: &[u8],
    cancelled: &AtomicBool,
) -> Result<(), String> {
    let branch_bytes = match &status.head {
        super::Head::Unborn { .. } => return Ok(()),
        super::Head::Branch { name, .. } => Some(name.clone()),
        super::Head::Detached { .. } => None,
    };
    let repo = status.repository.clone();
    let cfg = parse_branch_config(root_config, branch_bytes.as_deref());

    if let Some((_, short, summary)) = load_single_commit(&repo, "HEAD", cancelled)? {
        status.meta.head_short = Some(short);
        status.meta.head_summary = Some(summary);
    }

    if let Some(branch) = branch_bytes.as_deref().and_then(|b| std::str::from_utf8(b).ok()) {
        if cfg.remote.is_some() || cfg.merge.is_some() {
            let rebase = cfg.rebase.or(cfg.pull_rebase).unwrap_or(false);
            let label = if rebase { "Rebase: " } else { "Merge: " }.to_owned();
            let upstream_short = run_optional(
                &repo,
                &[
                    "rev-parse",
                    "--abbrev-ref",
                    "--symbolic-full-name",
                    "@{upstream}",
                ],
                cancelled,
            )?
            .and_then(|bytes| {
                let s = String::from_utf8_lossy(&bytes)
                    .trim_end_matches(&['\r', '\n'][..])
                    .to_owned();
                (!s.is_empty()).then_some(s)
            });
            let is_remote = cfg.remote.as_deref() != Some(".");
            if let Some(target) = upstream_short {
                let info = load_single_commit(&repo, "@{upstream}", cancelled)?;
                let (oid, summary, warning) = match info {
                    Some((oid, _, summary)) => (Some(oid), Some(summary), None),
                    None => (None, None, Some("does not exist".to_owned())),
                };
                status.meta.upstream = Some(super::RefHeader {
                    label,
                    target,
                    oid,
                    summary,
                    warning,
                    is_remote,
                });
            } else if let (Some(remote), Some(merge)) = (&cfg.remote, &cfg.merge) {
                let short_branch = merge.strip_prefix("refs/heads/").unwrap_or(merge);
                let target = if remote == "." {
                    short_branch.to_owned()
                } else {
                    format!("{remote}/{short_branch}")
                };
                let warning = if remote == "." {
                    "does not exist".to_owned()
                } else {
                    format!("does not exist on {remote}")
                };
                status.meta.upstream = Some(super::RefHeader {
                    label,
                    target,
                    oid: None,
                    summary: None,
                    warning: Some(warning),
                    is_remote,
                });
            } else {
                status.meta.upstream = Some(super::RefHeader {
                    label,
                    target: String::new(),
                    oid: None,
                    summary: None,
                    warning: Some("invalid upstream configuration".to_owned()),
                    is_remote: false,
                });
            }
        }

        let push_remote = cfg.push_remote.or(cfg.push_default);
        if let Some(remote) = push_remote {
            let target = format!("{remote}/{branch}");
            let commit_ref = format!("{target}^{{commit}}");
            let valid = run_optional(
                &repo,
                &[
                    "rev-parse",
                    "--verify",
                    "--quiet",
                    "--end-of-options",
                    &commit_ref,
                ],
                cancelled,
            )?
            .is_some();
            if valid {
                let info = load_single_commit(&repo, &target, cancelled)?;
                let (oid, summary) = match info {
                    Some((oid, _, summary)) => (Some(oid), Some(summary)),
                    None => (None, None),
                };
                status.meta.push = Some(super::RefHeader {
                    label: "Push: ".to_owned(),
                    target,
                    oid,
                    summary,
                    warning: None,
                    is_remote: true,
                });
            } else {
                status.meta.push = Some(super::RefHeader {
                    label: "Push: ".to_owned(),
                    target,
                    oid: None,
                    summary: None,
                    warning: Some("does not exist".to_owned()),
                    is_remote: true,
                });
            }
        }
    }

    let (current_tag, next_tag) = load_tags(&repo, cancelled)?;
    status.meta.current_tag = current_tag;
    status.meta.next_tag = next_tag;

    let merge_head_path = repo.git_dir.join("MERGE_HEAD");
    if let Ok(content) = std::fs::read_to_string(&merge_head_path) {
        let heads: Vec<String> = content
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(ToOwned::to_owned)
            .collect();
        if let Some(first) = heads.first() {
            let mut names = Vec::with_capacity(heads.len());
            for head in &heads {
                let short = if let Some(bytes) = run_optional(
                    &repo,
                    &["name-rev", "--name-only", "--no-undefined", head],
                    cancelled,
                )? {
                    let s = String::from_utf8_lossy(&bytes).trim().to_owned();
                    if s.is_empty() {
                        head[..7.min(head.len())].to_owned()
                    } else {
                        s
                    }
                } else {
                    head[..7.min(head.len())].to_owned()
                };
                names.push(short);
            }
            let range = format!("HEAD..{first}");
            let commits = load_commits(&repo, &range, 256, cancelled)?;
            status.meta.merge_section = Some(super::CommitSection {
                key: "merging".to_owned(),
                title: format!("Merging {}", names.join(", ")),
                range: Some(range),
                commits,
                show_count: true,
                collapsed_by_default: false,
            });
        }
    }

    if run_optional(&repo, &["rev-parse", "--verify", "--quiet", "refs/stash"], cancelled)?
        .is_some()
        && let Some(bytes) = run_optional(
            &repo,
            &[
                "reflog",
                "--no-show-signature",
                "--encoding=UTF-8",
                "-n256",
                "--format=%gd%x00%H%x00%gs%x00",
                "refs/stash",
            ],
            cancelled,
        )?
    {
        let mut slice = bytes.as_slice();
        while !slice.is_empty() {
            while slice.first() == Some(&b'\n') || slice.first() == Some(&b'\r') {
                slice = &slice[1..];
            }
            if slice.is_empty() {
                break;
            }
            let mut fields = [None, None, None];
            let mut valid = true;
            for slot in &mut fields {
                if let Some(pos) = slice.iter().position(|b| *b == 0) {
                    *slot = Some(String::from_utf8_lossy(&slice[..pos]).into_owned());
                    slice = &slice[pos + 1..];
                } else {
                    valid = false;
                    break;
                }
            }
            if !valid {
                break;
            }
            status.meta.stashes.push(super::StashSummary {
                ref_name: fields[0].take().unwrap(),
                oid: fields[1].take().unwrap(),
                summary: fields[2].take().unwrap(),
            });
        }
    }

    let upstream_target = status
        .meta
        .upstream
        .as_ref()
        .filter(|h| h.oid.is_some())
        .map(|h| h.target.clone());
    let push_target = status
        .meta
        .push
        .as_ref()
        .filter(|h| h.oid.is_some() && Some(&h.target) != upstream_target.as_ref())
        .map(|h| h.target.clone());

    if let Some(push) = &push_target {
        let range = format!("{push}..HEAD");
        let commits = load_commits(&repo, &range, 256, cancelled)?;
        if !commits.is_empty() {
            status.meta.log_sections.push(super::CommitSection {
                key: "unpushed-pushremote".to_owned(),
                title: format!("Unpushed to {push}"),
                range: Some(range),
                commits,
                show_count: true,
                collapsed_by_default: true,
            });
        }
    }
    let mut inserted_unmerged = false;
    if let Some(upstream) = &upstream_target {
        let range = format!("{upstream}..HEAD");
        let commits = load_commits(&repo, &range, 256, cancelled)?;
        if !commits.is_empty() {
            status.meta.log_sections.push(super::CommitSection {
                key: "unpushed-upstream".to_owned(),
                title: format!("Unmerged into {upstream}"),
                range: Some(range),
                commits,
                show_count: true,
                collapsed_by_default: false,
            });
            inserted_unmerged = true;
        }
    }
    if !inserted_unmerged {
        let commits = load_commits(&repo, "HEAD", 10, cancelled)?;
        if !commits.is_empty() {
            status.meta.log_sections.push(super::CommitSection {
                key: "recent".to_owned(),
                title: "Recent commits".to_owned(),
                range: None,
                commits,
                show_count: false,
                collapsed_by_default: true,
            });
        }
    }
    if let Some(push) = &push_target {
        let range = format!("HEAD..{push}");
        let commits = load_commits(&repo, &range, 256, cancelled)?;
        if !commits.is_empty() {
            status.meta.log_sections.push(super::CommitSection {
                key: "unpulled-pushremote".to_owned(),
                title: format!("Unpulled from {push}"),
                range: Some(range),
                commits,
                show_count: true,
                collapsed_by_default: true,
            });
        }
    }
    if let Some(upstream) = &upstream_target {
        let range = format!("HEAD..{upstream}");
        let commits = load_commits(&repo, &range, 256, cancelled)?;
        if !commits.is_empty() {
            status.meta.log_sections.push(super::CommitSection {
                key: "unpulled-upstream".to_owned(),
                title: format!("Unpulled from {upstream}"),
                range: Some(range),
                commits,
                show_count: true,
                collapsed_by_default: true,
            });
        }
    }
    Ok(())
}
