use std::sync::{Arc, atomic::AtomicBool};

use gpui_kit::{Context, Entity, Window};

use super::{
    AppShell,
    git::{GitRead, GitTask, operation_id},
};
use crate::buffer::Buffer;
use crate::command::Command;
use crate::history::{Action, Data, Query, RenderedHistory, Repository};
use crate::session::ReadKind;
use crate::view::BufferView;

#[cfg(test)]
#[path = "history_tests.rs"]
mod tests;

pub(super) enum Purpose {
    Browse,
    #[cfg(unix)]
    Select(super::commit_actions::Selection),
}

impl Purpose {
    fn selecting(&self) -> bool {
        match self {
            Self::Browse => false,
            #[cfg(unix)]
            Self::Select(_) => true,
        }
    }
}

impl AppShell {
    pub(super) fn history_command(
        &mut self,
        command: Command,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match command {
            Command::MagitLogCurrent => self.open_log(origin, None, Purpose::Browse, window, cx),
            Command::MagitLogMore | Command::MagitLogLess => {
                let Some(mut query) = origin.read(cx).buffer().history_query() else {
                    return;
                };
                if let Query::Log { limit, .. } = &mut query {
                    let next = if command == Command::MagitLogMore {
                        limit.checked_mul(2)
                    } else {
                        Some((*limit / 2).max(1))
                    };
                    let Some(next) = next else {
                        self.show_message(origin, "History limit is too large", cx);
                        return;
                    };
                    *limit = next;
                    self.refresh_history_query(origin, query, window, cx);
                }
            }
            Command::MagitVisitFileVersion => match origin.read(cx).buffer().history_action() {
                Ok(Action::File { versions, .. }) if !versions.is_empty() => {
                    self.begin_read(
                        ReadKind::HistoryFileVersion { versions },
                        origin,
                        "",
                        window,
                        cx,
                    );
                }
                _ => self.show_message(origin, "No supported file versions here", cx),
            },
            Command::MagitShowCommit => {
                if let Some(oid) = origin.read(cx).buffer().history_commit() {
                    self.show_history_commit(oid, origin, window, cx);
                } else {
                    self.show_message(origin, "No commit here", cx);
                }
            }
            Command::MagitVisitThing | Command::MagitDiffVisit => {
                match origin.read(cx).buffer().history_action() {
                    Ok(Action::Commit(oid)) if command == Command::MagitVisitThing => {
                        self.show_history_commit(oid, origin, window, cx)
                    }
                    Ok(Action::Commit(_)) => self.show_message(origin, "No file version here", cx),
                    Ok(Action::File {
                        default: Some(visit),
                        ..
                    }) => self.visit_git_location(visit, origin, window, cx),
                    Ok(Action::File { default: None, .. }) => self.show_message(
                        origin,
                        "No supported file version exists on this side",
                        cx,
                    ),
                    Err(error) => self.show_message(origin, error, cx),
                }
            }
            _ => {}
        }
    }

    fn show_history_commit(
        &mut self,
        oid: String,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(query) = origin.read(cx).buffer().history_query() else {
            return;
        };
        self.open_revision(
            Query::Revision {
                repository: query.repository().clone(),
                oid,
                environment: query.environment(),
            },
            origin,
            window,
            cx,
        );
    }

    pub(super) fn open_log(
        &mut self,
        origin: &Entity<BufferView>,
        repository: Option<Repository>,
        purpose: Purpose,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.git_available(origin, cx)
            || !origin.read(cx).git_open_ready()
            || self.minibuffer.is_some()
        {
            return;
        }
        let repository = repository.or_else(|| origin.read(cx).buffer().repository());
        let directory = self.default_directory_for(origin, cx);
        let source = origin.read(cx).display_snapshot().id();
        let origin = origin.clone();
        let operation = operation_id();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = cancelled.clone();
        let selecting = purpose.selecting();
        let task = cx.spawn_in(window, async move |shell, cx| {
            let query = cx
                .background_executor()
                .spawn(async move {
                    let repository = match repository {
                        Some(repository) => repository,
                        None => crate::git::discover(&directory, &worker_cancelled)?,
                    };
                    Query::current(repository, selecting, &worker_cancelled)
                })
                .await;
            shell
                .update_in(cx, |shell, window, cx| {
                    if !shell.finish_git_read(operation, window, cx)
                        || !shell.history_open_current(&origin, source, cx)
                    {
                        return;
                    }
                    match query {
                        Ok(query) => shell.open_history_query(query, &origin, purpose, window, cx),
                        Err(error) => shell.show_message(&origin, error, cx),
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

    fn history_open_current(
        &self,
        origin: &Entity<BufferView>,
        source: crate::input::DisplayId,
        cx: &gpui_kit::App,
    ) -> bool {
        !self.exiting()
            && self.minibuffer.is_none()
            && self.active_view() == origin
            && self.buffer_index(origin, cx).is_some()
            && origin.read(cx).display_snapshot().id() == source
            && origin.read(cx).git_open_ready()
    }

    fn open_revision(
        &mut self,
        query: Query,
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
        self.open_history_query(query, origin, Purpose::Browse, window, cx);
    }

    fn open_history_query(
        &mut self,
        query: Query,
        origin: &Entity<BufferView>,
        purpose: Purpose,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let existing = self.buffers.iter().position(|slot| {
            slot.buffer
                .history_query()
                .is_some_and(|current| current.same_buffer(&query))
        });
        let index = existing
            .unwrap_or_else(|| self.create_buffer(Buffer::history(query.clone()), window, cx));
        let fresh = existing.is_none();
        let id = self.buffers[index].id();
        let request = self.buffers[index]
            .buffer
            .begin_history_request(query.clone())
            .unwrap();
        let source = origin.read(cx).display_snapshot().id();
        let origin = origin.clone();
        #[cfg(test)]
        let gate = self.history_read_gate.take();
        let worker_query = query.clone();
        let operation = operation_id();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = cancelled.clone();
        let task = cx.spawn_in(window, async move |shell, cx| {
            #[cfg(test)]
            if let Some(gate) = gate {
                let _ = gate.await;
            }
            let result = cx
                .background_executor()
                .spawn(async move {
                    let data = worker_query.load(&worker_cancelled)?;
                    let rendered = data.render(&worker_query)?;
                    Ok::<_, String>((data, rendered))
                })
                .await;
            shell
                .update_in(cx, |shell, window, cx| {
                    if !shell.finish_git_read(operation, window, cx) {
                        return;
                    }
                    let Some(index) = shell.buffers.index_of(id) else {
                        return;
                    };
                    if !shell.history_open_current(&origin, source, cx) {
                        return;
                    }
                    match result {
                        Ok((data, rendered)) => {
                            if !shell.history_read_current(operation, &query)
                                || !shell.install_history_result(
                                    index, &query, request, data, rendered, cx,
                                )
                            {
                                return;
                            }
                            shell.display_temporary(index, window, cx);
                            #[cfg(unix)]
                            if let Purpose::Select(selection) = purpose {
                                let initial = selection.initial.clone();
                                shell.discard_commit_selection(id, window, cx);
                                shell.commit_selections.insert(id, selection);
                                shell.show_selection_diff(id, window, cx);
                                if let Some(point) = shell.buffers[index]
                                    .buffer
                                    .history_commit_point(initial.as_deref(), true)
                                {
                                    shell.active_view().clone().update(cx, |view, cx| {
                                        view.show_location(point, window, cx)
                                    });
                                }
                                return;
                            }
                            if fresh
                                && let Some(point) = shell.buffers[index]
                                    .buffer
                                    .history_commit_point(None, false)
                            {
                                shell
                                    .active_view()
                                    .clone()
                                    .update(cx, |view, cx| view.show_location(point, window, cx));
                            }
                        }
                        Err(error) => shell.show_message(&origin, error, cx),
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

    pub(super) fn refresh_history(
        &mut self,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(query) = origin.read(cx).buffer().history_query() {
            self.refresh_history_query(origin, query, window, cx);
        }
    }

    fn refresh_history_query(
        &mut self,
        origin: &Entity<BufferView>,
        query: Query,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.git_available(origin, cx) {
            return;
        }
        let Some(index) = self.buffer_index(origin, cx) else {
            return;
        };
        let request = match self.buffers[index]
            .buffer
            .begin_history_request(query.clone())
        {
            Ok(request) => request,
            Err(error) => {
                self.show_message(origin, error, cx);
                return;
            }
        };
        let id = self.buffers[index].id();
        let origin = origin.clone();
        #[cfg(test)]
        let gate = self.history_read_gate.take();
        let worker_query = query.clone();
        let operation = operation_id();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = cancelled.clone();
        let task = cx.spawn_in(window, async move |shell, cx| {
            #[cfg(test)]
            if let Some(gate) = gate {
                let _ = gate.await;
            }
            let result = cx
                .background_executor()
                .spawn(async move {
                    let data = worker_query.load(&worker_cancelled)?;
                    let rendered = data.render(&worker_query)?;
                    Ok::<_, String>((data, rendered))
                })
                .await;
            shell
                .update_in(cx, |shell, window, cx| {
                    if !shell.finish_git_read(operation, window, cx) {
                        return;
                    }
                    let Some(index) = shell.buffers.index_of(id) else {
                        return;
                    };
                    match result {
                        Ok((data, rendered)) => {
                            if !shell.history_read_current(operation, &query) {
                                return;
                            }
                            shell
                                .install_history_result(index, &query, request, data, rendered, cx);
                        }
                        Err(error) => {
                            shell.show_message(&origin, format!("History unchanged: {error}"), cx)
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

    fn history_read_current(&self, operation: u64, query: &Query) -> bool {
        matches!(query, Query::Revision { .. })
            || self.git_read_current(operation, query.repository())
    }

    pub(super) fn install_history_result(
        &mut self,
        index: usize,
        query: &Query,
        request: u64,
        data: Data,
        rendered: RenderedHistory,
        cx: &mut Context<Self>,
    ) -> bool {
        let id = self.buffers[index].id();
        let anchors = self
            .all_views()
            .into_iter()
            .filter(|view| view.read(cx).buffer().snapshot_id().buffer == id)
            .map(|view| {
                let anchor = view.read(cx).section_scroll_anchor();
                (view, anchor)
            })
            .collect::<Vec<_>>();
        if !self.buffers[index]
            .buffer
            .install_history(query, request, data, rendered)
        {
            return false;
        }
        self.refresh_buffer(id, cx);
        for (view, anchor) in anchors {
            view.update(cx, |view, cx| view.restore_section_scroll(anchor, cx));
        }
        true
    }
}
