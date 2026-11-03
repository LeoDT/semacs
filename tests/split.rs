mod support;

use gpui_kit as gpui;
use gpui_kit::{Entity, TestAppContext, VisualTestContext, point, px, size};
use semacs::app::AppShell;
use semacs::buffer::Buffer;
use semacs::text::ByteOffset;
use semacs::view::BufferView;
use std::fs;
use support::*;

fn setup(
    text: &str,
    cx: &mut TestAppContext,
) -> (VisualTestContext, Entity<AppShell>, Entity<BufferView>) {
    let result = open(Buffer::new(None, decoded(text)), cx);
    let (mut visual, shell, view) = result;
    visual.simulate_resize(size(px(960.), px(640.)));
    redraw(&mut visual);
    (visual, shell, view)
}

fn other(visual: &mut VisualTestContext, shell: &Entity<AppShell>) -> Entity<BufferView> {
    visual.simulate_keystrokes("ctrl-x o");
    active_view(visual, shell)
}

#[gpui::test]
fn split_copies_position_and_scroll_without_switching_focus(cx: &mut TestAppContext) {
    let (mut visual, shell, first) = setup(&"abcdef\n".repeat(200), cx);
    visual.simulate_keystrokes("ctrl-v ctrl-f");
    redraw(&mut visual);
    let (position, scroll) = visual.read(|cx| {
        let view = first.read(cx);
        (
            view.buffer().point(),
            view.viewport_layout().unwrap().scroll(),
        )
    });
    visual.simulate_keystrokes("ctrl-x 3");
    redraw(&mut visual);
    assert_focused(&mut visual, &first);
    let second = other(&mut visual, &shell);
    visual.read(|cx| {
        assert_ne!(first, second);
        assert_eq!(shell.read(cx).buffer_count(), 1);
        let a = first.read(cx);
        let b = second.read(cx);
        assert_eq!(a.buffer().snapshot_id(), b.buffer().snapshot_id());
        assert_eq!(b.buffer().point(), position);
        assert_eq!(b.viewport_layout().unwrap().scroll(), scroll);
        assert_eq!(
            b.viewport_layout().unwrap().bounds().left()
                - a.viewport_layout().unwrap().bounds().right(),
            px(17.)
        );
    });
}

#[gpui::test]
fn shared_edits_and_undo_preserve_independent_points_and_scroll(cx: &mut TestAppContext) {
    let (mut visual, shell, first) = setup(&"abcdef\n".repeat(200), cx);
    visual.simulate_keystrokes("ctrl-x 3");
    let second = other(&mut visual, &shell);
    visual.simulate_keystrokes("ctrl-v ctrl-f");
    redraw(&mut visual);
    let (position, scroll) = visual.read(|cx| {
        let view = second.read(cx);
        (
            view.buffer().point(),
            view.viewport_layout().unwrap().scroll(),
        )
    });
    other(&mut visual, &shell);
    visual.simulate_input("中");
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(first.read(cx).buffer().point(), ByteOffset(3));
        assert_eq!(second.read(cx).buffer().point(), ByteOffset(position.0 + 3));
        assert_eq!(second.read(cx).viewport_layout().unwrap().scroll(), scroll);
        assert_eq!(
            first.read(cx).buffer().text().to_string(),
            second.read(cx).buffer().text().to_string()
        );
        assert!(second.read(cx).layout_ready());
    });
    other(&mut visual, &shell);
    visual.simulate_keystrokes("ctrl-/");
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(second.read(cx).buffer().point(), ByteOffset(0));
        assert_eq!(first.read(cx).buffer().point(), ByteOffset(0));
        assert!(!first.read(cx).buffer().is_modified());
    });
    visual.simulate_keystrokes("ctrl-?");
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(second.read(cx).buffer().point(), ByteOffset(0));
        assert_eq!(first.read(cx).buffer().point(), ByteOffset(0));
        assert!(
            first
                .read(cx)
                .buffer()
                .text()
                .to_string()
                .starts_with("中abcdef")
        );
    });
}

#[gpui::test]
fn switching_buffers_only_changes_the_selected_pane_and_restores_its_view_state(
    cx: &mut TestAppContext,
) {
    let (mut visual, shell, first) = setup("alpha", cx);
    visual.simulate_keystrokes("ctrl-f ctrl-t ctrl-f ctrl-x 3");
    let second = other(&mut visual, &shell);
    let beta = create(&mut visual, &shell, "beta", None);
    activate(&mut visual, &shell, beta);
    visual.simulate_input("X");
    other(&mut visual, &shell);
    visual.read(|cx| {
        assert_eq!(shell.read(cx).active_view(), &first);
        assert_eq!(first.read(cx).buffer().text().to_string(), "alpha");
    });
    other(&mut visual, &shell);
    activate(&mut visual, &shell, 0);
    visual.read(|cx| {
        assert_eq!(shell.read(cx).active_view(), &second);
        assert_eq!(second.read(cx).buffer().point(), ByteOffset(2));
        assert_eq!(second.read(cx).buffer().mark(), Some(ByteOffset(1)));
        assert!(second.read(cx).buffer().mark_active());
    });
}

#[gpui::test]
fn nested_splits_cycle_in_visual_order_and_collapse_on_close(cx: &mut TestAppContext) {
    let (mut visual, shell, first) = setup("abcdef", cx);
    visual.simulate_keystrokes("ctrl-x 3");
    let second = other(&mut visual, &shell);
    visual.simulate_keystrokes("ctrl-x 2");
    redraw(&mut visual);
    assert_focused(&mut visual, &second);
    let third = other(&mut visual, &shell);
    visual.read(|cx| {
        let a = first.read(cx).viewport_layout().unwrap().bounds();
        let b = second.read(cx).viewport_layout().unwrap().bounds();
        let c = third.read(cx).viewport_layout().unwrap().bounds();
        assert!(a.right() < b.left());
        assert_ne!(third, first);
        assert_ne!(third, second);
        assert!(b.bottom() < c.top(), "upper: {b:?}; lower: {c:?}");
        assert_eq!(b.left(), c.left());
    });
    visual.read(|cx| {
        let a = first.read(cx).viewport_layout().unwrap().bounds();
        let b = second.read(cx).viewport_layout().unwrap().bounds();
        let c = third.read(cx).viewport_layout().unwrap().bounds();
        assert_eq!(b.left() - a.right(), px(17.));
        assert_eq!(c.left() - a.right(), px(17.));
    });
    assert_eq!(other(&mut visual, &shell), first);
    assert_eq!(other(&mut visual, &shell), second);
    visual.simulate_keystrokes("ctrl-x 0");
    redraw(&mut visual);
    assert_eq!(active_view(&mut visual, &shell), third);
    visual.read(|cx| assert_eq!(shell.read(cx).buffer_count(), 1));
    visual.simulate_keystrokes("ctrl-x 1 ctrl-x 0");
    redraw(&mut visual);
    assert_eq!(active_view(&mut visual, &shell), third);
    assert_eq!(other(&mut visual, &shell), third);
    visual.read(|cx| assert_eq!(third.read(cx).message(), None));
}

#[gpui::test]
fn closing_dirty_view_does_not_kill_or_prompt_and_hidden_buffer_remains_available(
    cx: &mut TestAppContext,
) {
    let (mut visual, shell, first) = setup("alpha", cx);
    visual.simulate_keystrokes("ctrl-x 3");
    other(&mut visual, &shell);
    let beta = create(&mut visual, &shell, "beta", None);
    activate(&mut visual, &shell, beta);
    visual.simulate_input("X");
    visual.simulate_keystrokes("ctrl-x 0");
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(shell.read(cx).active_view(), &first);
        assert_eq!(shell.read(cx).buffer_count(), 2);
        assert!(shell.read(cx).session().is_none());
    });
    activate(&mut visual, &shell, beta);
    let beta_view = active_view(&mut visual, &shell);
    visual.read(|cx| {
        assert_eq!(beta_view.read(cx).buffer().text().to_string(), "Xbeta");
        assert!(beta_view.read(cx).buffer().is_modified());
    });
}

#[gpui::test]
fn killing_a_shared_buffer_replaces_all_panes_without_destroying_layout(cx: &mut TestAppContext) {
    let (mut visual, shell, first) = setup("alpha", cx);
    create(&mut visual, &shell, "beta", None);
    visual.simulate_keystrokes("ctrl-x 3");
    let second = other(&mut visual, &shell);
    visual.simulate_input("X");
    visual.simulate_keystrokes("ctrl-x k enter");
    redraw(&mut visual);
    visual.simulate_keystrokes("ctrl-g");
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(shell.read(cx).buffer_count(), 3);
        assert_eq!(second.read(cx).buffer().text().to_string(), "Xalpha");
    });
    visual.simulate_keystrokes("ctrl-x k enter y");
    redraw(&mut visual);
    let right = active_view(&mut visual, &shell);
    let left = other(&mut visual, &shell);
    visual.read(|cx| {
        assert_ne!(left, right);
        assert_eq!(shell.read(cx).buffer_count(), 2);
        assert_eq!(left.read(cx).buffer().text().to_string(), "beta");
        assert_eq!(
            left.read(cx).buffer().snapshot_id(),
            right.read(cx).buffer().snapshot_id()
        );
        assert_ne!(
            left.read(cx).buffer().snapshot_id().buffer,
            first.read(cx).buffer().snapshot_id().buffer
        );
    });
}

#[gpui::test]
fn killing_the_last_buffer_creates_one_shared_scratch_for_all_panes(cx: &mut TestAppContext) {
    let (mut visual, shell, _) = setup("alpha", cx);
    visual.simulate_keystrokes("ctrl-x 3 ctrl-x k enter");
    redraw(&mut visual);
    let first = active_view(&mut visual, &shell);
    let second = other(&mut visual, &shell);
    visual.read(|cx| {
        assert_eq!(shell.read(cx).buffer_count(), 1);
        assert_eq!(first.read(cx).buffer().display_name(), "*scratch*");
        assert_eq!(
            first.read(cx).buffer().snapshot_id(),
            second.read(cx).buffer().snapshot_id()
        );
        assert!(first.read(cx).buffer().text().is_empty());
    });
}

#[gpui::test]
fn pointer_selection_updates_command_and_platform_input_target(cx: &mut TestAppContext) {
    let (mut visual, shell, first) = setup("abcdef", cx);
    visual.simulate_keystrokes("ctrl-x 3");
    let second = other(&mut visual, &shell);
    let location = visual.read(|cx| first.read(cx).viewport_layout().unwrap().bounds().origin);
    visual.simulate_click(
        point(location.x + px(1.), location.y + px(8.)),
        Default::default(),
    );
    visual.simulate_input("X");
    redraw(&mut visual);
    assert_focused(&mut visual, &first);
    visual.read(|cx| {
        assert_eq!(shell.read(cx).active_view(), &first);
        assert_eq!(first.read(cx).buffer().point(), ByteOffset(1));
        assert_eq!(second.read(cx).buffer().point(), ByteOffset(0));
        assert_eq!(second.read(cx).buffer().text().to_string(), "Xabcdef");
    });
}

#[gpui::test]
fn minibuffer_blocks_body_selection_and_restores_the_original_pane(cx: &mut TestAppContext) {
    let (mut visual, shell, first) = setup("abcdef", cx);
    visual.simulate_keystrokes("ctrl-x 3");
    let second = other(&mut visual, &shell);
    visual.simulate_keystrokes("ctrl-f ctrl-t ctrl-f alt-x");
    redraw(&mut visual);
    let location = visual.read(|cx| first.read(cx).viewport_layout().unwrap().bounds().origin);
    visual.simulate_click(
        point(location.x + px(1.), location.y + px(8.)),
        Default::default(),
    );
    visual.simulate_keystrokes("ctrl-x o");
    visual.simulate_keystrokes("ctrl-g");
    redraw(&mut visual);
    assert_focused(&mut visual, &second);
    visual.read(|cx| {
        assert_eq!(shell.read(cx).active_view(), &second);
        assert_eq!(second.read(cx).buffer().point(), ByteOffset(2));
        assert_eq!(second.read(cx).buffer().mark(), Some(ByteOffset(1)));
        assert!(second.read(cx).buffer().mark_active());
    });
}

#[gpui::test]
fn a_save_started_in_a_closed_pane_finishes_on_the_shared_buffer(cx: &mut TestAppContext) {
    let dir = TempDir::new("split-save-close");
    let path = dir.path().join("file.txt");
    fs::write(&path, b"base").unwrap();
    let (mut visual, shell, first) = open(buffer_at(&path, "base"), cx);
    visual.simulate_resize(size(px(960.), px(640.)));
    redraw(&mut visual);
    visual.simulate_keystrokes("ctrl-x 3");
    let second = other(&mut visual, &shell);
    visual.simulate_input("X");
    redraw(&mut visual);
    dispatch_keys(&mut visual, &["ctrl-x", "ctrl-s", "ctrl-x", "0"]);
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(shell.read(cx).active_view(), &first);
        assert_eq!(shell.read(cx).buffer_count(), 2);
        assert!(!first.read(cx).buffer().is_modified());
        assert!(!second.read(cx).buffer().is_modified());
    });
    assert_eq!(fs::read(&path).unwrap(), b"Xbase");
}

#[gpui::test]
fn switching_views_breaks_self_insert_groups_even_without_edits_in_the_other_view(
    cx: &mut TestAppContext,
) {
    let (mut visual, shell, first) = setup("base", cx);
    visual.simulate_keystrokes("ctrl-x 3");
    visual.simulate_input("X");
    other(&mut visual, &shell);
    other(&mut visual, &shell);
    visual.simulate_input("Y");
    visual.simulate_keystrokes("ctrl-/");
    redraw(&mut visual);
    visual.read(|cx| assert_eq!(first.read(cx).buffer().text().to_string(), "Xbase"));
    visual.simulate_keystrokes("ctrl-/");
    redraw(&mut visual);
    visual.read(|cx| assert_eq!(first.read(cx).buffer().text().to_string(), "base"));
}

#[gpui::test]
fn saves_from_two_views_share_one_queue_and_one_save_identity(cx: &mut TestAppContext) {
    let dir = TempDir::new("split-save-queue");
    let path = dir.path().join("file.txt");
    fs::write(&path, b"base").unwrap();
    let (mut visual, shell, first) = open(buffer_at(&path, "base"), cx);
    visual.simulate_resize(size(px(960.), px(640.)));
    redraw(&mut visual);
    visual.simulate_keystrokes("ctrl-x 3 ctrl-e");
    visual.simulate_input("X");
    redraw(&mut visual);
    dispatch_keys(&mut visual, &["ctrl-x", "ctrl-s", "ctrl-x", "o", "alt->"]);
    dispatch_text(&mut visual, "Y");
    dispatch_keys(&mut visual, &["ctrl-x", "ctrl-s"]);
    dispatch_text(&mut visual, "Z");
    redraw(&mut visual);
    let second = active_view(&mut visual, &shell);
    visual.read(|cx| {
        assert_ne!(first, second);
        assert_eq!(first.read(cx).buffer().text().to_string(), "baseXYZ");
        assert_eq!(second.read(cx).buffer().text().to_string(), "baseXYZ");
        assert!(first.read(cx).buffer().is_modified());
        assert!(second.read(cx).buffer().is_modified());
        assert_eq!(
            first.read(cx).buffer().path(),
            second.read(cx).buffer().path()
        );
    });
    assert_eq!(fs::read(&path).unwrap(), b"baseXY");
}

#[gpui::test]
fn find_file_publishes_only_into_the_origin_pane(cx: &mut TestAppContext) {
    let dir = TempDir::new("split-find-file");
    let path = dir.path().join("other.txt");
    fs::write(&path, b"other").unwrap();
    let (mut visual, shell, first) = setup("base", cx);
    visual.simulate_keystrokes("ctrl-x 3");
    let second = other(&mut visual, &shell);
    find_file(&mut visual, &path);
    let opened = active_view(&mut visual, &shell);
    assert_ne!(opened, second);
    visual.read(|cx| {
        assert_eq!(opened.read(cx).buffer().text().to_string(), "other");
        assert!(opened.read(cx).layout_ready());
        assert_eq!(
            opened
                .read(cx)
                .viewport_layout()
                .unwrap()
                .bounds()
                .size
                .width,
            first
                .read(cx)
                .viewport_layout()
                .unwrap()
                .bounds()
                .size
                .width
        );
        assert_eq!(first.read(cx).buffer().text().to_string(), "base");
        assert_eq!(shell.read(cx).buffer_count(), 2);
    });
    assert_eq!(other(&mut visual, &shell), first);
}

#[gpui::test]
fn an_unselected_split_does_not_break_typing_groups_on_its_first_paint(cx: &mut TestAppContext) {
    let (mut visual, shell, first) = setup("base", cx);
    dispatch_keys(&mut visual, &["ctrl-x", "3"]);
    dispatch_text(&mut visual, "a");
    redraw(&mut visual);
    visual.simulate_input("b");
    visual.simulate_keystrokes("ctrl-/");
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(shell.read(cx).active_view(), &first);
        assert_eq!(first.read(cx).buffer().text().to_string(), "base");
    });
}

#[gpui::test]
fn repeated_splits_respect_the_available_space(cx: &mut TestAppContext) {
    let (mut visual, shell, first) = setup("base", cx);
    dispatch_keys(&mut visual, &["ctrl-x", "3", "ctrl-x", "3", "ctrl-x", "3"]);
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(
            first.read(cx).message(),
            Some("Window too small for splitting")
        )
    });
    let second = other(&mut visual, &shell);
    let third = other(&mut visual, &shell);
    assert_ne!(second, first);
    assert_ne!(third, first);
    assert_ne!(third, second);
    assert_eq!(other(&mut visual, &shell), first);
}

#[gpui::test]
fn insufficient_space_refuses_split_without_changing_buffer_or_focus(cx: &mut TestAppContext) {
    let (mut visual, shell, first) = open(Buffer::scratch(), cx);
    visual.simulate_resize(size(px(220.), px(140.)));
    redraw(&mut visual);
    visual.simulate_keystrokes("ctrl-x 3");
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(
            first.read(cx).message(),
            Some("Window too small for splitting")
        )
    });
    assert_eq!(other(&mut visual, &shell), first);
    assert_focused(&mut visual, &first);
}
