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
    source: u64,
}

impl AppShell {
    pub(super) fn display_temporary(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.minibuffer.is_some() {
            return;
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
        let previous = self.active_buffer_id();
        let split = self
            .active_view()
            .read(cx)
            .check_split(Axis::Vertical)
            .is_ok();
        if split {
            self.cancel_file_open();
            let view = Self::create_view(self.buffers[index].buffer.new_view(), window, cx);
            let pane = self.split_selected(view, Axis::Vertical, cx);
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
        let reusable = self.panes.leaves().into_iter().find(|pane| {
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
            self.update_selected_views(cx);
            pane_id
        } else {
            self.active_view()
                .read(cx)
                .check_split(Axis::Vertical)
                .ok()?;
            let view = Self::create_view(self.buffers[index].buffer.new_view(), window, cx);
            let pane = self.split_selected(view, Axis::Vertical, cx);
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
            && self.panes.leaves().len() > 1
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
        self.update_selected_views(cx);
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
