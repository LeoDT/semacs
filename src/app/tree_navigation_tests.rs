use super::AppShell;
use super::tests::{command, open, point_at, wait};
use crate::buffer::Buffer;
use crate::command::Command;
use crate::panes::Axis;
use crate::panes::Pane;
use crate::session::ReadKind;
use crate::tree::tests::Fixture;
use crate::view::BufferView;
use gpui_kit as gpui;
use gpui_kit::{Entity, Focusable, TestAppContext, VisualTestContext};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn tree_view(visual: &VisualTestContext, shell: &Entity<AppShell>) -> Entity<BufferView> {
    visual.read(|cx| {
        let shell = shell.read(cx);
        shell.body_panes[shell.panes.side().unwrap()]
            .active_view()
            .clone()
    })
}

fn assert_path(visual: &VisualTestContext, shell: &Entity<AppShell>, path: &Path) {
    visual.read(|cx| {
        assert_eq!(
            shell
                .read(cx)
                .active_view()
                .read(cx)
                .buffer()
                .path()
                .as_deref(),
            Some(path)
        );
    });
}

fn show_tree(visual: &mut VisualTestContext, shell: &Entity<AppShell>) {
    command(visual, shell, Command::TreemacsSelectWindow);
    wait(visual, shell);
}

fn activate_file(visual: &mut VisualTestContext, shell: &Entity<AppShell>, path: &Path) {
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let index = shell.create_buffer(Buffer::new_file(path.to_path_buf()), window, cx);
            assert!(shell.activate(index, window, cx));
        })
    });
}

fn tick(visual: &mut VisualTestContext, shell: &Entity<AppShell>, millis: u64) {
    visual.run_until_parked();
    visual
        .executor()
        .advance_clock(Duration::from_millis(millis));
    wait(visual, shell);
}

#[gpui::test]
fn follow_resumes_after_its_idle_timer_expires_inside_the_minibuffer(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let file = fixture.file("a/file");
    let (mut visual, shell) = open(&fixture, cx);
    show_tree(&mut visual, &shell);
    let tree = tree_view(&visual, &shell);
    command(&mut visual, &shell, Command::OtherWindow);
    activate_file(&mut visual, &shell, &file);
    visual.simulate_keystrokes("alt-x");
    tick(&mut visual, &shell, 300);
    visual.read(|cx| {
        assert!(shell.read(cx).session.is_some());
        assert_eq!(tree.read(cx).buffer().point(), crate::text::ByteOffset(0));
    });
    visual.simulate_keystrokes("ctrl-g");
    tick(&mut visual, &shell, 200);
    visual.update(|window, cx| {
        let shell = shell.read(cx);
        assert!(shell.session.is_none());
        assert_eq!(tree.read(cx).buffer().tree_node().unwrap().path, file);
        assert!(shell.active_view().focus_handle(cx).is_focused(window));
    });
}

#[gpui::test]
fn closing_the_selected_ordinary_pane_follows_the_remaining_active_file(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let first = fixture.file("a/file");
    let second = fixture.file("b/file");
    let (mut visual, shell) = open(&fixture, cx);
    activate_file(&mut visual, &shell, &first);
    command(&mut visual, &shell, Command::SplitWindowBelow);
    let pane = visual.read(|cx| shell.read(cx).panes.ordinary_leaves()[1]);
    visual.update(|window, cx| shell.update(cx, |shell, cx| shell.select_pane(pane, window, cx)));
    activate_file(&mut visual, &shell, &second);
    show_tree(&mut visual, &shell);
    let tree = tree_view(&visual, &shell);
    visual.update(|window, cx| shell.update(cx, |shell, cx| shell.select_pane(pane, window, cx)));
    tick(&mut visual, &shell, 200);
    visual.read(|cx| assert_eq!(tree.read(cx).buffer().tree_node().unwrap().path, second));
    visual.simulate_keystrokes("ctrl-x 0");
    assert_path(&visual, &shell, &first);
    tick(&mut visual, &shell, 200);
    visual.read(|cx| assert_eq!(tree.read(cx).buffer().tree_node().unwrap().path, first));
}

#[gpui::test]
fn killing_the_active_file_buffer_follows_its_replacement(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let first = fixture.file("a/file");
    let second = fixture.file("b/file");
    let (mut visual, shell) = open(&fixture, cx);
    show_tree(&mut visual, &shell);
    let tree = tree_view(&visual, &shell);
    command(&mut visual, &shell, Command::OtherWindow);
    activate_file(&mut visual, &shell, &first);
    activate_file(&mut visual, &shell, &second);
    tick(&mut visual, &shell, 200);
    visual.simulate_keystrokes("ctrl-x ctrl-k");
    assert_path(&visual, &shell, &first);
    tick(&mut visual, &shell, 200);
    visual.read(|cx| assert_eq!(tree.read(cx).buffer().tree_node().unwrap().path, first));
}

#[gpui::test]
fn return_reuses_visible_files_and_new_visits_apply_stay_and_close_after_loading(
    cx: &mut TestAppContext,
) {
    let fixture = Fixture::new();
    let first = fixture.file("first.txt");
    let second = fixture.file("second.txt");
    let (mut visual, shell) = open(&fixture, cx);
    let original = visual.read(|cx| shell.read(cx).active_view().clone());
    command(&mut visual, &shell, Command::SplitWindowBelow);
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let pane = shell.panes.ordinary_leaves()[1];
            shell.select_pane(pane, window, cx);
        })
    });
    activate_file(&mut visual, &shell, &first);
    let existing = visual.read(|cx| shell.read(cx).active_view().clone());
    show_tree(&mut visual, &shell);
    let tree = tree_view(&visual, &shell);
    point_at(&mut visual, &shell, &first);
    visual.simulate_keystrokes("enter");
    wait(&mut visual, &shell);
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert_eq!(shell.active_view(), &existing);
        assert_eq!(shell.panes.leaves().len(), 3);
        assert_eq!(
            original.read(cx).buffer().path(),
            Some(fixture.0.join("file.txt"))
        );
    });
    show_tree(&mut visual, &shell);
    point_at(&mut visual, &shell, &second);
    visual.simulate_keystrokes("ctrl-u enter");
    wait(&mut visual, &shell);
    visual.update(|window, cx| {
        let shell = shell.read(cx);
        assert_eq!(shell.active_view(), &tree);
        assert!(tree.focus_handle(cx).is_focused(window));
        let pane = shell.panes.ordinary_leaves()[0];
        assert_eq!(
            shell.body_panes[&pane]
                .active_view()
                .read(cx)
                .buffer()
                .path(),
            Some(second.clone())
        );
    });
    point_at(&mut visual, &shell, &first);
    visual.simulate_keystrokes("ctrl-u ctrl-u enter");
    wait(&mut visual, &shell);
    assert_path(&visual, &shell, &first);
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert!(shell.panes.side().is_none());
        assert!(shell.tree_window.hidden.is_some());
        assert_eq!(shell.panes.ordinary_leaves().len(), 2);
    });
}

#[gpui::test]
fn return_on_directories_toggles_but_explicit_visits_do_not_split_or_open_dired(
    cx: &mut TestAppContext,
) {
    let fixture = Fixture::new();
    fixture.file("dir/child");
    let directory = fixture.0.join("dir");
    let (mut visual, shell) = open(&fixture, cx);
    show_tree(&mut visual, &shell);
    point_at(&mut visual, &shell, &directory);
    visual.simulate_keystrokes("l");
    wait(&mut visual, &shell);
    let tree = tree_view(&visual, &shell);
    visual.read(|cx| assert!(tree.read(cx).buffer().text().to_string().contains("- dir")));
    let text = visual.read(|cx| tree.read(cx).buffer().text().to_string());
    for keys in ["o o", "o h", "o v", "o r", "o c"] {
        visual.simulate_keystrokes(keys);
        wait(&mut visual, &shell);
        visual.read(|cx| {
            let shell = shell.read(cx);
            assert_eq!(shell.panes.leaves().len(), 2);
            assert_eq!(shell.active_view(), &tree);
            assert_eq!(tree.read(cx).buffer().text().to_string(), text);
            assert_eq!(
                tree.read(cx).message(),
                Some("Directory visits are not supported (no dired)")
            );
        });
    }
    visual.simulate_keystrokes("ctrl-u ctrl-u enter");
    wait(&mut visual, &shell);
    visual.read(|cx| {
        assert!(shell.read(cx).panes.side().is_some());
        assert!(tree.read(cx).buffer().text().to_string().contains("+ dir"));
    });
    visual.simulate_keystrokes("q");
    show_tree(&mut visual, &shell);
    visual.simulate_keystrokes("Q");
    wait(&mut visual, &shell);
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert!(shell.panes.side().is_none());
        assert!(shell.tree_window.hidden.is_none());
        assert!(
            !shell
                .buffers
                .iter()
                .any(|slot| slot.buffer.tree_root().is_some())
        );
    });
}

#[gpui::test]
fn explicit_split_visits_split_the_adjacent_pane_in_the_requested_direction(
    cx: &mut TestAppContext,
) {
    let fixture = Fixture::new();
    let file = fixture.file("visit.txt");
    let (mut visual, shell) = open(&fixture, cx);
    for (keys, axis) in [("o h", Axis::Horizontal), ("o v", Axis::Vertical)] {
        command(&mut visual, &shell, Command::DeleteOtherWindows);
        wait(&mut visual, &shell);
        show_tree(&mut visual, &shell);
        point_at(&mut visual, &shell, &file);
        visual.simulate_keystrokes(keys);
        wait(&mut visual, &shell);
        assert_path(&visual, &shell, &file);
        visual.read(|cx| {
            let shell = shell.read(cx);
            assert_eq!(shell.panes.ordinary_leaves().len(), 2);
            assert!(
                matches!(shell.panes.root(), Pane::Split {axis: actual, ..} if *actual == axis)
            );
        });
    }
}

#[gpui::test]
fn select_window_moves_back_and_recent_visit_uses_window_not_buffer_history(
    cx: &mut TestAppContext,
) {
    let fixture = Fixture::new();
    let file = fixture.file("visit.txt");
    let (mut visual, shell) = open(&fixture, cx);
    command(&mut visual, &shell, Command::SplitWindowBelow);
    let second = visual.read(|cx| shell.read(cx).panes.ordinary_leaves()[1]);
    visual.update(|window, cx| shell.update(cx, |shell, cx| shell.select_pane(second, window, cx)));
    show_tree(&mut visual, &shell);
    command(&mut visual, &shell, Command::TreemacsSelectWindow);
    visual.read(|cx| assert_eq!(*shell.read(cx).panes.selected(), second));
    show_tree(&mut visual, &shell);
    point_at(&mut visual, &shell, &file);
    visual.simulate_keystrokes("o r");
    wait(&mut visual, &shell);
    assert_path(&visual, &shell, &file);
    visual.read(|cx| assert_eq!(*shell.read(cx).panes.selected(), second));
}

#[gpui::test]
fn failed_or_cancelled_async_visit_does_not_close_or_retarget_the_tree(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let file = fixture.file("bad.txt");
    std::fs::write(&file, [0xff]).unwrap();
    let (mut visual, shell) = open(&fixture, cx);
    show_tree(&mut visual, &shell);
    let tree = tree_view(&visual, &shell);
    point_at(&mut visual, &shell, &file);
    visual.simulate_keystrokes("o c");
    wait(&mut visual, &shell);
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert!(shell.panes.side().is_some());
        assert!(shell.active_view().read(cx).message().is_some());
        assert!(shell.file_task.is_none());
        assert!(shell.opening.is_none());
        assert!(tree.read(cx).buffer().tree_root().is_some());
    });
    std::fs::write(&file, "content").unwrap();
    show_tree(&mut visual, &shell);
    point_at(&mut visual, &shell, &file);
    command(&mut visual, &shell, Command::TreemacsVisitClose);
    command(&mut visual, &shell, Command::OtherWindow);
    wait(&mut visual, &shell);
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert_eq!(shell.active_view(), &tree);
        assert!(shell.panes.side().is_some());
        assert!(
            !shell
                .buffers
                .iter()
                .any(|slot| slot.buffer.path() == Some(file.clone()))
        );
    });
}

#[gpui::test]
fn find_file_reveals_hidden_tree_and_current_file_without_changing_selected_view(
    cx: &mut TestAppContext,
) {
    let fixture = Fixture::new();
    let file = fixture.file("a/b/c/file");
    let (mut visual, shell) = open(&fixture, cx);
    show_tree(&mut visual, &shell);
    command(&mut visual, &shell, Command::TreemacsQuit);
    activate_file(&mut visual, &shell, &file);
    let selected = visual.read(|cx| shell.read(cx).active_view().clone());
    command(&mut visual, &shell, Command::TreemacsFindFile);
    wait(&mut visual, &shell);
    let tree = tree_view(&visual, &shell);
    visual.update(|window, cx| {
        let shell = shell.read(cx);
        assert_eq!(shell.active_view(), &selected);
        assert!(selected.focus_handle(cx).is_focused(window));
        assert_eq!(tree.read(cx).buffer().tree_node().unwrap().path, file);
        let buffer = tree.read(cx).buffer();
        assert_eq!(
            buffer.sections().unwrap().collapse_point(buffer.point()),
            buffer.point()
        );
    });
}

#[gpui::test]
fn follow_waits_for_idle_uses_the_latest_file_and_never_takes_focus(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let first = fixture.file("one/file");
    let latest = fixture.file("two/file");
    let (mut visual, shell) = open(&fixture, cx);
    show_tree(&mut visual, &shell);
    let tree = tree_view(&visual, &shell);
    command(&mut visual, &shell, Command::OtherWindow);
    activate_file(&mut visual, &shell, &first);
    tick(&mut visual, &shell, 150);
    visual.read(|cx| assert_eq!(tree.read(cx).buffer().point(), crate::text::ByteOffset(0)));
    activate_file(&mut visual, &shell, &latest);
    visual.simulate_keystrokes("ctrl-f");
    tick(&mut visual, &shell, 199);
    visual.read(|cx| assert_eq!(tree.read(cx).buffer().point(), crate::text::ByteOffset(0)));
    let selected = visual.read(|cx| shell.read(cx).active_view().clone());
    tick(&mut visual, &shell, 1);
    visual.update(|window, cx| {
        assert_eq!(tree.read(cx).buffer().tree_node().unwrap().path, latest);
        assert_eq!(shell.read(cx).active_view(), &selected);
        assert!(selected.focus_handle(cx).is_focused(window));
    });
    show_tree(&mut visual, &shell);
    visual.simulate_keystrokes("t f");
    command(&mut visual, &shell, Command::OtherWindow);
    activate_file(&mut visual, &shell, &first);
    tick(&mut visual, &shell, 200);
    visual.read(|cx| assert_eq!(tree.read(cx).buffer().tree_node().unwrap().path, latest));
}

#[gpui::test]
fn follow_does_not_reopen_a_hidden_or_killed_tree(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let file = fixture.file("a/file");
    let (mut visual, shell) = open(&fixture, cx);
    show_tree(&mut visual, &shell);
    command(&mut visual, &shell, Command::OtherWindow);
    activate_file(&mut visual, &shell, &file);
    command(&mut visual, &shell, Command::Treemacs);
    tick(&mut visual, &shell, 200);
    visual.read(|cx| assert!(shell.read(cx).panes.side().is_none()));
    show_tree(&mut visual, &shell);
    command(&mut visual, &shell, Command::TreemacsKill);
    tick(&mut visual, &shell, 200);
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert!(shell.panes.side().is_none());
        assert!(
            !shell
                .buffers
                .iter()
                .any(|slot| slot.buffer.tree_root().is_some())
        );
    });
}

#[gpui::test]
fn width_read_validation_resize_and_fit_use_all_visible_nodes_not_only_the_viewport(
    cx: &mut TestAppContext,
) {
    let fixture = Fixture::new();
    let long = "longest-visible-node-abcdefghijklmnopqrstuvwxyz.txt";
    fixture.file(long);
    fixture.file("dir/hidden-node-with-an-even-longer-name-abcdefghijklmnopqrstuvwxyz.txt");
    let (mut visual, shell) = open(&fixture, cx);
    show_tree(&mut visual, &shell);
    visual.simulate_keystrokes("w");
    visual.simulate_input("invalid");
    visual.simulate_keystrokes("enter");
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert_eq!(shell.session.as_ref().unwrap().kind(), &ReadKind::TreeWidth);
        assert_eq!(shell.tree_window.columns, 35);
    });
    visual.simulate_keystrokes("ctrl-a ctrl-k");
    visual.simulate_input("42");
    visual.simulate_keystrokes("enter < >");
    wait(&mut visual, &shell);
    visual.read(|cx| assert_eq!(shell.read(cx).tree_window.columns, 42));
    visual.simulate_keystrokes("=");
    wait(&mut visual, &shell);
    visual.read(|cx| assert_eq!(shell.read(cx).tree_window.columns, 3 + long.len() as isize));
}

#[gpui::test]
fn missing_root_asks_for_directory_and_a_non_file_buffer_can_read_a_locate_path(
    cx: &mut TestAppContext,
) {
    let fixture = Fixture::new();
    let file = fixture.file("dir/file");
    let (mut visual, shell) = open(&fixture, cx);
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let index = shell.create_buffer(Buffer::named("scratch"), window, cx);
            shell.activate(index, window, cx);
            shell.startup_directory =
                PathBuf::from(format!("/semacs-missing-root-{}", std::process::id()));
        })
    });
    command(&mut visual, &shell, Command::Treemacs);
    visual.read(|cx| {
        assert_eq!(
            shell.read(cx).session.as_ref().unwrap().kind(),
            &ReadKind::TreeRoot
        )
    });
    visual.simulate_keystrokes("ctrl-a ctrl-k");
    visual.simulate_input(fixture.0.to_str().unwrap());
    visual.simulate_keystrokes("alt-enter");
    wait(&mut visual, &shell);
    visual.read(|cx| {
        assert_eq!(
            shell.read(cx).active_view().read(cx).buffer().tree_root(),
            Some(fixture.0.clone())
        )
    });
    command(&mut visual, &shell, Command::OtherWindow);
    command(&mut visual, &shell, Command::TreemacsFindFile);
    visual.read(|cx| {
        assert_eq!(
            shell.read(cx).session.as_ref().unwrap().kind(),
            &ReadKind::TreeFile
        )
    });
    visual.simulate_keystrokes("ctrl-a ctrl-k");
    visual.simulate_input(file.to_str().unwrap());
    visual.simulate_keystrokes("alt-enter");
    wait(&mut visual, &shell);
    let tree = tree_view(&visual, &shell);
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert_ne!(shell.active_view(), &tree);
        assert_eq!(tree.read(cx).buffer().tree_node().unwrap().path, file);
    });
}

#[gpui::test]
fn two_hidden_toggles_during_a_read_are_not_lost(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    fixture.file(".hidden");
    let (mut visual, shell) = open(&fixture, cx);
    show_tree(&mut visual, &shell);
    let (send, receive) = futures_channel::oneshot::channel();
    visual.update(|_, cx| shell.update(cx, |shell, _| shell.tree_read_gate = Some(receive)));
    visual.simulate_keystrokes("t h t h");
    send.send(()).unwrap();
    wait(&mut visual, &shell);
    visual.read(|cx| {
        assert!(
            shell
                .read(cx)
                .active_view()
                .read(cx)
                .buffer()
                .text()
                .to_string()
                .contains(".hidden")
        )
    });
}

#[gpui::test]
fn queued_navigation_observes_the_previous_expansion_and_then_visits_the_child(
    cx: &mut TestAppContext,
) {
    let fixture = Fixture::new();
    let file = fixture.file("a/file");
    let (mut visual, shell) = open(&fixture, cx);
    show_tree(&mut visual, &shell);
    point_at(&mut visual, &shell, &fixture.0.join("a"));
    let (send, receive) = futures_channel::oneshot::channel();
    visual.update(|_, cx| shell.update(cx, |shell, _| shell.tree_read_gate = Some(receive)));
    visual.simulate_keystrokes("tab n enter");
    send.send(()).unwrap();
    wait(&mut visual, &shell);
    assert_path(&visual, &shell, &file);
}

#[gpui::test]
fn consecutive_collapse_commands_move_to_the_parent_after_the_first_collapse(
    cx: &mut TestAppContext,
) {
    let fixture = Fixture::new();
    fixture.file("a/file");
    let (mut visual, shell) = open(&fixture, cx);
    show_tree(&mut visual, &shell);
    point_at(&mut visual, &shell, &fixture.0.join("a"));
    visual.simulate_keystrokes("tab");
    wait(&mut visual, &shell);
    let (send, receive) = futures_channel::oneshot::channel();
    visual.update(|_, cx| shell.update(cx, |shell, _| shell.tree_read_gate = Some(receive)));
    visual.simulate_keystrokes("h h");
    send.send(()).unwrap();
    wait(&mut visual, &shell);
    visual.read(|cx| {
        assert_eq!(
            shell.read(cx).active_view().read(cx).buffer().point(),
            crate::text::ByteOffset(0)
        )
    });
}

#[gpui::test]
fn width_extremes_use_window_geometry_and_never_remove_the_ordinary_pane(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (mut visual, shell) = open(&fixture, cx);
    show_tree(&mut visual, &shell);
    for width in [-10, 0, isize::MAX] {
        visual.update(|_, cx| shell.update(cx, |shell, cx| shell.set_tree_width(width, cx)));
        wait(&mut visual, &shell);
        visual.update(|window, cx| {
            let shell = shell.read(cx);
            let pane = shell.panes.ordinary_leaves()[0];
            assert!(
                shell.body_panes[&pane]
                    .active_view()
                    .read(cx)
                    .viewport_layout()
                    .unwrap()
                    .bounds()
                    .size
                    .width
                    >= crate::view::MIN_PANE_WIDTH - gpui_kit::px(16.)
            );
            assert!(shell.tree_columns(window, cx) >= 2);
        });
    }
}

#[gpui::test]
fn file_tab_reports_missing_tags_without_editing_the_buffer(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (mut visual, shell) = open(&fixture, cx);
    show_tree(&mut visual, &shell);
    point_at(&mut visual, &shell, &fixture.0.join("file.txt"));
    let tree = tree_view(&visual, &shell);
    let text = visual.read(|cx| tree.read(cx).buffer().text().to_string());
    visual.simulate_keystrokes("tab");
    visual.read(|cx| {
        assert_eq!(tree.read(cx).buffer().text().to_string(), text);
        assert!(
            tree.read(cx)
                .message()
                .unwrap()
                .starts_with("No tags found for")
        );
    });
}

#[gpui::test]
fn failed_locate_preserves_projection_and_does_not_change_the_project(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (mut visual, shell) = open(&fixture, cx);
    show_tree(&mut visual, &shell);
    let tree = tree_view(&visual, &shell);
    let before = visual.read(|cx| tree.read(cx).buffer().text().to_string());
    command(&mut visual, &shell, Command::OtherWindow);
    activate_file(
        &mut visual,
        &shell,
        &fixture.0.parent().unwrap().join("outside"),
    );
    command(&mut visual, &shell, Command::TreemacsFindFile);
    wait(&mut visual, &shell);
    visual.read(|cx| {
        assert_eq!(tree.read(cx).buffer().text().to_string(), before);
        assert_eq!(tree.read(cx).buffer().tree_root(), Some(fixture.0.clone()));
        assert!(
            shell
                .read(cx)
                .active_view()
                .read(cx)
                .message()
                .unwrap()
                .contains("does not fall under")
        );
    });
}
