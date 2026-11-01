use gpui_kit::component::{ActiveTheme, v_flex};
use gpui_kit::*;

use crate::buffer::Buffer;
use crate::command::{Command, Scope};
use crate::echo::EchoArea;
use crate::editing::{KillRing, RouteError};
use crate::frame;
use crate::keymap::Keymap;
use crate::minibuffer::Minibuffer;
use crate::session::{self, ReadKind, ReadSession};
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

/// A mounted minibuffer target. A `ReadSession` adds origin, kind and
/// confirmation semantics on top of the same mount.
struct MinibufferMount {
    view: Entity<Minibuffer>,
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
    minibuffer: Option<MinibufferMount>,
    session: Option<ReadSession>,
    next_session_id: u64,
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
            session: None,
            next_session_id: 0,
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
        let in_minibuffer = self
            .minibuffer
            .as_ref()
            .is_some_and(|minibuffer| minibuffer.view.read(cx).editor() == &view);
        let outer = if in_minibuffer {
            // A bare mount has no read semantics: only the edit layer applies.
            self.session.as_ref().map(|_| &self.session_keymap)
        } else {
            Some(&self.application_keymap)
        };
        let route = view.update(cx, |view, cx| {
            view.route_key(stroke, prefer_character_input, outer, window, cx)
        });
        match route {
            KeyRoute::Unhandled => false,
            KeyRoute::Handled => true,
            KeyRoute::Requested(command) => {
                match command.scope() {
                    Scope::Application => self.execute_application(command, &view, window, cx),
                    Scope::ReadSession => self.execute_session(command, window, cx),
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
            // M2-C..E connect the remaining implementations; until then the
            // requests are reported on their origin instead of doing nothing.
            _ => origin.update(cx, |view, cx| {
                view.set_message(RouteError::Unavailable(command).to_string(), cx);
            }),
        }
    }

    fn execute_session(&mut self, command: Command, window: &mut Window, cx: &mut Context<Self>) {
        match command {
            Command::MinibufferComplete => self.complete_session(window, cx),
            Command::MinibufferCompleteAndExit => self.confirm_session(window, cx),
            Command::AbortMinibuffers => self.cancel_session(window, cx),
            _ => debug_assert!(false, "{} is not a session command", command.name()),
        }
    }

    fn begin_read(
        &mut self,
        kind: ReadKind,
        origin: &Entity<BufferView>,
        initial: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<u64> {
        if self.session.is_some() {
            return None;
        }
        self.mount_minibuffer(kind.prompt(), initial, window, cx)?;
        let id = self
            .next_session_id
            .checked_add(1)
            .expect("session id exhausted");
        self.next_session_id = id;
        self.session = Some(ReadSession::new(id, kind, origin.clone()));
        Some(id)
    }

    /// Ends a session and hands the origin back its own input session and focus.
    /// A stale id leaves the current session untouched.
    fn end_session(
        &mut self,
        id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<BufferView>> {
        let session = self.session.take()?;
        if session.id() != id {
            self.session = Some(session);
            return None;
        }
        self.remove_minibuffer(cx);
        let origin = session.origin().clone();
        origin.update(cx, |view, cx| view.restore_input(window, cx));
        cx.notify();
        Some(origin)
    }

    fn session_input(&self, cx: &App) -> Option<String> {
        Some(self.minibuffer.as_ref()?.view.read(cx).contents(cx))
    }

    fn message_minibuffer(&mut self, message: impl Into<String>, cx: &mut Context<Self>) {
        if let Some(minibuffer) = &self.minibuffer {
            let view = minibuffer.view.read(cx).editor().clone();
            view.update(cx, |view, cx| view.set_message(message, cx));
        }
    }

    fn record_completion(&mut self, id: u64, completion: &session::Completion) {
        if let Some(session) = &mut self.session
            && session.id() == id
        {
            session.record_completion(completion);
        }
    }

    fn replace_session_input(
        &mut self,
        id: u64,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self
            .session
            .as_ref()
            .is_some_and(|session| session.id() == id)
        {
            return;
        }
        let Some(minibuffer) = &self.minibuffer else {
            return;
        };
        let editor = minibuffer.view.read(cx).editor().clone();
        editor.update(cx, |view, cx| view.replace_input(text, window, cx));
    }

    fn complete_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = &self.session else {
            return;
        };
        let (id, kind) = (session.id(), session.kind());
        let input = self.session_input(cx).unwrap_or_default();
        let completion = session::complete(kind, &input);
        if let Some(replacement) = &completion.replacement {
            self.replace_session_input(id, replacement, window, cx);
        }
        let message = completion.message();
        self.record_completion(id, &completion);
        self.message_minibuffer(message, cx);
    }

    fn confirm_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = &self.session else {
            return;
        };
        let (id, kind) = (session.id(), session.kind());
        match kind {
            ReadKind::Command => self.confirm_command(id, window, cx),
        }
    }

    fn confirm_command(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let input = self.session_input(cx).unwrap_or_default();
        // Emacs lets an explicit empty answer leave the minibuffer and reports
        // the invalid name afterwards.
        if input.is_empty() {
            if let Some(origin) = self.end_session(id, window, cx) {
                origin.update(cx, |view, cx| {
                    view.set_message(session::invalid_command_message(""), cx)
                });
            }
            return;
        }
        if let Some(command) = session::session_command(&input) {
            if let Some(origin) = self.end_session(id, window, cx) {
                self.run_command(command, &origin, window, cx);
            }
            return;
        }
        let Some(kind) = self.session.as_ref().map(ReadSession::kind) else {
            return;
        };
        let completion = session::complete(kind, &input);
        self.record_completion(id, &completion);
        if let [candidate] = completion.candidates.as_slice() {
            let command = session::session_command(candidate)
                .expect("command candidates resolve to commands");
            if let Some(origin) = self.end_session(id, window, cx) {
                self.run_command(command, &origin, window, cx);
            }
            return;
        }
        if let Some(replacement) = &completion.replacement {
            // Completion made progress but is not unique: Emacs completes the
            // input and waits for another RET.
            self.replace_session_input(id, replacement, window, cx);
            return;
        }
        if completion.candidates.is_empty() {
            self.message_minibuffer("No match", cx);
        }
    }

    fn cancel_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.session.as_ref().map(ReadSession::id) else {
            return;
        };
        if let Some(origin) = self.end_session(id, window, cx) {
            origin.update(cx, |view, cx| view.set_message("Quit", cx));
        }
    }

    fn run_command(
        &mut self,
        command: Command,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match command.scope() {
            Scope::Edit => origin.update(cx, |view, cx| view.execute_command(command, window, cx)),
            Scope::Application => self.execute_application(command, origin, window, cx),
            Scope::ReadSession => debug_assert!(false, "{} is not runnable here", command.name()),
        }
    }

    fn request_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.close_pending {
            return false;
        }
        let restore = self.focused_editor(window, cx);
        let mut targets = vec![self.active_view().clone()];
        if let Some(minibuffer) = &self.minibuffer {
            targets.push(minibuffer.view.read(cx).editor().clone());
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

    /// Mounts a bare minibuffer target without read semantics. Read sessions add
    /// their origin and confirmation rules on top of it.
    pub fn mount_minibuffer(
        &mut self,
        prompt: &str,
        initial: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<Minibuffer>> {
        if self.close_pending || self.session.is_some() {
            return None;
        }
        self.focused_editor(window, cx)
            .update(cx, |view, cx| view.end_input(cx));
        self.remove_minibuffer(cx);
        let minibuffer = cx.new(|cx| Minibuffer::new(prompt, initial, window, cx));
        let subscription = cx.observe(&minibuffer, |_, _, cx| cx.notify());
        window.focus(&minibuffer.focus_handle(cx), cx);
        self.minibuffer = Some(MinibufferMount {
            view: minibuffer.clone(),
            _subscription: subscription,
        });
        cx.notify();
        Some(minibuffer)
    }

    /// Unmounts a bare minibuffer and restores focus to the active buffer. Read
    /// sessions must be ended through completion or cancel so their origin is
    /// restored.
    pub fn unmount_minibuffer(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        debug_assert!(
            self.session.is_none(),
            "finish read sessions through their own commands"
        );
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
            let view = minibuffer.view.read(cx).editor().clone();
            view.update(cx, |view, cx| view.suspend_input(cx));
        }
    }

    fn focused_editor(&self, window: &Window, cx: &App) -> Entity<BufferView> {
        self.minibuffer
            .as_ref()
            .filter(|minibuffer| minibuffer.view.focus_handle(cx).is_focused(window))
            .map_or_else(
                || self.active_view().clone(),
                |minibuffer| minibuffer.view.read(cx).editor().clone(),
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
        self.minibuffer.as_ref().map(|minibuffer| &minibuffer.view)
    }

    pub fn session(&self) -> Option<&ReadSession> {
        self.session.as_ref()
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
        let minibuffer = self
            .minibuffer
            .as_ref()
            .map(|minibuffer| minibuffer.view.clone());
        v_flex()
            .id("app")
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(v_flex().flex_1().min_h_0().child(editor))
            .child(mode_line)
            .children(minibuffer)
            .child(echo_area)
    }
}
