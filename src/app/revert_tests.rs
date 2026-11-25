use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use gpui_kit as gpui;
use gpui_kit::{AppContext, Entity, InputHandler, TestAppContext, VisualTestContext, px, size};

use super::AppShell;
use crate::buffer::{Buffer, TextVersion};
use crate::command::Command;
use crate::files::{self, FileFormat};
use crate::revert::AutoRevertLocal;
use crate::session::ReadKind;
use crate::text::ByteOffset;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(".pi/tmp")
            .join(format!(
                "revert-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    fn write_file(&self, name: &str, text: &str) -> PathBuf {
        let path = self.path(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        // Atomic write ensures a fresh inode/stamp even if length and clock tick match.
        files::write_atomic(&path, text, FileFormat::default()).unwrap();
        path
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn open_with_config(
    buffer: Buffer,
    config_source: Option<&str>,
    cx: &mut TestAppContext,
) -> (VisualTestContext, Entity<AppShell>) {
    cx.background_executor.allow_parking();
    cx.update(crate::app::init);
    let loaded = match config_source {
        Some(source) => {
            let dir = Fixture::new();
            let init = dir.write_file("init.js", source);
            let result = gpui::block_on(crate::scripting::load(init.clone()));
            Some((init, result))
        }
        None => None,
    };
    let window = cx.open_window(size(px(960.), px(640.)), |window, cx| match loaded {
        Some((path, result)) => {
            AppShell::with_configuration(buffer, Some(path), result, window, cx)
        }
        None => AppShell::new(buffer, window, cx),
    });
    let shell = window.entity(cx).unwrap();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.update(|window, cx| {
        window.activate_window();
        window.draw(cx).clear(cx);
    });
    visual.run_until_parked();
    // Flush the initial window-activation focus-debounce timer before tests run.
    visual.executor().advance_clock(Duration::from_millis(200));
    wait_revert(&mut visual, &shell);
    (visual, shell)
}

fn open_file(path: &Path, cx: &mut TestAppContext) -> (VisualTestContext, Entity<AppShell>) {
    let loaded = files::load(path).unwrap();
    let buffer = Buffer::visiting(path.to_path_buf(), loaded.file, loaded.stamp);
    open_with_config(buffer, None, cx)
}

fn run_cmd(visual: &mut VisualTestContext, shell: &Entity<AppShell>, command: Command) {
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let origin = shell.active_view().clone();
            shell.run_command(command.into(), &origin, window, cx);
        });
    });
}

fn wait_stat(visual: &mut VisualTestContext, shell: &Entity<AppShell>) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        let done = visual.read(|cx| !shell.read(cx).auto_revert_stat_in_flight);
        if done {
            return;
        }
        assert!(Instant::now() < deadline, "stat task did not finish");
        std::thread::yield_now();
    }
}

fn wait_revert(visual: &mut VisualTestContext, shell: &Entity<AppShell>) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        let done = visual.read(|cx| {
            let shell = shell.read(cx);
            !shell.auto_revert_stat_in_flight
                && shell.auto_revert_focus_task.is_none()
                && shell.buffers.iter().all(|slot| slot.revert.task.is_none())
        });
        if done {
            return;
        }
        assert!(Instant::now() < deadline, "revert task did not finish");
        std::thread::yield_now();
    }
}

fn advance_and_wait_stat(visual: &mut VisualTestContext, shell: &Entity<AppShell>, ms: u64) {
    visual.executor().advance_clock(Duration::from_millis(ms));
    wait_stat(visual, shell);
}

fn advance_and_settle(visual: &mut VisualTestContext, shell: &Entity<AppShell>, ms: u64) {
    visual.executor().advance_clock(Duration::from_millis(ms));
    wait_revert(visual, shell);
}

#[gpui::test]
fn manual_revert_buffer_handles_scratch_clean_dirty_cancel_and_errors(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let path = fixture.write_file("sample.txt", "alpha\nbeta\ngamma\n");

    // Unvisited buffer reports Emacs error without opening a minibuffer query.
    let (mut scratch_visual, scratch_shell) = open_with_config(Buffer::scratch(), None, cx);
    run_cmd(&mut scratch_visual, &scratch_shell, Command::RevertBuffer);
    scratch_visual.read(|cx| {
        let shell = scratch_shell.read(cx);
        assert!(shell.session.is_none());
        assert_eq!(
            shell.active_view().read(cx).message(),
            Some("Buffer does not seem to be associated with any file")
        );
    });
    drop(scratch_visual);

    // Visited file: clean confirmation prompt, cancel with `no`, then accept `yes`.
    let (mut visual, shell) = open_file(&path, cx);
    run_cmd(&mut visual, &shell, Command::GlobalAutoRevertMode);
    wait_revert(&mut visual, &shell);

    fixture.write_file("sample.txt", "alpha\nBETA\ngamma\n");
    run_cmd(&mut visual, &shell, Command::RevertBuffer);
    visual.run_until_parked();
    visual.read(|cx| {
        let session = shell.read(cx).session.as_ref().unwrap();
        assert!(matches!(
            session.kind(),
            ReadKind::RevertBuffer {
                modified: false,
                ..
            }
        ));
        assert!(
            session
                .kind()
                .prompt()
                .starts_with("Revert buffer from file ")
        );
    });
    visual.simulate_input("no");
    visual.simulate_keystrokes("enter");
    wait_revert(&mut visual, &shell);
    visual.read(|cx| {
        assert_eq!(
            shell
                .read(cx)
                .active_view()
                .read(cx)
                .buffer()
                .text()
                .to_string(),
            "alpha\nbeta\ngamma\n"
        );
        assert_eq!(
            shell.read(cx).active_view().read(cx).message(),
            Some("Canceled")
        );
    });

    // Save in progress refuses manual revert.
    visual.update(|_, cx| {
        shell.update(cx, |shell, cx| {
            let view = shell.active_view().clone();
            let id = view.read(cx).buffer().snapshot_id().buffer;
            let idx = shell.buffers.index_of(id).unwrap();
            let point = shell.buffers[idx].buffer.begin_save();
            let req = crate::saves::SaveRequest {
                generation: shell.buffers[idx].save.next_generation(),
                save_point: point,
                intent: crate::saves::SaveIntent::Current,
                confirmed: false,
                format: FileFormat::default(),
                text: crate::text::Rope::from("x"),
            };
            shell.buffers[idx].save.start(&req);
            assert!(shell.buffers[idx].save.is_saving());
        });
    });
    run_cmd(&mut visual, &shell, Command::RevertBuffer);
    visual.read(|cx| {
        assert!(shell.read(cx).session.is_none());
        assert_eq!(
            shell.read(cx).active_view().read(cx).message(),
            Some("Save in progress; wait before reverting")
        );
    });
    // Reset synthetic saving state.
    visual.update(|_, cx| {
        shell.update(cx, |shell, _| {
            shell.buffers[0].save = Default::default();
        });
    });

    // Dirty buffer uses Discard edits prompt; C-g cancels; yes reverts and undo restores draft.
    visual.simulate_input("DIRTY");
    visual.run_until_parked();
    assert!(visual.read(|cx| shell.read(cx).active_view().read(cx).buffer().is_modified()));

    run_cmd(&mut visual, &shell, Command::RevertBuffer);
    visual.run_until_parked();
    visual.read(|cx| {
        let session = shell.read(cx).session.as_ref().unwrap();
        assert!(matches!(
            session.kind(),
            ReadKind::RevertBuffer { modified: true, .. }
        ));
        assert!(
            session
                .kind()
                .prompt()
                .starts_with("Discard edits and reread from ")
        );
    });
    visual.simulate_keystrokes("ctrl-g");
    visual.run_until_parked();
    assert!(visual.read(|cx| shell.read(cx).active_view().read(cx).buffer().is_modified()));

    run_cmd(&mut visual, &shell, Command::RevertBuffer);
    visual.run_until_parked();
    visual.simulate_input("yes");
    visual.simulate_keystrokes("enter");
    wait_revert(&mut visual, &shell);
    visual.read(|cx| {
        let buf = shell.read(cx).active_view().read(cx).buffer();
        assert_eq!(buf.text().to_string(), "alpha\nBETA\ngamma\n");
        assert!(!buf.is_modified());
    });

    // Undo restores discarded draft and marks dirty; redo returns to reverted clean text.
    visual.simulate_keystrokes("ctrl-/");
    visual.run_until_parked();
    visual.read(|cx| {
        let buf = shell.read(cx).active_view().read(cx).buffer();
        assert_eq!(buf.text().to_string(), "DIRTYalpha\nbeta\ngamma\n");
        assert!(buf.is_modified());
    });
    visual.simulate_keystrokes("ctrl-?");
    visual.run_until_parked();
    visual.read(|cx| {
        let buf = shell.read(cx).active_view().read(cx).buffer();
        assert_eq!(buf.text().to_string(), "alpha\nBETA\ngamma\n");
        assert!(!buf.is_modified());
    });

    // Invalid UTF-8 keeps buffer intact and reports error.
    std::fs::write(&path, [0xff, 0xfe]).unwrap();
    run_cmd(&mut visual, &shell, Command::RevertBuffer);
    visual.run_until_parked();
    visual.simulate_input("yes");
    visual.simulate_keystrokes("enter");
    wait_revert(&mut visual, &shell);
    visual.read(|cx| {
        let view = shell.read(cx).active_view().read(cx);
        assert_eq!(view.buffer().text().to_string(), "alpha\nBETA\ngamma\n");
        assert!(view.message().unwrap().starts_with("Cannot revert "));
    });

    // Deleted file keeps buffer intact and reports missing file.
    std::fs::remove_file(&path).unwrap();
    run_cmd(&mut visual, &shell, Command::RevertBuffer);
    visual.run_until_parked();
    visual.simulate_input("yes");
    visual.simulate_keystrokes("enter");
    wait_revert(&mut visual, &shell);
    visual.read(|cx| {
        let view = shell.read(cx).active_view().read(cx);
        assert_eq!(view.buffer().text().to_string(), "alpha\nBETA\ngamma\n");
        assert!(view.message().unwrap().ends_with("no longer exists!"));
    });
}

#[gpui::test]
fn auto_revert_polls_only_visible_clean_buffers_and_coalesces_burst_writes(
    cx: &mut TestAppContext,
) {
    let fixture = Fixture::new();
    let visible_path = fixture.write_file("visible.txt", "line1\nline2\n");
    let hidden_path = fixture.write_file("hidden.txt", "hidden1\n");

    let (mut visual, shell) = open_file(&visible_path, cx);
    // Add a hidden visited buffer in background.
    let hidden_id = visual.update(|_, cx| {
        let loaded = files::load(&hidden_path).unwrap();
        let buffer = Buffer::visiting(hidden_path.clone(), loaded.file, loaded.stamp);
        let id = buffer.snapshot_id().buffer;
        shell.update(cx, |shell, _| {
            shell
                .buffers
                .insert_background(crate::buffers::BufferSlot::new(buffer));
        });
        id
    });

    // Place point at EOB of visible buffer to verify EOB tailing.
    visual.update(|_, cx| {
        let view = shell.read(cx).active_view().clone();
        view.update(cx, |view, cx| view.show_end(cx));
    });

    // Polling when disk is unchanged must not bump text version or emit messages.
    let initial_version = visual.read(|cx| {
        shell
            .read(cx)
            .active_view()
            .read(cx)
            .buffer()
            .snapshot_id()
            .version
    });
    // Poll interval fires at t=1000ms (was at 200ms after open_file -> advance 800ms).
    // Test again at t=2000ms after modifying both files.
    advance_and_settle(&mut visual, &shell, 800);
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert_eq!(
            shell.active_view().read(cx).buffer().snapshot_id().version,
            initial_version
        );
        assert!(shell.active_view().read(cx).message().is_none());
    });

    // Modify BOTH visible and hidden files on disk.
    fixture.write_file("visible.txt", "line1\nline2\nline3\n");
    fixture.write_file("hidden.txt", "hidden2\n");

    // Next poll interval fires at t=2000ms -> schedules 250ms debounced load (due at t=2250ms).
    advance_and_wait_stat(&mut visual, &shell, 1000);
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert!(shell.buffers[0].revert.task.is_some());
        assert_eq!(
            shell.active_view().read(cx).buffer().text().to_string(),
            "line1\nline2\n"
        );
    });
    // Burst write within 250ms debounce (at t=2100ms) should coalesce to the latest content.
    advance_and_wait_stat(&mut visual, &shell, 100);
    fixture.write_file("visible.txt", "line1\nline2\nline3\nline4\n");
    advance_and_settle(&mut visual, &shell, 200);

    visual.read(|cx| {
        let shell = shell.read(cx);
        let buf = shell.active_view().read(cx).buffer();
        assert_eq!(buf.text().to_string(), "line1\nline2\nline3\nline4\n");
        assert!(!buf.is_modified());
        assert_eq!(
            buf.point(),
            ByteOffset("line1\nline2\nline3\nline4\n".len())
        );
        assert_eq!(
            shell.active_view().read(cx).message(),
            Some("Reverting buffer ‘visible.txt’")
        );
        // Hidden buffer was NOT touched by polling.
        let hidden_idx = shell.buffers.index_of(hidden_id).unwrap();
        assert_eq!(
            shell.buffers[hidden_idx].buffer.text().to_string(),
            "hidden1\n"
        );
    });

    // A single undo reverts the coalesced burst back to the original content, and redo restores it.
    visual.simulate_keystrokes("ctrl-/");
    visual.run_until_parked();
    visual.read(|cx| {
        let buf = shell.read(cx).active_view().read(cx).buffer();
        assert_eq!(buf.text().to_string(), "line1\nline2\n");
        assert!(buf.is_modified());
    });
    visual.simulate_keystrokes("ctrl-?");
    visual.run_until_parked();
    visual.read(|cx| {
        let buf = shell.read(cx).active_view().read(cx).buffer();
        assert_eq!(buf.text().to_string(), "line1\nline2\nline3\nline4\n");
        assert!(!buf.is_modified());
    });

    // Switching to hidden buffer immediately checks and reverts it.
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let idx = shell.buffers.index_of(hidden_id).unwrap();
            assert!(shell.activate(idx, window, cx));
        });
    });
    wait_revert(&mut visual, &shell);
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert_eq!(
            shell.active_view().read(cx).buffer().text().to_string(),
            "hidden2\n"
        );
    });

    // Dirty visible buffer is never auto-reverted.
    visual.simulate_input("local ");
    visual.run_until_parked();
    fixture.write_file("hidden.txt", "external_while_dirty\n");
    advance_and_wait_stat(&mut visual, &shell, 1000);
    advance_and_settle(&mut visual, &shell, 250);
    visual.read(|cx| {
        let buf = shell.read(cx).active_view().read(cx).buffer();
        assert_eq!(buf.text().to_string(), "local hidden2\n");
        assert!(buf.is_modified());
    });
}

#[gpui::test]
fn auto_revert_handles_focus_regain_deleted_and_invalid_utf8_silently(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let path = fixture.write_file("focus.txt", "one\n");
    let (mut visual, shell) = open_file(&path, cx);

    // Invalid UTF-8 is ignored silently and cached in failed_stamp until disk changes again.
    std::fs::write(&path, [0xff, 0xfe, 0xfd]).unwrap();
    advance_and_wait_stat(&mut visual, &shell, 1000);
    advance_and_settle(&mut visual, &shell, 250);
    visual.read(|cx| {
        let shell = shell.read(cx);
        let view = shell.active_view().read(cx);
        assert_eq!(view.buffer().text().to_string(), "one\n");
        assert!(view.message().is_none());
        assert!(shell.buffers[0].revert.failed_stamp.is_some());
    });

    // Deleted file is ignored silently.
    std::fs::remove_file(&path).unwrap();
    advance_and_wait_stat(&mut visual, &shell, 1000);
    advance_and_settle(&mut visual, &shell, 250);
    visual.read(|cx| {
        assert_eq!(
            shell
                .read(cx)
                .active_view()
                .read(cx)
                .buffer()
                .text()
                .to_string(),
            "one\n"
        );
    });

    // Recreate valid file and simulate window deactivation + reactivation (150ms focus debounce).
    let other_window = cx.open_window(size(px(200.), px(200.)), |window, cx| {
        AppShell::new(Buffer::scratch(), window, cx)
    });
    cx.update_window(other_window.into(), |_, window, _| {
        window.activate_window();
    })
    .unwrap();
    visual.run_until_parked();

    fixture.write_file("focus.txt", "two\n");
    visual.update(|window, _| window.activate_window());
    advance_and_settle(&mut visual, &shell, 150);
    visual.read(|cx| {
        let buf = shell.read(cx).active_view().read(cx).buffer();
        assert_eq!(buf.text().to_string(), "two\n");
        assert!(shell.read(cx).buffers[0].revert.failed_stamp.is_none());
    });
}

#[gpui::test]
fn auto_revert_defers_during_minibuffer_isearch_and_ime_preedit(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let path = fixture.write_file("defer.txt", "hello world\n");
    let (mut visual, shell) = open_file(&path, cx);

    // 1. Active minibuffer defers auto-revert until minibuffer ends.
    visual.simulate_keystrokes("alt-x");
    visual.run_until_parked();
    fixture.write_file("defer.txt", "hello minibuffer\n");
    advance_and_settle(&mut visual, &shell, 1000);
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert_eq!(
            shell.active_view().read(cx).buffer().text().to_string(),
            "hello world\n"
        );
        assert!(shell.buffers[0].revert.deferred);
    });
    visual.simulate_keystrokes("ctrl-g");
    wait_revert(&mut visual, &shell);
    visual.read(|cx| {
        assert_eq!(
            shell
                .read(cx)
                .active_view()
                .read(cx)
                .buffer()
                .text()
                .to_string(),
            "hello minibuffer\n"
        );
    });

    // 2. Active isearch defers auto-revert until isearch finishes.
    visual.simulate_keystrokes("ctrl-s");
    visual.simulate_input("hello");
    visual.run_until_parked();
    fixture.write_file("defer.txt", "hello isearch\n");
    advance_and_settle(&mut visual, &shell, 1000);
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert_eq!(
            shell.active_view().read(cx).buffer().text().to_string(),
            "hello minibuffer\n"
        );
        assert!(shell.buffers[0].revert.deferred);
    });
    visual.simulate_keystrokes("enter");
    wait_revert(&mut visual, &shell);
    visual.read(|cx| {
        assert_eq!(
            shell
                .read(cx)
                .active_view()
                .read(cx)
                .buffer()
                .text()
                .to_string(),
            "hello isearch\n"
        );
    });

    // 3. Active IME preedit defers auto-revert until clearing preedit keeps buffer clean.
    let view = visual.read(|cx| shell.read(cx).active_view().clone());
    let mut input = crate::view::input_tests::handler(&view, &visual);
    visual.update(|window, cx| {
        input.replace_and_mark_text_in_range(None, "ni", Some(2..2), window, cx);
    });
    visual.run_until_parked();
    fixture.write_file("defer.txt", "hello ime\n");
    advance_and_settle(&mut visual, &shell, 1000);
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert_eq!(
            shell.active_view().read(cx).buffer().text().to_string(),
            "hello isearch\n"
        );
        assert!(shell.buffers[0].revert.deferred);
    });
    let mut input = crate::view::input_tests::handler(&view, &visual);
    visual.update(|window, cx| {
        input.replace_and_mark_text_in_range(None, "", None, window, cx);
    });
    wait_revert(&mut visual, &shell);
    visual.read(|cx| {
        assert_eq!(
            shell
                .read(cx)
                .active_view()
                .read(cx)
                .buffer()
                .text()
                .to_string(),
            "hello ime\n"
        );
    });
}

#[gpui::test]
fn auto_revert_mode_toggles_and_js_configuration_control_polling(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let path = fixture.write_file("cfg.txt", "start\n");
    let loaded = files::load(&path).unwrap();
    let buffer = Buffer::visiting(path.clone(), loaded.file, loaded.stamp);

    let (mut visual, shell) = open_with_config(
        buffer,
        Some("semacs.setAutoRevert({ enabled: false, intervalMs: 500, verbose: false });"),
        cx,
    );
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert!(!shell.auto_revert.enabled);
        assert_eq!(shell.auto_revert.interval_ms, 500);
        assert!(!shell.auto_revert.verbose);
        assert!(shell.auto_revert_poll.is_none());
    });

    // Global disabled: disk write does not auto-revert.
    fixture.write_file("cfg.txt", "ignored\n");
    advance_and_settle(&mut visual, &shell, 1500);
    visual.read(|cx| {
        assert_eq!(
            shell
                .read(cx)
                .active_view()
                .read(cx)
                .buffer()
                .text()
                .to_string(),
            "start\n"
        );
    });

    // Buffer-local auto-revert-mode enables polling for this buffer and shows ARev lighter.
    run_cmd(&mut visual, &shell, Command::AutoRevertMode);
    wait_revert(&mut visual, &shell);
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert_eq!(shell.buffers[0].revert.local, AutoRevertLocal::Enabled);
        assert!(shell.buffers[0].revert.local.shows_lighter());
        assert_eq!(
            shell.active_view().read(cx).buffer().text().to_string(),
            "ignored\n"
        );
    });

    // With verbose: false, later auto-revert updates text without echoing a message.
    fixture.write_file("cfg.txt", "quiet\n");
    advance_and_wait_stat(&mut visual, &shell, 500);
    advance_and_settle(&mut visual, &shell, 250);
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert_eq!(
            shell.active_view().read(cx).buffer().text().to_string(),
            "quiet\n"
        );
        // Message remains the mode-toggle confirmation, not "Reverting buffer ...".
        assert_eq!(
            shell.active_view().read(cx).message(),
            Some("Auto-Revert mode enabled in current buffer")
        );
    });

    // Toggling local off reverts local state to Inherit (since global is false).
    run_cmd(&mut visual, &shell, Command::AutoRevertMode);
    wait_revert(&mut visual, &shell);
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert_eq!(shell.buffers[0].revert.local, AutoRevertLocal::Inherit);
        assert!(shell.auto_revert_poll.is_none());
    });

    // Toggling global on resumes polling with configured 500ms interval.
    run_cmd(&mut visual, &shell, Command::GlobalAutoRevertMode);
    wait_revert(&mut visual, &shell);
    fixture.write_file("cfg.txt", "global\n");
    advance_and_wait_stat(&mut visual, &shell, 500);
    advance_and_settle(&mut visual, &shell, 250);
    visual.read(|cx| {
        assert_eq!(
            shell
                .read(cx)
                .active_view()
                .read(cx)
                .buffer()
                .text()
                .to_string(),
            "global\n"
        );
    });
}

#[cfg(unix)]
#[gpui::test]
fn auto_revert_skips_active_commit_editor_buffers(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let path = fixture.write_file("COMMIT_EDITMSG", "initial commit msg\n");
    cx.background_executor.allow_parking();
    cx.update(crate::app::init);
    let window = cx.open_window(size(px(960.), px(640.)), |window, cx| {
        AppShell::new(Buffer::named("origin"), window, cx)
    });
    let shell = window.entity(cx).unwrap();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.update(|window, cx| {
        window.activate_window();
        window.draw(cx).clear(cx);
    });
    let (request, _peer) = crate::commit::EditorRequest::test_pair(path.clone());
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.edit_commit_message(request, "#".into(), window, cx);
        });
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        visual.run_until_parked();
        if visual.read(|cx| !shell.read(cx).commit_editors.is_empty()) {
            break;
        }
        assert!(Instant::now() < deadline, "commit editor did not open");
        std::thread::yield_now();
    }

    fixture.write_file("COMMIT_EDITMSG", "external commit change\n");
    advance_and_settle(&mut visual, &shell, 1000);
    advance_and_settle(&mut visual, &shell, 250);
    visual.read(|cx| {
        assert_eq!(
            shell
                .read(cx)
                .active_view()
                .read(cx)
                .buffer()
                .text()
                .to_string(),
            "initial commit msg\n"
        );
    });
}

#[gpui::test]
fn switching_to_scratch_stops_polling_timer(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let path = fixture.write_file("stop.txt", "content\n");
    let (mut visual, shell) = open_file(&path, cx);
    visual.read(|cx| {
        assert!(shell.read(cx).auto_revert_poll.is_some());
    });

    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let idx = shell.create_buffer(Buffer::scratch(), window, cx);
            assert!(shell.activate(idx, window, cx));
        });
    });
    wait_revert(&mut visual, &shell);
    visual.read(|cx| {
        assert!(shell.read(cx).auto_revert_poll.is_none());
    });
}

#[gpui::test]
fn idle_text_version_and_timer_stay_clean_when_disk_unchanged(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let path = fixture.write_file("idle.txt", "stable\n");
    let (mut visual, shell) = open_file(&path, cx);
    let version = visual.read(|cx| {
        shell
            .read(cx)
            .active_view()
            .read(cx)
            .buffer()
            .snapshot_id()
            .version
    });
    assert_eq!(version, TextVersion::default());
    for _ in 0..3 {
        advance_and_settle(&mut visual, &shell, 1000);
    }
    visual.read(|cx| {
        assert_eq!(
            shell
                .read(cx)
                .active_view()
                .read(cx)
                .buffer()
                .snapshot_id()
                .version,
            version
        );
    });
}

#[gpui::test]
fn auto_revert_defers_during_active_git_task_without_breaking_session_token(
    cx: &mut TestAppContext,
) {
    let fixture = Fixture::new();
    let run_git = |args: &[&str]| {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(&fixture.0)
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@example.com")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@example.com")
            .status()
            .unwrap();
        assert!(status.success(), "git {:?} failed", args);
    };
    run_git(&["init", "-q"]);
    let path = fixture.write_file("a.txt", "initial\n");
    run_git(&["add", "a.txt"]);
    run_git(&["commit", "-q", "-m", "init"]);

    let (mut visual, shell) = open_file(&path, cx);
    // Split so `a.txt` stays visible in the first pane while `magit-status` opens in the second.
    run_cmd(&mut visual, &shell, Command::SplitWindowRight);
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));

    let (gate_tx, gate_rx) = futures_channel::oneshot::channel();
    visual.update(|_, cx| {
        shell.update(cx, |shell, _| {
            shell.revert_read_gate = Some(gate_rx);
        });
    });

    // Trigger poll + 250ms debounce so auto-revert load blocks at `revert_read_gate`.
    fixture.write_file("a.txt", "external\n");
    advance_and_wait_stat(&mut visual, &shell, 1000);
    visual.executor().advance_clock(Duration::from_millis(250));
    visual.run_until_parked();
    visual.read(|cx| {
        assert!(shell.read(cx).buffers[0].revert.task.is_some());
    });

    // Start `magit-status` (`git_task` active) and release the in-flight revert read while `git_task` runs.
    run_cmd(&mut visual, &shell, Command::MagitStatus);
    visual.read(|cx| {
        assert!(shell.read(cx).git_task.is_some());
    });
    let _ = gate_tx.send(());

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        if visual.read(|cx| {
            let shell = shell.read(cx);
            shell.git_task.is_none()
                && !shell.auto_revert_stat_in_flight
                && shell.buffers[0].revert.task.is_none()
                && !shell.buffers[0].revert.deferred
        }) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "magit-status and deferred revert did not finish"
        );
        std::thread::yield_now();
    }

    visual.read(|cx| {
        let shell = shell.read(cx);
        assert_eq!(
            shell.active_view().read(cx).buffer().mode(),
            crate::mode::ModeId::MAGIT_STATUS
        );
        // Once `git_task` finished, the deferred revert flushed cleanly to `external\n`.
        assert_eq!(shell.buffers[0].buffer.text().to_string(), "external\n");
    });
}
