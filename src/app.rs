use gpui_kit::component::{ActiveTheme, v_flex};
use gpui_kit::*;

use crate::buffer::Buffer;
use crate::echo::EchoArea;
use crate::editing::KillRing;
use crate::frame;
use crate::minibuffer::Minibuffer;
use crate::view::BufferView;

impl Global for KillRing {}

pub fn init(cx: &mut App) {
    gpui_kit::init(cx);
    crate::prompt::init(cx);
}

pub struct AppShell {
    editor: Entity<BufferView>,
    echo: EchoArea,
    _editor_subscription: Subscription,
    minibuffer: Option<Entity<Minibuffer>>,
    minibuffer_subscription: Option<Subscription>,
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
        echo.set("M1 editing and undo/redo available; file commands are not available yet");
        Self {
            editor,
            echo,
            _editor_subscription: subscription,
            minibuffer: None,
            minibuffer_subscription: None,
            close_pending: false,
        }
    }

    fn request_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.close_pending {
            return false;
        }
        let restore = self.focused_editor(window, cx);
        let mut targets = vec![self.editor.clone()];
        if let Some(minibuffer) = &self.minibuffer {
            targets.push(minibuffer.read(cx).editor().clone());
        }
        for target in &targets {
            target.update(cx, |view, cx| view.suspend_input(cx));
        }
        if !self.editor.read(cx).buffer().is_modified() {
            return true;
        }
        self.close_pending = true;
        let response = window.prompt(
            PromptLevel::Warning,
            "Discard unsaved changes?",
            Some("Saving is not available yet."),
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
                        for target in &targets {
                            target.update(cx, |view, cx| view.resume_accepting_input(cx));
                        }
                        window.focus(&restore.focus_handle(cx), cx);
                        cx.notify();
                    }
                })
                .ok();
        })
        .detach();
        false
    }

    pub fn show_minibuffer(
        &mut self,
        prompt: &str,
        initial: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<Minibuffer>> {
        if self.close_pending {
            return None;
        }
        self.focused_editor(window, cx)
            .update(cx, |view, cx| view.end_input(cx));
        self.remove_minibuffer(cx);
        let minibuffer = cx.new(|cx| Minibuffer::new(prompt, initial, window, cx));
        self.minibuffer_subscription = Some(cx.observe(&minibuffer, |_, _, cx| cx.notify()));
        window.focus(&minibuffer.focus_handle(cx), cx);
        self.minibuffer = Some(minibuffer.clone());
        cx.notify();
        Some(minibuffer)
    }

    pub fn hide_minibuffer(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.close_pending || self.minibuffer.is_none() {
            return false;
        }
        self.remove_minibuffer(cx);
        self.editor
            .update(cx, |view, cx| view.restore_input(window, cx));
        cx.notify();
        true
    }

    fn remove_minibuffer(&mut self, cx: &mut Context<Self>) {
        if let Some(minibuffer) = self.minibuffer.take() {
            minibuffer
                .read(cx)
                .editor()
                .clone()
                .update(cx, |view, cx| view.suspend_input(cx));
        }
        self.minibuffer_subscription = None;
    }

    fn focused_editor(&self, window: &Window, cx: &App) -> Entity<BufferView> {
        self.minibuffer
            .as_ref()
            .filter(|minibuffer| minibuffer.focus_handle(cx).is_focused(window))
            .map_or_else(
                || self.editor.clone(),
                |minibuffer| minibuffer.read(cx).editor().clone(),
            )
    }

    pub(crate) fn echo_message(&self, window: &Window, cx: &App) -> String {
        let active = self.focused_editor(window, cx);
        active
            .read(cx)
            .message()
            .unwrap_or_else(|| {
                if active == self.editor {
                    self.echo.message()
                } else {
                    ""
                }
            })
            .to_owned()
    }

    pub fn minibuffer(&self) -> Option<&Entity<Minibuffer>> {
        self.minibuffer.as_ref()
    }

    pub fn editor(&self) -> &Entity<BufferView> {
        &self.editor
    }
}

impl Render for AppShell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
        echo.set(self.echo_message(window, cx));
        let echo_area = frame::echo_area(&echo, cx);
        v_flex()
            .id("app")
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(v_flex().flex_1().min_h_0().child(self.editor.clone()))
            .child(mode_line)
            .children(self.minibuffer.clone())
            .child(echo_area)
    }
}
