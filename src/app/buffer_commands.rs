use super::AppShell;
use crate::buffer::{Buffer, BufferId};
use crate::buffers::{BufferSlot, available_name};
use crate::exit::Closing;
use crate::session::ReadKind;
use crate::view::BufferView;
use gpui_kit::{App, Context, Entity, Window};
use std::collections::HashSet;
use std::path::PathBuf;

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
        let buffer = view.read(cx).buffer().new_view();
        let id = buffer.snapshot_id().buffer;
        let index = self.buffers.push(BufferSlot::new(buffer));
        self.cache_view(*self.panes.selected(), id, view, cx);
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
            .unwrap_or_else(|| self.startup_directory.clone())
    }

    pub(super) fn kill_buffer_at(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let buffer = &self.buffers[index].buffer;
        let (modified, name) = (buffer.is_modified(), buffer.display_name());
        if modified {
            let target = self.buffer_target(index, window, cx);
            self.begin_read(
                ReadKind::KillBufferConfirm { name },
                &target,
                "",
                window,
                cx,
            );
        } else {
            self.remove_buffer(index, window, cx);
        }
    }

    pub fn activate(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if index >= self.buffers.len() || self.minibuffer.is_some() {
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
            .current = id;
        self.buffers.touch(id);
        self.update_selected_views(cx);
        view.update(cx, |view, cx| {
            view.invalidate_split_geometry();
            view.restore_input(window, cx);
        });
        cx.notify();
        true
    }

    pub(super) fn remove_buffer(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = self.buffers[index].id();
        self.cancel_file_open();
        if matches!(&self.closing, Some(Closing::Kill(target)) if *target == id) {
            self.closing = None;
        }
        self.reset_message_log(id);
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
            self.body_panes.get_mut(&pane_id).unwrap().current = replacement;
            view.update(cx, |view, cx| {
                view.invalidate_split_geometry();
                view.resume_accepting_input(cx);
            });
        }
        for pane in self.body_panes.values_mut() {
            if let Some(mount) = pane.editors.remove(&id) {
                mount.view.update(cx, |view, cx| view.suspend_input(cx));
            }
        }
        self.update_selected_views(cx);
        let active = self.active_view().clone();
        active.update(cx, |view, cx| view.restore_input(window, cx));
        cx.notify();
    }

    pub(super) fn active_buffer_id(&self) -> BufferId {
        self.body_panes[self.panes.selected()].current
    }
}
