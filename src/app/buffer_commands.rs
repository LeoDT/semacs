use super::AppShell;
use crate::buffer::{Buffer, BufferId};
use crate::buffers::{BufferSlot, available_name};
use crate::exit::Closing;
use crate::session::ReadKind;
use crate::view::BufferView;
use gpui_kit::{App, Context, Entity, Window};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

impl AppShell {
    pub fn create_buffer(
        &mut self,
        buffer: Buffer,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> usize {
        let index = self.buffers.push(BufferSlot::new(buffer));
        self.rename_buffer(index, cx);
        self.buffer_target(index, window, cx);
        index
    }

    pub(super) fn insert_view(
        &mut self,
        view: Entity<BufferView>,
        cx: &mut Context<Self>,
    ) -> usize {
        self.insert_view_in(*self.panes.selected(), view, cx)
    }

    pub(super) fn insert_view_in(
        &mut self,
        pane: u64,
        view: Entity<BufferView>,
        cx: &mut Context<Self>,
    ) -> usize {
        let buffer = view.read(cx).buffer().new_view();
        let id = buffer.snapshot_id().buffer;
        let index = self.buffers.push(BufferSlot::new(buffer));
        self.cache_view(pane, id, view, cx);
        self.rename_buffer(index, cx);
        index
    }

    pub fn buffers(&self) -> impl Iterator<Item = &Buffer> {
        self.buffers.iter().map(|slot| &slot.buffer)
    }

    pub fn buffer_count(&self) -> usize {
        self.buffers.len()
    }

    pub(super) fn rename_buffer(&mut self, index: usize, cx: &mut Context<Self>) {
        let buffer = &self.buffers[index].buffer;
        let (base, path) = (buffer.base_name(), buffer.path());
        let used: HashSet<String> = self
            .buffers
            .iter()
            .enumerate()
            .filter(|(other, _)| *other != index)
            .map(|(_, slot)| slot.buffer.display_name())
            .collect();
        let name = available_name(&base, path.as_deref(), &used);
        self.buffers[index].buffer.set_display_name(name);
        self.refresh_buffer(self.buffers[index].id(), cx);
    }

    pub fn active_index(&self) -> usize {
        self.buffers
            .index_of(self.body_panes[self.panes.selected()].current)
            .expect("selected pane displays a live buffer")
    }

    pub fn active_view(&self) -> &Entity<BufferView> {
        self.body_panes[self.panes.selected()].active_view()
    }

    pub fn buffer_view(&self, index: usize) -> Option<&Entity<BufferView>> {
        let id = self.buffers.get(index)?.id();
        self.body_panes[self.panes.selected()]
            .editors
            .get(&id)
            .or_else(|| {
                self.body_panes
                    .values()
                    .find_map(|pane| pane.editors.get(&id))
            })
            .map(|mount| &mount.view)
    }

    pub(super) fn buffer_index(&self, target: &Entity<BufferView>, cx: &App) -> Option<usize> {
        self.buffers
            .index_of(target.read(cx).buffer().snapshot_id().buffer)
    }

    pub(super) fn buffer_target(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<BufferView> {
        let id = self.buffers[index].id();
        let pane = *self.panes.selected();
        if let Some(mount) = self.body_panes[&pane].editors.get(&id) {
            return mount.view.clone();
        }
        let buffer = self.buffers[index].buffer.new_view();
        let view = Self::create_view(buffer, window, cx);
        view.update(cx, |view, cx| view.suspend_input(cx));
        self.cache_view(pane, id, view.clone(), cx);
        view
    }

    pub(super) fn default_directory_for(&self, view: &Entity<BufferView>, cx: &App) -> PathBuf {
        view.read(cx)
            .buffer()
            .path()
            .and_then(|path| path.parent().map(ToOwned::to_owned))
            .or_else(|| view.read(cx).buffer().tree_root())
            .unwrap_or_else(|| self.startup_directory.clone())
    }

    /// Directory a file read starts from: the origin buffer's repository root,
    /// then its own directory, then the startup directory.
    pub(super) fn file_read_directory(&self, view: &Entity<BufferView>, cx: &App) -> PathBuf {
        let buffer = view.read(cx).buffer();
        if buffer.tree_root().is_some()
            && let Ok(node) = buffer.tree_node()
        {
            return if node.directory {
                node.target
            } else {
                node.target.parent().unwrap().to_path_buf()
            };
        }
        if let Some(repository) = buffer.repository() {
            return repository.root;
        }
        if let Some(directory) = buffer.search_directory() {
            return directory;
        }
        buffer
            .path()
            .and_then(|path| path.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| self.startup_directory.clone())
    }

    /// Starts a file read whose initial input is the default directory, as
    /// `insert-default-directory` does upstream.
    pub(super) fn begin_file_read(
        &mut self,
        kind: ReadKind,
        target: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<u64> {
        let initial = crate::files::directory_input(&self.file_read_directory(target, cx));
        self.begin_read(kind, target, &initial, window, cx)
    }

    pub(super) fn kill_buffer_at(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        #[cfg(unix)]
        if self.commit_editors.contains_key(&self.buffers[index].id()) {
            let target = self.buffer_target(index, window, cx);
            self.show_message(
                &target,
                "Use with-editor-cancel to end this editor session",
                cx,
            );
            return;
        }
        let buffer = &self.buffers[index].buffer;
        let (modified, name) = (buffer.is_modified(), buffer.display_name());
        if modified {
            let target = self.buffer_target(index, window, cx);
            self.begin_read(
                ReadKind::KillBufferConfirm { name, batch: false },
                &target,
                "",
                window,
                cx,
            );
        } else {
            self.remove_buffer(index, window, cx);
        }
    }

    pub(super) fn show_buffer_in_pane(
        &mut self,
        pane_id: u64,
        index: usize,
        select: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if index >= self.buffers.len()
            || self.minibuffer.is_some()
            || !self.body_panes.contains_key(&pane_id)
            || self.panes.side() == Some(&pane_id)
        {
            return false;
        }
        if self.isearch.is_some() {
            self.finish_isearch(false, false, true, Some(window), cx);
        }
        self.cancel_file_open();
        let id = self.buffers[index].id();
        if !self.body_panes[&pane_id].editors.contains_key(&id) {
            let buffer = self.buffers[index].buffer.new_view();
            let view = Self::create_view(buffer, window, cx);
            view.update(cx, |view, cx| view.suspend_input(cx));
            self.cache_view(pane_id, id, view, cx);
        }
        let selected = *self.panes.selected();
        if select && selected != pane_id {
            self.active_view()
                .clone()
                .update(cx, |view, cx| view.suspend_input(cx));
            self.panes.select(&pane_id);
        } else if self.body_panes[&pane_id].current != id {
            self.body_panes[&pane_id]
                .active_view()
                .clone()
                .update(cx, |view, cx| view.suspend_input(cx));
        }
        self.body_panes.get_mut(&pane_id).unwrap().show_buffer(id);
        self.buffers.touch(id);
        self.update_selected_views(window, cx);
        let view = self.body_panes[&pane_id].active_view().clone();
        if select || *self.panes.selected() == pane_id {
            self.echo = None;
            view.update(cx, |view, cx| {
                view.invalidate_split_geometry();
                view.restore_input(window, cx);
            });
        } else {
            view.update(cx, |view, _| view.invalidate_split_geometry());
        }
        cx.notify();
        true
    }

    pub fn activate(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if index >= self.buffers.len() || self.minibuffer.is_some() {
            return false;
        }
        if self.isearch.is_some() {
            self.finish_isearch(false, false, true, Some(window), cx);
        }
        if self.panes.side() == Some(self.panes.selected())
            && self.active_buffer_id() != self.buffers[index].id()
        {
            self.cancel_file_open();
            let target = self.active_view().clone();
            self.show_message(&target, "Window is dedicated to *Treemacs*", cx);
            return false;
        }
        self.echo = None;
        self.cancel_file_open();
        let old = self.active_view().clone();
        old.update(cx, |view, cx| view.suspend_input(cx));
        let id = self.buffers[index].id();
        let view = self.buffer_target(index, window, cx);
        self.body_panes
            .get_mut(self.panes.selected())
            .unwrap()
            .show_buffer(id);
        self.buffers.touch(id);
        self.update_selected_views(window, cx);
        view.update(cx, |view, cx| {
            view.invalidate_split_geometry();
            view.restore_input(window, cx);
        });
        cx.notify();
        true
    }

    fn is_cyclable_buffer(&self, id: BufferId) -> bool {
        self.buffers.index_of(id).is_some_and(|index| {
            let buffer = &self.buffers[index].buffer;
            !buffer.display_name().starts_with(' ')
                && buffer.mode() != crate::mode::ModeId::TREEMACS
        })
    }

    /// Upstream GNU Emacs 31.1 `previous-buffer` / `next-buffer`
    /// (`switch-to-prev-buffer` / `switch-to-next-buffer`).
    pub(super) fn cycle_buffer(
        &mut self,
        forward: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.minibuffer.is_some() {
            return;
        }
        if self.isearch.is_some() {
            self.finish_isearch(false, false, true, Some(window), cx);
        }
        let pane_id = *self.panes.selected();
        if self.panes.side() == Some(&pane_id) {
            let target = self.active_view().clone();
            self.show_message(&target, "Window is dedicated to *Treemacs*", cx);
            return;
        }
        let old = self.body_panes[&pane_id].current;
        let mut next_buffers = self.body_panes[&pane_id].next.clone();
        let prev_buffers = &self.body_panes[&pane_id].previous;
        let new_id = if forward {
            next_buffers
                .iter()
                .copied()
                .find(|&id| {
                    id != old && self.buffers.index_of(id).is_some() && prev_buffers.contains(&id)
                })
                .or_else(|| {
                    self.buffers.ids_in_mru().iter().copied().find(|&id| {
                        id != old && self.is_cyclable_buffer(id) && !prev_buffers.contains(&id)
                    })
                })
                .or_else(|| {
                    prev_buffers
                        .iter()
                        .rev()
                        .copied()
                        .find(|&id| id != old && self.buffers.index_of(id).is_some())
                })
        } else {
            prev_buffers
                .iter()
                .copied()
                .find(|&id| {
                    id != old && self.buffers.index_of(id).is_some() && !next_buffers.contains(&id)
                })
                .or_else(|| {
                    self.buffers.ids_in_mru().iter().rev().copied().find(|&id| {
                        id != old && self.is_cyclable_buffer(id) && !next_buffers.contains(&id)
                    })
                })
                .or_else(|| {
                    next_buffers.iter().rev().copied().find(|&id| {
                        id != old
                            && self.buffers.index_of(id).is_some()
                            && prev_buffers.contains(&id)
                    })
                })
        };
        let Some(new_id) = new_id else {
            let target = self.active_view().clone();
            self.show_message(
                &target,
                if forward {
                    "No next buffer"
                } else {
                    "No previous buffer"
                },
                cx,
            );
            return;
        };
        let Some(index) = self.buffers.index_of(new_id) else {
            return;
        };
        self.echo = None;
        self.cancel_file_open();
        let old_view = self.active_view().clone();
        old_view.update(cx, |view, cx| view.suspend_input(cx));
        let view = self.buffer_target(index, window, cx);
        let pane = self.body_panes.get_mut(&pane_id).unwrap();
        pane.show_buffer(new_id);
        if forward {
            next_buffers.retain(|id| *id != new_id);
        } else {
            next_buffers.retain(|id| *id != old);
            next_buffers.insert(0, old);
        }
        pane.next = next_buffers;
        self.update_selected_views(window, cx);
        view.update(cx, |view, cx| {
            view.invalidate_split_geometry();
            view.restore_input(window, cx);
        });
        cx.notify();
    }

    pub(super) fn remove_buffer(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = self.buffers[index].id();
        self.finish_isearch_if_buffer(id, Some(window), cx);
        #[cfg(unix)]
        if self.commit_editors.contains_key(&id) {
            let target = self.buffer_target(index, window, cx);
            self.show_message(
                &target,
                "Use with-editor-cancel to end this editor session",
                cx,
            );
            return;
        }
        self.cancel_file_open();
        if matches!(&self.closing, Some(Closing::Kill(target)) if *target == id) {
            self.closing = None;
        } else if let Some((waiting_save, remaining)) =
            self.closing.as_mut().and_then(Closing::kill_batch_mut)
        {
            if *waiting_save == Some(id) {
                // The buffer whose save was gating this batch was killed out of
                // band; cancel the batch so a dangling queue cannot fire later.
                self.closing = None;
            } else {
                remaining.retain(|other| *other != id);
                if waiting_save.is_none() && remaining.is_empty() {
                    self.closing = None;
                }
            }
        }
        self.reset_message_log(id);
        self.discard_search_buffer(id);
        self.discard_tree_window(id, window, cx);
        #[cfg(unix)]
        self.discard_commit_selection(id, window, cx);
        self.buffers.remove(id);
        if self.buffers.is_empty() {
            self.create_buffer(Buffer::scratch(), window, cx);
        }
        let replacement = self.buffers.ids_in_mru()[0];
        let affected: Vec<_> = self
            .body_panes
            .iter()
            .filter_map(|(pane_id, pane)| (pane.current == id).then_some(*pane_id))
            .collect();
        for pane_id in affected {
            let view = if let Some(mount) = self.body_panes[&pane_id].editors.get(&replacement) {
                mount.view.clone()
            } else {
                let index = self.buffers.index_of(replacement).unwrap();
                let buffer = self.buffers[index].buffer.new_view();
                let view = Self::create_view(buffer, window, cx);
                self.cache_view(pane_id, replacement, view.clone(), cx);
                view
            };
            self.body_panes
                .get_mut(&pane_id)
                .unwrap()
                .show_buffer(replacement);
            view.update(cx, |view, cx| {
                view.invalidate_split_geometry();
                view.resume_accepting_input(cx);
            });
        }
        for pane in self
            .body_panes
            .values_mut()
            .chain(self.tree_window.hidden.iter_mut())
        {
            pane.previous.retain(|previous| *previous != id);
            pane.next.retain(|next| *next != id);
            pane.quit_restore.retain(|restore| restore.buffer != id);
            if let Some(mount) = pane.editors.remove(&id) {
                mount.view.update(cx, |view, cx| view.suspend_input(cx));
            }
        }
        self.update_selected_views(window, cx);
        let active = self.active_view().clone();
        if self.minibuffer.is_none() {
            active.update(cx, |view, cx| view.restore_input(window, cx));
        } else {
            for view in self.all_views() {
                view.update(cx, |view, cx| view.suspend_input(cx));
            }
        }
        cx.notify();
    }

    pub(super) fn active_buffer_id(&self) -> BufferId {
        self.body_panes[self.panes.selected()].current
    }
}
