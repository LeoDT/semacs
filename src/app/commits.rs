use std::path::PathBuf;
use std::process::ExitStatus;
use std::sync::atomic::AtomicBool;

use gpui_kit::{Context, Task, Window};

use super::{AppShell, git::operation_id};
use crate::commit::{Commit, Event, Repository};

#[cfg(test)]
#[path = "commit_session_tests.rs"]
mod tests;

#[derive(Debug)]
pub enum CommitPhase {
    Starting,
    Running,
    Exited(ExitStatus),
    Failed { started: bool, diagnostic: String },
}

impl CommitPhase {
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Starting | Self::Running)
    }
}

pub struct CommitRecord {
    pub operation: u64,
    pub repository: Repository,
    pub phase: CommitPhase,
    pub output: Vec<u8>,
    pub diagnostics: Vec<String>,
}

pub(super) struct Session {
    pub record: CommitRecord,
    pub(super) scope: crate::commit::Scope,
    pub(super) target: super::commit_editor::EditorTarget,
    pub(super) process: Option<Commit>,
    pub(super) interrupted: bool,
    pub(super) exit_termination_requested: bool,
    pub(super) interaction: super::commit_prompt::Interaction,
    _task: Task<()>,
}

impl AppShell {
    pub fn commit_records(&self) -> impl Iterator<Item = &CommitRecord> {
        self.commits.iter().map(|session| &session.record)
    }

    pub(super) fn commit_active(&self, operation: u64) -> bool {
        self.commits.iter().any(|session| {
            session.record.operation == operation && session.record.phase.is_active()
        })
    }

    pub fn start_commit(
        &mut self,
        repository: Repository,
        editor_executable: PathBuf,
        runtime_directory: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> u64 {
        self.start_scoped_commit(
            repository,
            crate::commit::Runtime {
                executable: editor_executable,
                directory: runtime_directory,
            },
            crate::commit::Scope::Staged,
            window,
            cx,
        )
    }

    pub(super) fn start_scoped_commit(
        &mut self,
        repository: Repository,
        runtime: crate::commit::Runtime,
        scope: crate::commit::Scope,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> u64 {
        let operation = operation_id();
        let worker_repository = repository.clone();
        let task = cx.spawn_in(window, async move |shell, cx| {
            let started = cx
                .background_executor()
                .spawn(async move {
                    let prefix = comment_prefix(&worker_repository);
                    Commit::start_with_scope(
                        &worker_repository,
                        &runtime.executable,
                        &runtime.directory,
                        scope,
                    )
                    .map(|(process, events)| (process, events, prefix))
                })
                .await;
            let (process, mut events, prefix) = match started {
                Ok(started) => started,
                Err(error) => {
                    shell
                        .update_in(cx, |shell, window, cx| {
                            shell.commit_terminal(
                                operation,
                                CommitPhase::Failed {
                                    started: error.started,
                                    diagnostic: error.diagnostic,
                                },
                                window,
                                cx,
                            );
                        })
                        .ok();
                    return;
                }
            };
            if shell
                .update_in(cx, |shell, _, cx| {
                    let Some(session) = shell
                        .commits
                        .iter_mut()
                        .find(|session| session.record.operation == operation)
                    else {
                        return false;
                    };
                    session.process = Some(process);
                    session.record.phase = CommitPhase::Running;
                    let repository = session.record.repository.clone();
                    if session.exit_termination_requested
                        && let Err(message) = session.terminate_for_exit()
                    {
                        shell.cancel_closing();
                        shell.show_message(&shell.active_view().clone(), message, cx);
                    }
                    shell.refresh_commit_process(&repository, cx);
                    cx.notify();
                    true
                })
                .ok()
                != Some(true)
            {
                return;
            }
            loop {
                let event = events.next().await;
                let terminal =
                    matches!(&event, Ok(Event::Exited(_) | Event::WaitFailed(_)) | Err(_));
                if terminal {
                    let process = shell
                        .update_in(cx, |shell, _, _| {
                            shell
                                .commits
                                .iter_mut()
                                .find(|s| s.record.operation == operation)
                                .and_then(|s| s.process.take())
                        })
                        .ok()
                        .flatten();
                    if let Some(process) = process {
                        cx.background_executor()
                            .spawn(async move {
                                let _ = process.join();
                            })
                            .await;
                    }
                }
                let consumed = shell.update_in(cx, |shell, window, cx| {
                    shell.consume_commit_event(operation, event, &prefix, window, cx);
                });
                if consumed.is_err() || terminal {
                    break;
                }
            }
        });
        self.prune_commit_history(&repository);
        self.commits.push(Session {
            scope,
            target: super::commit_editor::EditorTarget::capture(self, cx),
            record: CommitRecord {
                operation,
                repository,
                phase: CommitPhase::Starting,
                output: Vec::new(),
                diagnostics: Vec::new(),
            },
            process: None,
            interrupted: false,
            exit_termination_requested: false,
            interaction: Default::default(),
            _task: task,
        });
        let repository = self.commits.last().unwrap().record.repository.clone();
        self.refresh_commit_process(&repository, cx);
        cx.notify();
        operation
    }

    fn prune_commit_history(&mut self, repository: &Repository) {
        let count = self
            .commits
            .iter()
            .filter(|session| &session.record.repository == repository)
            .count();
        if count < 32 {
            return;
        }
        let mut remaining = count / 2;
        self.commits.retain(|session| {
            if remaining > 0
                && &session.record.repository == repository
                && !session.record.phase.is_active()
            {
                remaining -= 1;
                false
            } else {
                true
            }
        });
    }

    fn consume_commit_event(
        &mut self,
        operation: u64,
        event: Result<Event, std::sync::mpsc::RecvError>,
        prefix: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.commit_active(operation) {
            return;
        }
        match event {
            Ok(Event::Edit(request)) => self.edit_owned_commit_message(
                request,
                prefix.to_owned(),
                Some(operation),
                self.commits
                    .iter()
                    .find(|session| session.record.operation == operation)
                    .unwrap()
                    .target
                    .clone(),
                window,
                cx,
            ),
            Ok(Event::Output(bytes)) => {
                let session = self
                    .commits
                    .iter_mut()
                    .find(|session| session.record.operation == operation)
                    .unwrap();
                session.record.output.extend(bytes);
                session.interaction.scan(&session.record.output);
            }
            Ok(Event::Diagnostic(message)) => self
                .commits
                .iter_mut()
                .find(|session| session.record.operation == operation)
                .unwrap()
                .record
                .diagnostics
                .push(message),
            Ok(Event::Exited(status)) => {
                self.commit_terminal(operation, CommitPhase::Exited(status), window, cx);
            }
            Ok(Event::WaitFailed(diagnostic)) => {
                self.commit_terminal(
                    operation,
                    CommitPhase::Failed {
                        started: true,
                        diagnostic,
                    },
                    window,
                    cx,
                );
            }
            Err(error) => {
                self.commit_terminal(
                    operation,
                    CommitPhase::Failed {
                        started: true,
                        diagnostic: format!(
                            "Git event stream ended without a terminal event: {error}"
                        ),
                    },
                    window,
                    cx,
                );
            }
        }
        if self.commit_active(operation) {
            let repository = self
                .commits
                .iter()
                .find(|session| session.record.operation == operation)
                .unwrap()
                .record
                .repository
                .clone();
            self.refresh_commit_process(&repository, cx);
        }
        self.present_commit_prompt(window, cx);
        cx.notify();
    }

    fn commit_terminal(
        &mut self,
        operation: u64,
        phase: CommitPhase,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self
            .commits
            .iter_mut()
            .find(|session| session.record.operation == operation)
        else {
            return;
        };
        if !session.record.phase.is_active() {
            return;
        }
        session.record.phase = phase;
        session.interaction.clear();
        let repository = session.record.repository.clone();
        let command_buffer = session.target.buffer_id(cx);
        let started = !matches!(
            session.record.phase,
            CommitPhase::Failed { started: false, .. }
        );
        self.release_commit_prompt(operation, window, cx);
        self.release_commit_editors(operation, window, cx);
        if started {
            self.queue_commit_refresh(&repository, command_buffer, window, cx);
        }
        self.refresh_commit_process(&repository, cx);
        let origin = self.active_view().clone();
        let record = &self
            .commits
            .iter()
            .find(|session| session.record.operation == operation)
            .unwrap()
            .record;
        let message = match &record.phase {
            CommitPhase::Exited(status) if status.success() => "Git commit finished".into(),
            CommitPhase::Exited(status) => format!("Git commit failed ({status})"),
            CommitPhase::Failed { diagnostic, .. } => format!("Git commit failed: {diagnostic}"),
            _ => unreachable!(),
        };
        self.show_message(&origin, message, cx);
        self.resume_exit_after_commit(window, cx);
    }
}

fn comment_prefix(repository: &Repository) -> String {
    crate::git::ReadEnvironment::Commit
        .read(
            repository,
            &[
                "config".as_ref(),
                "--get".as_ref(),
                "core.commentChar".as_ref(),
            ],
            &AtomicBool::new(false),
        )
        .ok()
        .and_then(|mut bytes| {
            if bytes.last() == Some(&b'\n') {
                bytes.pop();
            }
            String::from_utf8(bytes).ok()
        })
        .unwrap_or_else(|| "#".into())
}
