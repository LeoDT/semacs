use super::*;
use std::fs;
use std::sync::atomic::AtomicU64;

pub(crate) struct Fixture(pub PathBuf);

impl Fixture {
    pub fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
            ".pi/tmp/f6-tree-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }

    pub fn directory(&self, name: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::create_dir_all(&path).unwrap();
        path
    }

    pub fn file(&self, name: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "").unwrap();
        path
    }

    pub fn tree(&self) -> Tree {
        Tree::new(self.0.clone())
            .refresh(&AtomicBool::new(false))
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn default_text_matches_the_treemacs_reference() {
    let fixture = Fixture::new();
    fixture.directory(".git");
    fixture.directory(".jj");
    for name in [
        "aaa.txt",
        "B-upper.txt",
        "1.txt",
        "ignored.log",
        ".gitignore",
        ".hidden.txt",
        "b-dir/inner.txt",
        "chain/one/two/deep.txt",
        "Dir1/y.txt",
        "dir2/x.txt",
    ] {
        fixture.file(name);
    }
    let tree = fixture.tree();
    let rendered = tree.render(None, None).unwrap();
    let root = fixture.0.file_name().unwrap().to_string_lossy();
    assert_eq!(
        rendered.text,
        format!(
            "{root}\n  + Dir1\n  + b-dir\n  + chain/one/two\n  + dir2\n   .gitignore\n   .hidden.txt\n   1.txt\n   B-upper.txt\n   aaa.txt\n   ignored.log"
        )
    );
    assert_eq!(rendered.sections.visible_lines().len(), 11);
    assert_eq!(tree.loaded.len(), 1);
}

#[test]
fn directory_expansion_is_lazy_and_only_follows_single_child_chains() {
    let fixture = Fixture::new();
    fixture.file("dir/a/one.txt");
    fixture.file("dir/b/two.txt");
    let tree = fixture.tree();
    assert!(!tree.is_loaded(&fixture.0.join("dir")));
    assert!(!tree.directories.contains_key(&fixture.0.join("dir/a")));
    assert!(!tree.render(None, None).unwrap().text.contains("one.txt"));
    let tree = tree
        .expand(&fixture.0.join("dir"), &AtomicBool::new(false))
        .unwrap();
    let rendered = tree
        .render(None, Some((&fixture.0.join("dir"), false)))
        .unwrap();
    assert!(rendered.text.contains("  - dir\n    + a\n    + b"));
    assert!(!rendered.text.contains("one.txt"));
}

#[test]
fn collapsing_stops_after_three_links() {
    let fixture = Fixture::new();
    fixture.file("a/b/c/d/e/f/file");
    let tree = fixture.tree();
    assert!(tree.render(None, None).unwrap().text.contains("+ a/b/c/d"));
    assert!(!tree.render(None, None).unwrap().text.contains("/e"));
}

#[test]
fn refresh_does_not_turn_chain_probes_into_recursive_scans() {
    let fixture = Fixture::new();
    fixture.file("a/b/c/d/e/f/g/h/file");
    let mut tree = fixture.tree();
    let count = tree.directories.len();
    for _ in 0..5 {
        tree = tree.refresh(&AtomicBool::new(false)).unwrap();
        assert_eq!(tree.directories.len(), count);
        assert_eq!(tree.loaded.len(), 1);
    }
}

#[test]
fn hidden_filter_is_applied_before_sorting_and_chain_collapsing() {
    let fixture = Fixture::new();
    fixture.file(".hidden");
    fixture.file("chain/.stop");
    fixture.file("chain/next/deep");
    let mut tree = fixture.tree();
    assert!(tree.render(None, None).unwrap().text.contains("+ chain\n"));
    tree.show_hidden = false;
    let tree = tree.refresh(&AtomicBool::new(false)).unwrap();
    let text = tree.render(None, None).unwrap().text;
    assert!(!text.contains(".hidden"));
    assert!(text.contains("+ chain/next"));
}

#[test]
fn disappeared_directories_are_pruned_on_refresh() {
    let fixture = Fixture::new();
    fixture.file("dir/one");
    let path = fixture.0.join("dir");
    let tree = fixture
        .tree()
        .expand(&path, &AtomicBool::new(false))
        .unwrap();
    fs::remove_dir_all(&path).unwrap();
    let tree = tree.refresh(&AtomicBool::new(false)).unwrap();
    assert!(!tree.is_loaded(&path));
    assert!(!tree.render(None, None).unwrap().text.contains("dir"));
}

#[test]
fn root_read_failure_and_cancellation_do_not_fall_back_or_truncate() {
    let fixture = Fixture::new();
    fixture.file("one");
    let tree = fixture.tree();
    assert!(tree.clone().refresh(&AtomicBool::new(true)).is_err());
    fs::remove_dir_all(&fixture.0).unwrap();
    assert!(tree.refresh(&AtomicBool::new(false)).is_err());
}

#[cfg(unix)]
#[test]
fn symlink_cycles_are_bounded_by_the_chain_probe_depth() {
    let fixture = Fixture::new();
    std::os::unix::fs::symlink(&fixture.0, fixture.0.join("loop")).unwrap();
    let tree = fixture.tree();
    assert_eq!(tree.directories.len(), 5);
    assert_eq!(tree.loaded.len(), 1);
    assert!(
        tree.render(None, None)
            .unwrap()
            .text
            .contains("loop/loop/loop/loop")
    );
}

#[cfg(unix)]
#[test]
fn non_utf8_names_have_distinct_keys_and_newlines_do_not_create_fake_rows() {
    use std::os::unix::ffi::OsStringExt;
    for bytes in [vec![0xff], vec![0xfe]] {
        assert_ne!(
            node_key(Path::new(&std::ffi::OsString::from_vec(bytes))),
            node_key(Path::new("�"))
        );
    }
    let fixture = Fixture::new();
    fixture.file("one\ntwo");
    let rendered = fixture.tree().render(None, None).unwrap();
    assert_eq!(rendered.sections.visible_lines().len(), 2);
    assert!(rendered.text.contains("one\\ntwo"));
}
