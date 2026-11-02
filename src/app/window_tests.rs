use crate::app::AppShell;
use crate::buffer::Buffer;
use crate::command::Command;
use gpui_kit as gpui;
use gpui_kit::{TestAppContext, VisualTestContext, px, size};

#[gpui::test]
fn split_commands_in_one_update_cannot_reuse_stale_bounds(cx: &mut TestAppContext) {
    cx.update(crate::app::init);
    let window = cx.open_window(size(px(960.), px(640.)), |window, cx| {
        AppShell::new(Buffer::scratch(), window, cx)
    });
    let shell = window.entity(cx).unwrap();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.update(|window, cx| {
        window.activate_window();
        window.draw(cx).clear(cx);
        shell.update(cx, |shell, cx| {
            shell.window_command(Command::SplitWindowRight, window, cx);
            shell.window_command(Command::SplitWindowRight, window, cx);
            shell.window_command(Command::SplitWindowRight, window, cx);
            assert_eq!(shell.panes.leaves().len(), 2);
            assert_eq!(
                shell.active_view().read(cx).message(),
                Some("Window layout not ready for splitting; try again")
            );
        });
    });
}

#[gpui::test]
fn activating_a_cached_buffer_cannot_restore_pre_split_geometry(cx: &mut TestAppContext) {
    cx.update(crate::app::init);
    let window = cx.open_window(size(px(400.), px(300.)), |window, cx| {
        AppShell::new(Buffer::scratch(), window, cx)
    });
    let shell = window.entity(cx).unwrap();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let other = visual.update(|window, cx| {
        window.activate_window();
        window.draw(cx).clear(cx);
        let index = shell.update(cx, |shell, cx| {
            let index = shell.create_buffer(Buffer::named("other"), window, cx);
            shell.activate(index, window, cx);
            index
        });
        window.draw(cx).clear(cx);
        shell.update(cx, |shell, cx| {
            shell.activate(0, window, cx);
        });
        window.draw(cx).clear(cx);
        index
    });
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.window_command(Command::SplitWindowRight, window, cx);
            shell.activate(other, window, cx);
            shell.window_command(Command::SplitWindowRight, window, cx);
            assert_eq!(shell.panes.leaves().len(), 2);
            assert_eq!(
                shell.active_view().read(cx).message(),
                Some("Window layout not ready for splitting; try again")
            );
        });
    });
}

#[gpui::test]
fn ordinary_view_notifications_do_not_synchronize_unrelated_cached_buffers(
    cx: &mut TestAppContext,
) {
    cx.update(crate::app::init);
    let window = cx.open_window(size(px(960.), px(640.)), |window, cx| {
        AppShell::new(Buffer::scratch(), window, cx)
    });
    let shell = window.entity(cx).unwrap();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let hidden = visual.update(|window, cx| {
        window.activate_window();
        window.draw(cx).clear(cx);
        shell.update(cx, |shell, cx| {
            let index = shell.create_buffer(Buffer::named("hidden"), window, cx);
            shell.buffer_view(index).unwrap().clone()
        })
    });
    visual.run_until_parked();
    let notifications = std::rc::Rc::new(std::cell::Cell::new(0));
    let subscription = visual.update(|_, cx| {
        let notifications = notifications.clone();
        cx.observe(&hidden, move |_, _| {
            notifications.set(notifications.get() + 1)
        })
    });
    visual.simulate_keystrokes("ctrl-f ctrl-x ctrl-z");
    visual.run_until_parked();
    assert_eq!(notifications.get(), 0);
    drop(subscription);
}
