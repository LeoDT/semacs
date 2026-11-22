use std::path::Path;
use std::sync::{Arc, Mutex};

use gpui_kit::{Context, Entity, Task, Window};

use super::AppShell;
use super::documents::VisitOptions;
use crate::buffer::{Buffer, BufferId};
use crate::command::Command;
use crate::files;
use crate::panes::Axis;
use crate::project::Project;
use crate::search::{
    self, MatchLocation, SearchController, SearchData, SearchQuery, SearchSignal, SearchUpdate,
};
use crate::session::{ProjectAction, ReadKind, SearchReadTarget};
use crate::view::BufferView;

#[cfg(test)]
#[path = "search_tests.rs"]
mod tests;

pub(super) struct SearchWork {
    pub(super) buffer: BufferId,
    pub(super) request: u64,
    controller: Arc<SearchController>,
    _task: Task<()>,
}

impl Drop for SearchWork {
    fn drop(&mut self) {
        self.controller.cancel();
    }
}

impl AppShell {
    pub(super) fn search_command(
        &mut self,
        command: Command,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.minibuffer.is_some() {
            return;
        }
        match command {
            Command::ProjectFindRegexp | Command::RgProject => {
                self.with_current_project(ProjectAction::FindRegexp, origin, window, cx);
            }
            Command::Rg => {
                let default = origin.read(cx).buffer().symbol_at_point();
                let prompt = match &default {
                    Some(sym) => format!("Search for (default {sym}): "),
                    None => "Search for: ".to_owned(),
                };
                self.begin_read(
                    ReadKind::SearchPattern {
                        target: SearchReadTarget::Directory,
                        default,
                        prompt,
                    },
                    origin,
                    "",
                    window,
                    cx,
                );
            }
            Command::NextError => self.global_step_error(true, origin, window, cx),
            Command::PreviousError => self.global_step_error(false, origin, window, cx),
            Command::CompileGotoError => self.search_goto_at_point(true, origin, window, cx),
            Command::CompilationDisplayError => {
                self.search_goto_at_point(false, origin, window, cx)
            }
            Command::NextErrorNoSelect => self.search_step_local(true, true, origin, window, cx),
            Command::PreviousErrorNoSelect => {
                self.search_step_local(false, true, origin, window, cx)
            }
            Command::CompilationNextError => {
                self.search_step_local(true, false, origin, window, cx)
            }
            Command::CompilationPreviousError => {
                self.search_step_local(false, false, origin, window, cx)
            }
            Command::CompilationNextFile => self.search_step_file(true, origin, window, cx),
            Command::CompilationPreviousFile => self.search_step_file(false, origin, window, cx),
            Command::Recompile => self.search_recompile(origin, window, cx),
            Command::RgRerunChangeRegexp => {
                self.begin_search_rerun_pattern(false, origin, window, cx)
            }
            Command::RgRerunChangeLiteral => {
                self.begin_search_rerun_pattern(true, origin, window, cx)
            }
            Command::RgRerunChangeDir => self.begin_search_rerun_dir(origin, window, cx),
            Command::RgRerunChangeFiles => self.begin_search_rerun_files(origin, window, cx),
            Command::RgRerunToggleCase => self.search_toggle_case(origin, window, cx),
            Command::RgRerunToggleIgnore => self.search_toggle_ignore(origin, window, cx),
            Command::KillCompilation => self.search_kill_running(origin, cx),
            Command::QuitWindow => self.quit_temporary(window, cx),
            _ => debug_assert!(false, "{} is not a search command", command.name()),
        }
    }

    pub(super) fn begin_project_find_regexp(
        &mut self,
        project: Project,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let default = origin.read(cx).buffer().symbol_at_point();
        let dir = files::directory_input(&project.root);
        let prompt = match &default {
            Some(sym) => format!("Find regexp in {dir} (default {sym}): "),
            None => format!("Find regexp in {dir}: "),
        };
        self.begin_read(
            ReadKind::SearchPattern {
                target: SearchReadTarget::Project(project),
                default,
                prompt,
            },
            origin,
            "",
            window,
            cx,
        );
    }

    pub(super) fn confirm_search_read(
        &mut self,
        kind: ReadKind,
        id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match kind {
            ReadKind::SearchPattern {
                target, default, ..
            } => self.confirm_search_pattern(id, target, default, window, cx),
            ReadKind::SearchDirectory {
                query,
                rerun_buffer,
            } => self.confirm_search_directory(id, query, rerun_buffer, window, cx),
            ReadKind::SearchFiles { buffer, query } => {
                self.confirm_search_files(id, buffer, query, window, cx)
            }
            _ => debug_assert!(false, "unexpected search read kind"),
        }
    }

    fn confirm_search_pattern(
        &mut self,
        id: u64,
        target: SearchReadTarget,
        default: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = self.session_input(cx).unwrap_or_default();
        let pattern = if input.is_empty() {
            match default {
                Some(default) if !default.is_empty() => default,
                _ => {
                    self.message_minibuffer("Search pattern cannot be empty", cx);
                    return;
                }
            }
        } else {
            input
        };
        let Some(origin) = self.end_session(id, window, cx) else {
            return;
        };
        match target {
            SearchReadTarget::Project(project) => {
                let query = SearchQuery::new(pattern, project.root.clone(), Some(project.root));
                self.start_search(query, None, false, &origin, window, cx);
            }
            SearchReadTarget::Directory => {
                let directory = self.file_read_directory(&origin, cx);
                let query = SearchQuery::new(pattern, directory.clone(), None);
                let initial = files::directory_input(&directory);
                self.begin_read(
                    ReadKind::SearchDirectory {
                        query,
                        rerun_buffer: None,
                    },
                    &origin,
                    &initial,
                    window,
                    cx,
                );
            }
            SearchReadTarget::RerunRegexp(buffer_id) => {
                let Some(mut query) = self.search_query_for(buffer_id) else {
                    self.show_message(&origin, "Search buffer no longer exists", cx);
                    return;
                };
                query.pattern = pattern;
                query.literal = false;
                self.start_search(query, Some(buffer_id), false, &origin, window, cx);
            }
            SearchReadTarget::RerunLiteral(buffer_id) => {
                let Some(mut query) = self.search_query_for(buffer_id) else {
                    self.show_message(&origin, "Search buffer no longer exists", cx);
                    return;
                };
                query.pattern = pattern;
                query.literal = true;
                self.start_search(query, Some(buffer_id), false, &origin, window, cx);
            }
        }
    }

    fn confirm_search_directory(
        &mut self,
        id: u64,
        mut query: SearchQuery,
        rerun_buffer: Option<BufferId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = self.session_input(cx).unwrap_or_default();
        let origin = self.session.as_ref().unwrap().origin();
        let base_dir = self.file_read_directory(origin, cx);
        let path = match files::expand(Path::new(&input), &base_dir) {
            Ok(path) => path,
            Err(error) => {
                self.message_minibuffer(error.to_string(), cx);
                return;
            }
        };
        if !path.is_dir() {
            self.message_minibuffer("Select an existing directory", cx);
            return;
        }
        let Some(origin) = self.end_session(id, window, cx) else {
            return;
        };
        query.directory = path.clone();
        query.project = crate::project::find_in_directory(&path).map(|p| p.root);
        self.start_search(query, rerun_buffer, false, &origin, window, cx);
    }

    fn confirm_search_files(
        &mut self,
        id: u64,
        buffer_id: BufferId,
        mut query: SearchQuery,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = self.session_input(cx).unwrap_or_default();
        let Some(origin) = self.end_session(id, window, cx) else {
            return;
        };
        let trimmed = input.trim();
        query.files_glob = (!trimmed.is_empty()).then(|| trimmed.to_owned());
        self.start_search(query, Some(buffer_id), false, &origin, window, cx);
    }

    fn search_query_for(&self, buffer_id: BufferId) -> Option<SearchQuery> {
        let index = self.buffers.index_of(buffer_id)?;
        self.buffers[index].buffer.search_query()
    }

    pub(super) fn discard_search_buffer(&mut self, id: BufferId) {
        if self
            .search_work
            .as_ref()
            .is_some_and(|work| work.buffer == id)
        {
            self.search_work = None;
        }
        if self.last_search_buffer == Some(id) {
            self.last_search_buffer = None;
        }
    }

    fn start_search(
        &mut self,
        query: SearchQuery,
        reuse_buffer: Option<BufferId>,
        preserve_collapse: bool,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !query.directory.is_dir() {
            self.show_message(
                origin,
                format!("Unable to open directory: {}", query.directory.display()),
                cx,
            );
            return;
        }

        // Cancel any previous running search before starting the next one.
        self.search_work = None;

        let existing = reuse_buffer
            .and_then(|id| self.buffers.index_of(id))
            .filter(|&idx| self.buffers[idx].buffer.is_search_buffer())
            .or_else(|| {
                self.buffers
                    .iter()
                    .position(|slot| slot.buffer.is_search_buffer())
            });
        let index = existing
            .unwrap_or_else(|| self.create_buffer(Buffer::search(query.clone()), window, cx));
        let buffer_id = self.buffers[index].id();
        let request = match self.buffers[index]
            .buffer
            .begin_search_request(query.clone())
        {
            Ok(request) => request,
            Err(error) => {
                self.show_message(origin, error, cx);
                return;
            }
        };

        let initial = SearchData::running(query.clone()).render();
        self.install_search_projection(
            index,
            request,
            query.clone(),
            initial,
            preserve_collapse,
            cx,
        );
        if !preserve_collapse {
            for view in self.all_views() {
                if view.read(cx).buffer().snapshot_id().buffer == buffer_id {
                    view.update(cx, |view, cx| {
                        view.show_location(crate::text::ByteOffset(0), window, cx)
                    });
                }
            }
        }

        self.last_search_buffer = Some(buffer_id);
        self.display_temporary(index, window, cx);

        let controller = Arc::new(SearchController::new());
        let worker_controller = controller.clone();
        let latest = Arc::new(Mutex::new(None::<SearchUpdate>));
        let worker_latest = latest.clone();
        let signal = Arc::new(SearchSignal::new());
        let worker_signal = signal.clone();

        cx.background_executor()
            .spawn(async move {
                search::run_streaming(query, worker_controller, worker_latest, worker_signal);
            })
            .detach();

        let task = cx.spawn_in(window, async move |shell, cx| {
            while signal.wait().await {
                let update = latest.lock().ok().and_then(|mut guard| guard.take());
                if let Some(update) = update {
                    let keep_going = shell
                        .update_in(cx, |shell, _, cx| {
                            shell.apply_search_update(buffer_id, request, update, cx)
                        })
                        .unwrap_or(false);
                    if !keep_going {
                        break;
                    }
                }
            }

            // Drain any final snapshot published right before channel close.
            let final_update = latest.lock().ok().and_then(|mut guard| guard.take());
            shell
                .update_in(cx, |shell, _, cx| {
                    if let Some(update) = final_update {
                        shell.apply_search_update(buffer_id, request, update, cx);
                    }
                    if shell
                        .search_work
                        .as_ref()
                        .is_some_and(|work| work.buffer == buffer_id && work.request == request)
                    {
                        shell.search_work = None;
                    }
                })
                .ok();
        });

        self.search_work = Some(SearchWork {
            buffer: buffer_id,
            request,
            controller,
            _task: task,
        });
    }

    fn apply_search_update(
        &mut self,
        buffer_id: BufferId,
        request: u64,
        update: SearchUpdate,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(index) = self.buffers.index_of(buffer_id) else {
            return false;
        };
        if !self.buffers[index].buffer.search_request_matches(request) {
            return false;
        }
        let installed =
            self.install_search_projection(index, request, update.query, update.rendered, true, cx);
        if !installed {
            return false;
        }
        if !update.status.is_running()
            && self
                .search_work
                .as_ref()
                .is_some_and(|work| work.buffer == buffer_id && work.request == request)
        {
            self.search_work = None;
        }
        true
    }

    fn install_search_projection(
        &mut self,
        index: usize,
        request: u64,
        query: SearchQuery,
        rendered: search::RenderedSearch,
        preserve_collapse: bool,
        cx: &mut Context<Self>,
    ) -> bool {
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
            .install_search(request, query, rendered, preserve_collapse)
        {
            self.refresh_buffer(id, cx);
            if preserve_collapse {
                for (view, anchor) in views {
                    view.update(cx, |view, cx| view.restore_section_scroll(anchor, cx));
                }
            }
            true
        } else {
            false
        }
    }

    fn search_recompile(
        &mut self,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let buffer_id = origin.read(cx).buffer().snapshot_id().buffer;
        let Some(query) = origin.read(cx).buffer().search_query() else {
            self.show_message(origin, "Not in a search buffer", cx);
            return;
        };
        self.start_search(query, Some(buffer_id), true, origin, window, cx);
    }

    fn begin_search_rerun_pattern(
        &mut self,
        literal: bool,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let buffer_id = origin.read(cx).buffer().snapshot_id().buffer;
        let Some(query) = origin.read(cx).buffer().search_query() else {
            self.show_message(origin, "Not in a search buffer", cx);
            return;
        };
        let prompt = if literal {
            "Literal search for: ".to_owned()
        } else {
            "Regexp search for: ".to_owned()
        };
        let target = if literal {
            SearchReadTarget::RerunLiteral(buffer_id)
        } else {
            SearchReadTarget::RerunRegexp(buffer_id)
        };
        self.begin_read(
            ReadKind::SearchPattern {
                target,
                default: None,
                prompt,
            },
            origin,
            &query.pattern,
            window,
            cx,
        );
    }

    fn begin_search_rerun_dir(
        &mut self,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let buffer_id = origin.read(cx).buffer().snapshot_id().buffer;
        let Some(query) = origin.read(cx).buffer().search_query() else {
            self.show_message(origin, "Not in a search buffer", cx);
            return;
        };
        let initial = files::directory_input(&query.directory);
        self.begin_read(
            ReadKind::SearchDirectory {
                query,
                rerun_buffer: Some(buffer_id),
            },
            origin,
            &initial,
            window,
            cx,
        );
    }

    fn begin_search_rerun_files(
        &mut self,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let buffer_id = origin.read(cx).buffer().snapshot_id().buffer;
        let Some(query) = origin.read(cx).buffer().search_query() else {
            self.show_message(origin, "Not in a search buffer", cx);
            return;
        };
        let initial = query.files_glob.clone().unwrap_or_default();
        self.begin_read(
            ReadKind::SearchFiles {
                buffer: buffer_id,
                query,
            },
            origin,
            &initial,
            window,
            cx,
        );
    }

    fn search_toggle_case(
        &mut self,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let buffer_id = origin.read(cx).buffer().snapshot_id().buffer;
        let Some(mut query) = origin.read(cx).buffer().search_query() else {
            self.show_message(origin, "Not in a search buffer", cx);
            return;
        };
        query.case_mode = query.case_mode.next();
        self.start_search(query, Some(buffer_id), false, origin, window, cx);
    }

    fn search_toggle_ignore(
        &mut self,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let buffer_id = origin.read(cx).buffer().snapshot_id().buffer;
        let Some(mut query) = origin.read(cx).buffer().search_query() else {
            self.show_message(origin, "Not in a search buffer", cx);
            return;
        };
        query.respect_ignore = !query.respect_ignore;
        self.start_search(query, Some(buffer_id), false, origin, window, cx);
    }

    fn search_kill_running(&mut self, origin: &Entity<BufferView>, cx: &mut Context<Self>) {
        let buffer_id = origin.read(cx).buffer().snapshot_id().buffer;
        if let Some(work) = &self.search_work
            && work.buffer == buffer_id
        {
            work.controller.cancel();
            self.show_message(origin, "Killed rg search", cx);
        } else {
            self.show_message(origin, "No active search process", cx);
        }
    }

    fn search_goto_at_point(
        &mut self,
        select_target: bool,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let buffer_id = origin.read(cx).buffer().snapshot_id().buffer;
        let location = match origin.read(cx).buffer().search_match_at_point() {
            Ok(loc) => loc,
            Err(error) => {
                self.show_message(origin, error, cx);
                return;
            }
        };
        self.last_search_buffer = Some(buffer_id);
        self.reveal_and_move_search_view(buffer_id, origin, &location, window, cx);
        let rg_pane = *self.panes.selected();
        self.visit_search_match(location, Some(rg_pane), select_target, origin, window, cx);
    }

    fn search_step_local(
        &mut self,
        forward: bool,
        display_source: bool,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let buffer_id = origin.read(cx).buffer().snapshot_id().buffer;
        let location = match origin.read(cx).buffer().search_step_match(forward) {
            Ok(loc) => loc,
            Err(error) => {
                self.show_message(origin, error, cx);
                return;
            }
        };
        self.last_search_buffer = Some(buffer_id);
        self.reveal_and_move_search_view(buffer_id, origin, &location, window, cx);
        if display_source {
            let rg_pane = *self.panes.selected();
            self.visit_search_match(location, Some(rg_pane), false, origin, window, cx);
        }
    }

    fn search_step_file(
        &mut self,
        forward: bool,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let buffer_id = origin.read(cx).buffer().snapshot_id().buffer;
        let location = match origin.read(cx).buffer().search_step_file(forward) {
            Ok(loc) => loc,
            Err(error) => {
                self.show_message(origin, error, cx);
                return;
            }
        };
        self.last_search_buffer = Some(buffer_id);
        self.reveal_and_move_search_view(buffer_id, origin, &location, window, cx);
    }

    fn global_step_error(
        &mut self,
        forward: bool,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let search_buffer_id = if origin.read(cx).buffer().is_search_buffer() {
            Some(origin.read(cx).buffer().snapshot_id().buffer)
        } else {
            self.last_search_buffer
                .filter(|id| {
                    self.buffers
                        .index_of(*id)
                        .is_some_and(|idx| self.buffers[idx].buffer.is_search_buffer())
                })
                .or_else(|| {
                    self.buffers.ids_in_mru().iter().copied().find(|id| {
                        self.buffers
                            .index_of(*id)
                            .is_some_and(|idx| self.buffers[idx].buffer.is_search_buffer())
                    })
                })
        };
        let Some(search_buffer_id) = search_buffer_id else {
            self.show_message(origin, "No search results buffer", cx);
            return;
        };
        let Some(search_index) = self.buffers.index_of(search_buffer_id) else {
            self.show_message(origin, "No search results buffer", cx);
            return;
        };

        // Prefer the currently selected pane when it already shows `*rg*`;
        // otherwise find any visible ordinary pane displaying `*rg*`.
        let selected_pane = *self.panes.selected();
        let rg_pane = if self.panes.side() != Some(&selected_pane)
            && self.body_panes[&selected_pane].current == search_buffer_id
        {
            Some(selected_pane)
        } else {
            self.panes
                .ordinary_leaves()
                .into_iter()
                .find(|pane| self.body_panes[pane].current == search_buffer_id)
        };
        let rg_view = rg_pane.map(|pane| self.body_panes[&pane].active_view().clone());

        let location = match &rg_view {
            Some(view) => view.read(cx).buffer().search_step_match(forward),
            None => self.buffers[search_index].buffer.search_step_match(forward),
        };
        let location = match location {
            Ok(loc) => loc,
            Err(error) => {
                self.show_message(origin, error, cx);
                return;
            }
        };

        self.last_search_buffer = Some(search_buffer_id);
        self.buffers[search_index]
            .buffer
            .reveal_section(location.target);
        let _ = self.buffers[search_index].buffer.set_point(location.target);
        self.refresh_buffer(search_buffer_id, cx);
        // Keep every visible and cached `*rg*` view synchronized with the stepped match.
        for view in self.all_views() {
            if view.read(cx).buffer().snapshot_id().buffer == search_buffer_id {
                view.update(cx, |view, cx| {
                    view.show_location(location.target, window, cx)
                });
            }
        }
        self.visit_search_match(location, rg_pane, true, origin, window, cx);
    }

    fn reveal_and_move_search_view(
        &mut self,
        buffer_id: BufferId,
        view: &Entity<BufferView>,
        location: &MatchLocation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(index) = self.buffers.index_of(buffer_id) {
            self.buffers[index].buffer.reveal_section(location.target);
            let _ = self.buffers[index].buffer.set_point(location.target);
            self.refresh_buffer(buffer_id, cx);
        }
        view.update(cx, |view, cx| {
            view.show_location(location.target, window, cx)
        });
    }

    fn visit_search_match(
        &mut self,
        location: MatchLocation,
        rg_pane: Option<u64>,
        select_target: bool,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !location.path.is_file() {
            self.show_message(
                origin,
                format!("File not found: {}", location.path.display()),
                cx,
            );
            return;
        }
        let target_pane = self.choose_search_target_pane(&location.path, rg_pane, window, cx);
        let active_origin = self.active_view().clone();
        self.visit_file_with(
            location.path,
            &active_origin,
            VisitOptions {
                location: Some((location.line, location.column)),
                tree: None,
                pane: target_pane,
                select_pane: select_target,
            },
            window,
            cx,
        );
    }

    fn choose_search_target_pane(
        &mut self,
        path: &Path,
        rg_pane: Option<u64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<u64> {
        let ordinary = self.panes.ordinary_leaves();
        // 1. Reuse an ordinary non-rg pane already showing this file.
        if let Some(existing_id) = self
            .buffers
            .iter()
            .find(|slot| slot.buffer.path().as_deref() == Some(path))
            .map(|slot| slot.id())
            && let Some(pane) = ordinary
                .iter()
                .copied()
                .find(|pane| Some(*pane) != rg_pane && self.body_panes[pane].current == existing_id)
        {
            return Some(pane);
        }
        // 2. If the currently selected pane is an ordinary non-rg pane (e.g.
        //    calling `M-g n` from a code pane), reuse it directly.
        let selected = *self.panes.selected();
        if self.panes.side() != Some(&selected) && Some(selected) != rg_pane {
            return Some(selected);
        }
        // 3. Prefer the source pane recorded when `*rg*` was popped up.
        if let Some(rg_id) = rg_pane
            && let Some(rg_body) = self.body_panes.get(&rg_id)
            && let Some(source) = rg_body.quit_restore.iter().rev().find_map(|restore| {
                (restore.source != rg_id
                    && self.body_panes.contains_key(&restore.source)
                    && self.panes.side() != Some(&restore.source))
                .then_some(restore.source)
            })
        {
            return Some(source);
        }
        // 4. Otherwise pick another ordinary pane.
        if let Some(other) = ordinary.iter().copied().find(|pane| Some(*pane) != rg_pane) {
            return Some(other);
        }
        // 5. `*rg*` is the only ordinary pane (whether currently focused or
        //    with focus on the Treemacs side window): try splitting `*rg*` so
        //    it stays visible, otherwise reuse `rg_id` instead of failing on a
        //    dedicated side window.
        if let Some(rg_id) = rg_pane {
            let was_side = self.panes.side() == Some(&selected);
            if was_side {
                self.select_pane(rg_id, window, cx);
            }
            if *self.panes.selected() == rg_id {
                let axis = if self.check_split(Axis::Vertical, cx).is_ok() {
                    Some(Axis::Vertical)
                } else if self.check_split(Axis::Horizontal, cx).is_ok() {
                    Some(Axis::Horizontal)
                } else {
                    None
                };
                if let Some(axis) = axis {
                    let fallback = self
                        .buffers
                        .other_buffer(self.body_panes[&rg_id].current)
                        .and_then(|id| self.buffers.index_of(id))
                        .unwrap_or_else(|| self.create_buffer(Buffer::scratch(), window, cx));
                    let view =
                        Self::create_view(self.buffers[fallback].buffer.new_view(), window, cx);
                    let new_pane = self.split_selected(view, axis, window, cx);
                    return Some(new_pane);
                }
            }
            return Some(rg_id);
        }
        None
    }
}
