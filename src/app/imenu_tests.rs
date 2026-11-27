use super::*;
use crate::buffer::Buffer;
use crate::command::Command;
use crate::project::ProjectList;
use gpui_kit as gpui;
use gpui_kit::{Entity, TestAppContext, VisualTestContext, px, size};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

struct Fixture(PathBuf);

impl Fixture {
    fn new(tag: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "semacs-app-imenu-{tag}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn git_repo(&self, name: &str) -> PathBuf {
        let dir = self.0.join(name);
        fs::create_dir_all(&dir).unwrap();
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(&dir)
            .args(["init"])
            .status()
            .expect("git init");
        assert!(status.success());
        dir
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

fn open(
    buffer: Buffer,
    startup: PathBuf,
    cx: &mut TestAppContext,
) -> (VisualTestContext, Entity<AppShell>) {
    cx.background_executor.allow_parking();
    cx.update(|cx| {
        crate::app::init(cx);
        cx.set_global(ProjectList::default());
        cx.set_global(crate::file_history::FileHistory::default());
        cx.set_global(ProjectOutlineCache::default());
    });
    let window = cx.open_window(size(px(900.), px(600.)), |window, cx| {
        let mut shell = AppShell::new(buffer, window, cx);
        shell.startup_directory = startup;
        shell
    });
    let shell = window.entity(cx).unwrap();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.update(|window, _| window.activate_window());
    draw(&mut visual);
    (visual, shell)
}

fn draw(visual: &mut VisualTestContext) {
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    visual.run_until_parked();
}

fn wait_ready(visual: &mut VisualTestContext, shell: &Entity<AppShell>) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        draw(visual);
        let busy = visual.read(|cx| {
            let shell = shell.read(cx);
            let mini_busy = shell.minibuffer.as_ref().is_some_and(|mount| {
                mount.imenu_work.is_some()
                    || mount.project_imenu_work.is_some()
                    || mount.completion.is_some()
            });
            mini_busy
                || shell.file_task.is_some()
                || shell.opening.is_some()
                || shell.tree_work.is_some()
        });
        if !busy {
            return;
        }
        assert!(Instant::now() < deadline, "async imenu work timed out");
        std::thread::yield_now();
    }
}

fn echo(visual: &mut VisualTestContext, shell: &Entity<AppShell>) -> String {
    visual.update(|window, cx| shell.read(cx).echo_message(window, cx))
}

fn line_at_point(visual: &VisualTestContext, shell: &Entity<AppShell>) -> usize {
    visual.read(|cx| {
        let buf = shell.read(cx).active_view().read(cx).buffer();
        buf.text().offset_to_point(buf.point().0).row as usize + 1
    })
}

fn command(visual: &mut VisualTestContext, shell: &Entity<AppShell>, command: Command) {
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let origin = shell.active_view().clone();
            shell.run_command(command.into(), &origin, window, cx);
        })
    });
}

#[gpui::test]
fn imenu_lists_filters_disambiguates_duplicates_saves_mark_and_uses_default(
    cx: &mut TestAppContext,
) {
    let fixture = Fixture::new("single");
    let rs = fixture.file(
        "sample.rs",
        "struct Alpha;\nimpl Alpha {\n    fn new() {}\n}\nstruct Beta;\nimpl Beta {\n    fn new() {}\n}\nfn target() {\n    Alpha::new();\n}\n",
    );
    let loaded = crate::files::load(&rs).unwrap();
    let (mut visual, shell) = open(
        Buffer::visiting(rs.clone(), loaded.file, loaded.stamp),
        fixture.0.clone(),
        cx,
    );

    // Wait for background syntax parse, then verify document-order candidates.
    let deadline = Instant::now() + Duration::from_secs(5);
    while !visual.read(|cx| {
        shell
            .read(cx)
            .active_view()
            .read(cx)
            .buffer()
            .syntax_ready()
    }) {
        draw(&mut visual);
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }

    // Place point on `target` at line 10 (`Alpha::new`)? Let's place point on `Beta` at line 5.
    visual.update(|window, cx| {
        shell
            .read(cx)
            .active_view()
            .clone()
            .update(cx, |view, cx| view.show_line_column(5, 8, window, cx));
    });
    let initial_point = visual.read(|cx| shell.read(cx).active_view().read(cx).buffer().point());

    visual.simulate_keystrokes("alt-g i");
    wait_ready(&mut visual, &shell);

    visual.read(|cx| {
        let shell = shell.read(cx);
        let session = shell.session().expect("imenu session active");
        assert_eq!(session.kind().prompt(), "Index item (default Beta): ");
        let list = session.candidates().unwrap();
        let labels: Vec<_> = list
            .items
            .iter()
            .map(|item| item.candidate.label.as_str())
            .collect();
        assert_eq!(
            labels,
            vec![
                "Alpha",
                "Alpha",
                "Alpha / new",
                "Beta",
                "Beta",
                "Beta / new",
                "target"
            ]
        );
        assert_eq!(list.selected_candidate().unwrap().value, "Beta");
    });

    // Empty RET accepts default `Beta`.
    visual.simulate_keystrokes("enter");
    draw(&mut visual);
    assert_eq!(line_at_point(&visual, &shell), 5);
    visual.read(|cx| {
        let buf = shell.read(cx).active_view().read(cx).buffer();
        assert_eq!(buf.mark(), Some(initial_point));
        assert!(!buf.mark_active());
    });

    // Now test duplicate-name disambiguation via Candidate.id on `new`.
    let md = fixture.file("doc.md", "# A\n## sub\n# B\n## sub\n");
    let loaded_md = crate::files::load(&md).unwrap();
    let (mut visual, shell) = open(
        Buffer::visiting(md, loaded_md.file, loaded_md.stamp),
        fixture.0.clone(),
        cx,
    );
    visual.simulate_keystrokes("alt-g i");
    wait_ready(&mut visual, &shell);
    visual.simulate_input("sub");
    wait_ready(&mut visual, &shell);
    // Select the second `sub` (B / sub) using C-n and accept.
    visual.simulate_keystrokes("ctrl-n");
    draw(&mut visual);
    visual.read(|cx| {
        let list = shell.read(cx).session().unwrap().candidates().unwrap();
        assert_eq!(list.selected_candidate().unwrap().label, "B / sub");
    });
    visual.simulate_keystrokes("enter");
    draw(&mut visual);
    assert_eq!(line_at_point(&visual, &shell), 4);
}

#[gpui::test]
fn imenu_reports_missing_items_and_reflects_unsaved_edits_before_reparse(cx: &mut TestAppContext) {
    let fixture = Fixture::new("empty");
    let txt = fixture.file("plain.txt", "hello world\n");
    let loaded_txt = crate::files::load(&txt).unwrap();
    let (mut visual, shell) = open(
        Buffer::visiting(txt, loaded_txt.file, loaded_txt.stamp),
        fixture.0.clone(),
        cx,
    );
    visual.simulate_keystrokes("alt-g i");
    draw(&mut visual);
    visual.read(|cx| assert!(shell.read(cx).session().is_none()));
    assert_eq!(
        echo(&mut visual, &shell),
        "No items suitable for an index found in this buffer"
    );

    // Empty .rs file also reports missing items without keeping session open.
    let empty_rs = fixture.file("empty.rs", "// just a comment\n");
    let loaded_rs = crate::files::load(&empty_rs).unwrap();
    let (mut visual, shell) = open(
        Buffer::visiting(empty_rs, loaded_rs.file, loaded_rs.stamp),
        fixture.0.clone(),
        cx,
    );
    visual.simulate_keystrokes("alt-g i");
    wait_ready(&mut visual, &shell);
    visual.read(|cx| assert!(shell.read(cx).session().is_none()));
    assert_eq!(
        echo(&mut visual, &shell),
        "No items suitable for an index found in this buffer"
    );

    // Unsaved edit adds a function; invoking imenu immediately sees it via async rope fallback.
    visual.simulate_input("fn fresh_unsaved() {}");
    visual.simulate_keystrokes("alt-g i");
    wait_ready(&mut visual, &shell);
    visual.read(|cx| {
        let list = shell.read(cx).session().unwrap().candidates().unwrap();
        assert_eq!(list.items.len(), 1);
        assert_eq!(list.items[0].candidate.label, "fresh_unsaved");
    });
    visual.simulate_keystrokes("enter");
    draw(&mut visual);
    assert_eq!(line_at_point(&visual, &shell), 1);
}

#[gpui::test]
fn project_imenu_scans_overlays_caches_cancels_and_opens_from_treemacs(cx: &mut TestAppContext) {
    let fixture = Fixture::new("project");
    let repo = fixture.git_repo("app");
    let a_rs = fixture.file("app/src/a.rs", "fn from_a() {}\n");
    let b_rs = fixture.file(
        "app/src/b.rs",
        "struct Model;\nimpl Model {\n    fn update() {}\n}\n",
    );
    let la = crate::files::load(&a_rs).unwrap();
    let (mut visual, shell) = open(
        Buffer::visiting(a_rs.clone(), la.file, la.stamp),
        repo.clone(),
        cx,
    );

    // Unsaved edit in a.rs should appear in project-imenu results.
    visual.simulate_input("fn unsaved_a() {}\n");
    draw(&mut visual);
    visual.update(|_, cx| {
        cx.global_mut::<ProjectList>().remember(&repo);
    });

    // Trigger via C-x p p -> enter -> i
    visual.simulate_keystrokes("ctrl-x p p");
    draw(&mut visual);
    visual.simulate_keystrokes("enter");
    draw(&mut visual);
    visual.simulate_keystrokes("i");
    wait_ready(&mut visual, &shell);
    visual.read(|cx| {
        let session = shell.read(cx).session().unwrap();
        assert!(matches!(session.kind(), ReadKind::ProjectImenu { .. }));
        let list = session.candidates().unwrap();
        let labels: Vec<_> = list
            .items
            .iter()
            .map(|item| item.candidate.label.as_str())
            .collect();
        assert!(labels.contains(&"unsaved_a"));
        assert!(labels.contains(&"Model / update"));
    });

    // Cancel with C-g
    visual.simulate_keystrokes("ctrl-g");
    draw(&mut visual);
    visual.read(|cx| assert!(shell.read(cx).session().is_none()));

    // Open treemacs side window and invoke C-x p i from inside treemacs; verify ordinary pane opens b.rs at Model::update (line 3).
    command(&mut visual, &shell, Command::TreemacsSelectWindow);
    wait_ready(&mut visual, &shell);
    visual.simulate_keystrokes("ctrl-x p i");
    wait_ready(&mut visual, &shell);
    visual.simulate_input("update");
    wait_ready(&mut visual, &shell);
    visual.simulate_keystrokes("enter");
    wait_ready(&mut visual, &shell);

    visual.read(|cx| {
        let shell = shell.read(cx);
        assert_ne!(shell.panes.side(), Some(shell.panes.selected()));
        let active = shell.active_view().read(cx);
        assert_eq!(active.buffer().path().as_deref(), Some(b_rs.as_path()));
        let line = active
            .buffer()
            .text()
            .offset_to_point(active.buffer().point().0)
            .row as usize
            + 1;
        assert_eq!(line, 3);
    });
}
