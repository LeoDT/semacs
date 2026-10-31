//! The single M0 text view over a buffer.
//!
//! Keeping `Editor` and the input-handler registration in this module is what
//! lets the command layer treat `Buffer` as an editing target instead of a
//! widget (architecture section 6).

use gpui_kit::component::input::Editor;
use gpui_kit::*;

use super::{Buffer, BufferEvent};

pub struct BufferView {
    buffer: Entity<Buffer>,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl BufferView {
    pub fn new(buffer: Entity<Buffer>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = buffer.read(cx).focus_handle(cx);
        let subscriptions =
            vec![cx.subscribe_in(&buffer, window, |_, _, _: &BufferEvent, _, cx| cx.notify())];
        Self {
            buffer,
            focus,
            _subscriptions: subscriptions,
        }
    }
}

impl Render for BufferView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let editor = self.buffer.read(cx).editor();
        let focus = self.focus.clone();
        let target = self.buffer.clone();

        div()
            .id("buffer-view")
            .relative()
            .size_full()
            .child(Editor::new(&editor).bordered(false).p_0().h(relative(1.0)))
            .child(
                canvas(
                    |_, _, _| (),
                    move |bounds, _, window, cx| {
                        window.handle_input(
                            &focus,
                            ElementInputHandler::new(bounds, target.clone()),
                            cx,
                        );
                    },
                )
                .absolute()
                .inset_0(),
            )
    }
}
