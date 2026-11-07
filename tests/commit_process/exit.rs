use super::*;

#[gpui_kit::test]
fn isolated_exit_ui_child(cx: &mut gpui_kit::TestAppContext) {
    if std::env::var_os("SEMACS_COMMIT_EXIT_CHILD").is_none() {
        return;
    }
    use gpui_kit::{VisualTestContext, px, size};
    use semacs::app::{AppShell, CommitPhase};
    use semacs::buffer::Buffer;
    use semacs::mode::ModeId;
    use semacs::session::ReadKind;

    cx.background_executor.allow_parking();
    cx.update(semacs::app::init);
    for phase in ["starting", "hook", "editor", "resume", "after-editor"] {
        let fixture = Fixture::new();
        if phase == "starting" || phase == "hook" {
            fixture.hook(
                "pre-commit",
                "echo $PPID > .git/git-pid; echo BLOCKED; while :; do sleep 1; done",
            );
        } else if phase == "after-editor" {
            fixture.hook("commit-msg", "echo BLOCKED; while :; do sleep 1; done");
        }
        let window = cx.open_window(size(px(960.), px(640.)), |window, cx| {
            AppShell::new(Buffer::named("origin"), window, cx)
        });
        let shell = window.entity(cx).unwrap();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        visual.update(|window, cx| {
            window.activate_window();
            window.draw(cx).clear(cx);
        });
        let operation = visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.start_commit(
                    fixture.repository.clone(),
                    PathBuf::from(env!("CARGO_BIN_EXE_semacs")),
                    fixture.base.clone(),
                    window,
                    cx,
                )
            })
        });
        if phase == "hook" {
            pump(&mut visual, |visual| {
                visual.read(|cx| {
                    String::from_utf8_lossy(&shell.read(cx).commit_records().last().unwrap().output)
                        .contains("BLOCKED")
                })
            });
        } else if phase != "starting" {
            pump(&mut visual, |visual| {
                visual.read(|cx| {
                    shell.read(cx).active_view().read(cx).buffer().mode() == ModeId::GIT_COMMIT
                })
            });
            visual.simulate_input("exit draft\n");
            if phase == "after-editor" {
                visual.simulate_keystrokes("ctrl-c ctrl-c");
                pump(&mut visual, |visual| {
                    visual.read(|cx| {
                        String::from_utf8_lossy(
                            &shell.read(cx).commit_records().last().unwrap().output,
                        )
                        .contains("BLOCKED")
                    })
                });
            }
        }
        visual.simulate_keystrokes("ctrl-x ctrl-c");
        if phase == "editor" || phase == "resume" {
            visual.read(|cx| {
                assert!(matches!(
                    shell.read(cx).session().unwrap().kind(),
                    ReadKind::ExitSave { .. }
                ))
            });
            visual.simulate_keystrokes("y");
        }
        pump(&mut visual, |visual| {
            visual.read(|cx| {
                shell
                    .read(cx)
                    .session()
                    .is_some_and(|s| s.kind() == &ReadKind::ExitActiveCommits)
            })
        });
        if phase == "resume" {
            visual.simulate_input("no");
            visual.simulate_keystrokes("enter");
            visual.read(|cx| {
                assert!(shell.read(cx).session().is_none());
                assert!(
                    shell
                        .read(cx)
                        .commit_records()
                        .last()
                        .unwrap()
                        .phase
                        .is_active()
                );
                assert_eq!(
                    shell.read(cx).active_view().read(cx).buffer().mode(),
                    ModeId::GIT_COMMIT
                );
            });
            visual.simulate_input("continued ");
            visual.simulate_keystrokes("ctrl-c ctrl-c");
            pump(&mut visual, |visual| {
                visual.read(|cx| {
                    !shell
                        .read(cx)
                        .commit_records()
                        .last()
                        .unwrap()
                        .phase
                        .is_active()
                })
            });
            visual.read(|cx| {
                assert!(
                    matches!(shell.read(cx).commit_records().last().unwrap().phase,
                    CommitPhase::Exited(status) if status.success())
                );
            });
            visual.simulate_keystrokes("ctrl-x ctrl-c");
        } else {
            visual.simulate_input("yes");
            visual.simulate_keystrokes("enter");
            pump(&mut visual, |visual| visual.windows().is_empty());
            visual.read(|cx| {
                let record = shell
                    .read(cx)
                    .commit_records()
                    .find(|r| r.operation == operation)
                    .unwrap();
                assert!(matches!(record.phase, CommitPhase::Exited(status)
                    if status.signal() == Some(libc::SIGHUP) || status.code() == Some(128 + libc::SIGHUP)));
                assert!(record.diagnostics.is_empty(), "{:?}", record.diagnostics);
                assert!(
                    shell
                        .read(cx)
                        .buffers()
                        .all(|b| b.mode() != ModeId::GIT_COMMIT)
                );
            });
            assert!(fixture.head().is_none());
            if phase == "editor" || phase == "after-editor" {
                assert!(
                    fs::read_to_string(fixture.repository.git_dir.join("COMMIT_EDITMSG"))
                        .unwrap()
                        .starts_with("exit draft\n")
                );
            }
        }
        assert!(visual.windows().is_empty());
        assert!(fs::read_dir(&fixture.base).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".semacs-editor-")
        }));
    }
}

#[test]
fn normal_exit_preserves_declined_editors_and_reaps_confirmed_git_before_closing() {
    isolated_test("exit::isolated_exit_ui_child", "SEMACS_COMMIT_EXIT_CHILD");
}
