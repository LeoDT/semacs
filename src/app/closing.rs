use super::AppShell;
use crate::exit::{AfterSave, Closing, ExitBufferState, ExitFlow, ExitStep};
use crate::session::{ReadKind, ReadSession};
use crate::view::BufferView;
use gpui_kit::{Context, Entity, Window};
use std::path::Path;

impl AppShell {
    pub(super) fn exiting(&self) -> bool {
        self.closing.as_ref().is_some_and(Closing::is_exit)
    }

    pub(super) fn cancel_closing(&mut self) {
        self.closing = None;
    }

    pub(super) fn wait_for_save(&mut self, view: &Entity<BufferView>) {
        if let Some(flow) = self.closing.as_mut().and_then(Closing::exit_mut) {
            flow.wait_for_save(view.clone());
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
        let modified = view.read(cx).buffer().is_modified();
        let action = self.closing.as_mut().map_or(AfterSave::Message, |closing| {
            closing.after_save(view, saved, modified)
        });
        match action {
            AfterSave::Kill => {
                self.cancel_closing();
                if let Some(index) = self.buffers.index_of(view) {
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
        let Some(index) = self.buffers.index_of(target) else {
            return;
        };
        if !target.read(cx).buffer().is_modified() && !self.buffers[index].save.is_saving() {
            self.remove_buffer(index, window, cx);
            return;
        }
        self.closing = Some(Closing::Kill(target.clone()));
        if !self.save_buffer(target, window, cx) {
            self.cancel_closing();
        }
    }

    pub(super) fn request_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.exiting() {
            return false;
        }
        if let Some(id) = self.session.as_ref().map(ReadSession::id) {
            self.cancel_save_confirmation();
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
            .views_in_mru()
            .iter()
            .filter(|view| {
                self.buffers.index_of(view).is_some_and(|index| {
                    self.buffers[index].save.is_saving() || view.read(cx).buffer().is_modified()
                })
            })
            .cloned()
            .collect();
        if remaining.is_empty() {
            return true;
        }
        self.closing = Some(Closing::Exit(ExitFlow::new(remaining)));
        self.process_next_exit(window, cx);
        false
    }

    pub(super) fn process_next_exit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(flow) = self.closing.as_mut().and_then(Closing::exit_mut) else {
            return;
        };
        let step = flow.next(|view| {
            self.buffers.index_of(view).map(|index| {
                if self.buffers[index].save.is_saving() {
                    ExitBufferState::Saving
                } else if view.read(cx).buffer().is_modified() {
                    ExitBufferState::Modified
                } else {
                    ExitBufferState::Clean
                }
            })
        });
        match step {
            ExitStep::Wait => {}
            ExitStep::Ask(target) => {
                let buffer = target.read(cx).buffer();
                let kind = ReadKind::ExitSave {
                    path: buffer.path().map(Path::to_path_buf),
                    name: buffer.display_name(),
                };
                if self.begin_read(kind, &target, "", window, cx).is_none() {
                    self.cancel_closing();
                    self.show_message(&target, "Cannot confirm exit; exit canceled", cx);
                }
            }
            ExitStep::CheckRemaining => {
                if self
                    .buffers
                    .iter()
                    .any(|slot| slot.view.read(cx).buffer().is_modified())
                {
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
        self.cancel_closing();
        window.remove_window();
        cx.notify();
    }

    pub(super) fn end_all_input(&mut self, cx: &mut Context<Self>) {
        for slot in self.buffers.iter() {
            slot.view.update(cx, |view, cx| view.end_input(cx));
        }
        if let Some(minibuffer) = &self.minibuffer {
            let editor = minibuffer.view.read(cx).editor().clone();
            editor.update(cx, |view, cx| view.end_input(cx));
        }
    }
}
