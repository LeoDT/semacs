use super::{
    AppShell,
    git::{GitRead, operation_id},
};
use crate::buffer::{Buffer, BufferId};
use crate::input::SessionToken;
use crate::status::{RenderedStatus, Status, StatusAction};
use crate::view::BufferView;
use gpui_kit::{Context, Entity, Window};
use std::sync::{Arc, atomic::AtomicBool};

#[cfg(test)]
#[path = "status_hunk_tests.rs"]
mod hunk_tests;
#[cfg(test)]
#[path = "status_tests.rs"]
mod tests;

enum OpenTarget {
    Existing { buffer: BufferId, request: u64 },
    New,
}

impl AppShell {
    fn status_open_valid(
        &self,
        origin: &Entity<BufferView>,
        session: SessionToken,
        cx: &gpui_kit::App,
    ) -> bool {
        self.minibuffer.is_none()
            && self.active_view() == origin
            && self.buffer_index(origin, cx).is_some()
            && origin.read(cx).display_snapshot().id().session == session
            && origin.read(cx).git_open_ready()
    }

    pub(super) fn open_status(
        &mut self,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_status_in(origin, None, window, cx);
    }

    pub(super) fn open_status_in(
        &mut self,
        origin: &Entity<BufferView>,
        directory_override: Option<std::path::PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.git_available(origin, cx) || !origin.read(cx).git_open_ready() {
            return;
        }
        let (repository, directory) = match directory_override {
            Some(directory) => (None, directory),
            None => (
                origin.read(cx).buffer().repository(),
                self.default_directory_for(origin, cx),
            ),
        };
        let session = origin.read(cx).display_snapshot().id().session;
        let operation = operation_id();
        let origin = origin.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = cancelled.clone();
        let task = cx.spawn_in(window, async move |shell, cx| {
            let repository = match repository {
                Some(repository) => Ok(repository),
                None => {
                    let cancel = worker_cancelled.clone();
                    cx.background_executor()
                        .spawn(async move { crate::git::discover(&directory, &cancel) })
                        .await
                }
            };
            let bound = shell
                .update_in(cx, |shell, _, cx| {
                    if !shell.owns_git_read(operation)
                        || !shell.status_open_valid(&origin, session, cx)
                    {
                        return None;
                    }
                    let repository = match repository {
                        Ok(repository) => repository,
                        Err(error) => {
                            shell.show_message(&origin, error, cx);
                            return None;
                        }
                    };
                    let existing = shell.buffers.iter().position(|slot| {
                        slot.buffer.status_repository().as_ref() == Some(&repository)
                    });
                    let target = if let Some(index) = existing {
                        let (_, request) =
                            shell.buffers[index].buffer.begin_status_request().unwrap();
                        OpenTarget::Existing {
                            buffer: shell.buffers[index].id(),
                            request,
                        }
                    } else {
                        OpenTarget::New
                    };
                    Some((repository, target))
                })
                .ok()
                .flatten();
            let Some((repository, target)) = bound else {
                shell
                    .update_in(cx, |shell, window, cx| {
                        shell.finish_git_read(operation, window, cx);
                    })
                    .ok();
                return;
            };
            #[cfg(test)]
            if let Some(gate) = shell
                .update_in(cx, |shell, _, _| shell.status_read_gate.take())
                .ok()
                .flatten()
            {
                let _ = gate.await;
            }
            let loaded_repository = repository.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    let data = Status::load(loaded_repository, &worker_cancelled)?;
                    let rendered = data.render()?;
                    Ok::<_, String>((data, rendered))
                })
                .await;
            shell
                .update_in(cx, |shell, window, cx| {
                    if !shell.finish_git_read(operation, window, cx)
                        || !shell.status_open_valid(&origin, session, cx)
                    {
                        return;
                    }
                    if !shell.git_read_current(operation, &repository) {
                        shell.show_message(
                            &origin,
                            "Repository changed while reading status; refresh again",
                            cx,
                        );
                        return;
                    }
                    let existing = match target {
                        OpenTarget::Existing { buffer, request } => {
                            let Some(index) = shell.buffers.index_of(buffer) else {
                                return;
                            };
                            if !shell.buffers[index]
                                .buffer
                                .status_request_matches(request, &repository)
                            {
                                return;
                            }
                            Some((index, request))
                        }
                        OpenTarget::New => {
                            if shell.buffers.iter().any(|slot| {
                                slot.buffer.status_repository().as_ref() == Some(&repository)
                            }) {
                                return;
                            }
                            None
                        }
                    };
                    match result {
                        Ok((data, rendered)) => {
                            let (index, request) = existing.unwrap_or_else(|| {
                                let index =
                                    shell.create_buffer(Buffer::status(repository), window, cx);
                                let (_, request) =
                                    shell.buffers[index].buffer.begin_status_request().unwrap();
                                (index, request)
                            });
                            if shell.install_status_result(index, request, data, rendered, cx) {
                                shell.display_temporary(index, window, cx);
                            }
                        }
                        Err(error) => {
                            shell.show_message(&origin, format!("Status unchanged: {error}"), cx)
                        }
                    }
                })
                .ok();
        });
        self.git_task = Some(super::git::GitTask::Read(GitRead {
            operation,
            cancelled,
            _task: task,
        }));
    }

    pub(super) fn refresh_status(
        &mut self,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.git_available(origin, cx) {
            return;
        }
        let Some(index) = self.buffer_index(origin, cx) else {
            return;
        };
        let Ok((repository, request)) = self.buffers[index].buffer.begin_status_request() else {
            return;
        };
        let buffer = self.buffers[index].id();
        let operation = operation_id();
        let origin = origin.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = cancelled.clone();
        let task = cx.spawn_in(window, async move |shell, cx| {
            #[cfg(test)]
            if let Some(gate) = shell
                .update_in(cx, |shell, _, _| shell.status_read_gate.take())
                .ok()
                .flatten()
            {
                let _ = gate.await;
            }
            let expected = repository.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    let data = Status::load(repository, &worker_cancelled)?;
                    let rendered = data.render()?;
                    Ok::<_, String>((data, rendered))
                })
                .await;
            shell
                .update_in(cx, |shell, window, cx| {
                    if !shell.finish_git_read(operation, window, cx) {
                        return;
                    }
                    let Some(index) = shell.buffers.index_of(buffer) else {
                        return;
                    };
                    if !shell.buffers[index]
                        .buffer
                        .status_request_matches(request, &expected)
                    {
                        return;
                    }
                    match result {
                        Ok((data, rendered)) => {
                            shell.install_status_result(index, request, data, rendered, cx);
                        }
                        Err(error) => {
                            shell.show_message(&origin, format!("Status unchanged: {error}"), cx)
                        }
                    }
                })
                .ok();
        });
        self.git_task = Some(super::git::GitTask::Read(GitRead {
            operation,
            cancelled,
            _task: task,
        }));
    }

    pub(super) fn install_status_result(
        &mut self,
        index: usize,
        request: u64,
        data: Status,
        rendered: RenderedStatus,
        cx: &mut Context<Self>,
    ) -> bool {
        let buffer = self.buffers[index].id();
        let views: Vec<_> = self
            .all_views()
            .into_iter()
            .filter(|view| view.read(cx).buffer().snapshot_id().buffer == buffer)
            .map(|view| {
                let anchor = view.read(cx).section_scroll_anchor();
                (view, anchor)
            })
            .collect();
        if !self.buffers[index]
            .buffer
            .install_status(request, data, rendered)
        {
            return false;
        }
        self.refresh_buffer(buffer, cx);
        for (view, anchor) in views {
            view.update(cx, |view, cx| view.restore_section_scroll(anchor, cx));
        }
        true
    }

    pub(super) fn answer_status_jump_menu(
        &mut self,
        stroke: &gpui_kit::Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(session) = &self.session else {
            return false;
        };
        let id = session.id();
        let crate::session::ReadKind::StatusJumpMenu { prefix } = session.kind().clone() else {
            return false;
        };
        let key = stroke.key.as_str();
        if prefix.is_none() && matches!(key, "p" | "f") {
            let next = key.chars().next();
            if let Some(origin) = self.end_session(id, window, cx) {
                self.begin_read(
                    crate::session::ReadKind::StatusJumpMenu { prefix: next },
                    &origin,
                    "",
                    window,
                    cx,
                );
            }
            return true;
        }
        let target: Option<(&'static str, &'static [&'static str])> = match (prefix, key) {
            (None, "n") => Some(("Untracked files", &["untracked"])),
            (None, "u") => Some(("Unstaged changes", &["unstaged"])),
            (None, "s") => Some(("Staged changes", &["staged"])),
            (None, "m") => Some(("Unmerged paths", &["unmerged"])),
            (None, "z") => Some(("Stashes", &["stashes"])),
            (Some('p'), "u") => Some(("Unpushed to @{upstream}", &["unpushed-upstream", "recent"])),
            (Some('p'), "p") => Some(("Unpushed to pushremote", &["unpushed-pushremote"])),
            (Some('f'), "u") => Some(("Unpulled from @{upstream}", &["unpulled-upstream"])),
            (Some('f'), "p") => Some(("Unpulled from pushremote", &["unpulled-pushremote"])),
            _ => None,
        };
        let Some((label, keys)) = target else {
            self.message_minibuffer(
                crate::session::ReadKind::StatusJumpMenu { prefix }.answer_hint(),
                cx,
            );
            return true;
        };
        if let Some(origin) = self.end_session(id, window, cx) {
            let point = origin.read(cx).buffer().sections().and_then(|sections| {
                keys.iter().find_map(|candidate| sections.find_key(candidate))
            });
            match point {
                Some(point) => origin.update(cx, |view, cx| view.show_location(point, window, cx)),
                None => self.show_message(&origin, format!("Section \"{label}\" wasn't found"), cx),
            }
        }
        true
    }

    pub(super) fn visit_status(
        &mut self,
        thing: bool,
        other_window: bool,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let action = origin.read(cx).buffer().status_visit(thing);
        self.apply_status_action(action, other_window, origin, window, cx);
    }

    pub(super) fn visit_status_worktree(
        &mut self,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let action = origin.read(cx).buffer().status_visit_worktree();
        self.apply_status_action(action, false, origin, window, cx);
    }

    fn apply_status_action(
        &mut self,
        action: Result<StatusAction, String>,
        other_window: bool,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            Ok(StatusAction::Diff(kind)) => self.open_diff(kind, origin, window, cx),
            Ok(StatusAction::DiffRange(spec)) => {
                if let Some(repository) = origin.read(cx).buffer().status_repository() {
                    self.open_diff_range(
                        repository,
                        crate::diff::ReadEnvironment::Projection,
                        spec,
                        origin,
                        window,
                        cx,
                    );
                }
            }
            Ok(StatusAction::Commit(oid)) => {
                if let Some(repository) = origin.read(cx).buffer().status_repository() {
                    self.open_revision(
                        crate::history::Query::Revision {
                            repository,
                            oid,
                            environment: crate::git::ReadEnvironment::Projection,
                        },
                        origin,
                        window,
                        cx,
                    );
                }
            }
            Ok(StatusAction::Rev(rev)) => {
                if let Some(repository) = origin.read(cx).buffer().status_repository() {
                    self.resolve_and_open_revision(
                        repository,
                        crate::git::ReadEnvironment::Projection,
                        rev,
                        origin,
                        window,
                        cx,
                    );
                }
            }
            Ok(StatusAction::Visit(visit)) => {
                self.visit_git_location(visit, other_window, origin, window, cx)
            }
            Ok(StatusAction::WorktreeFile { path, location }) => {
                if !path.is_file() {
                    self.show_message(
                        origin,
                        "Worktree file is missing or is not a regular file",
                        cx,
                    );
                    return;
                }
                self.visit_file_with(
                    path,
                    origin,
                    super::documents::VisitOptions {
                        location,
                        other_window,
                        ..Default::default()
                    },
                    window,
                    cx,
                );
            }
            Ok(StatusAction::Submodule(path)) => {
                if !path.is_dir() {
                    self.show_message(origin, "Submodule directory is missing or inaccessible", cx);
                    return;
                }
                self.open_status_in(origin, Some(path), window, cx);
            }
            Ok(StatusAction::Directory(path)) => {
                if std::fs::symlink_metadata(path.join(".git")).is_ok() {
                    self.open_status_in(origin, Some(path), window, cx);
                } else {
                    self.show_message(
                        origin,
                        "Cannot visit directory (dired is not available)",
                        cx,
                    );
                }
            }
            Err(error) => self.show_message(origin, error, cx),
        }
    }
}
