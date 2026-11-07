use std::collections::hash_map::Entry;

use gpui_kit::{Context, Entity, Global, Window};

use super::AppShell;
use crate::buffer::{Buffer, BufferId};
use crate::command::Command;
use crate::commit::message::{MessagePosition, MessageRing};
use crate::commit::{EditorOutcome, EditorRequest};
use crate::mode::ModeId;
use crate::saves::SaveIntent;
use crate::session::ReadKind;
use crate::view::BufferView;

#[cfg(test)]
#[path = "commit_editor_tests.rs"]
mod tests;

const ENDING: &str = "Finishing commit message; editing and new saves are temporarily disabled";

impl Global for MessageRing {}

#[derive(Clone)]
pub(super) struct EditorTarget {
    origin: Entity<BufferView>,
    input: crate::input::DisplayId,
}

impl EditorTarget {
    pub fn capture(shell: &AppShell, cx: &gpui_kit::App) -> Self {
        let origin = shell.active_view().clone();
        let input = origin.read(cx).display_snapshot().id();
        Self { origin, input }
    }

    pub(super) fn resume_after_read(&mut self, current: Self) {
        if self.origin == current.origin && self.input.committed == current.input.committed {
            self.input = current.input;
        }
    }

    pub(super) fn buffer_id(&self, cx: &gpui_kit::App) -> BufferId {
        self.origin.read(cx).buffer().snapshot_id().buffer
    }

    pub(super) fn is_current(&self, shell: &AppShell, cx: &gpui_kit::App) -> bool {
        shell.minibuffer.is_none()
            && shell.active_view() == &self.origin
            && self.origin.read(cx).display_snapshot().id() == self.input
            && self.origin.read(cx).git_open_ready()
    }
}

pub(super) struct Editor {
    buffer: Buffer,
    requests: Vec<(Option<u64>, EditorRequest)>,
    companions: Vec<(u64, BufferId)>,
    comment_prefix: String,
    position: MessagePosition,
    ending: Option<EditorOutcome>,
    previous_mode: ModeId,
    reload: Option<crate::files::LoadedFile>,
}

impl Drop for Editor {
    fn drop(&mut self) {
        self.buffer.set_edit_inhibition(None);
        self.buffer.set_mode(self.previous_mode);
    }
}

impl AppShell {
    pub fn edit_commit_message(
        &mut self,
        request: EditorRequest,
        comment_prefix: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = EditorTarget::capture(self, cx);
        self.edit_owned_commit_message(request, comment_prefix, None, target, window, cx);
    }

    pub(super) fn edit_owned_commit_message(
        &mut self,
        request: EditorRequest,
        comment_prefix: String,
        operation: Option<u64>,
        target: EditorTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let origin = target.origin.clone();
        let path = request.path.clone();
        cx.spawn_in(window, async move |shell, cx| {
            let loaded = cx
                .background_executor()
                .spawn(async move { crate::files::load(&path).map(|loaded| (path, loaded)) })
                .await;
            shell
                .update_in(cx, |shell, window, cx| {
                    if operation.is_some_and(|operation| !shell.commit_active(operation)) {
                        return;
                    }
                    match loaded {
                        Ok((path, loaded)) => {
                            let identity = loaded.stamp.and_then(crate::files::FileStamp::identity);
                            let existing = shell.buffers.iter().position(|slot| {
                                slot.buffer.path().as_ref() == Some(&path)
                                    || (identity.is_some() && slot.buffer.identity() == identity)
                            });
                            let (index, reload) = match existing {
                                Some(index) => {
                                    let changed =
                                        shell.buffers[index].buffer.disk_stamp() != loaded.stamp;
                                    (index, changed.then_some(loaded))
                                }
                                None => {
                                    let buffer = Buffer::visiting(path, loaded.file, loaded.stamp);
                                    (shell.create_buffer(buffer, window, cx), None)
                                }
                            };
                            shell.attach_commit_editor(
                                index,
                                request,
                                comment_prefix,
                                operation,
                                cx,
                            );
                            if let Some(reload) = reload {
                                let editor = shell
                                    .commit_editors
                                    .get_mut(&shell.buffers[index].id())
                                    .unwrap();
                                if editor.ending.is_none() {
                                    editor.reload = Some(reload);
                                }
                            }
                            if target.is_current(shell, cx) {
                                shell.display_temporary(index, window, cx);
                                let view = shell.active_view().clone();
                                if !shell.query_commit_reload(&view, None, window, cx)
                                    && let Some(operation) = operation
                                {
                                    shell.show_commit_diff(operation, index, window, cx);
                                }
                            } else {
                                shell.show_message(
                                    &origin,
                                    "Git is waiting for its commit message buffer",
                                    cx,
                                );
                            }
                        }
                        Err(error) => {
                            let _ = request.respond(EditorOutcome::Cancel);
                            shell.show_message(
                                &origin,
                                format!("Cannot open Git editor file: {error}"),
                                cx,
                            );
                        }
                    }
                })
                .ok();
        })
        .detach();
    }

    fn show_commit_diff(
        &mut self,
        operation: u64,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let session = self
            .commits
            .iter()
            .find(|session| session.record.operation == operation)
            .unwrap();
        let repository = session.record.repository.clone();
        let kind = if session.scope == crate::commit::Scope::All {
            crate::diff::DiffKind::Worktree
        } else {
            crate::diff::DiffKind::Staged
        };
        let editor_id = self.buffers[index].id();
        let comparison = crate::diff::Comparison {
            repository: repository.clone(),
            kind,
            environment: crate::diff::ReadEnvironment::Commit,
        };
        let existing = self
            .buffers
            .iter()
            .position(|slot| slot.buffer.comparison().as_ref() == Some(&comparison));
        let diff_index = existing
            .unwrap_or_else(|| self.create_buffer(Buffer::diff(comparison.clone()), window, cx));
        let diff_id = self.buffers[diff_index].id();
        let request = self.buffers[diff_index]
            .buffer
            .begin_diff_request()
            .unwrap()
            .1;
        let origin = self.active_view().clone();
        let input = origin.read(cx).display_snapshot().id().session;
        cx.spawn_in(window, async move |shell, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let data = crate::diff::Diff::load(
                        comparison,
                        &std::sync::atomic::AtomicBool::new(false),
                    )?;
                    let rendered = data.render()?;
                    Ok::<_, String>((data, rendered))
                })
                .await;
            shell
                .update_in(cx, |shell, window, cx| {
                    if !shell.commit_active(operation)
                        || !shell.commit_editors.contains_key(&editor_id)
                    {
                        return;
                    }
                    match result {
                        Ok((data, rendered)) => {
                            let Some(index) = shell.buffers.index_of(diff_id) else {
                                return;
                            };
                            if !shell.install_diff_result(index, request, data, rendered, cx) {
                                return;
                            }
                            if shell.minibuffer.is_none()
                                && shell.active_view() == &origin
                                && origin.read(cx).display_snapshot().id().session == input
                                && origin.read(cx).git_open_ready()
                                && let Some(companion) =
                                    shell.display_temporary_beside(index, &repository, window, cx)
                            {
                                shell
                                    .commit_editors
                                    .get_mut(&editor_id)
                                    .unwrap()
                                    .companions
                                    .push(companion);
                            }
                        }
                        Err(error) => shell.show_message(
                            &origin,
                            format!("Cannot show commit diff: {error}"),
                            cx,
                        ),
                    }
                })
                .ok();
        })
        .detach();
    }

    fn attach_commit_editor(
        &mut self,
        index: usize,
        request: EditorRequest,
        comment_prefix: String,
        operation: Option<u64>,
        cx: &mut Context<Self>,
    ) {
        let buffer = &mut self.buffers[index].buffer;
        let id = buffer.snapshot_id().buffer;
        cx.default_global::<MessageRing>()
            .save(&buffer.text().to_string(), &comment_prefix);
        match self.commit_editors.entry(id) {
            Entry::Occupied(mut entry) => entry.get_mut().requests.push((operation, request)),
            Entry::Vacant(entry) => {
                entry.insert(Editor {
                    buffer: buffer.new_view(),
                    requests: vec![(operation, request)],
                    companions: Vec::new(),
                    comment_prefix,
                    position: MessagePosition::default(),
                    ending: None,
                    previous_mode: buffer.mode(),
                    reload: None,
                });
                buffer.set_mode(ModeId::GIT_COMMIT);
            }
        }
        self.refresh_buffer(id, cx);
    }

    fn query_commit_reload(
        &mut self,
        origin: &Entity<BufferView>,
        outcome: Option<EditorOutcome>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let buffer = origin.read(cx).buffer();
        let Some(index) = self.buffers.index_of(buffer.snapshot_id().buffer) else {
            return false;
        };
        if self.buffers[index].save.is_saving()
            || !self
                .commit_editors
                .get(&buffer.snapshot_id().buffer)
                .is_some_and(|editor| editor.reload.is_some())
        {
            return false;
        }
        self.begin_read(
            ReadKind::CommitReload {
                snapshot: buffer.snapshot_id(),
                modified: buffer.is_modified(),
                outcome,
            },
            origin,
            "",
            window,
            cx,
        );
        true
    }

    pub(super) fn answer_commit_reload(
        &mut self,
        origin: &Entity<BufferView>,
        snapshot: crate::buffer::SnapshotId,
        reload: bool,
        outcome: Option<EditorOutcome>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.buffer_index(origin, cx) else {
            return;
        };
        let id = self.buffers[index].id();
        if self.buffers[index].buffer.snapshot_id() != snapshot {
            self.query_commit_reload(origin, outcome, window, cx);
            return;
        }
        if self.buffers[index].save.is_saving() {
            return;
        }
        let Some(editor) = self.commit_editors.get_mut(&id) else {
            return;
        };
        if let Some(loaded) = editor.reload.take()
            && reload
        {
            self.buffers[index].buffer.reload_visited(loaded);
            self.reset_commit_input(id, cx);
        }
        if let Some(outcome) = outcome {
            self.finish_commit_editor(origin, outcome, false, window, cx);
        } else if self.active_view() == origin && self.minibuffer.is_none() {
            let operations: Vec<_> = self.commit_editors[&id]
                .requests
                .iter()
                .filter_map(|(operation, _)| *operation)
                .collect();
            for operation in operations {
                if self.commit_active(operation) {
                    self.show_commit_diff(operation, index, window, cx);
                }
            }
        }
    }

    pub(super) fn commit_editor_repository(&self, id: BufferId) -> Option<crate::git::Repository> {
        self.commit_editors
            .get(&id)?
            .requests
            .iter()
            .find_map(|(operation, _)| {
                let operation = operation.as_ref()?;
                self.commits
                    .iter()
                    .find(|session| session.record.operation == *operation)
                    .map(|session| session.record.repository.clone())
            })
    }

    pub(super) fn commit_editor_command(
        &mut self,
        command: Command,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match command {
            Command::CommitFinish | Command::CommitCancel => self.finish_commit_editor(
                origin,
                if command == Command::CommitFinish {
                    EditorOutcome::Finish
                } else {
                    EditorOutcome::Cancel
                },
                false,
                window,
                cx,
            ),
            _ => {
                let buffer = origin.read(cx).buffer();
                let id = buffer.snapshot_id().buffer;
                if let Some(reason) = buffer.edit_inhibition() {
                    self.show_message(origin, reason, cx);
                    return;
                }
                if !origin.read(cx).git_open_ready() {
                    return;
                }
                let text = buffer.text().to_string();
                let Some(editor) = self.commit_editors.get_mut(&id) else {
                    return;
                };
                if command == Command::CommitSaveMessage {
                    let saved = cx
                        .default_global::<MessageRing>()
                        .save(&text, &editor.comment_prefix);
                    self.show_message(
                        origin,
                        if saved {
                            "Message saved"
                        } else {
                            "Only whitespace and/or comments; message not saved"
                        },
                        cx,
                    );
                } else {
                    let recalled = cx.default_global::<MessageRing>().recall(
                        &mut editor.position,
                        &text,
                        &editor.comment_prefix,
                        if command == Command::CommitPreviousMessage {
                            1
                        } else {
                            -1
                        },
                    );
                    match recalled {
                        Some(recalled) => {
                            let point = crate::text::ByteOffset(
                                recalled.range.start.0 + recalled.text.len(),
                            );
                            let result = origin.update(cx, |view, cx| {
                                view.replace_range(recalled.range, &recalled.text, window, cx)?;
                                view.show_location(point, window, cx);
                                Ok::<_, crate::editing::RouteError>(())
                            });
                            self.show_message(
                                origin,
                                result.map_or_else(
                                    |error| error.to_string(),
                                    |_| format!("Comment {}", recalled.number),
                                ),
                                cx,
                            );
                        }
                        None => self.show_message(origin, "Empty comment ring", cx),
                    }
                }
            }
        }
    }

    pub(super) fn finish_commit_editor(
        &mut self,
        origin: &Entity<BufferView>,
        outcome: EditorOutcome,
        confirmed: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.buffer_index(origin, cx) else {
            return;
        };
        let id = self.buffers[index].id();
        let Some(editor) = self.commit_editors.get(&id) else {
            return;
        };
        if editor.ending.is_some() {
            self.show_message(origin, ENDING, cx);
            return;
        }
        if self.minibuffer.is_some() || !origin.read(cx).git_open_ready() {
            return;
        }
        if self.query_commit_reload(origin, Some(outcome), window, cx) {
            return;
        }
        let editor = self.commit_editors.get(&id).unwrap();
        let text = self.buffers[index].buffer.text().to_string();
        if matches!(outcome, EditorOutcome::Finish)
            && !confirmed
            && needs_second_line_confirmation(&text, &editor.comment_prefix)
        {
            self.begin_read(ReadKind::CommitSecondLine, origin, "", window, cx);
            return;
        }
        cx.default_global::<MessageRing>()
            .save(&text, &editor.comment_prefix);
        self.commit_editors.get_mut(&id).unwrap().ending = Some(outcome);
        self.buffers[index].buffer.set_edit_inhibition(Some(ENDING));
        self.reset_commit_input(id, cx);
        self.show_message(origin, ENDING, cx);
        if !self.buffers[index].save.is_saving() && !self.buffers[index].buffer.is_modified() {
            self.after_save(origin, true, String::new(), window, cx);
        } else {
            self.enqueue_save(origin, SaveIntent::Current, window, cx);
        }
    }

    pub(super) fn release_commit_editors(
        &mut self,
        operation: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ids: Vec<_> = self.commit_editors.keys().copied().collect();
        for id in ids {
            let editor = self.commit_editors.get_mut(&id).unwrap();
            editor
                .requests
                .retain(|(owner, _)| *owner != Some(operation));
            if !editor.requests.is_empty() {
                continue;
            }
            let editor = self.commit_editors.remove(&id).unwrap();
            if let Some(read) = self
                .session
                .as_ref()
                .filter(|session| {
                    matches!(session.kind(), ReadKind::CommitReload { .. })
                        && session.origin().read(cx).buffer().snapshot_id().buffer == id
                })
                .map(|session| session.id())
            {
                self.end_session(read, window, cx);
            }
            cx.default_global::<MessageRing>()
                .save(&editor.buffer.text().to_string(), &editor.comment_prefix);
            drop(editor);
            self.reset_commit_input(id, cx);
        }
    }

    fn reset_commit_input(&mut self, id: BufferId, cx: &mut Context<Self>) {
        for view in self.all_views() {
            if view.read(cx).buffer().snapshot_id().buffer == id {
                view.update(cx, |view, cx| view.reset_input(cx));
            }
        }
        self.refresh_buffer(id, cx);
    }

    pub(super) fn cancel_commit_ending(&mut self, index: usize, cx: &mut Context<Self>) {
        let id = self.buffers[index].id();
        if let Some(editor) = self.commit_editors.get_mut(&id)
            && editor.ending.take().is_some()
        {
            self.buffers[index].buffer.set_edit_inhibition(None);
            self.reset_commit_input(id, cx);
        }
    }

    pub(super) fn after_commit_save(
        &mut self,
        id: BufferId,
        saved: bool,
        message: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> String {
        let Some(index) = self.buffers.index_of(id) else {
            return message;
        };
        if saved && let Some(editor) = self.commit_editors.get_mut(&id) {
            editor.reload = None;
        }
        let Some(outcome) = self
            .commit_editors
            .get(&id)
            .and_then(|editor| editor.ending)
        else {
            return message;
        };
        if !saved {
            self.cancel_commit_ending(index, cx);
            return message;
        }
        let mut editor = self.commit_editors.remove(&id).unwrap();
        let errors: Vec<_> = std::mem::take(&mut editor.requests)
            .into_iter()
            .filter_map(|(_, request)| request.respond(outcome).err())
            .collect();
        let companions = std::mem::take(&mut editor.companions);
        drop(editor);
        if !errors.is_empty() {
            self.reset_commit_input(id, cx);
            return format!("Git editor reply failed: {}", errors[0]);
        }
        for (pane, buffer) in companions {
            self.quit_commit_companion(pane, buffer, id, window, cx);
        }
        let panes: Vec<_> = self
            .panes
            .leaves()
            .into_iter()
            .filter(|pane| self.body_panes[pane].current == id)
            .collect();
        for pane in panes {
            self.quit_temporary_pane(pane, window, cx);
        }
        if let Some(index) = self.buffers.index_of(id) {
            self.remove_buffer(index, window, cx);
        }
        match outcome {
            EditorOutcome::Finish => "Commit message finished; Git is continuing".into(),
            EditorOutcome::Cancel => "Commit canceled; message saved to history".into(),
        }
    }
}

fn needs_second_line_confirmation(text: &str, prefix: &str) -> bool {
    let comment = |line: &str| !prefix.is_empty() && line.starts_with(prefix);
    let mut lines = text
        .split('\n')
        .skip_while(|line| line.trim().is_empty() || comment(line));
    lines.next().is_some_and(|line| !line.is_empty())
        && lines
            .next()
            .is_some_and(|line| !line.is_empty() && !comment(line))
}
