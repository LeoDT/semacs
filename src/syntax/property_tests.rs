use super::*;
use crate::buffer::{Buffer, Edit, EditSource};
use crate::files::{DecodedFile, FileFormat};
use crate::text::ByteOffset;
use rand::{Rng, SeedableRng, rngs::StdRng};

#[test]
fn coalesced_ranges_and_incremental_trees_match_fresh_parses_across_edit_sequences() {
    for seed in 0..32 {
        let mut rng = StdRng::seed_from_u64(seed);
        let mut buffer = Buffer::new(
            Some("random.rs".into()),
            DecodedFile {
                text: "fn main() { let 中文 = \"🙂\"; }\nconst A: usize = 1;\n".into(),
                format: FileFormat::default(),
            },
        );
        let mut base = parse(buffer.snapshot(), None, None, &AtomicBool::new(false)).unwrap();
        for _ in 0..12 {
            let mut change: Option<Change> = None;
            for _ in 0..rng.random_range(1..8) {
                let text = buffer.text().to_string();
                let boundaries: Vec<_> = text
                    .char_indices()
                    .map(|(i, _)| i)
                    .chain([text.len()])
                    .collect();
                let start = rng.random_range(0..boundaries.len());
                let end = rng.random_range(start..boundaries.len());
                let range = boundaries[start]..boundaries[end];
                let inserted = ["", "中🙂", "fn g() {}\n", "/*", "*/", "\"", "\n", "a"]
                    [rng.random_range(0..8)];
                buffer
                    .edit(Edit {
                        snapshot: buffer.snapshot_id(),
                        range: ByteOffset(range.start)..ByteOffset(range.end),
                        text: inserted,
                        source: EditSource::Command,
                    })
                    .unwrap();
                let next = Change {
                    new_end: range.start + inserted.len(),
                    old: range,
                };
                if let Some(change) = &mut change {
                    change.compose(next);
                } else {
                    change = Some(next);
                }
            }
            let change = change.unwrap();
            let before = base.snapshot.text().to_string();
            let after = buffer.text().to_string();
            assert_eq!(before[..change.old.start], after[..change.old.start]);
            assert_eq!(before[change.old.end..], after[change.new_end..]);
            base = parse(
                buffer.snapshot(),
                Some(base),
                Some(change),
                &AtomicBool::new(false),
            )
            .unwrap();
            let fresh = parse(buffer.snapshot(), None, None, &AtomicBool::new(false)).unwrap();
            assert_eq!(
                base.tree.root_node().to_sexp(),
                fresh.tree.root_node().to_sexp(),
                "seed {seed}"
            );
            assert_eq!(base.highlights.spans, fresh.highlights.spans, "seed {seed}");
        }
    }
}
