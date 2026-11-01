use std::path::PathBuf;

use gpui_kit::component::{ActiveTheme, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::buffer::Buffer;
use crate::buffers::{BufferSet, BufferSlot};
use crate::command::{Command, Scope};
use crate::editing::KillRing;
use crate::exit::Closing;
use crate::frame;
use crate::keymap::Keymap;
use crate::minibuffer::Minibuffer;
use crate::session::{Answer, ReadKind, ReadSession};
use crate::view::{BufferView, KeyLayers, KeyRoute};

mod appearance;
mod buffer_commands;
mod closing;
mod documents;
mod reads;

impl Global for KillRing {}

pub fn init(cx: &mut App) {
    gpui_kit::init(cx);
}

struct MinibufferMount {
    view: Entity<Minibuffer>,
    completion: Option<Task<()>>,
    _subscription: Subscription,
}

/// Window-level ownership, command routing and UI composition. Session targets
/// may name inactive buffers; focus always returns to the active body view.
pub struct AppShell {
    buffers: BufferSet,
    application_keymap: Keymap,
    startup_directory: PathBuf,
    minibuffer: Option<MinibufferMount>,
    session: Option<ReadSession>,
    next_session_id: u64,
    closing: Option<Closing>,
    file_task: Option<Task<()>>,
    opening: Option<documents::Opening>,
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
        Self {
            buffers: BufferSet::new(BufferSlot::new(view, subscription)),
            application_keymap: Keymap::for_scope(Scope::Application),
            startup_directory: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            minibuffer: None,
            session: None,
            next_session_id: 0,
            closing: None,
            file_task: None,
            opening: None,
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
        // A read session owns the keyboard even if focus drifted back to a
        // body view. Re-focusing here keeps platform text input on the
        // minibuffer and makes RET reach the session layer.
        let (view, outer, require_focus) = if let Some(session) = &self.session {
            let Some(minibuffer) = &self.minibuffer else {
                return false;
            };
            let editor = minibuffer.view.read(cx).editor().clone();
            if !minibuffer.view.focus_handle(cx).is_focused(window) {
                editor.update(cx, |view, cx| view.restore_input(window, cx));
            }
            (editor, Some(session.keymap()), false)
        } else if let Some(minibuffer) = self
            .minibuffer
            .as_ref()
            .filter(|minibuffer| minibuffer.view.focus_handle(cx).is_focused(window))
        {
            (minibuffer.view.read(cx).editor().clone(), None, true)
        } else {
            (
                self.active_view().clone(),
                Some(&self.application_keymap),
                true,
            )
        };
        let single_key = self
            .session
            .as_ref()
            .is_some_and(|session| session.kind().is_single_key());
        let layers = if single_key {
            KeyLayers::Exclusive(outer.unwrap())
        } else {
            KeyLayers::Edit(outer)
        };
        let route = view.update(cx, |view, cx| {
            view.route_key(
                stroke,
                prefer_character_input,
                layers,
                require_focus,
                window,
                cx,
            )
        });
        if single_key && !matches!(route, KeyRoute::Requested(_)) {
            let hint = self.session.as_ref().unwrap().kind().answer_hint();
            self.message_minibuffer(hint, cx);
            return true;
        }
        match route {
            KeyRoute::Unhandled => false,
            KeyRoute::Handled => true,
            KeyRoute::Requested(command) => {
                match command.scope() {
                    Scope::Application => self.execute_application(command, &view, window, cx),
                    Scope::ReadSession => {
                        if single_key && command == Command::MinibufferCompleteAndExit {
                            self.answer_query(Answer::key(stroke), window, cx);
                        } else {
                            self.execute_session(command, window, cx);
                        }
                    }
                    Scope::Edit => {
                        debug_assert!(false, "{} is not a layered command", command.name())
                    }
                }
                true
            }
        }
    }

    fn execute_application(
        &mut self,
        command: Command,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match command {
            Command::ExecuteExtendedCommand => {
                self.begin_read(ReadKind::Command, origin, "", window, cx);
            }
            Command::SetFontFamily => {
                self.begin_read(ReadKind::FontFamily, origin, "", window, cx);
            }
            Command::IncreaseFontSize | Command::DecreaseFontSize | Command::ResetFontSize => {
                self.change_font_size(command, origin, cx);
            }
            Command::FindFile => {
                self.begin_read(ReadKind::FindFile, origin, "", window, cx);
            }
            Command::SaveBuffer => {
                self.save_buffer(origin, window, cx);
            }
            Command::WriteFile => {
                self.begin_read(ReadKind::WriteFile, origin, "", window, cx);
            }
            Command::SwitchToBuffer => {
                let default = self
                    .buffers
                    .other_view()
                    .map(|view| view.read(cx).buffer().display_name());
                self.begin_read(ReadKind::SwitchToBuffer { default }, origin, "", window, cx);
            }
            Command::KillBuffer => {
                let default = origin.read(cx).buffer().display_name();
                self.begin_read(ReadKind::KillBuffer { default }, origin, "", window, cx);
            }
            Command::SaveBuffersKillTerminal => {
                if self.begin_exit(window, cx) {
                    window.remove_window();
                }
            }
            _ => debug_assert!(false, "{} is not an application command", command.name()),
        }
    }

    fn run_command(
        &mut self,
        command: Command,
        target: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match command.scope() {
            Scope::Edit => target.update(cx, |view, cx| view.execute_command(command, window, cx)),
            Scope::Application => self.execute_application(command, target, window, cx),
            Scope::ReadSession => debug_assert!(false, "{} is not runnable here", command.name()),
        }
    }

    /// Shows a message in the echo area. Messages from non-active targets are
    /// written to the active view so they stay visible.
    fn show_message(
        &mut self,
        target: &Entity<BufferView>,
        message: impl Into<String>,
        cx: &mut Context<Self>,
    ) {
        let view = if self.buffers.index_of(target) == Some(self.buffers.active_index()) {
            target.clone()
        } else {
            self.active_view().clone()
        };
        view.update(cx, |view, cx| view.set_message(message, cx));
    }

    pub(crate) fn echo_message(&self, window: &Window, cx: &App) -> String {
        self.focused_editor(window, cx)
            .read(cx)
            .message()
            .unwrap_or_default()
            .to_owned()
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
        let message = self.echo_message(window, cx);
        let echo_area = frame::echo_area(&message, cx);
        let minibuffer = self
            .minibuffer
            .as_ref()
            .map(|minibuffer| minibuffer.view.clone());
        let shell = cx.weak_entity();
        v_flex()
            .id("app")
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .child(editor)
                    .when(self.opening.is_some(), |body| {
                        body.on_children_prepainted(move |bounds, window, cx| {
                            if let Some(bounds) = bounds.first() {
                                shell
                                    .update(cx, |shell, cx| {
                                        shell.prepare_opening(*bounds, window, cx)
                                    })
                                    .ok();
                            }
                        })
                    }),
            )
            .child(mode_line)
            .children(minibuffer)
            .child(echo_area)
    }
}
