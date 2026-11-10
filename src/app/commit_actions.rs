use std::sync::{Arc, atomic::AtomicBool};

use gpui_kit::{Context, Entity, Keystroke, Window};

use super::{
    AppShell,
    git::{GitRead, GitTask, operation_id},
    history::Purpose,
};
use crate::buffer::BufferId;
use crate::command::Command;
use crate::commit::{Action, Continuation, Intent, Plan, Repository, Scope, TargetKind};
use crate::diff::{Comparison, Diff, DiffKind, ReadEnvironment};
use crate::session::ReadKind;
use crate::view::BufferView;

pub(super) struct Selection {
    pub repository: Repository,
    pub scope: Scope,
    pub kind: TargetKind,
    pub initial: Option<String>,
    operation: u64,
    companions: Vec<(u64, BufferId)>,
}

impl AppShell {
    pub(super) fn commit_action_command(
        &mut self,
        command: Command,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match command {
            Command::MagitCommit => self.open_commit_menu(origin, window, cx),
            Command::MagitCommitAmend => {
                self.prepare_commit_action(Action::Amend, origin, window, cx)
            }
            Command::MagitCommitReword => {
                self.prepare_commit_action(Action::Reword, origin, window, cx)
            }
            Command::MagitCommitFixup => {
                self.prepare_commit_action(Action::Target(TargetKind::Fixup), origin, window, cx)
            }
            Command::MagitCommitSquash => {
                self.prepare_commit_action(Action::Target(TargetKind::Squash), origin, window, cx)
            }
            Command::MagitLogSelectPick => self.finish_commit_selection(true, origin, window, cx),
            Command::MagitLogSelectQuit => self.finish_commit_selection(false, origin, window, cx),
            _ => {}
        }
    }

    fn show_commit_menu(
        &mut self,
        repository: &Repository,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let editing = self
            .commit_editors
            .keys()
            .copied()
            .find(|id| self.commit_editor_repository(*id).as_ref() == Some(repository));
        if let Some(index) = editing.and_then(|id| self.buffers.index_of(id)) {
            self.display_temporary(index, window, cx);
        } else {
            self.begin_read(ReadKind::CommitMenu, origin, "", window, cx);
        }
    }

    fn open_commit_menu(
        &mut self,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let repository = self
            .commit_editor_repository(origin.read(cx).buffer().snapshot_id().buffer)
            .or_else(|| origin.read(cx).buffer().repository());
        if let Some(repository) = repository {
            self.show_commit_menu(&repository, origin, window, cx);
            return;
        }
        if !self.git_available(origin, cx)
            || !origin.read(cx).git_open_ready()
            || self.minibuffer.is_some()
        {
            return;
        }
        let directory = self.default_directory_for(origin, cx);
        let source = origin.read(cx).display_snapshot().id();
        let origin = origin.clone();
        let operation = operation_id();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = cancelled.clone();
        let task = cx.spawn_in(window, async move |shell, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    crate::git::ReadEnvironment::Commit.discover(&directory, &worker_cancelled)
                })
                .await;
            shell
                .update_in(cx, |shell, window, cx| {
                    if !shell.finish_git_read(operation, window, cx)
                        || shell.exiting()
                        || shell.minibuffer.is_some()
                        || shell.active_view() != &origin
                        || shell.buffer_index(&origin, cx).is_none()
                        || origin.read(cx).display_snapshot().id() != source
                        || !origin.read(cx).git_open_ready()
                    {
                        return;
                    }
                    match result {
                        Ok(repository) => shell.show_commit_menu(&repository, &origin, window, cx),
                        Err(error) => shell.show_message(&origin, error, cx),
                    }
                })
                .ok();
        });
        self.git_task = Some(GitTask::CommitPrepare(GitRead {
            operation,
            cancelled,
            _task: task,
        }));
    }

    pub(super) fn answer_commit_choice(
        &mut self,
        stroke: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(session) = &self.session else {
            return false;
        };
        let id = session.id();
        let kind = session.kind().clone();
        match kind {
            ReadKind::CommitMenu => {
                let command = match stroke.key.as_str() {
                    "c" => Command::MagitCommitCreate,
                    "a" => Command::MagitCommitAmend,
                    "w" => Command::MagitCommitReword,
                    "f" => Command::MagitCommitFixup,
                    "s" => Command::MagitCommitSquash,
                    _ => {
                        self.message_minibuffer(kind.answer_hint(), cx);
                        return true;
                    }
                };
                if let Some(origin) = self.end_session(id, window, cx) {
                    self.run_command(command.into(), &origin, window, cx);
                }
                true
            }
            ReadKind::CommitTargetMerges { next } => {
                let key = stroke.key.as_str();
                if !["c", "s", "a"].contains(&key) {
                    self.message_minibuffer("Choose c, s, or a", cx);
                    return true;
                }
                if let Some(origin) = self.end_session(id, window, cx) {
                    match key {
                        "c" => self.perform_commit_continuation(next, window, cx),
                        "s" => {
                            let select = match next {
                                Continuation::Select {
                                    repository,
                                    scope,
                                    kind,
                                    ..
                                } => Some(Continuation::Select {
                                    repository,
                                    scope,
                                    kind,
                                    initial: None,
                                }),
                                Continuation::Launch(Plan {
                                    repository,
                                    scope,
                                    intent: Intent::Fixup(_),
                                }) => Some(Continuation::Select {
                                    repository,
                                    scope,
                                    kind: TargetKind::Fixup,
                                    initial: None,
                                }),
                                Continuation::Launch(Plan {
                                    repository,
                                    scope,
                                    intent: Intent::Squash(_),
                                }) => Some(Continuation::Select {
                                    repository,
                                    scope,
                                    kind: TargetKind::Squash,
                                    initial: None,
                                }),
                                _ => None,
                            };
                            if let Some(next) = select {
                                self.perform_commit_continuation(next, window, cx);
                            }
                        }
                        _ => self.show_message(&origin, "Quit", cx),
                    }
                }
                true
            }
            _ => false,
        }
    }

    pub(super) fn continue_commit_action(
        &mut self,
        request: crate::commit::Request,
        scope: Scope,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let crate::commit::Request {
            repository,
            action,
            initial,
        } = request;
        let next = match action {
            Action::Create => {
                self.launch_commit(repository, scope, window, cx);
                return;
            }
            Action::Amend => Continuation::Launch(Plan {
                repository,
                scope,
                intent: Intent::Amend,
            }),
            Action::Reword => Continuation::Launch(Plan {
                repository,
                scope,
                intent: Intent::Reword,
            }),
            Action::Target(kind) => Continuation::Select {
                repository,
                scope,
                kind,
                initial,
            },
        };
        self.check_commit_continuation(next, window, cx);
    }

    fn check_commit_continuation(
        &mut self,
        next: Continuation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (repository, target, amending) = match &next {
            Continuation::Select {
                repository,
                initial: Some(target),
                ..
            } => (repository.clone(), target.clone(), false),
            Continuation::Select { initial: None, .. } => {
                self.perform_commit_continuation(next, window, cx);
                return;
            }
            Continuation::Launch(Plan {
                repository,
                intent: Intent::Amend | Intent::Reword,
                ..
            }) => (repository.clone(), "HEAD".into(), true),
            Continuation::Launch(Plan {
                repository,
                intent: Intent::Fixup(target) | Intent::Squash(target),
                ..
            }) => (repository.clone(), target.clone(), false),
            Continuation::Launch(_) => {
                self.perform_commit_continuation(next, window, cx);
                return;
            }
        };
        let origin = self.active_view().clone();
        if !self.git_available(&origin, cx) || !origin.read(cx).git_open_ready() {
            return;
        }
        let source = origin.read(cx).display_snapshot().id();
        let operation = operation_id();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = cancelled.clone();
        let task = cx.spawn_in(window, async move |shell, cx| {
            let checked = cx
                .background_executor()
                .spawn(async move {
                    crate::commit::check_target(&repository, &target, amending, &worker_cancelled)
                })
                .await;
            shell
                .update_in(cx, |shell, window, cx| {
                    if !shell.finish_git_read(operation, window, cx)
                        || shell.exiting()
                        || shell.minibuffer.is_some()
                        || shell.active_view() != &origin
                        || shell.buffer_index(&origin, cx).is_none()
                        || origin.read(cx).display_snapshot().id() != source
                        || !origin.read(cx).git_open_ready()
                    {
                        return;
                    }
                    match checked {
                        Ok(check) if !check.published.is_empty() => {
                            shell.begin_read(
                                ReadKind::CommitPublished {
                                    next,
                                    branches: check.published,
                                    merges: check.merges,
                                    amending,
                                },
                                &origin,
                                "",
                                window,
                                cx,
                            );
                        }
                        Ok(check) => shell.continue_checked_commit(next, check.merges, window, cx),
                        Err(error) => shell.show_message(&origin, error, cx),
                    }
                })
                .ok();
        });
        self.git_task = Some(GitTask::CommitPrepare(GitRead {
            operation,
            cancelled,
            _task: task,
        }));
    }

    pub(super) fn continue_checked_commit(
        &mut self,
        next: Continuation,
        merges: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if merges {
            let origin = self.active_view().clone();
            self.begin_read(
                ReadKind::CommitTargetMerges { next },
                &origin,
                "",
                window,
                cx,
            );
        } else {
            self.perform_commit_continuation(next, window, cx);
        }
    }

    fn perform_commit_continuation(
        &mut self,
        next: Continuation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match next {
            Continuation::Launch(plan) => {
                if let Some(runtime) = self.commit_runtime.clone() {
                    self.start_planned_commit(plan, runtime, window, cx);
                }
            }
            Continuation::Select {
                repository,
                scope,
                kind,
                initial,
            } => {
                let selection = Selection {
                    repository: repository.clone(),
                    scope,
                    kind,
                    initial,
                    operation: operation_id(),
                    companions: Vec::new(),
                };
                let origin = self.active_view().clone();
                self.open_log(
                    &origin,
                    Some(repository),
                    Purpose::Select(selection),
                    window,
                    cx,
                );
            }
        }
    }

    fn finish_commit_selection(
        &mut self,
        picked: bool,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = origin.read(cx).buffer().snapshot_id().buffer;
        let target = if picked {
            match origin.read(cx).buffer().history_action() {
                Ok(crate::history::Action::Commit(oid)) => Some(oid),
                _ => {
                    self.show_message(origin, "No commit here", cx);
                    return;
                }
            }
        } else {
            None
        };
        let Some(selection) = self.commit_selections.remove(&id) else {
            self.show_message(origin, "No pending commit selection", cx);
            return;
        };
        self.close_selection_companions(id, &selection, window, cx);
        self.quit_temporary(window, cx);
        if let Some(index) = self.buffers.index_of(id) {
            self.kill_buffer_at(index, window, cx);
        }
        if let Some(target) = target {
            self.check_commit_continuation(
                Continuation::Launch(Plan {
                    repository: selection.repository,
                    scope: selection.scope,
                    intent: selection.kind.intent(target),
                }),
                window,
                cx,
            );
        } else {
            self.show_message(&self.active_view().clone(), "Quit", cx);
        }
    }

    fn close_selection_companions(
        &mut self,
        owner: BufferId,
        selection: &Selection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for (pane, buffer) in &selection.companions {
            self.quit_commit_companion(*pane, *buffer, owner, window, cx);
        }
    }

    pub(super) fn discard_commit_selection(
        &mut self,
        buffer: BufferId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(selection) = self.commit_selections.remove(&buffer) {
            self.close_selection_companions(buffer, &selection, window, cx);
        }
    }

    pub(super) fn show_selection_diff(
        &mut self,
        owner: BufferId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(selection) = self.commit_selections.get(&owner) else {
            return;
        };
        let token = selection.operation;
        let repository = selection.repository.clone();
        let comparison = Comparison {
            repository: repository.clone(),
            kind: DiffKind::Staged,
            environment: ReadEnvironment::Commit,
        };
        let existing = self
            .buffers
            .iter()
            .position(|slot| slot.buffer.comparison().as_ref() == Some(&comparison));
        let index = existing.unwrap_or_else(|| {
            self.create_buffer(crate::buffer::Buffer::diff(comparison.clone()), window, cx)
        });
        let id = self.buffers[index].id();
        let request = self.buffers[index].buffer.begin_diff_request().unwrap().1;
        let origin = self.active_view().clone();
        let source = origin.read(cx).display_snapshot().id();
        cx.spawn_in(window, async move |shell, cx| {
            let loaded = cx
                .background_executor()
                .spawn(async move {
                    let data = Diff::load(comparison, &AtomicBool::new(false))?;
                    let rendered = data.render()?;
                    Ok::<_, String>((data, rendered))
                })
                .await;
            shell
                .update_in(cx, |shell, window, cx| {
                    if !shell
                        .commit_selections
                        .get(&owner)
                        .is_some_and(|selection| selection.operation == token)
                    {
                        return;
                    }
                    let Some(index) = shell.buffers.index_of(id) else {
                        return;
                    };
                    match loaded {
                        Ok((data, rendered)) => {
                            if !shell.install_diff_result(index, request, data, rendered, cx) {
                                return;
                            }
                            if shell.minibuffer.is_none()
                                && shell.active_view() == &origin
                                && origin.read(cx).display_snapshot().id() == source
                                && origin.read(cx).git_open_ready()
                                && let Some(companion) =
                                    shell.display_temporary_beside(index, &repository, window, cx)
                            {
                                shell
                                    .commit_selections
                                    .get_mut(&owner)
                                    .unwrap()
                                    .companions
                                    .push(companion);
                            }
                        }
                        Err(error) => shell.show_message(
                            &origin,
                            format!("Cannot show staged diff: {error}"),
                            cx,
                        ),
                    }
                })
                .ok();
        })
        .detach();
    }
}
