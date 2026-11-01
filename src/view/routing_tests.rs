use std::cell::Cell;
use std::rc::Rc;

use gpui_kit as gpui;
use gpui_kit::{
    AppContext, Context, Entity, FocusHandle, Focusable, InputHandler, InteractiveElement,
    IntoElement, KeyBinding, KeyDownEvent, Keystroke, ParentElement, PlatformInput, Render, Styled,
    TestAppContext, VisualTestContext, Window, div, px, size,
};

use super::input_tests::{app_shell, handler, open, redraw};
use crate::app::AppShell;
use crate::buffer::Buffer;
use crate::command::Command;
use crate::files::{DecodedFile, FileFormat};
use crate::keymap::parse_sequence;
use crate::text::ByteOffset;

gpui::actions!(routing_test, [FrameworkAction]);

#[gpui::test]
fn root_and_framework_prefix_bindings_do_not_preempt_editor_routing(cx: &mut TestAppContext) {
    let (mut visual, editor) = open("abc", cx);
    let calls = Rc::new(Cell::new(0));
    visual.update(|_, cx| {
        let calls = calls.clone();
        cx.on_action(move |_: &FrameworkAction, _| calls.set(calls.get() + 1));
        cx.bind_keys([
            KeyBinding::new("ctrl-f", FrameworkAction, Some("Root")),
            KeyBinding::new("ctrl-x u", FrameworkAction, Some("Root")),
            KeyBinding::new("ctrl-z", FrameworkAction, Some("Root")),
        ]);
    });
    redraw(&mut visual);
    visual.simulate_keystrokes("tab");
    visual.update(|window, cx| {
        assert_eq!(editor.read(cx).buffer().text().to_string(), "\tabc");
        assert_eq!(editor.read(cx).buffer().point(), ByteOffset(1));
        assert!(editor.focus_handle(cx).is_focused(window));
    });
    visual.simulate_keystrokes("ctrl-f x");
    redraw(&mut visual);
    assert_eq!(
        visual.read(|cx| editor.read(cx).buffer().text().to_string()),
        "\taxbc"
    );
    visual.simulate_keystrokes("ctrl-x");
    visual.update(|window, cx| {
        assert!(editor.read(cx).target.is_pending());
        assert!(!window.has_pending_keystrokes());
    });
    visual.simulate_keystrokes("u");
    assert_eq!(
        visual.read(|cx| editor.read(cx).buffer().text().to_string()),
        "\tabc"
    );
    visual.simulate_keystrokes("ctrl-x ctrl-z");
    assert_eq!(
        visual.read(|cx| editor.read(cx).message().map(str::to_owned)),
        Some("C-x C-z is undefined".into())
    );
    visual.simulate_keystrokes("ctrl-z");
    assert_eq!(
        visual.read(|cx| editor.read(cx).message().map(str::to_owned)),
        Some("C-z is undefined".into())
    );
    assert_eq!(calls.get(), 0);
}

#[gpui::test]
fn root_tab_stays_inside_readonly_and_composing_targets(cx: &mut TestAppContext) {
    let (mut visual, editor) = open("abc", cx);
    editor.update(&mut visual, |view, _| view.target.set_read_only(true));
    visual.simulate_keystrokes("tab");
    visual.update(|window, cx| {
        assert!(editor.focus_handle(cx).is_focused(window));
        assert_eq!(editor.read(cx).message(), Some("Buffer is read-only"));
        assert_eq!(editor.read(cx).buffer().text().to_string(), "abc");
    });
    editor.update(&mut visual, |view, _| view.target.set_read_only(false));
    redraw(&mut visual);
    let mut input = handler(&editor, &visual);
    visual.update(|window, cx| input.replace_and_mark_text_in_range(None, "ni", None, window, cx));
    redraw(&mut visual);
    let mut input = handler(&editor, &visual);
    visual.update(|window, cx| {
        window.dispatch_keystroke(Keystroke::parse("tab").unwrap(), cx);
        assert!(editor.focus_handle(cx).is_focused(window));
        assert_eq!(
            editor.read(cx).message(),
            Some("Input method composition is active; finish it first")
        );
        assert!(input.accepts_text_input(window, cx));
        input.replace_text_in_range(None, "你", window, cx);
        assert_eq!(editor.read(cx).buffer().text().to_string(), "你abc");
    });
}

#[gpui::test]
fn focused_minibuffer_owns_tab_and_a_retained_hidden_target_does_not_intercept(
    cx: &mut TestAppContext,
) {
    let (mut visual, body) = open("body", cx);
    let (shell, mini) = visual.update(|window, cx| {
        let shell = app_shell(window, cx);
        let mini = shell.update(cx, |shell, cx| {
            shell.show_minibuffer("P: ", "", window, cx).unwrap()
        });
        (shell, mini)
    });
    redraw(&mut visual);
    visual.simulate_keystrokes("tab");
    visual.update(|window, cx| {
        assert!(mini.focus_handle(cx).is_focused(window));
        assert_eq!(mini.read(cx).contents(cx), "\t");
        assert_eq!(body.read(cx).buffer().text().to_string(), "body");
        assert!(shell.update(cx, |shell, cx| shell.hide_minibuffer(window, cx)));
    });
    redraw(&mut visual);
    visual.simulate_keystrokes("tab");
    assert_eq!(
        visual.read(|cx| body.read(cx).buffer().text().to_string()),
        "\tbody"
    );
    assert_eq!(visual.read(|cx| mini.read(cx).contents(cx)), "\t");
}

#[gpui::test]
fn interceptors_only_route_the_dispatching_windows_focused_target(cx: &mut TestAppContext) {
    let (mut first, a) = open("a", cx);
    let (mut second, b) = open("b", cx);
    first.update(|window, _| window.activate_window());
    redraw(&mut first);
    first.simulate_keystrokes("tab");
    second.update(|window, _| window.activate_window());
    redraw(&mut second);
    second.simulate_keystrokes("tab");
    assert_eq!(
        first.read(|cx| a.read(cx).buffer().text().to_string()),
        "\ta"
    );
    assert_eq!(
        second.read(|cx| b.read(cx).buffer().text().to_string()),
        "\tb"
    );
    second.simulate_keystrokes("ctrl-x");
    assert!(!first.read(|cx| a.read(cx).target.is_pending()));
    assert!(second.read(|cx| b.read(cx).target.is_pending()));
    second.simulate_keystrokes("ctrl-g");
}

#[gpui::test]
fn resolved_character_preference_preserves_platform_input_over_editor_and_framework_bindings(
    cx: &mut TestAppContext,
) {
    let (mut visual, editor) = open("abc", cx);
    editor.update(&mut visual, |view, _| {
        view.keymap
            .bind(&parse_sequence("ctrl-alt-a").unwrap(), Command::ForwardChar)
            .unwrap();
    });
    let calls = Rc::new(Cell::new(0));
    visual.update(|_, cx| {
        let calls = calls.clone();
        cx.on_action(move |_: &FrameworkAction, _| calls.set(calls.get() + 1));
        cx.bind_keys([KeyBinding::new("ctrl-alt-a", FrameworkAction, Some("Root"))]);
    });
    redraw(&mut visual);
    visual.update(|window, cx| {
        let mut key = Keystroke::parse("ctrl-alt-a").unwrap();
        key.key_char = Some("@".into());
        let result = window.dispatch_event(
            PlatformInput::KeyDown(KeyDownEvent {
                keystroke: key,
                is_held: false,
                prefer_character_input: true,
            }),
            cx,
        );
        assert!(result.propagate);
        assert_eq!(editor.read(cx).buffer().point(), ByteOffset(0));
    });
    visual.simulate_input("@");
    redraw(&mut visual);
    assert_eq!(
        visual.read(|cx| editor.read(cx).buffer().text().to_string()),
        "@abc"
    );
    visual.simulate_keystrokes("ctrl-alt-a");
    assert_eq!(
        visual.read(|cx| editor.read(cx).buffer().point()),
        ByteOffset(2)
    );
    assert_eq!(calls.get(), 0);
}

struct Controls {
    shell: Entity<AppShell>,
    first: FocusHandle,
    second: FocusHandle,
}

impl Render for Controls {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(self.shell.clone())
            .child(div().track_focus(&self.first).child("first"))
            .child(div().track_focus(&self.second).child("second"))
    }
}

#[gpui::test]
fn root_tab_traversal_and_framework_actions_still_work_outside_editor_focus(
    cx: &mut TestAppContext,
) {
    cx.update(crate::app::init);
    let mut controls = None;
    let window = cx.open_window(size(px(400.), px(240.)), |window, cx| {
        let host = cx.new(|cx| Controls {
            shell: cx.new(|cx| {
                AppShell::new(
                    Buffer::new(
                        None,
                        DecodedFile {
                            text: "abc".into(),
                            format: FileFormat::default(),
                        },
                    ),
                    window,
                    cx,
                )
            }),
            first: cx.focus_handle().tab_stop(true),
            second: cx.focus_handle().tab_stop(true),
        });
        controls = Some(host.clone());
        gpui_kit::base::Root::new(host, window, cx)
    });
    let controls = controls.unwrap();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    redraw(&mut visual);
    let body = visual.read(|cx| controls.read(cx).shell.read(cx).active_view().clone());
    let calls = Rc::new(Cell::new(0));
    visual.update(|window, cx| {
        let calls = calls.clone();
        cx.on_action(move |_: &FrameworkAction, _| calls.set(calls.get() + 1));
        cx.bind_keys([KeyBinding::new("ctrl-z", FrameworkAction, Some("Root"))]);
        let first = controls.read(cx).first.clone();
        window.focus(&first, cx);
    });
    redraw(&mut visual);
    visual.simulate_keystrokes("tab ctrl-z");
    visual.update(|window, cx| assert!(controls.read(cx).second.is_focused(window)));
    assert_eq!(calls.get(), 1);
    visual.update(|window, cx| {
        window.focus(&body.focus_handle(cx), cx);
        body.update(cx, |view, cx| view.suspend_input(cx));
    });
    visual.simulate_keystrokes("ctrl-z");
    assert_eq!(calls.get(), 2);
    assert_eq!(
        visual.read(|cx| body.read(cx).buffer().text().to_string()),
        "abc"
    );
}
