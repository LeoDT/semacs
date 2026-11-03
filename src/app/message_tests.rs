use crate::app::AppShell;
use crate::session::ReadKind;
use crate::text::ByteOffset;
use gpui_kit as gpui;
use gpui_kit::{Entity, Focusable, TestAppContext, VisualTestContext, px, size};

fn open(cx: &mut TestAppContext) -> (VisualTestContext, Entity<AppShell>) {
    cx.update(crate::app::init);
    let window = cx.open_window(size(px(400.), px(240.)), |window, cx| {
        AppShell::new(crate::buffer::Buffer::scratch(), window, cx)
    });
    let shell = window.entity(cx).unwrap();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.update(|window, _| window.activate_window());
    draw(&mut visual);
    (visual, shell)
}

fn draw(visual: &mut VisualTestContext) {
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
}

fn show(visual: &mut VisualTestContext, shell: &Entity<AppShell>, message: &str) {
    shell.update(visual, |shell, cx| {
        shell.show_message(&shell.active_view().clone(), message, cx);
    });
    draw(visual);
}

#[gpui::test]
fn command_messages_are_logged_once_but_prefixes_and_completion_hints_are_not(
    cx: &mut TestAppContext,
) {
    let (mut visual, shell) = open(cx);
    visual.simulate_keystrokes("ctrl-x");
    visual.read(|cx| {
        assert_eq!(shell.read(cx).echo.as_ref().unwrap().1.as_ref(), "C-x");
        assert!(shell.read(cx).messages.buffer.text().is_empty());
    });
    visual.simulate_keystrokes("ctrl-g");
    draw(&mut visual);
    show(&mut visual, &shell, "same");
    show(&mut visual, &shell, "same");
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let origin = shell.active_view().clone();
            shell.begin_read(ReadKind::Command, &origin, "", window, cx);
            shell.message_minibuffer("No match", cx);
        })
    });
    draw(&mut visual);
    visual.read(|cx| {
        assert_eq!(
            shell.read(cx).messages.buffer.text().to_string(),
            "C-x C-g is undefined\nsame [2 times]\n"
        )
    });
}

#[gpui::test]
fn long_echo_is_bounded_and_does_not_overlap_mode_line(cx: &mut TestAppContext) {
    let (mut visual, shell) = open(cx);
    for message in [
        "one\ntwo\nthree".to_owned(),
        "很长🙂".repeat(400),
        "line\n".repeat(100),
    ] {
        show(&mut visual, &shell, &message);
        let echo = visual.debug_bounds("echo-area").unwrap();
        assert!(echo.size.height > px(24.), "{echo:?}");
        assert!(echo.size.height <= px(60.), "{echo:?}");
        assert_eq!(echo.bottom(), px(240.));
        visual.read(|cx| {
            let body = shell
                .read(cx)
                .active_view()
                .read(cx)
                .viewport_layout()
                .unwrap()
                .bounds();
            assert!(body.bottom() + px(8. + 24.) <= echo.top() + px(0.01));
        });
    }
    visual.simulate_keystrokes("ctrl-f");
    draw(&mut visual);
    assert_eq!(
        visual.debug_bounds("echo-area").unwrap().size.height,
        px(24.)
    );
}

#[gpui::test]
fn minibuffer_feedback_is_an_annotation_not_an_edit_or_extra_echo_row(cx: &mut TestAppContext) {
    let (mut visual, shell) = open(cx);
    let editor = visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let origin = shell.active_view().clone();
            shell.begin_read(ReadKind::Command, &origin, "abc", window, cx);
            shell
                .minibuffer
                .as_ref()
                .unwrap()
                .view
                .read(cx)
                .editor()
                .clone()
        })
    });
    draw(&mut visual);
    let before = visual.read(|cx| editor.read(cx).buffer().snapshot_id());
    show(&mut visual, &shell, "脚本错误\n详细原因");
    draw(&mut visual);
    assert!(visual.debug_bounds("echo-area").is_none());
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.buffer().snapshot_id(), before);
        assert_eq!(view.buffer().text().to_string(), "M-x abc");
        assert_eq!(view.buffer().point(), ByteOffset(7));
        assert_eq!(view.message(), Some("脚本错误\n详细原因"));
    });
    visual.simulate_keystrokes("d");
    draw(&mut visual);
    visual.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.message(), None);
        assert_eq!(view.buffer().text().to_string(), "M-x abcd");
    });
    visual.simulate_keystrokes("ctrl-g");
    draw(&mut visual);
    visual.read(|cx| {
        assert_eq!(
            shell
                .read(cx)
                .active_view()
                .read(cx)
                .buffer()
                .text()
                .to_string(),
            ""
        )
    });
}

#[gpui::test]
fn messages_display_reuses_an_unselected_view_and_updates_without_dirty_or_undo(
    cx: &mut TestAppContext,
) {
    let (mut visual, shell) = open(cx);
    show(&mut visual, &shell, "first");
    let origin = visual.read(|cx| shell.read(cx).active_view().clone());
    visual.simulate_keystrokes("ctrl-h e");
    draw(&mut visual);
    let log = visual.update(|window, cx| {
        let shell = shell.read(cx);
        assert_eq!(shell.panes.leaves().len(), 2);
        assert_eq!(shell.active_view(), &origin);
        assert!(origin.focus_handle(cx).is_focused(window));
        let id = shell.messages.buffer.snapshot_id().buffer;
        shell
            .body_panes
            .values()
            .find(|pane| pane.current == id)
            .unwrap()
            .active_view()
            .clone()
    });
    show(&mut visual, &shell, "second");
    visual.read(|cx| {
        let buffer = log.read(cx).buffer();
        assert_eq!(buffer.text().to_string(), "first\nsecond\n");
        assert!(buffer.is_read_only());
        assert!(!buffer.is_modified());
        assert_eq!(buffer.point().0, buffer.text().len());
    });
    visual.simulate_keystrokes("ctrl-h e");
    draw(&mut visual);
    visual.read(|cx| assert_eq!(shell.read(cx).panes.leaves().len(), 2));
    visual.simulate_keystrokes("ctrl-x o x ctrl-/");
    draw(&mut visual);
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert!(shell.active_view().read(cx).buffer().is_message_log());
        assert!(
            !shell
                .active_view()
                .read(cx)
                .buffer()
                .text()
                .to_string()
                .starts_with('x')
        );
        assert!(!shell.active_view().read(cx).buffer().is_modified());
        assert_eq!(origin.read(cx).buffer().text().to_string(), "");
    });
}

#[gpui::test]
fn expired_hint_cannot_clear_a_new_message_and_old_views_do_not_restore_echo(
    cx: &mut TestAppContext,
) {
    let (mut visual, shell) = open(cx);
    show(&mut visual, &shell, "old");
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let index = shell.create_buffer(crate::buffer::Buffer::named("other"), window, cx);
            shell.activate(index, window, cx);
            shell.activate(0, window, cx);
            assert!(shell.echo.is_none());
            let origin = shell.active_view().clone();
            shell.begin_read(ReadKind::Command, &origin, "", window, cx);
            shell.message_minibuffer("No match", cx);
        })
    });
    visual.run_until_parked();
    show(&mut visual, &shell, "new");
    visual
        .executor()
        .advance_clock(std::time::Duration::from_secs(3));
    draw(&mut visual);
    visual.read(|cx| {
        let shell = shell.read(cx);
        let editor = shell.minibuffer.as_ref().unwrap().view.read(cx).editor();
        assert_eq!(editor.read(cx).message(), Some("new"));
    });
}

#[gpui::test]
fn exporting_messages_detaches_the_saved_buffer_from_future_logging(cx: &mut TestAppContext) {
    let (mut visual, shell) = open(cx);
    show(&mut visual, &shell, "before export");
    let dir = std::path::PathBuf::from(".pi/tmp").join(format!(
        "d1-export-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.canonicalize().unwrap().join("history.txt");
    let old = visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let id = shell.messages.buffer.snapshot_id().buffer;
            let index = shell.buffers.index_of(id).unwrap();
            let old = shell.buffer_target(index, window, cx);
            shell.write_file(&old, path.clone(), window, cx);
            old
        })
    });
    draw(&mut visual);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "before export\n");
    show(&mut visual, &shell, "after export");
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert_ne!(
            shell.messages.buffer.snapshot_id().buffer,
            old.read(cx).buffer().snapshot_id().buffer
        );
        assert_eq!(old.read(cx).buffer().text().to_string(), "before export\n");
        assert!(
            shell
                .messages
                .buffer
                .text()
                .to_string()
                .ends_with("after export\n")
        );
        assert_eq!(
            old.read(cx).buffer().path().as_deref(),
            Some(path.as_path())
        );
    });
}

#[gpui::test]
fn messages_arriving_during_export_remain_dirty_and_can_be_saved(cx: &mut TestAppContext) {
    let (mut visual, shell) = open(cx);
    show(&mut visual, &shell, "before export");
    let dir = std::path::PathBuf::from(".pi/tmp").join(format!(
        "d1-export-race-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.canonicalize().unwrap().join("history.txt");
    let old = visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let index = shell
                .buffers
                .index_of(shell.messages.buffer.snapshot_id().buffer)
                .unwrap();
            let old = shell.buffer_target(index, window, cx);
            shell.write_file(&old, path.clone(), window, cx);
            shell.messages.push("arrived while saving");
            old
        })
    });
    draw(&mut visual);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "before export\n");
    visual.read(|cx| {
        assert_eq!(
            old.read(cx).buffer().text().to_string(),
            "before export\narrived while saving\n"
        );
        assert!(old.read(cx).buffer().is_modified());
        assert!(!old.read(cx).buffer().is_message_log());
    });
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            assert!(shell.save_buffer(&old, window, cx));
        })
    });
    draw(&mut visual);
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "before export\narrived while saving\n"
    );
    visual.read(|cx| assert!(!old.read(cx).buffer().is_modified()));
}

#[gpui::test]
fn logged_messages_are_available_through_the_normal_buffer_reader(cx: &mut TestAppContext) {
    let (mut visual, shell) = open(cx);
    show(&mut visual, &shell, "logged before C-h e");
    visual.simulate_keystrokes("ctrl-x b");
    visual.simulate_input("*Messages*");
    visual.simulate_keystrokes("enter");
    draw(&mut visual);
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert_eq!(
            shell.active_view().read(cx).buffer().display_name(),
            "*Messages*"
        );
        assert_eq!(
            shell.active_view().read(cx).buffer().text().to_string(),
            "logged before C-h e\n"
        );
    });
}

#[gpui::test]
fn killing_messages_recreates_a_clean_log_without_reviving_old_history(cx: &mut TestAppContext) {
    let (mut visual, shell) = open(cx);
    show(&mut visual, &shell, "old");
    visual.simulate_keystrokes("ctrl-h e");
    draw(&mut visual);
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let id = shell.messages.buffer.snapshot_id().buffer;
            shell.remove_buffer(shell.buffers.index_of(id).unwrap(), window, cx);
        })
    });
    show(&mut visual, &shell, "new");
    visual.read(|cx| assert_eq!(shell.read(cx).messages.buffer.text().to_string(), "new\n"));
}
