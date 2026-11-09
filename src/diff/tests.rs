use super::*;
use crate::buffer::Buffer;
use crate::text::ByteOffset;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) struct Repository(pub PathBuf);
impl Repository {
    pub fn new() -> Self {
        Self::with_format("sha1")
    }
    fn with_format(format: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(".pi/tmp")
            .join(format!(
                "diff-test-{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        std::fs::create_dir(&root).unwrap();
        let repo = Self(root);
        repo.git(&["init", "-q", &format!("--object-format={format}")]);
        repo.git(&["config", "user.name", "Fixture"]);
        repo.git(&["config", "user.email", "fixture@example.invalid"]);
        repo
    }
    pub fn command(&self) -> Command {
        let mut command = Command::new("git");
        command
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("LC_ALL", "C")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .arg("-C")
            .arg(&self.0);
        command
    }
    pub fn git(&self, args: &[&str]) -> Vec<u8> {
        let result = self.command().args(args).output().unwrap();
        assert!(
            result.status.success(),
            "{:?}: {}",
            args,
            String::from_utf8_lossy(&result.stderr)
        );
        result.stdout
    }
    pub fn write(&self, path: impl AsRef<Path>, text: impl AsRef<[u8]>) {
        std::fs::write(self.0.join(path), text).unwrap();
    }
    pub fn commit(&self) {
        self.git(&["add", "--all"]);
        self.git(&["commit", "-qm", "fixture"]);
    }
    pub fn load(&self, kind: DiffKind) -> Diff {
        Diff::load(
            Comparison {
                repository: crate::git::Repository {
                    root: self.0.clone(),
                    git_dir: self.0.join(".git"),
                },
                kind,
                environment: ReadEnvironment::Projection,
            },
            &AtomicBool::new(false),
        )
        .unwrap()
    }
}
impl Drop for Repository {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn head_to_worktree_includes_both_sides_of_the_index_and_visits_disk() {
    let repo = Repository::new();
    repo.write("a", "base\n");
    repo.commit();
    repo.write("a", "staged\n");
    repo.git(&["add", "a"]);
    repo.write("a", "worktree\n");
    repo.write("untracked", "not included\n");
    let diff = repo.load(DiffKind::Worktree);
    assert_eq!(diff.files.len(), 1);
    let text = diff.render().unwrap().text;
    assert!(text.contains("-base"));
    assert!(text.contains("+worktree"));
    assert!(!text.contains("+staged"));
    assert!(
        matches!(diff.visit(0, None, Some(1), 0).unwrap(), Visit::Worktree { path, .. } if path == repo.0.join("a"))
    );
    assert!(
        matches!(diff.visit(0, Some(1), None, 0).unwrap(), Visit::Blob { oid, .. } if oid == diff.files[0].old_oid)
    );
}

#[test]
fn semantic_styles_cover_diffstat_headings_and_every_hunk_line() {
    use crate::theme::StyleRole;
    let repo = Repository::new();
    repo.write("中文.txt", "old\ncontext\n");
    repo.commit();
    repo.write("中文.txt", "new\ncontext\n");
    let rendered = repo.load(DiffKind::Unstaged).render().unwrap();
    let spans: Vec<_> = rendered
        .styles
        .iter()
        .map(|(range, role)| (&rendered.text[range.start.0..range.end.0], *role))
        .collect();
    for (text, role) in [
        ("+1", StyleRole::DiffStatAdded),
        ("-1", StyleRole::DiffStatRemoved),
        ("+new\n", StyleRole::DiffAdded),
        ("-old\n", StyleRole::DiffRemoved),
        (" context\n", StyleRole::DiffContext),
    ] {
        assert!(spans.contains(&(text, role)), "{text:?}");
    }
    assert!(
        spans
            .iter()
            .any(|(text, role)| text.starts_with("@@") && *role == StyleRole::DiffHunk)
    );
    assert!(
        rendered
            .styles
            .windows(2)
            .all(|pair| pair[0].0.end <= pair[1].0.start)
    );
}

#[test]
fn head_to_worktree_uses_the_repository_empty_tree_for_unborn_sha1_and_sha256() {
    for format in ["sha1", "sha256"] {
        let repo = Repository::with_format(format);
        repo.write("a", "initial\n");
        repo.git(&["add", "-N", "a"]);
        let diff = repo.load(DiffKind::Worktree);
        assert_eq!(diff.files.len(), 1);
        assert_eq!(diff.files[0].status, 'A');
        assert!(diff.render().unwrap().text.contains("+initial"));
    }
}

#[test]
fn fixture_commands_ignore_inherited_git_environment_child() {
    if std::env::var_os("SEMACS_TEST_GIT_ENV_CHILD").is_none() {
        return;
    }
    let repo = Repository::new();
    repo.write("only-in-fixture", "fixture\n");
    repo.commit();
    let root = repo.git(&["rev-parse", "--show-toplevel"]);
    assert_eq!(
        Path::new(std::str::from_utf8(&root).unwrap().trim_end()),
        repo.0
    );
    assert_eq!(repo.git(&["show", "HEAD:only-in-fixture"]), b"fixture\n");
}

#[test]
fn fixture_commands_isolate_redirects_and_config_in_a_child_process() {
    let decoy = Repository::new();
    decoy.write("untouched", "untouched\n");
    decoy.commit();
    let index_before = std::fs::read(decoy.0.join(".git/index")).unwrap();
    let head_before = decoy.git(&["rev-parse", "HEAD"]);
    let result = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "diff::tests::fixture_commands_ignore_inherited_git_environment_child",
            "--nocapture",
        ])
        .env("SEMACS_TEST_GIT_ENV_CHILD", "1")
        .env("GIT_DIR", decoy.0.join(".git"))
        .env("GIT_COMMON_DIR", decoy.0.join(".git"))
        .env("GIT_WORK_TREE", &decoy.0)
        .env("GIT_INDEX_FILE", decoy.0.join(".git/index"))
        .env("GIT_OBJECT_DIRECTORY", decoy.0.join(".git/objects"))
        .env(
            "GIT_ALTERNATE_OBJECT_DIRECTORIES",
            decoy.0.join(".git/objects"),
        )
        .env("GIT_CONFIG_COUNT", "1")
        .env("GIT_CONFIG_KEY_0", "core.bare")
        .env("GIT_CONFIG_VALUE_0", "true")
        .env("GIT_CONFIG_PARAMETERS", "invalid config injection")
        .env("GIT_TRACE", decoy.0.join("SHOULD_NOT_EXIST"))
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        std::fs::read(decoy.0.join(".git/index")).unwrap(),
        index_before
    );
    assert_eq!(decoy.git(&["rev-parse", "HEAD"]), head_before);
    assert!(!decoy.0.join("only-in-fixture").exists());
    assert!(!decoy.0.join("SHOULD_NOT_EXIST").exists());
}

#[test]
fn real_staged_unstaged_and_line_versions_are_distinct() {
    let repo = Repository::new();
    repo.write("a.txt", "old\nkeep\n");
    repo.commit();
    repo.write("a.txt", "indexed\nkeep\n");
    repo.git(&["add", "a.txt"]);
    repo.write("a.txt", "worktree\nkeep\n");
    let staged = repo.load(DiffKind::Staged);
    let unstaged = repo.load(DiffKind::Unstaged);
    assert_eq!(staged.files.len(), 1);
    assert_eq!(unstaged.files.len(), 1);
    assert_eq!(staged.files[0].new_oid, unstaged.files[0].old_oid);
    let rendered = staged.render().unwrap();
    for (needle, contents) in [("-old", "old\nkeep\n"), ("+indexed", "indexed\nkeep\n")] {
        let offset = rendered.text.find(needle).unwrap();
        let Visit::Blob {
            oid, line, column, ..
        } = rendered
            .navigation
            .visit(&staged, ByteOffset(offset + 2))
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(line, 1);
        assert_eq!(column, 1);
        assert_eq!(
            crate::git::blob(&repo.0, &repo.0.join(".git"), &oid, &AtomicBool::new(false)).unwrap(),
            contents
        );
    }
    let rendered = unstaged.render().unwrap();
    let offset = rendered.text.find("+worktree").unwrap();
    assert!(matches!(
        rendered.navigation.visit(&unstaged, ByteOffset(offset)),
        Ok(Visit::Worktree { line: 1, .. })
    ));
    let offset = rendered.text.find("-indexed").unwrap();
    assert!(matches!(
        rendered.navigation.visit(&unstaged, ByteOffset(offset)),
        Ok(Visit::Blob { line: 1, .. })
    ));
}

#[test]
fn raw_paths_rename_modes_binary_and_no_newline_are_not_guessed_from_headers() {
    let repo = Repository::new();
    for name in ["old name", "delete", "binary", "mode", "empty"] {
        repo.write(
            name,
            if name == "binary" {
                b"old\0binary".as_slice()
            } else if name == "old name" {
                b"unique rename source".as_slice()
            } else if name == "empty" {
                b"".as_slice()
            } else {
                b"old".as_slice()
            },
        );
    }
    repo.commit();
    repo.git(&["mv", "old name", "new\tname\n\"x"]);
    repo.git(&["rm", "delete"]);
    repo.write("binary", b"new\0binary");
    repo.write("added", "new");
    repo.git(&["add", "--all"]);
    repo.git(&["update-index", "--chmod=+x", "mode"]);
    let diff = repo.load(DiffKind::Staged);
    let renamed = diff.files.iter().find(|file| file.status == 'R').unwrap();
    assert_eq!(renamed.old_path, PathBuf::from("old name"));
    assert_eq!(renamed.new_path, PathBuf::from("new\tname\n\"x"));
    assert!(
        diff.files
            .iter()
            .find(|file| file.new_path == Path::new("binary"))
            .unwrap()
            .summary
            .is_some()
    );
    let mode = diff
        .files
        .iter()
        .find(|file| file.new_path == Path::new("mode"))
        .unwrap();
    assert_eq!(mode.new_mode, "100755");
    assert!(mode.hunks.is_empty());
    let rendered = diff.render().unwrap();
    assert!(rendered.text.contains("new\\tname\\n"));
    let deleted = rendered
        .sections
        .nodes()
        .iter()
        .find(|node| node.key == "file/\"delete\"")
        .unwrap();
    assert!(deleted.collapsed);
    assert!(
        diff.files
            .iter()
            .flat_map(|file| &file.hunks)
            .flat_map(|hunk| &hunk.lines)
            .any(|line| line.text.starts_with("\\ No newline"))
    );
}

#[test]
fn unborn_repository_and_no_changes_are_supported() {
    let repo = Repository::new();
    repo.write("first", "a\n");
    repo.git(&["add", "."]);
    assert_eq!(repo.load(DiffKind::Staged).files[0].status, 'A');
    assert!(repo.load(DiffKind::Unstaged).files.is_empty());
    repo.commit();
    assert!(
        repo.load(DiffKind::Staged)
            .render()
            .unwrap()
            .text
            .contains("No changes")
    );
}

#[test]
fn diff_refresh_rejects_old_requests_and_restores_shared_folding_and_independent_points() {
    let repo = Repository::new();
    repo.write("a", "old\n");
    repo.commit();
    repo.write("a", "new\n");
    let data = repo.load(DiffKind::Unstaged);
    let mut buffer = Buffer::diff(data.comparison.clone());
    let first = buffer.begin_diff_request().unwrap().1;
    let request = buffer.begin_diff_request().unwrap().1;
    assert!(
        !buffer
            .install_diff(first, data.clone(), data.render().unwrap())
            .unwrap()
    );
    assert!(
        buffer
            .install_diff(request, data.clone(), data.render().unwrap())
            .unwrap()
    );
    assert!(buffer.is_read_only());
    assert!(!buffer.is_modified());
    assert_eq!(buffer.mode(), crate::mode::ModeId::MAGIT_DIFF);
    let hunk = buffer
        .sections()
        .unwrap()
        .nodes()
        .iter()
        .position(|node| node.key.contains("/@@"))
        .unwrap();
    let body = buffer.sections().unwrap().nodes()[hunk].body;
    buffer.set_point(body).unwrap();
    let mut second = buffer.new_view();
    let root_point = ByteOffset(0);
    second.set_point(root_point).unwrap();
    buffer.toggle_section().unwrap();
    assert_eq!(
        buffer.point(),
        buffer.sections().unwrap().nodes()[hunk].range.start
    );
    assert_eq!(second.point(), root_point);
    let file = buffer.sections().unwrap().nodes()[hunk].parent.unwrap();
    buffer
        .set_point(buffer.sections().unwrap().nodes()[file].range.start)
        .unwrap();
    buffer.toggle_section().unwrap();
    buffer.toggle_section().unwrap();
    assert!(buffer.sections().unwrap().nodes()[hunk].collapsed);
    let request = buffer.begin_diff_request().unwrap().1;
    buffer
        .install_diff(request, data.clone(), data.render().unwrap())
        .unwrap();
    assert!(second.sections().unwrap().nodes()[hunk].collapsed);
    assert_eq!(second.point(), root_point);
}

#[test]
fn worktree_gitdir_is_pinned_and_user_diff_drivers_cannot_execute() {
    let repo = Repository::new();
    repo.write("a", "old\n\ncontext\n");
    repo.commit();
    let linked = repo.0.join("linked");
    repo.git(&[
        "worktree",
        "add",
        "--detach",
        linked.to_str().unwrap(),
        "HEAD",
    ]);
    std::fs::write(linked.join("a"), "new\n\ncontext\n").unwrap();
    let repository = crate::git::discover(&linked, &AtomicBool::new(false)).unwrap();
    assert_eq!(repository.root, linked);
    assert!(repository.git_dir.is_dir());
    assert_ne!(repository.git_dir, linked.join(".git"));
    repo.git(&["config", "diff.suppressBlankEmpty", "true"]);
    repo.git(&["config", "diff.outputIndicatorNew", "!"]);
    repo.git(&[
        "config",
        "diff.evil.command",
        "sh -c 'touch SHOULD_NOT_EXIST; exit 1'",
    ]);
    repo.git(&[
        "config",
        "diff.evil.textconv",
        "sh -c 'touch SHOULD_NOT_EXIST; exit 1'",
    ]);
    std::fs::write(linked.join(".gitattributes"), "a diff=evil\n").unwrap();
    for environment in [ReadEnvironment::Projection, ReadEnvironment::Commit] {
        let diff = Diff::load(
            Comparison {
                repository: repository.clone(),
                kind: DiffKind::Unstaged,
                environment,
            },
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(diff.files.len(), 1);
        assert!(diff.render().unwrap().text.contains("+new"));
    }
    assert!(!linked.join("SHOULD_NOT_EXIST").exists());
}

#[cfg(unix)]
#[test]
fn file_type_split_patches_and_non_utf8_paths_are_explicitly_supported() {
    use std::os::unix::ffi::OsStringExt;
    use std::os::unix::fs::symlink;
    let repo = Repository::new();
    repo.write("a", "old\n");
    repo.commit();
    std::fs::remove_file(repo.0.join("a")).unwrap();
    symlink("target", repo.0.join("a")).unwrap();
    for kind in [DiffKind::Unstaged, DiffKind::Staged] {
        if kind == DiffKind::Staged {
            repo.git(&["add", "a"]);
        }
        let diff = repo.load(kind);
        assert_eq!(diff.files[0].status, 'T');
        assert!(diff.files[0].hunks.is_empty());
        assert!(diff.files[0].summary.is_some());
        assert!(diff.render().unwrap().text.contains("File-type change"));
    }
    repo.commit();
    std::fs::remove_file(repo.0.join("a")).unwrap();
    repo.write("a", "regular\n");
    repo.git(&["add", "a"]);
    let diff = repo.load(DiffKind::Staged);
    assert_eq!(diff.files[0].status, 'T');
    assert!(diff.files[0].summary.is_some());
    repo.commit();
    let name = PathBuf::from(std::ffi::OsString::from_vec(b"path-\xff".to_vec()));
    repo.write(&name, "old\n");
    repo.commit();
    repo.write(&name, "new\n");
    repo.git(&["config", "core.quotePath", "false"]);
    let diff = repo.load(DiffKind::Unstaged);
    assert_eq!(diff.files[0].new_path, name);
    assert!(diff.render().unwrap().text.contains("+new"));
}

#[test]
fn malformed_protocol_and_unsafe_paths_fail_closed() {
    for bytes in [b":bad\0".as_slice(), b"random", b"\0diff --cc conflict\n"] {
        assert!(super::parse::parse(bytes).is_err());
    }
    assert!(
        crate::git::blob(
            Path::new("."),
            Path::new(".git"),
            "--help",
            &AtomicBool::new(false)
        )
        .is_err()
    );
    let repo = Repository::new();
    let cancelled = AtomicBool::new(true);
    assert!(
        Diff::load(
            Comparison {
                repository: crate::git::Repository {
                    root: repo.0.clone(),
                    git_dir: repo.0.join(".git"),
                },
                kind: DiffKind::Unstaged,
                environment: ReadEnvironment::Projection,
            },
            &cancelled
        )
        .unwrap_err()
        .contains("cancelled")
    );
}
