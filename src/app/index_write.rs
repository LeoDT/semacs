use super::{
    AppShell, GitWritePhase,
    git::{GitTask, operation_id},
};
use crate::buffer::{BufferId, RegionId};
use crate::input::DisplayId;
use crate::status::{
    RenderedStatus, Repository, Status, StatusLocation,
    index::{self, Operation, Outcome},
};
use crate::view::BufferView;
use gpui_kit::{Context, Entity, Window};
use std::sync::Arc;

#[cfg(test)]
#[path = "index_write_tests.rs"]
mod tests;

/// A host-only target fence. Scripts receive neither this value nor its repository/path.
pub struct StatusWriteTarget {
    origin: Entity<BufferView>,
    generation: u64,
    display: DisplayId,
    region: RegionId,
    revision: u64,
    request: u64,
    location: StatusLocation,
    data: Arc<Status>,
}

struct Completion {
    outcome: Outcome,
    diagnostic: String,
    stale: bool,
    verification: Option<Result<(Status, RenderedStatus), String>>,
}

impl Completion {
    fn from_report(report: index::Report) -> Self {
        let verification = report.verification.map(|result| {
            result.and_then(|data| {
                let rendered = data.render()?;
                Ok((data, rendered))
            })
        });
        Self {
            outcome: report.outcome,
            diagnostic: report.diagnostic,
            stale: report.stale,
            verification,
        }
    }
}

impl AppShell {
    pub fn capture_status_write(
        &self,
        origin: &Entity<BufferView>,
        cx: &gpui_kit::App,
    ) -> Result<StatusWriteTarget, String> {
        if self.exiting()
            || self.minibuffer.is_some()
            || self.session.is_some()
            || self.opening.is_some()
            || self.active_view() != origin
            || self.buffer_index(origin, cx).is_none()
            || !origin.read(cx).git_write_ready()
        {
            return Err("Status write target is not ready".into());
        }
        let view = origin.read(cx);
        let buffer = view.buffer();
        let (location, request) = buffer.status_selection().ok_or(
            "Select one visible status file without an active region; refresh stale status first",
        )?;
        let data = buffer.status_data().ok_or("Status is not ready")?;
        Ok(StatusWriteTarget {
            origin: origin.clone(),
            generation: self.config_generation,
            display: view.display_snapshot().id(),
            region: buffer.region_id(),
            revision: buffer.revision(),
            request,
            location,
            data,
        })
    }

    /// Accepts an already captured target; it never reselects a file from the current point.
    pub fn accept_status_write(
        &mut self,
        target: StatusWriteTarget,
        operation: Operation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<u64, String> {
        if self.git_task.is_some() {
            return Err("A Git operation is already running".into());
        }
        if target.generation != self.config_generation {
            return Err("Status write belongs to an obsolete configuration".into());
        }
        let current = self.capture_status_write(&target.origin, cx)?;
        if current.display != target.display
            || current.region != target.region
            || current.revision != target.revision
            || current.request != target.request
            || current.location.group != target.location.group
            || current.location.entry != target.location.entry
            || !Arc::ptr_eq(&current.data, &target.data)
        {
            return Err("Status write target changed; invoke the command again".into());
        }
        let request =
            target
                .data
                .index_request(target.location.group, target.location.entry, operation)?;
        let buffer = target.display.committed.buffer;
        let index = self
            .buffers
            .index_of(buffer)
            .ok_or("Status session disappeared")?;
        let (repository, refresh_request) = self.buffers[index].buffer.begin_status_request()?;
        let id = operation_id();
        self.git_writes
            .accept(id, &request, buffer, refresh_request);
        let origin = target.origin;
        self.show_message(&origin, match operation {
            Operation::Stage => "Staging current disk content; unsaved buffers are not saved. This can replace an index-only version; no automatic undo.",
            Operation::Unstage => "Restoring this index path to the captured HEAD; worktree is unchanged. Index-only content can be lost; no automatic undo.",
        }, cx);

        let (started_send, started_receive) = futures_channel::oneshot::channel();
        #[cfg(test)]
        let write_gate = self.index_write_gate.take();
        #[cfg(test)]
        let verify_gate = self.index_verify_gate.take();
        let expected_repository = repository.clone();
        let task = cx.spawn_in(window, async move |shell, cx| {
            #[cfg(test)]
            if let Some(gate) = write_gate {
                let _ = gate.await;
            }
            let worker = cx.background_executor().spawn(async move {
                let mut started_send = Some(started_send);
                request.write_observed(&mut || {
                    if let Some(send) = started_send.take() {
                        let _ = send.send(());
                    }
                })
            });
            if started_receive.await.is_ok() {
                let _ = shell.update_in(cx, |shell, _, cx| {
                    if !shell.owns_git_write(id) {
                        return;
                    }
                    if let Some(record) = shell.git_writes.get_mut(id) {
                        record.phase = GitWritePhase::Executing;
                    }
                    shell.mark_repository_stale(&expected_repository, cx);
                });
            }
            let completion = match worker.await {
                Err(report) => Completion::from_report(*report),
                Ok(executed) => {
                    let _ = shell.update_in(cx, |shell, _, _| {
                        if !shell.owns_git_write(id) {
                            return;
                        }
                        if let Some(record) = shell.git_writes.get_mut(id) {
                            record.phase = GitWritePhase::Verifying;
                            record.outcome = Some(executed.outcome);
                            record.diagnostic = executed.diagnostic.clone();
                        }
                    });
                    #[cfg(test)]
                    if let Some(gate) = verify_gate {
                        let _ = gate.await;
                    }
                    cx.background_executor()
                        .spawn(async move { Completion::from_report(executed.verify()) })
                        .await
                }
            };
            let _ = shell.update_in(cx, |shell, window, cx| {
                if !shell.owns_git_write(id) {
                    return;
                }
                shell.complete_status_write(
                    id,
                    buffer,
                    refresh_request,
                    repository,
                    completion,
                    cx,
                );
                shell.git_task = None;
                if shell.exiting() {
                    shell.process_next_exit(window, cx);
                }
            });
        });
        self.git_task = Some(GitTask::IndexWrite {
            operation: id,
            _task: task,
        });
        Ok(id)
    }

    fn complete_status_write(
        &mut self,
        id: u64,
        buffer: BufferId,
        request: u64,
        repository: Repository,
        completion: Completion,
        cx: &mut Context<Self>,
    ) {
        if completion.stale {
            self.mark_repository_stale(&repository, cx);
        }
        let (verified, installed, refresh_error) = match completion.verification {
            None => (None, false, None),
            Some(Err(error)) => (Some(false), false, Some(error)),
            Some(Ok((data, rendered))) => {
                let installed = self.buffers.index_of(buffer).is_some_and(|index| {
                    self.buffers[index]
                        .buffer
                        .status_request_matches(request, &repository)
                        && self.install_status_result(index, request, data, rendered, cx)
                });
                (Some(true), installed, None)
            }
        };
        if let Some(record) = self.git_writes.get_mut(id) {
            record.outcome = Some(completion.outcome);
            record.verified = verified;
            record.installed = installed;
            record.diagnostic = index::bounded(match refresh_error {
                Some(error) => format!("{}\nRefresh failed: {error}", completion.diagnostic),
                None => completion.diagnostic,
            });
        }
        self.git_writes.complete(id);
        let message = self
            .git_writes
            .iter()
            .find(|record| record.id == id)
            .map(|record| record.summary());
        if let Some(message) = message {
            self.show_message(&self.active_view().clone(), message, cx);
        }
    }
}
