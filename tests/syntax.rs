use std::fs;
use std::path::{Path, PathBuf};

use gpui_kit::{Entity, TestAppContext, VisualTestContext};
use semacs::view::BufferView;

mod support;
use support::*;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = PathBuf::from(format!(
            ".pi/tmp/e1-syntax-save-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(fs::canonicalize(path).unwrap())
    }
    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn settle(visual: &mut VisualTestContext, view: &Entity<BufferView>) {
    redraw(visual);
    redraw(visual);
    assert!(visual.read(|cx| view.read(cx).syntax_ready()));
}

fn write_file(visual: &mut VisualTestContext, target: &Path) {
    visual.simulate_keystrokes("ctrl-x ctrl-w");
    clear_input(visual);
    visual.simulate_input(target.to_str().unwrap());
    visual.simulate_keystrokes("alt-enter");
    redraw(visual);
}

#[gpui_kit::test]
fn successful_write_file_changes_language_but_failure_keeps_old_language_and_identity(
    cx: &mut TestAppContext,
) {
    let fixture = Fixture::new();
    let source = fixture.path("source.txt");
    fs::write(&source, "fn main() {}\n").unwrap();
    let (mut visual, _, view) = open(buffer_at(&source, "fn main() {}\n"), cx);
    let before = visual.read(|cx| view.read(cx).buffer().snapshot_id());
    let target = fixture.path("saved.rs");
    write_file(&mut visual, &target);
    settle(&mut visual, &view);
    visual.read(|cx| {
        let view = view.read(cx);
        assert_eq!(view.buffer().path().as_deref(), Some(target.as_path()));
        assert_eq!(view.buffer().snapshot_id(), before);
        assert!(!view.buffer().is_modified());
        assert_eq!(view.syntax_snapshot(), Some(before));
    });
    assert_eq!(fs::read(&target).unwrap(), b"fn main() {}\n");
    let invalid = fixture.path("missing/saved.txt");
    write_file(&mut visual, &invalid);
    settle(&mut visual, &view);
    visual.read(|cx| {
        let view = view.read(cx);
        assert_eq!(view.buffer().path().as_deref(), Some(target.as_path()));
        assert_eq!(view.buffer().snapshot_id(), before);
        assert_eq!(view.syntax_snapshot(), Some(before));
    });
    assert!(!invalid.exists());
    for name in ["saved.jsx", "saved.tsx", "saved.md", "saved.html"] {
        let target = fixture.path(name);
        write_file(&mut visual, &target);
        settle(&mut visual, &view);
        visual.read(|cx| {
            let view = view.read(cx);
            assert_eq!(view.buffer().path().as_deref(), Some(target.as_path()));
            assert_eq!(view.buffer().snapshot_id(), before);
            assert_eq!(view.syntax_snapshot(), Some(before));
            assert!(!view.buffer().is_modified());
        });
        assert_eq!(fs::read(target).unwrap(), b"fn main() {}\n");
    }
    let plain = fixture.path("plain.txt");
    write_file(&mut visual, &plain);
    settle(&mut visual, &view);
    visual.read(|cx| {
        assert_eq!(
            view.read(cx).buffer().path().as_deref(),
            Some(plain.as_path())
        )
    });
    assert!(visual.read(|cx| view.read(cx).syntax_snapshot().is_none()));
    assert_eq!(fs::read(plain).unwrap(), b"fn main() {}\n");
}
