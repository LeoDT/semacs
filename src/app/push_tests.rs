use super::*;
use crate::app::git_test_support::until;
use crate::buffer::Buffer;
use crate::command::Command;
use crate::mode::ModeId;
use gpui_kit as gpui;
use gpui_kit::{ClipboardItem, TestAppContext, VisualTestContext, px, size};
use std::path::PathBuf;
use std::process::Command as ProcessCommand;
use std::sync::atomic::{AtomicU64, Ordering};

struct PushFixture {
    root: PathBuf,
    bare: PathBuf,
}

impl PushFixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let base =
            std::env::temp_dir().join(format!("semacs-push-app-{}-{id}", std::process::id()));
        let root = base.join("work");
        let bare = base.join("remote.git");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&bare).unwrap();
        run_git(&bare, &["init", "--bare", "-q"]);
        run_git(&root, &["init", "-q", "-b", "main"]);
        run_git(&root, &["config", "user.email", "a@b.c"]);
        run_git(&root, &["config", "user.name", "Tester"]);
        run_git(&root, &["remote", "add", "origin", bare.to_str().unwrap()]);
        std::fs::write(root.join("a.txt"), "hello\n").unwrap();
        run_git(&root, &["add", "a.txt"]);
        run_git(&root, &["commit", "-qm", "init"]);
        Self { root, bare }
    }
}

impl Drop for PushFixture {
    fn drop(&mut self) {
        if let Some(parent) = self.root.parent() {
            let _ = std::fs::remove_dir_all(parent);
        }
    }
}

fn run_git(dir: &std::path::Path, args: &[&str]) -> String {
    let output = ProcessCommand::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn open_repo(
    fixture: &PushFixture,
    cx: &mut TestAppContext,
) -> (VisualTestContext, Entity<AppShell>) {
    cx.background_executor.allow_parking();
    cx.update(crate::app::init);
    let buffer = Buffer::new_file(fixture.root.join("a.txt"));
    let window = cx.open_window(size(px(960.), px(640.)), |window, cx| {
        AppShell::new(buffer, window, cx)
    });
    let shell = window.entity(cx).unwrap();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.update(|window, cx| {
        window.activate_window();
        window.draw(cx).clear(cx);
    });
    (visual, shell)
}

#[gpui::test]
fn push_menu_handles_pushremote_upstream_and_elsewhere_end_to_end(cx: &mut TestAppContext) {
    let fixture = PushFixture::new();
    let (mut visual, shell) = open_repo(&fixture, cx);

    // Open status and invoke `P` (`magit-push`).
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let origin = shell.active_view().clone();
            shell.run_command(Command::MagitStatus.into(), &origin, window, cx);
        });
    });
    until(&mut visual, &shell, |shell| shell.git_task.is_none());
    visual.read(|cx| {
        assert_eq!(
            shell.read(cx).active_view().read(cx).buffer().mode(),
            ModeId::MAGIT_STATUS
        );
    });

    // `P p` when pushRemote is unset prompts to set `branch.main.pushRemote` and pushes.
    visual.simulate_keystrokes("P");
    until(&mut visual, &shell, |shell| shell.session().is_some());
    visual.read(|cx| {
        assert!(matches!(
            shell.read(cx).session().unwrap().kind(),
            ReadKind::PushMenu { .. }
        ));
    });
    visual.simulate_keystrokes("p");
    visual.read(|cx| {
        assert!(matches!(
            shell.read(cx).session().unwrap().kind(),
            ReadKind::PushSetRemote { .. }
        ));
    });
    visual.simulate_keystrokes("enter");
    until(&mut visual, &shell, |shell| {
        shell.git_task.is_none()
            && shell.session().is_none()
            && shell
                .commit_records()
                .any(|record| !record.phase.is_active())
    });

    let head = run_git(&fixture.root, &["rev-parse", "HEAD"]);
    assert_eq!(
        run_git(&fixture.bare, &["rev-parse", "refs/heads/main"]),
        head
    );
    assert_eq!(
        run_git(
            &fixture.root,
            &["config", "--get", "branch.main.pushRemote"]
        ),
        "origin"
    );

    // Verify `$` (`magit-process`) displays `git push -v ...`.
    visual.simulate_keystrokes("$");
    visual.read(|cx| {
        let text = shell
            .read(cx)
            .active_view()
            .read(cx)
            .buffer()
            .text()
            .to_string();
        assert!(text.contains("git push -v origin refs/heads/main:refs/heads/main"));
    });
    visual.simulate_keystrokes("q");

    // New commit: `P u` with unset upstream prompts `PushSetUpstream`, sets upstream and pushes.
    std::fs::write(fixture.root.join("a.txt"), "second\n").unwrap();
    run_git(&fixture.root, &["commit", "-am", "second"]);
    visual.simulate_keystrokes("P");
    until(&mut visual, &shell, |shell| shell.session().is_some());
    visual.simulate_keystrokes("u");
    visual.read(|cx| {
        assert!(matches!(
            shell.read(cx).session().unwrap().kind(),
            ReadKind::PushSetUpstream { .. }
        ));
    });
    visual.simulate_keystrokes("enter");
    until(&mut visual, &shell, |shell| {
        shell.git_task.is_none()
            && shell.commit_records().count() == 2
            && shell
                .commit_records()
                .all(|record| !record.phase.is_active())
    });
    let head2 = run_git(&fixture.root, &["rev-parse", "HEAD"]);
    assert_eq!(
        run_git(&fixture.bare, &["rev-parse", "refs/heads/main"]),
        head2
    );
    assert_eq!(
        run_git(
            &fixture.root,
            &["rev-parse", "--abbrev-ref", "main@{upstream}"]
        ),
        "origin/main"
    );

    // `P e` pushes to an arbitrary branch (`origin/topic`).
    visual.simulate_keystrokes("P");
    until(&mut visual, &shell, |shell| shell.session().is_some());
    visual.simulate_keystrokes("e");
    visual.read(|cx| {
        assert!(matches!(
            shell.read(cx).session().unwrap().kind(),
            ReadKind::PushElsewhere { .. }
        ));
    });
    visual.simulate_input("origin/topic");
    visual.simulate_keystrokes("alt-enter");
    until(&mut visual, &shell, |shell| {
        shell.commit_records().count() == 3
            && shell
                .commit_records()
                .all(|record| !record.phase.is_active())
    });
    assert_eq!(
        run_git(&fixture.bare, &["rev-parse", "refs/heads/topic"]),
        head2
    );

    // `P p` now pushes immediately without prompting since `branch.main.pushRemote` is set.
    std::fs::write(fixture.root.join("a.txt"), "third\n").unwrap();
    run_git(&fixture.root, &["commit", "-am", "third"]);
    visual.simulate_keystrokes("P");
    until(&mut visual, &shell, |shell| shell.session().is_some());
    visual.simulate_keystrokes("p");
    until(&mut visual, &shell, |shell| {
        shell.commit_records().count() == 4
            && shell
                .commit_records()
                .all(|record| !record.phase.is_active())
    });
    let head3 = run_git(&fixture.root, &["rev-parse", "HEAD"]);
    assert_eq!(
        run_git(&fixture.bare, &["rev-parse", "refs/heads/main"]),
        head3
    );
}

#[gpui::test]
fn confidential_passphrase_masks_display_and_isolates_clipboard_and_history(
    cx: &mut TestAppContext,
) {
    use crate::editing::KillRing;
    use crate::platform::BufferInputHandler;
    use gpui_kit::InputHandler;

    cx.background_executor.allow_parking();
    cx.update(crate::app::init);
    cx.update(|cx| {
        cx.write_to_clipboard(ClipboardItem::new_string("public-clipboard".into()));
    });
    let window = cx.open_window(size(px(960.), px(640.)), |window, cx| {
        AppShell::new(Buffer::named("origin"), window, cx)
    });
    let shell = window.entity(cx).unwrap();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.update(|window, cx| {
        window.activate_window();
        window.draw(cx).clear(cx);
        shell.update(cx, |shell, cx| {
            let origin = shell.active_view().clone();
            shell.begin_read(
                ReadKind::CommitPassword {
                    operation: 77,
                    request: 88,
                    prompt: "Enter passphrase for key '/tmp/k': ".into(),
                },
                &origin,
                "",
                window,
                cx,
            );
        });
    });
    visual.simulate_input("s3cr3t中文");
    visual.update(|window, cx| window.draw(cx).clear(cx));

    // Secret view masks input, hides text from platform InputHandler, and preserves global clipboard/kill-ring.
    visual.update(|window, cx| {
        let editor = shell
            .read(cx)
            .minibuffer()
            .unwrap()
            .read(cx)
            .editor()
            .clone();
        assert!(editor.read(cx).is_secret());
        let layout = editor.read(cx).viewport_layout().unwrap();
        assert_eq!(layout.visible_row_count(), 1);
        let mut handler: BufferInputHandler = editor.update(cx, |v, cx| v.test_input_handler(cx));
        let mut adjusted = None;
        assert_eq!(
            handler.text_for_range(0..10, &mut adjusted, window, cx),
            None
        );
    });

    // Kill/copy inside secret minibuffer must NOT leak to system clipboard or global KillRing.
    visual.simulate_keystrokes("ctrl-a ctrl-k");
    visual.read(|cx| {
        assert_eq!(
            cx.read_from_clipboard().and_then(|item| item.text()),
            Some("public-clipboard".to_owned())
        );
        assert!(cx.global::<KillRing>().current_kill(None).is_none());
    });

    // `C-y` can paste from the system clipboard into the secret prompt.
    visual.simulate_keystrokes("ctrl-y");
    visual.read(|cx| {
        assert_eq!(
            shell.read(cx).session_input(cx).as_deref(),
            Some("public-clipboard")
        );
    });
    visual.simulate_keystrokes("ctrl-g");
    visual.run_until_parked();
    visual.read(|cx| {
        assert!(shell.read(cx).session().is_none());
        assert_eq!(shell.read(cx).echo_text(), "Quit");
    });
}
