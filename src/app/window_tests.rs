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

#[gpui::test]
fn previous_and_next_buffer_match_emacs_window_history_and_bindings(cx: &mut TestAppContext) {
    cx.update(crate::app::init);
    let window = cx.open_window(size(px(960.), px(640.)), |window, cx| {
        AppShell::new(Buffer::scratch(), window, cx)
    });
    let shell = window.entity(cx).unwrap();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.update(|window, cx| {
        window.activate_window();
        window.draw(cx).clear(cx);
    });

    // Only one buffer: previous-buffer / next-buffer report boundary errors
    // (before any echo message lazily creates *Messages*).
    visual.simulate_keystrokes("ctrl-x left");
    visual.read(|cx| {
        assert_eq!(
            shell.read(cx).active_view().read(cx).message(),
            Some("No previous buffer")
        );
    });
    // Kill the lazily created *Messages* buffer to also verify empty next-buffer.
    let remove_messages = |visual: &mut VisualTestContext| {
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                let index = shell
                    .buffers
                    .iter()
                    .position(|slot| slot.buffer.is_message_log());
                if let Some(index) = index {
                    shell.remove_buffer(index, window, cx);
                }
            });
        });
    };
    remove_messages(&mut visual);
    visual.simulate_keystrokes("ctrl-x right");
    visual.read(|cx| {
        assert_eq!(
            shell.read(cx).active_view().read(cx).message(),
            Some("No next buffer")
        );
    });
    remove_messages(&mut visual);

    // Create A, B, C (activated in order) and background D (never shown in this window).
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let a = shell.create_buffer(Buffer::named("A"), window, cx);
            let b = shell.create_buffer(Buffer::named("B"), window, cx);
            let c = shell.create_buffer(Buffer::named("C"), window, cx);
            let d = shell
                .buffers
                .insert_background(crate::buffers::BufferSlot::new(Buffer::named("D")));
            shell.rename_buffer(d, cx);
            shell.activate(a, window, cx);
            shell.activate(b, window, cx);
            shell.activate(c, window, cx);
        });
    });

    let active_name = |visual: &VisualTestContext| {
        visual.read(|cx| {
            shell
                .read(cx)
                .active_view()
                .read(cx)
                .buffer()
                .display_name()
        })
    };
    let mru_names = |visual: &VisualTestContext| {
        visual.read(|cx| {
            let shell = shell.read(cx);
            shell
                .buffers
                .ids_in_mru()
                .iter()
                .map(|id| {
                    let index = shell.buffers.index_of(*id).unwrap();
                    shell.buffers[index].buffer.display_name()
                })
                .collect::<Vec<_>>()
        })
    };

    assert_eq!(active_name(&visual), "C");
    assert_eq!(mru_names(&visual), vec!["C", "B", "A", "*scratch*", "D"]);

    // Exact Emacs 31.1 sequence: C -> B -> A -> *scratch* -> D, then wrap -> C -> B.
    // Use ctrl-x left and ctrl-x ctrl-left to verify all bindings.
    for (keys, expected) in [
        ("ctrl-x left", "B"),
        ("ctrl-x ctrl-left", "A"),
        ("ctrl-x left", "*scratch*"),
        ("ctrl-x ctrl-left", "D"),
    ] {
        visual.simulate_keystrokes(keys);
        assert_eq!(active_name(&visual), expected, "after {keys}");
        // Global MRU order must remain untouched by previous-buffer (matches Emacs set-window-buffer).
        assert_eq!(mru_names(&visual), vec!["C", "B", "A", "*scratch*", "D"]);
    }

    // Mirror roundtrip with next-buffer via ctrl-x right and ctrl-x ctrl-right.
    for (keys, expected) in [
        ("ctrl-x right", "*scratch*"),
        ("ctrl-x ctrl-right", "A"),
        ("ctrl-x right", "B"),
        ("ctrl-x ctrl-right", "C"),
    ] {
        visual.simulate_keystrokes(keys);
        assert_eq!(active_name(&visual), expected, "after {keys}");
        assert_eq!(mru_names(&visual), vec!["C", "B", "A", "*scratch*", "D"]);
    }

    // Wrapping around in previous-buffer cycles through all buffers without getting stuck.
    let mut visited = Vec::new();
    for _ in 0..6 {
        visual.simulate_keystrokes("ctrl-x left");
        visited.push(active_name(&visual));
    }
    assert_eq!(visited, vec!["B", "A", "*scratch*", "D", "C", "B"]);

    // Split panes maintain independent previous/next history and per-pane point.
    visual.update(|window, cx| window.draw(cx).clear(cx));
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.window_command(Command::SplitWindowRight, window, cx);
            shell.window_command(Command::OtherWindow, window, cx);
        });
    });
    assert_eq!(active_name(&visual), "B");
    visual.simulate_keystrokes("ctrl-x left");
    // New pane has empty previous list, so it scans reversed global MRU -> D.
    assert_eq!(active_name(&visual), "D");
    visual.simulate_keystrokes("ctrl-x right");
    assert_eq!(active_name(&visual), "B");

    // Killing a buffer removes it cleanly from pane history and cycling skips it.
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let d = shell
                .buffers
                .iter()
                .position(|slot| slot.buffer.display_name() == "D")
                .unwrap();
            shell.remove_buffer(d, window, cx);
        });
    });
    visual.simulate_keystrokes("ctrl-x left");
    assert_ne!(active_name(&visual), "D");
}
