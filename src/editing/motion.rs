use crate::text::{ByteOffset, Point, Rope};

use super::column;

pub(super) fn word(text: &Rope, point: ByteOffset, forward: bool) -> ByteOffset {
    if forward {
        scan_word(text.chars_at(point.0), point, true)
    } else {
        scan_word(text.reversed_chars_at(point.0), point, false)
    }
}

fn scan_word(chars: impl Iterator<Item = char>, point: ByteOffset, forward: bool) -> ByteOffset {
    let mut offset = point.0;
    let mut in_word = false;
    for ch in chars {
        if in_word && !ch.is_alphanumeric() {
            break;
        }
        in_word |= ch.is_alphanumeric();
        if forward {
            offset += ch.len_utf8();
        } else {
            offset -= ch.len_utf8();
        }
    }
    ByteOffset(offset)
}

pub(super) fn goal(text: &Rope, point: ByteOffset) -> usize {
    let row = text.offset_to_point(point.0).row;
    let start = text.point_to_offset(Point::new(row, 0));
    column::column(text, start..point.0)
}

pub(super) fn line(text: &Rope, row: u32, goal: usize) -> ByteOffset {
    let start = text.point_to_offset(Point::new(row, 0));
    column::offset(text, start..start + text.line_len(row) as usize, goal)
}
