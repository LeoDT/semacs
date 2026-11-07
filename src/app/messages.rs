use super::AppShell;
use crate::buffer::BufferId;
use crate::buffers::BufferSlot;
use crate::command::Command;
use crate::view::{BufferView, Feedback};
use gpui_kit::*;

#[cfg(test)]
#[path = "message_tests.rs"]
mod tests;

impl AppShell {
    pub(super) fn feedback(
        &mut self,
        source: Entity<BufferView>,
        feedback: &Feedback,
        cx: &mut Context<Self>,
    ) {
        if let Feedback::Message(text) = feedback {
            let id = self.messages.buffer.snapshot_id().buffer;
            let end = self.messages.buffer.text().len();
            let followers: Vec<_> = self
                .all_views()
                .into_iter()
                .filter(|view| {
                    let buffer = view.read(cx).buffer();
                    buffer.snapshot_id().buffer == id && buffer.point().0 == end
                })
                .collect();
            self.messages.push(text);
            if !text.is_empty() {
                self.register_messages(cx);
            }
            self.refresh_buffer(id, cx);
            for view in followers {
                view.update(cx, |view, cx| view.show_end(cx));
            }
        }
        let current = self
            .minibuffer
            .as_ref()
            .map(|mount| mount.view.read(cx).editor())
            .unwrap_or_else(|| self.active_view());
        match feedback {
            Feedback::Clear => {
                if self
                    .echo
                    .as_ref()
                    .is_some_and(|(owner, _)| *owner == source.entity_id())
                    || (self.session.is_none() && source == *self.active_view())
                {
                    self.echo = None;
                }
            }
            _ if *current == source
                || (self.session.is_none() && source == *self.active_view()) =>
            {
                self.echo = feedback
                    .text()
                    .map(|text| (source.entity_id(), SharedString::from(text.to_owned())));
            }
            _ => {}
        }
        cx.notify();
    }

    pub(super) fn view_messages(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.messages.buffer.snapshot_id().buffer;
        let index = self.register_messages(cx);
        let mut pane = self
            .body_panes
            .iter()
            .find_map(|(pane, body)| (body.current == id).then_some(*pane));
        if pane.is_none() {
            pane = self
                .panes
                .leaves()
                .into_iter()
                .find(|pane| pane != self.panes.selected());
        }
        if pane.is_none() {
            let source = self.active_view().read(cx);
            let split = if source.check_split(crate::panes::Axis::Vertical).is_ok() {
                Some(Command::SplitWindowBelow)
            } else if source.check_split(crate::panes::Axis::Horizontal).is_ok() {
                Some(Command::SplitWindowRight)
            } else {
                None
            };
            if let Some(command) = split {
                let next = self.next_pane;
                self.window_command(command, window, cx);
                pane = Some(next);
            }
        }
        let pane = pane.unwrap_or(*self.panes.selected());
        let view = if let Some(mount) = self.body_panes[&pane].editors.get(&id) {
            mount.view.clone()
        } else {
            let view = Self::create_view(self.buffers[index].buffer.new_view(), window, cx);
            self.cache_view(pane, id, view.clone(), cx);
            view
        };
        let old = self.body_panes[&pane].active_view().clone();
        if old != view {
            old.update(cx, |view, cx| view.suspend_input(cx));
        }
        self.body_panes.get_mut(&pane).unwrap().show_buffer(id);
        self.update_selected_views(cx);
        view.update(cx, |view, cx| {
            view.show_end(cx);
            if pane == *self.panes.selected() {
                view.restore_input(window, cx);
            }
        });
        self.echo = None;
        cx.notify();
    }

    fn register_messages(&mut self, cx: &mut Context<Self>) -> usize {
        let id = self.messages.buffer.snapshot_id().buffer;
        if let Some(index) = self.buffers.index_of(id) {
            return index;
        }
        let index = self
            .buffers
            .insert_background(BufferSlot::new(self.messages.buffer.new_view()));
        self.rename_buffer(index, cx);
        index
    }

    pub(super) fn reset_message_log(&mut self, id: BufferId) {
        if self.messages.buffer.snapshot_id().buffer == id {
            self.messages = crate::messages::Messages::default();
        }
    }
}
