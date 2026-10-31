#![recursion_limit = "256"]

use gpui_kit::{
    AnyWindowHandle, AppContext as _, Context, Entity, EntityInputHandler, IntoElement, Render,
    TestAppContext, Window, WindowOptions, div,
};
use semacs::buffer::Buffer;
use semacs::files::FileFormat;

struct Harness {
    buffer: Entity<Buffer>,
}

impl Render for Harness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

fn open_buffer(cx: &mut TestAppContext, text: &str) -> (AnyWindowHandle, Entity<Buffer>) {
    cx.update(gpui_kit::init);
    let text = text.to_owned();
    cx.update(|cx| {
        let (handle, harness) =
            gpui_kit::open_window(WindowOptions::default(), cx, move |window, cx| {
                let buffer =
                    cx.new(|cx| Buffer::new(window, cx, None, text.clone(), FileFormat::default()));
                cx.new(|_| Harness { buffer })
            })
            .expect("open test window");
        let buffer = harness.read(cx).buffer.clone();
        (handle, buffer)
    })
}

#[gpui_kit::test]
fn loading_does_not_dirty_and_committed_input_does(cx: &mut TestAppContext) {
    let (handle, buffer) = open_buffer(cx, "hello");
    cx.update_window(handle, |_, window, cx| {
        assert!(!buffer.read(cx).is_modified());

        buffer.update(cx, |buffer, cx| {
            EntityInputHandler::replace_text_in_range(buffer, Some(0..0), "x", window, cx);
        });

        assert!(buffer.read(cx).is_modified());
        assert_eq!(buffer.read(cx).text(cx), "xhello");
    })
    .expect("window");
}

#[gpui_kit::test]
fn preedit_is_temporary_and_cancel_keeps_the_buffer_clean(cx: &mut TestAppContext) {
    let (handle, buffer) = open_buffer(cx, "hello");
    cx.update_window(handle, |_, window, cx| {
        buffer.update(cx, |buffer, cx| {
            EntityInputHandler::replace_and_mark_text_in_range(
                buffer, None, "ni hao", None, window, cx,
            );
        });
        assert!(!buffer.read(cx).is_modified());
        assert_eq!(buffer.read(cx).text(cx), "ni haohello");

        buffer.update(cx, |buffer, cx| {
            assert!(buffer.cancel_composition(window, cx));
        });
        assert!(!buffer.read(cx).is_modified());
        assert_eq!(buffer.read(cx).text(cx), "hello");
    })
    .expect("window");
}

#[gpui_kit::test]
fn unmark_keeps_preedit_as_committed_text_and_dirties(cx: &mut TestAppContext) {
    let (handle, buffer) = open_buffer(cx, "hello");
    cx.update_window(handle, |_, window, cx| {
        buffer.update(cx, |buffer, cx| {
            EntityInputHandler::replace_and_mark_text_in_range(
                buffer, None, "ni hao", None, window, cx,
            );
            EntityInputHandler::unmark_text(buffer, window, cx);
        });

        assert_eq!(buffer.read(cx).text(cx), "ni haohello");
        assert!(buffer.read(cx).is_modified());
    })
    .expect("window");
}

#[gpui_kit::test]
fn committing_preedit_dirties_the_buffer(cx: &mut TestAppContext) {
    let (handle, buffer) = open_buffer(cx, "hello");
    cx.update_window(handle, |_, window, cx| {
        buffer.update(cx, |buffer, cx| {
            EntityInputHandler::replace_and_mark_text_in_range(
                buffer, None, "ni hao", None, window, cx,
            );
            let marked = EntityInputHandler::marked_text_range(buffer, window, cx)
                .expect("preedit is marked");
            EntityInputHandler::replace_text_in_range(buffer, Some(marked), "你好", window, cx);
        });

        assert!(buffer.read(cx).is_modified());
        assert_eq!(buffer.read(cx).text(cx), "你好hello");
    })
    .expect("window");
}
