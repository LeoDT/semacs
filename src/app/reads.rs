use super::{AppShell, MinibufferMount};
use crate::command::Command;
use crate::files;
use crate::minibuffer::Minibuffer;
use crate::session::{self, Answer, ReadKind, ReadSession};
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
        let subscription = cx.observe(&minibuffer, |_, _, cx| cx.notify());
        window.focus(&minibuffer.focus_handle(cx), cx);
        self.minibuffer = Some(MinibufferMount {
            view: minibuffer.clone(),
            completion: None,
            _subscription: subscription,
        });
        cx.notify();
        Some(minibuffer)
    }

    fn confirm_command(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let input = self.session_input(cx).unwrap_or_default();
        // Emacs lets an explicit empty answer leave the minibuffer and reports
        // the invalid name afterwards.
        if input.is_empty() {
            if let Some(target) = self.end_session(id, window, cx) {
                self.show_message(&target, session::invalid_command_message(""), cx);
            }
            return;
        }
        if let Some(command) = session::session_command(&input) {
            if let Some(target) = self.end_session(id, window, cx) {
                self.run_command(command, &target, window, cx);
            }
            return;
        }
        let completion = session::complete(session::command_names(), &input);
        if let [candidate] = completion.candidates.as_slice() {
            let command = session::session_command(candidate)
                .expect("command candidates resolve to commands");
            if let Some(target) = self.end_session(id, window, cx) {
                self.run_command(command, &target, window, cx);
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
            Command::MinibufferComplete => self.complete_session(window, cx),
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
        let target = session.origin().clone();
        let active = self.active_view().clone();
        active.update(cx, |view, cx| view.restore_input(window, cx));
        cx.notify();
        Some(target)
    }

    fn session_input(&self, cx: &App) -> Option<String> {
        Some(self.minibuffer.as_ref()?.view.read(cx).contents(cx))
    }

    pub(super) fn message_minibuffer(
        &mut self,
        message: impl Into<String>,
        cx: &mut Context<Self>,
    ) {
        if let Some(minibuffer) = &self.minibuffer {
            let view = minibuffer.view.read(cx).editor().clone();
            view.update(cx, |view, cx| view.set_message(message, cx));
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
        if self.end_session(id, window, cx).is_some() {
            self.switch_to_buffer(&name, window, cx);
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
            .position(|slot| slot.view.read(cx).buffer().display_name() == name);
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

    fn complete_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = &self.session else { return };
        let (id, kind, origin) = (
            session.id(),
            session.kind().clone(),
            session.origin().clone(),
        );
        let input = self.session_input(cx).unwrap_or_default();
        if matches!(kind, ReadKind::FindFile | ReadKind::WriteFile) {
            let directory = self.default_directory_for(&origin, cx);
            let editor = self
                .minibuffer
                .as_ref()
                .unwrap()
                .view
                .read(cx)
                .editor()
                .clone();
            let snapshot = editor.read(cx).display_snapshot().id();
            let task = cx.spawn_in(window, async move |shell, cx| {
                let completion = cx
                    .background_executor()
                    .spawn(async move {
                        session::complete(files::complete_path(&input, &directory), &input)
                    })
                    .await;
                shell
                    .update_in(cx, |shell, window, cx| {
                        if !shell.session.as_ref().is_some_and(|session| {
                            session.id() == id && session.origin() == &origin
                        }) || editor.read(cx).display_snapshot().id() != snapshot
                        {
                            return;
                        }
                        shell.apply_completion(id, completion, window, cx);
                    })
                    .ok();
            });
            self.minibuffer.as_mut().unwrap().completion = Some(task);
            return;
        }
        let candidates = match kind {
            ReadKind::Command => session::command_names(),
            ReadKind::SwitchToBuffer { .. } | ReadKind::KillBuffer { .. } => {
                let active = self.active_view();
                let mut names: Vec<_> = self
                    .buffers
                    .iter()
                    .filter(|slot| {
                        !matches!(kind, ReadKind::SwitchToBuffer { .. }) || &slot.view != active
                    })
                    .map(|slot| slot.view.read(cx).buffer().display_name())
                    .collect();
                names.sort_unstable();
                names
            }
            _ => return,
        };
        self.apply_completion(id, session::complete(candidates, &input), window, cx);
    }

    fn apply_completion(
        &mut self,
        id: u64,
        completion: session::Completion,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(replacement) = &completion.replacement {
            self.replace_session_input(id, replacement, window, cx);
        }
        self.message_minibuffer(completion.message(), cx);
    }

    fn confirm_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = &self.session else { return };
        let (id, kind) = (session.id(), session.kind().clone());
        match kind {
            ReadKind::Command => self.confirm_command(id, window, cx),
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
            self.cancel_save_confirmation();
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
                    if let Some(index) = self.buffers.index_of(&target) {
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
                    self.wait_for_save(&target);
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
        self.cancel_save_confirmation();
        if let Some(target) = self.end_session(id, window, cx) {
            self.cancel_closing();
            self.show_message(&target, "Quit", cx);
        }
    }
}
