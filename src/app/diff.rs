use super::AppShell;
use super::git::{GitRead as GitWork, operation_id};
use crate::buffer::Buffer;
use crate::command::Command;
use crate::diff::{Comparison, Diff, DiffKind, Visit};
use crate::view::BufferView;
use gpui_kit::{Context, Entity, Window};
use std::sync::{Arc, atomic::AtomicBool};

#[cfg(test)]
#[path = "diff_tests.rs"]
mod tests;

impl AppShell {
    pub(super) fn diff_command(
        &mut self,
        command: Command,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match command {
            Command::MagitDiffStaged | Command::MagitDiffUnstaged => self.open_diff(
                if command == Command::MagitDiffStaged {
                    DiffKind::Staged
                } else {
                    DiffKind::Unstaged
                },
                origin,
                window,
                cx,
            ),
            Command::MagitRefresh if origin.read(cx).buffer().status_repository().is_some() => {
                self.refresh_status(origin, window, cx)
            }
            Command::MagitRefresh => self.refresh_diff(origin, window, cx),
            Command::MagitDiffVisit => self.visit_diff(origin, window, cx),
            Command::ScrollLeft | Command::ScrollRight => origin.update(cx, |view, cx| {
                view.scroll_horizontal(command == Command::ScrollLeft, cx)
            }),
            _ => {
                let mut buffer = origin.read(cx).buffer().new_view();
                let anchors: Vec<_> = if command == Command::SectionToggle {
                    let id = buffer.snapshot_id().buffer;
                    self.all_views()
                        .into_iter()
                        .filter(|view| view.read(cx).buffer().snapshot_id().buffer == id)
                        .map(|view| {
                            let anchor = view.read(cx).section_scroll_anchor();
                            (view, anchor)
                        })
                        .collect()
                } else {
                    Vec::new()
                };
                let result = if command == Command::SectionToggle {
                    buffer.toggle_section().map(|_| None)
                } else if command == Command::MagitDiffJump {
                    buffer.diff_peer().map(Some)
                } else {
                    buffer
                        .sections()
                        .ok_or("No sections in this buffer".to_owned())
                        .and_then(|sections| {
                            let point = buffer.point();
                            let target = match command {
                                Command::SectionForward => sections.navigate(point, true),
                                Command::SectionBackward => sections.navigate(point, false),
                                Command::SectionUp => sections.nodes()[sections.current(point)]
                                    .parent
                                    .map(|parent| sections.nodes()[parent].range.start),
                                _ => None,
                            };
                            target.map(Some).ok_or("No further section".into())
                        })
                };
                match result {
                    Ok(point) => {
                        if command == Command::MagitDiffJump
                            && let Some(point) = point
                        {
                            buffer.reveal_section(point);
                        }
                        self.refresh_buffer(buffer.snapshot_id().buffer, cx);
                        for (view, anchor) in anchors {
                            view.update(cx, |view, cx| view.restore_section_scroll(anchor, cx));
                        }
                        let point = point.unwrap_or_else(|| origin.read(cx).buffer().point());
                        origin.update(cx, |view, cx| view.show_location(point, window, cx));
                    }
                    Err(error) => self.show_message(origin, error, cx),
                }
            }
        }
    }

    pub(super) fn open_diff(
        &mut self,
        kind: DiffKind,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.git_available(origin, cx) || !origin.read(cx).git_open_ready() {
            return;
        }
        let repository = origin.read(cx).buffer().repository();
        let operation = operation_id();
        let directory = self.default_directory_for(origin, cx);
        let input_session = origin.read(cx).display_snapshot().id().session;
        let origin = origin.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = cancelled.clone();
        #[cfg(feature = "native-profiler")]
        let trace = crate::preparation_trace::Context::worker("git-worker");
        let task = cx.spawn_in(window, async move |shell, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    #[cfg(feature = "native-profiler")]
                    let _recording = trace.map(crate::preparation_trace::Context::enter);
                    let repository = match repository {
                        Some(repository) => repository,
                        None => crate::git::discover(&directory, &worker_cancelled)?,
                    };
                    let comparison = Comparison {
                        repository,
                        kind,
                        environment: crate::diff::ReadEnvironment::Projection,
                    };
                    let diff = Diff::load(comparison, &worker_cancelled)?;
                    let rendered = diff.render()?;
                    Ok::<_, String>((diff, rendered))
                })
                .await;
            shell
                .update_in(cx, |shell, window, cx| {
                    if !shell.finish_git_read(operation, window, cx) {
                        return;
                    }
                    if shell.minibuffer.is_some()
                        || shell.active_view() != &origin
                        || origin.read(cx).display_snapshot().id().session != input_session
                        || shell.buffer_index(&origin, cx).is_none()
                        || !origin.read(cx).git_open_ready()
                    {
                        return;
                    }
                    match result {
                        Ok((data, rendered)) => {
                            if !shell.git_read_current(operation, &data.comparison.repository) {
                                shell.show_message(
                                    &origin,
                                    "Repository changed while reading diff; refresh again",
                                    cx,
                                );
                                return;
                            }
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
                        }
                        Err(error) => shell.show_message(&origin, error, cx),
                    }
                })
                .ok();
        });
        self.git_task = Some(super::git::GitTask::Read(GitWork {
            operation,
            cancelled,
            _task: task,
        }));
    }

    fn refresh_diff(
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
        let (comparison, request) = match self.buffers[index].buffer.begin_diff_request() {
            Ok(request) => request,
            Err(error) => {
                self.show_message(origin, error, cx);
                return;
            }
        };
        let id = self.buffers[index].id();
        let operation = operation_id();
        let origin = origin.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = cancelled.clone();
        #[cfg(feature = "native-profiler")]
        let trace = crate::preparation_trace::Context::worker("git-worker");
        let task = cx.spawn_in(window, async move |shell, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    #[cfg(feature = "native-profiler")]
                    let _recording = trace.map(crate::preparation_trace::Context::enter);
                    let data = Diff::load(comparison, &worker_cancelled)?;
                    let rendered = data.render()?;
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
                            shell.install_diff_result(index, request, data, rendered, cx);
                        }
                        Err(error) => {
                            shell.show_message(&origin, format!("Diff unchanged: {error}"), cx)
                        }
                    }
                })
                .ok();
        });
        self.git_task = Some(super::git::GitTask::Read(GitWork {
            operation,
            cancelled,
            _task: task,
        }));
    }

    pub(super) fn install_diff_result(
        &mut self,
        index: usize,
        request: u64,
        data: Diff,
        rendered: crate::diff::RenderedDiff,
        cx: &mut Context<Self>,
    ) -> bool {
        #[cfg(feature = "native-profiler")]
        let _span =
            crate::preparation_trace::Span::new("diff-install-and-invalidate", rendered.text.len());
        let id = self.buffers[index].id();
        let views: Vec<_> = self
            .all_views()
            .into_iter()
            .filter(|view| view.read(cx).buffer().snapshot_id().buffer == id)
            .map(|view| {
                let anchor = view.read(cx).section_scroll_anchor();
                (view, anchor)
            })
            .collect();
        if self.buffers[index]
            .buffer
            .install_diff(request, data, rendered)
            .unwrap_or(false)
        {
            self.refresh_buffer(id, cx);
            for (view, anchor) in views {
                view.update(cx, |view, cx| view.restore_section_scroll(anchor, cx));
            }
            true
        } else {
            false
        }
    }

    fn visit_diff(
        &mut self,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let visit = match origin.read(cx).buffer().diff_visit() {
            Ok(visit) => visit,
            Err(error) => {
                self.show_message(origin, error, cx);
                return;
            }
        };
        match visit {
            Visit::Worktree { path, line, column } => {
                if !path.is_file() {
                    self.show_message(
                        origin,
                        "Worktree file is missing or is not a regular file",
                        cx,
                    );
                    return;
                }
                self.visit_file_at(path, origin, Some((line, column)), window, cx);
            }
            Visit::Blob {
                root,
                git_dir,
                oid,
                path,
                line,
                column,
            } => {
                if !self.git_available(origin, cx) {
                    return;
                }
                let origin = origin.clone();
                let source = origin.read(cx).display_snapshot().id();
                let operation = operation_id();
                let cancelled = Arc::new(AtomicBool::new(false));
                let worker_cancelled = cancelled.clone();
                let task = cx.spawn_in(window, async move |shell, cx| {
                    let blob_oid = oid.clone();
                    let result = cx
                        .background_executor()
                        .spawn(async move {
                            crate::git::blob(&root, &git_dir, &blob_oid, &worker_cancelled)
                        })
                        .await;
                    shell
                        .update_in(cx, |shell, window, cx| {
                            if !shell.finish_git_read(operation, window, cx) {
                                return;
                            }
                            if shell.minibuffer.is_some()
                                || shell.active_view() != &origin
                                || origin.read(cx).display_snapshot().id() != source
                                || shell.buffer_index(&origin, cx).is_none()
                            {
                                return;
                            }
                            match result {
                                Ok(text) => {
                                    let name = format!(
                                        "*git {}:{}*",
                                        &oid[..12],
                                        path.to_string_lossy().escape_debug()
                                    );
                                    let buffer = Buffer::version_snapshot(&name, &text);
                                    let index = shell.create_buffer(buffer, window, cx);
                                    shell.activate(index, window, cx);
                                    shell.active_view().clone().update(cx, |view, cx| {
                                        view.show_line_column(line, column, window, cx)
                                    });
                                }
                                Err(error) => shell.show_message(&origin, error, cx),
                            }
                        })
                        .ok();
                });
                self.git_task = Some(super::git::GitTask::Read(GitWork {
                    operation,
                    cancelled,
                    _task: task,
                }));
            }
        }
    }
}
