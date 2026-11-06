use super::AppShell;
use crate::buffer::BufferId;
use crate::exit::{AfterSave, Closing, ExitBufferState, ExitFlow, ExitStep};
use crate::session::{ReadKind, ReadSession};
use crate::view::BufferView;
use gpui_kit::{Context, Entity, Window};

impl AppShell {
    pub(super) fn exiting(&self) -> bool {
        self.closing.as_ref().is_some_and(Closing::is_exit)
    }

    pub(super) fn cancel_closing(&mut self) {
        self.closing = None;
    }

    pub(super) fn wait_for_save(&mut self, buffer: BufferId) {
        if let Some(flow) = self.closing.as_mut().and_then(Closing::exit_mut) {
            flow.wait_for_save(buffer);
        }
    }

    pub(super) fn decline_save(
        &mut self,
        view: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.exiting() {
            if let Some(flow) = self.closing.as_mut().and_then(Closing::exit_mut) {
                flow.clear_waiting_save();
            }
            self.process_next_exit(window, cx);
        } else {
            self.cancel_closing();
            self.show_message(view, "Canceled", cx);
        }
    }

    pub(super) fn after_save(
        &mut self,
        view: &Entity<BufferView>,
        saved: bool,
        message: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let buffer = view.read(cx).buffer().snapshot_id().buffer;
        let modified = self
            .buffers
            .index_of(buffer)
            .is_some_and(|i| self.buffers[i].buffer.is_modified());
        let action = self.closing.as_mut().map_or(AfterSave::Message, |closing| {
            closing.after_save(buffer, saved, modified)
        });
        match action {
            AfterSave::Kill => {
                self.cancel_closing();
                if let Some(index) = self.buffer_index(view, cx) {
                    self.remove_buffer(index, window, cx);
                }
            }
            AfterSave::ContinueExit => self.process_next_exit(window, cx),
            AfterSave::Cancel(reason) => {
                self.cancel_closing();
                self.show_message(view, reason.map_or(message, str::to_owned), cx);
            }
            AfterSave::Message => self.show_message(view, message, cx),
        }
    }

    pub(super) fn save_then_kill(
        &mut self,
        target: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.buffer_index(target, cx) else {
            return;
        };
        if !self.buffers[index].buffer.is_modified() && !self.buffers[index].save.is_saving() {
            self.remove_buffer(index, window, cx);
            return;
        }
        self.closing = Some(Closing::Kill(target.read(cx).buffer().snapshot_id().buffer));
        if !self.save_buffer(target, window, cx) {
            self.cancel_closing();
        }
    }

    pub(super) fn request_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.exiting() {
            return false;
        }
        if let Some(id) = self.session.as_ref().map(ReadSession::id) {
            self.cancel_save_confirmation(cx);
            self.end_session(id, window, cx);
        }
        self.begin_exit(window, cx)
    }

    pub(super) fn begin_exit(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.exiting() {
            return false;
        }
        self.cancel_file_open();
        self.end_all_input(cx);
        let remaining: Vec<_> = self
            .buffers
            .ids_in_mru()
            .iter()
            .copied()
            .filter(|id| {
                self.buffers.index_of(*id).is_some_and(|index| {
                    self.buffers[index].save.is_saving() || self.buffers[index].buffer.is_modified()
                })
            })
            .collect();
        if remaining.is_empty() && !self.git_write_active() && !self.git_writes.uncertain() {
            return true;
        }
        self.closing = Some(Closing::Exit(ExitFlow::new(remaining)));
        self.process_next_exit(window, cx);
        false
    }

    pub(super) fn process_next_exit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.exiting() {
            return;
        }
        if self.git_write_active() {
            self.show_message(
                &self.active_view().clone(),
                "Waiting for Git write and repository verification before exit",
                cx,
            );
            return;
        }
        let Some(flow) = self.closing.as_mut().and_then(Closing::exit_mut) else {
            return;
        };
        if self.git_writes.uncertain() && !flow.uncertain_git_confirmed {
            if self
                .session
                .as_ref()
                .is_some_and(|session| matches!(session.kind(), ReadKind::ExitUncertainGit))
            {
                return;
            }
            let origin = self.active_view().clone();
            if self
                .begin_read(ReadKind::ExitUncertainGit, &origin, "", window, cx)
                .is_none()
            {
                self.cancel_closing();
                self.show_message(
                    &origin,
                    "Cannot confirm uncertain Git result; exit canceled",
                    cx,
                );
            }
            return;
        }
        let step = flow.next(|id| {
            self.buffers.index_of(id).map(|index| {
                if self.buffers[index].save.is_saving() {
                    ExitBufferState::Saving
                } else if self.buffers[index].buffer.is_modified() {
                    ExitBufferState::Modified
                } else {
                    ExitBufferState::Clean
                }
            })
        });
        match step {
            ExitStep::Wait => {}
            ExitStep::Ask(id) => {
                let Some(index) = self.buffers.index_of(id) else {
                    return;
                };
                let (path, name) = (
                    self.buffers[index].buffer.path(),
                    self.buffers[index].buffer.display_name(),
                );
                let target = self.buffer_target(index, window, cx);
                let kind = ReadKind::ExitSave { path, name };
                if self.begin_read(kind, &target, "", window, cx).is_none() {
                    self.cancel_closing();
                    self.show_message(&target, "Cannot confirm exit; exit canceled", cx);
                }
            }
            ExitStep::CheckRemaining => {
                if self.buffers.iter().any(|slot| slot.buffer.is_modified()) {
                    let origin = self.active_view().clone();
                    if self
                        .begin_read(ReadKind::ExitModified, &origin, "", window, cx)
                        .is_none()
                    {
                        self.cancel_closing();
                        self.show_message(&origin, "Cannot confirm exit; exit canceled", cx);
                    }
                } else {
                    self.finish_exit(window, cx);
                }
            }
        }
    }

    pub(super) fn finish_exit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.git_write_active() {
            self.process_next_exit(window, cx);
            return;
        }
        self.cancel_closing();
        window.remove_window();
        cx.notify();
    }

    pub(super) fn end_all_input(&mut self, cx: &mut Context<Self>) {
        for view in self.all_views() {
            view.update(cx, |view, cx| view.end_input(cx));
        }
        if let Some(minibuffer) = &self.minibuffer {
            let editor = minibuffer.view.read(cx).editor().clone();
            editor.update(cx, |view, cx| view.end_input(cx));
        }
    }
}
