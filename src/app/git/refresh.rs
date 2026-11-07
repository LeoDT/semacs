use std::sync::{Arc, atomic::AtomicBool};

use gpui_kit::{Context, Window};

use super::{GitRead, GitTask, operation_id};
use crate::app::AppShell;
use crate::buffer::{Buffer, BufferId};
use crate::diff::{Comparison, Diff, RenderedDiff};
use crate::git::Repository;
use crate::status::{RenderedStatus, Status};

pub(crate) struct Target {
    buffer: BufferId,
    request: u64,
    projection: Projection,
}

enum Projection {
    Status(Repository),
    Diff(Comparison),
}

enum Loaded {
    Status(Status, RenderedStatus),
    Diff(Diff, RenderedDiff),
}

impl Target {
    fn matches(&self, buffer: &Buffer) -> bool {
        match &self.projection {
            Projection::Status(repository) => {
                buffer.status_request_matches(self.request, repository)
            }
            Projection::Diff(comparison) => buffer.diff_request_matches(self.request, comparison),
        }
    }

    fn load(&self, cancelled: &AtomicBool) -> Result<Loaded, String> {
        match &self.projection {
            Projection::Status(repository) => {
                let data = Status::load(repository.clone(), cancelled)?;
                let rendered = data.render()?;
                Ok(Loaded::Status(data, rendered))
            }
            Projection::Diff(comparison) => {
                let data = Diff::load(comparison.clone(), cancelled)?;
                let rendered = data.render()?;
                Ok(Loaded::Diff(data, rendered))
            }
        }
    }
}

impl AppShell {
    pub(in crate::app) fn git_read_current(&self, operation: u64, repository: &Repository) -> bool {
        self.git_changes
            .get(repository)
            .is_none_or(|changed| operation > *changed)
    }

    pub(in crate::app) fn queue_commit_refresh(
        &mut self,
        repository: &Repository,
        command_buffer: BufferId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.git_changes.insert(repository.clone(), operation_id());
        self.mark_repository_stale(repository, cx);
        for slot in self.buffers.iter_mut() {
            if slot.buffer.repository().as_ref() != Some(repository) {
                continue;
            }
            let projection = if let Ok((repository, request)) = slot.buffer.begin_status_request() {
                Some((Projection::Status(repository), request))
            } else if let Ok((comparison, request)) = slot.buffer.begin_diff_request() {
                (slot.id() == command_buffer).then_some((Projection::Diff(comparison), request))
            } else {
                None
            };
            self.git_refreshes
                .retain(|target| target.buffer != slot.id());
            if let Some((projection, request)) = projection {
                self.git_refreshes.push_back(Target {
                    buffer: slot.id(),
                    request,
                    projection,
                });
            }
        }
        self.schedule_repository_refresh(window, cx);
    }

    pub(in crate::app) fn schedule_repository_refresh(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.git_refreshes.is_empty() {
            return;
        }
        cx.defer_in(window, |shell, window, cx| {
            shell.next_repository_refresh(window, cx)
        });
    }

    fn next_repository_refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.git_task.is_some() || self.exiting() {
            return;
        }
        let target = loop {
            let Some(target) = self.git_refreshes.pop_front() else {
                return;
            };
            if self
                .buffers
                .index_of(target.buffer)
                .is_some_and(|index| target.matches(&self.buffers[index].buffer))
            {
                break target;
            }
        };
        let operation = operation_id();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = cancelled.clone();
        #[cfg(test)]
        let gate = self.repository_refresh_gate.take();
        let task = cx.spawn_in(window, async move |shell, cx| {
            #[cfg(test)]
            if let Some(gate) = gate {
                let _ = gate.await;
            }
            let (target, result) = cx
                .background_executor()
                .spawn(async move {
                    let result = target.load(&worker_cancelled);
                    (target, result)
                })
                .await;
            shell
                .update_in(cx, |shell, window, cx| {
                    if !shell.finish_git_read(operation, window, cx) {
                        return;
                    }
                    let Some(index) = shell.buffers.index_of(target.buffer) else {
                        return;
                    };
                    if !target.matches(&shell.buffers[index].buffer) {
                        return;
                    }
                    match result {
                        Ok(Loaded::Status(data, rendered)) => {
                            shell.install_status_result(index, target.request, data, rendered, cx);
                        }
                        Ok(Loaded::Diff(data, rendered)) => {
                            shell.install_diff_result(index, target.request, data, rendered, cx);
                        }
                        Err(error) => {
                            shell.show_message(
                                &shell.active_view().clone(),
                                format!("Git process finished; repository refresh failed: {error}"),
                                cx,
                            );
                        }
                    }
                })
                .ok();
        });
        self.git_task = Some(GitTask::Read(GitRead {
            operation,
            cancelled,
            _task: task,
        }));
    }
}
