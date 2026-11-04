use super::*;
use crate::buffer::{Buffer, Edit, EditSource};
use crate::files::{DecodedFile, FileFormat};
use crate::syntax::{LanguageId, parse};
use crate::text::ByteOffset;
use crate::theme::StyleRole;
use rand::{Rng, SeedableRng, rngs::StdRng};
use std::sync::atomic::AtomicBool;

fn colors(
    display: &DisplayHighlights,
    range: Range<usize>,
    marked: Option<Range<usize>>,
) -> Vec<Option<StyleRole>> {
    let mut colors = vec![None; range.len()];
    for span in display.in_display_range(range.clone(), marked) {
        for byte in span.range {
            assert!(colors[byte - range.start].replace(span.style).is_none());
        }
    }
    colors
}

#[test]
fn persistent_mapping_matches_byte_oracle_through_unicode_edits_and_preedit() {
    for seed in 0..32 {
        let mut rng = StdRng::seed_from_u64(seed);
        let mut buffer = Buffer::new(
            Some("map.rs".into()),
            DecodedFile {
                text: "fn 中文() { let x = \"🙂\"; } // hi\n".repeat(20),
                format: FileFormat::default(),
            },
        );
        let parsed = parse(
            LanguageId::Rust,
            buffer.snapshot(),
            None,
            None,
            &AtomicBool::new(false),
        )
        .unwrap();
        let source = parsed.highlights;
        let mut display = DisplayHighlights::new(source.clone(), buffer.text().len());
        let mut expected = colors(&display, 0..buffer.text().len(), None);
        for step in 0..128 {
            let before = buffer.text().to_string();
            let boundaries: Vec<_> = before
                .char_indices()
                .map(|(i, _)| i)
                .chain([before.len()])
                .collect();
            let a = rng.random_range(0..boundaries.len());
            let b = rng.random_range(a..boundaries.len().min(a + 8));
            let old = boundaries[a]..boundaries[b];
            let inserted = ["", "中🙂", "a", "\n", " fn "][rng.random_range(0..5)];
            buffer
                .edit(Edit {
                    snapshot: buffer.snapshot_id(),
                    range: ByteOffset(old.start)..ByteOffset(old.end),
                    text: inserted,
                    source: EditSource::Command,
                })
                .unwrap();
            display = display.edited(
                buffer.snapshot_id(),
                old.clone(),
                old.start + inserted.len(),
            );
            expected.splice(old, vec![None; inserted.len()]);
            assert_eq!(display.snapshot, buffer.snapshot_id());
            assert!(Arc::ptr_eq(&source, &display.source));
            assert_eq!(display.pieces.summary().0, expected.len());
            assert!(display.pieces.iter().all(|piece| piece.len > 0));
            assert_eq!(
                colors(&display, 0..expected.len(), None),
                expected,
                "seed {seed}, step {step}"
            );
            let start = rng.random_range(0..=expected.len());
            let end = rng.random_range(start..=expected.len());
            assert_eq!(colors(&display, start..end, None), expected[start..end]);
            let marked = start..start + "中文".len();
            let mut projected = expected.clone();
            projected.splice(start..start, vec![None; marked.len()]);
            assert_eq!(
                colors(&display, 0..projected.len(), Some(marked.clone())),
                projected
            );
            let clip = start.saturating_sub(1)..marked.end + (expected.len() - start).min(3);
            assert_eq!(
                colors(&display, clip.clone(), Some(marked)),
                projected[clip]
            );
        }
        display = display.edited(buffer.snapshot_id(), 0..expected.len(), 0);
        assert!(display.pieces.is_empty());
        display = display.edited(buffer.snapshot_id(), 0..0, 7);
        assert_eq!(colors(&display, 0..7, None), vec![None; 7]);
    }
}
