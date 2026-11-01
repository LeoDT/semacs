use std::ops::Range;

use crate::text::ByteOffset;

#[derive(Clone, Copy)]
pub(super) struct FieldBoundary(pub ByteOffset);

impl FieldBoundary {
    pub fn range(self, point: ByteOffset, len: usize) -> Range<ByteOffset> {
        if point < self.0 {
            ByteOffset(0)..self.0
        } else {
            self.0..ByteOffset(len)
        }
    }
}
