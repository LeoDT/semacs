use super::{Highlights, Span};

#[cfg(test)]
#[path = "display_tests.rs"]
mod tests;
use crate::buffer::SnapshotId;
use crate::text::Bias;
use crate::text::sum_tree::{ContextLessSummary, Item, SumTree};
use std::ops::Range;
use std::sync::Arc;

#[derive(Clone)]
struct Piece {
    len: usize,
    source: Option<usize>,
}

impl Piece {
    fn slice(&self, range: Range<usize>) -> Self {
        Self {
            len: range.len(),
            source: self.source.map(|start| start + range.start),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Bytes(usize);

impl ContextLessSummary for Bytes {
    fn zero() -> Self {
        Self(0)
    }

    fn add_summary(&mut self, other: &Self) {
        self.0 += other.0;
    }
}

impl Item for Piece {
    type Summary = Bytes;

    fn summary(&self, (): ()) -> Bytes {
        Bytes(self.len)
    }
}

/// Display-only mapping; its snapshot identifies coordinates, not parse completion.
pub(crate) struct DisplayHighlights {
    pub snapshot: SnapshotId,
    source: Arc<Highlights>,
    pieces: SumTree<Piece>,
}

impl DisplayHighlights {
    pub(super) fn new(source: Arc<Highlights>, len: usize) -> Self {
        Self {
            snapshot: source.snapshot,
            source,
            pieces: SumTree::from_iter(
                (len > 0).then_some(Piece {
                    len,
                    source: Some(0),
                }),
                (),
            ),
        }
    }

    pub(super) fn edited(&self, snapshot: SnapshotId, old: Range<usize>, new_end: usize) -> Self {
        let mut cursor = self.pieces.cursor::<Bytes>(());
        let mut pieces = cursor.slice(&Bytes(old.start), Bias::Left);
        if let Some(piece) = cursor.item()
            && old.start > cursor.start().0
        {
            pieces.push(piece.slice(0..old.start - cursor.start().0), ());
        }
        if new_end > old.start {
            pieces.push(
                Piece {
                    len: new_end - old.start,
                    source: None,
                },
                (),
            );
        }
        cursor.seek(&Bytes(old.end), Bias::Right);
        if let Some(piece) = cursor.item() {
            pieces.push(piece.slice(old.end - cursor.start().0..piece.len), ());
            cursor.next();
        }
        pieces.append(cursor.suffix(), ());
        Self {
            snapshot,
            source: self.source.clone(),
            pieces,
        }
    }

    pub fn in_range(&self, range: Range<usize>) -> impl Iterator<Item = Span> + '_ {
        let mut cursor = self.pieces.cursor::<Bytes>(());
        cursor.seek(&Bytes(range.start), Bias::Right);
        std::iter::from_fn(move || {
            let piece = cursor.item()?;
            let start = cursor.start().0;
            if start >= range.end || range.is_empty() {
                return None;
            }
            let local = range.start.saturating_sub(start)..piece.len.min(range.end - start);
            cursor.next();
            Some((piece, start, local))
        })
        .flat_map(move |(piece, start, local)| {
            piece.source.into_iter().flat_map(move |source| {
                self.source
                    .in_range(source + local.start..source + local.end)
                    .map(move |span| Span {
                        range: start + (span.range.start - source)
                            ..start + (span.range.end - source),
                        style: span.style,
                    })
            })
        })
    }

    pub fn in_display_range(
        &self,
        range: Range<usize>,
        marked: Option<Range<usize>>,
    ) -> impl Iterator<Item = Span> + '_ {
        let marked = marked.unwrap_or(range.end..range.end);
        let before = range.start.min(marked.start)..range.end.min(marked.start);
        let after =
            range.start.max(marked.end) - marked.len()..range.end.max(marked.end) - marked.len();
        self.in_range(before)
            .chain(self.in_range(after).map(move |mut span| {
                span.range.start += marked.len();
                span.range.end += marked.len();
                span
            }))
    }
}
