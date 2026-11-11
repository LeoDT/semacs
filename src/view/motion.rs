use gpui_kit::{Bounds, Pixels};

use crate::display::{LayoutSettings, WrapIndex};
use crate::editing::{CommandViewport, PageMotion, RouteError, RowMotion};
use crate::input::DisplayId;
use crate::text::ByteOffset;

pub(super) struct Viewport<'a> {
    pub index: &'a WrapIndex,
    pub first_row: usize,
    pub visible_rows: usize,
}

impl<'a> Viewport<'a> {
    /// Commands may only use a wrap index that matches the current display and
    /// layout settings; stale geometry is reported as not ready instead of used.
    pub(super) fn for_view(
        index: Option<&'a WrapIndex>,
        content_bounds: Option<Bounds<Pixels>>,
        scroll: gpui_kit::Point<Pixels>,
        settings: &'a LayoutSettings,
        display: DisplayId,
    ) -> Option<Self> {
        let index = index?;
        let bounds = content_bounds
            .filter(|_| index.key().display == display && index.key().settings == *settings)?;
        let line_height = f32::from(settings.line_height);
        Some(Self {
            index,
            first_row: (f32::from(scroll.y) / line_height).floor() as usize,
            visible_rows: (f32::from(bounds.size.height) / line_height)
                .floor()
                .max(1.) as usize,
        })
    }
}

impl CommandViewport for Viewport<'_> {
    fn page(&self, point: ByteOffset, forward: bool) -> Result<PageMotion, RouteError> {
        let index = self.index;
        if !index.is_complete() {
            return Err(RouteError::LayoutNotReady);
        }
        let end = ByteOffset(index.text().len());
        if forward && point == end {
            return Err(RouteError::EndOfBuffer);
        }
        if !forward && point == ByteOffset(0) {
            return Err(RouteError::BeginningOfBuffer);
        }
        let visible = self.visible_rows.max(1);
        let last = index.row_count() - 1;
        let first = self.first_row.min(last);
        let step = visible.saturating_sub(2).max(1);
        if forward && first.saturating_add(visible) > last {
            return Ok(PageMotion {
                point: end,
                first_row: first,
            });
        }
        if !forward && first == 0 {
            return Ok(PageMotion {
                point: ByteOffset(0),
                first_row: 0,
            });
        }
        let first_row = if forward {
            first.saturating_add(step).min(last)
        } else {
            first.saturating_sub(step)
        };
        let row = index
            .row_for_offset(point)
            .ok_or(RouteError::LayoutNotReady)?;
        let target = row
            .index
            .clamp(first_row, first_row.saturating_add(visible - 1).min(last));
        let point = if target == row.index {
            point
        } else {
            index.row(target).ok_or(RouteError::LayoutNotReady)?.start
        };
        Ok(PageMotion { point, first_row })
    }

    fn adjacent_row(&self, point: ByteOffset, forward: bool) -> Result<RowMotion, RouteError> {
        let index = self.index;
        let row = index
            .row_for_offset(point)
            .ok_or(RouteError::LayoutNotReady)?;
        let current = row.start..ByteOffset(row.start.0 + row.row.content_len);
        let target = if forward {
            index.row(row.index + 1)
        } else {
            row.index
                .checked_sub(1)
                .and_then(|previous| index.row(previous))
        };
        // A missing row is a buffer edge only once the index covers the text;
        // before that it may simply lie beyond the scanned prefix.
        let target = match target {
            Some(target) => Some(target.start..ByteOffset(target.start.0 + target.row.content_len)),
            None if index.is_complete() => None,
            None => return Err(RouteError::LayoutNotReady),
        };
        Ok(RowMotion { current, target })
    }
}
