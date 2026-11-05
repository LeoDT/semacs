use super::*;
use crate::diff::tests::Repository;
use crate::files::{DecodedFile, FileFormat};
use crate::mode::ModeId;
use gpui_kit as gpui;
use gpui_kit::{TestAppContext, VisualTestContext, px, size};
use std::time::{Duration, Instant};

#[path = "diff_review_tests.rs"]
mod review;
#[path = "diff_state_tests.rs"]
mod state;

fn open(repo: &Repository, cx: &mut TestAppContext) -> (VisualTestContext, Entity<AppShell>) {
    cx.background_executor.allow_parking();
    cx.update(crate::app::init);
    let buffer = Buffer::new(
        Some(repo.0.join("a")),
        DecodedFile {
            text: "old\n".into(),
            format: FileFormat::default(),
        },
    );
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
fn command(visual: &mut VisualTestContext, shell: &Entity<AppShell>, command: Command) {
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let origin = shell.active_view().clone();
            shell.run_command(command.into(), &origin, window, cx);
        })
    });
}
fn wait(visual: &mut VisualTestContext, shell: &Entity<AppShell>) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        if visual.read(|cx| {
            let shell = shell.read(cx);
            shell.git_task.is_none() && shell.file_task.is_none() && shell.opening.is_none()
        }) {
            break;
        }
        assert!(Instant::now() < deadline, "Git/file read did not complete");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[gpui::test]
fn commands_open_refresh_and_visit_index_snapshots_not_worktree(cx: &mut TestAppContext) {
    let repo = Repository::new();
    repo.write("a", "old\n");
    repo.commit();
    repo.write("a", "indexed\n");
    repo.git(&["add", "a"]);
    repo.write("a", "worktree\n");
    let (mut visual, shell) = open(&repo, cx);
    command(&mut visual, &shell, Command::MagitDiffStaged);
    wait(&mut visual, &shell);
    let diff = visual.read(|cx| shell.read(cx).active_view().clone());
    visual.read(|cx| {
        assert_eq!(diff.read(cx).buffer().mode(), ModeId::MAGIT_DIFF);
        assert!(
            diff.read(cx)
                .buffer()
                .text()
                .to_string()
                .contains("+indexed")
        );
    });
    visual.update(|window, cx| {
        diff.update(cx, |view, cx| {
            let offset = view.buffer().text().to_string().find("+indexed").unwrap();
            view.show_location(crate::text::ByteOffset(offset + 2), window, cx);
        })
    });
    command(&mut visual, &shell, Command::MagitDiffVisit);
    wait(&mut visual, &shell);
    visual.read(|cx| {
        let view = shell.read(cx).active_view().read(cx);
        assert_eq!(view.buffer().text().to_string(), "indexed\n");
        assert!(view.buffer().is_read_only());
        assert_eq!(view.buffer().point(), crate::text::ByteOffset(1));
    });
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let index = shell.buffer_index(&diff, cx).unwrap();
            shell.activate(index, window, cx);
        })
    });
    repo.write("a", "indexed2\n");
    repo.git(&["add", "a"]);
    command(&mut visual, &shell, Command::MagitRefresh);
    wait(&mut visual, &shell);
    visual.read(|cx| {
        assert_eq!(shell.read(cx).active_view(), &diff);
        assert!(
            diff.read(cx)
                .buffer()
                .text()
                .to_string()
                .contains("+indexed2")
        );
    });
    command(&mut visual, &shell, Command::MagitDiffUnstaged);
    wait(&mut visual, &shell);
    visual.read(|cx| {
        assert_eq!(
            shell
                .read(cx)
                .active_view()
                .read(cx)
                .buffer()
                .comparison()
                .unwrap()
                .kind,
            DiffKind::Unstaged
        )
    });
}

#[gpui::test]
fn unavailable_commands_and_contextual_mx_do_not_change_fundamental_buffers(
    cx: &mut TestAppContext,
) {
    let repo = Repository::new();
    repo.write("a", "old\n");
    repo.commit();
    let (mut visual, shell) = open(&repo, cx);
    command(&mut visual, &shell, Command::SectionToggle);
    visual.read(|cx| {
        assert!(
            shell
                .read(cx)
                .active_view()
                .read(cx)
                .message()
                .unwrap()
                .contains("unavailable")
        )
    });
    command(&mut visual, &shell, Command::ExecuteExtendedCommand);
    visual.read(|cx| {
        let shell = shell.read(cx);
        let candidates = shell.session.as_ref().unwrap().candidates().unwrap();
        assert!(
            !candidates
                .items
                .iter()
                .any(|item| item.candidate.label == "section-toggle")
        );
    });
}

#[gpui::test]
fn late_open_does_not_steal_focus_and_killed_refresh_cannot_publish(cx: &mut TestAppContext) {
    let repo = Repository::new();
    repo.write("a", "old\n");
    repo.commit();
    repo.write("a", "new\n");
    let (mut visual, shell) = open(&repo, cx);
    command(&mut visual, &shell, Command::MagitDiffUnstaged);
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let index = shell.create_buffer(Buffer::named("other"), window, cx);
            shell.activate(index, window, cx);
        })
    });
    wait(&mut visual, &shell);
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert_eq!(shell.active_view().read(cx).buffer().base_name(), "other");
        assert!(
            !shell
                .buffers
                .iter()
                .any(|slot| slot.buffer.mode() == ModeId::MAGIT_DIFF)
        );
    });
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.activate(0, window, cx);
        })
    });
    command(&mut visual, &shell, Command::MagitDiffUnstaged);
    wait(&mut visual, &shell);
    command(&mut visual, &shell, Command::MagitRefresh);
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let index = shell.active_index();
            shell.remove_buffer(index, window, cx);
        })
    });
    wait(&mut visual, &shell);
    visual.read(|cx| {
        assert!(
            !shell
                .read(cx)
                .buffers
                .iter()
                .any(|slot| slot.buffer.mode() == ModeId::MAGIT_DIFF)
        )
    });
}
