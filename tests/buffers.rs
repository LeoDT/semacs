use std::path::PathBuf;

use gpui_kit as gpui;
use gpui_kit::{Entity, Focusable, TestAppContext, VisualTestContext, point, px, size};

use semacs::app::AppShell;
use semacs::buffer::Buffer;
use semacs::files::{DecodedFile, FileFormat};
use semacs::text::ByteOffset;
use semacs::view::BufferView;

fn buffer(text: &str, path: Option<&str>) -> Buffer {
    Buffer::new(
        path.map(PathBuf::from),
        DecodedFile {
            text: text.into(),
            format: FileFormat::default(),
        },
    )
}

fn open(
    text: &str,
    path: Option<&str>,
    cx: &mut TestAppContext,
) -> (VisualTestContext, Entity<AppShell>, Entity<BufferView>) {
    cx.update(semacs::app::init);
    let window = cx.open_window(size(px(400.), px(240.)), |window, cx| {
        AppShell::new(buffer(text, path), window, cx)
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

fn create(
    visual: &mut VisualTestContext,
    shell: &Entity<AppShell>,
    text: &str,
    path: Option<&str>,
) -> usize {
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.create_buffer(buffer(text, path), window, cx)
        })
    })
}

fn activate(visual: &mut VisualTestContext, shell: &Entity<AppShell>, index: usize) {
    let activated =
        visual.update(|window, cx| shell.update(cx, |shell, cx| shell.activate(index, window, cx)));
    assert!(activated, "buffer {index} should activate");
    redraw(visual);
}

fn open_mx(visual: &mut VisualTestContext, shell: &Entity<AppShell>) -> Entity<BufferView> {
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

fn assert_focused(visual: &mut VisualTestContext, view: &Entity<BufferView>) {
    visual.update(|window, cx| {
        assert!(
            view.read(cx).focus_handle(cx).is_focused(window),
            "expected the view to keep focus"
        );
    });
}

#[gpui::test]
fn activating_buffers_only_moves_rendering_and_focus(cx: &mut TestAppContext) {
    let (mut visual, shell, first) = open(
        "alpha\nbeta\ngamma\ndelta\nepsilon\nzeta\n",
        Some("/tmp/semacs-a.txt"),
        cx,
    );
    visual.simulate_keystrokes("ctrl-e");
    visual.simulate_input("X");
    visual.simulate_keystrokes("ctrl-t ctrl-n ctrl-n");
    first.update(&mut visual, |view, cx| {
        view.scroll_by(point(px(0.), px(36.)), cx);
    });
    redraw(&mut visual);
    let (text, point, mark, scroll) = visual.read(|cx| {
        let view = first.read(cx);
        let buffer = view.buffer();
        assert!(buffer.mark_active());
        assert_eq!(
            buffer.path(),
            Some(std::path::Path::new("/tmp/semacs-a.txt"))
        );
        assert!(buffer.is_modified());
        (
            buffer.text().to_string(),
            buffer.point(),
            buffer.mark(),
            view.viewport_layout().unwrap().scroll().y,
        )
    });
    assert_eq!(text, "alphaX\nbeta\ngamma\ndelta\nepsilon\nzeta\n");
    assert_eq!(scroll, px(36.));
    assert_eq!(mark, Some(ByteOffset(6)));

    let second = create(&mut visual, &shell, "one\ntwo\n", Some("/tmp/semacs-b.txt"));
    activate(&mut visual, &shell, second);
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert_eq!(shell.active_index(), second);
        assert_eq!(
            shell.active_view().read(cx).buffer().display_name(),
            "semacs-b.txt"
        );
    });
    visual.simulate_input("Y");
    visual.simulate_keystrokes("ctrl-/");
    redraw(&mut visual);
    visual.read(|cx| {
        let view = shell.read(cx).active_view();
        assert_eq!(view.read(cx).buffer().text().to_string(), "one\ntwo\n");
        assert!(!view.read(cx).buffer().is_modified());
        assert_eq!(first.read(cx).buffer().text().to_string(), text);
    });

    activate(&mut visual, &shell, 0);
    visual.read(|cx| {
        let view = first.read(cx);
        assert_eq!(view.buffer().text().to_string(), text);
        assert_eq!(view.buffer().point(), point);
        assert_eq!(view.buffer().mark(), mark);
        assert!(view.buffer().mark_active());
        assert!(view.buffer().is_modified());
        assert_eq!(view.viewport_layout().unwrap().scroll().y, scroll);
    });
    assert_focused(&mut visual, &first);
    visual.simulate_keystrokes("ctrl-/");
    redraw(&mut visual);
    visual.read(|cx| {
        let buffer = first.read(cx).buffer();
        assert_eq!(
            buffer.text().to_string(),
            "alpha\nbeta\ngamma\ndelta\nepsilon\nzeta\n"
        );
        assert!(!buffer.is_modified());
    });

    activate(&mut visual, &shell, second);
    visual.simulate_keystrokes("ctrl-?");
    redraw(&mut visual);
    visual.read(|cx| {
        let buffer = shell.read(cx).active_view().read(cx).buffer();
        assert_eq!(buffer.text().to_string(), "Yone\ntwo\n");
        assert!(buffer.is_modified());
    });
}

#[gpui::test]
fn application_commands_are_requests_and_never_edit_the_wrong_target(cx: &mut TestAppContext) {
    let (mut visual, shell, first) = open("alpha\n", None, cx);
    visual.simulate_input("Z");
    redraw(&mut visual);
    visual.simulate_keystrokes("ctrl-x ctrl-s");
    redraw(&mut visual);
    visual.read(|cx| {
        assert!(shell.read(cx).session().is_some());
        assert_eq!(
            shell.read(cx).minibuffer().unwrap().read(cx).prompt(cx),
            "Write file: "
        );
        assert_eq!(first.read(cx).buffer().text().to_string(), "Zalpha\n");
        assert!(first.read(cx).buffer().is_modified());
    });
    visual.simulate_keystrokes("ctrl-g");
    redraw(&mut visual);
    visual.read(|cx| assert!(shell.read(cx).session().is_none()));
    let second = create(&mut visual, &shell, "beta\n", None);
    activate(&mut visual, &shell, second);
    visual.simulate_keystrokes("ctrl-x ctrl-f");
    redraw(&mut visual);
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert!(shell.session().is_some());
        assert_eq!(
            shell.minibuffer().unwrap().read(cx).prompt(cx),
            "Find file: "
        );
        let view = shell.active_view();
        assert_eq!(view.read(cx).buffer().text().to_string(), "beta\n");
        assert_eq!(first.read(cx).buffer().text().to_string(), "Zalpha\n");
        assert_eq!(first.read(cx).message(), Some("Quit"));
    });
    visual.simulate_keystrokes("ctrl-g");
    redraw(&mut visual);
    visual.read(|cx| {
        assert!(shell.read(cx).session().is_none());
        assert_eq!(
            shell.read(cx).active_view().read(cx).message(),
            Some("Quit")
        );
    });
    visual.simulate_input("Z");
    redraw(&mut visual);

    let session = open_mx(&mut visual, &shell);
    visual.simulate_keystrokes("ctrl-x ctrl-f");
    redraw(&mut visual);
    visual.read(|cx| {
        assert_eq!(
            session.read(cx).message(),
            Some("C-x C-f is undefined"),
            "application bindings are not part of the session layer"
        );
        assert_eq!(
            shell
                .read(cx)
                .active_view()
                .read(cx)
                .buffer()
                .text()
                .to_string(),
            "Zbeta\n"
        );
    });
    let refused =
        visual.update(|window, cx| shell.update(cx, |shell, cx| shell.activate(0, window, cx)));
    assert!(!refused, "a session keeps its origin active");
    visual.simulate_keystrokes("ctrl-g");
    redraw(&mut visual);
    visual.read(|cx| {
        assert!(shell.read(cx).session().is_none());
        assert!(shell.read(cx).minibuffer().is_none());
    });
    let active = shell.read_with(&visual, |shell, _| shell.active_view().clone());
    assert_focused(&mut visual, &active);
}
