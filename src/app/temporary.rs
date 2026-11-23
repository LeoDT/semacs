use gpui_kit::{Context, Window};

use super::AppShell;
use crate::buffer::{Buffer, BufferId};
use crate::panes::Axis;

#[cfg(test)]
#[path = "temporary_tests.rs"]
mod tests;

pub(super) struct QuitRestore {
    pub buffer: BufferId,
    owner: Option<BufferId>,
    previous: Option<BufferId>,
    pub(super) source: u64,
}

impl AppShell {
    /// Target pane for `other-window` file/blob visits (Emacs `switch-to-buffer-other-window`).
    /// Reuses another ordinary pane already displaying `buffer`, the recorded origin pane, or
    /// another existing ordinary pane; splits only when the selected pane is the only ordinary pane.
    pub(super) fn other_window_pane(
        &mut self,
        buffer: BufferId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<u64> {
        let source = *self.panes.selected();
        let ordinary = self.panes.ordinary_leaves();
        if let Some(pane) = ordinary
            .iter()
            .copied()
            .find(|pane| *pane != source && self.body_panes[pane].current == buffer)
        {
            return Some(pane);
        }
        if let Some(body) = self.body_panes.get(&source)
            && let Some(origin_pane) = body.quit_restore.iter().rev().find_map(|restore| {
                (restore.source != source
                    && self.body_panes.contains_key(&restore.source)
                    && self.panes.side() != Some(&restore.source))
                .then_some(restore.source)
            })
        {
            return Some(origin_pane);
        }
        if let Some(other) = ordinary.iter().copied().find(|pane| *pane != source) {
            return Some(other);
        }
        if self.panes.side() != Some(&source) {
            let axis = if self.check_split(Axis::Vertical, cx).is_ok() {
                Some(Axis::Vertical)
            } else if self.check_split(Axis::Horizontal, cx).is_ok() {
                Some(Axis::Horizontal)
            } else {
                None
            };
            if let Some(axis) = axis {
                let (buf, scroll) = self.active_view().read(cx).split_state();
                let view = Self::create_view(buf, window, cx);
                view.update(cx, |view, cx| view.initialize_scroll(scroll, cx));
                return Some(self.split_selected(view, axis, window, cx));
            }
        }
        None
    }

    pub(super) fn display_temporary(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.minibuffer.is_some() {
            return;
        }
        if self.panes.side() == Some(self.panes.selected()) {
            let pane = self.panes.ordinary_leaves()[0];
            self.select_pane(pane, window, cx);
        }
        let buffer = self.buffers[index].id();
        let source = *self.panes.selected();
        if self.active_buffer_id() == buffer {
            return;
        }
        if let Some(pane) = self
            .panes
            .leaves()
            .into_iter()
            .find(|pane| self.body_panes[pane].current == buffer)
        {
            self.select_pane(pane, window, cx);
            return;
        }
        if let Some(other) = self
            .panes
            .ordinary_leaves()
            .into_iter()
            .find(|pane| *pane != source)
        {
            let previous = self.body_panes[&other].current;
            if self.show_buffer_in_pane(other, index, true, window, cx) {
                let pane = self.body_panes.get_mut(&other).unwrap();
                pane.quit_restore.retain(|restore| restore.buffer != buffer);
                pane.quit_restore.push(QuitRestore {
                    buffer,
                    owner: None,
                    previous: Some(previous),
                    source,
                });
            }
            return;
        }
        let previous = self.active_buffer_id();
        let split = self.check_split(Axis::Vertical, cx).is_ok();
        if split {
            self.cancel_file_open();
            let view = Self::create_view(self.buffers[index].buffer.new_view(), window, cx);
            let pane = self.split_selected(view, Axis::Vertical, window, cx);
            self.body_panes
                .get_mut(&pane)
                .unwrap()
                .quit_restore
                .push(QuitRestore {
                    buffer,
                    owner: None,
                    previous: None,
                    source,
                });
            self.select_pane(pane, window, cx);
        } else {
            self.activate(index, window, cx);
            let pane = self.body_panes.get_mut(&source).unwrap();
            pane.quit_restore.retain(|restore| restore.buffer != buffer);
            pane.quit_restore.push(QuitRestore {
                buffer,
                owner: None,
                previous: Some(previous),
                source,
            });
        }
    }

    #[cfg(unix)]
    pub(super) fn display_temporary_beside(
        &mut self,
        index: usize,
        repository: &crate::git::Repository,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<(u64, BufferId)> {
        if self.minibuffer.is_some() {
            return None;
        }
        let buffer = self.buffers[index].id();
        if self.body_panes.values().any(|pane| pane.current == buffer) {
            return None;
        }
        let source = *self.panes.selected();
        let owner = Some(self.active_buffer_id());
        let reusable = self.panes.ordinary_leaves().into_iter().find(|pane| {
            *pane != source
                && self
                    .buffers
                    .index_of(self.body_panes[pane].current)
                    .is_some_and(|index| {
                        self.buffers[index].buffer.repository().as_ref() == Some(repository)
                    })
        });
        let pane = if let Some(pane_id) = reusable {
            let previous = self.body_panes[&pane_id].current;
            if !self.body_panes[&pane_id].editors.contains_key(&buffer) {
                let view = Self::create_view(self.buffers[index].buffer.new_view(), window, cx);
                self.cache_view(pane_id, buffer, view, cx);
            }
            self.body_panes[&pane_id]
                .active_view()
                .clone()
                .update(cx, |view, cx| view.suspend_input(cx));
            let pane = self.body_panes.get_mut(&pane_id).unwrap();
            pane.show_buffer(buffer);
            pane.quit_restore.retain(|restore| restore.buffer != buffer);
            pane.quit_restore.push(QuitRestore {
                buffer,
                owner,
                previous: Some(previous),
                source,
            });
            self.update_selected_views(window, cx);
            pane_id
        } else {
            self.check_split(Axis::Vertical, cx).ok()?;
            let view = Self::create_view(self.buffers[index].buffer.new_view(), window, cx);
            let pane = self.split_selected(view, Axis::Vertical, window, cx);
            self.body_panes
                .get_mut(&pane)
                .unwrap()
                .quit_restore
                .push(QuitRestore {
                    buffer,
                    owner,
                    previous: None,
                    source,
                });
            pane
        };
        cx.notify();
        Some((pane, buffer))
    }

    #[cfg(unix)]
    pub(super) fn quit_commit_companion(
        &mut self,
        pane: u64,
        buffer: BufferId,
        owner: BufferId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.body_panes.get(&pane).is_some_and(|pane| {
            pane.current == buffer
                && pane
                    .quit_restore
                    .iter()
                    .any(|restore| restore.buffer == buffer && restore.owner == Some(owner))
        }) {
            self.quit_temporary_pane(pane, window, cx);
        }
    }

    pub(super) fn quit_temporary(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.minibuffer.is_some() {
            return;
        }
        self.quit_temporary_pane(*self.panes.selected(), window, cx);
    }

    pub(super) fn quit_temporary_pane(
        &mut self,
        pane_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let selected = *self.panes.selected();
        let buffer = self.body_panes[&pane_id].current;
        self.cancel_file_open();
        self.echo = None;
        self.body_panes[&pane_id]
            .active_view()
            .clone()
            .update(cx, |view, cx| view.suspend_input(cx));
        self.panes.select(&pane_id);
        let pane = self.body_panes.get_mut(&pane_id).unwrap();
        let restore = pane
            .quit_restore
            .iter()
            .position(|restore| restore.buffer == buffer)
            .map(|index| pane.quit_restore.remove(index));
        let previous = pane
            .previous
            .iter()
            .copied()
            .find(|id| *id != buffer && self.buffers.index_of(*id).is_some());
        if let Some(restore) = &restore
            && restore.previous.is_none()
            && previous.is_none()
            && self.panes.can_close_selected()
        {
            self.panes.close_selected();
            self.body_panes.remove(&pane_id);
            if selected == pane_id {
                self.panes.select(&restore.source);
            }
        } else {
            let replacement = restore
                .as_ref()
                .and_then(|restore| restore.previous)
                .filter(|id| *id != buffer && self.buffers.index_of(*id).is_some())
                .or(previous)
                .or_else(|| self.buffers.other_buffer(buffer));
            let index = replacement
                .and_then(|id| self.buffers.index_of(id))
                .unwrap_or_else(|| self.create_buffer(Buffer::scratch(), window, cx));
            self.buffer_target(index, window, cx);
            let replacement = self.buffers[index].id();
            let pane = self.body_panes.get_mut(&pane_id).unwrap();
            pane.show_buffer(replacement);
            pane.previous.retain(|previous| *previous != buffer);
            if let Some(restore) = restore
                && restore.previous.is_some()
                && selected == pane_id
            {
                self.panes.select(&restore.source);
            }
        }
        if selected != pane_id {
            self.panes.select(&selected);
        }
        self.buffers.touch(self.active_buffer_id());
        self.buffers.bury(buffer);
        self.update_selected_views(window, cx);
        if self.minibuffer.is_none() {
            self.active_view().clone().update(cx, |view, cx| {
                view.invalidate_split_geometry();
                view.restore_input(window, cx);
            });
        } else {
            for view in self.all_views() {
                view.update(cx, |view, cx| view.suspend_input(cx));
            }
        }
        cx.notify();
    }
}
