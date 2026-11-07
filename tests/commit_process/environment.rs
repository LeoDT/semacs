use super::*;

#[gpui_kit::test]
fn isolated_commit_environment_ui_child(cx: &mut gpui_kit::TestAppContext) {
    if std::env::var_os("SEMACS_COMMIT_ENV_CHILD").is_none() {
        return;
    }
    use gpui_kit::{VisualTestContext, px, size};
    use semacs::app::{AppShell, CommitPhase};
    use semacs::buffer::Buffer;
    use semacs::files::{DecodedFile, FileFormat};
    use semacs::mode::ModeId;

    cx.background_executor.allow_parking();
    cx.update(semacs::app::init);
    let fixture = Fixture::new();
    assert!(!fixture.repository.git_dir.join("index").exists());
    let window = cx.open_window(size(px(960.), px(640.)), |window, cx| {
        let mut shell = AppShell::new(
            Buffer::new(
                Some(fixture.repository.root.join("a")),
                DecodedFile {
                    text: "unsaved editor text\n".into(),
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
    pump(&mut visual, |visual| {
        visual
            .read(|cx| shell.read(cx).active_view().read(cx).buffer().mode() == ModeId::GIT_COMMIT)
    });
    pump(&mut visual, |visual| {
        visual.read(|cx| {
            shell.read(cx).buffers().any(|buffer| {
                buffer.mode() == ModeId::MAGIT_DIFF && buffer.text().to_string().contains("+staged")
            })
        })
    });
    for _ in 0..4 {
        visual.simulate_keystrokes("ctrl-x o");
        if visual
            .read(|cx| shell.read(cx).active_view().read(cx).buffer().mode() == ModeId::MAGIT_DIFF)
        {
            break;
        }
    }
    fixture.write("a", "restaged\n");
    fixture.git(&["add", "a"]);
    visual.simulate_keystrokes("g");
    pump(&mut visual, |visual| {
        visual.read(|cx| {
            shell
                .read(cx)
                .active_view()
                .read(cx)
                .buffer()
                .text()
                .to_string()
                .contains("+restaged")
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
                .contains("+restaged")
        )
    });
    for _ in 0..4 {
        visual.simulate_keystrokes("ctrl-x o");
        if visual
            .read(|cx| shell.read(cx).active_view().read(cx).buffer().mode() == ModeId::GIT_COMMIT)
        {
            break;
        }
    }
    visual.simulate_input("alternate index\n\n");
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
    assert_eq!(fixture.git(&["show", "HEAD:a"]), b"staged\n");
    assert!(!fixture.repository.git_dir.join("index").exists());
}

#[test]
fn commit_preparation_editor_diff_and_refresh_keep_the_inherited_index() {
    let index = runtime().join(format!("commit-inherited-index-{}", std::process::id()));
    isolated_test_using(
        "starts_environment::isolated_commit_environment_ui_child",
        "SEMACS_COMMIT_ENV_CHILD",
        |command| {
            command.env("GIT_INDEX_FILE", &index);
        },
    );
    fs::remove_file(index).unwrap();
}
