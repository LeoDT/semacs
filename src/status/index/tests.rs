use super::*;
use crate::diff::tests::Repository as Fixture;

fn repository(repo: &Fixture) -> Repository {
    Repository {
        root: repo.0.clone(),
        git_dir: repo.0.join(".git"),
    }
}

fn load(repo: &Fixture) -> Status {
    Status::load(repository(repo), &AtomicBool::new(false)).unwrap()
}

pub(super) fn request(
    repo: &Fixture,
    path: impl AsRef<Path>,
    group: Group,
    operation: Operation,
) -> Request {
    let status = load(repo);
    let index = status
        .entries()
        .iter()
        .position(|entry| entry.path == path.as_ref() && entry.in_group(group))
        .unwrap();
    status.index_request(group, index, operation).unwrap()
}

pub(super) fn fixture() -> Fixture {
    let repo = Fixture::new();
    repo.write("a", b"head\n");
    repo.write("unrelated", b"unrelated\n");
    repo.commit();
    repo
}

fn success(report: Report) -> Status {
    assert_eq!(report.outcome, Outcome::Succeeded, "{}", report.diagnostic);
    assert!(report.stale);
    report.verification.unwrap().unwrap()
}

pub(super) fn staged(repo: &Fixture, path: &str) -> Vec<u8> {
    repo.git(&["show", &format!(":{path}")])
}

struct Invariants {
    head: Vec<u8>,
    refs: Vec<u8>,
    config: Vec<u8>,
    unrelated: Vec<u8>,
}

impl Invariants {
    fn capture(repo: &Fixture) -> Self {
        Self {
            head: repo.git(&["rev-parse", "HEAD"]),
            refs: repo.git(&["show-ref"]),
            config: std::fs::read(repo.0.join(".git/config")).unwrap(),
            unrelated: repo.git(&["ls-files", "--stage", "--", "unrelated"]),
        }
    }
    fn check(self, repo: &Fixture) {
        assert_eq!(self.head, repo.git(&["rev-parse", "HEAD"]));
        assert_eq!(self.refs, repo.git(&["show-ref"]));
        assert_eq!(
            self.config,
            std::fs::read(repo.0.join(".git/config")).unwrap()
        );
        assert_eq!(
            self.unrelated,
            repo.git(&["ls-files", "--stage", "--", "unrelated"])
        );
        assert_eq!(
            std::fs::read(repo.0.join("unrelated")).unwrap(),
            b"unrelated\n"
        );
    }
}

#[test]
fn stage_uses_execution_time_disk_bytes_and_replaces_staged_intermediate_version() {
    for bytes in [b"disk\n".as_slice(), b"binary\0payload\xff"] {
        let repo = fixture();
        repo.write("a", "intermediate\n");
        repo.git(&["add", "--", "a"]);
        repo.write("a", "captured unstaged\n");
        let request = request(&repo, "a", Group::Unstaged, Operation::Stage);
        repo.write("a", bytes);
        let invariants = Invariants::capture(&repo);
        success(request.execute());
        assert_eq!(staged(&repo, "a"), bytes);
        assert_eq!(std::fs::read(repo.0.join("a")).unwrap(), bytes);
        invariants.check(&repo);
    }
}

#[test]
fn stage_and_unstage_additions_and_deletions_do_not_touch_worktree() {
    let repo = fixture();
    repo.write("new", b"new bytes\0");
    let invariants = Invariants::capture(&repo);
    success(request(&repo, "new", Group::Untracked, Operation::Stage).execute());
    assert_eq!(staged(&repo, "new"), b"new bytes\0");
    success(request(&repo, "new", Group::Staged, Operation::Unstage).execute());
    assert!(repo.git(&["ls-files", "--stage", "--", "new"]).is_empty());
    assert_eq!(std::fs::read(repo.0.join("new")).unwrap(), b"new bytes\0");
    std::fs::remove_file(repo.0.join("a")).unwrap();
    success(request(&repo, "a", Group::Unstaged, Operation::Stage).execute());
    assert!(repo.git(&["ls-files", "--stage", "--", "a"]).is_empty());
    success(request(&repo, "a", Group::Staged, Operation::Unstage).execute());
    assert_eq!(staged(&repo, "a"), b"head\n");
    assert!(!repo.0.join("a").exists());
    invariants.check(&repo);
}

#[test]
fn deletion_with_missing_parent_directory_is_supported() {
    let repo = fixture();
    std::fs::create_dir(repo.0.join("nested")).unwrap();
    repo.write("nested/a", "nested\n");
    repo.commit();
    std::fs::remove_dir_all(repo.0.join("nested")).unwrap();
    success(request(&repo, "nested/a", Group::Unstaged, Operation::Stage).execute());
    success(request(&repo, "nested/a", Group::Staged, Operation::Unstage).execute());
    assert_eq!(staged(&repo, "nested/a"), b"nested\n");
    assert!(!repo.0.join("nested").exists());
}

#[test]
fn unstage_modification_restores_head_not_worktree_or_intermediate_index() {
    let repo = fixture();
    repo.write("a", "intermediate\n");
    repo.git(&["add", "--", "a"]);
    repo.write("a", "disk after staging\n");
    let invariants = Invariants::capture(&repo);
    success(request(&repo, "a", Group::Staged, Operation::Unstage).execute());
    assert_eq!(staged(&repo, "a"), b"head\n");
    assert_eq!(
        std::fs::read(repo.0.join("a")).unwrap(),
        b"disk after staging\n"
    );
    invariants.check(&repo);
}

#[test]
fn unborn_addition_unstage_only_removes_its_index_entry() {
    let repo = Fixture::new();
    repo.write("a", "unborn\n");
    repo.write("keep", "keep staged\n");
    repo.git(&["add", "--", "keep"]);
    success(request(&repo, "a", Group::Untracked, Operation::Stage).execute());
    assert_eq!(staged(&repo, "a"), b"unborn\n");
    let other = repo.git(&["ls-files", "--stage", "--", "keep"]);
    success(request(&repo, "a", Group::Staged, Operation::Unstage).execute());
    assert!(repo.git(&["ls-files", "--stage", "--", "a"]).is_empty());
    assert_eq!(repo.git(&["ls-files", "--stage", "--", "keep"]), other);
    assert_eq!(std::fs::read(repo.0.join("a")).unwrap(), b"unborn\n");
    assert!(
        !repo
            .command()
            .args(["rev-parse", "--verify", "HEAD"])
            .output()
            .unwrap()
            .status
            .success()
    );
}

#[cfg(unix)]
#[test]
fn executable_bits_are_staged_and_restored_without_changing_disk_mode() {
    use std::os::unix::fs::PermissionsExt;
    let repo = fixture();
    std::fs::set_permissions(repo.0.join("a"), std::fs::Permissions::from_mode(0o755)).unwrap();
    success(request(&repo, "a", Group::Unstaged, Operation::Stage).execute());
    assert!(
        repo.git(&["ls-files", "--stage", "--", "a"])
            .starts_with(b"100755 ")
    );
    success(request(&repo, "a", Group::Staged, Operation::Unstage).execute());
    assert!(
        repo.git(&["ls-files", "--stage", "--", "a"])
            .starts_with(b"100644 ")
    );
    assert_ne!(
        std::fs::metadata(repo.0.join("a"))
            .unwrap()
            .permissions()
            .mode()
            & 0o111,
        0
    );
}

#[test]
fn stage_keeps_builtin_normalization() {
    let repo = fixture();
    repo.write(".gitattributes", "a text eol=lf\n");
    repo.commit();
    repo.write("a", b"normalized\r\n");
    success(request(&repo, "a", Group::Unstaged, Operation::Stage).execute());
    assert_eq!(staged(&repo, "a"), b"normalized\n");
    assert_eq!(std::fs::read(repo.0.join("a")).unwrap(), b"normalized\r\n");
}

#[test]
fn leading_dashes_spaces_and_newlines_are_literal_single_file_targets() {
    for name in ["--all", "space name", "line\nbreak"] {
        let repo = fixture();
        repo.write(name, "literal\n");
        repo.write("also untracked", "must not be staged\n");
        success(request(&repo, name, Group::Untracked, Operation::Stage).execute());
        assert_eq!(staged(&repo, name), b"literal\n");
        assert!(repo.git(&["ls-files", "--", "also untracked"]).is_empty());
        success(request(&repo, name, Group::Staged, Operation::Unstage).execute());
        assert!(repo.git(&["ls-files", "--", name]).is_empty());
    }
}

#[cfg(unix)]
#[test]
fn raw_non_utf8_path_is_not_round_tripped_through_display() {
    use std::os::unix::ffi::OsStringExt;
    let repo = fixture();
    let path = PathBuf::from(std::ffi::OsString::from_vec(b"raw-\xff".to_vec()));
    repo.write(&path, "raw bytes\n");
    success(request(&repo, &path, Group::Untracked, Operation::Stage).execute());
    let output = repo
        .command()
        .arg("show")
        .arg({
            let mut value = std::ffi::OsString::from(":");
            value.push(&path);
            value
        })
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"raw bytes\n");
    success(request(&repo, &path, Group::Staged, Operation::Unstage).execute());
}

fn refused(report: Report, repo: &Fixture, index: &[u8]) -> String {
    assert_eq!(
        report.outcome,
        Outcome::NotExecuted,
        "{}",
        report.diagnostic
    );
    assert!(report.verification.is_none());
    assert_eq!(std::fs::read(repo.0.join(".git/index")).unwrap(), index);
    report.diagnostic
}

#[test]
fn stale_head_index_xy_and_path_type_are_rejected_before_write() {
    for change in ["head", "index", "xy", "type", "gone"] {
        let repo = fixture();
        repo.write("a", "old changed\n");
        let request = request(&repo, "a", Group::Unstaged, Operation::Stage);
        match change {
            "head" => {
                repo.git(&["commit", "--allow-empty", "-qm", "external"]);
            }
            "index" => {
                repo.write("a", "external staged\n");
                repo.git(&["add", "--", "a"]);
                repo.write("a", "old changed\n");
            }
            "xy" => {
                repo.write("a", "head\n");
            }
            "type" => {
                std::fs::remove_file(repo.0.join("a")).unwrap();
                std::fs::create_dir(repo.0.join("a")).unwrap();
            }
            "gone" => {
                std::fs::remove_file(repo.0.join("a")).unwrap();
            }
            _ => unreachable!(),
        }
        let index = std::fs::read(repo.0.join(".git/index")).unwrap();
        let report = request.execute();
        assert!(report.stale);
        refused(report, &repo, &index);
    }
}

#[test]
fn malformed_and_magic_paths_are_rejected_before_any_write() {
    let repo = fixture();
    repo.write("new", "new bytes\n");
    for path in [
        "../outside",
        "/absolute",
        "a/../unrelated",
        ":(top)unrelated",
    ] {
        let mut request = request(&repo, "new", Group::Untracked, Operation::Stage);
        request.entry.path = path.into();
        let before = std::fs::read(repo.0.join(".git/index")).unwrap();
        refused(request.execute(), &repo, &before);
    }
}

#[test]
fn sparse_index_is_rejected_even_when_configuration_no_longer_enables_it() {
    let repo = fixture();
    for directory in ["keep", "omit"] {
        std::fs::create_dir(repo.0.join(directory)).unwrap();
        repo.write(format!("{directory}/a"), "nested\n");
    }
    repo.commit();
    repo.git(&["sparse-checkout", "init", "--cone", "--sparse-index"]);
    repo.git(&["sparse-checkout", "set", "keep"]);
    repo.write("keep/a", "dirty\n");
    let request = request(&repo, "keep/a", Group::Unstaged, Operation::Stage);
    repo.git(&["config", "--worktree", "core.sparseCheckout", "false"]);
    repo.git(&["config", "--worktree", "index.sparse", "false"]);
    let before = std::fs::read(repo.0.join(".git/index")).unwrap();
    let error = refused(request.execute(), &repo, &before);
    assert!(error.contains("Sparse index"), "{error}");
}

#[test]
fn ignored_target_after_capture_is_not_forced_into_index() {
    let repo = fixture();
    repo.write("new", "untracked\n");
    let request = request(&repo, "new", Group::Untracked, Operation::Stage);
    repo.write(".git/info/exclude", "new\n");
    let index = std::fs::read(repo.0.join(".git/index")).unwrap();
    refused(request.execute(), &repo, &index);
}

#[test]
fn special_index_flags_and_sparse_settings_are_rejected_without_clearing_them() {
    for setting in [
        "--assume-unchanged",
        "--skip-worktree",
        "core.sparseCheckout",
        "index.sparse",
    ] {
        let repo = fixture();
        repo.write("a", "staged\n");
        repo.git(&["add", "--", "a"]);
        let request = request(&repo, "a", Group::Staged, Operation::Unstage);
        if setting.starts_with("--") {
            repo.git(&["update-index", setting, "--", "a"]);
        } else {
            repo.git(&["config", setting, "true"]);
        }
        let index = std::fs::read(repo.0.join(".git/index")).unwrap();
        let error = refused(request.execute(), &repo, &index);
        assert!(
            error.contains("Special") || error.contains("Sparse"),
            "{error}"
        );
    }
    let repo = fixture();
    repo.write("ita", "intent\n");
    repo.git(&["add", "-N", "--", "ita"]);
    let status = load(&repo);
    let index = status
        .entries()
        .iter()
        .position(|entry| entry.path == Path::new("ita"))
        .unwrap();
    assert!(
        status
            .index_request(Group::Unstaged, index, Operation::Stage)
            .is_err()
    );
}

#[test]
fn intent_to_add_replacing_an_identical_empty_index_blob_is_still_rejected() {
    let repo = fixture();
    repo.write("new", "");
    repo.git(&["add", "--", "new"]);
    let request = request(&repo, "new", Group::Staged, Operation::Unstage);
    let identity = repo.git(&["ls-files", "--stage", "--", "new"]);
    repo.git(&["rm", "--cached", "--", "new"]);
    repo.git(&["add", "-N", "--", "new"]);
    assert_eq!(repo.git(&["ls-files", "--stage", "--", "new"]), identity);
    let before = std::fs::read(repo.0.join(".git/index")).unwrap();
    refused(request.execute(), &repo, &before);
}

#[test]
fn directories_renames_conflicts_and_wrong_groups_cannot_be_selected() {
    let repo = fixture();
    std::fs::create_dir(repo.0.join("dir")).unwrap();
    repo.write("dir/file", "directory\n");
    repo.git(&["mv", "a", "renamed"]);
    repo.write("a", "rename source reappeared\n");
    let status = load(&repo);
    for (index, entry) in status.entries().iter().enumerate() {
        if ["dir", "a", "renamed"]
            .iter()
            .any(|name| entry.path == Path::new(name))
        {
            for group in Group::ALL {
                for operation in [Operation::Stage, Operation::Unstage] {
                    assert!(
                        status.index_request(group, index, operation).is_err(),
                        "{entry:?}"
                    );
                }
            }
        }
    }
    assert!(
        status
            .index_request(Group::Staged, usize::MAX, Operation::Unstage)
            .is_err()
    );
    let repo = fixture();
    repo.git(&["checkout", "-qb", "other"]);
    repo.write("a", "other\n");
    repo.commit();
    repo.git(&["checkout", "-qb", "main-fixture", "HEAD~1"]);
    repo.write("a", "main\n");
    repo.commit();
    assert!(
        !repo
            .command()
            .args(["merge", "other"])
            .output()
            .unwrap()
            .status
            .success()
    );
    let status = load(&repo);
    let index = status
        .entries()
        .iter()
        .position(|entry| entry.path == Path::new("a"))
        .unwrap();
    assert!(
        status
            .index_request(Group::Unmerged, index, Operation::Stage)
            .is_err()
    );
    assert!(
        status
            .index_request(Group::Staged, index, Operation::Unstage)
            .is_err()
    );
}

#[cfg(unix)]
#[test]
fn symlinks_type_changes_and_symbolic_link_parents_are_rejected() {
    use std::os::unix::fs::symlink;
    let repo = fixture();
    symlink("unrelated", repo.0.join("link")).unwrap();
    let status = load(&repo);
    let index = status
        .entries()
        .iter()
        .position(|entry| entry.path == Path::new("link"))
        .unwrap();
    let symlink_request = status
        .index_request(Group::Untracked, index, Operation::Stage)
        .unwrap();
    let before = std::fs::read(repo.0.join(".git/index")).unwrap();
    refused(symlink_request.execute(), &repo, &before);
    std::fs::remove_file(repo.0.join("a")).unwrap();
    symlink("unrelated", repo.0.join("a")).unwrap();
    let status = load(&repo);
    let index = status
        .entries()
        .iter()
        .position(|entry| entry.path == Path::new("a"))
        .unwrap();
    assert!(
        status
            .index_request(Group::Unstaged, index, Operation::Stage)
            .is_err()
    );

    let outside = fixture();
    std::fs::create_dir(repo.0.join("nested")).unwrap();
    repo.write("nested/a", "tracked\n");
    repo.git(&["add", "--", "nested/a"]);
    repo.git(&["commit", "-qm", "nested"]);
    repo.write("nested/a", "dirty\n");
    let request = request(&repo, "nested/a", Group::Unstaged, Operation::Stage);
    std::fs::remove_dir_all(repo.0.join("nested")).unwrap();
    symlink(&outside.0, repo.0.join("nested")).unwrap();
    let before = std::fs::read(repo.0.join(".git/index")).unwrap();
    refused(request.execute(), &repo, &before);
    assert_eq!(std::fs::read(outside.0.join("a")).unwrap(), b"head\n");
}

#[test]
fn submodule_entry_is_display_only() {
    let source = fixture();
    let repo = fixture();
    repo.git(&[
        "-c",
        "protocol.file.allow=always",
        "submodule",
        "add",
        "-q",
        "--",
        source.0.to_str().unwrap(),
        "module",
    ]);
    let status = load(&repo);
    let index = status
        .entries()
        .iter()
        .position(|entry| entry.path == Path::new("module"))
        .unwrap();
    assert!(
        status
            .index_request(Group::Staged, index, Operation::Unstage)
            .is_err()
    );
}

#[test]
fn fixed_full_oid_is_used_even_if_head_moves_after_preflight() {
    let repo = fixture();
    repo.write("a", "second commit\n");
    repo.commit();
    repo.write("a", "staged intermediate\n");
    repo.git(&["add", "--", "a"]);
    let prepared = request(&repo, "a", Group::Staged, Operation::Unstage)
        .prepare()
        .unwrap();
    repo.git(&["update-ref", "HEAD", "HEAD~1"]);
    let moved = repo.git(&["rev-parse", "HEAD"]);
    success(prepared.execute());
    assert_eq!(staged(&repo, "a"), b"second commit\n");
    assert_eq!(repo.git(&["rev-parse", "HEAD"]), moved);
    assert_eq!(
        std::fs::read(repo.0.join("a")).unwrap(),
        b"staged intermediate\n"
    );
}

#[test]
fn ordinary_index_lock_failure_is_not_retried_or_deleted_and_still_verifies() {
    let repo = fixture();
    repo.write("a", "changed\n");
    let request = request(&repo, "a", Group::Unstaged, Operation::Stage);
    repo.write(".git/index.lock", "owned by external Git\n");
    let before = std::fs::read(repo.0.join(".git/index")).unwrap();
    let report = request.execute();
    assert_eq!(report.outcome, Outcome::Failed, "{}", report.diagnostic);
    assert!(report.stale);
    assert!(report.verification.unwrap().is_ok());
    assert_eq!(std::fs::read(repo.0.join(".git/index")).unwrap(), before);
    assert_eq!(
        std::fs::read(repo.0.join(".git/index.lock")).unwrap(),
        b"owned by external Git\n"
    );
}

#[test]
fn external_filter_is_screened_for_untracked_target_before_status_or_add() {
    for kind in ["clean", "process"] {
        let repo = fixture();
        repo.write("new", "new bytes\n");
        let request = request(&repo, "new", Group::Untracked, Operation::Stage);
        repo.write(".gitattributes", "new filter=evil\n");
        repo.git(&[
            "config",
            &format!("filter.evil.{kind}"),
            "touch SHOULD_NOT_EXIST; cat",
        ]);
        let before = std::fs::read(repo.0.join(".git/index")).unwrap();
        let error = refused(request.execute(), &repo, &before);
        assert!(error.contains("external clean/process filter"), "{error}");
        assert!(!repo.0.join("SHOULD_NOT_EXIST").exists());
    }
}

#[cfg(unix)]
#[test]
fn write_disables_hooks_fsmonitor_textconv_and_diff_helpers() {
    use std::os::unix::fs::PermissionsExt;
    let repo = fixture();
    repo.write("a", "changed\n");
    for name in ["post-index-change", "pre-commit", "monitor"] {
        let path = repo.0.join(".git/hooks").join(name);
        std::fs::write(&path, "#!/bin/sh\ntouch SHOULD_NOT_EXIST\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    repo.git(&[
        "config",
        "core.fsmonitor",
        repo.0.join(".git/hooks/monitor").to_str().unwrap(),
    ]);
    repo.git(&["config", "diff.evil.textconv", "touch SHOULD_NOT_EXIST"]);
    repo.git(&["config", "diff.evil.command", "touch SHOULD_NOT_EXIST"]);
    repo.write(".gitattributes", "a diff=evil\n");
    success(request(&repo, "a", Group::Unstaged, Operation::Stage).execute());
    success(request(&repo, "a", Group::Staged, Operation::Unstage).execute());
    assert!(!repo.0.join("SHOULD_NOT_EXIST").exists());
}

#[test]
fn write_success_and_verification_failure_are_independent() {
    let repo = fixture();
    repo.write("a", "write me\n");
    let prepared = request(&repo, "a", Group::Unstaged, Operation::Stage)
        .prepare()
        .unwrap();
    let executed = prepared.write().unwrap();
    repo.write(".gitattributes", "unrelated filter=evil\n");
    repo.git(&["config", "filter.evil.clean", "touch SHOULD_NOT_EXIST; cat"]);
    let report = executed.verify();
    assert_eq!(report.outcome, Outcome::Succeeded, "{}", report.diagnostic);
    assert!(report.verification.unwrap().is_err());
    assert_eq!(staged(&repo, "a"), b"write me\n");
    assert!(!repo.0.join("SHOULD_NOT_EXIST").exists());
}

#[test]
fn diagnostics_are_bounded_at_utf8_boundary_with_explicit_marker() {
    let value = bounded("诊断".repeat(DIAGNOSTIC_LIMIT));
    assert!(value.len() <= DIAGNOSTIC_LIMIT);
    assert!(value.ends_with("[diagnostic truncated]"));
}

#[test]
fn redirected_write_environment_child() {
    if std::env::var_os("SEMACS_INDEX_ENV_CHILD").is_none() {
        return;
    }
    let repo = fixture();
    repo.write("a", "changed\n");
    let request = request(&repo, "a", Group::Unstaged, Operation::Stage);
    let before = std::fs::read(repo.0.join(".git/index")).unwrap();
    let error = refused(request.execute(), &repo, &before);
    assert!(error.contains("environment override"), "{error}");
}

#[test]
fn environment_overrides_are_refused_not_silently_sanitized_for_writes() {
    let decoy = fixture();
    let before = std::fs::read(decoy.0.join(".git/index")).unwrap();
    for name in [
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_QUARANTINE_PATH",
    ] {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "status::index::tests::redirected_write_environment_child",
                "--nocapture",
            ])
            .env("SEMACS_INDEX_ENV_CHILD", "1")
            .env(name, decoy.0.join(".git/index"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert_eq!(std::fs::read(decoy.0.join(".git/index")).unwrap(), before);
}
