use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use gpui_kit::{Context, Task, Window};

use super::AppShell;
use crate::completion::{self, Candidate, CandidateAction, CandidateList, CandidateResolution};
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
        if matches!(kind, ReadKind::FindFile | ReadKind::WriteFile) {
            let directory = self.default_directory_for(&origin, cx);
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
                        let items = completion::filter(
                            listing.candidates,
                            &listing.query,
                            &worker_cancelled,
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
        } else {
            let candidates = match &kind {
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
            CandidateResolution::FileNameRequired => {
                self.message_minibuffer("Select a file or enter a file name", cx)
            }
        }
    }
}
