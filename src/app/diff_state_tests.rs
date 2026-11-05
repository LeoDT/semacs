use super::*;

fn wait_reload(visual: &mut VisualTestContext, shell: &Entity<AppShell>) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        visual.run_until_parked();
        if visual.read(|cx| shell.read(cx).reload_task.is_none()) {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[gpui::test]
fn folds_positions_reload_and_failed_refresh_are_shared_without_reset(cx: &mut TestAppContext) {
    let repo = Repository::new();
    let base = (0..150)
        .map(|row| format!("line {row}\n"))
        .collect::<String>();
    repo.write("a", &base);
    repo.commit();
    repo.write(
        "a",
        base.replace("line 10\n", "first changed\n")
            .replace("line 100\n", "second changed\n"),
    );
    let (mut visual, shell) = open(&repo, cx);
    command(&mut visual, &shell, Command::MagitDiffUnstaged);
    wait(&mut visual, &shell);
    let first = visual.read(|cx| shell.read(cx).active_view().clone());
    command(&mut visual, &shell, Command::SplitWindowRight);
    visual.update(|window, cx| window.draw(cx).clear(cx));
    command(&mut visual, &shell, Command::OtherWindow);
    let second = visual.read(|cx| shell.read(cx).active_view().clone());
    assert_ne!(first, second);
    let sections = visual.read(|cx| second.read(cx).buffer().sections().unwrap());
    let hunks: Vec<_> = sections
        .nodes()
        .iter()
        .filter(|node| node.key.contains("/@@"))
        .map(|node| (node.range.start, node.body))
        .collect();
    assert_eq!(hunks.len(), 2);
    visual.update(|window, cx| {
        first.update(cx, |view, cx| view.show_location(hunks[0].1, window, cx));
        second.update(cx, |view, cx| view.show_location(hunks[1].0, window, cx));
    });
    command(&mut visual, &shell, Command::SectionToggle);
    visual.update(|window, cx| window.draw(cx).clear(cx));
    visual.read(|cx| {
        assert_eq!(first.read(cx).buffer().point(), hunks[0].1);
        assert!(
            first
                .read(cx)
                .buffer()
                .sections()
                .unwrap()
                .nodes()
                .iter()
                .find(|node| node.range.start == hunks[1].0)
                .unwrap()
                .collapsed
        );
    });
    let before = visual.read(|cx| {
        (
            first.read(cx).buffer().point(),
            second.read(cx).buffer().point(),
            first.read(cx).viewport_layout().unwrap().scroll(),
            second.read(cx).viewport_layout().unwrap().scroll(),
        )
    });
    command(&mut visual, &shell, Command::MagitRefresh);
    wait(&mut visual, &shell);
    visual.read(|cx| {
        assert_eq!(
            (
                first.read(cx).buffer().point(),
                second.read(cx).buffer().point()
            ),
            (before.0, before.1)
        );
        assert_eq!(
            (
                first.read(cx).viewport_layout().unwrap().scroll(),
                second.read(cx).viewport_layout().unwrap().scroll()
            ),
            (before.2, before.3)
        );
    });
    let init = repo.0.join("init.js");
    std::fs::write(
        &init,
        "semacs.bindKey('n', 'section-backward', {mode:'magit-diff'});",
    )
    .unwrap();
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.config_path = Some(init.clone());
            shell.reload_configuration(&second, window, cx);
        })
    });
    visual.simulate_keystrokes("ctrl-x");
    wait_reload(&mut visual, &shell);
    visual.read(|cx| {
        assert_eq!(first.read(cx).buffer().point(), before.0);
        assert!(
            first
                .read(cx)
                .buffer()
                .sections()
                .unwrap()
                .nodes()
                .iter()
                .find(|node| node.range.start == hunks[1].0)
                .unwrap()
                .collapsed
        );
    });
    visual.simulate_keystrokes("n");
    visual.read(|cx| assert_eq!(second.read(cx).buffer().point(), hunks[0].0));
    let generation = visual.read(|cx| shell.read(cx).config_generation);
    std::fs::write(
        &init,
        "semacs.bindKey('n', 'section-forward', {mode:'missing'});",
    )
    .unwrap();
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.reload_configuration(&second, window, cx)
        })
    });
    wait_reload(&mut visual, &shell);
    visual.read(|cx| assert_eq!(shell.read(cx).config_generation, generation));
    let text = visual.read(|cx| first.read(cx).buffer().text().to_string());
    let git = repo.0.join(".git");
    let hidden = repo.0.join("hidden-git");
    std::fs::rename(&git, &hidden).unwrap();
    command(&mut visual, &shell, Command::MagitRefresh);
    wait(&mut visual, &shell);
    visual.read(|cx| {
        assert_eq!(first.read(cx).buffer().text().to_string(), text);
        assert!(
            second
                .read(cx)
                .message()
                .unwrap()
                .contains("Diff unchanged")
        );
    });
    std::fs::rename(hidden, git).unwrap();
}

#[gpui::test]
fn unstaged_visit_reuses_editable_worktree_and_mode_keys_stay_out_of_minibuffer(
    cx: &mut TestAppContext,
) {
    let repo = Repository::new();
    repo.write("a", "old\n");
    repo.commit();
    repo.write("a", "new\n");
    let (mut visual, shell) = open(&repo, cx);
    let worktree = visual.read(|cx| shell.read(cx).active_view().clone());
    command(&mut visual, &shell, Command::MagitDiffUnstaged);
    wait(&mut visual, &shell);
    let diff = visual.read(|cx| shell.read(cx).active_view().clone());
    visual.update(|window, cx| {
        diff.update(cx, |view, cx| {
            let offset = view.buffer().text().to_string().find("+new").unwrap();
            view.show_location(crate::text::ByteOffset(offset + 2), window, cx);
        })
    });
    command(&mut visual, &shell, Command::MagitDiffVisit);
    wait(&mut visual, &shell);
    visual.read(|cx| {
        assert_eq!(shell.read(cx).active_view(), &worktree);
        assert!(!worktree.read(cx).buffer().is_read_only());
        assert_eq!(
            worktree.read(cx).buffer().point(),
            crate::text::ByteOffset(1)
        );
    });
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let index = shell.buffer_index(&diff, cx).unwrap();
            shell.activate(index, window, cx);
        })
    });
    command(&mut visual, &shell, Command::ExecuteExtendedCommand);
    visual.simulate_input("n");
    visual.read(|cx| {
        let editor = shell
            .read(cx)
            .minibuffer
            .as_ref()
            .unwrap()
            .view
            .read(cx)
            .editor();
        assert!(editor.read(cx).buffer().text().to_string().ends_with('n'));
    });
}
