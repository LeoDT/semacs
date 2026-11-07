use super::*;
use crate::session::ReadKind;
use std::os::unix::process::ExitStatusExt;

#[gpui::test]
fn only_owned_reads_resume_editor_display_and_stale_answers_cannot_consume_a_prompt(
    cx: &mut TestAppContext,
) {
    use crate::buffer::{Edit, EditSource};
    use crate::text::ByteOffset;

    cx.background_executor.allow_parking();
    cx.update(crate::app::init);
    for phase in ["owned", "unrelated", "changed"] {
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
                    .push(session(1, repository("/one"), true, target));
                let origin = shell.active_view().clone();
                if phase == "unrelated" {
                    let id = shell
                        .begin_read(ReadKind::Command, &origin, "", window, cx)
                        .unwrap();
                    shell.end_session(id, window, cx);
                }
                output(shell, 1, b"Continue [y/n] ", window, cx);
                let read = shell.session().unwrap();
                let id = read.id();
                let ReadKind::CommitYesNo { request, .. } = *read.kind() else {
                    panic!()
                };
                shell.answer_commit_prompt(1, request + 100, "yes".into(), &origin, window, cx);
                assert_eq!(shell.session().unwrap().id(), id);
                if phase == "changed" {
                    let mut buffer = origin.read(cx).buffer().new_view();
                    buffer
                        .edit(Edit {
                            snapshot: buffer.snapshot_id(),
                            range: ByteOffset(0)..ByteOffset(0),
                            text: "background edit",
                            source: EditSource::Command,
                        })
                        .unwrap();
                    shell.refresh_buffer(buffer.snapshot_id().buffer, cx);
                }
                shell.end_session(id, window, cx);
                shell.answer_commit_prompt(1, request, "yes".into(), &origin, window, cx);
                assert_eq!(
                    shell.commits[0].target.is_current(shell, cx),
                    phase == "owned"
                );
            });
        });
        crate::app::git_test_support::until(&mut visual, &shell, |shell| {
            !shell.commits[0].record.diagnostics.is_empty()
        });
        visual.read(|cx| {
            assert!(shell.read(cx).session().is_none());
            assert_eq!(shell.read(cx).commits[0].record.output, b"Continue [y/n] ");
            assert!(
                shell.read(cx).commits[0].record.diagnostics[0].contains("Git is still starting")
            );
        });
    }
}

fn output(
    shell: &mut AppShell,
    operation: u64,
    bytes: &[u8],
    window: &mut gpui::Window,
    cx: &mut gpui::Context<AppShell>,
) {
    shell.consume_commit_event(
        operation,
        Ok(Event::Output(bytes.to_vec())),
        "#",
        window,
        cx,
    );
}

#[gpui::test]
fn prompts_wait_for_reads_and_retire_only_their_owner(cx: &mut TestAppContext) {
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
        shell.update(cx, |shell, cx| {
            let target = EditorTarget::capture(shell, cx);
            shell
                .commits
                .push(session(1, repository("/one"), true, target.clone()));
            shell
                .commits
                .push(session(2, repository("/two"), true, target));
            let origin = shell.active_view().clone();
            shell.begin_read(ReadKind::FindFile, &origin, "unfinished", window, cx);
            output(shell, 1, b"Continue [y/", window, cx);
            output(shell, 1, b"n]", window, cx);
            output(shell, 1, b" ", window, cx);
            output(shell, 2, b"Username for 'host': ", window, cx);
            assert_eq!(shell.session().unwrap().kind(), &ReadKind::FindFile);
            assert_eq!(shell.session_input(cx).as_deref(), Some("unfinished"));
        })
    });
    visual.simulate_keystrokes("ctrl-g");
    let read = visual.read(|cx| {
        let read = shell.read(cx).session().unwrap();
        assert!(matches!(
            read.kind(),
            ReadKind::CommitYesNo { operation: 1, .. }
        ));
        read.id()
    });
    visual.simulate_input("maybe");
    visual.simulate_keystrokes("enter");
    visual.read(|cx| assert_eq!(shell.read(cx).session().unwrap().id(), read));
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.commit_terminal(
                2,
                CommitPhase::Exited(ExitStatus::from_raw(256)),
                window,
                cx,
            );
            assert_eq!(shell.session().unwrap().id(), read);
            shell.commit_terminal(
                1,
                CommitPhase::Exited(ExitStatus::from_raw(256)),
                window,
                cx,
            );
            assert!(shell.session().is_none());
        })
    });
    visual.run_until_parked();
    visual.read(|cx| assert!(shell.read(cx).session().is_none()));
    visual.simulate_input("body");
    visual.read(|cx| {
        assert_eq!(
            shell
                .read(cx)
                .active_view()
                .read(cx)
                .buffer()
                .text()
                .to_string(),
            "body"
        )
    });
}

#[gpui::test]
fn prompt_cancellation_does_not_cancel_another_process_and_terminal_preserves_other_reads(
    cx: &mut TestAppContext,
) {
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
                .push(session(1, repository("/one"), true, target.clone()));
            shell
                .commits
                .push(session(2, repository("/two"), true, target));
            output(shell, 1, b"Username for 'one': ", window, cx);
            output(shell, 2, b"Continue? (yes/no) ", window, cx);
            assert!(matches!(
                shell.session().unwrap().kind(),
                ReadKind::CommitUsername { operation: 1, .. }
            ));
        })
    });
    visual.simulate_keystrokes("ctrl-g");
    visual.run_until_parked();
    visual.read(|cx| {
        assert!(matches!(
            shell.read(cx).session().unwrap().kind(),
            ReadKind::CommitYesNo { operation: 2, .. }
        ))
    });
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.commit_terminal(
                1,
                CommitPhase::Exited(ExitStatus::from_raw(256)),
                window,
                cx,
            );
            assert!(matches!(
                shell.session().unwrap().kind(),
                ReadKind::CommitYesNo { operation: 2, .. }
            ));
        })
    });
    visual.simulate_keystrokes("ctrl-g alt-x");
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.commit_terminal(
                2,
                CommitPhase::Exited(ExitStatus::from_raw(256)),
                window,
                cx,
            );
            assert_eq!(shell.session().unwrap().kind(), &ReadKind::Command);
        })
    });
}
