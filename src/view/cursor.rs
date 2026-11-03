use super::{ShapedRow, ViewportLayout};
use crate::text::ByteOffset;
use gpui_kit::{Bounds, Pixels, ShapedLine, point, size};
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

pub(super) struct Cursor {
    pub row: usize,
    pub range: Range<ByteOffset>,
    pub bounds: Bounds<Pixels>,
}

// Scalar point movement may enter a grapheme or ligature; the painted block must not split either.
pub(super) fn cursor_stops(line: &ShapedLine) -> Vec<usize> {
    let mut glyph_starts: Vec<_> = line
        .runs
        .iter()
        .flat_map(|run| run.glyphs.iter().map(|glyph| glyph.index))
        .collect();
    glyph_starts.sort_unstable();
    glyph_starts.dedup();
    line.text
        .grapheme_indices(true)
        .map(|(start, _)| start)
        .filter(|start| *start == 0 || glyph_starts.binary_search(start).is_ok())
        .chain(std::iter::once(line.len()))
        .collect()
}

impl ShapedRow {
    fn cursor_cell(&self, offset: ByteOffset) -> Option<(Range<ByteOffset>, Pixels, Pixels)> {
        if offset < self.source.start
            || offset.0 > self.source.start.0 + self.source.row.content_len
        {
            return None;
        }
        for part in &self.parts {
            if offset < part.range.start || offset >= part.range.end {
                continue;
            }
            if let Some(line) = &part.line {
                let offset = offset.0 - part.range.start.0;
                let end = part.cursor_stops.partition_point(|stop| *stop <= offset);
                let start = part.cursor_stops[end - 1];
                let end = part.cursor_stops[end];
                let left = line.x_for_index(start);
                let right = line.x_for_index(end);
                return Some((
                    ByteOffset(part.range.start.0 + start)..ByteOffset(part.range.start.0 + end),
                    part.x + left,
                    if right > left {
                        right - left
                    } else {
                        self.space_width
                    },
                ));
            }
            return Some((part.range.clone(), part.x, self.space_width));
        }
        Some((offset..offset, self.width, self.space_width))
    }
}

impl ViewportLayout {
    pub(super) fn cursor(&self, offset: ByteOffset, row_index: usize) -> Option<Cursor> {
        let row = self.rows.iter().find(|row| row.source.index == row_index)?;
        let (range, x, width) = row.cursor_cell(offset)?;
        let origin = self.origin(row);
        let bounds = Bounds::new(
            point(origin.x + x, origin.y),
            size(width, self.key.settings.line_height),
        );
        if bounds.bottom() <= self.bounds.top() || bounds.top() >= self.bounds.bottom() {
            return None;
        }
        Some(Cursor {
            row: row_index,
            range,
            bounds,
        })
    }
}

#[cfg(test)]
mod tests;
