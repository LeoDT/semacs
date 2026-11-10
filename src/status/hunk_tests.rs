use super::*;
use crate::diff::tests::Repository as Fixture;
use crate::status::index::{Operation, Outcome, Request};
use crate::text::{ByteOffset, Rope};
use std::path::Path;
use std::sync::atomic::AtomicBool;

fn load(repo: &Fixture) -> Status {
    Status::load(
        Repository {
            root: repo.0.clone(),
            git_dir: repo.0.join(".git"),
        },
        &AtomicBool::new(false),
    )
    .unwrap()
}

fn request(
    status: &Status,
    path: &str,
    group: Group,
    hunk: usize,
    operation: Operation,
) -> Request {
    let entry = status
        .entries()
        .iter()
        .position(|entry| entry.path == Path::new(path))
        .unwrap();
    let rendered = status.render().unwrap();
    let location = rendered
        .locations
        .iter()
        .find(|location| {
            location.entry == entry && location.group == group && location.hunk == Some(hunk)
        })
        .unwrap();
    status.index_request_at(location, operation).unwrap()
}

fn apply(request: Request) -> Status {
    let report = request.execute();
    assert_eq!(report.outcome, Outcome::Succeeded, "{}", report.diagnostic);
    report.verification.unwrap().unwrap()
}

fn base() -> String {
    (1..=40).map(|n| format!("line {n}\n")).collect()
}

fn fixture() -> Fixture {
    let repo = Fixture::new();
    repo.write("a", base());
    repo.write("other", "untouched\n");
    repo.commit();
    repo
}

#[test]
fn single_hunk_stage_unstage_handles_prior_size_changes_and_preserves_all_other_bytes() {
    let repo = fixture();
    let base = base();
    let first = base.replace("line 2\n", "line 2\ninserted one\ninserted two\n");
    let both = first.replace("line 30\n", "changed thirty\n");
    repo.write("a", &both);
    let refs = repo.git(&["show-ref"]);
    let other = repo.git(&["ls-files", "--stage", "--", "other"]);
    let status = load(&repo);
    assert_eq!(status.file_diff(Group::Unstaged, 0).unwrap().hunks.len(), 2);
    let status = apply(request(&status, "a", Group::Unstaged, 1, Operation::Stage));
    assert_eq!(
        repo.git(&["show", ":a"]),
        base.replace("line 30\n", "changed thirty\n").as_bytes()
    );
    let status = apply(request(&status, "a", Group::Unstaged, 0, Operation::Stage));
    assert_eq!(repo.git(&["show", ":a"]), both.as_bytes());
    let status = apply(request(&status, "a", Group::Staged, 1, Operation::Unstage));
    assert_eq!(repo.git(&["show", ":a"]), first.as_bytes());
    apply(request(&status, "a", Group::Staged, 0, Operation::Unstage));
    assert_eq!(repo.git(&["show", ":a"]), base.as_bytes());
    assert_eq!(std::fs::read(repo.0.join("a")).unwrap(), both.as_bytes());
    assert_eq!(repo.git(&["show-ref"]), refs);
    assert_eq!(repo.git(&["ls-files", "--stage", "--", "other"]), other);
}

#[test]
fn captured_patch_uses_git_context_not_a_head_index_or_disk_snapshot_gate() {
    let repo = fixture();
    let base = base();
    let changed = base.replace("line 30\n", "captured thirty\n");
    repo.write("a", &changed);
    let pending = request(&load(&repo), "a", Group::Unstaged, 0, Operation::Stage);
    let earlier = base.replace("line 2\n", "independent earlier edit\n");
    repo.write("a", &earlier);
    repo.git(&["add", "a"]);
    repo.write("a", "disk no longer resembles captured patch\n");
    let status = apply(pending);
    let expected = earlier.replace("line 30\n", "captured thirty\n");
    assert_eq!(repo.git(&["show", ":a"]), expected.as_bytes());
    let pending = request(&status, "a", Group::Staged, 1, Operation::Unstage);
    repo.git(&[
        "-c",
        "core.hooksPath=/dev/null",
        "commit",
        "-qm",
        "head moved",
    ]);
    apply(pending);
    assert_eq!(repo.git(&["show", ":a"]), earlier.as_bytes());
    assert_eq!(
        std::fs::read(repo.0.join("a")).unwrap(),
        b"disk no longer resembles captured patch\n"
    );
}

#[test]
fn unmatched_hunk_fails_without_retry_and_refresh_does_not_change_the_outcome() {
    let repo = fixture();
    repo.write("a", base().replace("line 30\n", "wanted\n"));
    let pending = request(&load(&repo), "a", Group::Unstaged, 0, Operation::Stage);
    repo.write("a", "unrelated index content\n");
    repo.git(&["add", "a"]);
    let index = repo.git(&["ls-files", "--stage", "-z"]);
    let report = pending.execute();
    assert_eq!(report.outcome, Outcome::Failed, "{}", report.diagnostic);
    assert!(report.diagnostic.contains("patch"));
    assert!(report.verification.unwrap().is_ok());
    assert_eq!(repo.git(&["ls-files", "--stage", "-z"]), index);
}

#[test]
fn additions_deletions_and_missing_final_newline_round_trip_on_unborn_and_existing_heads() {
    for unborn in [true, false] {
        let repo = Fixture::new();
        if !unborn {
            repo.write("other", "keep\n");
            repo.commit();
        }
        repo.write("new", "new text without final newline");
        repo.git(&["add", "new"]);
        let status = load(&repo);
        let file = status.file_diff(Group::Staged, 0).unwrap();
        assert!(
            file.hunks[0]
                .lines
                .iter()
                .any(|line| line.text.starts_with("\\ No newline"))
        );
        apply(request(
            &status,
            "new",
            Group::Staged,
            0,
            Operation::Unstage,
        ));
        assert!(repo.git(&["ls-files", "--", "new"]).is_empty());
        assert_eq!(
            std::fs::read(repo.0.join("new")).unwrap(),
            b"new text without final newline"
        );
    }

    let repo = Fixture::new();
    repo.write("a", "old without newline");
    repo.commit();
    repo.write("a", "new without newline");
    let status = apply(request(
        &load(&repo),
        "a",
        Group::Unstaged,
        0,
        Operation::Stage,
    ));
    assert_eq!(repo.git(&["show", ":a"]), b"new without newline");
    apply(request(&status, "a", Group::Staged, 0, Operation::Unstage));
    assert_eq!(repo.git(&["show", ":a"]), b"old without newline");
    std::fs::remove_file(repo.0.join("a")).unwrap();
    let status = apply(request(
        &load(&repo),
        "a",
        Group::Unstaged,
        0,
        Operation::Stage,
    ));
    assert!(repo.git(&["ls-files", "--", "a"]).is_empty());
    apply(request(&status, "a", Group::Staged, 0, Operation::Unstage));
    assert_eq!(repo.git(&["show", ":a"]), b"old without newline");
    assert!(!repo.0.join("a").exists());
}

#[cfg(unix)]
#[test]
fn text_hunk_does_not_stage_chmod_and_git_quoted_raw_paths_are_kept() {
    use std::os::unix::{ffi::OsStringExt, fs::PermissionsExt};
    let repo = Fixture::new();
    let name = std::ffi::OsString::from_vec(b"quote\" tab\t line\n raw\xff".to_vec());
    repo.write(&name, "old\n");
    repo.commit();
    repo.write(&name, "new\n");
    std::fs::set_permissions(repo.0.join(&name), std::fs::Permissions::from_mode(0o755)).unwrap();
    repo.git(&["config", "core.filemode", "true"]);
    let status = load(&repo);
    let rendered = status.render().unwrap();
    let location = rendered
        .locations
        .iter()
        .find(|location| location.hunk == Some(0))
        .unwrap();
    let status = apply(status.index_request_at(location, Operation::Stage).unwrap());
    let index = repo.git(&["ls-files", "--stage", "-z"]);
    assert!(index.starts_with(b"100644 "));
    assert!(
        std::fs::metadata(repo.0.join(&name))
            .unwrap()
            .permissions()
            .mode()
            & 0o111
            != 0
    );
    let location = status
        .render()
        .unwrap()
        .locations
        .into_iter()
        .find(|location| location.group == Group::Staged && location.hunk == Some(0))
        .unwrap();
    apply(
        status
            .index_request_at(&location, Operation::Unstage)
            .unwrap(),
    );
    assert_eq!(repo.git(&["ls-files", "--stage", "-z"]), {
        let status = load(&repo);
        let entry = &status.entries()[0];
        let Record::Tracked { state, .. } = &entry.record else {
            panic!()
        };
        let mut expected = format!("100644 {} 0\t", state.head_oid).into_bytes();
        expected.extend_from_slice(name.as_encoded_bytes());
        expected.push(0);
        expected
    });
}

#[test]
fn binary_and_non_utf8_contents_are_explicit_summaries_without_losing_file_operations() {
    let repo = Fixture::new();
    repo.write("binary", b"old\0binary");
    repo.write("invalid", b"old\xff\n");
    repo.commit();
    repo.write("binary", b"new\0binary");
    repo.write("invalid", b"new\xfe\n");
    let status = load(&repo);
    let rendered = status.render().unwrap();
    assert!(rendered.text.contains("Binary contents"));
    assert!(rendered.text.contains("Non-UTF-8 contents"));
    assert!(
        rendered
            .locations
            .iter()
            .all(|location| location.hunk.is_none())
    );
    for entry in 0..status.entries().len() {
        assert!(
            status
                .index_request(Group::Unstaged, entry, Operation::Stage)
                .is_ok()
        );
    }
}

#[test]
fn hunk_writes_keep_builtin_crlf_normalization_and_do_not_modify_disk() {
    let repo = Fixture::new();
    repo.git(&["config", "core.autocrlf", "true"]);
    repo.write("a", b"old\r\ncontext\r\n");
    repo.commit();
    repo.write("a", b"new\r\ncontext\r\n");
    let status = apply(request(
        &load(&repo),
        "a",
        Group::Unstaged,
        0,
        Operation::Stage,
    ));
    assert_eq!(repo.git(&["show", ":a"]), b"new\ncontext\n");
    apply(request(&status, "a", Group::Staged, 0, Operation::Unstage));
    assert_eq!(repo.git(&["show", ":a"]), b"old\ncontext\n");
    assert_eq!(
        std::fs::read(repo.0.join("a")).unwrap(),
        b"new\r\ncontext\r\n"
    );
}

#[test]
fn hunk_write_respects_index_lock_and_rejects_new_special_flags_without_clearing_them() {
    for flag in ["lock", "--skip-worktree", "--assume-unchanged", "intent"] {
        let repo = fixture();
        repo.write("a", base().replace("line 30\n", "changed\n"));
        let pending = request(&load(&repo), "a", Group::Unstaged, 0, Operation::Stage);
        if flag == "lock" {
            repo.write(".git/index.lock", "foreign lock");
        } else if flag == "intent" {
            repo.git(&["rm", "--cached", "a"]);
            repo.git(&["add", "-N", "a"]);
        } else {
            repo.git(&["update-index", flag, "a"]);
        }
        let index = std::fs::read(repo.0.join(".git/index")).unwrap();
        let report = pending.execute();
        assert_eq!(
            report.outcome,
            if flag == "lock" {
                Outcome::Failed
            } else {
                Outcome::NotExecuted
            },
            "{flag}: {}",
            report.diagnostic
        );
        assert_eq!(std::fs::read(repo.0.join(".git/index")).unwrap(), index);
        if flag == "lock" {
            assert_eq!(
                std::fs::read(repo.0.join(".git/index.lock")).unwrap(),
                b"foreign lock"
            );
        }
    }
}

#[test]
fn conflict_paths_do_not_prevent_other_status_files_from_loading_hunks() {
    let repo = fixture();
    repo.git(&["checkout", "-qb", "side"]);
    repo.write("a", "side\n");
    repo.commit();
    repo.git(&["checkout", "-q", "-"]);
    repo.write("a", "main\n");
    repo.commit();
    assert!(
        !repo
            .command()
            .args(["merge", "side"])
            .output()
            .unwrap()
            .status
            .success()
    );
    repo.write("other", "ordinary modified\n");
    let status = load(&repo);
    assert_eq!(status.group(Group::Unmerged).count(), 1);
    let index = status
        .entries()
        .iter()
        .position(|entry| entry.path == Path::new("other"))
        .unwrap();
    assert_eq!(
        status
            .file_diff(Group::Unstaged, index)
            .unwrap()
            .hunks
            .len(),
        1
    );
    let staged = apply(request(
        &status,
        "other",
        Group::Unstaged,
        0,
        Operation::Stage,
    ));
    assert_eq!(staged.group(Group::Unmerged).count(), 1);
    assert_eq!(repo.git(&["show", ":other"]), b"ordinary modified\n");
}

#[test]
fn directory_file_conflicts_cannot_expand_deleted_or_renamed_pathspecs_into_unmerged_diffs() {
    let repo = Fixture::new();
    std::fs::create_dir(repo.0.join("old")).unwrap();
    repo.write("old/child", "base\n");
    repo.write("other", "ordinary base\n");
    repo.commit();
    repo.git(&["checkout", "-qb", "side"]);
    repo.write("old/child", "side modification\n");
    repo.commit();
    repo.git(&["checkout", "-q", "-"]);
    std::fs::remove_file(repo.0.join("old/child")).unwrap();
    std::fs::remove_dir(repo.0.join("old")).unwrap();
    repo.write("old", "replacement file\n");
    repo.commit();
    assert!(
        !repo
            .command()
            .args(["merge", "side"])
            .output()
            .unwrap()
            .status
            .success()
    );
    repo.write("other", "ordinary modified\n");

    for renamed in [false, true] {
        if renamed {
            repo.git(&["add", "--", "old~HEAD"]);
        }
        let status = load(&repo);
        let conflicts = if renamed { 1 } else { 2 };
        assert_eq!(status.group(Group::Unmerged).count(), conflicts);
        assert!(
            status
                .group(Group::Unmerged)
                .any(|(_, entry)| entry.path == Path::new("old/child"))
        );
        assert!(status.group(Group::Staged).any(|(_, entry)| {
            if renamed {
                matches!(&entry.record, Record::Tracked { rename: Some(rename), .. } if rename.source == Path::new("old"))
            } else {
                entry.path == Path::new("old") && entry.xy[0] == b'D'
            }
        }));
        let ordinary = status
            .entries()
            .iter()
            .position(|entry| entry.path == Path::new("other"))
            .unwrap();
        assert_eq!(
            status
                .file_diff(Group::Unstaged, ordinary)
                .unwrap()
                .hunks
                .len(),
            1
        );
        assert!(
            status
                .render()
                .unwrap()
                .text
                .contains(&format!("Unmerged paths ({conflicts})"))
        );
        assert!(
            crate::diff::Diff::load(
                crate::diff::Comparison {
                    repository: status.repository().clone(),
                    kind: crate::diff::DiffKind::Unstaged,
                    environment: crate::diff::ReadEnvironment::Projection,
                },
                &AtomicBool::new(false)
            )
            .is_err()
        );
    }
}

#[test]
fn status_files_start_collapsed_restore_hunks_and_fall_back_to_the_surviving_file() {
    let repo = fixture();
    repo.write(
        "a",
        base()
            .replace("line 2\n", "first\n")
            .replace("line 30\n", "second\n"),
    );
    let status = load(&repo);
    let mut rendered = status.render().unwrap();
    let file = rendered
        .sections
        .nodes()
        .iter()
        .position(|node| node.key.starts_with("unstaged/file/"))
        .unwrap();
    assert!(rendered.sections.nodes()[file].collapsed);
    let first = file + 1;
    let second = file + 2;
    let point = rendered.sections.nodes()[second].body;
    assert!(rendered.location(point).is_none());
    rendered.sections.toggle(file).unwrap();
    assert_eq!(rendered.location(point).unwrap().hunk, Some(1));
    rendered.sections.toggle(first).unwrap();
    let anchor = rendered
        .sections
        .anchor(&Rope::from(rendered.text.as_str()), point);
    let mut refreshed = status.render().unwrap();
    refreshed.sections.restore(&rendered.sections);
    assert!(!refreshed.sections.nodes()[file].collapsed);
    assert!(refreshed.sections.nodes()[first].collapsed);
    assert_eq!(
        refreshed
            .sections
            .resolve(&Rope::from(refreshed.text.as_str()), &anchor),
        point
    );

    repo.write("a", base().replace("line 2\n", "first\n"));
    let mut next = load(&repo).render().unwrap();
    next.sections.restore(&refreshed.sections);
    let rope = Rope::from(next.text.as_str());
    assert_eq!(
        next.sections.resolve(&rope, &anchor),
        next.sections.nodes()[file].range.start
    );
    assert!(next.location(ByteOffset(next.text.len())).is_none());
}
