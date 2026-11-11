use crate::text::{ByteOffset, Point, Rope};

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

pub(super) fn paragraph(text: &Rope, point: ByteOffset, forward: bool) -> ByteOffset {
    let mut row = text.offset_to_point(point.0).row;
    let last_row = text.max_point().row;
    let line_start = |row| ByteOffset(text.point_to_offset(Point::new(row, 0)));
    let separator = |row| {
        let start = line_start(row).0;
        text.chunks_in_range(start..start + text.line_len(row) as usize)
            .all(|chunk| chunk.bytes().all(|byte| matches!(byte, b' ' | b'\t')))
    };
    if forward {
        while separator(row) {
            if row == last_row {
                return ByteOffset(text.len());
            }
            row += 1;
        }
        while !separator(row) {
            if row == last_row {
                return ByteOffset(text.len());
            }
            row += 1;
        }
    } else {
        while row > 0 && separator(row) {
            row -= 1;
        }
        while row > 0 && !separator(row) {
            row -= 1;
        }
    }
    line_start(row)
}
