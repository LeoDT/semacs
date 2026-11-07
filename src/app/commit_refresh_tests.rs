use super::*;
use crate::app::git_test_support::{command, gate, open, repo, until, wait};
use crate::command::Command;
use std::os::unix::process::ExitStatusExt;

fn terminal(
    shell: &mut AppShell,
    repository: Repository,
    window: &mut gpui_kit::Window,
    cx: &mut gpui_kit::Context<AppShell>,
) {
    let target = EditorTarget::capture(shell, cx);
    let operation = operation_id();
    shell
        .commits
        .push(session(operation, repository, true, target));
    shell.commit_terminal(
        operation,
        CommitPhase::Exited(ExitStatus::from_raw(0)),
        window,
        cx,
    );
}

#[gpui::test]
fn terminal_refreshes_command_diff_and_status_not_other_diffs_or_focus(cx: &mut TestAppContext) {
    let fixture = repo();
    let repository = Repository {
        root: fixture.0.clone(),
        git_dir: fixture.0.join(".git"),
    };
    let (mut visual, shell) = open(&fixture, cx);
    command(&mut visual, &shell, Command::MagitStatus);
    wait(&mut visual, &shell);
    let status = visual.read(|cx| shell.read(cx).active_view().read(cx).buffer().new_view());
    command(&mut visual, &shell, Command::MagitDiffUnstaged);
    wait(&mut visual, &shell);
    let diff = visual.read(|cx| shell.read(cx).active_view().read(cx).buffer().new_view());
    let target = visual.read(|cx| EditorTarget::capture(shell.read(cx), cx));
    command(&mut visual, &shell, Command::MagitDiffStaged);
    wait(&mut visual, &shell);
    let other = visual.read(|cx| shell.read(cx).active_view().read(cx).buffer().new_view());
    let old_other = other.text().to_string();
    fixture.commit();
    visual.simulate_keystrokes("alt-x");
    let read = visual.read(|cx| shell.read(cx).session().unwrap().id());
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell
                .commits
                .push(session(1, repository.clone(), true, target));
            shell.commit_terminal(1, CommitPhase::Exited(ExitStatus::from_raw(0)), window, cx);
        })
    });
    wait(&mut visual, &shell);
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert_eq!(shell.session().unwrap().id(), read);
        assert!(!status.git_is_stale());
        assert!(!diff.git_is_stale());
        assert!(other.git_is_stale());
        assert_eq!(other.text().to_string(), old_other);
        assert!(!diff.text().to_string().contains("+disk changed"));
        assert!(status.status_data().unwrap().entries().is_empty());
        assert!(matches!(shell.commit_records().last().unwrap().phase, CommitPhase::Exited(status) if status.success()));
    });
}

#[gpui::test]
fn terminal_waits_for_shared_read_slot_and_invalidates_its_old_result(cx: &mut TestAppContext) {
    let fixture = repo();
    let repository = Repository {
        root: fixture.0.clone(),
        git_dir: fixture.0.join(".git"),
    };
    let (mut visual, shell) = open(&fixture, cx);
    command(&mut visual, &shell, Command::MagitStatus);
    wait(&mut visual, &shell);
    let release = gate(&mut visual, &shell);
    command(&mut visual, &shell, Command::MagitRefresh);
    until(&mut visual, &shell, |shell| {
        shell.status_read_gate.is_none()
    });
    let operation = visual.read(|cx| shell.read(cx).git_task.as_ref().unwrap().operation());
    fixture.commit();
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            terminal(shell, repository, window, cx);
            assert_eq!(shell.git_task.as_ref().unwrap().operation(), operation);
            assert_eq!(shell.git_refreshes.len(), 1);
            assert!(shell.active_view().read(cx).buffer().git_is_stale());
        })
    });
    release.send(()).unwrap();
    wait(&mut visual, &shell);
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert!(shell.git_refreshes.is_empty());
        assert!(!shell.active_view().read(cx).buffer().git_is_stale());
        assert!(
            shell
                .active_view()
                .read(cx)
                .buffer()
                .status_data()
                .unwrap()
                .entries()
                .is_empty()
        );
    });
}

#[gpui::test]
fn terminal_refresh_cannot_install_into_a_recreated_status(cx: &mut TestAppContext) {
    let fixture = repo();
    let repository = Repository {
        root: fixture.0.clone(),
        git_dir: fixture.0.join(".git"),
    };
    let (mut visual, shell) = open(&fixture, cx);
    command(&mut visual, &shell, Command::MagitStatus);
    wait(&mut visual, &shell);
    let (send, receive) = futures_channel::oneshot::channel();
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.repository_refresh_gate = Some(receive);
            terminal(shell, repository.clone(), window, cx);
        })
    });
    until(&mut visual, &shell, |shell| {
        shell.repository_refresh_gate.is_none()
    });
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.remove_buffer(shell.active_index(), window, cx);
            let index = shell.create_buffer(Buffer::status(repository), window, cx);
            shell.activate(index, window, cx);
        })
    });
    send.send(()).unwrap();
    wait(&mut visual, &shell);
    visual.read(|cx| {
        assert!(
            shell
                .read(cx)
                .active_view()
                .read(cx)
                .buffer()
                .status_data()
                .is_none()
        )
    });
}

#[gpui::test]
fn failed_refresh_preserves_snapshot_stale_marker_and_successful_process_result(
    cx: &mut TestAppContext,
) {
    let fixture = repo();
    let repository = Repository {
        root: fixture.0.clone(),
        git_dir: fixture.0.join(".git"),
    };
    let (mut visual, shell) = open(&fixture, cx);
    command(&mut visual, &shell, Command::MagitStatus);
    wait(&mut visual, &shell);
    let old = visual.read(|cx| {
        shell
            .read(cx)
            .active_view()
            .read(cx)
            .buffer()
            .text()
            .to_string()
    });
    std::fs::rename(fixture.0.join(".git"), fixture.0.join("hidden-git")).unwrap();
    visual
        .update(|window, cx| shell.update(cx, |shell, cx| terminal(shell, repository, window, cx)));
    wait(&mut visual, &shell);
    visual.read(|cx| {
        let shell = shell.read(cx);
        let buffer = shell.active_view().read(cx).buffer();
        assert_eq!(buffer.text().to_string(), old);
        assert!(buffer.git_is_stale());
        assert!(shell.messages.buffer.text().to_string().contains("repository refresh failed"));
        assert!(matches!(shell.commit_records().last().unwrap().phase, CommitPhase::Exited(status) if status.success()));
    });
}

#[gpui::test]
fn second_terminal_supersedes_an_inflight_refresh_without_losing_the_new_refresh(
    cx: &mut TestAppContext,
) {
    let fixture = repo();
    let repository = Repository {
        root: fixture.0.clone(),
        git_dir: fixture.0.join(".git"),
    };
    let (mut visual, shell) = open(&fixture, cx);
    command(&mut visual, &shell, Command::MagitStatus);
    wait(&mut visual, &shell);
    let (send, receive) = futures_channel::oneshot::channel();
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.repository_refresh_gate = Some(receive);
            terminal(shell, repository.clone(), window, cx);
        })
    });
    until(&mut visual, &shell, |shell| {
        shell.repository_refresh_gate.is_none()
    });
    fixture.commit();
    visual
        .update(|window, cx| shell.update(cx, |shell, cx| terminal(shell, repository, window, cx)));
    send.send(()).unwrap();
    wait(&mut visual, &shell);
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert!(!shell.active_view().read(cx).buffer().git_is_stale());
        assert!(
            shell
                .active_view()
                .read(cx)
                .buffer()
                .status_data()
                .unwrap()
                .entries()
                .is_empty()
        );
    });
}

#[gpui::test]
fn terminal_refresh_waits_through_an_index_write_without_occupying_its_slot(
    cx: &mut TestAppContext,
) {
    let fixture = repo();
    let repository = Repository {
        root: fixture.0.clone(),
        git_dir: fixture.0.join(".git"),
    };
    let (mut visual, shell) = open(&fixture, cx);
    command(&mut visual, &shell, Command::MagitStatus);
    wait(&mut visual, &shell);
    let (send, receive) = futures_channel::oneshot::channel();
    let write = visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let origin = shell.active_view().clone();
            let point = origin
                .read(cx)
                .buffer()
                .sections()
                .unwrap()
                .nodes()
                .iter()
                .find(|node| node.key.starts_with("unstaged/file/"))
                .unwrap()
                .range
                .start;
            origin.update(cx, |view, cx| view.show_location(point, window, cx));
            shell.index_verify_gate = Some(receive);
            let target = shell.capture_status_write(&origin, cx).unwrap();
            shell
                .accept_status_write(target, crate::status::index::Operation::Stage, window, cx)
                .unwrap()
        })
    });
    until(&mut visual, &shell, |shell| {
        shell
            .git_write_records()
            .any(|r| r.phase == crate::app::GitWritePhase::Verifying)
    });
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            terminal(shell, repository, window, cx);
            assert!(shell.owns_git_write(write));
            assert_eq!(shell.git_refreshes.len(), 1);
        })
    });
    send.send(()).unwrap();
    wait(&mut visual, &shell);
    visual.read(|cx| {
        let shell = shell.read(cx);
        let record = shell.git_write_records().find(|r| r.id == write).unwrap();
        assert_eq!(record.verified, Some(true));
        assert!(!record.installed);
        let buffer = shell.active_view().read(cx).buffer();
        assert!(!buffer.git_is_stale());
        assert_eq!(
            buffer
                .status_data()
                .unwrap()
                .group(crate::status::Group::Staged)
                .count(),
            1
        );
    });
}

#[gpui::test]
fn exporting_status_while_terminal_refresh_is_pending_does_not_restore_its_mode(
    cx: &mut TestAppContext,
) {
    let fixture = repo();
    let repository = Repository {
        root: fixture.0.clone(),
        git_dir: fixture.0.join(".git"),
    };
    let (mut visual, shell) = open(&fixture, cx);
    command(&mut visual, &shell, Command::MagitStatus);
    wait(&mut visual, &shell);
    let (send, receive) = futures_channel::oneshot::channel();
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.repository_refresh_gate = Some(receive);
            terminal(shell, repository, window, cx);
        })
    });
    until(&mut visual, &shell, |shell| {
        shell.repository_refresh_gate.is_none()
    });
    let path = fixture.0.join("status-export");
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let origin = shell.active_view().clone();
            shell.write_file(&origin, path.clone(), window, cx);
        })
    });
    until(&mut visual, &shell, |shell| {
        shell.buffers.iter().all(|slot| !slot.save.is_saving())
    });
    send.send(()).unwrap();
    wait(&mut visual, &shell);
    visual.read(|cx| {
        let buffer = shell.read(cx).active_view().read(cx).buffer();
        assert_eq!(buffer.mode(), ModeId::FUNDAMENTAL);
        assert_eq!(buffer.path().as_ref(), Some(&path));
        assert!(buffer.status_data().is_none());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            buffer.text().to_string()
        );
    });
}

#[gpui::test]
fn a_preterminal_open_cannot_create_a_new_projection_from_old_data(cx: &mut TestAppContext) {
    let fixture = repo();
    let repository = Repository {
        root: fixture.0.clone(),
        git_dir: fixture.0.join(".git"),
    };
    let (mut visual, shell) = open(&fixture, cx);
    let release = gate(&mut visual, &shell);
    command(&mut visual, &shell, Command::MagitStatus);
    until(&mut visual, &shell, |shell| {
        shell.status_read_gate.is_none()
    });
    visual
        .update(|window, cx| shell.update(cx, |shell, cx| terminal(shell, repository, window, cx)));
    release.send(()).unwrap();
    wait(&mut visual, &shell);
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert!(
            shell
                .buffers
                .iter()
                .all(|slot| slot.buffer.status_repository().is_none())
        );
        assert!(
            shell
                .messages
                .buffer
                .text()
                .to_string()
                .contains("Repository changed while reading status")
        );
    });
}
