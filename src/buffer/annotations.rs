use std::ops::Range;
use std::sync::Arc;

use gpui_kit::Hsla;

use super::{Buffer, EditError, SnapshotId};
use crate::text::ByteOffset;
use crate::theme::StyleOverride;

#[derive(Clone, Debug, PartialEq)]
pub struct StyledRange {
    pub range: Range<ByteOffset>,
    pub style: StyleOverride,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LineBackground {
    pub rows: Range<u32>,
    pub color: Hsla,
}

#[derive(Clone, Debug)]
pub(crate) struct Annotations {
    pub snapshot: SnapshotId,
    pub ranges: Arc<[StyledRange]>,
    pub lines: Arc<[LineBackground]>,
    pub fonts: Arc<[crate::display::FontSpan]>,
}

impl Buffer {
    pub fn set_annotations(
        &mut self,
        snapshot: SnapshotId,
        ranges: Vec<StyledRange>,
        lines: Vec<LineBackground>,
    ) -> Result<(), EditError> {
        self.validate_snapshot(snapshot)?;
        let text = self.text();
        let mut previous_end = ByteOffset(0);
        for span in &ranges {
            self.validate_range(&span.range)?;
            if span.range.is_empty() || span.range.start < previous_end {
                return Err(EditError::InvalidRange);
            }
            previous_end = span.range.end;
        }
        let mut previous_row = 0;
        for line in &lines {
            if line.rows.start >= line.rows.end
                || line.rows.start < previous_row
                || line.rows.end > text.max_point().row + 1
            {
                return Err(EditError::InvalidRange);
            }
            previous_row = line.rows.end;
        }
        let fonts = ranges
            .iter()
            .filter(|span| span.style.bold.is_some() || span.style.italic.is_some())
            .map(|span| crate::display::FontSpan {
                range: span.range.start.0..span.range.end.0,
                bold: span.style.bold,
                italic: span.style.italic,
            })
            .collect();
        let mut doc = self.document.borrow_mut();
        doc.annotations = Some(Arc::new(Annotations {
            snapshot,
            ranges: ranges.into(),
            lines: lines.into(),
            fonts,
        }));
        doc.touch();
        Ok(())
    }

    pub(crate) fn annotations(&self) -> Option<Arc<Annotations>> {
        let doc = self.document.borrow();
        doc.annotations
            .as_ref()
            .filter(|annotations| annotations.snapshot == doc.snapshot.id)
            .cloned()
    }
}

impl Annotations {
    pub fn line_color(&self, row: u32) -> Option<Hsla> {
        let index = self.lines.partition_point(|line| line.rows.end <= row);
        self.lines
            .get(index)
            .filter(|line| line.rows.contains(&row))
            .map(|line| line.color)
    }
}
