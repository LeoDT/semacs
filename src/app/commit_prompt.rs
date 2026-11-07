use super::{AppShell, git::operation_id};
use crate::commit::prompt::{Prompt, Scanner};
use crate::session::ReadKind;
use crate::view::BufferView;
use gpui_kit::{Context, Entity, Task, Window};

#[derive(Default)]
pub(super) struct Interaction {
    scanner: Scanner,
    pending: Option<Pending>,
    writing: Option<Task<()>>,
}

struct Pending {
    request: u64,
    prompt: Prompt,
    resume_target: bool,
}

impl Interaction {
    pub(super) fn scan(&mut self, output: &[u8]) {
        if let Some(prompt) = self.scanner.scan(output)
            && self.pending.is_none()
        {
            self.pending = Some(Pending {
                request: operation_id(),
                prompt,
                resume_target: false,
            });
        }
    }

    pub(super) fn clear(&mut self) {
        self.pending = None;
        self.writing = None;
    }
}

impl AppShell {
    pub(super) fn present_commit_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.minibuffer.is_some() || self.exiting() {
            return;
        }
        let candidate = self.commits.iter().find_map(|session| {
            if !session.record.phase.is_active() || session.interaction.writing.is_some() {
                return None;
            }
            let pending = session.interaction.pending.as_ref()?;
            let operation = session.record.operation;
            let request = pending.request;
            let kind = match &pending.prompt {
                Prompt::YesNo { question, .. } => ReadKind::CommitYesNo {
                    operation,
                    request,
                    prompt: format!("{question} (yes or no) "),
                },
                Prompt::Username(prompt) => ReadKind::CommitUsername {
                    operation,
                    request,
                    prompt: prompt.clone(),
                },
            };
            Some((kind, operation, session.target.is_current(self, cx)))
        });
        if let Some((kind, operation, resume_target)) = candidate {
            let origin = self.active_view().clone();
            if self.begin_read(kind, &origin, "", window, cx).is_some() {
                self.commits
                    .iter_mut()
                    .find(|session| session.record.operation == operation)
                    .unwrap()
                    .interaction
                    .pending
                    .as_mut()
                    .unwrap()
                    .resume_target = resume_target;
            }
        }
    }

    pub(super) fn answer_commit_prompt(
        &mut self,
        operation: u64,
        request: u64,
        answer: String,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let resumed_target = super::commit_editor::EditorTarget::capture(self, cx);
        let Some(session) = self.commits.iter_mut().find(|session| {
            session.record.operation == operation && session.record.phase.is_active()
        }) else {
            self.show_message(origin, "Process no longer running", cx);
            return;
        };
        if !session
            .interaction
            .pending
            .as_ref()
            .is_some_and(|pending| pending.request == request)
        {
            self.show_message(origin, "Process prompt no longer pending", cx);
            return;
        }
        let pending = session.interaction.pending.take().unwrap();
        if pending.resume_target {
            session.target.resume_after_read(resumed_target);
        }
        let answer = match pending.prompt {
            Prompt::YesNo { yes, no, .. } => {
                if answer == "yes" {
                    Some(yes)
                } else {
                    Some(no)
                }
            }
            Prompt::Username(_) if answer.is_empty() => None,
            Prompt::Username(_) => Some(answer),
        };
        session.interaction.scanner.answered(&session.record.output);
        let input = session
            .process
            .as_ref()
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::NotConnected, "Git is still starting")
            })
            .and_then(|process| process.input());
        let origin = origin.clone();
        session.interaction.writing = Some(cx.spawn_in(window, async move |shell, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let input = input?;
                    let answer = answer.unwrap_or_else(crate::commit::login_name);
                    input.write_all(format!("{answer}\n").as_bytes())
                })
                .await;
            shell
                .update_in(cx, |shell, window, cx| {
                    let Some(session) = shell
                        .commits
                        .iter_mut()
                        .find(|session| session.record.operation == operation)
                    else {
                        return;
                    };
                    session.interaction.writing = None;
                    if let Err(error) = result {
                        let message = format!("Git prompt input failed: {error}");
                        session.record.diagnostics.push(message.clone());
                        let repository = session.record.repository.clone();
                        shell.refresh_commit_process(&repository, cx);
                        shell.show_message(&origin, message, cx);
                    }
                    shell.present_commit_prompt(window, cx);
                })
                .ok();
        }));
    }

    pub(super) fn release_commit_prompt(
        &mut self,
        operation: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(id) = self
            .session
            .as_ref()
            .and_then(|session| match session.kind() {
                ReadKind::CommitYesNo {
                    operation: owner, ..
                }
                | ReadKind::CommitUsername {
                    operation: owner, ..
                } if *owner == operation => Some(session.id()),
                _ => None,
            })
        {
            self.end_session(id, window, cx);
        }
    }
}
