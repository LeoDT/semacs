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
        let mut base = parse(
            LanguageId::Rust,
            buffer.snapshot(),
            None,
            None,
            &AtomicBool::new(false),
        )
        .unwrap();
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
                LanguageId::Rust,
                buffer.snapshot(),
                Some(base),
                Some(change),
                &AtomicBool::new(false),
            )
            .unwrap();
            let fresh = parse(
                LanguageId::Rust,
                buffer.snapshot(),
                None,
                None,
                &AtomicBool::new(false),
            )
            .unwrap();
            assert_eq!(
                base.layers[0].tree.root_node().to_sexp(),
                fresh.layers[0].tree.root_node().to_sexp(),
                "seed {seed}"
            );
            assert_eq!(base.highlights.spans, fresh.highlights.spans, "seed {seed}");
        }
    }
}

#[test]
fn multilanguage_coalesced_edits_match_fresh_layer_forests() {
    for (path, source) in [
        ("a.jsx", "const n = <div title=\"中🙂\">{call(42)}</div>;\n"),
        (
            "a.ts",
            "interface User { name: string }\nconst n: number = 42;\n",
        ),
        ("a.tsx", "const n: Element = <div>{call(42)}</div>;\n"),
        ("a.css", ".box { width: 12px; content: \"中🙂\"; }\n"),
        (
            "a.html",
            "<script>const n = 42;</script><style>.box { width: 12px; }</style>\n",
        ),
        (
            "a.md",
            "# 中🙂\n\n> **bold**\n\n```html\n<script>const n = 42;</script>\n```\n",
        ),
    ] {
        let language = for_path(Some(Path::new(path))).unwrap();
        for seed in 0..8 {
            let mut rng = StdRng::seed_from_u64(seed);
            let mut buffer = Buffer::new(
                Some(path.into()),
                DecodedFile {
                    text: source.into(),
                    format: FileFormat::default(),
                },
            );
            let mut base = parse(
                language,
                buffer.snapshot(),
                None,
                None,
                &AtomicBool::new(false),
            )
            .unwrap();
            for _ in 0..12 {
                let mut change: Option<Change> = None;
                for _ in 0..rng.random_range(1..5) {
                    let text = buffer.text().to_string();
                    let boundaries: Vec<_> = text
                        .char_indices()
                        .map(|(i, _)| i)
                        .chain([text.len()])
                        .collect();
                    let start = rng.random_range(0..boundaries.len());
                    let end = rng.random_range(start..boundaries.len());
                    let range = boundaries[start]..boundaries[end];
                    let inserted = ["", "中🙂", "\n", "```js\n", "</script>", "**", "> ", "42"]
                        [rng.random_range(0..8)];
                    let next = super::tests::edit(&mut buffer, range, inserted);
                    if let Some(change) = &mut change {
                        change.compose(next);
                    } else {
                        change = Some(next);
                    }
                }
                base = parse(
                    language,
                    buffer.snapshot(),
                    Some(base),
                    change,
                    &AtomicBool::new(false),
                )
                .unwrap();
                super::language_tests::assert_matches_fresh(&base, &buffer);
            }
        }
    }
}
