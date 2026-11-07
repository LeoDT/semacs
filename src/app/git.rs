use super::AppShell;
use crate::view::BufferView;
use gpui_kit::{Context, Entity, Task, Window};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

pub(super) mod records;
pub(super) mod refresh;

pub(super) fn operation_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
        .expect("Git operation identity exhausted")
}

pub(super) enum GitTask {
    Read(GitRead),
    #[cfg(unix)]
    CommitPrepare(GitRead),
    IndexWrite {
        operation: u64,
        _task: Task<()>,
    },
}

impl GitTask {
    #[cfg(test)]
    pub fn operation(&self) -> u64 {
        match self {
            Self::Read(read) => read.operation,
            #[cfg(unix)]
            Self::CommitPrepare(read) => read.operation,
            Self::IndexWrite { operation, .. } => *operation,
        }
    }
}

pub(super) struct GitRead {
    pub operation: u64,
    pub cancelled: Arc<AtomicBool>,
    pub _task: Task<()>,
}
impl Drop for GitRead {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

impl AppShell {
    pub fn git_write_records(&self) -> impl Iterator<Item = &super::GitWriteRecord> {
        self.git_writes.iter()
    }

    pub(super) fn git_available(
        &mut self,
        origin: &Entity<BufferView>,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.exiting() || self.git_task.is_some() {
            self.show_message(origin, "A Git operation or exit is already running", cx);
            false
        } else {
            true
        }
    }

    pub(super) fn owns_git_read(&self, operation: u64) -> bool {
        match &self.git_task {
            Some(GitTask::Read(work)) => work.operation == operation,
            #[cfg(unix)]
            Some(GitTask::CommitPrepare(work)) => work.operation == operation,
            _ => false,
        }
    }

    pub(super) fn finish_git_read(
        &mut self,
        operation: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.owns_git_read(operation) {
            return false;
        }
        self.git_task = None;
        self.schedule_repository_refresh(window, cx);
        true
    }

    pub(super) fn owns_git_write(&self, operation: u64) -> bool {
        matches!(&self.git_task, Some(GitTask::IndexWrite { operation: active, .. }) if *active == operation)
    }

    pub(super) fn git_write_active(&self) -> bool {
        matches!(self.git_task, Some(GitTask::IndexWrite { .. }))
    }

    pub(super) fn mark_repository_stale(
        &mut self,
        repository: &crate::git::Repository,
        cx: &mut Context<Self>,
    ) {
        let mut changed = Vec::new();
        for slot in self.buffers.iter_mut() {
            if slot.buffer.repository().as_ref() == Some(repository) {
                slot.buffer.mark_git_stale();
                changed.push(slot.id());
            }
        }
        for buffer in changed {
            self.refresh_buffer(buffer, cx);
        }
        cx.notify();
    }
}
