use gpui_kit::*;

use crate::buffer::Buffer;
use crate::editing::{EditTarget, KillRing};
use crate::text::ByteOffset;
use crate::view::BufferView;

pub struct Minibuffer {
    editor: Entity<BufferView>,
    _editor_subscription: Subscription,
}

impl Minibuffer {
    pub fn new(prompt: &str, initial: &str, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let target = EditTarget::with_kill_ring(
            Buffer::minibuffer(prompt, initial),
            cx.default_global::<KillRing>().clone(),
        )
        .with_field_boundary(ByteOffset(prompt.len()))
        .expect("valid prompt field boundary");
        let editor = cx.new(|cx| BufferView::with_target(target, cx));
        editor.update(cx, |view, cx| view.install_input_lifecycle(window, cx));
        let subscription = cx.observe(&editor, |_, _, cx| cx.notify());
        Self {
            editor,
            _editor_subscription: subscription,
        }
    }

    pub fn editor(&self) -> &Entity<BufferView> {
        &self.editor
    }

    pub fn prompt(&self, cx: &App) -> String {
        let buffer = self.editor.read(cx).buffer();
        buffer
            .text()
            .chunks_in_range(0..buffer.read_only_prefix().0)
            .collect()
    }

    pub fn contents(&self, cx: &App) -> String {
        let buffer = self.editor.read(cx).buffer();
        buffer
            .text()
            .chunks_in_range(buffer.read_only_prefix().0..buffer.text().len())
            .collect()
    }
}

impl Focusable for Minibuffer {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.editor.focus_handle(cx)
    }
}

impl Render for Minibuffer {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("minibuffer")
            .w_full()
            .h(px(40.))
            .flex_shrink_0()
            .child(self.editor.clone())
    }
}
