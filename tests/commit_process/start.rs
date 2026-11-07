use super::*;

#[gpui_kit::test]
fn isolated_commit_start_ui_child(cx: &mut gpui_kit::TestAppContext) {
    if std::env::var_os("SEMACS_COMMIT_START_CHILD").is_none() {
        return;
    }
    use gpui_kit::{VisualTestContext, px, size};
    use semacs::app::{AppShell, CommitPhase};
    use semacs::buffer::Buffer;
    use semacs::files::{DecodedFile, FileFormat};
    use semacs::mode::ModeId;
    use semacs::session::ReadKind;

    cx.background_executor.allow_parking();
    cx.update(semacs::app::init);
    for phase in [
        "all",
        "all-status",
        "no",
        "quit",
        "staged",
        "untracked",
        "empty",
        "unborn-ita",
        "missing-helper",
    ] {
        eprintln!("commit start phase: {phase}");
        let fixture = Fixture::new();
        if phase == "unborn-ita" {
            fixture.git(&["rm", "--cached", "a"]);
            fixture.git(&["add", "-N", "a"]);
        } else {
            fixture.write("b", "base b\n\ncontext\n");
            fixture.git(&["config", "diff.suppressBlankEmpty", "true"]);
            fixture.git(&["add", "b"]);
            fixture.git(&["commit", "-qm", "initial"]);
            if !matches!(phase, "empty" | "untracked") {
                fixture.write("a", "worktree a\n");
                fixture.write("b", "worktree b\n\ncontext\n");
            }
            if phase == "staged" {
                fixture.git(&["add", "a"]);
                fixture.write("a", "later a\n");
            }
        }
        if phase != "empty" {
            fixture.write("untracked", "do not add me\n");
        }
        let before = fixture.head();
        let window = cx.open_window(size(px(960.), px(640.)), |window, cx| {
            let mut shell = AppShell::new(
                Buffer::new(
                    Some(fixture.repository.root.join("a")),
                    DecodedFile {
                        text: "unsaved editor only\n".into(),
                        format: FileFormat::default(),
                    },
                ),
                window,
                cx,
            );
            if phase != "missing-helper" {
                shell.set_commit_runtime(semacs::commit::Runtime {
                    executable: PathBuf::from(env!("CARGO_BIN_EXE_semacs")),
                    directory: fixture.base.clone(),
                });
            }
            shell
        });
        let shell = window.entity(cx).unwrap();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        visual.update(|window, cx| {
            window.activate_window();
            window.draw(cx).clear(cx);
        });
        if phase == "all-status" {
            visual.simulate_keystrokes("alt-x");
            visual.simulate_input("magit-status");
            visual.simulate_keystrokes("enter");
            pump(&mut visual, |visual| {
                visual.read(|cx| {
                    shell.read(cx).active_view().read(cx).buffer().mode() == ModeId::MAGIT_STATUS
                })
            });
        }
        visual.simulate_keystrokes("alt-x");
        visual.simulate_input("magit-commit-create");
        visual.simulate_keystrokes("enter");
        if matches!(phase, "empty" | "untracked" | "missing-helper") {
            let message = if phase == "missing-helper" {
                "Git editor helper is not configured for this application"
            } else {
                "Nothing staged (or unstaged)"
            };
            pump(&mut visual, |visual| {
                visual.read(|cx| shell.read(cx).active_view().read(cx).message() == Some(message))
            });
            visual.read(|cx| {
                assert!(shell.read(cx).session().is_none());
                assert_eq!(shell.read(cx).commit_records().count(), 0);
            });
            assert_eq!(fixture.head(), before);
            continue;
        }
        if phase != "staged" {
            pump(&mut visual, |visual| {
                visual.read(|cx| {
                    shell
                        .read(cx)
                        .session()
                        .is_some_and(|session| matches!(session.kind(), ReadKind::CommitAll { .. }))
                })
            });
            visual.read(|cx| {
                let view = shell.read(cx).active_view().read(cx);
                assert_eq!(view.buffer().mode(), ModeId::MAGIT_DIFF);
                let text = view.buffer().text().to_string();
                assert!(text.contains(if phase == "unborn-ita" {
                    "+staged"
                } else {
                    "+worktree a"
                }));
                assert!(!text.contains("do not add me"));
                assert_eq!(shell.read(cx).commit_records().count(), 0);
            });
            visual.simulate_keystrokes(match phase {
                "no" => "n",
                "quit" => "ctrl-g",
                _ => "y",
            });
            if matches!(phase, "no" | "quit") {
                visual.read(|cx| {
                    assert!(shell.read(cx).session().is_none());
                    assert_eq!(shell.read(cx).commit_records().count(), 0);
                    assert_eq!(
                        shell.read(cx).active_view().read(cx).buffer().mode() == ModeId::MAGIT_DIFF,
                        phase == "quit"
                    );
                });
                assert_eq!(fixture.head(), before);
                assert!(fixture.git(&["diff", "--cached", "--name-only"]).is_empty());
                continue;
            }
        }
        pump(&mut visual, |visual| {
            visual.read(|cx| {
                shell.read(cx).active_view().read(cx).buffer().mode() == ModeId::GIT_COMMIT
            })
        });
        pump(&mut visual, |visual| {
            visual.read(|cx| {
                shell.read(cx).buffers().any(|buffer| {
                    buffer.mode() == ModeId::MAGIT_DIFF
                        && buffer.text().to_string().starts_with(if phase == "staged" {
                            "staged: "
                        } else {
                            "worktree: "
                        })
                        && buffer
                            .text()
                            .to_string()
                            .contains(if phase == "unborn-ita" {
                                "+staged"
                            } else {
                                "+worktree a"
                            })
                })
            })
        });
        visual.simulate_input("entry message\n\n");
        visual.simulate_keystrokes("ctrl-c ctrl-c");
        pump(&mut visual, |visual| {
            visual.read(|cx| {
                shell
                    .read(cx)
                    .commit_records()
                    .last()
                    .is_some_and(|record| !record.phase.is_active())
            })
        });
        visual.read(|cx| {
            let record = shell.read(cx).commit_records().last().unwrap();
            assert!(
                matches!(record.phase, CommitPhase::Exited(status) if status.success()),
                "{:?}: {}",
                record.phase,
                String::from_utf8_lossy(&record.output)
            );
            assert!(record.diagnostics.is_empty());
        });
        assert_eq!(
            fixture.git(&["log", "-1", "--format=%B"]),
            b"entry message\n\n"
        );
        assert_eq!(
            fixture.git(&["show", "HEAD:a"]),
            if phase == "unborn-ita" {
                b"staged\n".as_slice()
            } else {
                b"worktree a\n".as_slice()
            }
        );
        if phase != "unborn-ita" {
            assert_eq!(
                fixture.git(&["show", "HEAD:b"]),
                if phase == "staged" {
                    b"base b\n\ncontext\n".as_slice()
                } else {
                    b"worktree b\n\ncontext\n".as_slice()
                }
            );
        }
        if phase == "all-status" {
            pump(&mut visual, |visual| {
                visual.read(|cx| {
                    shell.read(cx).buffers().any(|buffer| {
                        buffer.mode() == ModeId::MAGIT_STATUS && !buffer.git_is_stale()
                    })
                })
            });
            visual.read(|cx| {
                let status = shell
                    .read(cx)
                    .buffers()
                    .find_map(|buffer| buffer.status_data())
                    .unwrap();
                assert_eq!(status.group(semacs::status::Group::Staged).count(), 0);
                assert_eq!(status.group(semacs::status::Group::Unstaged).count(), 0);
                assert_eq!(status.group(semacs::status::Group::Untracked).count(), 1);
                assert_eq!(
                    shell.read(cx).active_view().read(cx).buffer().mode(),
                    ModeId::MAGIT_STATUS
                );
            });
        }
        assert_eq!(fixture.git(&["ls-files", "untracked"]), b"");
        assert_eq!(
            fs::read_to_string(fixture.repository.root.join("a")).unwrap(),
            if phase == "unborn-ita" {
                "staged\n"
            } else if phase == "staged" {
                "later a\n"
            } else {
                "worktree a\n"
            }
        );
        assert!(fixture.git(&["diff", "--cached", "--name-only"]).is_empty());
        assert!(fs::read_dir(&fixture.base).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .as_encoded_bytes()
                .starts_with(b".semacs-editor-")
        }));
    }
}

#[test]
fn commit_command_matches_upstream_staged_and_ask_to_stage_flow() {
    isolated_test(
        "starts::isolated_commit_start_ui_child",
        "SEMACS_COMMIT_START_CHILD",
    );
}
