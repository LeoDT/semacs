mod support;

use gpui_kit as gpui;
use gpui_kit::TestAppContext;
use semacs::buffer::Buffer;
use std::fs;
use support::*;

#[gpui::test]
fn exit_cancel_on_a_non_active_buffer_keeps_the_active_view_focused(cx: &mut TestAppContext) {
    let dir = TempDir::new("exit-focus");
    let alpha = dir.path().join("alpha.txt");
    let beta = dir.path().join("beta.txt");
    fs::write(&alpha, b"alpha").unwrap();
    fs::write(&beta, b"beta").unwrap();
    let (mut visual, shell, alpha_view) = open(buffer_at(&alpha, "alpha"), cx);
    let beta_index = create(&mut visual, &shell, "beta", Some(&beta));
    activate(&mut visual, &shell, beta_index);
    visual.simulate_input("Y");
    activate(&mut visual, &shell, 0);
    visual.simulate_input("X");
    redraw(&mut visual);

    visual.simulate_keystrokes("ctrl-x ctrl-c");
    redraw(&mut visual);
    assert!(prompt(&mut visual, &shell).contains("alpha.txt"));
    visual.simulate_keystrokes("n");
    redraw(&mut visual);
    assert!(
        prompt(&mut visual, &shell).contains("beta.txt"),
        "the second prompt targets the non-active dirty buffer"
    );

    visual.simulate_keystrokes("ctrl-g");
    redraw(&mut visual);
    visual.read(|cx| {
        assert!(shell.read(cx).session().is_none());
        assert_eq!(shell.read(cx).active_index(), 0);
        assert_eq!(alpha_view.read(cx).message(), Some("Quit"));
    });
    assert_focused(&mut visual, &alpha_view);

    visual.simulate_input("Z");
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(alpha_view.read(cx).buffer().text().to_string(), "XZalpha");
        assert_eq!(
            shell
                .read(cx)
                .buffer_view(beta_index)
                .unwrap()
                .read(cx)
                .buffer()
                .text()
                .to_string(),
            "Ybeta"
        );
    });
}

#[gpui::test]
fn deleting_a_file_then_saving_recreates_it_without_an_external_change_prompt(
    cx: &mut TestAppContext,
) {
    let dir = TempDir::new("deleted");
    let path = dir.path().join("file.txt");
    fs::write(&path, b"base").unwrap();
    let (mut visual, shell, view) = open(buffer_at(&path, "base"), cx);
    visual.simulate_input("X");
    fs::remove_file(&path).unwrap();
    redraw(&mut visual);

    visual.simulate_keystrokes("ctrl-x ctrl-s");
    redraw(&mut visual);
    visual.read(|cx| {
        assert!(shell.read(cx).session().is_none());
        assert_eq!(
            view.read(cx).buffer().path().as_deref(),
            Some(path.as_path())
        );
        assert!(!view.read(cx).buffer().is_modified());
    });
    assert_eq!(fs::read(&path).unwrap(), b"Xbase");
}

#[gpui::test]
fn save_buffer_prompts_for_a_name_for_an_unvisited_buffer(cx: &mut TestAppContext) {
    let dir = TempDir::new("save-as");
    let target = dir.path().join("saved.txt");
    let (mut visual, shell, view) = open(Buffer::scratch(), cx);
    visual.simulate_input("dirty");
    redraw(&mut visual);

    visual.simulate_keystrokes("ctrl-x ctrl-s");
    redraw(&mut visual);
    assert_eq!(prompt(&mut visual, &shell), "Write file: ");
    type_path(&mut visual, &target);
    visual.simulate_keystrokes("enter");
    redraw(&mut visual);
    visual.read(|cx| {
        assert!(shell.read(cx).session().is_none());
        let buffer = view.read(cx).buffer();
        assert_eq!(buffer.path().as_deref(), Some(target.as_path()));
        assert_eq!(buffer.display_name(), "saved.txt");
        assert!(!buffer.is_modified());
    });
    assert_eq!(fs::read(&target).unwrap(), b"dirty");

    // The saved identity is reused; only the document and *Messages* exist.
    visual.simulate_keystrokes("ctrl-x ctrl-f");
    type_path(&mut visual, &target);
    visual.simulate_keystrokes("enter");
    redraw(&mut visual);
    visual.read(|cx| assert_eq!(shell.read(cx).buffer_count(), 2));

    // A second save of a clean buffer does not open another read session.
    visual.simulate_keystrokes("ctrl-x ctrl-s");
    redraw(&mut visual);
    visual.read(|cx| {
        assert!(shell.read(cx).session().is_none());
        assert_eq!(view.read(cx).message(), Some("No changes need to be saved"));
    });
}

#[gpui::test]
fn find_file_reuses_the_file_object_for_links_and_dotdot(cx: &mut TestAppContext) {
    let dir = TempDir::new("identity");
    let real = dir.path().join("real.txt");
    let link = dir.path().join("link.txt");
    fs::write(&real, b"base").unwrap();
    std::os::unix::fs::symlink("real.txt", &link).unwrap();
    let (mut visual, shell, _) = open(Buffer::scratch(), cx);

    visual.simulate_keystrokes("ctrl-x ctrl-f");
    type_path(&mut visual, &real);
    visual.simulate_keystrokes("enter");
    let first = active_view(&mut visual, &shell);
    visual.read(|cx| assert_eq!(shell.read(cx).buffer_count(), 2));

    visual.simulate_keystrokes("ctrl-x ctrl-f");
    type_path(&mut visual, &link);
    visual.simulate_keystrokes("enter");
    let linked = active_view(&mut visual, &shell);
    assert_eq!(first, linked, "a symlink must reuse its target buffer");
    visual.read(|cx| {
        assert_eq!(shell.read(cx).buffer_count(), 2);
        assert_eq!(
            linked.read(cx).buffer().path().as_deref(),
            Some(real.as_path())
        );
    });

    let spelling = format!(
        "{}/../{}/real.txt",
        dir.path().display(),
        dir.path().file_name().unwrap().to_string_lossy()
    );
    visual.simulate_keystrokes("ctrl-x ctrl-f");
    visual.simulate_input(&spelling);
    visual.simulate_keystrokes("enter");
    let dotted = active_view(&mut visual, &shell);
    assert_eq!(first, dotted, "a/../ spelling must reuse the buffer");
    visual.read(|cx| assert_eq!(shell.read(cx).buffer_count(), 2));
}

#[gpui::test]
fn find_file_resolves_relative_paths_against_the_buffer_directory(cx: &mut TestAppContext) {
    let dir = TempDir::new("relative");
    let one = dir.path().join("one");
    fs::create_dir_all(&one).unwrap();
    let main = one.join("main.txt");
    let other = one.join("other.txt");
    fs::write(&main, b"main").unwrap();
    fs::write(&other, b"other").unwrap();
    let (mut visual, shell, _) = open(buffer_at(&main, "main"), cx);

    visual.simulate_keystrokes("ctrl-x ctrl-f");
    visual.simulate_input("other.txt");
    visual.simulate_keystrokes("enter");
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(shell.read(cx).buffer_count(), 2);
        assert_eq!(
            shell
                .read(cx)
                .active_view()
                .read(cx)
                .buffer()
                .path()
                .as_deref(),
            Some(other.as_path())
        );
    });
}

#[gpui::test]
fn switch_to_buffer_uses_the_other_buffer_default_and_excludes_current(cx: &mut TestAppContext) {
    let dir = TempDir::new("switch-default");
    let alpha = dir.path().join("alpha.txt");
    let beta = dir.path().join("beta.txt");
    fs::write(&alpha, b"alpha").unwrap();
    fs::write(&beta, b"beta").unwrap();
    let (mut visual, shell, _) = open(buffer_at(&alpha, "alpha"), cx);
    let beta_index = create(&mut visual, &shell, "beta", Some(&beta));
    activate(&mut visual, &shell, 0);

    visual.simulate_keystrokes("ctrl-x b");
    redraw(&mut visual);
    assert_eq!(
        prompt(&mut visual, &shell),
        "Switch to buffer (default beta.txt): "
    );
    visual.simulate_keystrokes("tab");
    redraw(&mut visual);
    assert_eq!(contents(&mut visual, &shell), "beta.txt");
    // Clear the unique completion so RET exercises the documented default.
    visual.simulate_keystrokes("ctrl-a ctrl-k");
    assert_eq!(contents(&mut visual, &shell), "");
    visual.simulate_keystrokes("enter");
    redraw(&mut visual);
    visual.read(|cx| assert_eq!(shell.read(cx).active_index(), beta_index));

    visual.simulate_keystrokes("ctrl-x b");
    redraw(&mut visual);
    assert_eq!(
        prompt(&mut visual, &shell),
        "Switch to buffer (default alpha.txt): "
    );
    visual.simulate_keystrokes("enter");
    redraw(&mut visual);
    visual.read(|cx| assert_eq!(shell.read(cx).active_index(), 0));
}

#[gpui::test]
fn kill_buffer_reads_a_name_and_can_kill_a_dirty_non_active_buffer(cx: &mut TestAppContext) {
    let dir = TempDir::new("kill-name");
    let alpha = dir.path().join("alpha.txt");
    let beta = dir.path().join("beta.txt");
    fs::write(&alpha, b"alpha").unwrap();
    fs::write(&beta, b"beta").unwrap();
    let (mut visual, shell, alpha_view) = open(buffer_at(&alpha, "alpha"), cx);
    let _beta_index = create(&mut visual, &shell, "beta", Some(&beta));
    activate(&mut visual, &shell, 0);

    visual.simulate_keystrokes("ctrl-x k");
    redraw(&mut visual);
    assert_eq!(
        prompt(&mut visual, &shell),
        "Kill buffer (default alpha.txt): "
    );
    visual.simulate_input("beta.txt");
    visual.simulate_keystrokes("enter");
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(shell.read(cx).buffer_count(), 1);
        assert_eq!(shell.read(cx).active_index(), 0);
    });

    let beta_index = create(&mut visual, &shell, "beta", Some(&beta));
    activate(&mut visual, &shell, beta_index);
    visual.simulate_input("Y");
    activate(&mut visual, &shell, 0);
    visual.simulate_keystrokes("ctrl-x k");
    visual.simulate_input("beta.txt");
    visual.simulate_keystrokes("enter");
    redraw(&mut visual);
    assert!(
        prompt(&mut visual, &shell).contains("Buffer beta.txt modified; kill anyway? (y, n, s) ")
    );
    visual.simulate_keystrokes("y");
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(shell.read(cx).buffer_count(), 1);
        assert_eq!(shell.read(cx).active_index(), 0);
    });
    assert_focused(&mut visual, &alpha_view);
    assert_eq!(fs::read(&beta).unwrap(), b"beta");
}

#[gpui::test]
fn yes_or_no_prompts_reject_single_letter_answers(cx: &mut TestAppContext) {
    let dir = TempDir::new("answers");
    let path = dir.path().join("file.txt");
    fs::write(&path, b"base").unwrap();
    let (mut visual, shell, view) = open(buffer_at(&path, "base"), cx);
    visual.simulate_input("X");
    fs::write(&path, b"outside").unwrap();
    redraw(&mut visual);

    visual.simulate_keystrokes("ctrl-x ctrl-s");
    redraw(&mut visual);
    assert!(prompt(&mut visual, &shell).contains("(yes or no)"));
    visual.simulate_input("y");
    visual.simulate_keystrokes("enter");
    redraw(&mut visual);
    visual.read(|cx| {
        assert!(shell.read(cx).session().is_some());
        assert_eq!(
            shell
                .read(cx)
                .minibuffer()
                .unwrap()
                .read(cx)
                .editor()
                .read(cx)
                .message(),
            Some("Please answer yes or no")
        );
    });
    visual.simulate_keystrokes("ctrl-a ctrl-k");
    visual.simulate_input("yes");
    visual.simulate_keystrokes("enter");
    redraw(&mut visual);
    visual.read(|cx| {
        assert!(shell.read(cx).session().is_none());
        assert!(!view.read(cx).buffer().is_modified());
    });
    assert_eq!(fs::read(&path).unwrap(), b"Xbase");
}

#[gpui::test]
fn exit_external_change_answered_no_continues_to_the_modified_prompt(cx: &mut TestAppContext) {
    let dir = TempDir::new("exit-external");
    let path = dir.path().join("file.txt");
    fs::write(&path, b"base").unwrap();
    let (mut visual, shell, view) = open(buffer_at(&path, "base"), cx);
    visual.simulate_input("X");
    fs::write(&path, b"outside").unwrap();
    redraw(&mut visual);

    visual.simulate_keystrokes("ctrl-x ctrl-c");
    redraw(&mut visual);
    assert!(prompt(&mut visual, &shell).contains("(y or n)"));
    visual.simulate_keystrokes("y");
    redraw(&mut visual);
    assert!(
        prompt(&mut visual, &shell).contains("has changed since visited or saved"),
        "the saved buffer must ask about the external change"
    );
    visual.simulate_input("no");
    visual.simulate_keystrokes("enter");
    redraw(&mut visual);
    assert!(
        prompt(&mut visual, &shell).contains("Modified buffers exist; exit anyway?"),
        "answering no keeps the buffer dirty and continues to the exit prompt"
    );
    visual.simulate_input("no");
    visual.simulate_keystrokes("enter");
    redraw(&mut visual);
    visual.read(|cx| {
        assert!(shell.read(cx).session().is_none());
        assert!(view.read(cx).buffer().is_modified());
        assert_eq!(shell.read(cx).buffer_count(), 2);
    });
    assert_eq!(visual.windows().len(), 1);
    assert_eq!(fs::read(&path).unwrap(), b"outside");
}

#[gpui::test]
fn exit_overwrite_answered_no_continues_to_the_modified_prompt(cx: &mut TestAppContext) {
    let dir = TempDir::new("exit-overwrite");
    let target = dir.path().join("existing.txt");
    fs::write(&target, b"old").unwrap();
    let (mut visual, shell, view) = open(Buffer::scratch(), cx);
    visual.simulate_input("dirty");
    redraw(&mut visual);

    visual.simulate_keystrokes("ctrl-x ctrl-c");
    redraw(&mut visual);
    assert!(prompt(&mut visual, &shell).contains("Save buffer *scratch*? (y or n)"));
    visual.simulate_keystrokes("y");
    redraw(&mut visual);
    assert_eq!(prompt(&mut visual, &shell), "Write file: ");
    type_path(&mut visual, &target);
    visual.simulate_keystrokes("enter");
    redraw(&mut visual);
    assert!(prompt(&mut visual, &shell).contains("exists; overwrite? (y or n)"));
    visual.simulate_keystrokes("n");
    redraw(&mut visual);
    assert!(
        prompt(&mut visual, &shell).contains("Modified buffers exist; exit anyway?"),
        "declining the overwrite must not leave exit waiting on a dead session"
    );
    visual.simulate_input("no");
    visual.simulate_keystrokes("enter");
    redraw(&mut visual);
    visual.read(|cx| {
        assert!(shell.read(cx).session().is_none());
        assert!(view.read(cx).buffer().is_modified());
    });
    assert_eq!(fs::read(&target).unwrap(), b"old");
    assert_eq!(visual.windows().len(), 1);
}

#[gpui::test]
fn keyboard_only_open_edit_mx_save_switch_exit(cx: &mut TestAppContext) {
    let dir = TempDir::new("full-chain");
    let target = dir.path().join("flow.txt");
    let (mut visual, shell, _) = open(Buffer::scratch(), cx);

    visual.simulate_keystrokes("ctrl-x ctrl-f");
    type_path(&mut visual, &target);
    visual.simulate_keystrokes("enter");
    let file_view = active_view(&mut visual, &shell);
    visual.simulate_input("hello");
    redraw(&mut visual);

    visual.simulate_keystrokes("alt-x");
    visual.simulate_input("save-buffer");
    visual.simulate_keystrokes("enter");
    redraw(&mut visual);
    visual.read(|cx| {
        assert!(shell.read(cx).session().is_none());
        assert!(!file_view.read(cx).buffer().is_modified());
    });
    assert_eq!(fs::read(&target).unwrap(), b"hello");

    visual.simulate_keystrokes("ctrl-x b");
    visual.simulate_input("*scratch*");
    visual.simulate_keystrokes("enter");
    redraw(&mut visual);
    visual.read(|cx| assert_eq!(shell.read(cx).active_index(), 0));

    visual.simulate_keystrokes("ctrl-x b");
    visual.simulate_input("flow.txt");
    visual.simulate_keystrokes("enter");
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(
            shell
                .read(cx)
                .active_view()
                .read(cx)
                .buffer()
                .path()
                .as_deref(),
            Some(target.as_path())
        )
    });

    visual.simulate_keystrokes("ctrl-x ctrl-c");
    visual.run_until_parked();
    assert!(visual.windows().is_empty());
}

#[gpui::test]
fn buffers_share_the_application_kill_ring(cx: &mut TestAppContext) {
    let (mut visual, shell, first) = open(Buffer::scratch(), cx);
    visual.simulate_input("alpha");
    redraw(&mut visual);
    visual.simulate_keystrokes("ctrl-a ctrl-k");
    redraw(&mut visual);

    let second = create(&mut visual, &shell, "", None);
    activate(&mut visual, &shell, second);
    visual.simulate_keystrokes("ctrl-y");
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(
            shell
                .read(cx)
                .active_view()
                .read(cx)
                .buffer()
                .text()
                .to_string(),
            "alpha"
        );
        assert_eq!(first.read(cx).buffer().text().to_string(), "");
    });
}

#[gpui::test]
fn save_queued_behind_save_as_follows_the_new_file(cx: &mut TestAppContext) {
    for visited in [true, false] {
        let dir = TempDir::new("save-as-queue");
        let source = dir.path().join("source.txt");
        let target = dir.path().join("target.txt");
        fs::write(&source, b"base").unwrap();
        let buffer = if visited {
            buffer_at(&source, "base")
        } else {
            Buffer::new(None, decoded("base"))
        };
        let (mut visual, shell, view) = open(buffer, cx);
        visual.simulate_keystrokes("ctrl-e");
        visual.simulate_input("X");
        visual.simulate_keystrokes("ctrl-x ctrl-w");
        type_path(&mut visual, &target);
        dispatch_keys(&mut visual, &["enter"]);
        dispatch_text(&mut visual, "Y");
        dispatch_keys(&mut visual, &["ctrl-x", "ctrl-s"]);
        redraw(&mut visual);

        visual.read(|cx| {
            assert!(shell.read(cx).session().is_none());
            assert_eq!(
                view.read(cx).buffer().path().as_deref(),
                Some(target.as_path())
            );
            assert!(!view.read(cx).buffer().is_modified());
        });
        assert_eq!(fs::read(&source).unwrap(), b"base");
        assert_eq!(fs::read(&target).unwrap(), b"baseXY");
    }
}

#[gpui::test]
fn path_completion_preserves_relative_and_explicit_directory_prefixes(cx: &mut TestAppContext) {
    let dir = TempDir::new("relative-completion");
    let source = dir.path().join("source.txt");
    fs::write(&source, b"source").unwrap();
    fs::write(dir.path().join("alpha.txt"), b"alpha").unwrap();
    let (mut visual, shell, _) = open(buffer_at(&source, "source"), cx);
    for (input, expected) in [("al", "alpha.txt"), ("./al", "./alpha.txt")] {
        visual.simulate_keystrokes("ctrl-x ctrl-f");
        visual.simulate_input(input);
        visual.simulate_keystrokes("tab");
        redraw(&mut visual);
        assert_eq!(contents(&mut visual, &shell), expected);
        visual.simulate_keystrokes("ctrl-g");
    }
}

#[gpui::test]
fn overwrite_accepts_one_key_without_enter_and_restores_body_input(cx: &mut TestAppContext) {
    let dir = TempDir::new("single-key-overwrite");
    let target = dir.path().join("existing.txt");
    fs::write(&target, b"old").unwrap();
    let (mut visual, shell, view) = open(Buffer::scratch(), cx);
    visual.simulate_input("new");
    visual.simulate_keystrokes("ctrl-x ctrl-w");
    type_path(&mut visual, &target);
    visual.simulate_keystrokes("enter");
    redraw(&mut visual);
    assert!(prompt(&mut visual, &shell).contains("(y or n)"));
    visual.simulate_keystrokes("enter tab ctrl-x");
    redraw(&mut visual);
    assert!(prompt(&mut visual, &shell).contains("(y or n)"));
    assert_eq!(contents(&mut visual, &shell), "");
    assert_eq!(fs::read(&target).unwrap(), b"old");
    visual.simulate_keystrokes("y");
    redraw(&mut visual);
    visual.read(|cx| assert!(shell.read(cx).session().is_none()));
    assert_eq!(fs::read(&target).unwrap(), b"new");
    assert_focused(&mut visual, &view);
    visual.simulate_input("X");
    visual.read(|cx| assert_eq!(view.read(cx).buffer().text().to_string(), "newX"));
}

#[gpui::test]
fn failed_save_as_drops_dependent_saves_and_keeps_the_old_file(cx: &mut TestAppContext) {
    let dir = TempDir::new("save-as-failure-queue");
    let source = dir.path().join("source.txt");
    let target = dir.path().join("missing/target.txt");
    fs::write(&source, b"base").unwrap();
    let (mut visual, shell, view) = open(buffer_at(&source, "base"), cx);
    visual.simulate_input("X");
    visual.simulate_keystrokes("ctrl-x ctrl-w");
    type_path(&mut visual, &target);
    dispatch_keys(&mut visual, &["enter"]);
    dispatch_text(&mut visual, "Y");
    dispatch_keys(&mut visual, &["ctrl-x", "ctrl-s"]);
    redraw(&mut visual);
    visual.read(|cx| {
        assert!(shell.read(cx).session().is_none());
        assert_eq!(
            view.read(cx).buffer().path().as_deref(),
            Some(source.as_path())
        );
        assert_eq!(view.read(cx).buffer().text().to_string(), "XYbase");
        assert!(view.read(cx).buffer().is_modified());
        assert!(view.read(cx).message().unwrap().starts_with("Save failed:"));
    });
    assert_eq!(fs::read(&source).unwrap(), b"base");
    assert!(!target.exists());
}

#[gpui::test]
fn canceled_save_as_drops_dependent_saves_and_allows_a_later_save(cx: &mut TestAppContext) {
    let dir = TempDir::new("save-as-cancel-queue");
    let source = dir.path().join("source.txt");
    let target = dir.path().join("target.txt");
    fs::write(&source, b"base").unwrap();
    fs::write(&target, b"outside").unwrap();
    let (mut visual, shell, view) = open(buffer_at(&source, "base"), cx);
    visual.simulate_input("X");
    visual.simulate_keystrokes("ctrl-x ctrl-w");
    type_path(&mut visual, &target);
    dispatch_keys(&mut visual, &["enter"]);
    dispatch_text(&mut visual, "Y");
    dispatch_keys(&mut visual, &["ctrl-x", "ctrl-s"]);
    redraw(&mut visual);
    assert!(prompt(&mut visual, &shell).contains("overwrite?"));
    visual.simulate_keystrokes("ctrl-x ctrl-g");
    redraw(&mut visual);
    visual.read(|cx| {
        assert!(shell.read(cx).session().is_none());
        assert_eq!(
            view.read(cx).buffer().path().as_deref(),
            Some(source.as_path())
        );
        assert!(view.read(cx).buffer().is_modified());
    });
    assert_eq!(fs::read(&source).unwrap(), b"base");
    assert_eq!(fs::read(&target).unwrap(), b"outside");

    visual.simulate_keystrokes("ctrl-x ctrl-s");
    redraw(&mut visual);
    assert_eq!(fs::read(&source).unwrap(), b"XYbase");
    visual.read(|cx| assert!(!view.read(cx).buffer().is_modified()));
}

#[gpui::test]
fn path_completion_ignores_changed_text_and_replaced_sessions(cx: &mut TestAppContext) {
    let dir = TempDir::new("late-completion");
    let source = dir.path().join("source.txt");
    fs::write(&source, b"source").unwrap();
    fs::write(dir.path().join("alpha.txt"), b"alpha").unwrap();
    let (mut visual, shell, _) = open(buffer_at(&source, "source"), cx);
    visual.simulate_keystrokes("ctrl-x ctrl-f");
    visual.update(|window, cx| {
        for key in ["a", "l", "tab", "z"] {
            let mut stroke = gpui_kit::Keystroke::parse(key).unwrap();
            if key != "tab" {
                stroke.key_char = Some(key.to_owned());
            }
            window.dispatch_keystroke(stroke, cx);
        }
    });
    redraw(&mut visual);
    assert_eq!(contents(&mut visual, &shell), "alz");

    visual.simulate_keystrokes("ctrl-a ctrl-k");
    visual.simulate_input("al");
    dispatch_keys(&mut visual, &["tab", "ctrl-g", "ctrl-x", "ctrl-f"]);
    redraw(&mut visual);
    assert_eq!(contents(&mut visual, &shell), "");
    assert_eq!(prompt(&mut visual, &shell), "Find file: ");
}

#[gpui::test]
fn a_late_file_load_cannot_replace_a_new_read_session(cx: &mut TestAppContext) {
    let dir = TempDir::new("late-file");
    let path = dir.path().join("file.txt");
    fs::write(&path, b"file").unwrap();
    let (mut visual, shell, body) = open(Buffer::scratch(), cx);
    visual.simulate_keystrokes("ctrl-x ctrl-f");
    type_path(&mut visual, &path);
    dispatch_keys(&mut visual, &["enter", "alt-x"]);
    redraw(&mut visual);
    assert_eq!(prompt(&mut visual, &shell), "M-x ");
    visual.read(|cx| {
        assert_eq!(shell.read(cx).buffer_count(), 1);
        assert_eq!(shell.read(cx).active_view(), &body);
    });
    visual.simulate_keystrokes("ctrl-g ctrl-x ctrl-f");
    type_path(&mut visual, &path);
    visual.simulate_keystrokes("enter");
    redraw(&mut visual);
    visual.read(|cx| {
        assert!(shell.read(cx).session().is_none());
        assert_eq!(shell.read(cx).buffer_count(), 3);
        assert_eq!(
            shell
                .read(cx)
                .active_view()
                .read(cx)
                .buffer()
                .path()
                .as_deref(),
            Some(path.as_path())
        );
    });
}
