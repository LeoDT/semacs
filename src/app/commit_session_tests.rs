use super::*;

#[path = "commit_exit_tests.rs"]
mod exit;
#[path = "commit_prompt_tests.rs"]
mod prompts;
#[path = "commit_refresh_tests.rs"]
mod refresh;
use crate::app::commit_editor::EditorTarget;
use crate::buffer::Buffer;
use crate::diff::{Comparison, DiffKind};
use crate::mode::ModeId;
use gpui_kit as gpui;
use gpui_kit::{TestAppContext, VisualTestContext, px, size};

fn repository(name: &str) -> Repository {
    Repository {
        root: PathBuf::from(name),
        git_dir: PathBuf::from(name).join(".git"),
    }
}

fn session(operation: u64, repository: Repository, active: bool, target: EditorTarget) -> Session {
    Session {
        scope: crate::commit::Scope::Staged,
        intent: crate::commit::Intent::Create,
        record: CommitRecord {
            operation,
            repository,
            phase: if active {
                CommitPhase::Running
            } else {
                CommitPhase::Failed {
                    started: false,
                    diagnostic: "test".into(),
                }
            },
            output: Vec::new(),
            diagnostics: Vec::new(),
        },
        target,
        process: None,
        interrupted: false,
        exit_termination_requested: false,
        interaction: Default::default(),
        _task: Task::ready(()),
    }
}

#[gpui::test]
fn history_evicts_old_inactive_sections_only_in_the_matching_repository(cx: &mut TestAppContext) {
    cx.update(crate::app::init);
    let window = cx.open_window(size(px(960.), px(640.)), |window, cx| {
        AppShell::new(Buffer::named("origin"), window, cx)
    });
    let shell = window.entity(cx).unwrap();
    let repo = repository("/test-repo");
    cx.update(|cx| {
        shell.update(cx, |shell, cx| {
            let target = EditorTarget::capture(shell, cx);
            shell.commits = (0..32)
                .map(|id| session(id, repo.clone(), id == 0, target.clone()))
                .collect();
            shell
                .commits
                .push(session(32, repository("/other"), false, target.clone()));
            shell.prune_commit_history(&repo);
            let ids: Vec<_> = shell
                .commit_records()
                .map(|record| record.operation)
                .collect();
            assert_eq!(ids, [vec![0], (17..33).collect()].concat());
            shell.commits = (0..32)
                .map(|id| session(id, repo.clone(), true, target.clone()))
                .collect();
            shell.prune_commit_history(&repo);
            assert_eq!(shell.commits.len(), 32);
        })
    });
}

#[gpui::test]
fn terminal_fences_late_loads_and_releases_only_its_owned_requests(cx: &mut TestAppContext) {
    use crate::app::git_test_support::{command, until, wait};
    use crate::command::Command;
    use crate::commit::EditorRequest;
    use std::io::Read;
    use std::os::unix::process::ExitStatusExt;

    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".pi/tmp")
        .join(format!(
            "commit-session-message-{}-{}",
            std::process::id(),
            operation_id()
        ));
    std::fs::write(&path, "draft\n").unwrap();
    cx.background_executor.allow_parking();
    cx.update(crate::app::init);
    let window = cx.open_window(size(px(960.), px(640.)), |window, cx| {
        AppShell::new(Buffer::named("origin"), window, cx)
    });
    let shell = window.entity(cx).unwrap();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.update(|window, cx| {
        window.activate_window();
        window.draw(cx).clear(cx);
    });
    let (request, mut peer) = EditorRequest::test_pair(path.clone());
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let target = EditorTarget::capture(shell, cx);
            shell
                .commits
                .push(session(1, repository("/repo"), true, target.clone()));
            shell.edit_owned_commit_message(request, "#".into(), Some(1), target, window, cx);
            shell.commit_terminal(
                1,
                CommitPhase::Exited(ExitStatus::from_raw(256)),
                window,
                cx,
            );
        })
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        visual.run_until_parked();
        if matches!(peer.read(&mut [0]), Ok(0)) {
            break;
        }
        assert!(std::time::Instant::now() < deadline);
    }
    visual.read(|cx| assert!(shell.read(cx).commit_editors.is_empty()));

    let (request, mut owned_peer) = EditorRequest::test_pair(path.clone());
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let target = EditorTarget::capture(shell, cx);
            shell
                .commits
                .push(session(2, repository("/repo"), true, target.clone()));
            shell.edit_owned_commit_message(request, "#".into(), Some(2), target, window, cx);
        })
    });
    until(&mut visual, &shell, |shell| {
        !shell.commit_editors.is_empty()
    });
    let (other, mut other_peer) = EditorRequest::test_pair(path.clone());
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let target = EditorTarget::capture(shell, cx);
            shell
                .commits
                .push(session(3, repository("/repo"), true, target.clone()));
            shell.edit_owned_commit_message(other, "#".into(), Some(3), target, window, cx);
        })
    });
    visual.run_until_parked();
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.commit_terminal(
                2,
                CommitPhase::Exited(ExitStatus::from_raw(256)),
                window,
                cx,
            );
            assert_eq!(shell.commit_editors.len(), 1);
        })
    });
    assert_eq!(owned_peer.read(&mut [0]).unwrap(), 0);
    assert_eq!(
        other_peer.read(&mut [0]).unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    visual.simulate_input("saved ");
    let (send, receive) = futures_channel::oneshot::channel();
    visual.update(|_, cx| shell.update(cx, |shell, _| shell.save_gate = Some(receive)));
    command(&mut visual, &shell, Command::CommitFinish);
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.commit_terminal(
                2,
                CommitPhase::Exited(ExitStatus::from_raw(256)),
                window,
                cx,
            );
            assert!(
                shell.buffers[shell.active_index()]
                    .buffer
                    .edit_inhibition()
                    .is_some()
            );
            shell.commit_terminal(
                3,
                CommitPhase::Exited(ExitStatus::from_raw(256)),
                window,
                cx,
            );
            assert!(shell.commit_editors.is_empty());
            assert!(
                shell.buffers[shell.active_index()]
                    .buffer
                    .edit_inhibition()
                    .is_none()
            );
        })
    });
    send.send(()).unwrap();
    wait(&mut visual, &shell);
    assert_eq!(other_peer.read(&mut [0]).unwrap(), 0);
    visual.read(|cx| {
        assert_eq!(
            shell.read(cx).active_view().read(cx).buffer().mode(),
            ModeId::FUNDAMENTAL
        );
        assert!(
            shell
                .read(cx)
                .active_view()
                .read(cx)
                .buffer()
                .text()
                .to_string()
                .contains("saved ")
        );
    });
    std::fs::remove_file(path).unwrap();
}

#[gpui::test]
fn staged_companion_preserves_editor_focus_and_does_not_own_a_later_redisplay(
    cx: &mut TestAppContext,
) {
    cx.update(crate::app::init);
    let repo = repository("/test-repo");
    let window = cx.open_window(size(px(960.), px(640.)), |window, cx| {
        AppShell::new(Buffer::status(repo.clone()), window, cx)
    });
    let shell = window.entity(cx).unwrap();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.update(|window, cx| {
        window.activate_window();
        window.draw(cx).clear(cx);
    });
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let status = shell.active_buffer_id();
            let editor = shell.create_buffer(Buffer::named("message"), window, cx);
            shell.display_temporary(editor, window, cx);
            let owner = shell.active_buffer_id();
            let editor_view = shell.active_view().clone();
            let input = editor_view.read(cx).display_snapshot().id();
            let diff = shell.create_buffer(
                Buffer::diff(Comparison {
                    repository: repo.clone(),
                    kind: DiffKind::Staged,
                    environment: crate::diff::ReadEnvironment::Commit,
                }),
                window,
                cx,
            );
            let (pane, buffer) = shell
                .display_temporary_beside(diff, &repo, window, cx)
                .unwrap();
            assert_eq!(shell.active_view(), &editor_view);
            assert_eq!(editor_view.read(cx).display_snapshot().id(), input);
            assert_eq!(shell.body_panes[&pane].current, buffer);
            assert_eq!(
                shell.buffers[shell.buffers.index_of(buffer).unwrap()]
                    .buffer
                    .mode(),
                ModeId::MAGIT_DIFF
            );
            assert!(
                shell
                    .display_temporary_beside(diff, &repo, window, cx)
                    .is_none()
            );
            shell.quit_commit_companion(pane, buffer, owner, window, cx);
            assert_eq!(shell.body_panes[&pane].current, status);
            shell.display_temporary(diff, window, cx);
            shell.quit_commit_companion(pane, buffer, owner, window, cx);
            assert_eq!(shell.active_buffer_id(), buffer);
        })
    });
}

#[gpui::test]
fn process_projection_tracks_output_without_stealing_reads_and_survives_view_lifecycle(
    cx: &mut TestAppContext,
) {
    use crate::app::git_test_support::{command, wait};
    use crate::command::Command;
    use crate::session::ReadKind;
    cx.background_executor.allow_parking();
    cx.update(crate::app::init);
    let fixture = crate::app::git_test_support::repo();
    let repo = Repository {
        root: fixture.0.clone(),
        git_dir: fixture.0.join(".git"),
    };
    let window = cx.open_window(size(px(960.), px(640.)), |window, cx| {
        AppShell::new(Buffer::status(repo.clone()), window, cx)
    });
    let shell = window.entity(cx).unwrap();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.update(|window, cx| {
        window.activate_window();
        window.draw(cx).clear(cx);
    });
    visual.update(|_, cx| {
        shell.update(cx, |shell, cx| {
            let target = EditorTarget::capture(shell, cx);
            let mut running = session(77, repo.clone(), true, target);
            running.record.output = b"first\n".to_vec();
            shell.commits.push(running);
        })
    });
    command(&mut visual, &shell, Command::MagitProcessBuffer);
    visual.simulate_keystrokes("alt->");
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.consume_commit_event(
                77,
                Ok(Event::Output("中文".as_bytes()[..2].to_vec())),
                "#",
                window,
                cx,
            );
            shell.consume_commit_event(
                77,
                Ok(Event::Output("中文".as_bytes()[2..].to_vec())),
                "#",
                window,
                cx,
            );
            let view = shell.active_view().read(cx);
            assert!(view.buffer().text().to_string().contains("first\n中文"));
            assert_eq!(view.buffer().point().0, view.buffer().text().len());
        })
    });
    visual.simulate_keystrokes("alt-< n tab");
    let point = visual.read(|cx| shell.read(cx).active_view().read(cx).buffer().point());
    visual.simulate_keystrokes("alt-x");
    let read = visual.read(|cx| shell.read(cx).session().unwrap().id());
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.consume_commit_event(77, Ok(Event::Output(b"more\n".to_vec())), "#", window, cx);
            assert_eq!(shell.session().unwrap().id(), read);
            assert_eq!(shell.session().unwrap().kind(), &ReadKind::Command);
            let view = shell.active_view().read(cx);
            assert_eq!(view.buffer().point(), point);
            assert!(view.buffer().sections().unwrap().nodes()[1].collapsed);
        })
    });
    visual.simulate_keystrokes("ctrl-g k");
    visual.read(|cx| {
        assert_eq!(
            shell.read(cx).session().unwrap().kind(),
            &ReadKind::CommitInterrupt { operation: 77 }
        );
    });
    visual.simulate_keystrokes("n k ctrl-g");
    visual.read(|cx| {
        assert!(shell.read(cx).commit_active(77));
        assert!(!shell.read(cx).commits[0].interrupted);
    });
    visual.simulate_keystrokes("k");
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.commit_terminal(
                77,
                CommitPhase::Failed {
                    started: true,
                    diagnostic: "finished meanwhile".into(),
                },
                window,
                cx,
            );
        })
    });
    visual.simulate_keystrokes("y");
    visual.read(|cx| {
        assert_eq!(
            shell.read(cx).active_view().read(cx).message(),
            Some("Process no longer running")
        )
    });
    visual.simulate_keystrokes("q $");
    visual.read(|cx| {
        assert!(
            shell
                .read(cx)
                .active_view()
                .read(cx)
                .buffer()
                .text()
                .to_string()
                .contains("finished meanwhile")
        )
    });
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let index = shell.active_index();
            shell.kill_buffer_at(index, window, cx);
            assert_eq!(shell.commits.len(), 1);
            assert!(
                shell
                    .buffers
                    .iter()
                    .all(|s| s.buffer.process_repository().is_none())
            );
        })
    });
    command(&mut visual, &shell, Command::MagitProcessBuffer);
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".pi/tmp")
        .join(format!(
            "process-export-{}-{}",
            std::process::id(),
            operation_id()
        ));
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let origin = shell.active_view().clone();
            shell.write_file(&origin, path.clone(), window, cx);
        })
    });
    wait(&mut visual, &shell);
    visual.read(|cx| {
        let view = shell.read(cx).active_view().read(cx);
        assert_eq!(view.buffer().mode(), ModeId::FUNDAMENTAL);
        assert!(!view.buffer().is_read_only());
        assert_eq!(view.buffer().repository(), None);
        assert_eq!(shell.read(cx).commits.len(), 1);
    });
    assert!(
        std::fs::read_to_string(&path)
            .unwrap()
            .contains("finished meanwhile")
    );
    std::fs::remove_file(path).unwrap();
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let index = shell
                .buffers
                .iter()
                .position(|s| s.buffer.status_repository().as_ref() == Some(&repo))
                .unwrap();
            shell.activate(index, window, cx);
            let origin = shell.active_view().clone();
            shell.view_commit_process(&origin, window, cx);
            assert!(
                shell
                    .active_view()
                    .read(cx)
                    .buffer()
                    .text()
                    .to_string()
                    .contains("finished meanwhile")
            );
        })
    });
}
