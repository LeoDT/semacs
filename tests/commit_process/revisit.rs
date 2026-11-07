use super::*;

#[gpui_kit::test]
fn isolated_revisited_message_and_owned_process_child(cx: &mut gpui_kit::TestAppContext) {
    if std::env::var_os("SEMACS_COMMIT_REVISIT_CHILD").is_none() {
        return;
    }
    use gpui_kit::{VisualTestContext, px, size};
    use semacs::app::{AppShell, CommitPhase};
    use semacs::buffer::Buffer;
    use semacs::mode::ModeId;
    use semacs::session::ReadKind;

    cx.background_executor.allow_parking();
    cx.update(semacs::app::init);
    for phase in ["revisit", "linked", "hook-path", "terminal"] {
        let mut fixture = Fixture::new();
        if phase == "linked" {
            fixture.git(&["commit", "-qm", "initial"]);
            let linked = fixture.base.join("linked");
            fixture.git(&[
                "worktree",
                "add",
                "--quiet",
                "-b",
                "linked",
                linked.to_str().unwrap(),
            ]);
            fixture.repository.root = linked;
            let directory = fixture.git(&["rev-parse", "--absolute-git-dir"]);
            fixture.repository.git_dir = PathBuf::from(OsString::from_vec(
                directory.strip_suffix(b"\n").unwrap().to_vec(),
            ));
            fixture.write("a", "linked staged\n");
            fixture.git(&["add", "a"]);
        }
        if phase == "hook-path" {
            fixture.hook("prepare-commit-msg",
                "printf 'hook subject\\n' > ../hook-message\neval \"$GIT_EDITOR ../hook-message\"\ncat ../hook-message > \"$1\"");
        } else {
            fixture.hook(
                "prepare-commit-msg",
                "echo \"$PPID\" > .git/git-pid\nprintf 'new actual subject\\n' > \"$1\"",
            );
        }
        let message_path = fixture.repository.git_dir.join("COMMIT_EDITMSG");
        let buffer = if matches!(phase, "revisit" | "terminal") {
            fs::write(&message_path, "old visible subject\n").unwrap();
            let loaded = semacs::files::load(&message_path).unwrap();
            Buffer::visiting(message_path, loaded.file, loaded.stamp)
        } else {
            let path = fixture.repository.root.join("a");
            let loaded = semacs::files::load(&path).unwrap();
            Buffer::visiting(path, loaded.file, loaded.stamp)
        };
        let window = cx.open_window(size(px(960.), px(640.)), |window, cx| {
            AppShell::new(buffer, window, cx)
        });
        let shell = window.entity(cx).unwrap();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        visual.update(|window, cx| {
            window.activate_window();
            window.draw(cx).clear(cx);
            shell.update(cx, |shell, cx| {
                shell.start_commit(
                    fixture.repository.clone(),
                    PathBuf::from(env!("CARGO_BIN_EXE_semacs")),
                    fixture.base.clone(),
                    window,
                    cx,
                );
            });
        });
        pump(&mut visual, |visual| {
            visual.read(|cx| {
                shell.read(cx).active_view().read(cx).buffer().mode() == ModeId::GIT_COMMIT
            })
        });
        if matches!(phase, "revisit" | "terminal") {
            visual.read(|cx| {
                assert!(matches!(
                    shell.read(cx).session().unwrap().kind(),
                    ReadKind::CommitReload {
                        modified: false,
                        ..
                    }
                ))
            });
            if phase == "terminal" {
                let pid: i32 = fs::read_to_string(fixture.repository.git_dir.join("git-pid"))
                    .unwrap()
                    .trim()
                    .parse()
                    .unwrap();
                assert_eq!(unsafe { libc::kill(pid, libc::SIGTERM) }, 0);
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
                visual.read(|cx| assert!(shell.read(cx).session().is_none()));
                continue;
            }
            visual.simulate_input("yes");
            visual.simulate_keystrokes("enter");
            visual.read(|cx| {
                assert_eq!(
                    shell
                        .read(cx)
                        .active_view()
                        .read(cx)
                        .buffer()
                        .text()
                        .to_string(),
                    "new actual subject\n"
                )
            });
        }
        pump(&mut visual, |visual| {
            visual.read(|cx| {
                shell.read(cx).buffers().any(|buffer| {
                    buffer.mode() == ModeId::MAGIT_DIFF && buffer.text().to_string().contains("+")
                })
            })
        });
        for mode in [ModeId::MAGIT_DIFF, ModeId::GIT_COMMIT] {
            for _ in 0..4 {
                visual.simulate_keystrokes("ctrl-x o");
                if visual.read(|cx| shell.read(cx).active_view().read(cx).buffer().mode() == mode) {
                    break;
                }
            }
            visual
                .read(|cx| assert_eq!(shell.read(cx).active_view().read(cx).buffer().mode(), mode));
        }
        if phase == "linked" {
            visual.simulate_input("new actual subject\n\n");
        }
        visual.simulate_keystrokes("alt-x");
        visual.simulate_input("magit-process-buffer");
        visual.simulate_keystrokes("enter");
        pump(&mut visual, |visual| {
            visual.read(|cx| {
                shell.read(cx).active_view().read(cx).buffer().mode() == ModeId::MAGIT_PROCESS
            })
        });
        visual.read(|cx| {
            assert!(
                shell
                    .read(cx)
                    .active_view()
                    .read(cx)
                    .buffer()
                    .text()
                    .to_string()
                    .contains(
                        &fixture
                            .repository
                            .root
                            .to_string_lossy()
                            .escape_debug()
                            .to_string()
                    )
            )
        });
        visual.simulate_keystrokes("q");
        visual.read(|cx| {
            assert_eq!(
                shell.read(cx).active_view().read(cx).buffer().mode(),
                ModeId::GIT_COMMIT
            )
        });
        visual.simulate_keystrokes("ctrl-c ctrl-c");
        if phase == "hook-path" {
            pump(&mut visual, |visual| {
                visual.read(|cx| {
                    shell
                        .read(cx)
                        .buffers()
                        .any(|buffer| buffer.mode() == ModeId::GIT_COMMIT)
                })
            });
            let name = visual.read(|cx| {
                shell
                    .read(cx)
                    .buffers()
                    .find(|buffer| buffer.mode() == ModeId::GIT_COMMIT)
                    .unwrap()
                    .display_name()
            });
            visual.simulate_keystrokes("ctrl-x b");
            visual.simulate_input(&name);
            visual.simulate_keystrokes("enter ctrl-c ctrl-c");
        }
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
            CommitPhase::Exited(status) if status.success()),
                "{phase}: {}",
                String::from_utf8_lossy(&shell.read(cx).commit_records().last().unwrap().output)
            )
        });
        assert_eq!(
            fixture.git(&["log", "-1", "--format=%s"]),
            if phase == "hook-path" {
                b"hook subject\n".as_slice()
            } else {
                b"new actual subject\n".as_slice()
            }
        );
    }
}

#[test]
fn revisited_editor_and_process_commands_follow_the_owned_session_not_the_message_directory() {
    isolated_test(
        "revisits::isolated_revisited_message_and_owned_process_child",
        "SEMACS_COMMIT_REVISIT_CHILD",
    );
}
