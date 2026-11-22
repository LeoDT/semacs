use super::*;
use crate::buffer::Buffer;
use crate::project::ProjectList;
use gpui_kit as gpui;
use gpui_kit::{Entity, TestAppContext, VisualTestContext, px, size};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

struct Fixture(PathBuf);

impl Fixture {
    fn new(tag: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "semacs-app-project-{tag}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
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

    fn commit_all(&self, repo: &Path) {
        for args in [vec!["add", "."], vec!["commit", "-m", "initial"]] {
            let status = std::process::Command::new("git")
                .arg("-C")
                .arg(repo)
                .args(args)
                .env("GIT_AUTHOR_NAME", "Test")
                .env("GIT_AUTHOR_EMAIL", "test@example.com")
                .env("GIT_COMMITTER_NAME", "Test")
                .env("GIT_COMMITTER_EMAIL", "test@example.com")
                .status()
                .expect("git commit");
            assert!(status.success());
        }
    }

    fn dir(&self, relative: &str) -> PathBuf {
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

fn open(
    buffer: Buffer,
    startup: PathBuf,
    cx: &mut TestAppContext,
) -> (VisualTestContext, Entity<AppShell>) {
    cx.update(|cx| {
        crate::app::init(cx);
        cx.set_global(ProjectList::default());
        cx.set_global(crate::file_history::FileHistory::default());
    });
    let window = cx.open_window(size(px(800.), px(480.)), |window, cx| {
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

fn echo(visual: &mut VisualTestContext, shell: &Entity<AppShell>) -> String {
    visual.update(|window, cx| shell.read(cx).echo_message(window, cx))
}

#[gpui::test]
fn project_find_file_lists_git_files_visits_choice_and_remembers_project(cx: &mut TestAppContext) {
    let fixture = Fixture::new("find-file");
    let repo = fixture.git_repo("alpha");
    fixture.file("alpha/.gitignore", "ignored.log\n");
    let readme = fixture.file("alpha/README.md", "hello\n");
    let target = fixture.file("alpha/src/main.rs", "fn main() {}\n");
    fixture.file("alpha/ignored.log", "ignored\n");
    fixture.commit_all(&repo);

    let loaded = crate::files::load(&readme).unwrap();
    let buffer = Buffer::visiting(readme, loaded.file, loaded.stamp);
    let (mut visual, shell) = open(buffer, fixture.0.clone(), cx);

    visual.simulate_keystrokes("ctrl-x p f");
    draw(&mut visual);
    visual.read(|cx| {
        let session = shell.read(cx).session().expect("project-find-file session");
        let labels: Vec<_> = session
            .candidates()
            .unwrap()
            .items
            .iter()
            .map(|item| item.candidate.label.as_str())
            .collect();
        assert!(labels.contains(&"src/main.rs"));
        assert!(labels.contains(&"README.md"));
        assert!(!labels.contains(&"ignored.log"));
        assert!(cx.global::<ProjectList>().contains(&repo));
    });

    visual.simulate_input("main");
    draw(&mut visual);
    visual.simulate_keystrokes("enter");
    draw(&mut visual);

    visual.read(|cx| {
        assert!(shell.read(cx).session().is_none());
        assert_eq!(
            shell.read(cx).active_view().read(cx).buffer().path(),
            Some(target.clone())
        );
        assert!(
            cx.global::<crate::file_history::FileHistory>()
                .entries()
                .contains(&crate::files::abbreviate(&target))
        );
    });
}

#[gpui::test]
fn project_find_file_outside_vcs_prompts_and_supports_transient_directory(cx: &mut TestAppContext) {
    let fixture = Fixture::new("transient");
    let plain = fixture.dir("plain");
    let note = fixture.file("plain/note.txt", "transient content\n");
    let (mut visual, shell) = open(Buffer::scratch(), fixture.0.clone(), cx);

    visual.simulate_keystrokes("ctrl-x p f");
    draw(&mut visual);
    visual.read(|cx| {
        let session = shell.read(cx).session().expect("project select");
        assert!(matches!(session.kind(), ReadKind::ProjectSelect { .. }));
        assert_eq!(
            session
                .candidates()
                .unwrap()
                .selected_candidate()
                .unwrap()
                .label,
            CHOOSE_DIR_LABEL
        );
    });

    // Accept `... (choose a dir)` -> enters directory reader
    visual.simulate_keystrokes("enter");
    draw(&mut visual);
    visual.read(|cx| {
        assert!(matches!(
            shell.read(cx).session().unwrap().kind(),
            ReadKind::ProjectDirectory { .. }
        ));
    });

    visual.simulate_keystrokes("ctrl-a ctrl-k");
    visual.simulate_input(&files::directory_input(&plain));
    draw(&mut visual);
    visual.simulate_keystrokes("enter");
    draw(&mut visual);

    visual.read(|cx| {
        assert!(matches!(
            shell.read(cx).session().unwrap().kind(),
            ReadKind::ProjectFindFile { .. }
        ));
        // Transient directory is not persisted in known projects
        assert!(!cx.global::<ProjectList>().contains(&plain));
    });

    visual.simulate_keystrokes("enter");
    draw(&mut visual);
    visual.read(|cx| {
        assert_eq!(
            shell.read(cx).active_view().read(cx).buffer().path(),
            Some(note)
        );
    });
}

#[gpui::test]
fn project_find_file_handles_async_cancellation_and_pending_accept(cx: &mut TestAppContext) {
    let fixture = Fixture::new("async-gate");
    let repo = fixture.git_repo("alpha");
    let a = fixture.file("alpha/a.rs", "a\n");
    let b = fixture.file("alpha/b.rs", "b\n");
    fixture.commit_all(&repo);

    let loaded = crate::files::load(&a).unwrap();
    let (mut visual, shell) = open(
        Buffer::visiting(a, loaded.file, loaded.stamp),
        fixture.0.clone(),
        cx,
    );

    // 1. Cancel while loading
    let (tx, rx) = futures_channel::oneshot::channel();
    shell.update(&mut visual, |shell, _| shell.gate_project_files(rx));
    visual.simulate_keystrokes("ctrl-x p f");
    draw(&mut visual);
    visual.read(|cx| {
        assert!(
            shell
                .read(cx)
                .session()
                .unwrap()
                .candidates()
                .unwrap()
                .loading
        )
    });
    visual.simulate_keystrokes("ctrl-g");
    draw(&mut visual);
    let _ = tx.send(());
    draw(&mut visual);
    visual.read(|cx| assert!(shell.read(cx).session().is_none()));

    // 2. Type and press RET while loading -> pending accept fires when list arrives
    let (tx, rx) = futures_channel::oneshot::channel();
    shell.update(&mut visual, |shell, _| shell.gate_project_files(rx));
    visual.simulate_keystrokes("ctrl-x p f");
    draw(&mut visual);
    visual.simulate_input("b.rs");
    visual.simulate_keystrokes("enter");
    draw(&mut visual);
    let _ = tx.send(());
    draw(&mut visual);
    visual.read(|cx| {
        assert!(shell.read(cx).session().is_none());
        assert_eq!(
            shell.read(cx).active_view().read(cx).buffer().path(),
            Some(b)
        );
    });
}

#[gpui::test]
fn project_find_file_reports_empty_file_list(cx: &mut TestAppContext) {
    let fixture = Fixture::new("empty");
    let repo = fixture.git_repo("empty-repo");
    let (mut visual, shell) = open(Buffer::scratch(), repo, cx);

    visual.simulate_keystrokes("ctrl-x p f");
    draw(&mut visual);
    visual.read(|cx| {
        assert!(shell.read(cx).session().is_none());
    });
    assert_eq!(echo(&mut visual, &shell), "Empty file list");
}

#[gpui::test]
fn project_switch_project_menu_supports_find_file_buffer_status_and_treemacs(
    cx: &mut TestAppContext,
) {
    let fixture = Fixture::new("switch");
    let alpha = fixture.git_repo("alpha");
    let a_file = fixture.file("alpha/a.rs", "alpha\n");
    fixture.commit_all(&alpha);

    let beta = fixture.git_repo("beta");
    let b_file = fixture.file("beta/b.rs", "beta\n");
    fixture.commit_all(&beta);

    let loaded_a = crate::files::load(&a_file).unwrap();
    let (mut visual, shell) = open(
        Buffer::visiting(a_file.clone(), loaded_a.file, loaded_a.stamp),
        fixture.0.clone(),
        cx,
    );
    visual.update(|_, cx| {
        let list = cx.global_mut::<ProjectList>();
        list.remember(&alpha);
        list.remember(&beta);
    });

    // Open treemacs on alpha first
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let origin = shell.active_view().clone();
            shell.toggle_tree(&origin, window, cx);
        });
    });
    draw(&mut visual);
    // Focus back to ordinary pane
    visual.simulate_keystrokes("ctrl-x o");
    draw(&mut visual);
    visual.read(|cx| {
        let tree = shell
            .read(cx)
            .buffers()
            .find_map(|b| b.tree_root())
            .expect("treemacs open");
        assert_eq!(tree, alpha);
    });

    // C-x p p -> pick beta -> unrecognized key keeps menu open -> 'f' opens beta/b.rs without changing treemacs root
    visual.simulate_keystrokes("ctrl-x p p");
    draw(&mut visual);
    visual.simulate_input("beta");
    draw(&mut visual);
    visual.simulate_keystrokes("enter");
    draw(&mut visual);
    visual.read(|cx| {
        assert!(matches!(
            shell.read(cx).session().unwrap().kind(),
            ReadKind::ProjectSwitchMenu { .. }
        ));
    });
    visual.simulate_keystrokes("z");
    draw(&mut visual);
    visual.read(|cx| {
        assert!(matches!(
            shell.read(cx).session().unwrap().kind(),
            ReadKind::ProjectSwitchMenu { .. }
        ));
    });
    visual.simulate_keystrokes("f");
    draw(&mut visual);
    visual.simulate_keystrokes("enter");
    draw(&mut visual);
    visual.read(|cx| {
        assert_eq!(
            shell.read(cx).active_view().read(cx).buffer().path(),
            Some(b_file.clone())
        );
        // Existing treemacs root stays on alpha when visiting a file in beta
        let tree = shell
            .read(cx)
            .buffers()
            .find_map(|b| b.tree_root())
            .expect("treemacs still open");
        assert_eq!(tree, alpha);
    });

    // C-x p p -> pick beta -> 't' switches treemacs root to beta
    visual.simulate_keystrokes("ctrl-x p p");
    draw(&mut visual);
    visual.simulate_input("beta");
    draw(&mut visual);
    visual.simulate_keystrokes("enter");
    draw(&mut visual);
    visual.simulate_keystrokes("t");
    draw(&mut visual);
    visual.read(|cx| {
        let tree = shell
            .read(cx)
            .buffers()
            .find_map(|b| b.tree_root())
            .expect("treemacs switched");
        assert_eq!(tree, beta);
    });

    // From treemacs side window, C-x p p -> pick alpha -> 'b' switches ordinary pane to a.rs
    visual.simulate_keystrokes("ctrl-x p p");
    draw(&mut visual);
    visual.simulate_input("alpha");
    draw(&mut visual);
    visual.simulate_keystrokes("enter");
    draw(&mut visual);
    visual.simulate_keystrokes("b");
    draw(&mut visual);
    visual.simulate_keystrokes("enter");
    draw(&mut visual);
    visual.read(|cx| {
        assert_eq!(
            shell.read(cx).active_view().read(cx).buffer().path(),
            Some(a_file.clone())
        );
    });

    // C-x p p -> pick beta -> 'v' opens magit-status for beta
    visual.simulate_keystrokes("ctrl-x p p");
    draw(&mut visual);
    visual.simulate_input("beta");
    draw(&mut visual);
    visual.simulate_keystrokes("enter");
    draw(&mut visual);
    visual.simulate_keystrokes("v");
    draw(&mut visual);
    visual.read(|cx| {
        let repo = shell
            .read(cx)
            .active_view()
            .read(cx)
            .buffer()
            .status_repository()
            .expect("magit-status opened for beta");
        assert_eq!(repo.root, beta.canonicalize().unwrap());
    });
}

#[gpui::test]
fn project_switch_to_buffer_and_kill_buffers_scope_to_current_project(cx: &mut TestAppContext) {
    let fixture = Fixture::new("buffers");
    let alpha = fixture.git_repo("alpha");
    let a1 = fixture.file("alpha/a1.rs", "one\n");
    let a2 = fixture.file("alpha/a2.rs", "two\n");
    fixture.commit_all(&alpha);

    let beta = fixture.git_repo("beta");
    let b1 = fixture.file("beta/b1.rs", "beta\n");
    fixture.commit_all(&beta);

    let loaded_a1 = crate::files::load(&a1).unwrap();
    let (mut visual, shell) = open(
        Buffer::visiting(a1.clone(), loaded_a1.file, loaded_a1.stamp),
        fixture.0.clone(),
        cx,
    );
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let l2 = crate::files::load(&a2).unwrap();
            let i2 =
                shell.create_buffer(Buffer::visiting(a2.clone(), l2.file, l2.stamp), window, cx);
            let lb = crate::files::load(&b1).unwrap();
            let _ib =
                shell.create_buffer(Buffer::visiting(b1.clone(), lb.file, lb.stamp), window, cx);
            shell.activate(i2, window, cx);
        });
    });
    draw(&mut visual);

    // C-x p b lists only alpha's other buffer (a1.rs), not b1.rs
    visual.simulate_keystrokes("ctrl-x p b");
    draw(&mut visual);
    visual.read(|cx| {
        let labels: Vec<_> = shell
            .read(cx)
            .session()
            .unwrap()
            .candidates()
            .unwrap()
            .items
            .iter()
            .map(|i| i.candidate.label.as_str())
            .collect();
        assert_eq!(labels, vec!["a1.rs"]);
    });
    visual.simulate_keystrokes("enter");
    draw(&mut visual);
    visual.read(|cx| {
        assert_eq!(
            shell.read(cx).active_view().read(cx).buffer().path(),
            Some(a1.clone())
        );
    });

    // Make a1.rs dirty, then C-x p k -> confirm yes -> confirm y for dirty a1.rs
    visual.simulate_input("dirty ");
    draw(&mut visual);
    visual.simulate_keystrokes("ctrl-x p k");
    draw(&mut visual);
    visual.read(|cx| {
        assert!(matches!(
            shell.read(cx).session().unwrap().kind(),
            ReadKind::ProjectKillBuffersConfirm { .. }
        ));
    });
    visual.simulate_input("yes");
    visual.simulate_keystrokes("enter");
    draw(&mut visual);
    // Dirty buffer a1.rs should now ask for confirmation
    visual.read(|cx| {
        assert!(matches!(
            shell.read(cx).session().unwrap().kind(),
            ReadKind::KillBufferConfirm { .. }
        ));
    });
    visual.simulate_keystrokes("y");
    draw(&mut visual);

    visual.read(|cx| {
        let paths: Vec<_> = shell.read(cx).buffers().filter_map(|b| b.path()).collect();
        assert!(!paths.contains(&a1));
        assert!(!paths.contains(&a2));
        assert!(paths.contains(&b1));
    });
}

#[gpui::test]
fn project_management_commands_remember_forget_prune_and_scan_under(cx: &mut TestAppContext) {
    let fixture = Fixture::new("manage");
    let workspace = fixture.dir("ws");
    let r1 = fixture.git_repo("ws/r1");
    let r2 = fixture.git_repo("ws/r2");
    let zombie = fixture.git_repo("zombie");

    let (mut visual, shell) = open(Buffer::scratch(), fixture.0.clone(), cx);
    visual.update(|_, cx| {
        cx.global_mut::<ProjectList>().remember(&zombie);
    });
    fs::remove_dir_all(&zombie).unwrap();

    // project-forget-zombie-projects
    visual.simulate_keystrokes("alt-x");
    visual.simulate_input("project-forget-zombie-projects");
    visual.simulate_keystrokes("enter");
    draw(&mut visual);
    visual.read(|cx| {
        assert!(cx.global::<ProjectList>().entries().is_empty());
    });
    assert_eq!(echo(&mut visual, &shell), "Forgot 1 zombie project");

    // project-remember-projects-under
    visual.simulate_keystrokes("alt-x");
    visual.simulate_input("project-remember-projects-under");
    visual.simulate_keystrokes("enter");
    draw(&mut visual);
    visual.simulate_keystrokes("ctrl-a ctrl-k");
    visual.simulate_input(&files::directory_input(&workspace));
    draw(&mut visual);
    visual.simulate_keystrokes("enter");
    draw(&mut visual);
    visual.read(|cx| {
        let list = cx.global::<ProjectList>();
        assert!(list.contains(&r1));
        assert!(list.contains(&r2));
    });
    assert_eq!(echo(&mut visual, &shell), "2 projects were found");

    // project-forget-project
    visual.simulate_keystrokes("alt-x");
    visual.simulate_input("project-forget-project");
    visual.simulate_keystrokes("enter");
    draw(&mut visual);
    visual.simulate_input("r1");
    draw(&mut visual);
    visual.simulate_keystrokes("enter");
    draw(&mut visual);
    visual.read(|cx| {
        let list = cx.global::<ProjectList>();
        assert!(!list.contains(&r1));
        assert!(list.contains(&r2));
    });
}

#[gpui::test]
fn project_kill_buffers_steps_through_multiple_dirty_buffers_and_supports_cancel(
    cx: &mut TestAppContext,
) {
    let fixture = Fixture::new("multi-dirty");
    let repo = fixture.git_repo("alpha");
    let f1 = fixture.file("alpha/f1.rs", "one\n");
    let f2 = fixture.file("alpha/f2.rs", "two\n");
    let f3 = fixture.file("alpha/f3.rs", "three\n");
    fixture.commit_all(&repo);

    let l1 = crate::files::load(&f1).unwrap();
    let (mut visual, shell) = open(
        Buffer::visiting(f1.clone(), l1.file, l1.stamp),
        fixture.0.clone(),
        cx,
    );
    visual.simulate_input("edit1 ");
    draw(&mut visual);
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let l2 = crate::files::load(&f2).unwrap();
            let i2 =
                shell.create_buffer(Buffer::visiting(f2.clone(), l2.file, l2.stamp), window, cx);
            let l3 = crate::files::load(&f3).unwrap();
            let _i3 =
                shell.create_buffer(Buffer::visiting(f3.clone(), l3.file, l3.stamp), window, cx);
            shell.activate(i2, window, cx);
        });
    });
    draw(&mut visual);
    visual.simulate_input("edit2 ");
    draw(&mut visual);

    // Start C-x p k, confirm yes -> f2 (dirty): press 'n' to skip -> f1 (dirty): press 's' to save & kill -> f3 (clean) killed
    visual.simulate_keystrokes("ctrl-x p k");
    draw(&mut visual);
    visual.simulate_input("yes");
    visual.simulate_keystrokes("enter");
    draw(&mut visual);
    visual.read(|cx| {
        assert!(matches!(
            shell.read(cx).session().unwrap().kind(),
            ReadKind::KillBufferConfirm { name, batch: true } if name == "f2.rs"
        ));
    });
    visual.simulate_keystrokes("n");
    draw(&mut visual);
    visual.read(|cx| {
        assert!(matches!(
            shell.read(cx).session().unwrap().kind(),
            ReadKind::KillBufferConfirm { name, batch: true } if name == "f1.rs"
        ));
    });
    visual.simulate_keystrokes("s");
    draw(&mut visual);

    visual.read(|cx| {
        let paths: Vec<_> = shell.read(cx).buffers().filter_map(|b| b.path()).collect();
        assert!(paths.contains(&f2), "skipped dirty buffer should remain");
        assert!(!paths.contains(&f1), "saved dirty buffer should be killed");
        assert!(!paths.contains(&f3), "clean buffer should be killed");
    });
    assert_eq!(fs::read_to_string(&f1).unwrap(), "edit1 one\n");
}

#[gpui::test]
fn project_find_file_allows_creating_new_file_with_raw_input(cx: &mut TestAppContext) {
    let fixture = Fixture::new("new-file");
    let repo = fixture.git_repo("alpha");
    let existing = fixture.file("alpha/existing.rs", "ok\n");
    fixture.commit_all(&repo);

    let loaded = crate::files::load(&existing).unwrap();
    let (mut visual, shell) = open(
        Buffer::visiting(existing, loaded.file, loaded.stamp),
        fixture.0.clone(),
        cx,
    );

    visual.simulate_keystrokes("ctrl-x p f");
    draw(&mut visual);
    visual.simulate_input("src/fresh.rs");
    draw(&mut visual);
    visual.simulate_keystrokes("alt-enter");
    draw(&mut visual);

    visual.read(|cx| {
        assert_eq!(
            shell.read(cx).active_view().read(cx).buffer().path(),
            Some(repo.join("src/fresh.rs"))
        );
    });
}

#[gpui::test]
fn project_remember_rejects_transient_and_select_empty_uses_default(cx: &mut TestAppContext) {
    let fixture = Fixture::new("remember-default");
    let plain = fixture.dir("plain");
    let repo = fixture.git_repo("alpha");
    let file = fixture.file("alpha/a.rs", "a\n");
    fixture.commit_all(&repo);

    // 1. M-x project-remember-project on a plain directory does not persist it
    let (mut visual, shell) = open(Buffer::scratch(), fixture.0.clone(), cx);
    visual.simulate_keystrokes("alt-x");
    visual.simulate_input("project-remember-project");
    visual.simulate_keystrokes("enter");
    draw(&mut visual);
    // Accept `... (choose a dir)`
    visual.simulate_keystrokes("enter");
    draw(&mut visual);
    visual.simulate_keystrokes("ctrl-a ctrl-k");
    visual.simulate_input(&files::directory_input(&plain));
    draw(&mut visual);
    visual.simulate_keystrokes("enter");
    draw(&mut visual);
    visual.read(|cx| {
        assert!(!cx.global::<ProjectList>().contains(&plain));
    });
    assert!(echo(&mut visual, &shell).starts_with("No project found in"));

    // 2. Empty raw input in ProjectSelect falls back to default current project
    let loaded = crate::files::load(&file).unwrap();
    let (mut visual, shell) = open(
        Buffer::visiting(file, loaded.file, loaded.stamp),
        fixture.0.clone(),
        cx,
    );
    visual.update(|_, cx| {
        cx.global_mut::<ProjectList>().remember(&repo);
    });
    visual.simulate_keystrokes("ctrl-x p p");
    draw(&mut visual);
    visual.simulate_keystrokes("alt-enter");
    draw(&mut visual);
    visual.read(|cx| {
        assert!(matches!(
            shell.read(cx).session().unwrap().kind(),
            ReadKind::ProjectSwitchMenu { project } if project.root == repo
        ));
    });
}

#[gpui::test]
fn canceling_or_removing_gated_batch_kill_buffer_does_not_leave_dangling_queue(
    cx: &mut TestAppContext,
) {
    let fixture = Fixture::new("batch-dangle");
    let alpha = fixture.git_repo("alpha");
    let a1 = fixture.file("alpha/a1.rs", "one\n");
    let a2 = fixture.file("alpha/a2.rs", "two\n");
    fixture.commit_all(&alpha);
    let beta = fixture.git_repo("beta");
    let b1 = fixture.file("beta/b1.rs", "beta\n");
    fixture.commit_all(&beta);

    let l1 = crate::files::load(&a1).unwrap();
    let (mut visual, shell) = open(
        Buffer::visiting(a1.clone(), l1.file, l1.stamp),
        fixture.0.clone(),
        cx,
    );
    visual.simulate_input("dirty1 ");
    draw(&mut visual);
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let l2 = crate::files::load(&a2).unwrap();
            let _i2 =
                shell.create_buffer(Buffer::visiting(a2.clone(), l2.file, l2.stamp), window, cx);
            let lb = crate::files::load(&b1).unwrap();
            let ib =
                shell.create_buffer(Buffer::visiting(b1.clone(), lb.file, lb.stamp), window, cx);
            shell.activate(ib, window, cx);
        });
    });
    draw(&mut visual);
    visual.simulate_input("dirty_b ");
    draw(&mut visual);
    // Switch back to a1 (index 0) and gate saves
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.activate(0, window, cx);
        });
    });
    draw(&mut visual);
    let (tx, rx) = futures_channel::oneshot::channel();
    shell.update(&mut visual, |shell, _| shell.save_gate = Some(rx));

    // Start C-x p k -> yes -> press 's' on dirty a1 (save paused by gate)
    visual.simulate_keystrokes("ctrl-x p k");
    draw(&mut visual);
    visual.simulate_input("yes");
    visual.simulate_keystrokes("enter");
    draw(&mut visual);
    visual.simulate_keystrokes("s");
    draw(&mut visual);

    // While a1 is saving, kill a1 out of band with C-x C-k -> y
    visual.simulate_keystrokes("ctrl-x ctrl-k");
    draw(&mut visual);
    visual.simulate_keystrokes("y");
    draw(&mut visual);
    let _ = tx.send(());
    draw(&mut visual);

    // Now switch to unrelated dirty b1, run C-x C-k -> n (cancel).
    // a2 must NOT be killed by a dangling batch queue.
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let idx = shell
                .buffers()
                .position(|b| b.path().as_deref() == Some(b1.as_path()))
                .unwrap();
            shell.activate(idx, window, cx);
        });
    });
    draw(&mut visual);
    visual.simulate_keystrokes("ctrl-x ctrl-k");
    draw(&mut visual);
    visual.simulate_keystrokes("n");
    draw(&mut visual);

    visual.read(|cx| {
        let paths: Vec<_> = shell.read(cx).buffers().filter_map(|b| b.path()).collect();
        assert!(paths.contains(&a2), "dangling batch queue must not kill a2");
        assert!(paths.contains(&b1), "canceled kill must keep b1");
    });
    assert_eq!(echo(&mut visual, &shell), "Canceled");
}
