use std::ops::Range;

use unicode_width::UnicodeWidthChar;

use crate::text::{ByteOffset, Rope};

use super::TAB_WIDTH;

pub(super) fn advance(column: usize, ch: char) -> usize {
    if ch == '\t' {
        column + TAB_WIDTH as usize - column % TAB_WIDTH as usize
    } else {
        column + ch.width().unwrap_or(2)
    }
}

pub(super) fn column(text: &Rope, range: Range<usize>) -> usize {
    text.chunks_in_range(range)
        .flat_map(str::chars)
        .fold(0, advance)
}

pub(super) fn offset(text: &Rope, range: Range<usize>, goal: usize) -> ByteOffset {
    let mut column = 0;
    let mut offset = range.start;
    for ch in text.chunks_in_range(range).flat_map(str::chars) {
        if column >= goal {
            break;
        }
        column = advance(column, ch);
        offset += ch.len_utf8();
    }
    ByteOffset(offset)
}
