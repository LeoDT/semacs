use super::*;

#[gpui_kit::test]
fn isolated_hangup_ui_child(cx: &mut gpui_kit::TestAppContext) {
    if std::env::var_os("SEMACS_COMMIT_HANGUP_CHILD").is_none() {
        return;
    }
    use gpui_kit::{VisualTestContext, px, size};
    use semacs::app::{AppShell, CommitPhase};
    use semacs::buffer::Buffer;
    use semacs::files::{DecodedFile, FileFormat};
    use semacs::mode::ModeId;
    use semacs::session::ReadKind;

    let behavior = std::env::var("SEMACS_TEST_HUP").unwrap();
    let fixture = Fixture::new();
    cx.background_executor.allow_parking();
    cx.update(semacs::app::init);
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
            String::from_utf8_lossy(&shell.read(cx).commit_records().last().unwrap().output)
                .contains("BLOCKED")
        })
    });
    if behavior == "edit" {
        visual.simulate_input("discarded text");
    }
    visual.simulate_keystrokes("ctrl-x ctrl-c");
    if behavior == "edit" {
        visual.simulate_keystrokes("n");
        visual.simulate_input("yes");
        visual.simulate_keystrokes("enter");
    }
    visual.read(|cx| {
        assert_eq!(
            shell.read(cx).session().unwrap().kind(),
            &ReadKind::ExitActiveCommits
        )
    });
    visual.simulate_input("yes");
    visual.simulate_keystrokes("enter");
    if behavior != "ignore" {
        pump(&mut visual, |_| {
            fixture.repository.git_dir.join("hup-count").exists()
        });
    }
    let deadline = Instant::now() + Duration::from_millis(80);
    while Instant::now() < deadline {
        visual.run_until_parked();
        std::thread::yield_now();
    }
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
    });
    assert_eq!(visual.windows().len(), 1);
    if behavior == "edit" {
        visual.simulate_input("new text after discard confirmation");
        fixture.write(".git/release", "");
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
                    .contains("new text after discard confirmation")
            );
        });
        visual.simulate_input("yes");
        visual.simulate_keystrokes("enter");
    } else if behavior == "catch" {
        assert_eq!(
            fs::read_to_string(fixture.repository.git_dir.join("hup-count")).unwrap(),
            "HUP\n"
        );
        visual.simulate_keystrokes("ctrl-g ctrl-x ctrl-c");
        visual.read(|cx| {
            assert_eq!(
                shell.read(cx).session().unwrap().kind(),
                &ReadKind::ExitActiveCommits
            )
        });
        visual.simulate_input("yes");
        visual.simulate_keystrokes("enter");
        pump(&mut visual, |_| {
            fs::read_to_string(fixture.repository.git_dir.join("hup-count")).unwrap()
                == "HUP\nHUP\n"
        });
        fixture.write(".git/release", "");
    } else {
        visual.simulate_keystrokes("alt-x");
        visual.simulate_input("magit-process-buffer");
        visual.simulate_keystrokes("enter");
        pump(&mut visual, |visual| {
            visual.read(|cx| {
                shell.read(cx).active_view().read(cx).buffer().mode() == ModeId::MAGIT_PROCESS
            })
        });
        visual.simulate_keystrokes("k");
        if behavior == "ignore" {
            visual.simulate_keystrokes("y");
            visual.read(|cx| {
                assert!(
                    shell
                        .read(cx)
                        .commit_records()
                        .last()
                        .unwrap()
                        .phase
                        .is_active()
                )
            });
            visual.simulate_keystrokes("k");
        } else {
            fixture.write(".git/release", "");
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
                assert!(matches!(
                    shell.read(cx).session().unwrap().kind(),
                    ReadKind::CommitInterrupt { .. }
                ))
            });
            visual.simulate_keystrokes("n");
        }
    }
    pump(&mut visual, |visual| visual.windows().is_empty());
    visual.read(|cx| {
        let record = shell.read(cx).commit_records().last().unwrap();
        assert!(record.diagnostics.is_empty(), "{:?}", record.diagnostics);
        let CommitPhase::Exited(status) = record.phase else {
            panic!("{:?}", record.phase)
        };
        if behavior == "ignore" {
            assert_eq!(status.signal(), Some(libc::SIGKILL));
        } else {
            assert_eq!(status.code(), Some(23));
        }
    });
    assert!(fs::read_dir(&fixture.base).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".semacs-editor-")
    }));
}

#[test]
fn normal_exit_hangup_can_be_caught_or_ignored_without_automatic_force_kill() {
    let path = runtime().join(format!("exit-signal-bin-{}", std::process::id()));
    fs::create_dir_all(&path).unwrap();
    let real_git = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|directory| directory.join("git"))
        .find(|file| file.is_file())
        .unwrap()
        .canonicalize()
        .unwrap();
    let quoted_git = format!("'{}'", real_git.to_str().unwrap().replace('\'', "'\\''"));
    let script = format!(
        r#"#!/bin/sh
set -u
last=
for arg do last=$arg; done
if test "$last" != commit; then exec {quoted_git} "$@"; fi
trap '' INT
if test "$SEMACS_TEST_HUP" = ignore; then
    trap '' HUP
else
    trap 'printf "HUP\n" >> .git/hup-count' HUP
fi
echo BLOCKED
while test ! -e .git/release; do sleep 0.01; done
exit 23
"#
    );
    fs::write(path.join("git"), script).unwrap();
    fs::set_permissions(path.join("git"), fs::Permissions::from_mode(0o755)).unwrap();
    let mut paths = vec![path.clone()];
    paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap()));
    let search_path = std::env::join_paths(paths).unwrap();
    for behavior in ["catch", "ignore", "query", "edit"] {
        isolated_test_using(
            "exit_signals::isolated_hangup_ui_child",
            "SEMACS_COMMIT_HANGUP_CHILD",
            |command| {
                command
                    .env("PATH", &search_path)
                    .env("SEMACS_TEST_HUP", behavior);
            },
        );
    }
    fs::remove_dir_all(path).unwrap();
}
