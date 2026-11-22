use super::*;
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

struct Fixture(PathBuf);

impl Fixture {
    fn new(tag: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "semacs-project-{tag}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn directory(&self, relative: &str) -> PathBuf {
        let path = self.0.join(relative);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn file(&self, relative: &str, content: &str) -> PathBuf {
        let path = self.0.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, content).unwrap();
        path
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn repository_root_is_the_nearest_git_ancestor() {
    let fixture = Fixture::new("repo");
    let repo = fixture.directory("repo");
    fixture.directory("repo/.git");
    let nested = fixture.directory("repo/src/nested");
    let file = fixture.file("repo/src/nested/main.rs", "");

    for directory in [&repo, &repo.join("src"), &nested] {
        assert_eq!(root_for(None, directory).unwrap(), repo);
    }
    assert_eq!(root_for(Some(&file), &fixture.0).unwrap(), repo);
}

#[test]
fn directory_without_vcs_is_its_own_root() {
    let fixture = Fixture::new("plain");
    let sub = fixture.directory("sub");
    let file = fixture.file("sub/file.txt", "");

    assert_eq!(root_for(None, &fixture.0).unwrap(), fixture.0);
    assert_eq!(root_for(None, &sub).unwrap(), sub);
    assert_eq!(root_for(Some(&file), &fixture.0).unwrap(), sub);
}

#[test]
fn git_file_marks_a_worktree_root_without_resolving_its_contents() {
    let fixture = Fixture::new("worktree");
    let repo = fixture.directory("worktree");
    fixture.file("worktree/.git", "gitdir: /not/present\n");
    let file = fixture.file("worktree/src/lib.rs", "");

    assert_eq!(root_for(Some(&file), &fixture.0).unwrap(), repo);
}

#[test]
fn inner_repository_wins_over_outer() {
    let fixture = Fixture::new("nested");
    fixture.directory(".git");
    let inner = fixture.directory("vendor/inner");
    fixture.file("vendor/inner/.git", "");
    let file = fixture.file("vendor/inner/src/mod.rs", "");

    assert_eq!(root_for(Some(&file), &fixture.0).unwrap(), inner);
}

#[test]
fn deleted_file_still_resolves_through_its_directory() {
    let fixture = Fixture::new("deleted");
    fixture.directory(".git");
    let file = fixture.file("src/gone.rs", "");
    fs::remove_file(&file).unwrap();

    assert_eq!(root_for(Some(&file), &fixture.0).unwrap(), fixture.0);
}

#[test]
fn new_file_without_vcs_uses_its_own_directory() {
    let fixture = Fixture::new("new");
    let sub = fixture.directory("sub");

    assert_eq!(
        root_for(Some(&sub.join("new.txt")), &fixture.0).unwrap(),
        sub
    );
}

#[test]
fn relative_files_use_the_supplied_working_directory() {
    let fixture = Fixture::new("relative");
    let sub = fixture.directory("sub");

    assert_eq!(root_for(Some(Path::new("file.txt")), &sub).unwrap(), sub);
    assert_eq!(
        root_for(Some(Path::new("sub/../sub/file.txt")), &fixture.0).unwrap(),
        sub
    );
}

#[test]
fn buffer_without_file_uses_working_directory() {
    let fixture = Fixture::new("scratch");
    let repo = fixture.directory("repo");
    fixture.directory("repo/.git");
    let nested = fixture.directory("repo/src");
    let other = fixture.directory("other");

    assert_eq!(root_for(None, &nested).unwrap(), repo);
    assert_eq!(root_for(None, &other).unwrap(), other);
}

#[test]
fn absolute_file_does_not_use_an_unrelated_working_directory() {
    let fixture = Fixture::new("absolute");
    let repo = fixture.directory("repo");
    fixture.directory("repo/.git");
    let file = fixture.file("repo/src/main.rs", "");
    let other = fixture.directory("other");
    fixture.directory("other/.git");

    assert_eq!(root_for(Some(&file), &other).unwrap(), repo);
}

#[test]
fn fixed_root_becomes_unavailable_without_falling_back_to_parent_repository() {
    let fixture = Fixture::new("unavailable");
    fixture.directory(".git");
    let inner = fixture.directory("inner");
    fixture.directory("inner/.git");
    let root = root_for(None, &inner).unwrap();
    assert!(is_available(&root));

    fs::remove_dir_all(&inner).unwrap();
    assert!(!is_available(&root));
    fs::write(&inner, "not a directory").unwrap();
    assert!(!is_available(&root));
}

#[test]
fn fixed_root_does_not_require_a_git_marker() {
    let fixture = Fixture::new("marker-removed");
    let marker = fixture.directory(".git");
    let root = root_for(None, &fixture.0).unwrap();

    fs::remove_dir(&marker).unwrap();
    assert!(is_available(&root));
}

#[cfg(unix)]
#[test]
fn symlink_path_spelling_is_preserved() {
    let fixture = Fixture::new("symlink");
    let repo = fixture.directory("repo");
    fixture.directory("repo/.git");
    fixture.file("repo/src/main.rs", "");
    let link = fixture.0.join("link");
    std::os::unix::fs::symlink(&repo, &link).unwrap();

    assert_eq!(
        root_for(Some(&link.join("src/main.rs")), &fixture.0).unwrap(),
        link
    );
    assert!(is_available(&link));
}

#[cfg(unix)]
#[test]
fn broken_marker_symlink_does_not_mark_a_repository() {
    let fixture = Fixture::new("broken-marker");
    std::os::unix::fs::symlink(fixture.0.join("missing"), fixture.0.join(".git")).unwrap();
    let sub = fixture.directory("sub");

    assert_eq!(root_for(None, &sub).unwrap(), sub);
}

#[test]
fn find_in_directory_distinguishes_git_projects_from_plain_directories() {
    let fixture = Fixture::new("find-in-dir");
    let plain = fixture.directory("plain");
    let repo = fixture.directory("repo");
    fixture.directory("repo/.git");
    let nested = fixture.directory("repo/a/b");

    assert_eq!(find_in_directory(&plain), None);
    assert_eq!(find_in_directory(&nested), Some(Project::git(repo.clone())));
    assert_eq!(find_in_directory(&repo).unwrap().name(), "repo");
}

#[test]
fn project_list_remembers_mru_forgets_prunes_zombies_and_persists() {
    let fixture = Fixture::new("list-persist");
    let alpha = fixture.directory("alpha");
    let beta = fixture.directory("beta");
    let gamma = fixture.directory("gamma");
    let state = fixture.0.join("state/projects");

    let (mut list, persistence) = open_list(state.clone()).unwrap();
    assert!(list.entries().is_empty());
    assert!(list.remember(&alpha));
    assert!(!list.remember(&alpha));
    assert!(list.remember(&beta));
    assert!(list.remember(&gamma));
    assert_eq!(
        list.roots(),
        vec![gamma.clone(), beta.clone(), alpha.clone()]
    );
    assert!(list.remember(&alpha));
    assert_eq!(
        list.roots(),
        vec![alpha.clone(), gamma.clone(), beta.clone()]
    );
    assert!(list.forget(&gamma));
    assert!(!list.forget(&gamma));
    assert_eq!(list.roots(), vec![alpha.clone(), beta.clone()]);

    fs::remove_dir_all(&beta).unwrap();
    assert_eq!(list.prune_zombies(), 1);
    assert_eq!(list.roots(), vec![alpha.clone()]);
    drop(persistence);

    let (reloaded, persistence) = open_list(state).unwrap();
    assert_eq!(reloaded.roots(), vec![alpha]);
    drop(persistence);
}

fn run_git(dir: &Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .status()
        .expect("run git");
    assert!(status.success(), "git {args:?} failed");
}

#[test]
fn list_files_for_git_includes_tracked_and_untracked_and_respects_gitignore() {
    let fixture = Fixture::new("git-files");
    let repo = fixture.directory("repo");
    run_git(&repo, &["init"]);
    fixture.file("repo/.gitignore", "ignored.txt\ntarget/\n");
    fixture.file("repo/tracked.rs", "fn main() {}\n");
    fixture.file("repo/src/lib.rs", "pub fn x() {}\n");
    fixture.file("repo/ignored.txt", "skip\n");
    fixture.file("repo/target/out.bin", "skip\n");
    run_git(&repo, &["add", ".gitignore", "tracked.rs"]);
    run_git(&repo, &["commit", "-m", "init"]);

    let files = list_files(&Project::git(repo), &AtomicBool::new(false)).unwrap();
    assert_eq!(files, vec![".gitignore", "src/lib.rs", "tracked.rs"]);
}

#[test]
fn list_files_for_transient_walks_files_and_skips_vcs_directories() {
    let fixture = Fixture::new("transient-files");
    let root = fixture.directory("dir");
    fixture.file("dir/a.txt", "a");
    fixture.file("dir/nested/b.txt", "b");
    fixture.file("dir/.svn/entries", "skip");
    fixture.file("dir/.hg/store", "skip");
    fixture.file("dir/.hidden", "keep");

    let files = list_files(&Project::transient(root), &AtomicBool::new(false)).unwrap();
    assert_eq!(files, vec![".hidden", "a.txt", "nested/b.txt"]);
}

#[test]
fn rank_project_files_prioritizes_history_excludes_current_then_length_alpha() {
    let root = PathBuf::from("/tmp/semacs-rank-proj");
    let candidates = vec![
        crate::completion::Candidate::named("src/long_name.rs".into(), String::new()),
        crate::completion::Candidate::named("a.rs".into(), String::new()),
        crate::completion::Candidate::named("src/b.rs".into(), String::new()),
        crate::completion::Candidate::named("src/cur.rs".into(), String::new()),
    ];
    let mut items = crate::completion::filter_unranked(candidates, "", &AtomicBool::new(false));
    let history = vec![
        "/tmp/semacs-rank-proj/src/cur.rs".to_owned(),
        "/tmp/semacs-rank-proj/src/long_name.rs".to_owned(),
    ];
    rank_project_files(
        &mut items,
        &history,
        &root,
        Some(Path::new("/tmp/semacs-rank-proj/src/cur.rs")),
    );
    let labels: Vec<_> = items.iter().map(|i| i.candidate.label.as_str()).collect();
    assert_eq!(
        labels,
        vec!["src/long_name.rs", "a.rs", "src/b.rs", "src/cur.rs"]
    );
}

#[test]
fn projects_under_finds_direct_and_recursive_git_roots() {
    let fixture = Fixture::new("scan-under");
    let parent = fixture.directory("workspace");
    let one = fixture.directory("workspace/one");
    fixture.directory("workspace/one/.git");
    let plain = fixture.directory("workspace/plain");
    let deep = fixture.directory("workspace/plain/deep");
    fixture.directory("workspace/plain/deep/.git");
    let _ = plain;

    let direct = projects_under(&parent, false, &AtomicBool::new(false)).unwrap();
    assert_eq!(direct, vec![one.clone()]);

    let recursive = projects_under(&parent, true, &AtomicBool::new(false)).unwrap();
    assert_eq!(recursive, vec![one, deep]);
}

#[test]
fn project_list_rejects_unnormalized_entries_and_deduplicates_on_load() {
    let fixture = Fixture::new("list-dedup");
    let alpha = fixture.directory("alpha");
    let beta = fixture.directory("beta");
    let state = fixture.0.join("state/projects");
    fs::create_dir_all(state.parent().unwrap()).unwrap();

    // Duplicates are deduplicated while preserving newest-first order
    let dup_text = format!(
        "{}\n{}\n{}\n",
        alpha.display(),
        beta.display(),
        alpha.display()
    );
    fs::write(&state, dup_text).unwrap();
    let (list, persistence) = open_list(state.clone()).unwrap();
    assert_eq!(list.roots(), vec![alpha.clone(), beta.clone()]);
    drop(persistence);

    // Non-canonical entries (trailing slash) fall back cleanly to an empty list
    fs::write(&state, format!("{}/\n", alpha.display())).unwrap();
    let (list, persistence) = open_list(state).unwrap();
    assert!(list.entries().is_empty());
    drop(persistence);
}

#[cfg(unix)]
#[test]
fn git_submodule_symlink_cannot_escape_or_loop() {
    let fixture = Fixture::new("submod-symlink");
    let repo = fixture.directory("repo");
    run_git(&repo, &["init"]);
    fixture.file("repo/main.rs", "fn main() {}\n");
    fixture.file(
        "repo/.gitmodules",
        "[submodule \"loop\"]\n\tpath = sub\n\turl = .\n",
    );
    std::os::unix::fs::symlink(&repo, repo.join("sub")).unwrap();
    run_git(&repo, &["add", "main.rs", ".gitmodules"]);
    run_git(&repo, &["commit", "-m", "init"]);

    let files = list_files(&Project::git(repo), &AtomicBool::new(false)).unwrap();
    assert!(files.contains(&"main.rs".to_owned()));
    assert!(!files.iter().any(|f| f.starts_with("sub/")));
}

#[cfg(unix)]
#[test]
fn projects_under_follows_symlinked_directories_non_recursively() {
    let fixture = Fixture::new("scan-symlink");
    let real = fixture.directory("real-repo");
    fixture.directory("real-repo/.git");
    let workspace = fixture.directory("ws");
    let link = workspace.join("linked-repo");
    std::os::unix::fs::symlink(&real, &link).unwrap();

    let found = projects_under(&workspace, false, &AtomicBool::new(false)).unwrap();
    assert_eq!(found, vec![link]);
}
