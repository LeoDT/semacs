use super::*;

#[gpui::test]
fn switching_comparison_cannot_rediscover_a_parent_repository(cx: &mut TestAppContext) {
    let parent = Repository::new();
    parent.write("p", "parent base\n");
    parent.commit();
    parent.write("p", "parent staged\n");
    parent.git(&["add", "p"]);
    let child = Repository(parent.0.join("child"));
    std::fs::create_dir_all(&child.0).unwrap();
    child.git(&["init", "-q"]);
    child.git(&["config", "user.name", "Fixture"]);
    child.git(&["config", "user.email", "fixture@example.invalid"]);
    child.write("a", "old\n");
    child.commit();
    child.write("a", "new\n");
    let (mut visual, shell) = open(&child, cx);
    command(&mut visual, &shell, Command::MagitDiffUnstaged);
    wait(&mut visual, &shell);
    let origin = visual.read(|cx| shell.read(cx).active_view().clone());
    let text = visual.read(|cx| origin.read(cx).buffer().text().to_string());
    std::fs::rename(child.0.join(".git"), child.0.join("hidden-git")).unwrap();
    command(&mut visual, &shell, Command::MagitDiffStaged);
    wait(&mut visual, &shell);
    visual.read(|cx| {
        assert_eq!(shell.read(cx).active_view(), &origin);
        assert_eq!(origin.read(cx).buffer().text().to_string(), text);
        assert_eq!(
            origin
                .read(cx)
                .buffer()
                .comparison()
                .unwrap()
                .repository
                .root,
            child.0
        );
        assert!(origin.read(cx).message().unwrap().contains("Git failed"));
    });
}

#[gpui::test]
fn cancelled_read_and_roundtrip_focus_invalidate_pending_opens(cx: &mut TestAppContext) {
    let repo = Repository::new();
    repo.write("a", "old\n");
    repo.commit();
    repo.write("a", "new\n");
    let (mut visual, shell) = open(&repo, cx);
    let origin = visual.read(|cx| shell.read(cx).active_view().clone());
    for read in [true, false] {
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.run_command(Command::MagitDiffUnstaged.into(), &origin, window, cx);
                if read {
                    shell.run_command(Command::ExecuteExtendedCommand.into(), &origin, window, cx);
                    shell.execute_session(Command::AbortMinibuffers, window, cx);
                } else {
                    let index = shell.create_buffer(Buffer::named("other"), window, cx);
                    shell.activate(index, window, cx);
                    shell.activate(0, window, cx);
                }
            })
        });
        wait(&mut visual, &shell);
        visual.read(|cx| {
            assert_eq!(shell.read(cx).active_view(), &origin);
            assert!(
                !shell
                    .read(cx)
                    .buffers
                    .iter()
                    .any(|slot| slot.buffer.mode() == ModeId::MAGIT_DIFF)
            );
        });
    }
    command(&mut visual, &shell, Command::MagitDiffStaged);
    wait(&mut visual, &shell);
    visual.read(|cx| {
        assert_eq!(
            shell.read(cx).active_view().read(cx).buffer().mode(),
            ModeId::MAGIT_DIFF
        )
    });
}

#[gpui::test]
fn cancelled_read_invalidates_pending_blob_visit(cx: &mut TestAppContext) {
    let repo = Repository::new();
    repo.write("a", "old\n");
    repo.commit();
    repo.write("a", "staged\n");
    repo.git(&["add", "a"]);
    let (mut visual, shell) = open(&repo, cx);
    command(&mut visual, &shell, Command::MagitDiffStaged);
    wait(&mut visual, &shell);
    let origin = visual.read(|cx| shell.read(cx).active_view().clone());
    visual.update(|window, cx| {
        origin.update(cx, |view, cx| {
            let offset = view.buffer().text().to_string().find("+staged").unwrap();
            view.show_location(crate::text::ByteOffset(offset), window, cx);
        });
        shell.update(cx, |shell, cx| {
            shell.run_command(Command::MagitDiffVisit.into(), &origin, window, cx);
            shell.run_command(Command::ExecuteExtendedCommand.into(), &origin, window, cx);
            shell.execute_session(Command::AbortMinibuffers, window, cx);
        });
    });
    wait(&mut visual, &shell);
    visual.read(|cx| assert_eq!(shell.read(cx).active_view(), &origin));
}
