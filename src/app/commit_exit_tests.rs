use super::*;
use crate::buffer::{Edit, EditSource};
use crate::session::{Answer, ReadKind};
use crate::text::ByteOffset;
use std::os::unix::process::ExitStatusExt;

fn opened(cx: &mut TestAppContext) -> (VisualTestContext, gpui_kit::Entity<AppShell>) {
    cx.update(crate::app::init);
    let window = cx.open_window(size(px(960.), px(640.)), |window, cx| {
        AppShell::new(Buffer::named("origin"), window, cx)
    });
    let shell = window.entity(cx).unwrap();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.update(|window, cx| {
        window.activate_window();
        window.draw(cx).clear(cx);
        shell.update(cx, |shell, cx| {
            let target = EditorTarget::capture(shell, cx);
            shell
                .commits
                .push(session(1, repository("/exit"), true, target.clone()));
            shell
                .commits
                .push(session(2, repository("/exit-other"), true, target));
        });
    });
    (visual, shell)
}

fn opened_file(cx: &mut TestAppContext) -> (VisualTestContext, gpui_kit::Entity<AppShell>) {
    let (visual, shell) = opened(cx);
    visual.read(|cx| {
        shell
            .read(cx)
            .active_view()
            .read(cx)
            .buffer()
            .new_view()
            .set_path(
                std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".pi/tmp/exit-draft.txt"),
            )
    });
    (visual, shell)
}

#[gpui::test]
fn active_exit_rejects_empty_no_and_quit_without_ending_sessions(cx: &mut TestAppContext) {
    let (mut visual, shell) = opened(cx);
    for answer in ["", "no", "quit"] {
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                assert!(!shell.request_close(window, cx));
                assert_eq!(
                    shell.session().unwrap().kind(),
                    &ReadKind::ExitActiveCommits
                );
                shell.answer_query(Answer::Invalid, window, cx);
                assert!(shell.exiting());
            })
        });
        if answer == "quit" {
            visual.simulate_keystrokes("ctrl-g");
        } else {
            visual.simulate_input(answer);
            visual.simulate_keystrokes("enter");
            if answer.is_empty() {
                visual.read(|cx| assert!(shell.read(cx).session().is_some()));
                visual.simulate_keystrokes("ctrl-g");
            }
        }
        visual.read(|cx| {
            let shell = shell.read(cx);
            assert!(!shell.exiting());
            assert!(shell.session().is_none());
            assert!(
                shell
                    .commits
                    .iter()
                    .all(|s| s.record.phase.is_active() && !s.exit_termination_requested)
            );
        });
        assert_eq!(visual.windows().len(), 1);
    }
}

#[gpui::test]
fn dirty_exit_precedes_process_query_and_terminal_does_not_advance_a_read(cx: &mut TestAppContext) {
    let (mut visual, shell) = opened_file(cx);
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let mut buffer = shell.active_view().read(cx).buffer().new_view();
            buffer
                .edit(Edit {
                    snapshot: buffer.snapshot_id(),
                    range: ByteOffset(0)..ByteOffset(0),
                    text: "unsaved",
                    source: EditSource::Command,
                })
                .unwrap();
            assert!(!shell.request_close(window, cx));
            let read = shell.session().unwrap().id();
            assert!(matches!(
                shell.session().unwrap().kind(),
                ReadKind::ExitSave { .. }
            ));
            shell.commit_terminal(
                1,
                CommitPhase::Exited(ExitStatus::from_raw(256)),
                window,
                cx,
            );
            assert_eq!(shell.session().unwrap().id(), read);
            shell.answer_query(Answer::No, window, cx);
            assert_eq!(shell.session().unwrap().kind(), &ReadKind::ExitModified);
            shell.answer_query(Answer::Yes, window, cx);
            assert_eq!(
                shell.session().unwrap().kind(),
                &ReadKind::ExitActiveCommits
            );
            shell.commit_terminal(2, CommitPhase::Exited(ExitStatus::from_raw(0)), window, cx);
        })
    });
    assert!(visual.windows().is_empty());
}

#[gpui::test]
fn delayed_exit_rechecks_new_dirty_text_after_previous_decisions(cx: &mut TestAppContext) {
    for initially_dirty in [false, true] {
        let (mut visual, shell) = opened_file(cx);
        if initially_dirty {
            visual.simulate_input("discarded");
        }
        visual.simulate_keystrokes("ctrl-x ctrl-c");
        if initially_dirty {
            visual.simulate_keystrokes("n");
            visual.simulate_input("yes");
            visual.simulate_keystrokes("enter");
        }
        visual.simulate_input("yes");
        visual.simulate_keystrokes("enter");
        visual.simulate_input("new unconfirmed text");
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.commit_terminal(1, CommitPhase::Exited(ExitStatus::from_raw(0)), window, cx);
                shell.commit_terminal(2, CommitPhase::Exited(ExitStatus::from_raw(0)), window, cx);
            })
        });
        assert_eq!(visual.windows().len(), 1);
        visual.read(|cx| {
            assert_eq!(
                shell.read(cx).session().unwrap().kind(),
                &ReadKind::ExitModified
            );
            assert!(
                shell
                    .read(cx)
                    .active_view()
                    .read(cx)
                    .buffer()
                    .text()
                    .to_string()
                    .contains("new unconfirmed text")
            );
        });
        visual.simulate_input("no");
        visual.simulate_keystrokes("enter ctrl-x ctrl-c n");
        visual.simulate_input("yes");
        visual.simulate_keystrokes("enter");
    }
}

#[gpui::test]
fn confirmed_exit_waits_for_every_real_terminal_including_starting_sessions(
    cx: &mut TestAppContext,
) {
    let (mut visual, shell) = opened(cx);
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.commits[0].record.phase = CommitPhase::Starting;
            assert!(!shell.begin_exit(window, cx));
            shell.answer_query(Answer::Yes, window, cx);
            assert!(shell.session().is_none());
            assert!(shell.commits.iter().all(|s| s.exit_termination_requested));
            shell.commit_terminal(2, CommitPhase::Exited(ExitStatus::from_raw(9)), window, cx);
            assert!(shell.exiting());
            assert!(shell.session().is_none());
            shell.commit_terminal(
                1,
                CommitPhase::Failed {
                    started: false,
                    diagnostic: "start failed".into(),
                },
                window,
                cx,
            );
        })
    });
    assert!(visual.windows().is_empty());
}

#[gpui::test]
fn native_close_replaces_a_process_prompt_and_refusal_resumes_its_request(cx: &mut TestAppContext) {
    let (mut visual, shell) = opened(cx);
    visual.update(|window, cx| shell.update(cx, |shell, cx| {
        shell.consume_commit_event(1, Ok(Event::Output(b"Continue [y/n] ".to_vec())), "#", window, cx);
        let ReadKind::CommitYesNo { request, .. } = *shell.session().unwrap().kind() else { panic!() };
        assert!(!shell.request_close(window, cx));
        assert_eq!(shell.session().unwrap().kind(), &ReadKind::ExitActiveCommits);
        shell.answer_query(Answer::No, window, cx);
        assert!(!shell.exiting());
        shell.present_commit_prompt(window, cx);
        assert!(matches!(shell.session().unwrap().kind(), ReadKind::CommitYesNo { operation: 1, request: pending, .. } if *pending == request));
        assert!(!shell.commits[0].exit_termination_requested);
        assert!(!shell.commits[0].interrupted);
    }));
}

#[gpui::test]
fn terminal_during_save_wait_does_not_skip_dirty_completion(cx: &mut TestAppContext) {
    use crate::app::git_test_support::until;
    let fixture = crate::app::git_test_support::repo();
    let (mut visual, shell) = opened(cx);
    let (send, receive) = futures_channel::oneshot::channel();
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let path = fixture.0.join("draft");
            std::fs::write(&path, "draft").unwrap();
            let loaded = crate::files::load(&path).unwrap();
            let index = shell.create_buffer(
                Buffer::visiting(path, loaded.file, loaded.stamp),
                window,
                cx,
            );
            shell.activate(index, window, cx);
        })
    });
    visual.simulate_input("new ");
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.save_gate = Some(receive);
            let origin = shell.active_view().clone();
            assert!(shell.save_buffer(&origin, window, cx));
            assert!(!shell.begin_exit(window, cx));
            assert!(shell.session().is_none());
            shell.commit_terminal(1, CommitPhase::Exited(ExitStatus::from_raw(0)), window, cx);
            assert!(shell.session().is_none());
            assert!(shell.exiting());
        })
    });
    send.send(()).unwrap();
    until(&mut visual, &shell, |shell| shell.session().is_some());
    visual.read(|cx| {
        assert_eq!(
            shell.read(cx).session().unwrap().kind(),
            &ReadKind::ExitActiveCommits
        )
    });
    visual.simulate_input("no");
    visual.simulate_keystrokes("enter");
    assert_eq!(
        std::fs::read_to_string(fixture.0.join("draft")).unwrap(),
        "new draft"
    );
    visual.read(|cx| assert!(!shell.read(cx).exiting()));
}

#[gpui::test]
fn canceling_a_confirmed_exit_clears_pending_hangups_for_a_new_attempt(cx: &mut TestAppContext) {
    let (mut visual, shell) = opened(cx);
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.commits[0].record.phase = CommitPhase::Starting;
            assert!(!shell.begin_exit(window, cx));
            shell.answer_query(Answer::Yes, window, cx);
            assert!(shell.waiting_commit_exit());
            assert!(shell.commits.iter().all(|s| s.exit_termination_requested));
        })
    });
    visual.simulate_keystrokes("ctrl-x ctrl-g");
    visual.read(|cx| assert!(shell.read(cx).exiting()));
    visual.simulate_keystrokes("ctrl-g");
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert!(!shell.exiting());
        assert!(shell.commits.iter().all(|s| !s.exit_termination_requested));
    });
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            assert!(!shell.begin_exit(window, cx));
            assert_eq!(
                shell.session().unwrap().kind(),
                &ReadKind::ExitActiveCommits
            );
            shell.answer_query(Answer::Yes, window, cx);
            assert!(shell.commits.iter().all(|s| s.exit_termination_requested));
        })
    });
}

#[gpui::test]
fn terminal_during_process_control_query_resumes_exit_after_its_answer(cx: &mut TestAppContext) {
    let (mut visual, shell) = opened(cx);
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            assert!(!shell.begin_exit(window, cx));
            shell.answer_query(Answer::Yes, window, cx);
            let origin = shell.active_view().clone();
            let read = shell
                .begin_read(
                    ReadKind::CommitInterrupt { operation: 1 },
                    &origin,
                    "",
                    window,
                    cx,
                )
                .unwrap();
            shell.commit_terminal(1, CommitPhase::Exited(ExitStatus::from_raw(0)), window, cx);
            shell.commit_terminal(2, CommitPhase::Exited(ExitStatus::from_raw(0)), window, cx);
            assert_eq!(shell.session().unwrap().id(), read);
        })
    });
    assert_eq!(visual.windows().len(), 1);
    visual.simulate_keystrokes("n");
    assert!(visual.windows().is_empty());
}

#[gpui::test]
fn ending_all_processes_dismisses_only_the_active_process_exit_query(cx: &mut TestAppContext) {
    let (mut visual, shell) = opened(cx);
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            assert!(!shell.begin_exit(window, cx));
            let id = shell.session().unwrap().id();
            shell.commit_terminal(1, CommitPhase::Exited(ExitStatus::from_raw(0)), window, cx);
            assert_eq!(shell.session().unwrap().id(), id);
            shell.commit_terminal(2, CommitPhase::Exited(ExitStatus::from_raw(0)), window, cx);
        })
    });
    assert!(visual.windows().is_empty());
}
