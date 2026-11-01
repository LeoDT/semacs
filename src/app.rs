use gpui_kit::component::{ActiveTheme, v_flex};
use gpui_kit::*;

use crate::buffer::Buffer;
use crate::command::{Command, Scope};
use crate::echo::EchoArea;
use crate::editing::{KillRing, RouteError};
use crate::frame;
use crate::keymap::Keymap;
use crate::minibuffer::Minibuffer;
use crate::view::{BufferView, KeyRoute};

impl Global for KillRing {}

pub fn init(cx: &mut App) {
    gpui_kit::init(cx);
    crate::prompt::init(cx);
}

struct BufferSlot {
    view: Entity<BufferView>,
    _subscription: Subscription,
}

/// Application shell: owns the buffer collection, the layers above the edit
/// target, and all keyboard routing for the window.
pub struct AppShell {
    buffers: Vec<BufferSlot>,
    active: usize,
    application_keymap: Keymap,
    session_keymap: Keymap,
    echo: EchoArea,
    minibuffer: Option<Entity<Minibuffer>>,
    minibuffer_subscription: Option<Subscription>,
    close_pending: bool,
    _keys_subscription: Subscription,
}

impl AppShell {
    pub fn new(buffer: Buffer, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let view = Self::create_view(buffer, window, cx);
        let subscription = cx.observe(&view, |_, _, cx| cx.notify());
        let owner = window.window_handle();
        let shell = cx.weak_entity();
        let keys_shell = shell.clone();
        let keys = cx.intercept_keystrokes(move |event, window, cx| {
            if window.window_handle() != owner {
                return;
            }
            let consumed = keys_shell
                .update(cx, |shell, cx| {
                    shell.route_keystroke(
                        &event.keystroke,
                        event.input_preference == InputPreference::CharacterInput,
                        window,
                        cx,
                    )
                })
                .unwrap_or(false);
            if consumed {
                cx.stop_propagation();
            }
        });
        window.on_window_should_close(cx, move |window, cx| {
            shell
                .update(cx, |shell, cx| shell.request_close(window, cx))
                .unwrap_or(true)
        });
        window.focus(&view.focus_handle(cx), cx);
        let mut echo = EchoArea::default();
        echo.set("M2 in progress: file and buffer commands are not available yet");
        Self {
            buffers: vec![BufferSlot {
                view,
                _subscription: subscription,
            }],
            active: 0,
            application_keymap: Keymap::for_scope(Scope::Application),
            session_keymap: Keymap::for_scope(Scope::ReadSession),
            echo,
            minibuffer: None,
            minibuffer_subscription: None,
            close_pending: false,
            _keys_subscription: keys,
        }
    }

    fn create_view(
        buffer: Buffer,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<BufferView> {
        let view = cx.new(|cx| BufferView::new(buffer, cx));
        view.update(cx, |view, cx| view.install_input_lifecycle(window, cx));
        view
    }

    /// Adds an inactive buffer and returns its index.
    pub fn create_buffer(
        &mut self,
        buffer: Buffer,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> usize {
        let view = Self::create_view(buffer, window, cx);
        let subscription = cx.observe(&view, |_, _, cx| cx.notify());
        self.buffers.push(BufferSlot {
            view,
            _subscription: subscription,
        });
        self.buffers.len() - 1
    }

    /// Activates an existing buffer. Switching only moves rendering and focus;
    /// each buffer keeps its own point, mark, history, scroll and file identity.
    /// Refused while a minibuffer session is open so it cannot be detached from
    /// its origin.
    pub fn activate(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(slot) = self.buffers.get(index) else {
            return false;
        };
        if self.close_pending || self.minibuffer.is_some() {
            return false;
        }
        let view = slot.view.clone();
        self.active = index;
        view.update(cx, |view, cx| view.restore_input(window, cx));
        cx.notify();
        true
    }

    pub fn active_index(&self) -> usize {
        self.active
    }

    pub fn active_view(&self) -> &Entity<BufferView> {
        &self.buffers[self.active].view
    }

    /// Keyboard routing for the window: the focused target supplies the edit
    /// layer, the shell supplies the layer above it. Commands owned by that
    /// higher layer come back as requests and are executed here, never by the
    /// edit target.
    fn route_keystroke(
        &mut self,
        stroke: &Keystroke,
        prefer_character_input: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.close_pending {
            return false;
        }
        let view = self.focused_editor(window, cx);
        let in_session = self
            .minibuffer
            .as_ref()
            .is_some_and(|minibuffer| minibuffer.read(cx).editor() == &view);
        let outer = if in_session {
            &self.session_keymap
        } else {
            &self.application_keymap
        };
        let route = view.update(cx, |view, cx| {
            view.route_key(stroke, prefer_character_input, outer, window, cx)
        });
        match route {
            KeyRoute::Unhandled => false,
            KeyRoute::Handled => true,
            KeyRoute::Requested(command) => {
                self.execute_requested(command, &view, in_session, cx);
                true
            }
        }
    }

    fn execute_requested(
        &mut self,
        command: Command,
        origin: &Entity<BufferView>,
        in_session: bool,
        cx: &mut Context<Self>,
    ) {
        debug_assert_eq!(
            in_session,
            command.scope() == Scope::ReadSession,
            "{} routed from the wrong layer",
            command.name()
        );
        // M2-B..E connect the real implementations; until then the requests are
        // reported on their origin instead of silently doing nothing.
        let message = RouteError::Unavailable(command).to_string();
        origin.update(cx, |view, cx| view.set_message(message, cx));
    }

    fn request_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.close_pending {
            return false;
        }
        let restore = self.focused_editor(window, cx);
        let mut targets = vec![self.active_view().clone()];
        if let Some(minibuffer) = &self.minibuffer {
            targets.push(minibuffer.read(cx).editor().clone());
        }
        for target in &targets {
            target.update(cx, |view, cx| view.suspend_input(cx));
        }
        if !self
            .buffers
            .iter()
            .any(|slot| slot.view.read(cx).buffer().is_modified())
        {
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
        let view = self.active_view().clone();
        view.update(cx, |view, cx| view.restore_input(window, cx));
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
                || self.active_view().clone(),
                |minibuffer| minibuffer.read(cx).editor().clone(),
            )
    }

    pub(crate) fn echo_message(&self, window: &Window, cx: &App) -> String {
        let active = self.focused_editor(window, cx);
        active
            .read(cx)
            .message()
            .unwrap_or_else(|| {
                if active == *self.active_view() {
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
}

impl Render for AppShell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let editor = self.active_view().clone();
        let buffer = editor.read(cx).buffer();
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
            .child(v_flex().flex_1().min_h_0().child(editor))
            .child(mode_line)
            .children(self.minibuffer.clone())
            .child(echo_area)
    }
}
