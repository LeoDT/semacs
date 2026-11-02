use gpui_kit as gpui;
use gpui_kit::{Entity, Focusable, TestAppContext, VisualTestContext, point, px, size};

use semacs::app::AppShell;
use semacs::buffer::Buffer;
use semacs::command::{COMMANDS, Scope};
use semacs::files::{DecodedFile, FileFormat};
use semacs::session::ReadKind;
use semacs::text::ByteOffset;
use semacs::view::BufferView;

fn buffer(text: &str) -> Buffer {
    Buffer::new(
        None,
        DecodedFile {
            text: text.into(),
            format: FileFormat::default(),
        },
    )
}

fn open(
    text: &str,
    cx: &mut TestAppContext,
) -> (VisualTestContext, Entity<AppShell>, Entity<BufferView>) {
    cx.update(semacs::app::init);
    let window = cx.open_window(size(px(400.), px(240.)), |window, cx| {
        AppShell::new(buffer(text), window, cx)
    });
    let shell = window.entity(cx).unwrap();
    let view = shell.read_with(cx, |shell, _| shell.active_view().clone());
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.update(|window, _| window.activate_window());
    redraw(&mut visual);
    (visual, shell, view)
}

fn redraw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// Opens `M-x` and returns its minibuffer editor.
fn mx(visual: &mut VisualTestContext, shell: &Entity<AppShell>) -> Entity<BufferView> {
    visual.simulate_keystrokes("alt-x");
    redraw(visual);
    visual.read(|cx| {
        shell
            .read(cx)
            .minibuffer()
            .expect("M-x opens a session")
            .read(cx)
            .editor()
            .clone()
    })
}

fn contents(visual: &mut VisualTestContext, shell: &Entity<AppShell>) -> String {
    visual.read(|cx| {
        shell
            .read(cx)
            .minibuffer()
            .expect("open minibuffer")
            .read(cx)
            .contents(cx)
    })
}

fn session_message(visual: &mut VisualTestContext, editor: &Entity<BufferView>) -> Option<String> {
    visual.read(|cx| editor.read(cx).message().map(str::to_owned))
}

fn assert_focused(visual: &mut VisualTestContext, view: &Entity<BufferView>) {
    visual.update(|window, cx| {
        assert!(
            view.read(cx).focus_handle(cx).is_focused(window),
            "expected the view to keep focus"
        );
    });
}

#[gpui::test]
fn mx_lists_candidates_and_tab_inserts_the_selection(cx: &mut TestAppContext) {
    let (mut visual, shell, body) = open("body", cx);
    let editor = mx(&mut visual, &shell);
    visual.read(|cx| {
        assert_eq!(
            shell
                .read(cx)
                .session()
                .map(|session| session.kind().clone()),
            Some(ReadKind::Command)
        );
        assert_eq!(
            shell.read(cx).minibuffer().unwrap().read(cx).prompt(cx),
            "M-x "
        );
    });

    visual.simulate_input("forw");
    redraw(&mut visual);
    assert_eq!(contents(&mut visual, &shell), "forw");
    visual.read(|cx| {
        let candidates = shell.read(cx).session().unwrap().candidates().unwrap();
        assert!(
            candidates
                .items
                .iter()
                .any(|item| item.candidate.value == "forward-word")
        );
    });
    visual.simulate_keystrokes("tab");
    redraw(&mut visual);
    assert_eq!(contents(&mut visual, &shell), "forward-char");
    visual.read(|cx| {
        let candidates = shell.read(cx).session().unwrap().candidates().unwrap();
        assert_eq!(
            candidates.selected_candidate().unwrap().value,
            "forward-char"
        );
    });
    assert_eq!(session_message(&mut visual, &editor), None);
    visual.simulate_keystrokes("tab");
    redraw(&mut visual);
    assert_eq!(contents(&mut visual, &shell), "forward-char");

    visual.simulate_keystrokes("ctrl-g");
    redraw(&mut visual);
    visual.read(|cx| {
        assert!(shell.read(cx).session().is_none());
        assert!(shell.read(cx).minibuffer().is_none());
        assert_eq!(body.read(cx).message(), Some("Quit"));
        assert_eq!(body.read(cx).buffer().text().to_string(), "body");
    });
    assert_focused(&mut visual, &body);
}

#[gpui::test]
fn mx_unique_completion_and_exact_names_run_on_the_origin(cx: &mut TestAppContext) {
    let (mut visual, shell, body) = open("abc def", cx);
    let editor = mx(&mut visual, &shell);
    visual.simulate_input("forward-cha");
    redraw(&mut visual);
    // Completion only touches the input area, never the committed prompt.
    visual.simulate_keystrokes("ctrl-a");
    redraw(&mut visual);
    visual.simulate_keystrokes("tab");
    redraw(&mut visual);
    assert_eq!(contents(&mut visual, &shell), "forward-char");
    visual.read(|cx| {
        let minibuffer = shell.read(cx).minibuffer().unwrap().read(cx);
        assert_eq!(minibuffer.prompt(cx), "M-x ");
        assert_eq!(
            minibuffer.editor().read(cx).buffer().point(),
            ByteOffset(16)
        );
    });
    assert_eq!(session_message(&mut visual, &editor), None);
    visual.simulate_keystrokes("enter");
    redraw(&mut visual);
    visual.read(|cx| {
        let state = (
            shell
                .read(cx)
                .session()
                .map(|session| session.kind().clone()),
            shell
                .read(cx)
                .minibuffer()
                .map(|mini| mini.read(cx).contents(cx)),
            body.read(cx).buffer().text().to_string(),
        );
        assert!(shell.read(cx).session().is_none(), "{state:?}");
        assert!(shell.read(cx).minibuffer().is_none());
        assert_eq!(body.read(cx).buffer().point(), ByteOffset(1));
        assert_eq!(body.read(cx).buffer().text().to_string(), "abc def");
        assert!(!body.read(cx).buffer().is_modified());
    });
    assert_focused(&mut visual, &body);

    // The origin has its own fresh input session after the command.
    visual.simulate_input("Z");
    redraw(&mut visual);
    visual.read(|cx| assert_eq!(body.read(cx).buffer().text().to_string(), "aZbc def"));

    let editor = mx(&mut visual, &shell);
    visual.simulate_input("forward-word");
    redraw(&mut visual);
    visual.simulate_keystrokes("enter");
    redraw(&mut visual);
    visual.read(|cx| {
        assert!(shell.read(cx).session().is_none());
        assert_eq!(body.read(cx).buffer().point(), ByteOffset(4));
        assert_eq!(
            editor.read(cx).buffer().text().to_string(),
            "M-x forward-word"
        );
    });
}

#[gpui::test]
fn mx_ret_accepts_the_selected_fuzzy_match(cx: &mut TestAppContext) {
    let (mut visual, shell, body) = open("body", cx);
    let _editor = mx(&mut visual, &shell);
    visual.simulate_input("fwd");
    redraw(&mut visual);
    visual.read(|cx| {
        let candidates = shell.read(cx).session().unwrap().candidates().unwrap();
        assert_eq!(
            candidates.selected_candidate().unwrap().value,
            "forward-word"
        );
    });
    visual.simulate_keystrokes("ctrl-n ctrl-p");
    assert_eq!(contents(&mut visual, &shell), "fwd");
    visual.simulate_keystrokes("enter");
    redraw(&mut visual);
    visual.read(|cx| {
        assert!(shell.read(cx).session().is_none());
        assert_eq!(body.read(cx).buffer().point(), ByteOffset(4));
        assert_eq!(body.read(cx).buffer().text().to_string(), "body");
    });
}

#[gpui::test]
fn mx_unknown_names_stay_open_and_empty_input_accepts_the_first_command(cx: &mut TestAppContext) {
    let (mut visual, shell, body) = open("body", cx);
    let editor = mx(&mut visual, &shell);
    visual.simulate_input("nosuch");
    redraw(&mut visual);
    visual.simulate_keystrokes("enter");
    redraw(&mut visual);
    visual.read(|cx| assert!(shell.read(cx).session().is_some()));
    assert_eq!(
        session_message(&mut visual, &editor),
        Some("No match".into())
    );
    assert_eq!(contents(&mut visual, &shell), "nosuch");
    visual.simulate_keystrokes("ctrl-g");
    redraw(&mut visual);
    visual.read(|cx| {
        assert!(shell.read(cx).session().is_none());
        assert_eq!(body.read(cx).message(), Some("Quit"));
    });

    let _editor = mx(&mut visual, &shell);
    visual.simulate_keystrokes("enter");
    redraw(&mut visual);
    visual.read(|cx| {
        assert!(shell.read(cx).session().is_none());
        assert_eq!(body.read(cx).message(), Some("Beginning of buffer"));
        assert_eq!(body.read(cx).buffer().text().to_string(), "body");
    });
    assert_focused(&mut visual, &body);
}

#[gpui::test]
fn mx_candidates_cover_the_body_layers_but_not_session_commands(cx: &mut TestAppContext) {
    let (mut visual, shell, _) = open("body", cx);
    let editor = mx(&mut visual, &shell);
    let count = COMMANDS
        .iter()
        .filter(|spec| spec.scope != Scope::ReadSession)
        .count();
    visual.read(|cx| {
        let candidates = shell.read(cx).session().unwrap().candidates().unwrap();
        assert_eq!(candidates.items.len(), count);
        assert!(
            candidates
                .items
                .iter()
                .all(|item| !item.candidate.annotation.contains("Candidates:"))
        );
        assert!(
            candidates
                .items
                .iter()
                .find(|item| item.candidate.value == "forward-char")
                .unwrap()
                .candidate
                .annotation
                .contains("C-f")
        );
    });

    // Session commands are not addressable through M-x either.
    visual.simulate_input("minibuffer-complete");
    redraw(&mut visual);
    visual.simulate_keystrokes("enter");
    redraw(&mut visual);
    visual.read(|cx| assert!(shell.read(cx).session().is_some()));
    assert_eq!(
        session_message(&mut visual, &editor),
        Some("No match".into())
    );
    visual.simulate_keystrokes("ctrl-g");
    redraw(&mut visual);
}

#[gpui::test]
fn mx_does_not_nest_and_cancel_restores_origin_state(cx: &mut TestAppContext) {
    let (mut visual, shell, body) = open("alpha\nbeta\ngamma\ndelta\nepsilon\nzeta\n", cx);
    visual.simulate_keystrokes("ctrl-e ctrl-t ctrl-n ctrl-n");
    body.update(&mut visual, |view, cx| {
        view.scroll_by(point(px(0.), px(36.)), cx);
    });
    redraw(&mut visual);
    let (point, mark, scroll) = visual.read(|cx| {
        let view = body.read(cx);
        assert!(view.buffer().mark_active());
        (
            view.buffer().point(),
            view.buffer().mark(),
            view.viewport_layout().unwrap().scroll().y,
        )
    });

    let editor = mx(&mut visual, &shell);
    // A session only offers its own layer; M-x is not recursively available.
    visual.simulate_keystrokes("alt-x");
    redraw(&mut visual);
    visual.read(|cx| {
        assert!(shell.read(cx).session().is_some());
        assert_eq!(editor.read(cx).message(), Some("M-x is undefined"));
        assert_eq!(
            shell.read(cx).minibuffer().unwrap().read(cx).contents(cx),
            ""
        );
    });
    visual.simulate_input("abc");
    redraw(&mut visual);
    visual.simulate_keystrokes("ctrl-g");
    redraw(&mut visual);
    visual.read(|cx| {
        assert!(shell.read(cx).session().is_none());
        let view = body.read(cx);
        assert_eq!(view.buffer().point(), point);
        assert_eq!(view.buffer().mark(), mark);
        assert!(view.buffer().mark_active());
        assert_eq!(view.viewport_layout().unwrap().scroll().y, scroll);
        assert!(!view.buffer().is_modified());
        assert_eq!(view.message(), Some("Quit"));
    });
    assert_focused(&mut visual, &body);
    // Marked region and typing still work on the restored origin.
    visual.simulate_keystrokes("ctrl-g");
    redraw(&mut visual);
    visual.simulate_input("X");
    redraw(&mut visual);
    visual.read(|cx| {
        let mut expected = "alpha\nbeta\ngamma\ndelta\nepsilon\nzeta\n".to_owned();
        expected.insert(point.0, 'X');
        assert_eq!(body.read(cx).buffer().text().to_string(), expected);
    });
}

#[gpui::test]
fn window_close_callback_can_mount_and_focus_a_minibuffer(cx: &mut TestAppContext) {
    let (mut visual, shell, _) = open("body", cx);
    // Replace the production close handler with a probe for the platform path:
    // a confirmation must be able to open, focus and receive input while the
    // close request is still pending.
    visual.update(|window, cx| {
        let shell = shell.clone();
        window.on_window_should_close(cx, move |window, cx| {
            let _ = shell.update(cx, |shell, cx| {
                shell.mount_minibuffer("Close? ", "", window, cx)
            });
            false
        });
    });
    redraw(&mut visual);
    assert!(!visual.simulate_close(), "the window must stay open");
    redraw(&mut visual);
    visual.update(|window, cx| {
        let shell = shell.read(cx);
        let minibuffer = shell.minibuffer().expect("confirmation is mounted");
        assert!(
            minibuffer
                .read(cx)
                .editor()
                .read(cx)
                .focus_handle(cx)
                .is_focused(window),
            "the minibuffer must take focus from the close callback"
        );
    });
    visual.simulate_input("y");
    redraw(&mut visual);
    assert_eq!(contents(&mut visual, &shell), "y");
}

#[gpui::test]
fn bare_minibuffer_does_not_expose_application_commands(cx: &mut TestAppContext) {
    let (mut visual, shell, _body) = open("body", cx);
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell
                .mount_minibuffer("P: ", "", window, cx)
                .expect("bare minibuffer mounts");
        });
    });
    redraw(&mut visual);
    let editor = visual.read(|cx| {
        shell
            .read(cx)
            .minibuffer()
            .unwrap()
            .read(cx)
            .editor()
            .clone()
    });
    visual.simulate_keystrokes("alt-x");
    redraw(&mut visual);
    visual.read(|cx| {
        assert!(shell.read(cx).session().is_none());
        assert_eq!(editor.read(cx).message(), Some("M-x is undefined"));
        assert_eq!(
            shell.read(cx).minibuffer().unwrap().read(cx).contents(cx),
            ""
        );
    });
}

#[gpui::test]
fn mx_runs_application_commands_through_the_shell_layer(cx: &mut TestAppContext) {
    let (mut visual, shell, body) = open("body", cx);
    visual.simulate_input("X");
    redraw(&mut visual);
    let _editor = mx(&mut visual, &shell);
    visual.simulate_input("save-buffer");
    redraw(&mut visual);
    visual.simulate_keystrokes("enter");
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(
            shell
                .read(cx)
                .session()
                .map(|session| session.kind().clone()),
            Some(ReadKind::WriteFile)
        );
        assert_eq!(
            shell.read(cx).minibuffer().unwrap().read(cx).prompt(cx),
            "Write file: "
        );
        assert_eq!(body.read(cx).buffer().text().to_string(), "Xbody");
        assert!(body.read(cx).buffer().is_modified());
    });
    visual.simulate_keystrokes("ctrl-g");
    redraw(&mut visual);
    visual.read(|cx| assert!(shell.read(cx).session().is_none()));
    assert_focused(&mut visual, &body);
}

#[gpui::test]
fn active_read_session_keeps_ret_event_routing_after_focus_drift(cx: &mut TestAppContext) {
    let (mut visual, shell, body) = open("abc", cx);
    let editor = mx(&mut visual, &shell);
    visual.simulate_input("forward-char");
    redraw(&mut visual);
    visual.update(|window, cx| window.focus(&body.focus_handle(cx), cx));
    visual.simulate_keystrokes("enter");
    redraw(&mut visual);
    visual.read(|cx| {
        let state = (
            shell
                .read(cx)
                .session()
                .map(|session| session.kind().clone()),
            shell
                .read(cx)
                .minibuffer()
                .map(|mini| mini.read(cx).contents(cx)),
            body.read(cx).buffer().text().to_string(),
        );
        assert!(shell.read(cx).session().is_none(), "{state:?}");
        assert!(shell.read(cx).minibuffer().is_none());
        assert_eq!(body.read(cx).buffer().point(), ByteOffset(1));
        assert_eq!(body.read(cx).buffer().text().to_string(), "abc");
        assert_eq!(
            editor.read(cx).buffer().text().to_string(),
            "M-x forward-char"
        );
    });
    assert_focused(&mut visual, &body);
}
