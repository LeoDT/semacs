use super::AppShell;
use super::git::{GitRead as GitWork, operation_id};
use crate::buffer::Buffer;
use crate::command::Command;
use crate::diff::{Comparison, Diff, DiffKind, ReadEnvironment, Repository, Visit};
use crate::session::ReadKind;
use crate::view::BufferView;
use gpui_kit::{Context, Entity, Keystroke, Window};
use std::sync::{Arc, atomic::AtomicBool};

#[cfg(test)]
#[path = "diff_tests.rs"]
mod tests;

impl AppShell {
    pub(super) fn answer_diff_menu(
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
            ReadKind::DiffMenu => {
                let command = match stroke.key.as_str() {
                    "d" => Command::MagitDiffDwim,
                    "r" => Command::MagitDiffRange,
                    "u" => Command::MagitDiffUnstaged,
                    "s" => Command::MagitDiffStaged,
                    "w" => Command::MagitDiffWorkingTree,
                    "c" => Command::MagitShowCommit,
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
            ReadKind::DiffRefreshMenu => {
                let command = match stroke.key.as_str() {
                    "r" => Command::MagitDiffSwitchRangeType,
                    "f" => Command::MagitDiffFlipRevs,
                    "g" => Command::MagitRefresh,
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
            _ => false,
        }
    }

    pub(super) fn diff_command(
        &mut self,
        command: Command,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match command {
            Command::MagitDiff => {
                self.begin_read(ReadKind::DiffMenu, origin, "", window, cx);
            }
            Command::MagitDiffRefreshMenu => {
                self.begin_read(ReadKind::DiffRefreshMenu, origin, "", window, cx);
            }
            Command::MagitDiffDwim => self.diff_dwim(origin, window, cx),
            Command::MagitDiffRange => self.diff_range_command(origin, window, cx),
            Command::MagitDiffWorkingTree => {
                self.open_diff(DiffKind::Worktree, origin, window, cx);
            }
            Command::MagitDiffSwitchRangeType | Command::MagitDiffFlipRevs => {
                self.transform_diff_range(
                    command == Command::MagitDiffSwitchRangeType,
                    origin,
                    window,
                    cx,
                );
            }
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
            Command::MagitRefresh if origin.read(cx).buffer().history_query().is_some() => {
                self.refresh_history(origin, window, cx)
            }
            Command::MagitRefresh => self.refresh_diff(origin, window, cx),
            Command::MagitDiffVisit | Command::MagitDiffVisitOtherWindow => self.visit_diff(
                command == Command::MagitDiffVisitOtherWindow,
                origin,
                window,
                cx,
            ),
            Command::MagitDiffVisitWorktree => self.visit_diff_worktree(origin, window, cx),
            Command::MagitCopySectionValue => self.copy_section_value(origin, cx),
            Command::ScrollLeft | Command::ScrollRight => origin.update(cx, |view, cx| {
                view.scroll_horizontal(command == Command::ScrollLeft, cx)
            }),
            _ => {
                let mut buffer = origin.read(cx).buffer().new_view();
                let is_visibility_change = matches!(
                    command,
                    Command::SectionToggle
                        | Command::SectionCycle
                        | Command::SectionCycleDiffs
                        | Command::SectionCycleGlobal
                        | Command::SectionShowLevel1
                        | Command::SectionShowLevel2
                        | Command::SectionShowLevel3
                        | Command::SectionShowLevel4
                        | Command::SectionShowLevel1All
                        | Command::SectionShowLevel2All
                        | Command::SectionShowLevel3All
                        | Command::SectionShowLevel4All
                );
                let anchors: Vec<_> = if is_visibility_change {
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
                let status_mode = buffer.mode() == crate::mode::ModeId::MAGIT_STATUS;
                let result = match command {
                    Command::SectionToggle => buffer.toggle_section().map(|_| None),
                    Command::SectionCycle => buffer.cycle_section().map(|_| None),
                    Command::SectionCycleDiffs => buffer.cycle_sections_diffs(status_mode).map(|_| None),
                    Command::SectionCycleGlobal => buffer.cycle_sections_global().map(|_| None),
                    Command::SectionShowLevel1
                    | Command::SectionShowLevel2
                    | Command::SectionShowLevel3
                    | Command::SectionShowLevel4
                    | Command::SectionShowLevel1All
                    | Command::SectionShowLevel2All
                    | Command::SectionShowLevel3All
                    | Command::SectionShowLevel4All => {
                        let (level, all) = match command {
                            Command::SectionShowLevel1 => (1, false),
                            Command::SectionShowLevel2 => (2, false),
                            Command::SectionShowLevel3 => (3, false),
                            Command::SectionShowLevel4 => (4, false),
                            Command::SectionShowLevel1All => (1, true),
                            Command::SectionShowLevel2All => (2, true),
                            Command::SectionShowLevel3All => (3, true),
                            _ => (4, true),
                        };
                        buffer.show_section_level(level, all).map(Some)
                    }
                    Command::MagitDiffJump => buffer.diff_peer().map(Some),
                    _ => buffer
                        .sections()
                        .ok_or("No sections in this buffer".to_owned())
                        .and_then(|sections| {
                            let point = buffer.point();
                            let (target, err) = match command {
                                Command::SectionForward => {
                                    (sections.navigate(point, true), "No next section")
                                }
                                Command::SectionBackward => {
                                    (sections.navigate(point, false), "No previous section")
                                }
                                Command::SectionForwardSibling => (
                                    sections.navigate_sibling(point, true),
                                    "No next section",
                                ),
                                Command::SectionBackwardSibling => (
                                    sections.navigate_sibling(point, false),
                                    "No previous section",
                                ),
                                Command::SectionUp => {
                                    let current = sections.current(point);
                                    let parent = sections.nodes()[current]
                                        .parent
                                        .filter(|&p| {
                                            !(p == 0
                                                && sections.nodes()[current].range.start
                                                    == crate::text::ByteOffset(0))
                                        })
                                        .map(|parent| sections.nodes()[parent].range.start);
                                    (parent, "No parent section")
                                }
                                _ => (None, "No further section"),
                            };
                            target.map(Some).ok_or_else(|| err.into())
                        }),
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
                        // Every section movement command runs Magit's
                        // `magit-section-movement-hook`, including `^`; only a
                        // hunk heading can follow `magit-hunk-set-window-start`.
                        let hunk = if matches!(
                            command,
                            Command::SectionForward
                                | Command::SectionBackward
                                | Command::SectionForwardSibling
                                | Command::SectionBackwardSibling
                                | Command::SectionUp
                        ) {
                            origin
                                .read(cx)
                                .buffer()
                                .sections()
                                .and_then(|sections| sections.hunk_at(point))
                        } else {
                            None
                        };
                        origin.update(cx, |view, cx| match hunk {
                            Some(range) => view.show_hunk(range, window, cx),
                            None => view.show_location(point, window, cx),
                        });
                    }
                    Err(error) => self.show_message(origin, error, cx),
                }
            }
        }
    }

    fn diff_environment(origin: &Entity<BufferView>, cx: &gpui_kit::App) -> ReadEnvironment {
        origin
            .read(cx)
            .buffer()
            .history_query()
            .map(|query| query.environment())
            .or_else(|| {
                origin
                    .read(cx)
                    .buffer()
                    .comparison()
                    .map(|comparison| comparison.environment)
            })
            .unwrap_or(ReadEnvironment::Projection)
    }

    fn diff_open_current(
        &self,
        origin: &Entity<BufferView>,
        input_session: crate::input::SessionToken,
        cx: &gpui_kit::App,
    ) -> bool {
        !self.exiting()
            && self.minibuffer.is_none()
            && self.active_view() == origin
            && origin.read(cx).display_snapshot().id().session == input_session
            && self.buffer_index(origin, cx).is_some()
            && origin.read(cx).git_open_ready()
    }

    fn diff_dwim(
        &mut self,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some((older, newer)) = origin.read(cx).buffer().history_region_commits() {
            let Some(query) = origin.read(cx).buffer().history_query() else {
                return;
            };
            if self.open_diff_range(
                query.repository().clone(),
                query.environment(),
                format!("{older}..{newer}"),
                origin,
                window,
                cx,
            ) {
                let mut buffer = origin.read(cx).buffer().new_view();
                buffer.deactivate_mark();
                self.refresh_buffer(buffer.snapshot_id().buffer, cx);
            }
            return;
        }
        if let Some(query) = origin.read(cx).buffer().history_query() {
            match query {
                crate::history::Query::Revision {
                    repository,
                    oid,
                    environment,
                } => {
                    self.open_diff_range(
                        repository,
                        environment,
                        format!("{oid}^..{oid}"),
                        origin,
                        window,
                        cx,
                    );
                    return;
                }
                crate::history::Query::Log {
                    repository,
                    environment,
                    ..
                } => {
                    if let Some(oid) = origin.read(cx).buffer().history_commit_at_point() {
                        self.open_diff_range(
                            repository,
                            environment,
                            format!("{oid}^..{oid}"),
                            origin,
                            window,
                            cx,
                        );
                        return;
                    }
                }
            }
        }
        if let Some(repository) = origin.read(cx).buffer().status_repository() {
            match origin.read(cx).buffer().status_group_at_point() {
                Some(crate::status::Group::Unstaged) => {
                    self.open_diff(DiffKind::Unstaged, origin, window, cx);
                    return;
                }
                Some(crate::status::Group::Staged) => {
                    self.open_diff(DiffKind::Staged, origin, window, cx);
                    return;
                }
                _ => {}
            }
            match origin.read(cx).buffer().status_preview_target() {
                Some(crate::status::StatusPreviewTarget::Range(spec)) => {
                    self.open_diff_range(
                        repository,
                        ReadEnvironment::Projection,
                        spec,
                        origin,
                        window,
                        cx,
                    );
                    return;
                }
                Some(crate::status::StatusPreviewTarget::Commit(oid)) => {
                    self.open_diff_range(
                        repository,
                        ReadEnvironment::Projection,
                        format!("{oid}^..{oid}"),
                        origin,
                        window,
                        cx,
                    );
                    return;
                }
                Some(crate::status::StatusPreviewTarget::Rev(rev)) => {
                    self.open_diff_range(
                        repository,
                        ReadEnvironment::Projection,
                        rev,
                        origin,
                        window,
                        cx,
                    );
                    return;
                }
                None => {}
            }
        }
        if origin.read(cx).buffer().comparison().is_some() {
            self.refresh_diff(origin, window, cx);
            return;
        }
        self.prompt_diff_range(origin, window, cx);
    }

    fn diff_range_command(
        &mut self,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some((older, newer)) = origin.read(cx).buffer().history_region_commits()
            && let Some(query) = origin.read(cx).buffer().history_query()
        {
            if self.open_diff_range(
                query.repository().clone(),
                query.environment(),
                format!("{older}..{newer}"),
                origin,
                window,
                cx,
            ) {
                let mut buffer = origin.read(cx).buffer().new_view();
                buffer.deactivate_mark();
                self.refresh_buffer(buffer.snapshot_id().buffer, cx);
            }
            return;
        }
        self.prompt_diff_range(origin, window, cx);
    }

    fn prompt_diff_range(
        &mut self,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.git_available(origin, cx)
            || !origin.read(cx).git_open_ready()
            || self.minibuffer.is_some()
        {
            return;
        }
        let default_from_context = {
            let buffer = origin.read(cx).buffer();
            if let Some(oid) = buffer.history_commit_at_point() {
                Some(format!("{oid}^..{oid}"))
            } else if let Some(crate::history::Query::Revision { oid, .. }) = buffer.history_query()
            {
                Some(format!("{oid}^..{oid}"))
            } else if let Some(comparison) = buffer.comparison() {
                match comparison.kind {
                    DiffKind::Range { spec, .. } | DiffKind::RevisionToWorktree { spec, .. } => {
                        Some(spec)
                    }
                    _ => None,
                }
            } else {
                None
            }
        };
        let repository = origin.read(cx).buffer().repository();
        let environment = Self::diff_environment(origin, cx);
        let directory = self.default_directory_for(origin, cx);
        let input_session = origin.read(cx).display_snapshot().id().session;
        let origin = origin.clone();
        let operation = operation_id();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = cancelled.clone();
        let task = cx.spawn_in(window, async move |shell, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let repository = match repository {
                        Some(repository) => repository,
                        None => environment.discover(&directory, &worker_cancelled)?,
                    };
                    let refs = crate::git::list_refnames(
                        &repository,
                        true,
                        environment,
                        &worker_cancelled,
                    )?;
                    let default = match default_from_context {
                        Some(default) => Some(default),
                        None => {
                            crate::git::current_branch(&repository, environment, &worker_cancelled)?
                        }
                    };
                    Ok::<_, String>((repository, refs, default))
                })
                .await;
            shell
                .update_in(cx, |shell, window, cx| {
                    if !shell.finish_git_read(operation, window, cx)
                        || !shell.diff_open_current(&origin, input_session, cx)
                    {
                        return;
                    }
                    match result {
                        Ok((repository, refs, default)) => {
                            shell.begin_read(
                                ReadKind::MagitDiffRange {
                                    repository,
                                    environment,
                                    refs,
                                    default,
                                },
                                &origin,
                                "",
                                window,
                                cx,
                            );
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

    pub(super) fn confirm_diff_range(
        &mut self,
        id: u64,
        repository: Repository,
        environment: ReadEnvironment,
        default: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = self.session_input(cx).unwrap_or_default();
        let spec = input.trim();
        let spec = if spec.is_empty() {
            match default {
                Some(default) => default,
                None => {
                    self.message_minibuffer("No revision or range", cx);
                    return;
                }
            }
        } else {
            spec.to_owned()
        };
        if let Some(target) = self.end_session(id, window, cx) {
            self.open_diff_range(repository, environment, spec, &target, window, cx);
        }
    }

    pub(super) fn open_diff_range(
        &mut self,
        repository: Repository,
        environment: ReadEnvironment,
        spec: String,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.git_available(origin, cx)
            || self.minibuffer.is_some()
            || !origin.read(cx).git_open_ready()
        {
            return false;
        }
        let operation = operation_id();
        let input_session = origin.read(cx).display_snapshot().id().session;
        let origin = origin.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = cancelled.clone();
        let task = cx.spawn_in(window, async move |shell, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let kind = DiffKind::resolve_range(
                        &repository,
                        &spec,
                        environment,
                        &worker_cancelled,
                    )?;
                    let comparison = Comparison {
                        repository,
                        kind,
                        environment,
                    };
                    let diff = Diff::load(comparison, &worker_cancelled)?;
                    let rendered = diff.render()?;
                    Ok::<_, String>((diff, rendered))
                })
                .await;
            shell
                .update_in(cx, |shell, window, cx| {
                    if !shell.finish_git_read(operation, window, cx)
                        || !shell.diff_open_current(&origin, input_session, cx)
                    {
                        return;
                    }
                    match result {
                        Ok((data, rendered)) => {
                            let immutable = matches!(data.comparison.kind, DiffKind::Range { .. });
                            if !immutable
                                && !shell.git_read_current(operation, &data.comparison.repository)
                            {
                                shell.show_message(
                                    &origin,
                                    "Repository changed while reading diff; refresh again",
                                    cx,
                                );
                                return;
                            }
                            let existing = shell.buffers.iter().position(|slot| {
                                slot.buffer
                                    .comparison()
                                    .as_ref()
                                    .is_some_and(|current| current.same_buffer(&data.comparison))
                            });
                            let index = existing.unwrap_or_else(|| {
                                shell.create_buffer(
                                    Buffer::diff(data.comparison.clone()),
                                    window,
                                    cx,
                                )
                            });
                            let request = shell.buffers[index]
                                .buffer
                                .begin_diff_request_for(data.comparison.clone())
                                .unwrap();
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
        true
    }

    fn transform_diff_range(
        &mut self,
        switch_type: bool,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(mut comparison) = origin.read(cx).buffer().comparison() else {
            self.show_message(origin, "Not a diff buffer", cx);
            return;
        };
        let next_kind = if switch_type {
            comparison.kind.switch_range_type()
        } else {
            comparison.kind.flip_revs()
        };
        match next_kind {
            Ok(kind) => {
                comparison.kind = kind;
                self.refresh_diff_comparison(origin, comparison, window, cx);
            }
            Err(error) => self.show_message(origin, error, cx),
        }
    }

    pub(super) fn open_diff(
        &mut self,
        kind: DiffKind,
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
                    if !shell.finish_git_read(operation, window, cx)
                        || !shell.diff_open_current(&origin, input_session, cx)
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
                                slot.buffer
                                    .comparison()
                                    .as_ref()
                                    .is_some_and(|current| current.same_buffer(&data.comparison))
                            });
                            let index = existing.unwrap_or_else(|| {
                                shell.create_buffer(
                                    Buffer::diff(data.comparison.clone()),
                                    window,
                                    cx,
                                )
                            });
                            let request = shell.buffers[index]
                                .buffer
                                .begin_diff_request_for(data.comparison.clone())
                                .unwrap();
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
        let Some(comparison) = origin.read(cx).buffer().comparison() else {
            self.show_message(origin, "Not a diff buffer", cx);
            return;
        };
        self.refresh_diff_comparison(origin, comparison, window, cx);
    }

    fn refresh_diff_comparison(
        &mut self,
        origin: &Entity<BufferView>,
        comparison: Comparison,
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
            .begin_diff_request_for(comparison.clone())
        {
            Ok(request) => request,
            Err(error) => {
                self.show_message(origin, error, cx);
                return;
            }
        };
        let expected = comparison.clone();
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
                            if !shell.exiting()
                                && shell.buffers[index]
                                    .buffer
                                    .diff_request_matches(request, &expected)
                            {
                                shell.show_message(&origin, format!("Diff unchanged: {error}"), cx);
                            }
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
        let same_comparison =
            self.buffers[index].buffer.comparison().as_ref() == Some(&data.comparison);
        let views: Vec<_> = self
            .all_views()
            .into_iter()
            .filter(|view| view.read(cx).buffer().snapshot_id().buffer == id)
            .map(|view| {
                let anchor = same_comparison
                    .then(|| view.read(cx).section_scroll_anchor())
                    .flatten();
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
                view.update(cx, |view, cx| {
                    if same_comparison {
                        view.restore_section_scroll(anchor, cx);
                    } else {
                        view.reset_section_scroll(cx);
                    }
                });
            }
            true
        } else {
            false
        }
    }

    fn visit_diff(
        &mut self,
        other_window: bool,
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
        self.visit_git_location(visit, other_window, origin, window, cx);
    }

    fn visit_diff_worktree(
        &mut self,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let buffer = origin.read(cx).buffer();
        if buffer.status_repository().is_some() {
            self.visit_status_worktree(origin, window, cx);
            return;
        }
        let visit = if buffer.history_query().is_some() {
            buffer.history_visit_worktree()
        } else {
            buffer.diff_visit_worktree()
        };
        match visit {
            Ok(visit) => self.visit_git_location(visit, false, origin, window, cx),
            Err(error) => self.show_message(origin, error, cx),
        }
    }

    fn copy_section_value(&mut self, origin: &Entity<BufferView>, cx: &mut Context<Self>) {
        let value = {
            let buffer = origin.read(cx).buffer();
            buffer
                .status_copy_value()
                .or_else(|| buffer.diff_copy_value())
                .or_else(|| buffer.history_copy_value())
        };
        let Some(value) = value else {
            self.show_message(
                origin,
                "There is no thing at point that we know how to copy",
                cx,
            );
            return;
        };
        let ring = cx.default_global::<crate::editing::KillRing>().clone();
        let clipboard = crate::platform::GpuiClipboard::new(cx);
        ring.copy(&value, Some(&clipboard));
        self.show_message(origin, value, cx);
    }

    pub(super) fn visit_git_location(
        &mut self,
        visit: Visit,
        other_window: bool,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
                self.visit_file_with(
                    path,
                    origin,
                    super::documents::VisitOptions {
                        location: Some((line, column)),
                        other_window,
                        ..Default::default()
                    },
                    window,
                    cx,
                );
            }
            Visit::Blob {
                root,
                git_dir,
                oid,
                path,
                environment,
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
                            crate::git::blob(
                                &root,
                                &git_dir,
                                &blob_oid,
                                environment,
                                &worker_cancelled,
                            )
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
                                    let target_pane = other_window.then(|| {
                                        shell.other_window_pane(
                                            shell.buffers[index].id(),
                                            window,
                                            cx,
                                        )
                                    });
                                    let switched = match target_pane.flatten() {
                                        Some(pane) => {
                                            shell.show_buffer_in_pane(pane, index, true, window, cx)
                                        }
                                        None => shell.activate(index, window, cx),
                                    };
                                    if switched {
                                        shell.active_view().clone().update(cx, |view, cx| {
                                            view.show_line_column(line, column, window, cx)
                                        });
                                    }
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
