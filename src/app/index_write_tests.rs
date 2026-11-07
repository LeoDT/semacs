use super::*;
use crate::app::{GitWriteRecord, git_test_support::*};
use crate::buffer::Buffer;
use crate::command::Command;
use crate::diff::tests::Repository as Fixture;
use crate::status::Group;
use crate::text::ByteOffset;
use gpui_kit as gpui;
use gpui_kit::{TestAppContext, VisualTestContext};

fn select(visual: &mut VisualTestContext, shell: &Entity<AppShell>, group: Group) {
    visual.update(|window, cx| {
        let view = shell.read(cx).active_view().clone();
        let sections = view.read(cx).buffer().sections().unwrap();
        let point = sections
            .nodes()
            .iter()
            .find(|node| node.key.starts_with(&format!("{}/file/", group.key())))
            .unwrap()
            .range
            .start;
        view.update(cx, |view, cx| view.show_location(point, window, cx));
    });
}

fn capture(visual: &VisualTestContext, shell: &Entity<AppShell>) -> StatusWriteTarget {
    visual.read(|cx| {
        shell
            .read(cx)
            .capture_status_write(shell.read(cx).active_view(), cx)
            .unwrap()
    })
}

fn accept(
    visual: &mut VisualTestContext,
    shell: &Entity<AppShell>,
    target: StatusWriteTarget,
    operation: Operation,
) -> Result<u64, String> {
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.accept_status_write(target, operation, window, cx)
        })
    })
}

fn verify_gate(
    visual: &mut VisualTestContext,
    shell: &Entity<AppShell>,
) -> futures_channel::oneshot::Sender<()> {
    let (send, receive) = futures_channel::oneshot::channel();
    visual.update(|_, cx| shell.update(cx, |shell, _| shell.index_verify_gate = Some(receive)));
    send
}

fn write_gate(
    visual: &mut VisualTestContext,
    shell: &Entity<AppShell>,
) -> futures_channel::oneshot::Sender<()> {
    let (send, receive) = futures_channel::oneshot::channel();
    visual.update(|_, cx| shell.update(cx, |shell, _| shell.index_write_gate = Some(receive)));
    send
}

fn opened(cx: &mut TestAppContext) -> (Fixture, VisualTestContext, Entity<AppShell>) {
    let repo = repo();
    let (mut visual, shell) = open(&repo, cx);
    command(&mut visual, &shell, Command::MagitStatus);
    wait(&mut visual, &shell);
    select(&mut visual, &shell, Group::Unstaged);
    (repo, visual, shell)
}

#[gpui::test]
fn accepted_write_refreshes_only_status_keeps_diff_stale_and_uses_shared_slot(
    cx: &mut TestAppContext,
) {
    let (repo, mut visual, shell) = opened(cx);
    let status = visual.read(|cx| shell.read(cx).active_view().clone());
    command(&mut visual, &shell, Command::MagitDiffUnstaged);
    wait(&mut visual, &shell);
    let diff = visual.read(|cx| shell.read(cx).active_view().clone());
    command(&mut visual, &shell, Command::MagitStatus);
    wait(&mut visual, &shell);
    select(&mut visual, &shell, Group::Unstaged);
    let release = verify_gate(&mut visual, &shell);
    let target = capture(&visual, &shell);
    let id = accept(&mut visual, &shell, target, Operation::Stage).unwrap();
    until(&mut visual, &shell, |shell| {
        shell
            .git_writes
            .iter()
            .any(|record| record.phase == GitWritePhase::Verifying)
    });
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            assert!(shell.git_write_active());
            assert!(!shell.finish_git_read(id, window, cx));
            assert!(!shell.finish_git_read(id + 1, window, cx));
            assert!(status.read(cx).buffer().git_is_stale());
            assert!(diff.read(cx).buffer().git_is_stale());
            assert!(shell.capture_status_write(&status, cx).is_err());
            shell.refresh_status(&status, window, cx);
            assert!(shell.git_write_active());
        })
    });
    release.send(()).unwrap();
    wait(&mut visual, &shell);
    visual.read(|cx| {
        let shell = shell.read(cx);
        let record = shell.git_write_records().last().unwrap();
        assert_eq!(record.id, id);
        assert_eq!(record.outcome, Some(Outcome::Succeeded));
        assert_eq!(record.phase, GitWritePhase::Complete);
        assert_eq!(record.verified, Some(true));
        assert!(record.installed);
        assert!(!status.read(cx).buffer().git_is_stale());
        assert!(diff.read(cx).buffer().git_is_stale());
        assert!(!status.read(cx).buffer().is_modified());
        assert!(
            shell
                .messages
                .buffer
                .text()
                .to_string()
                .contains("write succeeded")
        );
    });
    assert_eq!(repo.git(&["show", ":a"]), b"disk changed\n");
    assert_eq!(std::fs::read(repo.0.join("a")).unwrap(), b"disk changed\n");
    select(&mut visual, &shell, Group::Staged);
    let target = capture(&visual, &shell);
    accept(&mut visual, &shell, target, Operation::Unstage).unwrap();
    wait(&mut visual, &shell);
    assert_eq!(repo.git(&["show", ":a"]), b"disk base\n");
    command(&mut visual, &shell, Command::MagitDiffUnstaged);
    wait(&mut visual, &shell);
    visual.read(|cx| assert!(!diff.read(cx).buffer().git_is_stale()));
}

#[gpui::test]
fn target_fences_reject_point_region_projection_input_session_and_reload_changes(
    cx: &mut TestAppContext,
) {
    use gpui_kit::InputHandler;
    for scenario in 0..7 {
        let (repo, mut visual, shell) = opened(cx);
        let target = capture(&visual, &shell);
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                let origin = shell.active_view().clone();
                match scenario {
                    0 => {
                        let point = origin.read(cx).buffer().point();
                        origin.update(cx, |view, cx| {
                            view.show_location(ByteOffset(0), window, cx);
                            view.show_location(point, window, cx);
                        });
                    }
                    1 => {
                        let mut buffer = origin.read(cx).buffer().new_view();
                        buffer.set_mark(ByteOffset(0)).unwrap();
                    }
                    2 => {
                        let index = shell.buffer_index(&origin, cx).unwrap();
                        shell.buffers[index].buffer.begin_status_request().unwrap();
                    }
                    3 => origin.update(cx, |view, cx| {
                        view.suspend_input(cx);
                        view.restore_input(window, cx);
                    }),
                    4 => shell.config_generation += 1,
                    5 | 6 => {}
                    _ => unreachable!(),
                }
            })
        });
        if scenario == 5 {
            visual.simulate_keystrokes("ctrl-x");
        }
        if scenario == 6 {
            command(&mut visual, &shell, Command::ExecuteExtendedCommand);
            let editor = visual.read(|cx| {
                shell
                    .read(cx)
                    .minibuffer
                    .as_ref()
                    .unwrap()
                    .view
                    .read(cx)
                    .editor()
                    .clone()
            });
            let mut input = crate::view::input_tests::handler(&editor, &visual);
            visual.update(|window, cx| {
                input.replace_and_mark_text_in_range(None, "ni", Some(2..2), window, cx)
            });
        }
        assert!(
            accept(&mut visual, &shell, target, Operation::Stage).is_err(),
            "scenario {scenario}"
        );
        visual.read(|cx| {
            assert!(shell.read(cx).git_write_records().next().is_none());
            assert!(shell.read(cx).git_task.is_none());
        });
        assert_eq!(repo.git(&["show", ":a"]), b"disk base\n");
    }
}

#[gpui::test]
fn headings_regions_folded_leaves_and_diff_cannot_supply_write_targets(cx: &mut TestAppContext) {
    let (_repo, mut visual, shell) = opened(cx);
    visual.update(|window, cx| {
        let view = shell.read(cx).active_view().clone();
        view.update(cx, |view, cx| view.show_location(ByteOffset(0), window, cx));
        assert!(shell.read(cx).capture_status_write(&view, cx).is_err());
    });
    select(&mut visual, &shell, Group::Unstaged);
    visual.update(|_, cx| {
        let view = shell.read(cx).active_view().clone();
        let mut buffer = view.read(cx).buffer().new_view();
        buffer.set_mark(ByteOffset(0)).unwrap();
        assert!(shell.read(cx).capture_status_write(&view, cx).is_err());
        buffer.deactivate_mark();
        let sections = buffer.sections().unwrap();
        buffer
            .set_point(
                sections
                    .nodes()
                    .iter()
                    .find(|node| node.key == "unstaged")
                    .unwrap()
                    .range
                    .start,
            )
            .unwrap();
        buffer.toggle_section().unwrap();
        assert!(shell.read(cx).capture_status_write(&view, cx).is_err());
    });
    command(&mut visual, &shell, Command::MagitDiffUnstaged);
    wait(&mut visual, &shell);
    visual.read(|cx| {
        assert!(
            shell
                .read(cx)
                .capture_status_write(shell.read(cx).active_view(), cx)
                .is_err()
        )
    });
}

#[gpui::test]
fn accepted_write_survives_kill_export_and_reload_without_installing_into_replacements(
    cx: &mut TestAppContext,
) {
    for scenario in 0..5 {
        let (repo, mut visual, shell) = opened(cx);
        if scenario == 4 {
            command(&mut visual, &shell, Command::SplitWindowRight);
        }
        let original = visual.read(|cx| shell.read(cx).active_view().clone());
        let release = write_gate(&mut visual, &shell);
        let target = capture(&visual, &shell);
        accept(&mut visual, &shell, target, Operation::Stage).unwrap();
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                assert!(shell.git_write_active());
                let repository = original.read(cx).buffer().repository().unwrap();
                let index = shell.buffer_index(&original, cx).unwrap();
                match scenario {
                    0 => shell.remove_buffer(index, window, cx),
                    1 => shell.write_file(&original, repo.0.join("export.txt"), window, cx),
                    2 | 3 => {
                        repo.write(
                            "init.js",
                            if scenario == 2 {
                                "semacs.setFont({size: 15});"
                            } else {
                                "throw Error('controlled reload failure');"
                            },
                        );
                        shell.config_path = Some(repo.0.join("init.js"));
                        shell.reload_configuration(&original, window, cx);
                    }
                    4 => shell.window_command(Command::DeleteWindow, window, cx),
                    _ => unreachable!(),
                }
                if scenario == 0 {
                    shell.create_buffer(Buffer::status(repository), window, cx);
                }
            })
        });
        if scenario == 1 {
            until(&mut visual, &shell, |shell| {
                shell.buffers.iter().all(|slot| !slot.save.is_saving())
            });
        }
        if scenario >= 2 {
            until(&mut visual, &shell, |shell| shell.reload_task.is_none());
        }
        release.send(()).unwrap();
        wait(&mut visual, &shell);
        assert_eq!(repo.git(&["show", ":a"]), b"disk changed\n");
        visual.read(|cx| {
            let shell = shell.read(cx);
            let record = shell.git_write_records().last().unwrap();
            assert_eq!(record.outcome, Some(Outcome::Succeeded));
            assert_eq!(record.verified, Some(true));
            assert_eq!(record.installed, scenario >= 2);
            assert!(
                shell
                    .messages
                    .buffer
                    .text()
                    .to_string()
                    .contains("write succeeded")
            );
            if scenario == 0 {
                assert!(
                    shell
                        .buffers
                        .iter()
                        .find(|slot| slot.buffer.status_repository().is_some())
                        .unwrap()
                        .buffer
                        .text()
                        .is_empty()
                );
            }
            if scenario == 1 {
                assert!(original.read(cx).buffer().status_repository().is_none());
                assert_eq!(
                    original.read(cx).buffer().mode(),
                    crate::mode::ModeId::FUNDAMENTAL
                );
            }
        });
    }
}

#[gpui::test]
fn successful_write_failed_refresh_stays_successful_and_old_status_is_blocked(
    cx: &mut TestAppContext,
) {
    let (repo, mut visual, shell) = opened(cx);
    let status = visual.read(|cx| shell.read(cx).active_view().clone());
    let old = visual.read(|cx| status.read(cx).buffer().text().to_string());
    let release = verify_gate(&mut visual, &shell);
    let target = capture(&visual, &shell);
    accept(&mut visual, &shell, target, Operation::Stage).unwrap();
    until(&mut visual, &shell, |shell| {
        shell
            .git_writes
            .iter()
            .any(|record| record.phase == GitWritePhase::Verifying)
    });
    repo.write(".gitattributes", "a filter=evil\n");
    repo.git(&["config", "filter.evil.clean", "touch SHOULD_NOT_EXIST; cat"]);
    release.send(()).unwrap();
    wait(&mut visual, &shell);
    visual.read(|cx| {
        let shell = shell.read(cx);
        let record = shell.git_write_records().last().unwrap();
        assert_eq!(record.outcome, Some(Outcome::Succeeded));
        assert_eq!(record.verified, Some(false));
        assert!(!record.installed);
        assert_eq!(status.read(cx).buffer().text().to_string(), old);
        assert!(status.read(cx).buffer().git_is_stale());
        assert!(shell.capture_status_write(&status, cx).is_err());
        let history = shell.messages.buffer.text().to_string();
        assert!(
            history.contains("write succeeded; refresh failed"),
            "{history}"
        );
    });
    assert!(!repo.0.join("SHOULD_NOT_EXIST").exists());
    assert_eq!(repo.git(&["show", ":a"]), b"disk changed\n");
    repo.write(".gitattributes", "");
    command(&mut visual, &shell, Command::MagitRefresh);
    wait(&mut visual, &shell);
    select(&mut visual, &shell, Group::Staged);
    let _ = capture(&visual, &shell);
}

#[gpui::test]
fn preflight_refusal_and_nonzero_exit_keep_distinct_records_and_mark_stale(
    cx: &mut TestAppContext,
) {
    for nonzero in [false, true] {
        let (repo, mut visual, shell) = opened(cx);
        let release = write_gate(&mut visual, &shell);
        let target = capture(&visual, &shell);
        accept(&mut visual, &shell, target, Operation::Stage).unwrap();
        if nonzero {
            repo.write(".git/index.lock", "external lock\n");
        } else {
            repo.git(&["commit", "--allow-empty", "-qm", "external"]);
        }
        release.send(()).unwrap();
        wait(&mut visual, &shell);
        visual.read(|cx| {
            let shell = shell.read(cx);
            let record = shell.git_write_records().last().unwrap();
            assert_eq!(
                record.outcome,
                Some(if nonzero {
                    Outcome::Failed
                } else {
                    Outcome::NotExecuted
                })
            );
            assert_eq!(record.verified, if nonzero { Some(true) } else { None });
            assert_eq!(
                shell.active_view().read(cx).buffer().git_is_stale(),
                !nonzero
            );
        });
        assert_eq!(repo.git(&["show", ":a"]), b"disk base\n");
        if nonzero {
            assert_eq!(
                std::fs::read(repo.0.join(".git/index.lock")).unwrap(),
                b"external lock\n"
            );
        }
    }
}

#[gpui::test]
fn clean_exit_waits_through_accepted_write_and_verification(cx: &mut TestAppContext) {
    let (_repo, mut visual, shell) = opened(cx);
    let release = verify_gate(&mut visual, &shell);
    let target = capture(&visual, &shell);
    accept(&mut visual, &shell, target, Operation::Stage).unwrap();
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            assert!(!shell.begin_exit(window, cx));
            assert!(shell.exiting());
            assert!(shell.session.is_none());
        })
    });
    until(&mut visual, &shell, |shell| {
        shell
            .git_writes
            .iter()
            .any(|record| record.phase == GitWritePhase::Verifying)
    });
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert!(shell.exiting());
        assert!(shell.git_write_active());
        assert!(shell.session.is_none());
    });
    release.send(()).unwrap();
    wait(&mut visual, &shell);
    assert!(visual.windows().is_empty());
    visual.read(|cx| {
        assert!(!shell.read(cx).git_write_active());
        assert_eq!(
            shell.read(cx).git_write_records().last().unwrap().phase,
            GitWritePhase::Complete
        );
    });
}

#[gpui::test]
fn uncertain_exit_confirmation_precedes_dirty_flow_and_rejects_empty_or_cancel(
    cx: &mut TestAppContext,
) {
    let (_repo, mut visual, shell) = opened(cx);
    // Reuse the owned terminal-completion path, without an artificial Git shell command.
    let target = capture(&visual, &shell);
    let release = write_gate(&mut visual, &shell);
    let id = accept(&mut visual, &shell, target, Operation::Stage).unwrap();
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            assert!(!shell.request_close(window, cx));
            let buffer = shell.active_view().read(cx).buffer().snapshot_id().buffer;
            let repository = shell.active_view().read(cx).buffer().repository().unwrap();
            shell.complete_status_write(
                id,
                buffer,
                0,
                repository,
                Completion {
                    outcome: Outcome::Uncertain,
                    diagnostic: "controlled lost process outcome\n"
                        .repeat(crate::messages::LOG_LINES as usize + 250),
                    stale: true,
                    verification: Some(Err("controlled verification failure".into())),
                },
                cx,
            );
            // The task is still owned: even a terminal record cannot permit exit before its slot is released.
            shell.process_next_exit(window, cx);
            assert!(shell.session.is_none());
            shell.git_task = None;
            shell.process_next_exit(window, cx);
            assert!(matches!(
                shell.session.as_ref().unwrap().kind(),
                crate::session::ReadKind::ExitUncertainGit
            ));
            shell.answer_query(crate::session::Answer::Invalid, window, cx);
            assert!(shell.session.is_some());
            shell.answer_query(crate::session::Answer::No, window, cx);
            assert!(!shell.exiting());
            assert!(shell.session.is_none());
        })
    });
    drop(release);
    visual.run_until_parked();
    visual.read(|cx| {
        let shell = shell.read(cx);
        let history = shell.messages.buffer.text().to_string();
        assert!(
            history.contains(&format!("Git #{id} Stage: result uncertain")),
            "{history}"
        );
        assert!(history.contains("refresh failed"), "{history}");
        assert!(history.contains("diagnostic lines truncated"), "{history}");
        let record = shell
            .git_write_records()
            .find(|record| record.id == id)
            .unwrap();
        assert!(record.diagnostic.lines().count() > crate::messages::LOG_LINES as usize);
        assert!(record.diagnostic.len() <= 64 * 1024);
    });
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            assert!(!shell.begin_exit(window, cx));
            assert!(matches!(
                shell.session.as_ref().unwrap().kind(),
                crate::session::ReadKind::ExitUncertainGit
            ));
            shell.cancel_closing();
        })
    });
}

#[gpui::test]
fn uncertain_confirmation_yes_resumes_dirty_save_flow_and_cancel_keeps_records(
    cx: &mut TestAppContext,
) {
    let (_repo, mut visual, shell) = opened(cx);
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let status = shell.active_view().read(cx).buffer();
            let data = status.status_data().unwrap();
            let target = data
                .index_request(Group::Unstaged, 0, Operation::Stage)
                .unwrap();
            let id = operation_id();
            shell
                .git_writes
                .accept(id, &target, status.snapshot_id().buffer, 1);
            shell.git_writes.get_mut(id).unwrap().outcome = Some(Outcome::Uncertain);
            shell.git_writes.complete(id);
            let mut ordinary = shell
                .buffers
                .iter()
                .find(|slot| slot.buffer.path().is_some())
                .unwrap()
                .buffer
                .new_view();
            ordinary
                .edit(crate::buffer::Edit {
                    snapshot: ordinary.snapshot_id(),
                    range: ByteOffset(0)..ByteOffset(0),
                    text: "unsaved\n",
                    source: crate::buffer::EditSource::Command,
                })
                .unwrap();
            assert!(!shell.begin_exit(window, cx));
            assert!(matches!(
                shell.session.as_ref().unwrap().kind(),
                crate::session::ReadKind::ExitUncertainGit
            ));
            shell.answer_query(crate::session::Answer::Yes, window, cx);
            assert!(matches!(
                shell.session.as_ref().unwrap().kind(),
                crate::session::ReadKind::ExitSave { .. }
            ));
            shell.answer_query(crate::session::Answer::No, window, cx);
            assert!(matches!(
                shell.session.as_ref().unwrap().kind(),
                crate::session::ReadKind::ExitModified
            ));
            shell.answer_query(crate::session::Answer::No, window, cx);
            assert!(!shell.exiting());
            assert!(shell.git_writes.uncertain());
            assert_eq!(
                shell.git_write_records().last().unwrap().outcome,
                Some(Outcome::Uncertain)
            );
        })
    });
    assert!(!visual.windows().is_empty());
}

#[gpui::test]
fn records_evict_only_completed_entries_and_uncertainty_survives_eviction(cx: &mut TestAppContext) {
    let (_repo, visual, shell) = opened(cx);
    let data = visual.read(|cx| {
        shell
            .read(cx)
            .active_view()
            .read(cx)
            .buffer()
            .status_data()
            .unwrap()
    });
    let target = data
        .index_request(Group::Unstaged, 0, Operation::Stage)
        .unwrap();
    let buffer = visual.read(|cx| shell.read(cx).active_buffer_id());
    let mut records = super::super::git::records::WriteRecords::default();
    records.accept(1, &target, buffer, 1);
    records.accept(2, &target, buffer, 1);
    records.get_mut(2).unwrap().outcome = Some(Outcome::Uncertain);
    records.complete(2);
    for id in 3..100 {
        records.accept(id, &target, buffer, 1);
        records.get_mut(id).unwrap().outcome = Some(Outcome::Succeeded);
        records.complete(id);
    }
    let retained: Vec<&GitWriteRecord> = records.iter().collect();
    assert_eq!(retained.len(), 65);
    assert_eq!(retained[0].id, 1);
    assert_eq!(retained[0].phase, GitWritePhase::Accepted);
    assert!(records.uncertain());
}
