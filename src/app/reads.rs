use super::{AppShell, MinibufferMount};
use crate::command::Command;
use crate::completion::CandidateAction;
use crate::files;
use crate::minibuffer::Minibuffer;
use crate::session::{Answer, ReadKind, ReadSession};
use crate::view::BufferView;
use gpui_kit::{App, AppContext, Context, Entity, Focusable, Window};
use std::path::Path;

impl AppShell {
    pub(super) fn begin_read(
        &mut self,
        kind: ReadKind,
        target: &Entity<BufferView>,
        initial: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<u64> {
        if self.session.is_some() {
            return None;
        }
        self.cancel_file_open();
        let prompt = kind.prompt();
        self.mount_minibuffer(&prompt, initial, window, cx)?;
        for view in self.all_views() {
            view.update(cx, |view, cx| view.suspend_input(cx));
        }
        let id = self
            .next_session_id
            .checked_add(1)
            .expect("session id exhausted");
        self.next_session_id = id;
        if kind.is_single_key() {
            let editor = self
                .minibuffer
                .as_ref()
                .unwrap()
                .view
                .read(cx)
                .editor()
                .clone();
            editor.update(cx, |view, cx| view.set_read_only(true, cx));
        }
        self.session = Some(ReadSession::new(id, kind, target.clone()));
        self.refresh_candidates(window, cx);
        Some(id)
    }

    /// Mounts a bare minibuffer target without read semantics. Read sessions add
    /// their target and confirmation rules on top of it.
    pub fn mount_minibuffer(
        &mut self,
        prompt: &str,
        initial: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<Minibuffer>> {
        if self.session.is_some() {
            return None;
        }
        self.focused_editor(window, cx)
            .update(cx, |view, cx| view.end_input(cx));
        self.remove_minibuffer(cx);
        let minibuffer = cx.new(|cx| Minibuffer::new(prompt, initial, window, cx));
        let editor = minibuffer.read(cx).editor().clone();
        let feedback = cx.subscribe(&editor, Self::feedback);
        self.echo = None;
        let subscription = cx.observe_in(&minibuffer, window, |shell, _, window, cx| {
            shell.refresh_candidates(window, cx);
            cx.notify();
        });
        window.focus(&minibuffer.focus_handle(cx), cx);
        self.minibuffer = Some(MinibufferMount {
            view: minibuffer.clone(),
            completion: None,
            candidate_snapshot: None,
            pending_candidate_action: None,
            _subscription: subscription,
            _feedback_subscription: feedback,
        });
        cx.notify();
        Some(minibuffer)
    }

    fn confirm_command(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let input = self.session_input(cx).unwrap_or_default();
        if let Some(command) = self.commands.named(&input) {
            if let Some(target) = self.end_session(id, window, cx) {
                self.run_command(command, &target, window, cx);
            }
        } else {
            self.message_minibuffer("No match", cx);
        }
    }

    fn confirm_find_file(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let input = self.session_input(cx).unwrap_or_default();
        if input.is_empty() {
            self.message_minibuffer("No file name", cx);
            return;
        }
        let origin = self.session.as_ref().unwrap().origin();
        let path = files::expand(Path::new(&input), &self.default_directory_for(origin, cx));
        let path = match path {
            Ok(path) => path,
            Err(error) => {
                self.message_minibuffer(format!("{}: {error}", input), cx);
                return;
            }
        };
        if let Some(target) = self.end_session(id, window, cx) {
            self.visit_file(path, &target, window, cx);
        }
    }

    pub(super) fn execute_session(
        &mut self,
        command: Command,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match command {
            Command::MinibufferComplete
                if self
                    .session
                    .as_ref()
                    .is_some_and(|session| session.kind().has_candidates()) =>
            {
                self.candidate_action(CandidateAction::Insert, window, cx)
            }
            Command::MinibufferCompleteAndExit
                if self
                    .session
                    .as_ref()
                    .is_some_and(|session| session.kind().has_candidates()) =>
            {
                self.candidate_action(CandidateAction::Accept, window, cx)
            }
            Command::CandidateExitInput => self.candidate_action(CandidateAction::Raw, window, cx),
            Command::CandidateNext | Command::CandidatePrevious => {
                self.navigate_candidates(command == Command::CandidateNext, window, cx)
            }
            Command::MinibufferComplete => {}
            Command::MinibufferCompleteAndExit => self.confirm_session(window, cx),
            Command::AbortMinibuffers => self.cancel_session(window, cx),
            _ => debug_assert!(false, "{} is not a session command", command.name()),
        }
    }

    /// Ends a session and restores input and focus to the active buffer. The
    /// returned entity is the session target, which may not have had focus (for
    /// example a non-active dirty buffer during exit).
    pub(super) fn end_session(
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
        self.update_selected_views(cx);
        let target = session.origin().clone();
        let active = self.active_view().clone();
        active.update(cx, |view, cx| view.restore_input(window, cx));
        cx.notify();
        Some(target)
    }

    pub(super) fn session_input(&self, cx: &App) -> Option<String> {
        Some(self.minibuffer.as_ref()?.view.read(cx).contents(cx))
    }

    pub(super) fn message_minibuffer(
        &mut self,
        message: impl Into<String>,
        cx: &mut Context<Self>,
    ) {
        if let Some(minibuffer) = &self.minibuffer {
            let view = minibuffer.view.read(cx).editor().clone();
            view.update(cx, |view, cx| view.set_hint(message, cx));
        }
    }

    pub(super) fn replace_session_input(
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

    fn confirm_switch_to_buffer(
        &mut self,
        id: u64,
        default: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = self.session_input(cx).unwrap_or_default();
        let name = if input.is_empty() {
            default
        } else {
            Some(input)
        };
        let Some(name) = name else {
            self.message_minibuffer("No buffer name", cx);
            return;
        };
        let index = self
            .buffers
            .iter()
            .position(|slot| slot.buffer.display_name() == name);
        let Some(index) = index else {
            self.message_minibuffer(format!("No buffer named {name}"), cx);
            return;
        };
        if self.end_session(id, window, cx).is_some() {
            self.activate(index, window, cx);
        }
    }

    /// Reads the name of the buffer to kill. The default is the current buffer.
    fn confirm_kill_buffer_name(
        &mut self,
        id: u64,
        default: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = self.session_input(cx).unwrap_or_default();
        let name = if input.is_empty() { default } else { input };
        let index = self
            .buffers
            .iter()
            .position(|slot| slot.buffer.display_name() == name);
        let Some(index) = index else {
            self.message_minibuffer(format!("No buffer named {name}"), cx);
            return;
        };
        if self.end_session(id, window, cx).is_some() {
            self.kill_buffer_at(index, window, cx);
        }
    }

    /// Unmounts a bare minibuffer and restores focus to the active buffer. Read
    /// sessions must be ended through completion or cancel so their target is
    /// released.
    pub fn unmount_minibuffer(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        debug_assert!(
            self.session.is_none(),
            "finish read sessions through their own commands"
        );
        if self.minibuffer.is_none() {
            return false;
        }
        self.remove_minibuffer(cx);
        self.update_selected_views(cx);
        let view = self.active_view().clone();
        view.update(cx, |view, cx| view.restore_input(window, cx));
        cx.notify();
        true
    }

    fn remove_minibuffer(&mut self, cx: &mut Context<Self>) {
        self.echo = None;
        if let Some(minibuffer) = self.minibuffer.take() {
            let view = minibuffer.view.read(cx).editor().clone();
            view.update(cx, |view, cx| view.suspend_input(cx));
        }
    }

    pub(super) fn focused_editor(&self, window: &Window, cx: &App) -> Entity<BufferView> {
        self.minibuffer
            .as_ref()
            .filter(|minibuffer| minibuffer.view.focus_handle(cx).is_focused(window))
            .map_or_else(
                || self.active_view().clone(),
                |minibuffer| minibuffer.view.read(cx).editor().clone(),
            )
    }

    pub fn minibuffer(&self) -> Option<&Entity<Minibuffer>> {
        self.minibuffer.as_ref().map(|minibuffer| &minibuffer.view)
    }

    pub fn session(&self) -> Option<&ReadSession> {
        self.session.as_ref()
    }

    pub(super) fn confirm_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = &self.session else { return };
        let (id, kind) = (session.id(), session.kind().clone());
        match kind {
            ReadKind::Command => self.confirm_command(id, window, cx),
            ReadKind::Theme => {
                let input = self.session_input(cx).unwrap_or_default();
                self.confirm_theme(id, &input, window, cx);
            }
            ReadKind::FontFamily => {
                let input = self.session_input(cx).unwrap_or_default();
                self.confirm_font_family(id, &input, window, cx);
            }
            ReadKind::FindFile => self.confirm_find_file(id, window, cx),
            ReadKind::WriteFile => self.confirm_write_file(id, window, cx),
            ReadKind::SwitchToBuffer { default } => {
                self.confirm_switch_to_buffer(id, default, window, cx)
            }
            ReadKind::KillBuffer { default } => {
                self.confirm_kill_buffer_name(id, default, window, cx)
            }
            _ => {
                let input = self.session_input(cx).unwrap_or_default();
                self.answer_query(Answer::full(&input), window, cx);
            }
        }
    }

    fn confirm_write_file(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let input = self.session_input(cx).unwrap_or_default();
        if input.is_empty() {
            self.message_minibuffer("No file name", cx);
            return;
        }
        let origin = self.session.as_ref().unwrap().origin();
        match files::expand(Path::new(&input), &self.default_directory_for(origin, cx)) {
            Ok(path) => {
                if let Some(target) = self.end_session(id, window, cx) {
                    self.write_file(&target, path, window, cx);
                }
            }
            Err(error) => self.message_minibuffer(format!("{input}: {error}"), cx),
        }
    }

    pub(super) fn answer_query(
        &mut self,
        answer: Answer,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = &self.session else { return };
        let (id, kind) = (session.id(), session.kind().clone());
        let valid = match answer {
            Answer::Yes | Answer::No => true,
            Answer::Save => matches!(kind, ReadKind::KillBufferConfirm { .. }),
            Answer::Invalid => false,
        };
        if !valid {
            self.message_minibuffer(kind.answer_hint(), cx);
            return;
        }
        if answer == Answer::No {
            self.cancel_save_confirmation(cx);
        }
        let Some(target) = self.end_session(id, window, cx) else {
            return;
        };
        match kind {
            ReadKind::Overwrite(_) | ReadKind::ExternalChange(_) => {
                if answer == Answer::Yes {
                    self.resume_save(&target, window, cx);
                } else {
                    self.decline_save(&target, window, cx);
                }
            }
            ReadKind::KillBufferConfirm { .. } => match answer {
                Answer::Yes => {
                    if let Some(index) = self.buffer_index(&target, cx) {
                        self.remove_buffer(index, window, cx);
                    }
                }
                Answer::Save => self.save_then_kill(&target, window, cx),
                _ => self.show_message(&target, "Canceled", cx),
            },
            ReadKind::ExitSave { .. } => {
                if answer == Answer::No {
                    self.process_next_exit(window, cx);
                } else {
                    self.wait_for_save(target.read(cx).buffer().snapshot_id().buffer);
                    if !self.save_buffer(&target, window, cx) {
                        if !target.read(cx).buffer().is_modified() {
                            self.process_next_exit(window, cx);
                        } else {
                            self.cancel_closing();
                            self.show_message(&target, "Save could not start; exit canceled", cx);
                        }
                    }
                }
            }
            ReadKind::ExitUncertainGit => {
                if answer == Answer::Yes {
                    if let Some(flow) = self
                        .closing
                        .as_mut()
                        .and_then(crate::exit::Closing::exit_mut)
                    {
                        flow.uncertain_git_confirmed = true;
                    }
                    self.process_next_exit(window, cx);
                } else {
                    self.cancel_closing();
                    self.show_message(&target, "Canceled", cx);
                }
            }
            ReadKind::ExitModified => {
                if answer == Answer::Yes {
                    self.finish_exit(window, cx);
                } else {
                    self.cancel_closing();
                    self.show_message(&target, "Canceled", cx);
                }
            }
            _ => debug_assert!(false, "not a query"),
        }
    }

    fn cancel_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.session.as_ref().map(ReadSession::id) else {
            return;
        };
        self.cancel_save_confirmation(cx);
        if let Some(target) = self.end_session(id, window, cx) {
            self.cancel_closing();
            self.show_message(&target, "Quit", cx);
        }
    }
}
