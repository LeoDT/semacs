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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageMotion {
    pub point: ByteOffset,
    pub first_row: usize,
}

/// A read-only, display-compatible viewport supplied by the command caller.
pub trait CommandViewport {
    fn page(&self, point: ByteOffset, forward: bool) -> Result<PageMotion, RouteError>;
}
