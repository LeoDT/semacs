use crate::app::AppShell;
use crate::buffer::Buffer;
use crate::command::Command;
use crate::tree::tests::Fixture;
use gpui_kit as gpui;
use gpui_kit::{Entity, Focusable, TestAppContext, VisualTestContext, px, size};
use std::time::{Duration, Instant};

pub(super) fn open(
    fixture: &Fixture,
    cx: &mut TestAppContext,
) -> (VisualTestContext, Entity<AppShell>) {
    cx.background_executor.allow_parking();
    cx.update(crate::app::init);
    fixture.directory(".git");
    let path = fixture.file("file.txt");
    let window = cx.open_window(size(px(1100.), px(700.)), |window, cx| {
        AppShell::new(Buffer::new_file(path), window, cx)
    });
    let shell = window.entity(cx).unwrap();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.update(|window, cx| {
        window.activate_window();
        window.draw(cx).clear(cx);
    });
    (visual, shell)
}

pub(super) fn command(visual: &mut VisualTestContext, shell: &Entity<AppShell>, command: Command) {
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let origin = shell.active_view().clone();
            shell.run_command(command.into(), &origin, window, cx);
        })
    });
}

pub(super) fn wait(visual: &mut VisualTestContext, shell: &Entity<AppShell>) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        if visual.read(|cx| {
            let shell = shell.read(cx);
            shell.tree_work.is_none() && shell.file_task.is_none() && shell.opening.is_none()
        }) {
            return;
        }
        assert!(Instant::now() < deadline, "tree request did not finish");
        std::thread::yield_now();
    }
}

pub(super) fn point_at(
    visual: &mut VisualTestContext,
    shell: &Entity<AppShell>,
    path: &std::path::Path,
) {
    visual.update(|window, cx| {
        let view = shell.read(cx).active_view().clone();
        let key = crate::tree::node_key(path);
        let point = view
            .read(cx)
            .buffer()
            .sections()
            .unwrap()
            .nodes()
            .iter()
            .find(|node| node.key == key)
            .unwrap()
            .range
            .start;
        view.update(cx, |view, cx| view.show_location(point, window, cx));
    });
}

#[gpui::test]
fn two_tabs_during_a_pending_expand_restore_the_collapsed_state(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    fixture.file("dir/child");
    let (mut visual, shell) = open(&fixture, cx);
    command(&mut visual, &shell, Command::Treemacs);
    wait(&mut visual, &shell);
    point_at(&mut visual, &shell, &fixture.0.join("dir"));
    let (send, receive) = futures_channel::oneshot::channel();
    visual.update(|_, cx| shell.update(cx, |shell, _| shell.tree_read_gate = Some(receive)));
    command(&mut visual, &shell, Command::TreemacsToggle);
    command(&mut visual, &shell, Command::TreemacsToggle);
    send.send(()).unwrap();
    wait(&mut visual, &shell);
    visual.read(|cx| {
        let buffer = shell.read(cx).active_view().read(cx).buffer();
        assert!(buffer.text().to_string().contains("+ dir"));
        assert_eq!(buffer.sections().unwrap().visible_lines().len(), 3);
    });
}

#[gpui::test]
fn consecutive_expands_and_refresh_preserve_all_requested_operations(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    fixture.file("left/one");
    fixture.file("right/two");
    let (mut visual, shell) = open(&fixture, cx);
    command(&mut visual, &shell, Command::Treemacs);
    wait(&mut visual, &shell);
    let (send, receive) = futures_channel::oneshot::channel();
    visual.update(|_, cx| shell.update(cx, |shell, _| shell.tree_read_gate = Some(receive)));
    point_at(&mut visual, &shell, &fixture.0.join("left"));
    command(&mut visual, &shell, Command::TreemacsToggle);
    point_at(&mut visual, &shell, &fixture.0.join("right"));
    command(&mut visual, &shell, Command::TreemacsToggle);
    command(&mut visual, &shell, Command::TreemacsRefresh);
    send.send(()).unwrap();
    wait(&mut visual, &shell);
    visual.read(|cx| {
        let buffer = shell.read(cx).active_view().read(cx).buffer();
        let text = buffer.text().to_string();
        assert!(text.contains("- left"));
        assert!(text.contains("- right"));
        assert_eq!(buffer.sections().unwrap().visible_lines().len(), 6);
    });
}

#[gpui::test]
fn refusing_file_display_in_a_dedicated_side_window_finishes_the_open_lifecycle(
    cx: &mut TestAppContext,
) {
    let fixture = Fixture::new();
    let path = fixture.file("other.txt");
    let (mut visual, shell) = open(&fixture, cx);
    command(&mut visual, &shell, Command::Treemacs);
    wait(&mut visual, &shell);
    let tree_id = visual.read(|cx| shell.read(cx).active_buffer_id());
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let origin = shell.active_view().clone();
            shell.visit_file_at(path, &origin, Some((100, 0)), window, cx);
        })
    });
    wait(&mut visual, &shell);
    visual.update(|window, cx| {
        window.draw(cx).clear(cx);
        let shell = shell.read(cx);
        assert_eq!(shell.active_buffer_id(), tree_id);
        assert!(shell.opening.is_none());
        assert!(shell.file_task.is_none());
        assert_eq!(
            shell.active_view().read(cx).message(),
            Some("Window is dedicated to *Treemacs*")
        );
    });
}

#[gpui::test]
fn end_of_buffer_in_a_collapsed_tree_stays_on_a_visible_heading(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (mut visual, shell) = open(&fixture, cx);
    command(&mut visual, &shell, Command::Treemacs);
    wait(&mut visual, &shell);
    command(&mut visual, &shell, Command::TreemacsToggle);
    wait(&mut visual, &shell);
    command(&mut visual, &shell, Command::EndOfBuffer);
    visual.read(|cx| {
        let buffer = shell.read(cx).active_view().read(cx).buffer();
        assert_eq!(buffer.point(), crate::text::ByteOffset(0));
    });
}

#[gpui::test]
fn protected_side_window_survives_retain_and_can_be_closed(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (mut visual, shell) = open(&fixture, cx);
    command(&mut visual, &shell, Command::Treemacs);
    wait(&mut visual, &shell);
    let tree_id = visual.read(|cx| shell.read(cx).active_buffer_id());
    command(&mut visual, &shell, Command::DeleteOtherWindows);
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert_eq!(shell.panes.leaves().len(), 2);
        assert_eq!(
            shell.active_view().read(cx).message(),
            Some("Cannot make side window the only window")
        );
    });
    command(&mut visual, &shell, Command::OtherWindow);
    command(&mut visual, &shell, Command::DeleteWindow);
    visual.read(|cx| assert_eq!(shell.read(cx).panes.leaves().len(), 2));
    visual.update(|window, cx| window.draw(cx).clear(cx));
    command(&mut visual, &shell, Command::SplitWindowRight);
    visual.read(|cx| assert_eq!(shell.read(cx).panes.leaves().len(), 3));
    command(&mut visual, &shell, Command::DeleteOtherWindows);
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert_eq!(shell.panes.leaves().len(), 2);
        assert!(shell.panes.side().is_some());
    });
    command(&mut visual, &shell, Command::OtherWindow);
    command(&mut visual, &shell, Command::DeleteWindow);
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert_eq!(shell.panes.leaves().len(), 1);
        assert!(shell.buffers.index_of(tree_id).is_some());
        assert!(shell.tree_window.hidden.is_some());
    });
}

#[gpui::test]
fn toggle_restores_the_same_view_point_and_side_protection(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (mut visual, shell) = open(&fixture, cx);
    command(&mut visual, &shell, Command::Treemacs);
    wait(&mut visual, &shell);
    let view = visual.read(|cx| shell.read(cx).active_view().clone());
    visual.update(|window, cx| {
        view.update(cx, |view, cx| {
            let sections = view.buffer().sections().unwrap();
            view.show_location(sections.nodes().last().unwrap().range.start, window, cx);
        })
    });
    let point = visual.read(|cx| view.read(cx).buffer().point());
    command(&mut visual, &shell, Command::Treemacs);
    command(&mut visual, &shell, Command::Treemacs);
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert_eq!(shell.active_view(), &view);
        assert_eq!(view.read(cx).buffer().point(), point);
        assert_eq!(shell.buffer_count(), 2);
    });
    command(&mut visual, &shell, Command::OtherWindow);
    command(&mut visual, &shell, Command::DeleteOtherWindows);
    visual.read(|cx| assert_eq!(shell.read(cx).panes.leaves().len(), 2));
}

#[gpui::test]
fn sidebar_uses_35_text_columns_and_does_not_shrink_with_ordinary_splits(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (mut visual, shell) = open(&fixture, cx);
    command(&mut visual, &shell, Command::Treemacs);
    wait(&mut visual, &shell);
    let view = visual.read(|cx| shell.read(cx).active_view().clone());
    let width = visual.read(|cx| view.read(cx).viewport_layout().unwrap().bounds().size.width);
    visual.read(|cx| {
        assert_eq!(width, view.read(cx).column_width(35, cx) - px(16.));
    });
    command(&mut visual, &shell, Command::OtherWindow);
    command(&mut visual, &shell, Command::SplitWindowBelow);
    wait(&mut visual, &shell);
    visual.read(|cx| {
        assert_eq!(
            view.read(cx).viewport_layout().unwrap().bounds().size.width,
            width
        )
    });
}

#[gpui::test]
fn late_directory_refresh_does_not_steal_focus(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (mut visual, shell) = open(&fixture, cx);
    command(&mut visual, &shell, Command::Treemacs);
    wait(&mut visual, &shell);
    let tree = visual.read(|cx| shell.read(cx).active_view().clone());
    fixture.file("new.txt");
    let (send, receive) = futures_channel::oneshot::channel();
    visual.update(|_, cx| shell.update(cx, |shell, _| shell.tree_read_gate = Some(receive)));
    command(&mut visual, &shell, Command::TreemacsRefresh);
    command(&mut visual, &shell, Command::OtherWindow);
    let selected = visual.read(|cx| *shell.read(cx).panes.selected());
    send.send(()).unwrap();
    wait(&mut visual, &shell);
    visual.update(|window, cx| {
        let shell = shell.read(cx);
        assert_eq!(*shell.panes.selected(), selected);
        assert!(shell.active_view().focus_handle(cx).is_focused(window));
        assert!(
            tree.read(cx)
                .buffer()
                .text()
                .to_string()
                .contains("new.txt")
        );
    });
}

#[gpui::test]
fn killing_a_tree_closes_the_side_window_and_cancels_its_pending_read(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (mut visual, shell) = open(&fixture, cx);
    let (send, receive) = futures_channel::oneshot::channel();
    visual.update(|_, cx| shell.update(cx, |shell, _| shell.tree_read_gate = Some(receive)));
    command(&mut visual, &shell, Command::Treemacs);
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let index = shell.active_index();
            shell.remove_buffer(index, window, cx);
        })
    });
    let _ = send.send(());
    wait(&mut visual, &shell);
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert_eq!(shell.buffer_count(), 1);
        assert!(shell.panes.side().is_none());
        assert!(shell.tree_window.hidden.is_none());
        assert!(shell.tree_work.is_none());
    });
}

#[gpui::test]
fn tree_refresh_failure_preserves_text_and_reports_the_error(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (mut visual, shell) = open(&fixture, cx);
    command(&mut visual, &shell, Command::Treemacs);
    wait(&mut visual, &shell);
    let text = visual.read(|cx| {
        shell
            .read(cx)
            .active_view()
            .read(cx)
            .buffer()
            .text()
            .to_string()
    });
    std::fs::remove_dir_all(&fixture.0).unwrap();
    command(&mut visual, &shell, Command::TreemacsRefresh);
    wait(&mut visual, &shell);
    visual.read(|cx| {
        let view = shell.read(cx).active_view().read(cx);
        assert_eq!(view.buffer().text().to_string(), text);
        assert!(view.message().unwrap().starts_with("Tree unchanged:"));
    });
}

#[gpui::test]
fn messages_use_an_ordinary_pane_instead_of_replacing_the_tree(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (mut visual, shell) = open(&fixture, cx);
    command(&mut visual, &shell, Command::Treemacs);
    wait(&mut visual, &shell);
    let tree_id = visual.read(|cx| shell.read(cx).active_buffer_id());
    command(&mut visual, &shell, Command::ViewEchoAreaMessages);
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert_eq!(
            shell.body_panes[shell.panes.side().unwrap()].current,
            tree_id
        );
        assert_eq!(shell.panes.leaves().len(), 2);
    });
}

#[gpui::test]
fn opening_a_minibuffer_during_read_does_not_lose_its_focus(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (mut visual, shell) = open(&fixture, cx);
    let (send, receive) = futures_channel::oneshot::channel();
    visual.update(|_, cx| shell.update(cx, |shell, _| shell.tree_read_gate = Some(receive)));
    command(&mut visual, &shell, Command::Treemacs);
    command(&mut visual, &shell, Command::ExecuteExtendedCommand);
    send.send(()).unwrap();
    wait(&mut visual, &shell);
    visual.update(|window, cx| {
        let shell = shell.read(cx);
        assert!(shell.session.is_some());
        assert!(
            shell
                .minibuffer
                .as_ref()
                .unwrap()
                .view
                .focus_handle(cx)
                .is_focused(window)
        );
    });
}
