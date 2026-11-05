use std::collections::HashMap;
use std::path::PathBuf;

use gpui_kit::component::{ActiveTheme, v_flex};
use gpui_kit::*;

use crate::buffer::Buffer;
use crate::buffers::{BufferSet, BufferSlot};
use crate::command::{Command, Scope};
use crate::command_registry::{CommandId, CommandRegistry};
use crate::editing::KillRing;
use crate::exit::Closing;
use crate::frame;
use crate::keymap::Keymap;
use crate::minibuffer::Minibuffer;
use crate::session::{Answer, ReadKind, ReadSession};
use crate::view::{BufferView, KeyLayers, KeyRoute};

mod appearance;
mod buffer_commands;
mod candidates;
mod closing;
mod diff;
mod documents;
mod messages;
mod reads;
mod scripting;
#[cfg(test)]
mod scripting_tests;
mod windows;

impl Global for KillRing {}

pub fn init(cx: &mut App) {
    crate::appearance::load_bundled_fonts(cx.text_system())
        .expect("failed to load bundled Lilex fonts");
    gpui_kit::init(cx);
    crate::theme::ThemeSettings::default().install(cx);
}

struct MinibufferMount {
    view: Entity<Minibuffer>,
    completion: Option<candidates::CompletionWork>,
    candidate_snapshot: Option<crate::input::DisplayId>,
    pending_candidate_action: Option<crate::completion::CandidateAction>,
    _subscription: Subscription,
    _feedback_subscription: Subscription,
}

/// Window-level ownership, command routing and UI composition. Session targets
/// may name inactive buffers; focus always returns to the active body view.
pub struct AppShell {
    buffers: BufferSet,
    messages: crate::messages::Messages,
    echo: Option<(EntityId, SharedString)>,
    panes: crate::panes::Panes<u64>,
    body_panes: HashMap<u64, windows::BodyPane>,
    next_pane: u64,
    body_keymap: Keymap,
    mode_keymaps: HashMap<crate::mode::ModeId, Keymap>,
    commands: CommandRegistry,
    script_host: Option<crate::scripting::ScriptHost>,
    config_generation: u64,
    config_path: Option<PathBuf>,
    reload_task: Option<Task<()>>,
    script_task: Option<Task<()>>,
    git_task: Option<diff::GitWork>,
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
        let registry_buffer = buffer.new_view();
        let buffer_id = buffer.snapshot_id().buffer;
        let view = Self::create_view(buffer, window, cx);
        let subscription = cx.observe(&view, Self::view_changed);
        let feedback = cx.subscribe(&view, Self::feedback);
        let pane = windows::BodyPane::new(buffer_id, view.clone(), subscription, feedback);
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
        view.update(cx, |view, cx| view.restore_input(window, cx));
        let configuration = crate::scripting::Configuration::default();
        Self {
            buffers: BufferSet::new(BufferSlot::new(registry_buffer)),
            messages: crate::messages::Messages::default(),
            echo: None,
            panes: crate::panes::Panes::new(0),
            body_panes: HashMap::from([(0, pane)]),
            next_pane: 1,
            body_keymap: configuration.body_keymap,
            mode_keymaps: configuration.mode_keymaps,
            commands: configuration.commands,
            script_host: None,
            config_generation: 0,
            config_path: None,
            reload_task: None,
            script_task: None,
            git_task: None,
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
        view.update(cx, |view, cx| {
            view.install_input_lifecycle(window, cx);
            view.suspend_input(cx);
        });
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
        if let Some(mount) = &mut self.minibuffer {
            mount.pending_candidate_action = None;
        }
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
            (self.active_view().clone(), Some(&self.body_keymap), true)
        };
        let single_key = self
            .session
            .as_ref()
            .is_some_and(|session| session.kind().is_single_key());
        let layers = if single_key {
            KeyLayers::Exclusive(outer.unwrap())
        } else if self.session.is_none()
            && self
                .minibuffer
                .as_ref()
                .is_none_or(|mount| !mount.view.focus_handle(cx).is_focused(window))
        {
            KeyLayers::Body {
                local: &self.mode_keymaps[&view.read(cx).buffer().mode()],
                global: &self.body_keymap,
            }
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
                if let CommandId::Native(command) = command
                    && command.scope() == Scope::ReadSession
                {
                    if single_key && command == Command::MinibufferCompleteAndExit {
                        self.answer_query(Answer::key(stroke), window, cx);
                    } else {
                        self.execute_session(command, window, cx);
                    }
                } else {
                    self.run_command(command, &view, window, cx);
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
            Command::MagitDiffStaged
            | Command::MagitDiffUnstaged
            | Command::SectionToggle
            | Command::SectionForward
            | Command::SectionBackward
            | Command::SectionUp
            | Command::MagitDiffJump
            | Command::MagitRefresh
            | Command::MagitDiffVisit
            | Command::ScrollLeft
            | Command::ScrollRight => self.diff_command(command, origin, window, cx),
            Command::ViewEchoAreaMessages => self.view_messages(window, cx),
            Command::ReloadConfiguration => self.reload_configuration(origin, window, cx),
            Command::LoadTheme => {
                self.begin_read(ReadKind::Theme, origin, "", window, cx);
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
                    .other_buffer(self.active_buffer_id())
                    .and_then(|id| self.buffers.index_of(id))
                    .map(|index| self.buffers[index].buffer.display_name());
                self.begin_read(ReadKind::SwitchToBuffer { default }, origin, "", window, cx);
            }
            Command::KillBuffer => {
                let default = origin.read(cx).buffer().display_name();
                self.begin_read(ReadKind::KillBuffer { default }, origin, "", window, cx);
            }
            Command::SplitWindowBelow
            | Command::SplitWindowRight
            | Command::OtherWindow
            | Command::DeleteWindow
            | Command::DeleteOtherWindows => {
                self.window_command(command, window, cx);
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
        command: CommandId,
        target: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let command = match command {
            CommandId::Native(command) => command,
            CommandId::Script(command) => {
                self.execute_script(command, target, window, cx);
                return;
            }
        };
        if !command.available_in(target.read(cx).buffer().mode()) {
            self.show_message(
                target,
                format!("{} is unavailable in this mode", command.name()),
                cx,
            );
            return;
        }
        match command.scope() {
            Scope::Edit => target.update(cx, |view, cx| view.execute_command(command, window, cx)),
            Scope::Application => self.execute_application(command, target, window, cx),
            Scope::ReadSession => debug_assert!(false, "{} is not runnable here", command.name()),
        }
    }

    /// Async results remain visible without changing the selected view or read session.
    fn show_message(
        &mut self,
        target: &Entity<BufferView>,
        message: impl Into<String>,
        cx: &mut Context<Self>,
    ) {
        let view = self
            .minibuffer
            .as_ref()
            .map(|mount| mount.view.read(cx).editor().clone())
            .unwrap_or_else(|| {
                if target == self.active_view() {
                    target.clone()
                } else {
                    self.active_view().clone()
                }
            });
        view.update(cx, |view, cx| view.set_message(message, cx));
    }

    pub(crate) fn echo_message(&self, _window: &Window, _cx: &App) -> String {
        self.echo
            .as_ref()
            .map(|(_, text)| text.to_string())
            .unwrap_or_default()
    }
}

impl Render for AppShell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = self.render_panes(self.panes.root(), cx);
        let message = self.echo_message(window, cx);
        let echo_area = frame::echo_area(&message, window.viewport_size().height * 0.25, cx);
        let minibuffer = self
            .minibuffer
            .as_ref()
            .map(|minibuffer| minibuffer.view.clone());
        v_flex()
            .id("app")
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(body)
            .overflow_hidden()
            .child(match minibuffer {
                Some(minibuffer) => minibuffer.into_any_element(),
                None => echo_area.into_any_element(),
            })
    }
}
