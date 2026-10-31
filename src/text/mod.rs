//! Persistent text storage and summary indexes imported from Zed.
//! See THIRD_PARTY.md for source versions, licenses and local adaptations.

pub mod rope;
pub mod sum_tree;
mod util;

#[cfg(test)]
mod test_support;

pub use rope::{OffsetUtf16, Point, PointUtf16, Rope};
pub use sum_tree::Bias;

/// A UTF-8 byte position, never a character count or UTF-16 code unit count.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ByteOffset(pub usize);

/// Clip platform ranges to scalar boundaries; a collapsed range stays collapsed.
pub fn clip_utf16_range(
    text: &Rope,
    range: std::ops::Range<OffsetUtf16>,
) -> Option<std::ops::Range<ByteOffset>> {
    if range.start > range.end {
        return None;
    }
    let start = text.clip_offset_utf16(range.start, Bias::Left);
    let end = if range.is_empty() {
        start
    } else {
        text.clip_offset_utf16(range.end, Bias::Right)
    };
    Some(
        ByteOffset(text.offset_utf16_to_offset(start))
            ..ByteOffset(text.offset_utf16_to_offset(end)),
    )
}

pub fn range_to_utf16(
    text: &Rope,
    range: std::ops::Range<ByteOffset>,
) -> Option<std::ops::Range<OffsetUtf16>> {
    if range.start > range.end
        || !text.is_char_boundary(range.start.0)
        || !text.is_char_boundary(range.end.0)
    {
        return None;
    }
    Some(text.offset_to_offset_utf16(range.start.0)..text.offset_to_offset_utf16(range.end.0))
}
