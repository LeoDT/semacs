use super::*;
use crate::app::git_test_support::{command, open, repo, wait};
use crate::command::Command;
use crate::mode::ModeId;
use gpui_kit::{InputHandler, TestAppContext};

#[gpui_kit::test]
fn unconfigured_shell_does_not_use_the_test_executable_as_editor(cx: &mut TestAppContext) {
    let repo = repo();
    let (mut visual, shell) = open(&repo, cx);
    command(&mut visual, &shell, Command::MagitCommitCreate);
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert!(shell.git_task.is_none());
        assert_eq!(shell.commit_records().count(), 0);
        assert_eq!(
            shell.active_view().read(cx).message(),
            Some("Git editor helper is not configured for this application")
        );
    });
}

#[gpui_kit::test]
fn late_preparation_does_not_launch_or_prompt_on_another_input_target(cx: &mut TestAppContext) {
    for phase in [
        "read", "switch", "compose", "edit", "quit", "exit", "killed",
    ] {
        let repo = repo();
        let (mut visual, shell) = open(&repo, cx);
        let origin = visual.read(|cx| shell.read(cx).active_view().clone());
        let mut input = crate::view::input_tests::handler(&origin, &visual);
        let (send, receive) = futures_channel::oneshot::channel();
        visual.update(|_, cx| {
            shell.update(cx, |shell, _| {
                shell.set_commit_runtime(Runtime {
                    executable: repo.0.join("unused-helper"),
                    directory: repo.0.clone(),
                });
                shell.commit_start_gate = Some(receive);
            })
        });
        command(&mut visual, &shell, Command::MagitCommitCreate);
        match phase {
            "read" => command(&mut visual, &shell, Command::ExecuteExtendedCommand),
            "switch" => {
                command(&mut visual, &shell, Command::SplitWindowRight);
                command(&mut visual, &shell, Command::OtherWindow);
            }
            "compose" => visual.update(|window, cx| {
                input.replace_and_mark_text_in_range(None, "ni", Some(2..2), window, cx)
            }),
            "edit" => visual.simulate_input("changed"),
            "quit" => visual.simulate_keystrokes("ctrl-g"),
            "exit" => {
                visual.simulate_input("dirty");
                visual.update(|window, cx| {
                    shell.update(cx, |shell, cx| {
                        shell.begin_exit(window, cx);
                    })
                });
            }
            "killed" => visual.update(|window, cx| {
                shell.update(cx, |shell, cx| shell.remove_buffer(0, window, cx))
            }),
            _ => unreachable!(),
        }
        if phase == "quit" {
            assert!(send.send(()).is_err());
        } else {
            send.send(()).unwrap();
        }
        wait(&mut visual, &shell);
        visual.read(|cx| {
            let shell = shell.read(cx);
            assert_eq!(shell.commit_records().count(), 0, "{phase}");
            assert!(
                !shell
                    .buffers()
                    .any(|buffer| buffer.mode() == ModeId::MAGIT_DIFF),
                "{phase}"
            );
            assert!(
                !matches!(
                    shell.session().map(|session| session.kind()),
                    Some(ReadKind::CommitAll { .. })
                ),
                "{phase}"
            );
        });
    }
}
