use super::{
    AppShell,
    git::{GitRead, operation_id},
};
use crate::buffer::{Buffer, BufferId};
use crate::commit::{Action, Preparation, Repository, Runtime, Scope};
use crate::diff::{Comparison, Diff, DiffKind, RenderedDiff};
use crate::input::DisplayId;
use crate::session::ReadKind;
use crate::view::BufferView;
use gpui_kit::{Context, Entity, Window};
use std::sync::{Arc, atomic::AtomicBool};

#[cfg(test)]
#[path = "commit_start_tests.rs"]
mod tests;

enum Prepared {
    Staged(Repository),
    All(Box<(Diff, RenderedDiff)>),
    Merge(Repository),
}

impl AppShell {
    pub fn set_commit_runtime(&mut self, runtime: Runtime) {
        self.commit_runtime = Some(runtime);
    }

    fn commit_start_current(
        &self,
        origin: &Entity<BufferView>,
        input: DisplayId,
        cx: &gpui_kit::App,
    ) -> bool {
        !self.exiting()
            && self.minibuffer.is_none()
            && self.active_view() == origin
            && self.buffer_index(origin, cx).is_some()
            && origin.read(cx).display_snapshot().id() == input
            && origin.read(cx).git_open_ready()
    }

    pub(super) fn prepare_commit(
        &mut self,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.prepare_commit_action(Action::Create, origin, window, cx);
    }

    pub(super) fn prepare_commit_action(
        &mut self,
        action: Action,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.git_available(origin, cx)
            || self.minibuffer.is_some()
            || !origin.read(cx).git_open_ready()
        {
            return;
        }
        if self.commit_runtime.is_none() {
            self.show_message(
                origin,
                "Git editor helper is not configured for this application",
                cx,
            );
            return;
        }
        let initial = origin.read(cx).buffer().history_commit();
        let repository = origin.read(cx).buffer().repository();
        let directory = self.default_directory_for(origin, cx);
        let origin = origin.clone();
        let input = origin.read(cx).display_snapshot().id();
        let operation = operation_id();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = cancelled.clone();
        #[cfg(test)]
        let gate = self.commit_start_gate.take();
        let task = cx.spawn_in(window, async move |shell, cx| {
            #[cfg(test)]
            if let Some(gate) = gate {
                let _ = gate.await;
            }
            if shell
                .update_in(cx, |shell, window, cx| {
                    if !shell.owns_git_read(operation)
                        || !shell.commit_start_current(&origin, input, cx)
                    {
                        shell.finish_git_read(operation, window, cx);
                        false
                    } else {
                        true
                    }
                })
                .ok()
                != Some(true)
            {
                return;
            }
            let result = cx
                .background_executor()
                .spawn(async move {
                    let repository = match repository {
                        Some(repository) => repository,
                        None => crate::git::ReadEnvironment::Commit
                            .discover(&directory, &worker_cancelled)?,
                    };
                    let preparation = if matches!(action, Action::Amend | Action::Reword) {
                        Preparation::Staged
                    } else {
                        crate::commit::prepare(&repository, &worker_cancelled)?
                    };
                    match preparation {
                        Preparation::Staged => Ok(Prepared::Staged(repository)),
                        Preparation::Merge => Ok(Prepared::Merge(repository)),
                        Preparation::All => {
                            let data = Diff::load(
                                Comparison {
                                    repository,
                                    kind: DiffKind::Unstaged,
                                    environment: crate::diff::ReadEnvironment::Commit,
                                },
                                &worker_cancelled,
                            )?;
                            let rendered = data.render()?;
                            Ok::<_, String>(Prepared::All(Box::new((data, rendered))))
                        }
                    }
                })
                .await;
            shell
                .update_in(cx, |shell, window, cx| {
                    if !shell.finish_git_read(operation, window, cx)
                        || !shell.commit_start_current(&origin, input, cx)
                    {
                        return;
                    }
                    match result {
                        Ok(Prepared::All(preview)) => {
                            let (data, rendered) = *preview;
                            let repository = data.comparison.repository.clone();
                            let existing = shell.buffers.iter().position(|slot| {
                                slot.buffer.comparison().as_ref() == Some(&data.comparison)
                            });
                            let index = existing.unwrap_or_else(|| {
                                shell.create_buffer(
                                    Buffer::diff(data.comparison.clone()),
                                    window,
                                    cx,
                                )
                            });
                            let request =
                                shell.buffers[index].buffer.begin_diff_request().unwrap().1;
                            shell.install_diff_result(index, request, data, rendered, cx);
                            shell.display_temporary(index, window, cx);
                            let pane = *shell.panes.selected();
                            let buffer = shell.active_buffer_id();
                            let target = shell.active_view().clone();
                            shell.begin_read(
                                ReadKind::CommitAll {
                                    request: crate::commit::Request {
                                        repository,
                                        action,
                                        initial,
                                    },
                                    pane,
                                    buffer,
                                },
                                &target,
                                "",
                                window,
                                cx,
                            );
                        }
                        Ok(Prepared::Merge(repository)) => {
                            shell.begin_read(
                                ReadKind::CommitMerge {
                                    request: crate::commit::Request {
                                        repository,
                                        action,
                                        initial,
                                    },
                                },
                                &origin,
                                "",
                                window,
                                cx,
                            );
                        }
                        Ok(Prepared::Staged(repository)) => {
                            shell.continue_commit_action(
                                crate::commit::Request {
                                    repository,
                                    action,
                                    initial,
                                },
                                Scope::Staged,
                                window,
                                cx,
                            );
                        }
                        Err(error) => shell.show_message(&origin, error, cx),
                    }
                })
                .ok();
        });
        self.git_task = Some(super::git::GitTask::CommitPrepare(GitRead {
            operation,
            cancelled,
            _task: task,
        }));
    }

    pub(super) fn cancel_commit_preparation(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(self.git_task, Some(super::git::GitTask::CommitPrepare(_))) {
            self.git_task = None;
            self.schedule_repository_refresh(window, cx);
        }
    }

    pub(super) fn answer_commit_all(
        &mut self,
        request: crate::commit::Request,
        preview: (u64, BufferId),
        accepted: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (pane, buffer) = preview;
        if self
            .body_panes
            .get(&pane)
            .is_some_and(|pane| pane.current == buffer)
        {
            self.quit_temporary_pane(pane, window, cx);
        }
        if accepted {
            self.continue_commit_action(request, Scope::All, window, cx);
        }
    }

    pub(super) fn launch_commit(
        &mut self,
        repository: Repository,
        scope: Scope,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(runtime) = self.commit_runtime.clone() else {
            return;
        };
        self.start_scoped_commit(repository, runtime, scope, window, cx);
    }
}
