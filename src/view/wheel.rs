use std::time::{Duration, Instant};

use super::*;

const REPEAT_INTERVAL: Duration = Duration::from_millis(500);
const REPEAT_FUZZ: Pixels = px(3.);

#[derive(Clone, Copy, PartialEq, Eq)]
enum WheelDirection {
    Forward,
    Backward,
    Left,
    Right,
}

struct WheelClick {
    direction: WheelDirection,
    position: Point<Pixels>,
    time: Instant,
    count: isize,
}

#[derive(Default)]
pub(super) struct WheelState {
    remainder: Point<f32>,
    last: Option<WheelClick>,
}

#[derive(Default, Debug, PartialEq, Eq)]
struct WheelStep {
    rows: isize,
    columns: isize,
}

impl WheelState {
    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }

    fn step(
        &mut self,
        event: &ScrollEvent,
        line_height: Pixels,
        column_width: Pixels,
        now: Instant,
    ) -> WheelStep {
        let delta = match event.delta {
            ScrollDelta::Lines(delta) => point(-delta.x, -delta.y),
            ScrollDelta::Pixels(delta) => {
                point(-(delta.x / column_width), -(delta.y / line_height))
            }
        };
        if !delta.x.is_finite() || !delta.y.is_finite() {
            self.reset();
            return WheelStep::default();
        }
        self.remainder += delta;
        let rows = self.remainder.y.trunc() as isize;
        let columns = self.remainder.x.trunc() as isize;
        self.remainder -= point(columns as f32, rows as f32);
        if rows == 0 && columns == 0 {
            return WheelStep::default();
        }
        let direction = if rows > 0 {
            WheelDirection::Forward
        } else if rows < 0 {
            WheelDirection::Backward
        } else if columns > 0 {
            WheelDirection::Right
        } else {
            WheelDirection::Left
        };
        let count = self
            .last
            .as_ref()
            .filter(|last| {
                last.direction == direction
                    && now.saturating_duration_since(last.time) < REPEAT_INTERVAL
                    && (event.position.x - last.position.x).abs() <= REPEAT_FUZZ
                    && (event.position.y - last.position.y).abs() <= REPEAT_FUZZ
            })
            .map_or(1, |last| last.count.saturating_add(1));
        self.last = Some(WheelClick {
            direction,
            position: event.position,
            time: now,
            count,
        });
        // `mwheel-scroll` multiplies numeric vertical amounts by click count;
        // Shift's `hscroll` amount is symbolic and is not accelerated.
        WheelStep {
            rows: rows.saturating_mul(count),
            columns,
        }
    }
}

struct WheelMotion {
    scroll: Pixels,
    point: ByteOffset,
    boundary: Option<RouteError>,
    redisplay_point: Option<ByteOffset>,
}

fn forced_start(
    index: &WrapIndex,
    point: ByteOffset,
    start: ByteOffset,
    height: Pixels,
    line_height: Pixels,
) -> Result<WheelMotion, RouteError> {
    let start_row = index
        .row_for_offset(start)
        .or_else(|| {
            // A forced start inside a folded body overshoots the invisible text.
            let visible = index.key().settings.visible_lines.as_ref()?;
            let next = visible.partition_point(|line| line.start < start);
            index.row_for_offset(visible.get(next)?.start)
        })
        .ok_or(RouteError::LayoutNotReady)?;
    let first = start_row.index;
    let caret = index
        .row_for_offset(point)
        .ok_or(RouteError::LayoutNotReady)?
        .index;
    let top = line_height * caret as f32;
    let mut scroll = line_height * first as f32;
    let redisplay_point = if start == ByteOffset(index.text().len())
        || start_row.start == ByteOffset(index.text().len())
    {
        Some(start)
    } else if caret < first || top >= scroll + height {
        // A forced window start that hides point makes redisplay move it to
        // the middle, after mwheel's temporary-region check.
        let middle = ((height / line_height / 2.).ceil() as usize).saturating_sub(1);
        let target = first.saturating_add(middle);
        let target = if index.is_complete() {
            target.min(index.row_count() - 1)
        } else {
            target
        };
        Some(index.row(target).ok_or(RouteError::LayoutNotReady)?.start)
    } else {
        if top + line_height > scroll + height {
            scroll = (top - (height - line_height).max(px(0.)) / 2.).max(px(0.));
        }
        None
    };
    Ok(WheelMotion {
        scroll,
        point,
        boundary: None,
        redisplay_point,
    })
}

fn vertical_motion(
    index: &WrapIndex,
    point: ByteOffset,
    scroll: Pixels,
    height: Pixels,
    line_height: Pixels,
    rows: isize,
) -> Result<WheelMotion, RouteError> {
    let caret = index
        .row_for_offset(point)
        .ok_or(RouteError::LayoutNotReady)?;
    let visible = (height / line_height).floor().max(1.) as usize;
    let old_first = (scroll / line_height).floor() as usize;
    let top = line_height * caret.index as f32;
    let bottom = scroll + height;
    let vscroll = scroll - line_height * old_first as f32;
    let partial_top = top < scroll && top + line_height > scroll;
    let partial_bottom = top < bottom && top + line_height > bottom;
    if rows < 0 && partial_top && vscroll > px(0.) {
        return Ok(WheelMotion {
            scroll: scroll - vscroll,
            point,
            boundary: None,
            redisplay_point: None,
        });
    }
    if rows > 0 && partial_bottom {
        if vscroll > px(0.) || caret.index == old_first {
            return Ok(WheelMotion {
                scroll: scroll + (top + line_height - bottom).min(line_height * rows as f32),
                point,
                boundary: None,
                redisplay_point: None,
            });
        }
        // `auto-window-vscroll` handles a clipped point row before applying
        // the amount. Emacs uses the logical line beginning, even with wrap.
        let logical = index.text().offset_to_point(point.0).row;
        let start = ByteOffset(
            index
                .text()
                .point_to_offset(crate::text::Point::new(logical, 0)),
        );
        return forced_start(index, point, start, height, line_height);
    }
    if rows > 0 && partial_top && vscroll > px(0.) {
        let text = index.text();
        let logical = text.offset_to_point(point.0).row;
        let start = if logical < text.max_point().row {
            text.point_to_offset(crate::text::Point::new(logical + 1, 0))
        } else {
            text.len()
        };
        return forced_start(index, point, ByteOffset(start), height, line_height);
    }
    let first = if top + line_height <= scroll || top >= scroll + height {
        caret.index.saturating_sub(visible / 2)
    } else {
        old_first
    };
    let desired = first.saturating_add_signed(rows);
    let mut boundary = None;
    let target = if rows > 0 {
        if index
            .row(desired)
            .is_some_and(|row| row.start.0 < index.text().len())
        {
            desired
        } else if !index.is_complete() {
            return Err(RouteError::LayoutNotReady);
        } else {
            // At EOB `mwheel-scroll` retries whole screens until `scroll-up`
            // fails again; it does not replace an oversized amount by a clamp.
            boundary = Some(RouteError::EndOfBuffer);
            let last = index.row(index.row_count() - 1).unwrap();
            let max_start = last
                .index
                .saturating_sub(usize::from(last.start.0 == index.text().len()));
            let page = visible.saturating_sub(2).max(1);
            let pages = max_start.saturating_sub(first) / page;
            if pages == 0 {
                return Ok(WheelMotion {
                    scroll,
                    point,
                    boundary,
                    redisplay_point: None,
                });
            }
            first + pages * page
        }
    } else {
        if first == 0 {
            return Ok(WheelMotion {
                scroll: px(0.),
                point,
                boundary: Some(RouteError::BeginningOfBuffer),
                redisplay_point: None,
            });
        }
        desired
    };
    let edge = if rows > 0 && caret.index < target {
        Some(target)
    } else if rows < 0 && caret.index >= target.saturating_add(visible) {
        Some(target.saturating_add(visible - 1))
    } else {
        None
    };
    let point = match edge {
        Some(row) => index.row(row).ok_or(RouteError::LayoutNotReady)?.start,
        None => point,
    };
    Ok(WheelMotion {
        scroll: line_height * target as f32,
        point,
        boundary,
        redisplay_point: None,
    })
}

impl BufferView {
    pub(super) fn scroll_wheel(
        &mut self,
        event: &ScrollEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        if !self.accepting_input {
            return;
        }
        if self.target.input().is_composing() {
            self.set_message("Input method composition is active; finish it first", cx);
            return;
        }
        let Some(metrics) = self.metrics.as_mut() else {
            return;
        };
        if metrics.settings().font != self.settings.font
            || metrics.settings().font_size != self.settings.font_size
            || metrics.settings().typography != self.settings.typography
        {
            self.set_message(RouteError::LayoutNotReady.to_string(), cx);
            return;
        }
        let column_width = metrics.width(" ").max(px(1.));
        let step = self.wheel.step(
            event,
            self.settings.line_height,
            column_width,
            cx.background_executor().now(),
        );
        if step == WheelStep::default() {
            return;
        }
        let motion = if step.rows == 0 {
            Ok(WheelMotion {
                scroll: self.scroll.y,
                point: self.buffer().point(),
                boundary: None,
                redisplay_point: None,
            })
        } else {
            self.index
                .as_ref()
                .zip(self.content_bounds)
                .filter(|(index, _)| {
                    index.key().display == self.display_snapshot().id()
                        && index.key().settings == self.settings
                })
                .ok_or(RouteError::LayoutNotReady)
                .and_then(|(index, bounds)| {
                    vertical_motion(
                        index,
                        self.buffer().point(),
                        self.scroll.y,
                        bounds.size.height,
                        self.settings.line_height,
                        step.rows,
                    )
                })
        };
        let motion = match motion {
            Ok(motion) => motion,
            Err(error) => {
                self.set_message(error.to_string(), cx);
                return;
            }
        };
        if let Err(error) = self.target.scroll_point(motion.point, self.selected) {
            self.set_message(error.to_string(), cx);
            return;
        }
        if let Some(point) = motion.redisplay_point {
            self.target.show_point(point);
        }
        self.invalidate_input_registration();
        let horizontal = if step.columns == 0 {
            px(0.)
        } else {
            let column = (self.scroll.x / column_width).round();
            column_width * (column + step.columns as f32).max(0.) - self.scroll.x
        };
        if step.rows != 0 || horizontal != px(0.) {
            self.scroll_by(point(horizontal, motion.scroll - self.scroll.y), cx);
        }
        window.notify_text_selection_changed(&self.focus);
        match motion.boundary {
            Some(error) => self.set_message(error.to_string(), cx),
            None => self.clear_message(cx),
        }
    }
}

#[cfg(test)]
mod tests;
