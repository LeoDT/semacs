use std::ops::Range;

use crate::text::ByteOffset;

use super::RouteError;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RecenterPosition {
    #[default]
    Center,
    Top,
    Bottom,
}

impl RecenterPosition {
    pub(super) fn next(self) -> Self {
        match self {
            Self::Center => Self::Top,
            Self::Top => Self::Bottom,
            Self::Bottom => Self::Center,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewportRequest {
    ScrollToRow(usize),
    Recenter(RecenterPosition),
    /// `recenter` with a negative argument: put the point row this many lines
    /// above the bottom of the window. `end-of-buffer` uses 3 when the end is
    /// not already visible.
    RecenterFromBottom(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageMotion {
    pub point: ByteOffset,
    pub first_row: usize,
}

/// The display row holding point and, when the buffer continues in the motion
/// direction, its neighbour. Ranges exclude the row's line break.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowMotion {
    pub current: Range<ByteOffset>,
    pub target: Option<Range<ByteOffset>>,
}

/// A read-only, display-compatible viewport supplied by the command caller.
pub trait CommandViewport {
    fn page(&self, point: ByteOffset, forward: bool) -> Result<PageMotion, RouteError>;
    /// One display row up or down, so vertical motion follows soft wrapping.
    /// `target` is `None` at the buffer edge.
    fn adjacent_row(&self, point: ByteOffset, forward: bool) -> Result<RowMotion, RouteError>;
}
