use std::io;
use std::path::PathBuf;
use std::time::Duration;

use gpui_kit::{App, Context, Entity, Window};

use super::AppShell;
use crate::buffer::BufferId;
use crate::files::{self, FileStamp, ReadError};
use crate::revert::{AutoRevertLocal, CHANGE_DEBOUNCE_MS, FOCUS_DEBOUNCE_MS, RevertOutcome};
use crate::session::ReadKind;
use crate::view::BufferView;

#[cfg(test)]
#[path = "revert_tests.rs"]
mod tests;

impl AppShell {
    fn is_commit_editor(&self, id: BufferId) -> bool {
        #[cfg(unix)]
        {
            self.commit_editors.contains_key(&id)
        }
        #[cfg(not(unix))]
        {
            let _ = id;
            false
        }
    }

    pub(super) fn visible_buffer_ids(&self) -> Vec<BufferId> {
        let side = self.panes.side().copied();
        let mut ids = Vec::new();
        for (pane_id, pane) in &self.body_panes {
            if Some(*pane_id) == side {
                continue;
            }
            if !ids.contains(&pane.current) {
                ids.push(pane.current);
            }
        }
        ids
    }

    fn has_visible_auto_revert_file(&self) -> bool {
        self.visible_buffer_ids().into_iter().any(|id| {
            self.buffers.index_of(id).is_some_and(|index| {
                let slot = &self.buffers[index];
                slot.revert.local.is_enabled(self.auto_revert.enabled)
                    && slot.buffer.path().is_some()
                    && !self.is_commit_editor(id)
            })
        })
    }

    fn buffer_has_active_preedit(&self, id: BufferId, cx: &App) -> bool {
        self.body_panes.values().any(|pane| {
            pane.editors.get(&id).is_some_and(|mount| {
                mount
                    .view
                    .read(cx)
                    .display_snapshot()
                    .marked_range()
                    .is_some()
            })
        })
    }

    fn is_auto_revert_eligible(&self, index: usize) -> bool {
        let slot = &self.buffers[index];
        slot.revert.local.is_enabled(self.auto_revert.enabled)
            && slot.buffer.path().is_some()
            && !slot.buffer.is_modified()
            && slot.buffer.edit_inhibition().is_none()
            && !slot.save.is_saving()
            && !self.is_commit_editor(slot.id())
    }

    pub(super) fn revert_buffer_command(
        &mut self,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.buffer_index(origin, cx) else {
            return;
        };
        if let Some(reason) = self.buffers[index].buffer.edit_inhibition() {
            self.show_message(origin, reason, cx);
            return;
        }
        let Some(path) = self.buffers[index].buffer.path() else {
            self.show_message(
                origin,
                "Buffer does not seem to be associated with any file",
                cx,
            );
            return;
        };
        if self.buffers[index].save.is_saving() {
            self.show_message(origin, "Save in progress; wait before reverting", cx);
            return;
        }
        let modified = self.buffers[index].buffer.is_modified();
        self.begin_read(
            ReadKind::RevertBuffer { path, modified },
            origin,
            "",
            window,
            cx,
        );
    }

    pub(super) fn start_manual_revert(
        &mut self,
        target: &Entity<BufferView>,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.buffer_index(target, cx) else {
            return;
        };
        if let Some(reason) = self.buffers[index].buffer.edit_inhibition() {
            self.show_message(target, reason, cx);
            return;
        }
        let Some(path) = self.buffers[index].buffer.path() else {
            self.show_message(
                target,
                "Buffer does not seem to be associated with any file",
                cx,
            );
            return;
        };
        if self.buffers[index].save.is_saving() {
            self.show_message(target, "Save in progress; wait before reverting", cx);
            return;
        }

        let id = self.buffers[index].id();
        let existed_before = self.buffers[index].buffer.disk_stamp().is_some();
        let serial = self.buffers[index].revert.next_generation();
        self.buffers[index].revert.failed_stamp = None;
        self.buffers[index].revert.deferred = false;
        let load_path = path.clone();
        #[cfg(test)]
        let gate = self.revert_read_gate.take();
        let window_handle = self.window_handle;

        let task = cx.spawn(async move |shell, cx| {
            #[cfg(test)]
            if let Some(gate) = gate {
                let _ = gate.await;
            }
            let result = cx
                .background_executor()
                .spawn(async move { files::load(&load_path) })
                .await;
            let mut refresh_tree_git = false;
            let _ = shell.update(cx, |shell, cx| {
                let Some(index) = shell.buffers.index_of(id) else {
                    return;
                };
                if shell.buffers[index].revert.generation != serial {
                    return;
                }
                shell.buffers[index].revert.task = None;
                let view = shell.active_view().clone();
                if shell.buffers[index].buffer.path().as_ref() != Some(&path) {
                    return;
                }
                if let Some(reason) = shell.buffers[index].buffer.edit_inhibition() {
                    shell.show_message(&view, reason, cx);
                    return;
                }
                if shell.buffers[index].save.is_saving() {
                    shell.show_message(&view, "Save in progress; wait before reverting", cx);
                    return;
                }
                match result {
                    Ok(loaded) => {
                        shell.buffers[index].revert.failed_stamp = None;
                        let outcome = shell.buffers[index].buffer.revert_visited(loaded, false);
                        if outcome == RevertOutcome::Reverted {
                            shell.refresh_buffer(id, cx);
                            refresh_tree_git = shell.panes.side().is_some();
                        } else {
                            cx.notify();
                        }
                        shell.show_message(&view, format!("Reverted {}", path.display()), cx);
                    }
                    Err(ReadError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
                        let message = if existed_before {
                            format!("File {} no longer exists!", path.display())
                        } else {
                            format!("Cannot revert nonexistent file {}", path.display())
                        };
                        shell.show_message(&view, message, cx);
                    }
                    Err(ReadError::Io(error))
                        if error.kind() == io::ErrorKind::PermissionDenied =>
                    {
                        let message = if existed_before {
                            format!("File {} no longer readable!", path.display())
                        } else {
                            format!("Cannot revert unreadable file {}", path.display())
                        };
                        shell.show_message(&view, message, cx);
                    }
                    Err(error) => {
                        shell.show_message(
                            &view,
                            format!("Cannot revert {}: {error}", path.display()),
                            cx,
                        );
                    }
                }
            });
            if refresh_tree_git {
                let _ = window_handle.update(cx, |_, window, cx| {
                    let _ = shell.update(cx, |shell, cx| {
                        shell.schedule_tree_git_refresh(200, window, cx);
                    });
                });
            }
        });
        self.buffers[index].revert.task = Some(task);
    }

    pub(super) fn toggle_auto_revert_mode(
        &mut self,
        origin: &Entity<BufferView>,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.buffer_index(origin, cx) else {
            return;
        };
        let id = self.buffers[index].id();
        let was_enabled = self.buffers[index]
            .revert
            .local
            .is_enabled(self.auto_revert.enabled);
        if was_enabled {
            self.buffers[index].revert.local = if self.auto_revert.enabled {
                AutoRevertLocal::Disabled
            } else {
                AutoRevertLocal::Inherit
            };
            self.buffers[index].revert.task = None;
            self.buffers[index].revert.next_generation();
            self.buffers[index].revert.deferred = false;
            self.show_message(origin, "Auto-Revert mode disabled in current buffer", cx);
        } else {
            self.buffers[index].revert.local = AutoRevertLocal::Enabled;
            self.buffers[index].revert.failed_stamp = None;
            self.show_message(origin, "Auto-Revert mode enabled in current buffer", cx);
            if self.visible_buffer_ids().contains(&id) {
                self.check_auto_revert_buffers(vec![id], 0, cx);
            }
        }
        self.sync_auto_revert_timer(cx);
        cx.notify();
    }

    pub(super) fn toggle_global_auto_revert_mode(
        &mut self,
        origin: &Entity<BufferView>,
        cx: &mut Context<Self>,
    ) {
        self.auto_revert.enabled = !self.auto_revert.enabled;
        if self.auto_revert.enabled {
            self.show_message(origin, "Global Auto-Revert mode enabled", cx);
            let ids = self.visible_buffer_ids();
            if !ids.is_empty() {
                self.check_auto_revert_buffers(ids, 0, cx);
            }
        } else {
            for slot in self.buffers.iter_mut() {
                if slot.revert.local == AutoRevertLocal::Inherit {
                    slot.revert.task = None;
                    slot.revert.next_generation();
                    slot.revert.deferred = false;
                }
            }
            self.show_message(origin, "Global Auto-Revert mode disabled", cx);
        }
        self.sync_auto_revert_timer(cx);
        cx.notify();
    }

    pub(super) fn sync_auto_revert_after_config(&mut self, cx: &mut Context<Self>) {
        if !self.has_visible_auto_revert_file() || self.exiting() {
            for slot in self.buffers.iter_mut() {
                if !slot.revert.local.is_enabled(self.auto_revert.enabled) {
                    slot.revert.task = None;
                    slot.revert.next_generation();
                    slot.revert.deferred = false;
                }
            }
        } else {
            let ids = self.visible_buffer_ids();
            self.check_auto_revert_buffers(ids, 0, cx);
        }
        self.sync_auto_revert_timer(cx);
        cx.notify();
    }

    pub(super) fn sync_auto_revert_timer(&mut self, cx: &mut Context<Self>) {
        if !self.has_visible_auto_revert_file() || self.exiting() {
            self.auto_revert_poll = None;
            self.auto_revert_focus_task = None;
            return;
        }
        if self.auto_revert_poll.is_some()
            && self.auto_revert_poll_interval_ms == self.auto_revert.interval_ms
        {
            return;
        }
        let interval = Duration::from_millis(self.auto_revert.interval_ms);
        self.auto_revert_poll_interval_ms = self.auto_revert.interval_ms;
        self.auto_revert_poll = Some(cx.spawn(async move |shell, cx| {
            loop {
                cx.background_executor().timer(interval).await;
                let keep = shell
                    .update(cx, |shell, cx| {
                        if !shell.has_visible_auto_revert_file() || shell.exiting() {
                            shell.auto_revert_poll = None;
                            return false;
                        }
                        shell.poll_visible_auto_revert(cx);
                        true
                    })
                    .unwrap_or(false);
                if !keep {
                    break;
                }
            }
        }));
    }

    fn poll_visible_auto_revert(&mut self, cx: &mut Context<Self>) {
        let ids = self.visible_buffer_ids();
        self.check_auto_revert_buffers(ids, CHANGE_DEBOUNCE_MS, cx);
    }

    pub(super) fn schedule_focus_auto_revert(&mut self, cx: &mut Context<Self>) {
        if !self.has_visible_auto_revert_file() || self.exiting() {
            self.auto_revert_focus_task = None;
            return;
        }
        self.auto_revert_focus_task = Some(cx.spawn(async move |shell, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(FOCUS_DEBOUNCE_MS))
                .await;
            let _ = shell.update(cx, |shell, cx| {
                shell.auto_revert_focus_task = None;
                let ids = shell.visible_buffer_ids();
                shell.check_auto_revert_buffers(ids, 0, cx);
            });
        }));
    }

    pub(super) fn on_visible_buffers_updated(&mut self, cx: &mut Context<Self>) {
        let current = self.visible_buffer_ids();
        let newly_visible: Vec<BufferId> = current
            .iter()
            .copied()
            .filter(|id| !self.visible_buffers.contains(id))
            .collect();
        self.visible_buffers = current.iter().copied().collect();
        self.sync_auto_revert_timer(cx);
        if self.exiting() {
            return;
        }
        let mut to_check = newly_visible;
        for id in current {
            if !to_check.contains(&id)
                && self
                    .buffers
                    .index_of(id)
                    .is_some_and(|index| self.buffers[index].revert.deferred)
            {
                to_check.push(id);
            }
        }
        if !to_check.is_empty() {
            self.check_auto_revert_buffers(to_check, 0, cx);
        }
    }

    fn auto_revert_ui_blocked(&self) -> bool {
        self.minibuffer.is_some()
            || self.session.is_some()
            || self.isearch.is_some()
            || self.git_task.is_some()
            || self.script_task.is_some()
            || self.file_task.is_some()
            || self.opening.is_some()
            || self.reload_task.is_some()
    }

    pub(super) fn flush_deferred_auto_revert(&mut self, cx: &mut Context<Self>) {
        if self.exiting() || self.auto_revert_stat_in_flight || self.auto_revert_ui_blocked() {
            return;
        }
        let ids: Vec<BufferId> = self
            .visible_buffer_ids()
            .into_iter()
            .filter(|id| {
                self.buffers
                    .index_of(*id)
                    .is_some_and(|index| self.buffers[index].revert.deferred)
                    && !self.buffer_has_active_preedit(*id, cx)
            })
            .collect();
        if !ids.is_empty() {
            self.check_auto_revert_buffers(ids, 0, cx);
        }
    }

    pub(super) fn check_auto_revert_buffers(
        &mut self,
        ids: Vec<BufferId>,
        debounce_ms: u64,
        cx: &mut Context<Self>,
    ) {
        if self.exiting() || ids.is_empty() {
            return;
        }
        if self.auto_revert_ui_blocked() || self.auto_revert_stat_in_flight {
            for id in ids {
                if let Some(index) = self.buffers.index_of(id)
                    && self.is_auto_revert_eligible(index)
                    && self.buffers[index].revert.task.is_none()
                {
                    self.buffers[index].revert.deferred = true;
                }
            }
            return;
        }

        let mut targets: Vec<(BufferId, PathBuf, Option<FileStamp>, Option<FileStamp>)> =
            Vec::new();
        for id in ids {
            let Some(index) = self.buffers.index_of(id) else {
                continue;
            };
            if !self.is_auto_revert_eligible(index) || self.buffers[index].revert.task.is_some() {
                continue;
            }
            if self.buffer_has_active_preedit(id, cx) {
                self.buffers[index].revert.deferred = true;
                continue;
            }
            self.buffers[index].revert.deferred = false;
            if let Some(path) = self.buffers[index].buffer.path() {
                targets.push((
                    id,
                    path,
                    self.buffers[index].buffer.disk_stamp(),
                    self.buffers[index].revert.failed_stamp,
                ));
            }
        }

        if targets.is_empty() {
            return;
        }

        self.auto_revert_stat_in_flight = true;
        cx.spawn(async move |shell, cx| {
            let changed = cx
                .background_executor()
                .spawn(async move {
                    let mut changed = Vec::new();
                    for (id, path, known_stamp, failed_stamp) in targets {
                        if let Ok(Some(disk_stamp)) = files::stamp(&path)
                            && Some(disk_stamp) != known_stamp
                            && Some(disk_stamp) != failed_stamp
                        {
                            changed.push((id, disk_stamp));
                        }
                    }
                    changed
                })
                .await;
            let _ = shell.update(cx, |shell, cx| {
                shell.auto_revert_stat_in_flight = false;
                if !changed.is_empty() {
                    for (id, observed_stamp) in changed {
                        shell.schedule_auto_revert_load(id, observed_stamp, debounce_ms, cx);
                    }
                }
                shell.flush_deferred_auto_revert(cx);
            });
        })
        .detach();
    }

    fn schedule_auto_revert_load(
        &mut self,
        id: BufferId,
        observed_stamp: FileStamp,
        debounce_ms: u64,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.buffers.index_of(id) else {
            return;
        };
        if !self.visible_buffer_ids().contains(&id) || !self.is_auto_revert_eligible(index) {
            return;
        }
        if self.auto_revert_ui_blocked() || self.buffer_has_active_preedit(id, cx) {
            self.buffers[index].revert.deferred = true;
            return;
        }
        let Some(path) = self.buffers[index].buffer.path() else {
            return;
        };
        let serial = self.buffers[index].revert.next_generation();
        #[cfg(test)]
        let gate = self.revert_read_gate.take();
        let window_handle = self.window_handle;

        let task = cx.spawn(async move |shell, cx| {
            if debounce_ms > 0 {
                cx.background_executor()
                    .timer(Duration::from_millis(debounce_ms))
                    .await;
            }
            #[cfg(test)]
            if let Some(gate) = gate {
                let _ = gate.await;
            }
            let result = cx
                .background_executor()
                .spawn(async move { files::load(&path).map(|loaded| (path, loaded)) })
                .await;
            let mut refresh_tree_git = false;
            let _ = shell.update(cx, |shell, cx| {
                let Some(index) = shell.buffers.index_of(id) else {
                    return;
                };
                if shell.buffers[index].revert.generation != serial {
                    return;
                }
                shell.buffers[index].revert.task = None;

                if !shell.visible_buffer_ids().contains(&id)
                    || !shell.is_auto_revert_eligible(index)
                {
                    return;
                }
                if shell.auto_revert_ui_blocked() || shell.buffer_has_active_preedit(id, cx) {
                    shell.buffers[index].revert.deferred = true;
                    return;
                }
                match result {
                    Ok((load_path, loaded)) => {
                        if shell.buffers[index].buffer.path().as_ref() != Some(&load_path) {
                            return;
                        }
                        shell.buffers[index].revert.failed_stamp = None;
                        let old_len = shell.buffers[index].buffer.text().len();
                        let new_len = loaded.file.text.len();
                        let eob_views: Vec<_> = if new_len != old_len {
                            shell
                                .all_views()
                                .into_iter()
                                .filter(|view| {
                                    let buffer = view.read(cx).buffer();
                                    buffer.snapshot_id().buffer == id && buffer.point().0 == old_len
                                })
                                .collect()
                        } else {
                            Vec::new()
                        };
                        let outcome = shell.buffers[index].buffer.revert_visited(loaded, true);
                        if outcome == RevertOutcome::Reverted {
                            shell.refresh_buffer(id, cx);
                            for view in eob_views {
                                view.update(cx, |view, cx| view.show_end(cx));
                            }
                            refresh_tree_git = shell.panes.side().is_some();
                            if shell.auto_revert.verbose {
                                let name = shell.buffers[index].buffer.display_name();
                                let target = shell.active_view().clone();
                                shell.show_message(
                                    &target,
                                    format!("Reverting buffer ‘{name}’"),
                                    cx,
                                );
                            }
                        }
                    }
                    Err(_) => {
                        shell.buffers[index].revert.failed_stamp = Some(observed_stamp);
                    }
                }
            });
            if refresh_tree_git {
                let _ = window_handle.update(cx, |_, window, cx| {
                    let _ = shell.update(cx, |shell, cx| {
                        shell.schedule_tree_git_refresh(200, window, cx);
                    });
                });
            }
        });
        self.buffers[index].revert.task = Some(task);
    }
}
