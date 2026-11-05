use super::*;
use crate::buffer::{Buffer, Edit, EditSource};
use crate::files::{DecodedFile, FileFormat};
use crate::text::ByteOffset;

fn buffer(text: &str) -> Buffer {
    Buffer::new(
        Some("sample.rs".into()),
        DecodedFile {
            text: text.into(),
            format: FileFormat::default(),
        },
    )
}

pub(super) fn edit(buffer: &mut Buffer, range: Range<usize>, text: &str) -> Change {
    buffer
        .edit(Edit {
            snapshot: buffer.snapshot_id(),
            range: ByteOffset(range.start)..ByteOffset(range.end),
            text,
            source: EditSource::Command,
        })
        .unwrap();
    Change {
        new_end: range.start + text.len(),
        old: range,
    }
}

fn parsed(buffer: &Buffer) -> Parsed {
    parse(
        LanguageId::Rust,
        buffer.snapshot(),
        None,
        None,
        &AtomicBool::new(false),
    )
    .unwrap()
}

pub(super) fn style_at(parsed: &Parsed, offset: usize) -> Option<StyleRole> {
    parsed
        .highlights
        .in_range(offset..offset + 1)
        .next()
        .map(|span| span.style)
}

#[test]
fn bundled_query_colors_rust_categories_and_tolerates_incomplete_syntax() {
    let text = "// 中🙂\nconst answer: i32 = 42;\nfn greet(name: String) { let s = r#\"fn false\"#; let b = true; greet(name); }\nfn incomplete(";
    let buffer = buffer(text);
    let parsed = parsed(&buffer);
    for (token, style) in [
        ("//", StyleRole::SyntaxComment),
        ("const", StyleRole::SyntaxKeyword),
        ("answer", StyleRole::SyntaxConstant),
        ("i32", StyleRole::SyntaxType),
        ("42", StyleRole::SyntaxNumber),
        ("greet", StyleRole::SyntaxFunction),
        ("name", StyleRole::SyntaxVariable),
        ("String", StyleRole::SyntaxType),
        ("r#", StyleRole::SyntaxString),
        ("true", StyleRole::SyntaxConstant),
        ("=", StyleRole::SyntaxOperator),
        (";", StyleRole::SyntaxPunctuation),
    ] {
        assert_eq!(
            style_at(&parsed, text.find(token).unwrap()),
            Some(style),
            "{token}"
        );
    }
    assert_eq!(
        style_at(&parsed, text.find("fn false").unwrap()),
        Some(StyleRole::SyntaxString)
    );
    assert!(parsed.layers[0].tree.root_node().has_error());
    for span in &parsed.highlights.spans {
        assert!(text.is_char_boundary(span.range.start));
        assert!(text.is_char_boundary(span.range.end));
    }
}

#[test]
fn incremental_parse_matches_fresh_after_coalesced_unicode_edits_and_history() {
    let mut buffer = buffer("fn 你好() { let text = \"🙂\"; }\n");
    let mut base = parsed(&buffer);
    let first = edit(&mut buffer, 0..0, "// 中🙂\n");
    let mut change = first;
    change.compose(edit(&mut buffer, 3..6, "中文\n"));
    let end = buffer.text().len();
    change.compose(edit(&mut buffer, end..end, "fn extra() {}\n"));
    change.compose(edit(&mut buffer, 0..3, "/*"));
    base = parse(
        LanguageId::Rust,
        buffer.snapshot(),
        Some(base),
        Some(change),
        &AtomicBool::new(false),
    )
    .unwrap();
    let fresh = parsed(&buffer);
    assert_eq!(
        base.layers[0].tree.root_node().to_sexp(),
        fresh.layers[0].tree.root_node().to_sexp()
    );
    assert_eq!(base.highlights.spans, fresh.highlights.spans);
    for redo in [false, false, true, true] {
        let applied = if redo { buffer.redo() } else { buffer.undo() }
            .unwrap()
            .unwrap();
        let change = Change {
            old: applied.old_range.start.0..applied.old_range.end.0,
            new_end: applied.new_range.end.0,
        };
        base = parse(
            LanguageId::Rust,
            buffer.snapshot(),
            Some(base),
            Some(change),
            &AtomicBool::new(false),
        )
        .unwrap();
        let fresh = parsed(&buffer);
        assert_eq!(
            base.layers[0].tree.root_node().to_sexp(),
            fresh.layers[0].tree.root_node().to_sexp()
        );
        assert_eq!(base.highlights.spans, fresh.highlights.spans);
    }
}

#[test]
fn cancelled_work_never_produces_a_result() {
    assert!(
        parse(
            LanguageId::Rust,
            buffer("fn main() {}").snapshot(),
            None,
            None,
            &AtomicBool::new(true)
        )
        .is_err()
    );
}

#[test]
fn language_detection_and_path_changes_do_not_depend_on_text_version() {
    for (name, expected) in [
        ("test.rs", true),
        ("test.RS", false),
        (".rs", false),
        ("test.rs.txt", false),
        ("test.txt", false),
    ] {
        assert_eq!(
            for_path(Some(Path::new(name))) == Some(LanguageId::Rust),
            expected
        );
    }
    assert_eq!(for_path(None), None);
    let mut buffer = buffer("fn main() {}");
    let snapshot = buffer.snapshot_id();
    assert!(!buffer.syntax_ready());
    buffer.set_path("plain.txt".into());
    assert!(buffer.syntax_ready());
    buffer.set_path("other.rs".into());
    assert!(!buffer.syntax_ready());
    assert_eq!(buffer.snapshot_id(), snapshot);
}

#[test]
fn display_projection_splits_committed_spans_around_uncolored_preedit() {
    let parsed = parsed(&buffer("fn main() {}"));
    let display = DisplayHighlights::new(parsed.highlights, parsed.snapshot.text().len());
    let spans: Vec<_> = display.in_display_range(0..15, Some(1..4)).collect();
    assert_eq!(
        spans[0],
        Span {
            range: 0..1,
            modifiers: Default::default(),
            style: StyleRole::SyntaxKeyword
        }
    );
    assert_eq!(
        spans[1],
        Span {
            range: 4..5,
            modifiers: Default::default(),
            style: StyleRole::SyntaxKeyword
        }
    );
    assert!(
        spans
            .iter()
            .all(|span| span.range.end <= 1 || span.range.start >= 4)
    );
    for range in [0..1, 1..2, 2..4, 3..8, 4..5, 8..13] {
        let clipped: Vec<_> = display
            .in_display_range(range.clone(), Some(1..4))
            .collect();
        assert!(
            clipped
                .iter()
                .all(|span| span.range.start >= range.start && span.range.end <= range.end)
        );
    }
}
