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
