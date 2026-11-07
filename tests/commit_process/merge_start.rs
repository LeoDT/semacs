use super::*;

#[gpui_kit::test]
fn isolated_commit_merge_start_ui_child(cx: &mut gpui_kit::TestAppContext) {
    if std::env::var_os("SEMACS_COMMIT_MERGE_CHILD").is_none() {
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
    for phase in ["clean", "yes", "no", "quit"] {
        let fixture = Fixture::new();
        fixture.git(&["commit", "-qm", "initial"]);
        fixture.git(&["checkout", "-qb", "other"]);
        fixture.git(&["commit", "--allow-empty", "-qm", "side"]);
        fixture.git(&["checkout", "-q", "main"]);
        fixture.git(&["merge", "--no-ff", "--no-commit", "other"]);
        if phase != "clean" {
            fixture.write("a", "unstaged\n");
        }
        let before = fixture.head();
        let window = cx.open_window(size(px(960.), px(640.)), |window, cx| {
            let mut shell = AppShell::new(
                Buffer::new(
                    Some(fixture.repository.root.join("a")),
                    DecodedFile {
                        text: "editor only\n".into(),
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
        visual.simulate_input("magit-commit-create");
        visual.simulate_keystrokes("enter");
        if phase != "clean" {
            pump(&mut visual, |visual| {
                visual.read(|cx| {
                    matches!(
                        shell.read(cx).session().map(|session| session.kind()),
                        Some(ReadKind::CommitMerge { .. })
                    )
                })
            });
            visual.simulate_keystrokes(match phase {
                "yes" => "y",
                "no" => "n",
                _ => "ctrl-g",
            });
            if phase != "yes" {
                visual.read(|cx| {
                    assert!(shell.read(cx).session().is_none());
                    assert_eq!(shell.read(cx).commit_records().count(), 0);
                });
                assert_eq!(fixture.head(), before);
                assert!(fixture.repository.git_dir.join("MERGE_HEAD").exists());
                continue;
            }
        }
        pump(&mut visual, |visual| {
            visual.read(|cx| {
                shell.read(cx).active_view().read(cx).buffer().mode() == ModeId::GIT_COMMIT
            })
        });
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
        visual.read(|cx| assert!(matches!(shell.read(cx).commit_records().last().unwrap().phase, CommitPhase::Exited(status) if status.success())));
        assert!(!fixture.repository.git_dir.join("MERGE_HEAD").exists());
        assert_eq!(fixture.git(&["show", "HEAD:a"]), b"staged\n");
        assert_eq!(
            String::from_utf8(fixture.git(&["log", "-1", "--format=%P"]))
                .unwrap()
                .split_whitespace()
                .count(),
            2
        );
        if phase == "yes" {
            assert_eq!(
                fs::read_to_string(fixture.repository.root.join("a")).unwrap(),
                "unstaged\n"
            );
        }
    }
}

#[test]
fn normal_commit_preserves_upstream_merge_confirmation_without_staging_the_worktree() {
    isolated_test(
        "merge_starts::isolated_commit_merge_start_ui_child",
        "SEMACS_COMMIT_MERGE_CHILD",
    );
}
