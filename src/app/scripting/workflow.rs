use super::super::{AppShell, index_write::StatusWriteTarget};
use crate::buffer::RegionId;
use crate::command_registry::{CommandId, ScriptCommand};
use crate::input::DisplayId;
use crate::scripting::{StatusSelection, WorkflowContext, WorkflowRequest};
use crate::status::{Repository, index::Operation};
use crate::view::BufferView;
use gpui_kit::{Context, Entity, Window};

#[cfg(test)]
#[path = "workflow_tests.rs"]
mod tests;

struct Target {
    origin: Entity<BufferView>,
    generation: u64,
    display: DisplayId,
    region: RegionId,
    revision: u64,
    status: Option<(Repository, u64)>,
    repository: Option<Repository>,
    directory: std::path::PathBuf,
    write: Result<StatusWriteTarget, String>,
}

impl AppShell {
    fn workflow_ready(&self, origin: &Entity<BufferView>, cx: &gpui_kit::App) -> bool {
        !self.exiting()
            && self.minibuffer.is_none()
            && self.session.is_none()
            && self.opening.is_none()
            && self.active_view() == origin
            && self.buffer_index(origin, cx).is_some()
            && origin.read(cx).git_write_ready()
    }

    pub(super) fn start_workflow(
        &mut self,
        command: ScriptCommand,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if self.script_task.is_some() {
            return Err("A script command is already running".into());
        }
        if command.generation != self.config_generation || !self.workflow_ready(origin, cx) {
            return Err(
                "Workflow target is not ready or belongs to an obsolete configuration".into(),
            );
        }
        let view = origin.read(cx);
        let buffer = view.buffer();
        let write = self.capture_status_write(origin, cx);
        let selection = buffer.status_leaf().and_then(|(location, _)| {
            let data = buffer.status_data()?;
            let supported = |operation| {
                write.is_ok()
                    && data
                        .index_request(location.group, location.entry, operation)
                        .is_ok()
            };
            Some(StatusSelection {
                group: location.group,
                can_stage: supported(Operation::Stage),
                can_unstage: supported(Operation::Unstage),
            })
        });
        let context = WorkflowContext {
            mode: buffer.mode(),
            selection,
        };
        let target = Target {
            origin: origin.clone(),
            generation: self.config_generation,
            display: view.display_snapshot().id(),
            region: buffer.region_id(),
            revision: buffer.revision(),
            status: buffer.status_identity(),
            repository: buffer.repository(),
            directory: self.default_directory_for(origin, cx),
            write,
        };
        let result = self
            .script_host
            .as_ref()
            .ok_or("JavaScript worker is unavailable")?
            .workflow(command, context)?;
        #[cfg(test)]
        let gate = self.workflow_return_gate.take();
        self.script_task = Some(cx.spawn_in(window, async move |shell, cx| {
            let result = result
                .await
                .unwrap_or_else(|_| Err("JavaScript worker stopped; reload configuration".into()));
            #[cfg(test)]
            if let Some(gate) = gate {
                let _ = gate.await;
            }
            let _ = shell.update_in(cx, |shell, window, cx| {
                if shell.config_generation != command.generation {
                    return;
                }
                shell.script_task = None;
                let result = result.and_then(|request| match request {
                    None => Ok(()),
                    Some(request) => shell.accept_workflow(command, target, request, window, cx),
                });
                if let Err(error) = result {
                    shell.show_message(&shell.active_view().clone(), error, cx);
                }
            });
        }));
        Ok(())
    }

    fn accept_workflow(
        &mut self,
        command: ScriptCommand,
        target: Target,
        request: WorkflowRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if target.generation != self.config_generation || !self.workflow_ready(&target.origin, cx) {
            return Err(
                "Workflow result discarded: original target or input session changed".into(),
            );
        }
        let view = target.origin.read(cx);
        let buffer = view.buffer();
        if view.display_snapshot().id() != target.display
            || buffer.region_id() != target.region
            || buffer.revision() != target.revision
            || buffer.status_identity() != target.status
            || buffer.repository() != target.repository
            || self.default_directory_for(&target.origin, cx) != target.directory
            || !self
                .commands
                .available_in(CommandId::Script(command), buffer.mode())
        {
            return Err("Workflow result discarded: original target changed".into());
        }
        if self.git_task.is_some() {
            return Err("A Git operation is already running".into());
        }
        match request {
            WorkflowRequest::Stage | WorkflowRequest::Unstage => {
                let operation = if request == WorkflowRequest::Stage {
                    Operation::Stage
                } else {
                    Operation::Unstage
                };
                self.accept_status_write(target.write?, operation, window, cx)?;
            }
            WorkflowRequest::StatusOpen => self.open_status(&target.origin, window, cx),
            WorkflowRequest::StatusRefresh => {
                if target.status.is_none() {
                    return Err("Workflow status refresh requires a status session".into());
                }
                self.refresh_status(&target.origin, window, cx);
            }
            WorkflowRequest::DiffOpen(kind) => self.open_diff(kind, &target.origin, window, cx),
        }
        Ok(())
    }
}
