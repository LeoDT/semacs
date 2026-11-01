use super::AppShell;
use crate::buffer::Buffer;
use crate::buffers::{BufferSlot, available_name};
use crate::exit::Closing;
use crate::session::ReadKind;
use crate::view::BufferView;
use gpui_kit::{App, Context, Entity, Window};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

impl AppShell {
    /// Adds an inactive buffer and returns its index.
    pub fn create_buffer(
        &mut self,
        buffer: Buffer,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> usize {
        let view = Self::create_view(buffer, window, cx);
        self.insert_view(view, cx)
    }

    pub(super) fn insert_view(
        &mut self,
        view: Entity<BufferView>,
        cx: &mut Context<Self>,
    ) -> usize {
        let subscription = cx.observe(&view, |_, _, cx| cx.notify());
        let index = self.buffers.push(BufferSlot::new(view, subscription));
        self.rename_buffer(index, cx);
        index
    }

    pub fn buffer_count(&self) -> usize {
        self.buffers.len()
    }

    /// Gives only the new buffer a unique name. Existing buffers keep the names
    /// they were created with, matching `emacs -Q` (no uniquify-style rename).
    pub(super) fn rename_buffer(&mut self, index: usize, cx: &mut Context<Self>) {
        let (base, path) = {
            let buffer = self.buffers[index].view.read(cx).buffer();
            (buffer.base_name(), buffer.path().map(Path::to_path_buf))
        };
        let used: HashSet<String> = self
            .buffers
            .iter()
            .enumerate()
            .filter(|(other, _)| *other != index)
            .map(|(_, slot)| slot.view.read(cx).buffer().display_name())
            .collect();
        let name = available_name(&base, path.as_deref(), &used);
        self.buffers[index]
            .view
            .update(cx, |view, cx| view.set_display_name(name, cx));
    }

    pub fn active_index(&self) -> usize {
        self.buffers.active_index()
    }

    pub fn active_view(&self) -> &Entity<BufferView> {
        self.buffers.active_view()
    }

    pub fn buffer_view(&self, index: usize) -> Option<&Entity<BufferView>> {
        self.buffers.get(index).map(|slot| &slot.view)
    }

    /// A buffer's `default-directory`: its visited file's directory, or the
    /// process startup directory for a buffer that has no file yet.
    pub(super) fn default_directory_for(&self, view: &Entity<BufferView>, cx: &App) -> PathBuf {
        view.read(cx)
            .buffer()
            .path()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .unwrap_or_else(|| self.startup_directory.clone())
    }

    pub(super) fn kill_buffer_at(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (modified, name) = {
            let buffer = self.buffers[index].view.read(cx).buffer();
            (buffer.is_modified(), buffer.display_name())
        };
        if modified {
            let view = self.buffers[index].view.clone();
            self.begin_read(ReadKind::KillBufferConfirm { name }, &view, "", window, cx);
        } else {
            self.remove_buffer(index, window, cx);
        }
    }

    pub(super) fn switch_to_buffer(
        &mut self,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let existing = self
            .buffers
            .iter()
            .position(|slot| slot.view.read(cx).buffer().display_name() == name);
        let index = match existing {
            Some(index) => index,
            None => {
                let buffer = Buffer::named(name);
                self.create_buffer(buffer, window, cx)
            }
        };
        let activated = self.activate(index, window, cx);
        debug_assert!(activated, "a selected buffer should activate");
    }

    /// Activates an existing buffer. Switching only moves rendering and focus;
    /// each buffer keeps its own point, mark, history, scroll and file identity.
    /// Refused while a minibuffer session is open so it cannot be detached from
    /// its target.
    pub fn activate(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if index >= self.buffers.len() || self.minibuffer.is_some() {
            return false;
        }
        self.cancel_file_open();
        self.buffers.set_active(index);
        let view = self.buffers.active_view().clone();
        self.buffers.touch(&view);
        view.update(cx, |view, cx| view.restore_input(window, cx));
        cx.notify();
        true
    }

    pub(super) fn remove_buffer(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let was_active = index == self.buffers.active_index();
        let removed = self.buffers.remove(index);
        if matches!(&self.closing, Some(Closing::Kill(target)) if target == &removed.view) {
            self.closing = None;
        }
        if self.buffers.is_empty() {
            let index = self.create_buffer(Buffer::scratch(), window, cx);
            self.buffers.set_active(index);
            let view = self.active_view().clone();
            view.update(cx, |view, cx| view.restore_input(window, cx));
        } else if was_active {
            let view = self.active_view().clone();
            view.update(cx, |view, cx| view.restore_input(window, cx));
        }
        cx.notify();
    }
}
