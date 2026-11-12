use std::time::Duration;

use super::*;

/// How long a stationary pointer stays outside the view before another scroll step.
const AUTOSCROLL_DELAY: Duration = Duration::from_millis(250);

/// One left-button press over the text, from press to release.
pub(super) struct PointerDrag {
    pub(super) anchor: ByteOffset,
    pub(super) moved: bool,
    /// Last window position beyond the vertical edge, while it stays within the
    /// view's horizontal range.
    pub(super) exterior: Option<Point<Pixels>>,
    /// Repeating scroll task; dropping it cancels the remaining steps.
    scroll: Option<Task<()>>,
}

impl PointerDrag {
    fn new(anchor: ByteOffset) -> Self {
        Self {
            anchor,
            moved: false,
            exterior: None,
            scroll: None,
        }
    }
}

impl BufferView {
    pub(super) fn mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.accepting_input {
            return;
        }
        if self.target.input().is_composing() {
            self.set_message("Input method composition is active; finish it first", cx);
            return;
        }
        let Some(offset) = self
            .viewport_layout()
            .and_then(|layout| layout.hit_test(event.position))
        else {
            return;
        };
        self.resume_input(cx);
        if let Err(error) = self.target.press_pointer(self.target.token(), offset) {
            self.set_message(error.to_string(), cx);
            return;
        }
        self.pointer_drag = Some(PointerDrag::new(offset));
        self.invalidate_input_registration();
        self.style_scroll_anchor = None;
        self.pending_recenter = None;
        self.set_message("Mark set", cx);
        window.focus(&self.focus, cx);
        window.notify_text_selection_changed(&self.focus);
        cx.notify();
    }

    /// Pointer movement over this view: follows the pointer while it is inside
    /// the text, and ends a gesture whose button was released elsewhere.
    pub(super) fn mouse_moved(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pointer_drag.is_none() {
            return;
        }
        if !event.dragging() {
            self.finish_pointer(window, cx);
            return;
        }
        self.stop_autoscroll();
        let Some(offset) = self
            .viewport_layout()
            .and_then(|layout| layout.hit_test(event.position))
        else {
            return;
        };
        self.move_pointer(offset, window, cx);
    }

    pub(super) fn mouse_released(
        &mut self,
        _: &MouseUpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.finish_pointer(window, cx);
    }

    /// Pointer positions anywhere in the window while a press is tracked. Only
    /// positions outside the text are handled here; in-view movement goes
    /// through the element listener so a single event never updates twice.
    pub(super) fn window_pointer_moved(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pointer_drag.is_none() {
            return;
        }
        if !event.dragging() {
            self.finish_pointer(window, cx);
            return;
        }
        let (Some(view_bounds), Some(text_bounds)) = (self.view_bounds, self.content_bounds) else {
            return;
        };
        let position = event.position;
        if view_bounds.contains(&position) {
            return;
        }
        if position.x < view_bounds.left() || position.x > view_bounds.right() {
            self.stop_autoscroll();
            return;
        }
        if position.y >= text_bounds.top() && position.y <= text_bounds.bottom() {
            self.stop_autoscroll();
            return;
        }
        if let Some(drag) = &mut self.pointer_drag {
            drag.exterior = Some(position);
        }
        self.autoscroll_step(cx);
        self.schedule_autoscroll(window, cx);
    }

    pub(super) fn window_pointer_released(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.finish_pointer(window, cx);
    }

    /// The drag state must not outlive the input lifecycle that owns the press.
    pub(super) fn clear_pointer_drag(&mut self) {
        self.pointer_drag = None;
    }

    /// Keeps the dragging point on the newly visible edge of an auto-scrolled
    /// view. The edge offset is only known after the scrolled layout exists, so
    /// this runs from `prepare`.
    pub(super) fn sync_pointer_edge(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(position) = self.pointer_drag.as_ref().and_then(|drag| drag.exterior) else {
            return;
        };
        let Some(bounds) = self.geometry.as_ref().map(ViewportLayout::bounds) else {
            return;
        };
        let y = if position.y < bounds.top() {
            bounds.top()
        } else {
            (bounds.bottom() - px(1.)).max(bounds.top())
        };
        let Some(offset) = self
            .geometry
            .as_ref()
            .and_then(|layout| layout.hit_test(point(position.x, y)))
        else {
            return;
        };
        self.move_pointer(offset, window, cx);
    }

    fn move_pointer(&mut self, offset: ByteOffset, window: &mut Window, cx: &mut Context<Self>) {
        if offset == self.target.buffer().point() {
            return;
        }
        if let Err(error) = self.target.drag_pointer(self.target.token(), offset) {
            self.set_message(error.to_string(), cx);
            return;
        }
        if let Some(drag) = &mut self.pointer_drag
            && offset != drag.anchor
        {
            drag.moved = true;
        }
        window.notify_text_selection_changed(&self.focus);
        cx.notify();
    }

    fn finish_pointer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(drag) = self.pointer_drag.take() else {
            return;
        };
        self.target.release_pointer(!drag.moved);
        window.notify_text_selection_changed(&self.focus);
        cx.notify();
    }

    fn stop_autoscroll(&mut self) {
        let Some(drag) = &mut self.pointer_drag else {
            return;
        };
        drag.exterior = None;
        drag.scroll = None;
    }

    /// Rows between the pointer and the edge it is beyond, at least one.
    fn exterior_rows(&self) -> Option<(bool, usize)> {
        let position = self.pointer_drag.as_ref()?.exterior?;
        let bounds = self.content_bounds?;
        let line_height = f32::from(self.settings.line_height).max(1.);
        let (up, distance) = if position.y < bounds.top() {
            (true, f32::from(bounds.top() - position.y))
        } else if position.y > bounds.bottom() {
            (false, f32::from(position.y - bounds.bottom()))
        } else {
            return None;
        };
        Some((up, (distance / line_height).ceil().max(1.) as usize))
    }

    /// One scroll step of the gesture. Returns whether the view moved.
    fn autoscroll_step(&mut self, cx: &mut Context<Self>) -> bool {
        let Some((up, rows)) = self.exterior_rows() else {
            return false;
        };
        let step = self.settings.line_height * rows as f32;
        let desired = if up {
            self.scroll.y - step
        } else {
            self.scroll.y + step
        };
        let limit = self
            .index
            .as_ref()
            .filter(|index| index.is_complete())
            .map_or(f32::MAX, |index| {
                f32::from(self.settings.line_height) * index.row_count().saturating_sub(1) as f32
            });
        let target = px(f32::from(desired).clamp(0., limit));
        if target == self.scroll.y {
            return false;
        }
        self.scroll_by(point(px(0.), target - self.scroll.y), cx);
        true
    }

    fn schedule_autoscroll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(drag) = &mut self.pointer_drag else {
            return;
        };
        if drag
            .scroll
            .as_ref()
            .is_some_and(|scroll| !scroll.is_ready())
        {
            return;
        }
        drag.scroll = Some(cx.spawn_in(window, async move |view, cx| {
            loop {
                cx.background_executor().timer(AUTOSCROLL_DELAY).await;
                let keep = view
                    .update_in(cx, |view, _, cx| view.autoscroll_step(cx))
                    .unwrap_or(false);
                if !keep {
                    break;
                }
            }
        }));
    }

    /// Window-level tracking keeps the gesture alive outside the text, where
    /// element listeners no longer run. A pointer that leaves the window keeps
    /// its last outside position, so the scroll timer continues until the press
    /// ends or the buffer boundary is reached. Registered while a press is
    /// tracked.
    pub(super) fn register_pointer_listeners(
        view: Entity<Self>,
        window: &mut Window,
        cx: &mut App,
    ) {
        if view.read(cx).pointer_drag.is_none() {
            return;
        }
        let target = view.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
            if phase != DispatchPhase::Capture {
                return;
            }
            target.update(cx, |view, cx| {
                view.window_pointer_moved(event, window, cx);
            });
        });
        let target = view.clone();
        window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
            if phase != DispatchPhase::Capture || event.button != MouseButton::Left {
                return;
            }
            target.update(cx, |view, cx| view.window_pointer_released(window, cx));
        });
    }
}
