use gpui_kit::component::{ActiveTheme, v_flex};
use gpui_kit::*;

use crate::buffer::Buffer;
use crate::echo::EchoArea;
use crate::frame;
use crate::view::BufferView;

pub fn init(cx: &mut App) {
    gpui_kit::init(cx);
    crate::prompt::init(cx);
}

pub struct AppShell {
    editor: Entity<BufferView>,
    echo: EchoArea,
    _editor_subscription: Subscription,
    close_pending: bool,
}

impl AppShell {
    pub fn new(buffer: Buffer, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let editor = cx.new(|cx| BufferView::new(buffer, cx));
        let subscription = cx.observe(&editor, |_, _, cx| cx.notify());
        editor.update(cx, |view, cx| view.install_input_lifecycle(window, cx));
        window.focus(&editor.focus_handle(cx), cx);
        let shell = cx.weak_entity();
        window.on_window_should_close(cx, move |window, cx| {
            shell
                .update(cx, |shell, cx| shell.request_close(window, cx))
                .unwrap_or(true)
        });
        let mut echo = EchoArea::default();
        echo.set("R1 input driver; Emacs commands, undo and save are not connected yet");
        Self {
            editor,
            echo,
            _editor_subscription: subscription,
            close_pending: false,
        }
    }

    fn request_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.close_pending {
            return false;
        }
        self.editor.update(cx, |view, cx| view.suspend_input(cx));
        if !self.editor.read(cx).buffer().is_modified() {
            return true;
        }
        self.close_pending = true;
        let response = window.prompt(
            PromptLevel::Warning,
            "Discard unsaved changes?",
            Some("Saving is not available in the R1 input driver."),
            &["Cancel", "Discard"],
            cx,
        );
        cx.spawn_in(window, async move |shell, cx| {
            let discard = response.await == Ok(1);
            shell
                .update_in(cx, |shell, window, cx| {
                    shell.close_pending = false;
                    if discard {
                        window.remove_window();
                    } else {
                        shell
                            .editor
                            .update(cx, |view, cx| view.restore_input(window, cx));
                        cx.notify();
                    }
                })
                .ok();
        })
        .detach();
        false
    }

    pub fn editor(&self) -> &Entity<BufferView> {
        &self.editor
    }
}

impl Render for AppShell {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let buffer = self.editor.read(cx).buffer();
        let position = buffer.line_column();
        let mode_line = frame::mode_line(
            buffer.display_name(),
            buffer.format(),
            buffer.is_modified(),
            position,
            cx,
        );
        let mut echo = EchoArea::default();
        echo.set(
            self.editor
                .read(cx)
                .message()
                .unwrap_or(self.echo.message()),
        );
        let echo_area = frame::echo_area(&echo, cx);
        v_flex()
            .id("app")
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(v_flex().flex_1().min_h_0().child(self.editor.clone()))
            .child(mode_line)
            .child(echo_area)
    }
}
