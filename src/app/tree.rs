use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use gpui_kit::component::h_flex;
use gpui_kit::*;

use super::AppShell;
use super::windows::BodyPane;
use crate::buffer::Buffer;
use crate::buffer::tree::Action;
use crate::buffers::BufferSlot;
use crate::command::Command;
use crate::panes::Pane;
use crate::session::ReadKind;
use crate::view::BufferView;

#[cfg(test)]
#[path = "tree_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "tree_navigation_tests.rs"]
mod navigation_tests;

pub(super) struct TreeWindow {
    pub hidden: Option<BodyPane>,
    pub columns: isize,
    pub follow: bool,
    follow_task: Option<Task<()>>,
}

#[derive(Clone, Copy)]
pub(super) enum Disposition {
    Select,
    Stay,
    Close,
}

#[derive(Clone, Copy)]
pub(super) struct Visit {
    tree: crate::buffer::BufferId,
    disposition: Disposition,
}

impl Default for TreeWindow {
    fn default() -> Self {
        Self {
            hidden: None,
            columns: 35,
            follow: true,
            follow_task: None,
        }
    }
}

struct Pending {
    origin: Entity<BufferView>,
    action: PendingAction,
}

enum PendingAction {
    Read(Action),
    Command(Command),
}

pub(super) struct TreeWork {
    queued: VecDeque<Pending>,
    buffer: crate::buffer::BufferId,
    serial: u64,
    cancelled: Arc<AtomicBool>,
    _task: Task<()>,
}

impl Drop for TreeWork {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

impl AppShell {
    pub(super) fn toggle_tree(
        &mut self,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.minibuffer.is_some() {
            return;
        }
        if self.panes.side().is_some() {
            self.hide_tree(window, cx);
        } else {
            self.show_tree(origin, None, window, cx);
        }
    }

    fn hide_tree(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(side) = self.panes.side().copied() else {
            return;
        };
        let selected = *self.panes.selected();
        let buffer = self.body_panes[&side].current;
        self.select_pane(side, window, cx);
        self.window_command(Command::DeleteWindow, window, cx);
        if selected != side {
            self.select_pane(selected, window, cx);
        }
        self.buffers.bury(buffer);
    }

    fn show_tree(
        &mut self,
        origin: &Entity<BufferView>,
        root: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(side) = self.panes.side().copied() {
            self.select_pane(side, window, cx);
            return;
        }
        let existing = self
            .buffers
            .iter()
            .position(|slot| slot.buffer.tree_root().is_some());
        let (index, fresh) = match existing {
            Some(index) => (index, false),
            None => {
                let file = origin.read(cx).buffer().path();
                let root = root
                    .or_else(|| {
                        crate::project::root_for(file.as_deref(), &self.startup_directory).ok()
                    })
                    .filter(|root| crate::project::is_available(root));
                let Some(root) = root else {
                    self.begin_file_read(ReadKind::TreeRoot, origin, window, cx);
                    return;
                };
                let index = self
                    .buffers
                    .insert_background(BufferSlot::new(Buffer::tree(root)));
                self.rename_buffer(index, cx);
                (index, true)
            }
        };
        let id = self.buffers[index].id();
        let body = self
            .tree_window
            .hidden
            .take()
            .filter(|pane| pane.current == id)
            .unwrap_or_else(|| {
                let view = Self::create_view(self.buffers[index].buffer.new_view(), window, cx);
                let subscription = cx.observe(&view, Self::view_changed);
                let feedback = cx.subscribe(&view, Self::feedback);
                BodyPane::new(id, view, subscription, feedback)
            });
        let pane = self.next_pane;
        self.next_pane = self
            .next_pane
            .checked_add(1)
            .expect("pane identity exhausted");
        assert!(self.panes.show_side(pane));
        self.body_panes.insert(pane, body);
        for view in self.all_views() {
            view.update(cx, |view, _| view.invalidate_split_geometry());
        }
        self.select_pane(pane, window, cx);
        self.refresh_buffer(id, cx);
        if fresh {
            let target = self.active_view().clone();
            self.tree_command(Command::TreemacsRefresh, &target, window, cx);
        }
        cx.notify();
    }

    pub(super) fn tree_command(
        &mut self,
        command: Command,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use Command::*;
        if self.minibuffer.is_some() {
            return;
        }
        let buffer = origin.read(cx).buffer().snapshot_id().buffer;
        if let Some(work) = &mut self.tree_work
            && work.buffer == buffer
            && (!matches!(command, TreemacsToggle | TreemacsRefresh | TreemacsHidden)
                || work
                    .queued
                    .iter()
                    .any(|pending| matches!(pending.action, PendingAction::Command(_))))
        {
            work.queued.push_back(Pending {
                origin: origin.clone(),
                action: PendingAction::Command(command),
            });
            return;
        }
        match command {
            TreemacsNext
            | TreemacsPrevious
            | TreemacsNextNeighbour
            | TreemacsPreviousNeighbour
            | TreemacsCollapse
            | TreemacsCollapseParent
            | TreemacsParent => {
                let result = origin.read(cx).buffer().tree_navigation(command);
                match result {
                    Ok((point, action)) => {
                        origin.update(cx, |view, cx| view.show_location(point, window, cx));
                        if let Some(action) = action {
                            self.enqueue_tree(origin, action, window, cx);
                        }
                    }
                    Err(error) => self.show_message(origin, error, cx),
                }
            }
            TreemacsQuit => self.hide_tree(window, cx),
            TreemacsKill => {
                if let Some(index) = self.buffer_index(origin, cx) {
                    self.kill_buffer_at(index, window, cx);
                }
            }
            TreemacsReturn
            | TreemacsReturnStay
            | TreemacsReturnClose
            | TreemacsVisit
            | TreemacsVisitHorizontal
            | TreemacsVisitVertical
            | TreemacsVisitRecent
            | TreemacsVisitClose => {
                self.visit_tree_node(command, origin, window, cx);
            }
            TreemacsFollow => {
                self.tree_window.follow = !self.tree_window.follow;
                self.show_message(
                    origin,
                    if self.tree_window.follow {
                        "Follow mode enabled"
                    } else {
                        "Follow mode disabled"
                    },
                    cx,
                );
            }
            TreemacsWidth => {
                self.begin_read(ReadKind::TreeWidth, origin, "", window, cx);
            }
            TreemacsNarrow | TreemacsWiden | TreemacsFit => {
                let width = match command {
                    TreemacsNarrow => self.tree_columns(window, cx) as isize - 1,
                    TreemacsWiden => self.tree_columns(window, cx) as isize + 1,
                    _ => {
                        let buffer = origin.read(cx).buffer();
                        let text = buffer.text().to_string();
                        buffer
                            .sections()
                            .unwrap()
                            .visible_lines()
                            .iter()
                            .map(|line| {
                                text[line.start.0..line.end.0]
                                    .trim_end_matches('\n')
                                    .chars()
                                    .count()
                            })
                            .max()
                            .unwrap_or(1)
                            .max(1) as isize
                    }
                };
                let previous = self.tree_window.columns;
                self.set_tree_width(width, cx);
                if command != TreemacsFit && self.tree_columns(window, cx) as isize != width {
                    self.set_tree_width(previous, cx);
                    self.show_message(origin, "Could not change window width!", cx);
                }
            }
            TreemacsHidden => self.enqueue_tree(origin, Action::Hidden, window, cx),
            TreemacsToggle | TreemacsRefresh => {
                let result = origin
                    .read(cx)
                    .buffer()
                    .tree_action(command == TreemacsRefresh);
                match result {
                    Ok(action) => self.enqueue_tree(origin, action, window, cx),
                    Err(error) => self.show_message(origin, error, cx),
                }
            }
            _ => unreachable!(),
        }
    }

    fn enqueue_tree(
        &mut self,
        origin: &Entity<BufferView>,
        action: Action,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.buffer_index(origin, cx) else {
            return;
        };
        let id = self.buffers[index].id();
        let pending = Pending {
            origin: origin.clone(),
            action: PendingAction::Read(action),
        };
        if let Some(work) = &mut self.tree_work
            && work.buffer == id
        {
            work.queued.push_back(pending);
            return;
        }
        self.tree_work = None;
        self.start_tree_queue(VecDeque::from([pending]), window, cx);
    }

    fn start_tree_queue(
        &mut self,
        mut queued: VecDeque<Pending>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (origin, id, request, locate) = loop {
            let Some(pending) = queued.pop_front() else {
                return;
            };
            let Some(index) = self.buffer_index(&pending.origin, cx) else {
                continue;
            };
            let action = match pending.action {
                PendingAction::Read(action) => action,
                PendingAction::Command(command) => {
                    if self.active_view() == &pending.origin {
                        self.tree_command(command, &pending.origin, window, cx);
                    }
                    if let Some(work) = &mut self.tree_work {
                        work.queued.append(&mut queued);
                        return;
                    }
                    continue;
                }
            };
            let locate = if let Action::Locate(path) = &action {
                Some(path.clone())
            } else {
                None
            };
            let result = pending.origin.read(cx).buffer().begin_tree_action(action);
            match result {
                Ok(request) => break (pending.origin, self.buffers[index].id(), request, locate),
                Err(error) => self.show_message(&pending.origin, error, cx),
            }
        };
        let serial = request.serial;
        let root = request.root.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = cancelled.clone();
        #[cfg(test)]
        let gate = self.tree_read_gate.take();
        let task = cx.spawn_in(window, async move |shell, cx| {
            #[cfg(test)]
            if let Some(gate) = gate {
                let _ = gate.await;
            }
            let result = cx
                .background_executor()
                .spawn(async move { request.read(&worker_cancelled) })
                .await;
            shell
                .update_in(cx, |shell, window, cx| {
                    if !shell
                        .tree_work
                        .as_ref()
                        .is_some_and(|work| work.buffer == id && work.serial == serial)
                    {
                        return;
                    }
                    let mut work = shell.tree_work.take().unwrap();
                    let queued = std::mem::take(&mut work.queued);
                    let Some(index) = shell.buffers.index_of(id) else {
                        return;
                    };
                    match result {
                        Ok((data, rendered)) => {
                            let views: Vec<_> = shell
                                .all_views()
                                .into_iter()
                                .filter(|view| view.read(cx).buffer().snapshot_id().buffer == id)
                                .map(|view| {
                                    let anchor = view.read(cx).section_scroll_anchor();
                                    (view, anchor)
                                })
                                .collect();
                            if shell.buffers[index]
                                .buffer
                                .install_tree(serial, &root, data, rendered)
                            {
                                shell.refresh_buffer(id, cx);
                                for (view, anchor) in views {
                                    view.update(cx, |view, cx| {
                                        view.restore_section_scroll(anchor, cx);
                                    });
                                }
                                if let Some(path) = &locate {
                                    let heading = origin.read(cx).buffer().tree_heading(path);
                                    if let Some(heading) = heading {
                                        origin.update(cx, |view, cx| {
                                            view.show_location(heading, window, cx)
                                        });
                                    }
                                }
                            }
                        }
                        Err(error) => {
                            shell.show_message(&origin, format!("Tree unchanged: {error}"), cx)
                        }
                    }
                    shell.start_tree_queue(queued, window, cx);
                })
                .ok();
        });
        self.tree_work = Some(TreeWork {
            queued,
            buffer: id,
            serial,
            cancelled,
            _task: task,
        });
    }

    fn visit_tree_node(
        &mut self,
        command: Command,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use Command::*;
        let node = match origin.read(cx).buffer().tree_node() {
            Ok(node) => node,
            Err(error) => {
                self.show_message(origin, error, cx);
                return;
            }
        };
        if node.directory {
            if matches!(
                command,
                TreemacsReturn | TreemacsReturnStay | TreemacsReturnClose
            ) {
                self.enqueue_tree(origin, Action::Toggle(node.path), window, cx);
            } else {
                self.show_message(origin, "Directory visits are not supported (no dired)", cx);
            }
            return;
        }
        let tree = origin.read(cx).buffer().snapshot_id().buffer;
        let ordinary = self.panes.ordinary_leaves();
        let existing = ordinary.iter().copied().find(|pane| {
            self.body_panes[pane]
                .active_view()
                .read(cx)
                .buffer()
                .path()
                .as_ref()
                == Some(&node.target)
        });
        let target = if command == TreemacsVisitRecent {
            self.panes.recent_ordinary().unwrap_or(ordinary[0])
        } else if matches!(command, TreemacsVisitHorizontal | TreemacsVisitVertical) {
            ordinary[0]
        } else {
            existing.unwrap_or(ordinary[0])
        };
        self.select_pane(target, window, cx);
        if matches!(command, TreemacsVisitHorizontal | TreemacsVisitVertical) {
            let split = if command == TreemacsVisitHorizontal {
                SplitWindowRight
            } else {
                SplitWindowBelow
            };
            let new = self.next_pane;
            self.window_command(split, window, cx);
            if self.next_pane == new {
                return;
            }
            self.select_pane(new, window, cx);
        }
        let disposition = match command {
            TreemacsReturnStay => Disposition::Stay,
            TreemacsReturnClose | TreemacsVisitClose => Disposition::Close,
            _ => Disposition::Select,
        };
        let target = self.active_view().clone();
        self.visit_file_with(
            node.target,
            &target,
            super::documents::VisitOptions {
                tree: Some(Visit { tree, disposition }),
                ..Default::default()
            },
            window,
            cx,
        );
    }

    pub(super) fn finish_tree_visit(
        &mut self,
        visit: Visit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(side) = self
            .panes
            .side()
            .copied()
            .filter(|side| self.body_panes[side].current == visit.tree)
        else {
            return;
        };
        match visit.disposition {
            Disposition::Select => {}
            Disposition::Stay => self.select_pane(side, window, cx),
            Disposition::Close => self.hide_tree(window, cx),
        }
    }

    pub(super) fn select_tree_window(
        &mut self,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.minibuffer.is_some() {
            return;
        }
        if self.panes.side() == Some(self.panes.selected()) {
            if let Some(previous) = self.panes.recent_ordinary() {
                self.select_pane(previous, window, cx);
            }
        } else {
            self.show_tree(origin, None, window, cx);
        }
    }

    pub(super) fn find_tree_file(
        &mut self,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let path = origin.read(cx).buffer().path();
        if let Some(path) = path {
            self.locate_tree_file(path, origin, window, cx);
        } else {
            self.begin_file_read(ReadKind::TreeFile, origin, window, cx);
        }
    }

    fn locate_tree_file(
        &mut self,
        path: PathBuf,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let selected = *self.panes.selected();
        self.show_tree(origin, None, window, cx);
        let tree = self
            .panes
            .side()
            .map(|side| self.body_panes[side].active_view().clone());
        self.select_pane(selected, window, cx);
        if let Some(tree) = tree {
            self.enqueue_tree(&tree, Action::Locate(path), window, cx);
        }
    }

    fn tree_columns(&self, window: &Window, cx: &App) -> usize {
        let Some(side) = self.panes.side() else {
            return self.tree_window.columns.max(2) as usize;
        };
        let view = self.body_panes[side].active_view().read(cx);
        let padding = view.column_width(0, cx);
        let column = view.column_width(1, cx) - padding;
        let available =
            window.viewport_size().width - crate::view::MIN_PANE_WIDTH - px(1.) - padding;
        let maximum = (f32::from(available) / f32::from(column)).floor().max(2.) as isize;
        self.tree_window.columns.clamp(2, maximum) as usize
    }

    fn set_tree_width(&mut self, width: isize, cx: &mut Context<Self>) {
        self.tree_window.columns = width;
        for view in self.all_views() {
            view.update(cx, |view, _| view.invalidate_split_geometry());
        }
        cx.notify();
    }

    pub(super) fn confirm_tree_read(
        &mut self,
        kind: ReadKind,
        id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = self.session_input(cx).unwrap_or_default();
        if kind == ReadKind::TreeWidth {
            let Ok(width) = input.trim().parse::<isize>() else {
                self.message_minibuffer("Enter an integer width", cx);
                return;
            };
            if self.end_session(id, window, cx).is_some() {
                self.set_tree_width(width, cx);
            }
            return;
        }
        let directory = self.file_read_directory(self.session.as_ref().unwrap().origin(), cx);
        let path = match crate::files::expand(Path::new(&input), &directory) {
            Ok(path) => path,
            Err(error) => {
                self.message_minibuffer(error.to_string(), cx);
                return;
            }
        };
        if kind == ReadKind::TreeRoot && !path.is_dir() {
            self.message_minibuffer("Select an existing directory", cx);
            return;
        }
        if let Some(origin) = self.end_session(id, window, cx) {
            if kind == ReadKind::TreeRoot {
                self.show_tree(&origin, Some(path), window, cx);
            } else {
                self.locate_tree_file(path, &origin, window, cx);
            }
        }
    }

    pub(super) fn schedule_tree_follow(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.tree_window.follow
            || self.tree_window.follow_task.is_some()
            || self.panes.side().is_none()
            || self.panes.side() == Some(self.panes.selected())
            || self.active_view().read(cx).buffer().path().is_none()
        {
            return;
        }
        self.start_tree_follow_timer(window, cx);
    }

    pub(super) fn tree_input_activity(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tree_window.follow_task.take().is_some() {
            self.start_tree_follow_timer(window, cx);
        }
    }

    fn start_tree_follow_timer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.tree_window.follow_task = Some(cx.spawn_in(window, async move |shell, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(200))
                .await;
            shell
                .update_in(cx, |shell, window, cx| {
                    shell.tree_window.follow_task = None;
                    if shell.minibuffer.is_some()
                        || shell.panes.side() == Some(shell.panes.selected())
                    {
                        return;
                    }
                    let Some(side) = shell.panes.side().copied() else {
                        return;
                    };
                    let buffer = shell.active_view().read(cx).buffer();
                    if buffer.mode() == crate::mode::ModeId::GIT_COMMIT {
                        return;
                    }
                    let Some(path) = buffer.path() else { return };
                    let tree = shell.body_panes[&side].active_view().clone();
                    if tree
                        .read(cx)
                        .buffer()
                        .tree_root()
                        .is_some_and(|root| path.starts_with(root))
                        && path.is_file()
                    {
                        shell.enqueue_tree(&tree, Action::Locate(path), window, cx);
                    }
                })
                .ok();
        }));
    }

    pub(super) fn discard_tree_window(
        &mut self,
        id: crate::buffer::BufferId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .tree_work
            .as_ref()
            .is_some_and(|work| work.buffer == id)
        {
            self.tree_work = None;
        }
        if let Some(side) = self.panes.side().copied()
            && self.body_panes[&side].current == id
        {
            let selected = *self.panes.selected();
            self.panes.select(&side);
            self.panes.close_selected();
            if selected != side {
                self.panes.select(&selected);
            }
            if let Some(pane) = self.body_panes.remove(&side) {
                pane.active_view()
                    .update(cx, |view, cx| view.suspend_input(cx));
            }
            if self.minibuffer.is_none() {
                self.active_view()
                    .clone()
                    .update(cx, |view, cx| view.restore_input(window, cx));
            }
        }
        if self
            .tree_window
            .hidden
            .as_ref()
            .is_some_and(|pane| pane.current == id)
        {
            self.tree_window.hidden = None;
        }
    }

    pub(super) fn render_body(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let main = self.render_panes(self.panes.root(), cx);
        let Some(side) = self.panes.side().copied() else {
            return main;
        };
        let width = self.body_panes[&side]
            .active_view()
            .read(cx)
            .column_width(self.tree_columns(window, cx), cx);
        h_flex()
            .items_stretch()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .overflow_hidden()
            .child(
                gpui_kit::component::v_flex()
                    .w(width)
                    .flex_shrink_0()
                    .h_full()
                    .min_h_0()
                    .child(self.render_panes(&Pane::Leaf(side), cx)),
            )
            .child(
                div()
                    .w(px(1.))
                    .h_full()
                    .flex_shrink_0()
                    .bg(crate::theme::ThemeSettings::global(cx)
                        .style(crate::theme::StyleRole::Separator)
                        .foreground),
            )
            .child(main)
            .into_any_element()
    }
}
