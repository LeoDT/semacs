use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use gpui_kit::{Context, Task, Window};

use super::AppShell;
use crate::completion::{
    self, AnnotationPlacement, Candidate, CandidateAction, CandidateList, CandidateResolution,
};
use crate::files;
use crate::session::ReadKind;

pub(super) struct CompletionWork {
    cancelled: Arc<AtomicBool>,
    _task: Task<()>,
}

impl Drop for CompletionWork {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

impl AppShell {
    pub(super) fn refresh_candidates(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = &self.session else { return };
        if !session.kind().has_candidates() {
            return;
        }
        let (id, kind, origin) = (
            session.id(),
            session.kind().clone(),
            session.origin().clone(),
        );
        let Some(mount) = &mut self.minibuffer else {
            return;
        };
        let editor = mount.view.read(cx).editor().clone();
        let display = editor.read(cx).display_snapshot();
        if display.marked_range().is_some() {
            mount.completion = None;
            mount.candidate_snapshot = None;
            mount.pending_candidate_action = None;
            return;
        }
        let snapshot = display.id();
        if mount
            .candidate_snapshot
            .is_some_and(|previous| previous.committed == snapshot.committed)
        {
            return;
        }
        mount.completion = None;
        mount.pending_candidate_action = None;
        mount.candidate_snapshot = Some(snapshot);
        let input = mount.view.read(cx).contents(cx);
        if let ReadKind::ProjectFindFile { project } = &kind {
            if let Some(files) = mount.project_files.clone() {
                let history = cx
                    .global::<crate::file_history::FileHistory>()
                    .entries()
                    .to_vec();
                let current = origin.read(cx).buffer().path();
                if files.len() <= 4096 {
                    let items = Self::filter_project_files(
                        &files,
                        &input,
                        &history,
                        &project.root,
                        current.as_deref(),
                        &AtomicBool::new(false),
                    );
                    self.session.as_mut().unwrap().candidates =
                        Some(CandidateList::ready(items, &input, None, false));
                    self.publish_candidates(cx);
                } else {
                    self.session
                        .as_mut()
                        .unwrap()
                        .candidates
                        .as_mut()
                        .unwrap()
                        .loading = true;
                    self.publish_candidates(cx);
                    let cancelled = Arc::new(AtomicBool::new(false));
                    let worker_cancelled = cancelled.clone();
                    let root = project.root.clone();
                    let task = cx.spawn_in(window, async move |shell, cx| {
                        let items = cx
                            .background_executor()
                            .spawn(async move {
                                Self::filter_project_files(
                                    &files,
                                    &input,
                                    &history,
                                    &root,
                                    current.as_deref(),
                                    &worker_cancelled,
                                )
                            })
                            .await;
                        shell
                            .update_in(cx, |shell, window, cx| {
                                if !shell.session.as_ref().is_some_and(|session| {
                                    session.id() == id && session.origin() == &origin
                                }) || !shell
                                    .minibuffer
                                    .as_ref()
                                    .is_some_and(|mount| mount.candidate_snapshot == Some(snapshot))
                                    || editor.read(cx).display_snapshot().id().committed
                                        != snapshot.committed
                                    || editor.read(cx).display_snapshot().marked_range().is_some()
                                {
                                    return;
                                }
                                let input = shell.session_input(cx).unwrap_or_default();
                                shell.session.as_mut().unwrap().candidates =
                                    Some(CandidateList::ready(items, &input, None, false));
                                let mount = shell.minibuffer.as_mut().unwrap();
                                let action = mount.pending_candidate_action.take();
                                mount.completion = None;
                                shell.publish_candidates(cx);
                                if let Some(action) = action {
                                    shell.candidate_action(action, window, cx);
                                }
                            })
                            .ok();
                    });
                    self.minibuffer.as_mut().unwrap().completion = Some(CompletionWork {
                        cancelled,
                        _task: task,
                    });
                }
                return;
            }
            if mount.project_files_work.is_some() {
                return;
            }
            self.session
                .as_mut()
                .unwrap()
                .candidates
                .as_mut()
                .unwrap()
                .loading = true;
            self.publish_candidates(cx);
            let cancelled = Arc::new(AtomicBool::new(false));
            let worker_cancelled = cancelled.clone();
            let project = project.clone();
            #[cfg(test)]
            let gate = self.project_files_gate.take();
            let task = cx.spawn_in(window, async move |shell, cx| {
                #[cfg(test)]
                if let Some(gate) = gate {
                    let _ = gate.await;
                }
                let bg_cancelled = worker_cancelled.clone();
                let result = cx
                    .background_executor()
                    .spawn(async move { crate::project::list_files(&project, &bg_cancelled) })
                    .await;
                shell
                    .update_in(cx, |shell, window, cx| {
                        if !shell.session.as_ref().is_some_and(|session| {
                            session.id() == id && session.origin() == &origin
                        }) {
                            return;
                        }
                        if let Some(mount) = &mut shell.minibuffer {
                            mount.project_files_work = None;
                        }
                        match result {
                            Ok(files) if files.is_empty() => {
                                if let Some(target) = shell.end_session(id, window, cx) {
                                    shell.show_message(&target, "Empty file list", cx);
                                }
                            }
                            Ok(files) => {
                                let action = if let Some(mount) = &mut shell.minibuffer {
                                    mount.project_files = Some(Arc::new(files));
                                    mount.candidate_snapshot = None;
                                    mount.pending_candidate_action.take()
                                } else {
                                    None
                                };
                                shell.refresh_candidates(window, cx);
                                if let Some(action) = action {
                                    shell.candidate_action(action, window, cx);
                                }
                            }
                            Err(error) => {
                                if !worker_cancelled.load(Ordering::Relaxed)
                                    && let Some(target) = shell.end_session(id, window, cx)
                                {
                                    shell.show_message(&target, error, cx);
                                }
                            }
                        }
                    })
                    .ok();
            });
            self.minibuffer.as_mut().unwrap().project_files_work = Some(CompletionWork {
                cancelled,
                _task: task,
            });
            return;
        }
        if matches!(
            kind,
            ReadKind::FindFile
                | ReadKind::WriteFile
                | ReadKind::TreeRoot
                | ReadKind::TreeFile
                | ReadKind::ProjectDirectory { .. }
                | ReadKind::ProjectScanDirectory
                | ReadKind::SearchDirectory { .. }
        ) {
            let directories_only = matches!(
                kind,
                ReadKind::TreeRoot
                    | ReadKind::ProjectDirectory { .. }
                    | ReadKind::ProjectScanDirectory
                    | ReadKind::SearchDirectory { .. }
            );
            let directory = self.file_read_directory(&origin, cx);
            let history = cx
                .global::<crate::file_history::FileHistory>()
                .entries()
                .to_vec();
            let current = origin
                .read(cx)
                .buffer()
                .path()
                .map(|path| files::abbreviate(&path));
            self.session
                .as_mut()
                .unwrap()
                .candidates
                .as_mut()
                .unwrap()
                .loading = true;
            self.publish_candidates(cx);
            let cancelled = Arc::new(AtomicBool::new(false));
            let worker_cancelled = cancelled.clone();
            let task = cx.spawn_in(window, async move |shell, cx| {
                let (items, directory_input, raw_directory) = cx
                    .background_executor()
                    .spawn(async move {
                        let listing = files::path_candidates(&input, &directory, &worker_cancelled);
                        let candidates = listing
                            .candidates
                            .into_iter()
                            .filter(|candidate| !directories_only || candidate.directory)
                            .collect();
                        let mut items = completion::filter_unranked(
                            candidates,
                            &listing.query,
                            &worker_cancelled,
                        );
                        files::rank_candidates(
                            &mut items,
                            &history,
                            &listing.typed_directory,
                            current.as_deref(),
                        );
                        (items, listing.directory_input, listing.raw_directory)
                    })
                    .await;
                shell
                    .update_in(cx, |shell, window, cx| {
                        if !shell.session.as_ref().is_some_and(|session| {
                            session.id() == id && session.origin() == &origin
                        }) || !shell
                            .minibuffer
                            .as_ref()
                            .is_some_and(|mount| mount.candidate_snapshot == Some(snapshot))
                            || editor.read(cx).display_snapshot().id().committed
                                != snapshot.committed
                            || editor.read(cx).display_snapshot().marked_range().is_some()
                        {
                            return;
                        }
                        let input = shell.session_input(cx).unwrap_or_default();
                        let mut list = CandidateList::ready(items, &input, None, directory_input);
                        list.raw_directory = raw_directory;
                        list.annotation = AnnotationPlacement::MarginaliaLeft;
                        shell.session.as_mut().unwrap().candidates = Some(list);
                        let mount = shell.minibuffer.as_mut().unwrap();
                        let action = mount.pending_candidate_action.take();
                        mount.completion = None;
                        shell.publish_candidates(cx);
                        if let Some(action) = action {
                            shell.candidate_action(action, window, cx);
                        }
                    })
                    .ok();
            });
            self.minibuffer.as_mut().unwrap().completion = Some(CompletionWork {
                cancelled,
                _task: task,
            });
        } else if let ReadKind::ProjectSelect { require_known, .. } = &kind {
            let roots = cx.global::<crate::project::ProjectList>().roots();
            let items = super::project::project_select_candidates(&roots, *require_known, &input);
            self.session.as_mut().unwrap().candidates = Some(CandidateList::ready(
                items,
                &input,
                kind.candidate_default(),
                false,
            ));
            self.publish_candidates(cx);
        } else {
            let candidates = match &kind {
                ReadKind::HistoryFileVersion { versions } => versions
                    .iter()
                    .map(|version| {
                        let annotation = match &version.visit {
                            crate::diff::Visit::Blob { oid, path, .. } => {
                                format!("{} {}", &oid[..12], path.to_string_lossy().escape_debug())
                            }
                            _ => String::new(),
                        };
                        Candidate::named(version.name.clone(), annotation)
                    })
                    .collect(),
                ReadKind::Theme => crate::theme::ThemeSettings::global(cx)
                    .names()
                    .map(|name| {
                        Candidate::named(
                            name.into(),
                            if name == crate::theme::ThemeSettings::global(cx).active() {
                                "current".into()
                            } else {
                                String::new()
                            },
                        )
                    })
                    .collect(),
                ReadKind::Command => self
                    .commands
                    .commands_for(origin.read(cx).buffer().mode())
                    .map(|command| {
                        Candidate::named(command.name.clone(), command.annotation.clone())
                    })
                    .collect(),
                ReadKind::ProjectSwitchToBuffer { project, .. } => {
                    let allowed = self.project_buffers(project);
                    self.buffers
                        .iter()
                        .filter(|slot| {
                            slot.id() != self.active_buffer_id() && allowed.contains(&slot.id())
                        })
                        .map(|slot| {
                            let buffer = &slot.buffer;
                            let annotation = format!(
                                "{} {}",
                                if buffer.is_modified() { "**" } else { "--" },
                                buffer
                                    .path()
                                    .map(|path| path.display().to_string())
                                    .unwrap_or_default()
                            );
                            Candidate::named(buffer.display_name(), annotation)
                        })
                        .collect()
                }
                ReadKind::SwitchToBuffer { .. } | ReadKind::KillBuffer { .. } => self
                    .buffers
                    .iter()
                    .filter(|slot| {
                        !matches!(kind, ReadKind::SwitchToBuffer { .. })
                            || slot.id() != self.active_buffer_id()
                    })
                    .map(|slot| {
                        let buffer = &slot.buffer;
                        let annotation = format!(
                            "{} {}",
                            if buffer.is_modified() { "**" } else { "--" },
                            buffer
                                .path()
                                .map(|path| path.display().to_string())
                                .unwrap_or_default()
                        );
                        Candidate::named(buffer.display_name(), annotation)
                    })
                    .collect(),
                _ => unreachable!(),
            };
            let mut items = completion::filter(candidates, &input, &AtomicBool::new(false));
            if kind == ReadKind::Command {
                cx.global::<crate::command_history::CommandHistory>()
                    .sort(&mut items);
            }
            self.session.as_mut().unwrap().candidates = Some(CandidateList::ready(
                items,
                &input,
                kind.candidate_default(),
                false,
            ));
            self.publish_candidates(cx);
        }
    }

    fn publish_candidates(&self, cx: &mut Context<Self>) {
        if let (Some(mount), Some(list)) = (
            &self.minibuffer,
            self.session
                .as_ref()
                .and_then(|session| session.candidates()),
        ) {
            mount
                .view
                .update(cx, |mini, cx| mini.set_candidates(list.clone(), cx));
        }
    }

    pub(super) fn navigate_candidates(
        &mut self,
        forward: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.refresh_candidates(window, cx);
        if let Some(list) = self
            .session
            .as_mut()
            .and_then(|session| session.candidates.as_mut())
        {
            list.navigate(forward);
            self.publish_candidates(cx);
        }
    }

    pub(super) fn candidate_action(
        &mut self,
        action: CandidateAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.refresh_candidates(window, cx);
        let Some(session) = &self.session else { return };
        let Some(list) = session.candidates() else {
            return;
        };
        let id = session.id();
        let input = self.session_input(cx).unwrap_or_default();
        match list.resolve(action, &input) {
            CandidateResolution::Pending => {
                self.minibuffer.as_mut().unwrap().pending_candidate_action = Some(action);
            }
            CandidateResolution::Insert(text) => {
                if text != input {
                    self.replace_session_input(id, &text, window, cx);
                }
                self.refresh_candidates(window, cx);
            }
            CandidateResolution::Accept(text) => {
                if text != input {
                    self.replace_session_input(id, &text, window, cx);
                }
                self.confirm_session(window, cx);
            }
            CandidateResolution::NoMatch => self.message_minibuffer("No match", cx),
        }
    }
}
