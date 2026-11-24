//! Magit-style push target resolution and remote config helpers.

use std::collections::HashSet;
use std::sync::atomic::AtomicBool;

use crate::git::{ReadEnvironment, Repository};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Upstream {
    pub remote: String,
    pub merge: String,
    pub short: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PushContext {
    pub repository: Repository,
    pub branch: Option<String>,
    pub unborn: bool,
    pub push_remote: Option<String>,
    pub upstream: Option<Upstream>,
    pub remotes: Vec<String>,
    pub targets: Vec<String>,
}

impl PushContext {
    pub fn load(repository: Repository, cancelled: &AtomicBool) -> Result<Self, String> {
        let branch = crate::git::current_branch(&repository, ReadEnvironment::Commit, cancelled)?;
        let head = ReadEnvironment::Commit.execute(
            &repository,
            &[
                "rev-parse".as_ref(),
                "--verify".as_ref(),
                "--quiet".as_ref(),
                "HEAD".as_ref(),
            ],
            cancelled,
        )?;
        let unborn = !head.status.success();
        let remotes_raw =
            ReadEnvironment::Commit.read(&repository, &["remote".as_ref()], cancelled)?;
        let remotes: Vec<String> = String::from_utf8_lossy(&remotes_raw)
            .lines()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(ToOwned::to_owned)
            .collect();

        let (push_remote, upstream) = if let Some(branch) = branch.as_deref().filter(|_| !unborn) {
            let push_remote = read_config(
                &repository,
                &format!("branch.{branch}.pushRemote"),
                cancelled,
            )?
            .or(read_config(&repository, "remote.pushDefault", cancelled)?);
            let remote = read_config(&repository, &format!("branch.{branch}.remote"), cancelled)?;
            let merge = read_config(&repository, &format!("branch.{branch}.merge"), cancelled)?;
            let upstream = match (remote, merge) {
                (Some(remote), Some(merge)) => {
                    let short =
                        read_upstream_short(&repository, branch, cancelled).unwrap_or_else(|| {
                            let short_branch = merge.strip_prefix("refs/heads/").unwrap_or(&merge);
                            if remote == "." {
                                short_branch.to_owned()
                            } else {
                                format!("{remote}/{short_branch}")
                            }
                        });
                    Some(Upstream {
                        remote,
                        merge,
                        short,
                    })
                }
                _ => None,
            };
            (push_remote, upstream)
        } else {
            (None, None)
        };

        let mut targets = Vec::new();
        let mut seen = HashSet::new();
        if let Some(branch) = &branch {
            for remote in &remotes {
                let candidate = format!("{remote}/{branch}");
                if seen.insert(candidate.clone()) {
                    targets.push(candidate);
                }
            }
        }
        let refs_raw = ReadEnvironment::Commit.read(
            &repository,
            &[
                "for-each-ref".as_ref(),
                "--format=%(symref)%00%(refname:short)".as_ref(),
                "refs/remotes".as_ref(),
            ],
            cancelled,
        )?;
        for line in refs_raw.split(|byte| *byte == b'\n') {
            if line.is_empty() {
                continue;
            }
            let Some(sep) = line.iter().position(|byte| *byte == 0) else {
                continue;
            };
            if sep > 0 {
                continue;
            }
            if let Ok(name) = std::str::from_utf8(&line[sep + 1..])
                && !name.is_empty()
                && seen.insert(name.to_owned())
            {
                targets.push(name.to_owned());
            }
        }

        Ok(Self {
            repository,
            branch,
            unborn,
            push_remote,
            upstream,
            remotes,
            targets,
        })
    }

    pub fn validate_current_branch(&self) -> Result<&str, String> {
        let Some(branch) = self.branch.as_deref() else {
            return Err("Cannot push: HEAD is detached".into());
        };
        if self.unborn {
            return Err("Cannot push: current branch has no commits yet".into());
        }
        Ok(branch)
    }

    pub fn pushremote_args(&self, remote: &str) -> Result<(Vec<String>, String), String> {
        let branch = self.validate_current_branch()?;
        let args = vec![
            remote.to_owned(),
            format!("refs/heads/{branch}:refs/heads/{branch}"),
        ];
        let label = format!("git push -v {}", args.join(" "));
        Ok((args, label))
    }

    pub fn upstream_args(
        &self,
        remote: &str,
        merge: &str,
    ) -> Result<(Vec<String>, String), String> {
        let branch = self.validate_current_branch()?;
        let args = vec![remote.to_owned(), format!("refs/heads/{branch}:{merge}")];
        let label = format!("git push -v {}", args.join(" "));
        Ok((args, label))
    }

    pub fn set_upstream_args(
        &self,
        remote: &str,
        target_branch: &str,
    ) -> Result<(Vec<String>, String), String> {
        let branch = self.validate_current_branch()?;
        let args = vec![
            "--set-upstream".to_owned(),
            remote.to_owned(),
            format!("refs/heads/{branch}:refs/heads/{target_branch}"),
        ];
        let label = format!("git push -v {}", args.join(" "));
        Ok((args, label))
    }

    pub fn elsewhere_args(
        &self,
        remote: &str,
        target_branch: &str,
    ) -> Result<(Vec<String>, String), String> {
        let branch = self.validate_current_branch()?;
        let args = vec![
            remote.to_owned(),
            format!("refs/heads/{branch}:refs/heads/{target_branch}"),
        ];
        let label = format!("git push -v {}", args.join(" "));
        Ok((args, label))
    }
}

pub fn parse_remote_target(input: &str, remotes: &[String]) -> Result<(String, String), String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("Target cannot be empty".into());
    }
    if remotes.is_empty() {
        return Err("No remotes configured".into());
    }
    let mut matched: Option<(&str, &str)> = None;
    for remote in remotes {
        if let Some(rest) = trimmed.strip_prefix(remote.as_str())
            && let Some(branch) = rest.strip_prefix('/')
            && !branch.is_empty()
            && matched.is_none_or(|(best, _)| remote.len() > best.len())
        {
            matched = Some((remote.as_str(), branch));
        }
    }
    matched
        .map(|(remote, branch)| (remote.to_owned(), branch.to_owned()))
        .ok_or_else(|| {
            format!(
                "Specify <remote>/<branch> (known remotes: {})",
                remotes.join(", ")
            )
        })
}

pub fn set_push_remote(
    repository: &Repository,
    branch: &str,
    remote: &str,
    cancelled: &AtomicBool,
) -> Result<(), String> {
    ReadEnvironment::Commit.read(
        repository,
        &[
            "config".as_ref(),
            format!("branch.{branch}.pushRemote").as_ref(),
            remote.as_ref(),
        ],
        cancelled,
    )?;
    Ok(())
}

fn read_config(
    repository: &Repository,
    key: &str,
    cancelled: &AtomicBool,
) -> Result<Option<String>, String> {
    let output = ReadEnvironment::Commit.execute(
        repository,
        &["config".as_ref(), "--get".as_ref(), key.as_ref()],
        cancelled,
    )?;
    match output.status.code() {
        Some(0) => Ok(String::from_utf8(output.bytes)
            .ok()
            .map(|value| value.trim_end_matches(&['\r', '\n'][..]).to_owned())
            .filter(|value| !value.is_empty())),
        Some(1) => Ok(None),
        _ => Err(format!(
            "Cannot read Git config {key}: {}",
            output.diagnostic
        )),
    }
}

fn read_upstream_short(
    repository: &Repository,
    branch: &str,
    cancelled: &AtomicBool,
) -> Option<String> {
    let spec = format!("{branch}@{{upstream}}");
    let output = ReadEnvironment::Commit
        .execute(
            repository,
            &[
                "rev-parse".as_ref(),
                "--abbrev-ref".as_ref(),
                "--symbolic-full-name".as_ref(),
                spec.as_ref(),
            ],
            cancelled,
        )
        .ok()?;
    output.status.success().then(|| {
        String::from_utf8_lossy(&output.bytes)
            .trim_end_matches(&['\r', '\n'][..])
            .to_owned()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct TempRepo(PathBuf);

    impl TempRepo {
        fn new(suffix: &str) -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let dir = std::env::temp_dir().join(format!(
                "semacs-push-unit-{}-{}-{suffix}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for TempRepo {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn git(dir: &std::path::Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(dir)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    }

    #[test]
    fn parse_remote_target_prefers_longest_matching_remote_prefix() {
        let remotes = vec!["origin".into(), "team/origin".into()];
        assert_eq!(
            parse_remote_target("origin/main", &remotes).unwrap(),
            ("origin".into(), "main".into())
        );
        assert_eq!(
            parse_remote_target("team/origin/feat/x", &remotes).unwrap(),
            ("team/origin".into(), "feat/x".into())
        );
        assert!(parse_remote_target("unknown/main", &remotes).is_err());
        assert!(parse_remote_target("origin/", &remotes).is_err());
        assert!(parse_remote_target("", &remotes).is_err());
    }

    #[test]
    fn context_detects_unborn_detached_pushremote_and_upstream() {
        let work = TempRepo::new("work");
        let bare = TempRepo::new("bare");
        git(&bare.0, &["init", "--bare", "-q"]);
        git(&work.0, &["init", "-q", "-b", "main"]);
        git(&work.0, &["config", "user.email", "a@b.c"]);
        git(&work.0, &["config", "user.name", "Tester"]);
        git(
            &work.0,
            &["remote", "add", "origin", bare.0.to_str().unwrap()],
        );

        let repo = crate::git::discover(&work.0, &AtomicBool::new(false)).unwrap();
        let unborn = PushContext::load(repo.clone(), &AtomicBool::new(false)).unwrap();
        assert!(unborn.unborn);
        assert!(unborn.validate_current_branch().is_err());

        std::fs::write(work.0.join("a.txt"), "hello\n").unwrap();
        git(&work.0, &["add", "a.txt"]);
        git(&work.0, &["commit", "-qm", "init"]);

        let ctx = PushContext::load(repo.clone(), &AtomicBool::new(false)).unwrap();
        assert_eq!(ctx.branch.as_deref(), Some("main"));
        assert!(!ctx.unborn);
        assert_eq!(ctx.push_remote, None);
        assert_eq!(ctx.upstream, None);
        assert_eq!(ctx.remotes, vec!["origin"]);
        assert!(ctx.targets.contains(&"origin/main".to_owned()));

        set_push_remote(&repo, "main", "origin", &AtomicBool::new(false)).unwrap();
        let ctx = PushContext::load(repo.clone(), &AtomicBool::new(false)).unwrap();
        assert_eq!(ctx.push_remote.as_deref(), Some("origin"));

        git(&work.0, &["checkout", "--detach", "-q"]);
        let detached = PushContext::load(repo, &AtomicBool::new(false)).unwrap();
        assert_eq!(detached.branch, None);
        assert!(detached.validate_current_branch().is_err());
    }
}
