#![recursion_limit = "256"]

use gpui_kit::test::{TestQueryExt as _, TestWindowExt as _};
use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Entity, EntityInputHandler, TestAppContext, Window,
    WindowOptions,
};
use semacs::app::{AppShell, StartupDocument};
use semacs::buffer::Buffer;
use semacs::files::{DecodedFile, FileFormat};

fn open_document(
    cx: &mut TestAppContext,
    document: StartupDocument,
) -> (AnyWindowHandle, Entity<AppShell>) {
    cx.update(gpui_kit::init);
    cx.update(|cx| {
        gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
            cx.new(|cx| AppShell::new(document, window, cx))
        })
        .expect("open test window")
    })
}

fn open_app(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<AppShell>) {
    open_document(cx, StartupDocument::scratch())
}

fn open_app_with_text(cx: &mut TestAppContext, text: &str) -> (AnyWindowHandle, Entity<AppShell>) {
    open_document(
        cx,
        StartupDocument::visit(
            std::path::PathBuf::from("test.txt"),
            DecodedFile {
                text: text.to_owned(),
                format: FileFormat::default(),
            },
        ),
    )
}

fn start_preedit(buffer: &Entity<Buffer>, window: &mut Window, cx: &mut App) {
    buffer.update(cx, |buffer, cx| {
        EntityInputHandler::replace_and_mark_text_in_range(
            buffer, None, "ni hao", None, window, cx,
        );
    });
}

#[gpui_kit::test]
fn renders_the_buffer_mode_line_and_echo_area(cx: &mut TestAppContext) {
    let (handle, _) = open_app(cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            !window.elements().is_empty(),
            "the frame should register observable elements"
        );
    })
    .expect("window");
}

#[gpui_kit::test]
fn platform_text_input_reaches_the_buffer(cx: &mut TestAppContext) {
    let (handle, app) = open_app(cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
        window.input("你", cx);
        window.render_frame(cx);

        let buffer = app.read(cx).buffer().clone();
        assert_eq!(buffer.read(cx).text(cx), "你");
        assert!(buffer.read(cx).is_modified());
    })
    .expect("window");
}

#[gpui_kit::test]
fn native_editing_actions_dirty_the_buffer(cx: &mut TestAppContext) {
    let (handle, app) = open_app_with_text(cx, "hello");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);

        let buffer = app.read(cx).buffer().clone();
        buffer.update(cx, |buffer, cx| buffer.set_point(5, cx));
        window.render_frame(cx);
        window.press("backspace", cx);
    })
    .expect("window");

    cx.run_until_parked();
    cx.update(|cx| {
        let buffer = app.read(cx).buffer().clone();
        assert_eq!(buffer.read(cx).text(cx), "hell");
        assert!(buffer.read(cx).is_modified());
    });
}

#[gpui_kit::test]
fn cancelling_the_discard_dialog_keeps_the_buffer_and_focus(cx: &mut TestAppContext) {
    let (handle, app) = open_app_with_text(cx, "kept");
    cx.update_window(handle, |_, window, cx| {
        let buffer = app.read(cx).buffer().clone();
        buffer.update(cx, |buffer, cx| {
            EntityInputHandler::replace_text_in_range(buffer, Some(0..0), "x", window, cx);
        });
        assert!(!app.update(cx, |app, cx| app.should_close(window, cx)));
        window.render_frame(cx);

        window.click("cancel", cx);
        window.render_frame(cx);
    })
    .expect("window");

    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("ok").is_none(), "dialog should be gone");
        let buffer = app.read(cx).buffer().clone();
        assert_eq!(buffer.read(cx).text(cx), "xkept");
        assert!(buffer.read(cx).focus_handle(cx).is_focused(window));
    })
    .expect("window");
}

#[gpui_kit::test]
fn discarding_removes_the_window(cx: &mut TestAppContext) {
    let (handle, app) = open_app_with_text(cx, "kept");
    cx.update_window(handle, |_, window, cx| {
        let buffer = app.read(cx).buffer().clone();
        buffer.update(cx, |buffer, cx| {
            EntityInputHandler::replace_text_in_range(buffer, Some(0..0), "x", window, cx);
        });
        assert!(!app.update(cx, |app, cx| app.should_close(window, cx)));
        window.render_frame(cx);

        window.click("ok", cx);
    })
    .expect("window");

    cx.run_until_parked();
    assert!(!cx.windows().contains(&handle));
}

#[gpui_kit::test]
fn native_escape_finalizes_preedit_and_dirties(cx: &mut TestAppContext) {
    let (handle, app) = open_app(cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);

        let buffer = app.read(cx).buffer().clone();
        start_preedit(&buffer, window, cx);
        assert!(!buffer.read(cx).is_modified());
        assert_eq!(buffer.read(cx).text(cx), "ni hao");

        window.press("escape", cx);
    })
    .expect("window");

    cx.run_until_parked();
    cx.update(|cx| {
        let buffer = app.read(cx).buffer().clone();
        assert_eq!(buffer.read(cx).text(cx), "ni hao");
        assert!(buffer.read(cx).is_modified());
    });
}

#[gpui_kit::test]
fn losing_window_focus_cancels_uncommitted_preedit(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (app, cx) =
        cx.add_window_view(|window, cx| AppShell::new(StartupDocument::scratch(), window, cx));

    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
        let buffer = app.read(cx).buffer().clone();
        start_preedit(&buffer, window, cx);
        assert_eq!(buffer.read(cx).text(cx), "ni hao");
    });

    cx.deactivate_window();
    cx.run_until_parked();
    cx.update(|_, cx| {
        let buffer = app.read(cx).buffer().clone();
        assert_eq!(buffer.read(cx).text(cx), "");
        assert!(!buffer.read(cx).is_modified());
    });
}

#[gpui_kit::test]
fn clean_buffer_allows_close(cx: &mut TestAppContext) {
    let (handle, app) = open_app(cx);
    cx.update_window(handle, |_, window, cx| {
        assert!(app.update(cx, |app, cx| app.should_close(window, cx)));
    })
    .expect("window");
}

#[gpui_kit::test]
fn close_cancels_uncommitted_preedit_without_dirtying(cx: &mut TestAppContext) {
    let (handle, app) = open_app(cx);
    cx.update_window(handle, |_, window, cx| {
        let buffer = app.read(cx).buffer().clone();
        start_preedit(&buffer, window, cx);
        assert!(!buffer.read(cx).is_modified());

        assert!(app.update(cx, |app, cx| app.should_close(window, cx)));
        assert!(!buffer.read(cx).is_modified());
        assert_eq!(buffer.read(cx).text(cx), "");
    })
    .expect("window");
}

#[gpui_kit::test]
fn dirty_buffer_keeps_committed_text_and_asks_before_close(cx: &mut TestAppContext) {
    let (handle, app) = open_app(cx);
    cx.update_window(handle, |_, window, cx| {
        let buffer = app.read(cx).buffer().clone();
        buffer.update(cx, |buffer, cx| {
            EntityInputHandler::replace_text_in_range(buffer, Some(0..0), "kept", window, cx);
        });
        start_preedit(&buffer, window, cx);

        assert!(!app.update(cx, |app, cx| app.should_close(window, cx)));
        assert!(buffer.read(cx).is_modified());
        assert_eq!(buffer.read(cx).text(cx), "kept");
    })
    .expect("window");
}
