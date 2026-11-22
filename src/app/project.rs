use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use gpui_kit::{App, Context, Entity, Keystroke, Task, Window};

use super::AppShell;
use crate::buffer::{Buffer, BufferId};
use crate::command::Command;
use crate::files;
use crate::project::{self, CHOOSE_DIR_LABEL, Project, ProjectKind, ProjectList};
use crate::session::{ProjectAction, ReadKind};
use crate::view::BufferView;

#[cfg(test)]
#[path = "project_tests.rs"]
mod tests;

pub(super) struct ProjectScanWork {
    cancelled: Arc<AtomicBool>,
    _task: Task<()>,
}

impl Drop for ProjectScanWork {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

impl AppShell {
    pub(super) fn project_command(
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
            Command::ProjectSwitchProject => {
                self.begin_project_select(
                    ProjectAction::SwitchMenu,
                    None,
                    false,
                    origin,
                    window,
                    cx,
                );
            }
            Command::ProjectFindFile => {
                self.with_current_project(ProjectAction::FindFile, origin, window, cx);
            }
            Command::ProjectSwitchToBuffer => {
                self.with_current_project(ProjectAction::SwitchToBuffer, origin, window, cx);
            }
            Command::ProjectKillBuffers => {
                self.with_current_project(ProjectAction::KillBuffers, origin, window, cx);
            }
            Command::ProjectRememberProject => {
                self.with_current_project(ProjectAction::Remember, origin, window, cx);
            }
            Command::ProjectForgetProject => {
                cx.global_mut::<ProjectList>().prune_zombies();
                if cx.global::<ProjectList>().entries().is_empty() {
                    self.show_message(origin, "No known projects", cx);
                    return;
                }
                self.begin_project_select(
                    ProjectAction::Forget,
                    Some("Forget project: ".to_owned()),
                    true,
                    origin,
                    window,
                    cx,
                );
            }
            Command::ProjectForgetZombieProjects => {
                let removed = cx.global_mut::<ProjectList>().prune_zombies();
                let message = match removed {
                    0 => "No zombie projects found".to_owned(),
                    1 => "Forgot 1 zombie project".to_owned(),
                    n => format!("Forgot {n} zombie projects"),
                };
                self.show_message(origin, message, cx);
            }
            Command::ProjectRememberProjectsUnder => {
                self.begin_file_read(ReadKind::ProjectScanDirectory, origin, window, cx);
            }
            _ => debug_assert!(false, "{} is not a project command", command.name()),
        }
    }

    pub(super) fn project_directory_for_buffer(&self, buffer: &Buffer) -> PathBuf {
        if let Some(repository) = buffer.repository() {
            return repository.root;
        }
        if let Some(root) = buffer.tree_root() {
            return root;
        }
        buffer
            .path()
            .and_then(|path| path.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| self.startup_directory.clone())
    }

    pub(super) fn current_project_for(
        &self,
        origin: &Entity<BufferView>,
        cx: &App,
    ) -> Option<Project> {
        let directory = self.project_directory_for_buffer(origin.read(cx).buffer());
        project::find_in_directory(&directory)
    }

    pub(super) fn project_buffers(&self, project: &Project) -> Vec<BufferId> {
        let canonical_root = std::fs::canonicalize(&project.root).ok();
        self.buffers
            .ids_in_mru()
            .iter()
            .copied()
            .filter(|id| {
                self.buffers.index_of(*id).is_some_and(|index| {
                    let directory = self.project_directory_for_buffer(&self.buffers[index].buffer);
                    project.contains_directory_with_canonical(&directory, canonical_root.as_deref())
                })
            })
            .collect()
    }

    fn project_buffers_to_kill(&self, project: &Project) -> Vec<BufferId> {
        self.project_buffers(project)
            .into_iter()
            .filter(|id| {
                self.buffers.index_of(*id).is_some_and(|index| {
                    let buffer = &self.buffers[index].buffer;
                    !buffer.is_message_log() && !buffer.display_name().starts_with(' ')
                })
            })
            .collect()
    }

    fn with_current_project(
        &mut self,
        action: ProjectAction,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(project) = self.current_project_for(origin, cx) {
            cx.global_mut::<ProjectList>().remember(&project.root);
            self.run_project_action(project, action, origin, window, cx);
        } else {
            self.begin_project_select(action, None, false, origin, window, cx);
        }
    }

    fn begin_project_select(
        &mut self,
        action: ProjectAction,
        custom_prompt: Option<String>,
        require_known: bool,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.global_mut::<ProjectList>().prune_zombies();
        let current = self.current_project_for(origin, cx).and_then(|project| {
            cx.global::<ProjectList>()
                .contains(&project.root)
                .then(|| files::directory_input(&project.root))
        });
        let prompt = match custom_prompt {
            Some(prompt) => prompt,
            None if current.is_some() => "Select project (default current project): ".to_owned(),
            None => "Select project: ".to_owned(),
        };
        self.begin_read(
            ReadKind::ProjectSelect {
                action,
                prompt,
                default: current,
                require_known,
            },
            origin,
            "",
            window,
            cx,
        );
    }

    pub(super) fn confirm_project_read(
        &mut self,
        kind: ReadKind,
        id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match kind {
            ReadKind::ProjectSelect {
                action,
                default,
                require_known,
                ..
            } => self.confirm_project_select(id, action, default, require_known, window, cx),
            ReadKind::ProjectDirectory { action } => {
                self.confirm_project_directory(id, action, window, cx)
            }
            ReadKind::ProjectScanDirectory => self.confirm_project_scan_directory(id, window, cx),
            ReadKind::ProjectFindFile { project } => {
                self.confirm_project_find_file(id, project, window, cx)
            }
            ReadKind::ProjectSwitchToBuffer { project, default } => {
                self.confirm_project_switch_to_buffer(id, project, default, window, cx)
            }
            _ => debug_assert!(false, "unexpected project read kind"),
        }
    }

    fn confirm_project_select(
        &mut self,
        id: u64,
        action: ProjectAction,
        default: Option<String>,
        require_known: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = self.session_input(cx).unwrap_or_default();
        if !require_known && input == CHOOSE_DIR_LABEL {
            if let Some(origin) = self.end_session(id, window, cx) {
                self.begin_file_read(ReadKind::ProjectDirectory { action }, &origin, window, cx);
            }
            return;
        }
        let chosen = if input.is_empty() {
            match default {
                Some(default) => default,
                None => {
                    self.message_minibuffer("No match", cx);
                    return;
                }
            }
        } else {
            input
        };
        let origin = self.session.as_ref().unwrap().origin();
        let directory = self.file_read_directory(origin, cx);
        let Ok(path) = files::expand(Path::new(&chosen), &directory) else {
            self.message_minibuffer("No match", cx);
            return;
        };
        if !cx.global::<ProjectList>().contains(&path) {
            self.message_minibuffer("No match", cx);
            return;
        }
        let Some(origin) = self.end_session(id, window, cx) else {
            return;
        };
        if action == ProjectAction::Forget {
            if cx.global_mut::<ProjectList>().forget(&path) {
                self.show_message(
                    &origin,
                    format!(
                        "Project `{}' removed from known projects",
                        files::directory_input(&path)
                    ),
                    cx,
                );
            }
            return;
        }
        self.resolve_selected_project(path, action, &origin, window, cx);
    }

    fn confirm_project_directory(
        &mut self,
        id: u64,
        action: ProjectAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = self.session_input(cx).unwrap_or_default();
        let origin = self.session.as_ref().unwrap().origin();
        let directory = self.file_read_directory(origin, cx);
        let path = match files::expand(Path::new(&input), &directory) {
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
        self.resolve_selected_project(path, action, &origin, window, cx);
    }

    fn resolve_selected_project(
        &mut self,
        directory: PathBuf,
        action: ProjectAction,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !directory.is_dir() {
            cx.global_mut::<ProjectList>().forget(&directory);
            self.show_message(
                origin,
                format!(
                    "Project `{}' not found; removed from list",
                    files::directory_input(&directory)
                ),
                cx,
            );
            return;
        }
        let mut removed_notice = None;
        let project = if let Some(project) = project::find_in_directory(&directory) {
            cx.global_mut::<ProjectList>().remember(&project.root);
            project
        } else {
            if cx.global_mut::<ProjectList>().forget(&directory) {
                let notice = format!(
                    "Project `{}' not found; removed from list",
                    files::directory_input(&directory)
                );
                self.show_message(origin, notice.clone(), cx);
                removed_notice = Some(notice);
            }
            Project::transient(directory)
        };
        self.run_project_action(project, action, origin, window, cx);
        if let Some(notice) = removed_notice
            && self.minibuffer.is_some()
        {
            self.message_minibuffer(notice, cx);
        }
    }

    fn run_project_action(
        &mut self,
        project: Project,
        action: ProjectAction,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            ProjectAction::SwitchMenu => {
                self.begin_read(
                    ReadKind::ProjectSwitchMenu { project },
                    origin,
                    "",
                    window,
                    cx,
                );
            }
            ProjectAction::FindFile => {
                self.begin_read(
                    ReadKind::ProjectFindFile { project },
                    origin,
                    "",
                    window,
                    cx,
                );
            }
            ProjectAction::SwitchToBuffer => {
                self.begin_project_switch_to_buffer(project, origin, window, cx);
            }
            ProjectAction::KillBuffers => {
                self.begin_project_kill_buffers(project, origin, window, cx);
            }
            ProjectAction::Remember => {
                if project.kind == ProjectKind::Git {
                    cx.global_mut::<ProjectList>().remember(&project.root);
                    self.show_message(origin, "Current project remembered", cx);
                } else {
                    self.show_message(
                        origin,
                        format!(
                            "No project found in `{}'",
                            files::directory_input(&project.root)
                        ),
                        cx,
                    );
                }
            }
            ProjectAction::Forget => {
                unreachable!("ProjectAction::Forget is handled in confirm_project_select")
            }
        }
    }

    pub(super) fn answer_project_switch_menu(
        &mut self,
        stroke: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(session) = &self.session else {
            return false;
        };
        let ReadKind::ProjectSwitchMenu { project } = session.kind().clone() else {
            return false;
        };
        let id = session.id();
        let choice = match stroke.key.as_str() {
            "f" => Some('f'),
            "b" => Some('b'),
            "v" => Some('v'),
            "t" => Some('t'),
            _ => None,
        };
        let Some(choice) = choice else {
            self.message_minibuffer(session.kind().answer_hint(), cx);
            return true;
        };
        let Some(target) = self.end_session(id, window, cx) else {
            return true;
        };
        match choice {
            'f' => {
                self.begin_read(
                    ReadKind::ProjectFindFile { project },
                    &target,
                    "",
                    window,
                    cx,
                );
            }
            'b' => self.begin_project_switch_to_buffer(project, &target, window, cx),
            'v' => self.open_status_in(&target, Some(project.root), window, cx),
            't' => self.open_tree_for_project(project.root, &target, window, cx),
            _ => unreachable!(),
        }
        true
    }

    fn begin_project_switch_to_buffer(
        &mut self,
        project: Project,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let project_bufs = self.project_buffers(&project);
        if project_bufs.is_empty() {
            self.show_message(
                origin,
                format!("No buffers in project {}", project.name()),
                cx,
            );
            return;
        }
        let active = self.active_buffer_id();
        let default = project_bufs
            .into_iter()
            .find(|id| *id != active)
            .and_then(|id| self.buffers.index_of(id))
            .map(|index| self.buffers[index].buffer.display_name());
        self.begin_read(
            ReadKind::ProjectSwitchToBuffer { project, default },
            origin,
            "",
            window,
            cx,
        );
    }

    fn confirm_project_switch_to_buffer(
        &mut self,
        id: u64,
        project: Project,
        default: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = self.session_input(cx).unwrap_or_default();
        let name = if input.is_empty() {
            default
        } else {
            Some(input)
        };
        let Some(name) = name else {
            self.message_minibuffer("No buffer name", cx);
            return;
        };
        let allowed = self.project_buffers(&project);
        let index = self
            .buffers
            .iter()
            .position(|slot| slot.buffer.display_name() == name && allowed.contains(&slot.id()));
        let Some(index) = index else {
            self.message_minibuffer(format!("No buffer named {name}"), cx);
            return;
        };
        if self.end_session(id, window, cx).is_some() {
            self.select_ordinary_for_visit(window, cx);
            self.activate(index, window, cx);
        }
    }

    fn confirm_project_find_file(
        &mut self,
        id: u64,
        project: Project,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = self.session_input(cx).unwrap_or_default();
        if input.is_empty() {
            self.message_minibuffer("You didn't specify the file", cx);
            return;
        }
        let path = match files::expand(Path::new(&input), &project.root) {
            Ok(path) => path,
            Err(error) => {
                self.message_minibuffer(format!("{input}: {error}"), cx);
                return;
            }
        };
        if path.is_dir() {
            self.message_minibuffer("Select a file or enter a file name", cx);
            return;
        }
        if self.end_session(id, window, cx).is_some() {
            let entry = files::abbreviate(&path);
            cx.global_mut::<crate::file_history::FileHistory>()
                .record(&entry);
            self.select_ordinary_for_visit(window, cx);
            let target = self.active_view().clone();
            self.visit_file(path, &target, window, cx);
        }
    }

    fn select_ordinary_for_visit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.panes.side() == Some(self.panes.selected()) {
            let target = self
                .panes
                .recent_ordinary()
                .or_else(|| self.panes.ordinary_leaves().first().copied());
            if let Some(pane) = target {
                self.select_pane(pane, window, cx);
            }
        }
    }

    fn begin_project_kill_buffers(
        &mut self,
        project: Project,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let buffers = self.project_buffers_to_kill(&project);
        if buffers.is_empty() {
            self.show_message(origin, "No buffers to kill", cx);
            return;
        }
        self.begin_read(
            ReadKind::ProjectKillBuffersConfirm {
                project_name: project.name(),
                buffers,
            },
            origin,
            "",
            window,
            cx,
        );
    }

    fn confirm_project_scan_directory(
        &mut self,
        id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = self.session_input(cx).unwrap_or_default();
        let origin = self.session.as_ref().unwrap().origin();
        let directory = self.file_read_directory(origin, cx);
        let path = match files::expand(Path::new(&input), &directory) {
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
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = cancelled.clone();
        let task = cx.spawn_in(window, async move |shell, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { project::projects_under(&path, false, &worker_cancelled) })
                .await;
            shell
                .update_in(cx, |shell, _, cx| {
                    shell.project_scan_task = None;
                    match result {
                        Ok(roots) => {
                            let mut count = 0usize;
                            let list = cx.global_mut::<ProjectList>();
                            for root in roots {
                                if !list.contains(&root) {
                                    list.remember(&root);
                                    count += 1;
                                }
                            }
                            let message = match count {
                                0 => "No projects were found".to_owned(),
                                1 => "1 project was found".to_owned(),
                                n => format!("{n} projects were found"),
                            };
                            shell.show_message(&origin, message, cx);
                        }
                        Err(error) => shell.show_message(&origin, error, cx),
                    }
                })
                .ok();
        });
        self.project_scan_task = Some(ProjectScanWork {
            cancelled,
            _task: task,
        });
    }

    #[cfg(test)]
    pub(crate) fn gate_project_files(&mut self, gate: futures_channel::oneshot::Receiver<()>) {
        self.project_files_gate = Some(gate);
    }

    /// Helper used by candidate generation for `ProjectFindFile`: filters and
    /// ranks cached project-relative file names.
    pub(super) fn filter_project_files(
        files: &[String],
        query: &str,
        history: &[String],
        root: &Path,
        current: Option<&Path>,
        cancelled: &AtomicBool,
    ) -> Vec<crate::completion::MatchedCandidate> {
        let mut candidates = Vec::with_capacity(files.len());
        for name in files {
            if cancelled.load(Ordering::Relaxed) {
                return Vec::new();
            }
            candidates.push(crate::completion::Candidate::named(
                name.clone(),
                String::new(),
            ));
        }
        let mut items = crate::completion::filter_unranked(candidates, query, cancelled);
        if !cancelled.load(Ordering::Relaxed) {
            project::rank_project_files(&mut items, history, root, current);
        }
        items
    }
}

pub(super) fn project_select_candidates(
    roots: &[PathBuf],
    require_known: bool,
    query: &str,
) -> Vec<crate::completion::MatchedCandidate> {
    use crate::completion::{Candidate, MatchedCandidate};
    let make = |root: &PathBuf| {
        let label = files::directory_input(root);
        Candidate::named(label, String::new())
    };
    if query.is_empty() {
        let mut items: Vec<_> = roots
            .iter()
            .map(|root| MatchedCandidate {
                candidate: make(root),
                highlights: Vec::new(),
            })
            .collect();
        if !require_known {
            items.push(MatchedCandidate {
                candidate: Candidate::named(CHOOSE_DIR_LABEL.to_owned(), String::new()),
                highlights: Vec::new(),
            });
        }
        return items;
    }
    let cancelled = AtomicBool::new(false);
    let mut items = crate::completion::filter(roots.iter().map(make).collect(), query, &cancelled);
    if !require_known {
        items.extend(crate::completion::filter(
            vec![Candidate::named(CHOOSE_DIR_LABEL.to_owned(), String::new())],
            query,
            &cancelled,
        ));
    }
    items
}
