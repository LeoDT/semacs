use super::*;

fn background_input_waits_for_writability_and_stops_with_its_owner() {
    let fixture = Fixture::new();
    fixture.hook(
        "pre-commit",
        "stty -icanon -echo min 1 time 0 < /dev/tty
echo READY
while test ! -e .git/release; do sleep 0.01; done
dd bs=4096 count=64 iflag=fullblock < /dev/tty > .git/input 2>/dev/null
exit 1",
    );
    let (commit, events) = fixture.start();
    let mut output = Vec::new();
    wait_output(&events, &mut output, "READY");
    let input = commit.input().unwrap();
    let (send, receive) = std::sync::mpsc::channel();
    let writer = std::thread::spawn(move || {
        send.send(input.write_all(&vec![b'R'; 262144])).unwrap();
    });
    std::thread::sleep(Duration::from_millis(50));
    assert!(matches!(
        receive.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    ));
    fixture.write(".git/release", "");
    assert!(!finish(commit, &events, &mut output).success());
    receive
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    writer.join().unwrap();
    assert_eq!(
        fs::read(fixture.repository.git_dir.join("input")).unwrap(),
        vec![b'R'; 262144]
    );

    let fixture = Fixture::new();
    fixture.hook(
        "pre-commit",
        "stty -icanon -echo < /dev/tty; echo READY; sleep 1000",
    );
    let (commit, events) = fixture.start();
    wait_output(&events, &mut Vec::new(), "READY");
    let input = commit.input().unwrap();
    let (send, receive) = std::sync::mpsc::channel();
    let writer = std::thread::spawn(move || {
        send.send(input.write_all(&vec![b'X'; 1048576])).unwrap();
    });
    std::thread::sleep(Duration::from_millis(50));
    assert!(matches!(
        receive.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    ));
    drop(events);
    assert!(
        receive
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .is_err()
    );
    commit.join().unwrap();
    writer.join().unwrap();
}

#[gpui_kit::test]
fn isolated_prompt_ui_child(cx: &mut gpui_kit::TestAppContext) {
    if std::env::var_os("SEMACS_COMMIT_PROMPT_CHILD").is_none() {
        return;
    }
    use gpui_kit::{VisualTestContext, px, size};
    use semacs::app::{AppShell, CommitPhase};
    use semacs::buffer::Buffer;
    use semacs::files::{DecodedFile, FileFormat};
    use semacs::mode::ModeId;
    use semacs::session::ReadKind;

    background_input_waits_for_writability_and_stops_with_its_owner();
    cx.background_executor.allow_parking();
    cx.update(semacs::app::init);
    for phase in [
        "answer",
        "foreground",
        "cancel-username",
        "cancel-question",
        "terminal",
        "password",
    ] {
        eprintln!("prompt phase: {phase}");
        let fixture = Fixture::new();
        let answering = matches!(phase, "answer" | "foreground");
        fixture.hook(
            "pre-commit",
            if answering {
                r#"echo "$PPID" > .git/git-pid
exec 3<>/dev/tty
printf 'Continue [Y/' >&3
sleep 0.03
printf 'n] ' >&3
IFS= read -r reply <&3
printf '%s\n' "$reply" > .git/yes
printf '\nAgain (yes/no): ' >&3
IFS= read -r reply <&3
printf '%s\n' "$reply" > .git/no
printf "\nUsername for 'custom': " >&3
IFS= read -r reply <&3
printf '%s\n' "$reply" > .git/custom
printf "\nUsername for 'default': " >&3
IFS= read -r reply <&3
printf '%s\n' "$reply" > .git/default"#
            } else if phase == "password" {
                r#"echo "$PPID" > .git/git-pid
printf 'Password: ' > /dev/tty
IFS= read -r reply < /dev/tty"#
            } else if phase == "cancel-question" {
                r#"echo "$PPID" > .git/git-pid
printf 'Continue [y/n] ' > /dev/tty
IFS= read -r reply < /dev/tty"#
            } else {
                r#"echo "$PPID" > .git/git-pid
printf "Username for 'waiting': " > /dev/tty
IFS= read -r reply < /dev/tty"#
            },
        );
        let window = cx.open_window(size(px(960.), px(640.)), |window, cx| {
            AppShell::new(
                Buffer::new(
                    Some(fixture.repository.root.join("a")),
                    DecodedFile {
                        text: "staged\n".into(),
                        format: FileFormat::default(),
                    },
                ),
                window,
                cx,
            )
        });
        let shell = window.entity(cx).unwrap();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        visual.update(|window, cx| {
            window.activate_window();
            window.draw(cx).clear(cx);
        });
        if phase == "answer" {
            visual.simulate_keystrokes("alt-x");
            visual.simulate_input("reload-configuration");
        }
        let operation = visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.start_commit(
                    fixture.repository.clone(),
                    PathBuf::from(env!("CARGO_BIN_EXE_semacs")),
                    runtime(),
                    window,
                    cx,
                )
            })
        });
        pump(&mut visual, |visual| {
            visual.read(|cx| {
                let record = shell
                    .read(cx)
                    .commit_records()
                    .find(|record| record.operation == operation)
                    .unwrap();
                String::from_utf8_lossy(&record.output).contains(if answering {
                    "[Y/n]"
                } else if phase == "password" {
                    "Password: "
                } else if phase == "cancel-question" {
                    "[y/n]"
                } else {
                    "Username for"
                })
            })
        });
        if answering {
            if phase == "answer" {
                visual.read(|cx| {
                    assert_eq!(shell.read(cx).session().unwrap().kind(), &ReadKind::Command)
                });
                visual.simulate_keystrokes("enter");
            }
            pump(&mut visual, |visual| {
                visual.read(|cx| {
                    matches!(
                        shell.read(cx).session().map(|read| read.kind()),
                        Some(ReadKind::CommitYesNo { .. })
                    )
                })
            });
            let read = visual.read(|cx| shell.read(cx).session().unwrap().id());
            visual.simulate_input("y");
            visual.simulate_keystrokes("enter");
            visual.read(|cx| assert_eq!(shell.read(cx).session().unwrap().id(), read));
            visual.simulate_input("es");
            visual.simulate_keystrokes("enter");
            pump(&mut visual, |visual| {
                visual.read(|cx| {
                    matches!(
                        shell.read(cx).session().map(|read| read.kind()),
                        Some(ReadKind::CommitYesNo { .. })
                    ) && shell.read(cx).session().unwrap().id() != read
                })
            });
            visual.simulate_input("no");
            visual.simulate_keystrokes("enter");
            pump(&mut visual, |visual| {
                visual.read(|cx| matches!(shell.read(cx).session().map(|read| read.kind()), Some(ReadKind::CommitUsername { prompt, .. }) if prompt.contains("custom")))
            });
            visual.simulate_input("fixture-user");
            visual.simulate_keystrokes("enter");
            pump(&mut visual, |visual| {
                visual.read(|cx| matches!(shell.read(cx).session().map(|read| read.kind()), Some(ReadKind::CommitUsername { prompt, .. }) if prompt.contains("default")))
            });
            visual.simulate_keystrokes("enter");
            pump(&mut visual, |visual| {
                visual.read(|cx| {
                    shell
                        .read(cx)
                        .buffers()
                        .any(|buffer| buffer.mode() == ModeId::GIT_COMMIT)
                })
            });
            if phase == "answer" {
                visual.update(|window, cx| {
                    shell.update(cx, |shell, cx| {
                        let index = shell
                            .buffers()
                            .position(|buffer| buffer.mode() == ModeId::GIT_COMMIT)
                            .unwrap();
                        shell.activate(index, window, cx);
                    })
                });
            } else {
                visual.read(|cx| {
                    assert_eq!(
                        shell.read(cx).active_view().read(cx).buffer().mode(),
                        ModeId::GIT_COMMIT
                    )
                });
            }
            visual.simulate_input("prompt commit\n\n");
            visual.simulate_keystrokes("ctrl-c ctrl-c");
        } else if phase == "password" {
            visual.read(|cx| assert!(shell.read(cx).session().is_none()));
            visual.simulate_keystrokes("alt-x");
            visual.simulate_input("magit-process-buffer");
            visual.simulate_keystrokes("enter");
            pump(&mut visual, |visual| {
                visual.read(|cx| {
                    shell.read(cx).active_view().read(cx).buffer().mode() == ModeId::MAGIT_PROCESS
                })
            });
            visual.simulate_keystrokes("alt-< n k y k");
        } else if phase == "terminal" {
            pump(&mut visual, |visual| {
                visual.read(|cx| {
                    matches!(
                        shell.read(cx).session().map(|read| read.kind()),
                        Some(ReadKind::CommitUsername { .. })
                    )
                })
            });
            visual.simulate_input("not sent");
            let pid: i32 = fs::read_to_string(fixture.repository.git_dir.join("git-pid"))
                .unwrap()
                .trim()
                .parse()
                .unwrap();
            assert_eq!(unsafe { libc::kill(-pid, libc::SIGTERM) }, 0);
        } else {
            pump(&mut visual, |visual| {
                visual.read(|cx| {
                    matches!(
                        shell.read(cx).session().map(|read| read.kind()),
                        Some(ReadKind::CommitUsername { .. } | ReadKind::CommitYesNo { .. })
                    )
                })
            });
            visual.simulate_keystrokes("ctrl-g");
        }
        pump(&mut visual, |visual| {
            visual.read(|cx| {
                !shell
                    .read(cx)
                    .commit_records()
                    .find(|record| record.operation == operation)
                    .unwrap()
                    .phase
                    .is_active()
            })
        });
        visual.read(|cx| {
            assert!(shell.read(cx).session().is_none());
            let record = shell
                .read(cx)
                .commit_records()
                .find(|record| record.operation == operation)
                .unwrap();
            assert!(
                matches!(record.phase, CommitPhase::Exited(status) if status.success() == answering)
            );
            assert!(record.diagnostics.is_empty(), "{:?}", record.diagnostics);
            assert!(!String::from_utf8_lossy(&record.output).contains("fixture-user"));
        });
        if answering {
            for (name, expected) in [
                ("yes", "y".to_owned()),
                ("no", "no".to_owned()),
                ("custom", "fixture-user".to_owned()),
                ("default", semacs::commit::login_name()),
            ] {
                assert_eq!(
                    fs::read_to_string(fixture.repository.git_dir.join(name)).unwrap(),
                    format!("{expected}\n")
                );
            }
            assert!(fixture.head().is_some());
        } else {
            assert!(fixture.head().is_none());
        }
    }
}

#[test]
fn actual_non_password_prompts_and_background_input_follow_the_owned_git_process() {
    isolated_test(
        "prompts::isolated_prompt_ui_child",
        "SEMACS_COMMIT_PROMPT_CHILD",
    );
}
