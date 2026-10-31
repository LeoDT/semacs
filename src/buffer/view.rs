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

/// gpui-base adds the scroll offset to the editor's own input bounds because
/// that origin is also used for painting content. The overlay starts at the
/// viewport origin, so it has to apply the same offset exactly once.
fn input_element_bounds(viewport: Bounds<Pixels>, scroll_offset: Point<Pixels>) -> Bounds<Pixels> {
    Bounds {
        origin: viewport.origin + scroll_offset,
        size: viewport.size,
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
                        // Editor prepaint adds the scroll offset to its own input
                        // bounds (gpui-base element.rs). This overlay sits at the
                        // viewport origin, so apply the same adjustment or IME
                        // candidate placement drifts by exactly the scroll amount.
                        let scroll = target.read(cx).scroll_offset(cx);
                        let input_bounds = input_element_bounds(bounds, scroll);
                        window.handle_input(
                            &focus,
                            ElementInputHandler::new(input_bounds, target.clone()),
                            cx,
                        );
                    },
                )
                .absolute()
                .inset_0(),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::input_element_bounds;
    use gpui_kit::{Bounds, point, px, size};

    #[test]
    fn input_bounds_apply_the_scroll_offset_once() {
        let viewport = Bounds {
            origin: point(px(10.), px(20.)),
            size: size(px(300.), px(200.)),
        };
        let adjusted = input_element_bounds(viewport, point(px(-5.), px(-4000.)));
        assert_eq!(adjusted.origin, point(px(5.), px(-3980.)));
        assert_eq!(adjusted.size, viewport.size);
    }
}
