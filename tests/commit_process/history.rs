use super::*;

#[gpui_kit::test]
fn isolated_history_commit_ui_child(cx: &mut gpui_kit::TestAppContext) {
    if std::env::var_os("SEMACS_HISTORY_COMMIT_CHILD").is_none() {
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
        "amend",
        "reword",
        "amend-empty",
        "reword-empty",
        "fixup",
        "squash",
        "fixup-all",
        "fixup-hook",
        "squash-failure",
    ] {
        eprintln!("F5 phase: {phase}");
        let fixture = Fixture::new();
        fixture.git(&["commit", "-qm", "base"]);
        let old_head = fixture.head().unwrap();
        let old_tree = fixture.git(&["rev-parse", "HEAD^{tree}"]);
        if !phase.ends_with("-empty") {
            fixture.write("a", "indexed\n");
            if phase != "fixup-all" {
                fixture.git(&["add", "a"]);
            }
            fixture.write("a", "worktree\n");
        }
        fixture.write("untracked", "do not commit\n");
        let index = fixture.git(&["write-tree"]);
        if phase == "fixup-hook" {
            fixture.hook(
                "pre-commit",
                "echo ran > .git/hook-ran\nprintf '%s' \"$GIT_EDITOR\" > .git/hook-editor",
            );
        }
        if phase == "squash-failure" {
            fixture.hook("pre-commit", "echo EXPECTED_HOOK_FAILURE >&2\nexit 1");
        }
        let window = cx.open_window(size(px(960.), px(640.)), |window, cx| {
            let mut shell = AppShell::new(
                Buffer::new(
                    Some(fixture.repository.root.join("a")),
                    DecodedFile {
                        text: "unsaved text\n".into(),
                        format: FileFormat::default(),
                    },
                ),
                window,
                cx,
            );
            shell.set_commit_runtime(semacs::commit::Runtime {
                executable: PathBuf::from(env!("CARGO_BIN_EXE_semacs")),
                directory: fixture.base.clone(),
            });
            shell
        });
        let shell = window.entity(cx).unwrap();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        visual.update(|window, cx| {
            window.activate_window();
            window.draw(cx).clear(cx);
        });
        visual.simulate_keystrokes("alt-x");
        visual.simulate_input("magit-status");
        visual.simulate_keystrokes("enter");
        pump(&mut visual, |visual| {
            visual.read(|cx| {
                shell.read(cx).active_view().read(cx).buffer().mode() == ModeId::MAGIT_STATUS
            })
        });
        visual.simulate_keystrokes("c");
        visual.read(|cx| {
            assert!(matches!(
                shell.read(cx).session().map(|session| session.kind()),
                Some(ReadKind::CommitMenu)
            ))
        });
        let amend = phase.starts_with("amend");
        let reword = phase.starts_with("reword");
        visual.simulate_keystrokes(if amend {
            "a"
        } else if reword {
            "w"
        } else if phase.starts_with("squash") {
            "s"
        } else {
            "f"
        });
        if phase == "fixup-all" {
            pump(&mut visual, |visual| {
                visual.read(|cx| {
                    matches!(
                        shell.read(cx).session().map(|session| session.kind()),
                        Some(ReadKind::CommitAll { .. })
                    )
                })
            });
            visual.simulate_keystrokes("y");
        }
        if !amend && !reword {
            pump(&mut visual, |visual| {
                visual.read(|cx| {
                    shell.read(cx).active_view().read(cx).buffer().mode()
                        == ModeId::MAGIT_LOG_SELECT
                })
            });
            visual.read(|cx| assert_eq!(shell.read(cx).commit_records().count(), 0));
            visual.simulate_keystrokes("ctrl-c ctrl-c");
        }
        if amend || reword {
            pump(&mut visual, |visual| {
                visual.read(|cx| {
                    shell.read(cx).active_view().read(cx).buffer().mode() == ModeId::GIT_COMMIT
                })
            });
            visual.simulate_keystrokes("alt-x");
            visual.simulate_input("magit-commit");
            visual.simulate_keystrokes("enter");
            visual.read(|cx| {
                assert!(shell.read(cx).session().is_none());
                assert_eq!(
                    shell.read(cx).active_view().read(cx).buffer().mode(),
                    ModeId::GIT_COMMIT
                );
            });
            if amend || reword {
                pump(&mut visual, |visual| {
                    visual.read(|cx| {
                        shell.read(cx).buffers().any(|buffer| {
                            if reword {
                                buffer.mode() == ModeId::MAGIT_REVISION
                                    && buffer.text().to_string().contains("+staged")
                            } else {
                                buffer.mode() == ModeId::MAGIT_DIFF
                                    && buffer.text().to_string().starts_with("amend: ")
                                    && buffer.text().to_string().contains(
                                        if phase.ends_with("-empty") {
                                            "+staged"
                                        } else {
                                            "+indexed"
                                        },
                                    )
                            }
                        })
                    })
                });
                visual.simulate_keystrokes("alt-< ctrl-k");
                visual.simulate_input("revised message");
            }
            visual.simulate_keystrokes("ctrl-c ctrl-c");
        }
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
            assert!(matches!(record.phase, CommitPhase::Exited(status) if status.success() == (phase != "squash-failure")), "{phase}: {:?} {}", record.phase, String::from_utf8_lossy(&record.output));
            assert!(record.diagnostics.is_empty(), "{phase}: {:?}", record.diagnostics);
        });
        if phase == "squash-failure" {
            assert_eq!(fixture.head().unwrap(), old_head);
            assert_eq!(fixture.git(&["write-tree"]), index);
        } else if amend || reword {
            assert_eq!(fixture.git(&["rev-list", "--count", "HEAD"]), b"1\n");
            assert_eq!(
                fixture.git(&["log", "-1", "--format=%s"]),
                b"revised message\n"
            );
            assert_eq!(
                fixture.git(&["rev-parse", "HEAD^{tree}"]),
                if reword { old_tree } else { index.clone() }
            );
            assert_eq!(fixture.git(&["write-tree"]), index);
        } else {
            assert_eq!(fixture.git(&["rev-list", "--count", "HEAD"]), b"2\n");
            assert_eq!(
                fixture.git(&["log", "-1", "--format=%s"]),
                if phase.starts_with("squash") {
                    b"squash! base\n".as_slice()
                } else {
                    b"fixup! base\n".as_slice()
                }
            );
            assert_eq!(
                fixture.git(&["show", "HEAD:a"]),
                if phase == "fixup-all" {
                    b"worktree\n".as_slice()
                } else {
                    b"indexed\n".as_slice()
                }
            );
            assert!(fixture.git(&["diff", "--cached", "--name-only"]).is_empty());
        }
        assert_eq!(
            fs::read_to_string(fixture.repository.root.join("a")).unwrap(),
            if phase.ends_with("-empty") {
                "staged\n"
            } else {
                "worktree\n"
            }
        );
        assert!(fixture.git(&["ls-files", "untracked"]).is_empty());
        if phase == "fixup-hook" {
            assert_eq!(
                fs::read_to_string(fixture.repository.git_dir.join("hook-ran")).unwrap(),
                "ran\n"
            );
            assert_eq!(
                fs::read_to_string(fixture.repository.git_dir.join("hook-editor")).unwrap(),
                ":"
            );
        }
        assert!(fs::read_dir(&fixture.base).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .as_encoded_bytes()
                .starts_with(b".semacs-editor-")
        }));
    }
}

#[gpui_kit::test]
fn isolated_alternate_object_selection_child(cx: &mut gpui_kit::TestAppContext) {
    if std::env::var_os("SEMACS_HISTORY_OBJECTS_CHILD").is_none() {
        return;
    }
    use gpui_kit::{VisualTestContext, px, size};
    use semacs::app::{AppShell, CommitPhase};
    use semacs::buffer::Buffer;
    use semacs::files::{DecodedFile, FileFormat};
    use semacs::mode::ModeId;
    cx.background_executor.allow_parking();
    cx.update(semacs::app::init);
    for command in ["magit-commit-fixup", "magit-commit-squash"] {
        let fixture = Fixture::new();
        fixture.git(&["commit", "-qm", "base"]);
        fixture.write("a", "indexed\n");
        fixture.git(&["add", "a"]);
        let without_objects = Command::new("git")
            .env_remove("GIT_OBJECT_DIRECTORY")
            .arg("-C")
            .arg(&fixture.repository.root)
            .arg("--git-dir")
            .arg(&fixture.repository.git_dir)
            .arg("--work-tree")
            .arg(&fixture.repository.root)
            .args(["cat-file", "-e", "HEAD"])
            .output()
            .unwrap();
        assert!(!without_objects.status.success());
        let window = cx.open_window(size(px(960.), px(640.)), |window, cx| {
            let mut shell = AppShell::new(
                Buffer::new(
                    Some(fixture.repository.root.join("a")),
                    DecodedFile {
                        text: "unsaved\n".into(),
                        format: FileFormat::default(),
                    },
                ),
                window,
                cx,
            );
            shell.set_commit_runtime(semacs::commit::Runtime {
                executable: PathBuf::from(env!("CARGO_BIN_EXE_semacs")),
                directory: fixture.base.clone(),
            });
            shell
        });
        let shell = window.entity(cx).unwrap();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        visual.update(|window, cx| {
            window.activate_window();
            window.draw(cx).clear(cx);
        });
        visual.simulate_keystrokes("alt-x");
        visual.simulate_input(command);
        visual.simulate_keystrokes("enter");
        pump(&mut visual, |visual| {
            visual.read(|cx| {
                shell.read(cx).active_view().read(cx).buffer().mode() == ModeId::MAGIT_LOG_SELECT
            })
        });
        if command.ends_with("fixup") {
            visual.simulate_keystrokes("enter");
            pump(&mut visual, |visual| {
                visual.read(|cx| {
                    shell.read(cx).active_view().read(cx).buffer().mode() == ModeId::MAGIT_REVISION
                })
            });
            let row = visual.read(|cx| {
                let text = shell
                    .read(cx)
                    .active_view()
                    .read(cx)
                    .buffer()
                    .text()
                    .to_string();
                let offset = text.find("+staged").unwrap();
                text[..offset].bytes().filter(|byte| *byte == b'\n').count()
            });
            visual.simulate_keystrokes("alt-<");
            for _ in 0..row {
                visual.simulate_keystrokes("ctrl-n");
            }
            visual.simulate_keystrokes("enter");
            pump(&mut visual, |visual| {
                visual.read(|cx| {
                    shell
                        .read(cx)
                        .active_view()
                        .read(cx)
                        .buffer()
                        .text()
                        .to_string()
                        == "staged\n"
                })
            });
            visual.update(|window, cx| {
                shell.update(cx, |shell, cx| {
                    let index = shell
                        .buffers()
                        .position(|buffer| buffer.mode() == ModeId::MAGIT_LOG_SELECT)
                        .unwrap();
                    shell.activate(index, window, cx);
                })
            });
            visual.simulate_keystrokes("n");
        }
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
        });
        assert_eq!(fixture.git(&["show", "HEAD:a"]), b"indexed\n");
        assert_eq!(
            fixture.git(&["log", "-1", "--format=%s"]),
            if command.ends_with("fixup") {
                b"fixup! base\n".as_slice()
            } else {
                b"squash! base\n".as_slice()
            }
        );
    }
}

#[test]
fn fixup_and_squash_keep_alternate_objects_through_selection_revision_and_blob() {
    let objects = runtime().join(format!("f5-alternate-objects-{}", std::process::id()));
    fs::create_dir(&objects).unwrap();
    isolated_test_using(
        "history::isolated_alternate_object_selection_child",
        "SEMACS_HISTORY_OBJECTS_CHILD",
        |command| {
            command.env("GIT_OBJECT_DIRECTORY", &objects);
        },
    );
    fs::remove_dir_all(objects).unwrap();
}

#[test]
fn history_commit_actions_use_real_git_editor_hooks_and_index_semantics() {
    isolated_test(
        "history::isolated_history_commit_ui_child",
        "SEMACS_HISTORY_COMMIT_CHILD",
    );
}
