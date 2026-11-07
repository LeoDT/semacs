use super::{AppShell, commits::Session};
use crate::exit::Closing;
use crate::session::ReadKind;
use gpui_kit::{Context, Window};

impl Session {
    pub(super) fn terminate_for_exit(&mut self) -> Result<(), String> {
        let result = self
            .process
            .as_ref()
            .map(|process| process.hangup())
            .transpose();
        match result {
            Ok(_) => {}
            Err(error)
                if error.kind() == std::io::ErrorKind::NotConnected
                    || error.raw_os_error() == Some(libc::ESRCH) => {}
            Err(error) => {
                self.exit_termination_requested = false;
                let message = format!("Cannot terminate Git #{}: {error}", self.record.operation);
                self.record.diagnostics.push(message.clone());
                return Err(message);
            }
        }
        self.exit_termination_requested = true;
        Ok(())
    }
}

impl AppShell {
    pub(super) fn prepare_commit_exit(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.active_commits() {
            return true;
        }
        let confirmed = self
            .closing
            .as_mut()
            .and_then(Closing::exit_mut)
            .is_some_and(|flow| flow.commits_confirmed);
        if !confirmed {
            let origin = self.active_view().clone();
            if self
                .begin_read(ReadKind::ExitActiveCommits, &origin, "", window, cx)
                .is_none()
            {
                self.cancel_closing();
                self.show_message(
                    &origin,
                    "Cannot confirm active processes; exit canceled",
                    cx,
                );
            }
        } else {
            self.terminate_commits_for_exit(cx);
        }
        false
    }

    fn terminate_commits_for_exit(&mut self, cx: &mut Context<Self>) {
        let mut errors = Vec::new();
        for session in &mut self.commits {
            if !session.record.phase.is_active() || session.exit_termination_requested {
                continue;
            }
            if let Err(message) = session.terminate_for_exit() {
                errors.push((session.record.repository.clone(), message));
            }
        }
        if errors.is_empty() {
            self.show_message(
                &self.active_view().clone(),
                "Waiting for Git processes to terminate before exit",
                cx,
            );
        } else {
            self.cancel_closing();
            for (repository, message) in errors {
                self.refresh_commit_process(&repository, cx);
                self.show_message(&self.active_view().clone(), message, cx);
            }
        }
    }

    pub(super) fn resume_exit_after_commit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.exiting() {
            return;
        }
        if !self.active_commits()
            && let Some(id) = self
                .session
                .as_ref()
                .filter(|session| *session.kind() == ReadKind::ExitActiveCommits)
                .map(|session| session.id())
        {
            self.end_session(id, window, cx);
        }
        self.process_next_exit(window, cx);
    }
}
