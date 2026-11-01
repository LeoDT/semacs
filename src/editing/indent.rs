use std::ops::Range;

use crate::text::{ByteOffset, Point, Rope};

use super::TAB_WIDTH;
use super::column::{advance, column};

pub(super) fn tab(
    text: &Rope,
    point: ByteOffset,
    floor: ByteOffset,
) -> (Range<ByteOffset>, String) {
    let position = text.offset_to_point(point.0);
    let line_start = text.point_to_offset(Point::new(position.row, 0));
    let current = column(text, line_start..point.0);
    let mut target = current + TAB_WIDTH as usize - current % TAB_WIDTH as usize;
    for row in (0..position.row).rev() {
        let start = text.point_to_offset(Point::new(row, 0));
        let mut previous = 0;
        let mut whitespace = true;
        let mut has_text = false;
        let mut field = None;
        for ch in text.chars_at(start).take_while(|ch| *ch != '\n') {
            let is_whitespace = matches!(ch, ' ' | '\t');
            if !is_whitespace {
                has_text = true;
                if whitespace && previous > current {
                    field = Some(previous);
                    break;
                }
            }
            whitespace = is_whitespace;
            previous = advance(previous, ch);
        }
        if let Some(field) = field {
            target = field;
        }
        if has_text {
            break;
        }
        if previous > current {
            target = previous;
            break;
        }
    }
    let before = text
        .reversed_chars_at(point.0)
        .take(point.0 - floor.0)
        .take_while(|ch| matches!(ch, ' ' | '\t'))
        .count();
    let start = point.0 - before;
    let mut from = column(text, line_start..start);
    let mut inserted = String::new();
    while from < target {
        let next = advance(from, '\t');
        if next <= target {
            inserted.push('\t');
            from = next;
        } else {
            inserted.push(' ');
            from += 1;
        }
    }
    (ByteOffset(start)..point, inserted)
}

pub(super) fn newline(
    text: &Rope,
    point: ByteOffset,
    floor: ByteOffset,
) -> (Range<ByteOffset>, String) {
    let row = text.offset_to_point(point.0).row;
    let line_start = text.point_to_offset(Point::new(row, 0)).max(floor.0);
    let before = text
        .reversed_chars_at(point.0)
        .take(point.0 - floor.0)
        .take_while(|ch| matches!(ch, ' ' | '\t'))
        .count();
    let after = text
        .chars_at(point.0)
        .take_while(|ch| matches!(ch, ' ' | '\t'))
        .count();
    let start = point.0 - before;
    let mut inserted = String::from("\n");
    if start > line_start {
        let width = TAB_WIDTH as usize;
        let column = text
            .chars_at(line_start)
            .take_while(|ch| matches!(ch, ' ' | '\t'))
            .fold(0, |column, ch| {
                if ch == '\t' {
                    column + width - column % width
                } else {
                    column + 1
                }
            });
        inserted.push_str(&"\t".repeat(column / width));
        inserted.push_str(&" ".repeat(column % width));
    }
    (ByteOffset(start)..ByteOffset(point.0 + after), inserted)
}
