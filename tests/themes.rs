mod support;

use gpui_kit as gpui;
use gpui_kit::{TestAppContext, VisualTestContext};
use semacs::buffer::Buffer;
use semacs::session::ReadKind;
use semacs::theme::ThemeSettings;
use support::*;

fn command(visual: &mut VisualTestContext, name: &str) {
    visual.simulate_keystrokes("alt-x");
    visual.simulate_input(name);
    visual.simulate_keystrokes("enter");
    redraw(visual);
}

#[gpui::test]
fn theme_candidates_switch_cancel_and_unknown_input_preserve_editing(cx: &mut TestAppContext) {
    let (mut visual, shell, body) = open(Buffer::new(None, decoded("alpha beta\n")), cx);
    visual.simulate_input("X");
    redraw(&mut visual);
    let snapshot = visual.read(|cx| body.read(cx).buffer().snapshot_id());
    command(&mut visual, "load-theme");
    assert_eq!(prompt(&mut visual, &shell), "Load theme: ");
    visual.read(|cx| {
        let session = shell.read(cx).session().unwrap();
        assert_eq!(session.kind(), &ReadKind::Theme);
        let list = session.candidates().unwrap();
        assert_eq!(
            list.items
                .iter()
                .map(|item| item.candidate.value.as_str())
                .collect::<Vec<_>>(),
            ["dark", "light"]
        );
        assert_eq!(list.items[1].candidate.annotation, "current");
    });
    visual.simulate_input("dark");
    visual.simulate_keystrokes("ctrl-g");
    redraw(&mut visual);
    assert_focused(&mut visual, &body);
    visual.read(|cx| assert_eq!(ThemeSettings::global(cx).active(), "light"));

    command(&mut visual, "load-theme");
    visual.simulate_input("not-a-theme");
    visual.simulate_keystrokes("enter");
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(ThemeSettings::global(cx).active(), "light");
        assert!(shell.read(cx).session().is_some());
    });
    visual.simulate_keystrokes("ctrl-a ctrl-k");
    visual.simulate_input("dark");
    visual.simulate_keystrokes("enter");
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(ThemeSettings::global(cx).active(), "dark");
        assert!(shell.read(cx).session().is_none());
        assert_eq!(body.read(cx).buffer().snapshot_id(), snapshot);
    });
    assert_focused(&mut visual, &body);
    visual.simulate_keystrokes("ctrl-/");
    redraw(&mut visual);
    visual.read(|cx| assert_eq!(body.read(cx).buffer().text().to_string(), "alpha beta\n"));
}

#[gpui::test]
fn split_hidden_and_new_views_use_the_current_theme_without_reflow(cx: &mut TestAppContext) {
    let (mut visual, shell, first) = open(Buffer::new(None, decoded("first\n")), cx);
    let other = create(&mut visual, &shell, "other", None);
    let key = visual.read(|cx| first.read(cx).wrap_index().unwrap().key().clone());
    command(&mut visual, "split-window-right");
    command(&mut visual, "load-theme");
    visual.simulate_input("dark");
    visual.simulate_keystrokes("enter");
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(ThemeSettings::global(cx).active(), "dark");
        assert_eq!(
            first.read(cx).wrap_index().unwrap().key().display.committed,
            key.display.committed
        );
    });
    activate(&mut visual, &shell, other);
    let second = active_view(&mut visual, &shell);
    redraw(&mut visual);
    assert!(visual.read(|cx| second.read(cx).layout_ready()));
    activate(&mut visual, &shell, 0);
    redraw(&mut visual);
    assert!(visual.read(|cx| first.read(cx).layout_ready()));
    command(&mut visual, "load-theme");
    visual.simulate_input("light");
    visual.simulate_keystrokes("enter");
    redraw(&mut visual);
    visual.read(|cx| assert_eq!(ThemeSettings::global(cx).active(), "light"));
}
