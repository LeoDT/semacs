mod support;

use gpui_kit as gpui;
use gpui_kit::TestAppContext;
use semacs::buffer::Buffer;
use semacs::files::{self, FileFormat, LineEnding};
use std::fs;
use support::*;

#[gpui::test]
fn find_file_edit_save_preserves_bom_crlf_and_the_missing_final_newline(cx: &mut TestAppContext) {
    let dir = TempDir::new("roundtrip");
    let path = dir.path().join("notes.txt");
    let original = [&[0xEF, 0xBB, 0xBF][..], b"a\r\nb"].concat();
    fs::write(&path, &original).unwrap();

    let (mut visual, shell, _) = open(Buffer::scratch(), cx);
    visual.simulate_keystrokes("ctrl-x ctrl-f");
    visual.simulate_input(path.to_str().unwrap());
    visual.simulate_keystrokes("enter");
    let view = active_view(&mut visual, &shell);
    visual.read(|cx| {
        let buffer = view.read(cx).buffer();
        assert_eq!(buffer.path(), Some(path.as_path()));
        assert_eq!(buffer.text().to_string(), "a\nb");
        assert_eq!(
            buffer.format(),
            FileFormat {
                bom: true,
                line_ending: LineEnding::Crlf,
            }
        );
        assert!(!buffer.is_modified());
    });

    visual.simulate_keystrokes("alt->");
    visual.simulate_input("c");
    visual.simulate_keystrokes("enter ctrl-x ctrl-s");
    redraw(&mut visual);
    visual.read(|cx| {
        let buffer = view.read(cx).buffer();
        assert_eq!(buffer.text().to_string(), "a\nbc\n");
        assert!(!buffer.is_modified());
        assert_eq!(
            buffer.format(),
            FileFormat {
                bom: true,
                line_ending: LineEnding::Crlf,
            }
        );
    });
    assert_eq!(
        fs::read(&path).unwrap(),
        [&[0xEF, 0xBB, 0xBF][..], b"a\r\nbc\r\n"].concat(),
        "exactly one BOM, CRLF line breaks, no added or removed final newline"
    );
}

#[gpui::test]
fn saving_plain_lf_keeps_the_existing_final_newline(cx: &mut TestAppContext) {
    let dir = TempDir::new("lf");
    let path = dir.path().join("lf.txt");
    fs::write(&path, b"a\nb\n").unwrap();
    let buffer = Buffer::new(Some(path.clone()), decoded("a\nb\n"));
    let (mut visual, _shell, view) = open(buffer, cx);

    visual.simulate_keystrokes("alt->");
    visual.simulate_input("c");
    visual.simulate_keystrokes("enter ctrl-x ctrl-s");
    redraw(&mut visual);
    visual.read(|cx| {
        let buffer = view.read(cx).buffer();
        assert_eq!(buffer.text().to_string(), "a\nb\nc\n");
        assert_eq!(buffer.format(), FileFormat::default());
        assert!(!buffer.is_modified());
    });
    assert_eq!(fs::read(&path).unwrap(), b"a\nb\nc\n");
}

#[gpui::test]
fn saving_continues_edits_on_the_fixed_snapshot_and_keeps_later_text_dirty(
    cx: &mut TestAppContext,
) {
    let dir = TempDir::new("during-edit");
    let path = dir.path().join("file.txt");
    fs::write(&path, b"base").unwrap();
    let buffer = Buffer::new(Some(path.clone()), decoded("base"));
    let (mut visual, _shell, view) = open(buffer, cx);

    visual.simulate_keystrokes("ctrl-e");
    visual.simulate_input("X");
    redraw(&mut visual);
    // Dispatch the save and the next edit in one frame: the save task can only
    // publish its result when the foreground is pumped afterwards.
    dispatch_keys(&mut visual, &["ctrl-x", "ctrl-s"]);
    dispatch_text(&mut visual, "Z");
    redraw(&mut visual);
    visual.read(|cx| {
        let buffer = view.read(cx).buffer();
        assert_eq!(buffer.text().to_string(), "baseXZ");
        assert!(buffer.is_modified());
    });
    assert_eq!(
        fs::read(&path).unwrap(),
        b"baseX",
        "the save wrote the snapshot captured at its request"
    );

    visual.simulate_keystrokes("ctrl-x ctrl-s");
    redraw(&mut visual);
    visual.read(|cx| {
        let buffer = view.read(cx).buffer();
        assert_eq!(buffer.text().to_string(), "baseXZ");
        assert!(!buffer.is_modified());
    });
    assert_eq!(fs::read(&path).unwrap(), b"baseXZ");
}

#[gpui::test]
fn repeated_save_while_busy_keeps_the_latest_requested_snapshot(cx: &mut TestAppContext) {
    let dir = TempDir::new("pending");
    let path = dir.path().join("file.txt");
    fs::write(&path, b"base").unwrap();
    let buffer = Buffer::new(Some(path.clone()), decoded("base"));
    let (mut visual, _shell, view) = open(buffer, cx);

    visual.simulate_keystrokes("ctrl-e");
    visual.simulate_input("X");
    redraw(&mut visual);
    dispatch_keys(&mut visual, &["ctrl-x", "ctrl-s"]);
    dispatch_text(&mut visual, "Y");
    dispatch_keys(&mut visual, &["ctrl-x", "ctrl-s"]);
    dispatch_text(&mut visual, "Z");
    redraw(&mut visual);
    visual.read(|cx| {
        let buffer = view.read(cx).buffer();
        assert_eq!(buffer.text().to_string(), "baseXYZ");
        assert!(buffer.is_modified());
    });
    assert_eq!(
        fs::read(&path).unwrap(),
        b"baseXY",
        "the queued save used the newest snapshot requested while the first write ran"
    );
}

#[gpui::test]
fn a_second_save_request_during_the_write_also_covers_an_undone_clean_state(
    cx: &mut TestAppContext,
) {
    let dir = TempDir::new("undo-while-saving");
    let path = dir.path().join("file.txt");
    fs::write(&path, b"base").unwrap();
    let buffer = Buffer::new(Some(path.clone()), decoded("base"));
    let (mut visual, _shell, view) = open(buffer, cx);

    visual.simulate_keystrokes("ctrl-e");
    visual.simulate_input("X");
    redraw(&mut visual);
    dispatch_keys(&mut visual, &["ctrl-x", "ctrl-s"]);
    dispatch_keys(&mut visual, &["ctrl-/"]);
    dispatch_keys(&mut visual, &["ctrl-x", "ctrl-s"]);
    redraw(&mut visual);
    visual.read(|cx| {
        let buffer = view.read(cx).buffer();
        assert_eq!(buffer.text().to_string(), "base");
        assert!(!buffer.is_modified());
    });
    assert_eq!(
        fs::read(&path).unwrap(),
        b"base",
        "the request made while the first write ran must not be dropped"
    );
}

#[gpui::test]
fn failed_save_keeps_dirty_path_and_the_target_untouched(cx: &mut TestAppContext) {
    let dir = TempDir::new("failure");
    let path = dir.path().join("target-directory");
    fs::create_dir(&path).unwrap();
    let buffer = Buffer::new(Some(path.clone()), decoded("base"));
    let (mut visual, _shell, view) = open(buffer, cx);
    visual.simulate_input("X");
    redraw(&mut visual);

    visual.simulate_keystrokes("ctrl-x ctrl-s");
    redraw(&mut visual);
    visual.read(|cx| {
        let view = view.read(cx);
        assert_eq!(view.buffer().text().to_string(), "Xbase");
        assert!(view.buffer().is_modified());
        assert_eq!(view.buffer().path(), Some(path.as_path()));
        assert!(
            view.message()
                .unwrap_or_default()
                .starts_with("Save failed:"),
            "{:?}",
            view.message()
        );
    });
    assert!(
        path.is_dir(),
        "the failed rename did not replace the target"
    );
    assert!(
        fs::read_dir(dir.path()).unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains(".semacs-save-")),
        "no temporary file is left behind"
    );
}

#[gpui::test]
fn find_file_reports_read_errors_on_the_origin_without_leaving_a_session(cx: &mut TestAppContext) {
    let dir = TempDir::new("decode-error");
    let path = dir.path().join("binary");
    fs::write(&path, [0xFF, 0x00, 0x41]).unwrap();
    let (mut visual, shell, origin) = open(Buffer::scratch(), cx);
    visual.simulate_input("body");
    visual.simulate_keystrokes("ctrl-x ctrl-f");
    visual.simulate_input(path.to_str().unwrap());
    visual.simulate_keystrokes("enter");
    redraw(&mut visual);
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert!(shell.session().is_none());
        assert!(shell.minibuffer().is_none());
        assert_eq!(shell.active_view(), &origin);
        assert_eq!(origin.read(cx).buffer().text().to_string(), "body");
        assert!(
            origin
                .read(cx)
                .message()
                .unwrap_or_default()
                .contains("file is not valid UTF-8")
        );
    });
}

#[gpui::test]
fn find_file_creates_a_new_empty_buffer_for_a_missing_path(cx: &mut TestAppContext) {
    let dir = TempDir::new("missing");
    let path = dir.path().join("new.txt");
    let (mut visual, shell, _) = open(Buffer::scratch(), cx);
    visual.simulate_keystrokes("ctrl-x ctrl-f");
    visual.simulate_input(path.to_str().unwrap());
    visual.simulate_keystrokes("enter");
    let view = active_view(&mut visual, &shell);
    visual.read(|cx| {
        let buffer = view.read(cx).buffer();
        assert_eq!(buffer.path(), Some(path.as_path()));
        assert_eq!(buffer.text().to_string(), "");
        assert!(!buffer.is_modified());
    });
    assert!(!path.exists());
}

#[test]
fn atomic_write_follows_symlinks_without_replacing_them() {
    let dir = TempDir::new("symlink");
    let real = dir.path().join("real.txt");
    let link = dir.path().join("link.txt");
    fs::write(&real, b"old").unwrap();
    std::os::unix::fs::symlink("real.txt", &link).unwrap();

    files::write_atomic(&link, "new", FileFormat::default()).unwrap();
    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read(&real).unwrap(), b"new");
    assert_eq!(fs::read(&link).unwrap(), b"new");
}

#[test]
fn atomic_write_creates_a_dangling_symlink_target_and_keeps_the_link() {
    let dir = TempDir::new("dangling");
    let real = dir.path().join("missing.txt");
    let link = dir.path().join("link.txt");
    std::os::unix::fs::symlink("missing.txt", &link).unwrap();

    files::write_atomic(&link, "new", FileFormat::default()).unwrap();
    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read(&real).unwrap(), b"new");
}

#[test]
fn failed_atomic_write_leaves_the_original_bytes_and_no_temporary_file() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TempDir::new("atomic-failure");
    let path = dir.path().join("file.txt");
    fs::write(&path, b"old").unwrap();
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o500)).unwrap();

    let probe = dir.path().join(".probe");
    let writable = fs::write(&probe, b"").is_ok();
    let _ = fs::remove_file(&probe);
    if writable {
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
        return;
    }

    let result = files::write_atomic(&path, "new", FileFormat::default());
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
    assert!(
        result.is_err(),
        "a read-only directory must reject the save"
    );
    assert_eq!(fs::read(&path).unwrap(), b"old");
    assert!(fs::read_dir(dir.path()).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains(".semacs-save-")
    }));
}

#[gpui::test]
fn find_file_completes_a_path_and_reuses_the_same_buffer(cx: &mut TestAppContext) {
    let dir = TempDir::new("path-complete");
    let path = dir.path().join("alpha.txt");
    fs::write(&path, b"alpha").unwrap();
    let (mut visual, shell, _) = open(Buffer::scratch(), cx);

    visual.simulate_keystrokes("ctrl-x ctrl-f");
    let prefix = format!("{}/al", dir.path().display());
    visual.simulate_input(&prefix);
    visual.simulate_keystrokes("tab");
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(
            shell.read(cx).minibuffer().unwrap().read(cx).contents(cx),
            path.to_str().unwrap()
        );
    });
    visual.simulate_keystrokes("enter");
    let first = active_view(&mut visual, &shell);
    visual.read(|cx| {
        assert_eq!(first.read(cx).buffer().path(), Some(path.as_path()));
        assert_eq!(shell.read(cx).buffer_count(), 2);
    });

    find_file(&mut visual, &path);
    let second = active_view(&mut visual, &shell);
    assert_eq!(first, second, "the same path must reuse its buffer");
    visual.read(|cx| assert_eq!(shell.read(cx).buffer_count(), 2));
}

#[gpui::test]
fn file_and_non_file_names_are_unique(cx: &mut TestAppContext) {
    let dir = TempDir::new("names");
    let one = dir.path().join("one");
    let two = dir.path().join("two");
    fs::create_dir_all(&one).unwrap();
    fs::create_dir_all(&two).unwrap();
    let (mut visual, shell, _) = open(Buffer::scratch(), cx);
    let first = create(&mut visual, &shell, "", Some(&one.join("same.txt")));
    let second = create(&mut visual, &shell, "", Some(&two.join("same.txt")));
    let scratch_two = create(&mut visual, &shell, "", None);
    redraw(&mut visual);
    visual.read(|cx| {
        let name = |index: usize| {
            shell
                .read(cx)
                .buffer_view(index)
                .unwrap()
                .read(cx)
                .buffer()
                .display_name()
        };
        assert_eq!(name(first), "same.txt");
        assert_eq!(name(second), "same.txt<two>");
        assert_eq!(name(0), "*scratch*");
        assert_eq!(name(scratch_two), "*scratch*<2>");
    });
}

#[gpui::test]
fn switch_to_buffer_completes_and_activates_another_buffer(cx: &mut TestAppContext) {
    let dir = TempDir::new("switch");
    let alpha = dir.path().join("alpha.txt");
    let beta = dir.path().join("beta.txt");
    let (mut visual, shell, _) = open(Buffer::new(Some(alpha.clone()), decoded("a")), cx);
    let beta_index = create(&mut visual, &shell, "b", Some(&beta));
    let alpha_index = 0;
    activate(&mut visual, &shell, alpha_index);

    visual.simulate_keystrokes("ctrl-x b");
    visual.simulate_input("be");
    visual.simulate_keystrokes("tab");
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(
            shell.read(cx).minibuffer().unwrap().read(cx).contents(cx),
            "beta.txt"
        );
    });
    visual.simulate_keystrokes("enter");
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(shell.read(cx).active_index(), beta_index);
        assert_eq!(
            shell.read(cx).active_view().read(cx).buffer().path(),
            Some(beta.as_path())
        );
    });
}

#[gpui::test]
fn kill_buffer_replaces_the_last_buffer_and_confirms_dirty_buffers(cx: &mut TestAppContext) {
    let dir = TempDir::new("kill");
    let alpha = dir.path().join("alpha.txt");
    let beta = dir.path().join("beta.txt");
    fs::write(&alpha, b"alpha").unwrap();
    let (mut visual, shell, first) = open(Buffer::new(Some(alpha.clone()), decoded("alpha")), cx);
    let _second = create(&mut visual, &shell, "beta", Some(&beta));
    activate(&mut visual, &shell, 0);

    visual.simulate_input("X");
    redraw(&mut visual);
    visual.simulate_keystrokes("ctrl-x k");
    redraw(&mut visual);
    visual.read(|cx| {
        assert!(
            shell
                .read(cx)
                .minibuffer()
                .unwrap()
                .read(cx)
                .prompt(cx)
                .contains("Kill buffer (default alpha.txt):")
        );
    });
    visual.simulate_keystrokes("enter");
    redraw(&mut visual);
    visual.read(|cx| {
        assert!(
            shell
                .read(cx)
                .minibuffer()
                .unwrap()
                .read(cx)
                .prompt(cx)
                .contains("modified; kill anyway?")
        );
    });
    visual.simulate_keystrokes("n");
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(shell.read(cx).buffer_count(), 2);
        assert!(first.read(cx).buffer().is_modified());
    });

    visual.simulate_keystrokes("ctrl-x k");
    visual.simulate_keystrokes("enter");
    visual.simulate_keystrokes("y");
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(shell.read(cx).buffer_count(), 1);
        assert_eq!(shell.read(cx).active_index(), 0);
        assert_eq!(
            shell.read(cx).active_view().read(cx).buffer().path(),
            Some(beta.as_path())
        );
    });
    assert_eq!(fs::read(&alpha).unwrap(), b"alpha");
}

#[gpui::test]
fn kill_buffer_can_save_first(cx: &mut TestAppContext) {
    let dir = TempDir::new("kill-save");
    let alpha = dir.path().join("alpha.txt");
    let beta = dir.path().join("beta.txt");
    fs::write(&alpha, b"alpha").unwrap();
    let (mut visual, shell, _) = open(Buffer::new(Some(alpha.clone()), decoded("alpha")), cx);
    let _beta = create(&mut visual, &shell, "beta", Some(&beta));
    activate(&mut visual, &shell, 0);

    visual.simulate_input("X");
    redraw(&mut visual);
    visual.simulate_keystrokes("ctrl-x k");
    visual.simulate_keystrokes("enter");
    visual.simulate_keystrokes("s");
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(shell.read(cx).buffer_count(), 1);
        assert_eq!(shell.read(cx).active_index(), 0);
        assert_eq!(
            shell.read(cx).active_view().read(cx).buffer().path(),
            Some(beta.as_path())
        );
    });
    assert_eq!(fs::read(&alpha).unwrap(), b"Xalpha");
}

#[gpui::test]
fn write_file_writes_bytes_and_updates_the_buffer_identity(cx: &mut TestAppContext) {
    let dir = TempDir::new("write-file");
    let source = dir.path().join("source.txt");
    let target = dir.path().join("target.txt");
    fs::write(&source, b"one").unwrap();
    let buffer = Buffer::new(Some(source.clone()), decoded("one"));
    let (mut visual, _shell, view) = open(buffer, cx);

    visual.simulate_keystrokes("ctrl-e");
    visual.simulate_input("X");
    visual.simulate_keystrokes("ctrl-x ctrl-w");
    visual.simulate_input(target.to_str().unwrap());
    visual.simulate_keystrokes("enter");
    redraw(&mut visual);
    visual.read(|cx| {
        let buffer = view.read(cx).buffer();
        assert_eq!(buffer.path(), Some(target.as_path()));
        assert_eq!(buffer.display_name(), "target.txt");
        assert_eq!(buffer.text().to_string(), "oneX");
        assert!(!buffer.is_modified());
    });
    assert_eq!(fs::read(&source).unwrap(), b"one");
    assert_eq!(fs::read(&target).unwrap(), b"oneX");
}

#[gpui::test]
fn write_file_confirms_existing_targets_and_cancel_keeps_the_old_identity(cx: &mut TestAppContext) {
    let dir = TempDir::new("overwrite");
    let source = dir.path().join("source.txt");
    let target = dir.path().join("target.txt");
    fs::write(&source, b"base").unwrap();
    fs::write(&target, b"old").unwrap();
    let buffer = Buffer::new(Some(source.clone()), decoded("base"));
    let (mut visual, _shell, view) = open(buffer, cx);

    visual.simulate_keystrokes("ctrl-x ctrl-w");
    visual.simulate_input(target.to_str().unwrap());
    visual.simulate_keystrokes("enter");
    redraw(&mut visual);
    visual.read(|cx| {
        let prompt = _shell.read(cx).minibuffer().unwrap().read(cx).prompt(cx);
        assert!(prompt.contains("exists; overwrite?"), "{prompt}");
    });
    visual.simulate_keystrokes("n");
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(view.read(cx).buffer().path(), Some(source.as_path()));
        assert_eq!(view.read(cx).message(), Some("Canceled"));
    });
    assert_eq!(fs::read(&target).unwrap(), b"old");

    visual.simulate_keystrokes("ctrl-x ctrl-w");
    visual.simulate_input(target.to_str().unwrap());
    visual.simulate_keystrokes("enter");
    visual.simulate_keystrokes("y");
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(view.read(cx).buffer().path(), Some(target.as_path()));
        assert!(!view.read(cx).buffer().is_modified());
    });
    assert_eq!(fs::read(&target).unwrap(), b"base");
}

#[gpui::test]
fn externally_changed_files_need_explicit_confirmation(cx: &mut TestAppContext) {
    let dir = TempDir::new("external");
    let path = dir.path().join("file.txt");
    fs::write(&path, b"base").unwrap();
    let buffer = Buffer::new(Some(path.clone()), decoded("base"));
    let (mut visual, _shell, view) = open(buffer, cx);

    visual.simulate_keystrokes("ctrl-e");
    visual.simulate_input("X");
    fs::write(&path, b"outside").unwrap();
    redraw(&mut visual);
    visual.simulate_keystrokes("ctrl-x ctrl-s");
    redraw(&mut visual);
    visual.read(|cx| {
        let prompt = _shell.read(cx).minibuffer().unwrap().read(cx).prompt(cx);
        assert!(prompt.contains("has changed since visited or saved"));
    });
    visual.simulate_input("no");
    visual.simulate_keystrokes("enter");
    redraw(&mut visual);
    visual.read(|cx| {
        assert!(view.read(cx).buffer().is_modified());
        assert_eq!(view.read(cx).buffer().path(), Some(path.as_path()));
    });
    assert_eq!(fs::read(&path).unwrap(), b"outside");

    visual.simulate_keystrokes("ctrl-x ctrl-s");
    visual.simulate_input("yes");
    visual.simulate_keystrokes("enter");
    redraw(&mut visual);
    visual.read(|cx| {
        assert!(!view.read(cx).buffer().is_modified());
        assert_eq!(view.read(cx).buffer().text().to_string(), "baseX");
    });
    assert_eq!(fs::read(&path).unwrap(), b"baseX");
}

#[gpui::test]
fn terminal_exit_asks_each_dirty_buffer_and_cancel_keeps_unsaved_state(cx: &mut TestAppContext) {
    let dir = TempDir::new("exit-multiple");
    let alpha = dir.path().join("alpha.txt");
    let beta = dir.path().join("beta.txt");
    fs::write(&alpha, b"alpha").unwrap();
    fs::write(&beta, b"beta").unwrap();
    let (mut visual, shell, alpha_view) =
        open(Buffer::new(Some(alpha.clone()), decoded("alpha")), cx);
    let beta_view = {
        let index = create(&mut visual, &shell, "beta", Some(&beta));
        activate(&mut visual, &shell, index);
        shell.read_with(&visual, |shell, _| shell.active_view().clone())
    };
    visual.simulate_input("Y");
    activate(&mut visual, &shell, 0);
    visual.simulate_keystrokes("ctrl-e");
    visual.simulate_input("X");
    redraw(&mut visual);

    visual.simulate_keystrokes("ctrl-x ctrl-c");
    redraw(&mut visual);
    visual.read(|cx| {
        let prompt = shell.read(cx).minibuffer().unwrap().read(cx).prompt(cx);
        assert!(prompt.contains("Save file"), "{prompt}");
        assert!(prompt.contains("alpha.txt"), "{prompt}");
    });
    visual.simulate_keystrokes("y");
    redraw(&mut visual);
    visual.read(|cx| {
        let prompt = shell.read(cx).minibuffer().unwrap().read(cx).prompt(cx);
        assert!(prompt.contains("Save file"), "{prompt}");
        assert!(prompt.contains("beta.txt"), "{prompt}");
    });
    visual.simulate_keystrokes("n");
    redraw(&mut visual);
    visual.read(|cx| {
        let prompt = shell.read(cx).minibuffer().unwrap().read(cx).prompt(cx);
        assert!(prompt.contains("Modified buffers exist; exit anyway?"));
    });
    visual.simulate_input("no");
    visual.simulate_keystrokes("enter");
    redraw(&mut visual);
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert!(shell.session().is_none());
        assert_eq!(shell.buffer_count(), 2);
        assert!(!alpha_view.read(cx).buffer().is_modified());
        assert!(beta_view.read(cx).buffer().is_modified());
    });
    assert_eq!(fs::read(&alpha).unwrap(), b"alphaX");
    assert_eq!(fs::read(&beta).unwrap(), b"beta");
}

#[gpui::test]
fn terminal_exit_prompts_for_a_file_name_and_cancel_aborts_exit(cx: &mut TestAppContext) {
    let (mut visual, shell, body) = open(Buffer::scratch(), cx);
    visual.simulate_input("dirty");
    redraw(&mut visual);
    visual.simulate_keystrokes("ctrl-x ctrl-c");
    redraw(&mut visual);
    visual.read(|cx| {
        let prompt = shell.read(cx).minibuffer().unwrap().read(cx).prompt(cx);
        assert!(prompt.contains("Save buffer *scratch*?"));
    });
    visual.simulate_keystrokes("y");
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(
            shell.read(cx).minibuffer().unwrap().read(cx).prompt(cx),
            "Write file: "
        );
    });
    visual.simulate_keystrokes("ctrl-g");
    redraw(&mut visual);
    visual.read(|cx| {
        assert!(shell.read(cx).session().is_none());
        assert_eq!(body.read(cx).buffer().text().to_string(), "dirty");
        assert!(body.read(cx).buffer().is_modified());
    });
    assert_eq!(visual.windows().len(), 1);
}

#[gpui::test]
fn terminal_exit_saves_a_named_scratch_buffer_and_closes(cx: &mut TestAppContext) {
    let dir = TempDir::new("exit-scratch");
    let target = dir.path().join("saved.txt");
    let (mut visual, shell, _) = open(Buffer::scratch(), cx);
    visual.simulate_input("dirty");
    redraw(&mut visual);
    visual.simulate_keystrokes("ctrl-x ctrl-c");
    visual.simulate_keystrokes("y");
    visual.simulate_input(target.to_str().unwrap());
    visual.simulate_keystrokes("enter");
    visual.run_until_parked();
    assert_eq!(fs::read(&target).unwrap(), b"dirty");
    assert!(visual.windows().is_empty());
    assert!(shell.read_with(&visual, |shell, _| shell.session().is_none()));
}

#[gpui::test]
fn a_failed_exit_save_aborts_exit_and_keeps_the_dirty_buffer(cx: &mut TestAppContext) {
    let dir = TempDir::new("exit-failure");
    let target = dir.path().join("directory-target");
    fs::create_dir(&target).unwrap();
    let buffer = Buffer::new(Some(target.clone()), decoded("base"));
    let (mut visual, shell, body) = open(buffer, cx);
    visual.simulate_input("X");
    redraw(&mut visual);

    visual.simulate_keystrokes("ctrl-x ctrl-c");
    visual.simulate_keystrokes("y");
    redraw(&mut visual);
    visual.read(|cx| {
        assert!(shell.read(cx).session().is_none());
        assert!(body.read(cx).buffer().is_modified());
        assert!(
            body.read(cx)
                .message()
                .unwrap_or_default()
                .starts_with("Save failed:")
        );
    });
    assert_eq!(visual.windows().len(), 1);
    assert!(target.is_dir());
}
