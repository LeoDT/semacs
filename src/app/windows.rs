use std::collections::HashMap;

use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::AppShell;
use crate::buffer::BufferId;
use crate::command::Command;
use crate::frame;
use crate::panes::{Axis, Pane};
use crate::view::BufferView;

#[cfg(test)]
#[path = "window_tests.rs"]
mod tests;

pub(super) struct EditorMount {
    pub view: Entity<BufferView>,
    _subscription: Subscription,
}

pub(super) struct BodyPane {
    pub current: BufferId,
    pub editors: HashMap<BufferId, EditorMount>,
}

impl BodyPane {
    pub fn new(current: BufferId, view: Entity<BufferView>, subscription: Subscription) -> Self {
        Self {
            current,
            editors: HashMap::from([(
                current,
                EditorMount {
                    view,
                    _subscription: subscription,
                },
            )]),
        }
    }

    pub fn active_view(&self) -> &Entity<BufferView> {
        &self.editors[&self.current].view
    }
}

impl AppShell {
    pub(super) fn cache_view(
        &mut self,
        pane: u64,
        buffer: BufferId,
        view: Entity<BufferView>,
        cx: &mut Context<Self>,
    ) {
        let subscription = cx.observe(&view, Self::view_changed);
        self.body_panes.get_mut(&pane).unwrap().editors.insert(
            buffer,
            EditorMount {
                view,
                _subscription: subscription,
            },
        );
    }

    pub(super) fn all_views(&self) -> Vec<Entity<BufferView>> {
        self.body_panes
            .values()
            .flat_map(|pane| pane.editors.values().map(|mount| mount.view.clone()))
            .collect()
    }

    pub(super) fn view_changed(&mut self, source: Entity<BufferView>, cx: &mut Context<Self>) {
        if source.read(cx).buffer_changed() {
            let id = source.read(cx).buffer().snapshot_id().buffer;
            self.refresh_buffer(id, cx);
        } else {
            cx.notify();
        }
    }

    pub(super) fn refresh_buffer(&mut self, id: BufferId, cx: &mut Context<Self>) {
        for pane in self.body_panes.values() {
            if let Some(mount) = pane.editors.get(&id) {
                mount
                    .view
                    .update(cx, |view, cx| view.synchronize_buffer(cx));
            }
        }
        cx.notify();
    }

    pub(super) fn update_selected_views(&mut self, cx: &mut Context<Self>) {
        for (id, pane) in &self.body_panes {
            for (buffer, mount) in &pane.editors {
                mount.view.update(cx, |view, cx| {
                    view.set_body_state(*buffer == pane.current, *id == *self.panes.selected(), cx);
                });
            }
        }
    }

    fn select_pane(&mut self, pane: u64, window: &mut Window, cx: &mut Context<Self>) {
        if self.minibuffer.is_some() || pane == *self.panes.selected() {
            return;
        }
        let old = self.active_view().clone();
        old.update(cx, |view, cx| view.suspend_input(cx));
        if !self.panes.select(&pane) {
            return;
        }
        self.cancel_file_open();
        self.buffers.touch(self.active_buffer_id());
        self.update_selected_views(cx);
        let active = self.active_view().clone();
        active.update(cx, |view, cx| view.restore_input(window, cx));
        cx.notify();
    }

    pub(super) fn window_command(
        &mut self,
        command: Command,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.minibuffer.is_some() {
            return;
        }
        match command {
            Command::SplitWindowBelow | Command::SplitWindowRight => {
                let axis = if command == Command::SplitWindowBelow {
                    Axis::Vertical
                } else {
                    Axis::Horizontal
                };
                let source = self.active_view().clone();
                if let Err(message) = source.read(cx).check_split(axis) {
                    self.show_message(&source, message, cx);
                    return;
                }
                self.cancel_file_open();
                let (buffer, scroll) = source.read(cx).split_state();
                let id = buffer.snapshot_id().buffer;
                let view = Self::create_view(buffer, window, cx);
                view.update(cx, |view, cx| view.initialize_scroll(scroll, cx));
                let pane = self.next_pane;
                self.next_pane = self
                    .next_pane
                    .checked_add(1)
                    .expect("pane identity exhausted");
                let subscription = cx.observe(&view, Self::view_changed);
                self.body_panes
                    .insert(pane, BodyPane::new(id, view, subscription));
                self.panes.split(pane, axis);
                source.update(cx, |view, _| view.invalidate_split_geometry());
                self.update_selected_views(cx);
                cx.notify();
            }
            Command::OtherWindow => {
                let mut traversal = self.panes.clone();
                let next = *traversal.next();
                self.select_pane(next, window, cx);
            }
            Command::DeleteWindow => {
                let old_id = *self.panes.selected();
                if self.panes.leaves().len() == 1 {
                    let target = self.active_view().clone();
                    self.show_message(&target, "Cannot delete the only window", cx);
                    return;
                }
                self.cancel_file_open();
                let old = self.active_view().clone();
                old.update(cx, |view, cx| view.suspend_input(cx));
                self.panes.close_selected();
                self.body_panes.remove(&old_id);
                self.update_selected_views(cx);
                let active = self.active_view().clone();
                self.buffers.touch(self.active_buffer_id());
                active.update(cx, |view, cx| view.restore_input(window, cx));
                cx.notify();
            }
            Command::DeleteOtherWindows => {
                self.cancel_file_open();
                let selected = *self.panes.selected();
                for (id, pane) in &self.body_panes {
                    if *id != selected {
                        for mount in pane.editors.values() {
                            mount.view.update(cx, |view, cx| view.suspend_input(cx));
                        }
                    }
                }
                self.panes.retain_selected();
                self.body_panes.retain(|id, _| *id == selected);
                self.update_selected_views(cx);
                cx.notify();
            }
            _ => unreachable!(),
        }
    }

    pub(super) fn render_panes(&self, pane: &Pane<u64>, cx: &mut Context<Self>) -> AnyElement {
        match pane {
            Pane::Leaf(id) => {
                let editor = self.body_panes[id].active_view().clone();
                let buffer = editor.read(cx).buffer();
                let selected = *id == *self.panes.selected();
                let mode_line = frame::mode_line(
                    buffer.display_name(),
                    buffer.format(),
                    buffer.is_modified(),
                    buffer.line_column(),
                    selected,
                    cx,
                );
                let pending = selected && self.opening.is_some();
                let shell = cx.weak_entity();
                let pane_id = *id;
                v_flex()
                    .id(("pane", *id))
                    .flex_1()
                    .flex_basis(px(0.))
                    .min_h_0()
                    .min_w_0()
                    .overflow_hidden()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |shell, _, window, cx| {
                            shell.select_pane(pane_id, window, cx)
                        }),
                    )
                    .child(v_flex().flex_1().min_h_0().min_w_0().child(editor).when(
                        pending,
                        |body| {
                            body.on_children_prepainted(move |bounds, window, cx| {
                                if let Some(bounds) = bounds.first() {
                                    shell
                                        .update(cx, |shell, cx| {
                                            shell.prepare_opening(*bounds, window, cx)
                                        })
                                        .ok();
                                }
                            })
                        },
                    ))
                    .child(mode_line)
                    .into_any_element()
            }
            Pane::Split {
                axis,
                first,
                second,
            } => {
                let first = self.render_panes(first, cx);
                let second = self.render_panes(second, cx);
                let layout = match axis {
                    Axis::Horizontal => h_flex(),
                    Axis::Vertical => v_flex(),
                };
                layout
                    .items_stretch()
                    .flex_1()
                    .flex_basis(px(0.))
                    .min_h_0()
                    .min_w_0()
                    .overflow_hidden()
                    .child(first)
                    .when(*axis == Axis::Horizontal, |layout| {
                        layout.child(
                            div()
                                .w(px(1.))
                                .h_full()
                                .flex_shrink_0()
                                .bg(cx.theme().border),
                        )
                    })
                    .child(second)
                    .into_any_element()
            }
        }
    }
}
