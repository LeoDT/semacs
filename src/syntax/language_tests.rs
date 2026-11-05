use super::engine::capture_style;
use super::languages::{LANGUAGES, for_alias};
use super::tests::{edit, style_at};
use super::*;
use crate::buffer::Buffer;
use crate::files::{DecodedFile, FileFormat};

fn buffer(path: &str, text: &str) -> Buffer {
    Buffer::new(
        Some(path.into()),
        DecodedFile {
            text: text.into(),
            format: FileFormat::default(),
        },
    )
}

fn parsed(path: &str, text: &str) -> Parsed {
    parse(
        for_path(Some(Path::new(path))).unwrap(),
        buffer(path, text).snapshot(),
        None,
        None,
        &AtomicBool::new(false),
    )
    .unwrap()
}

fn assert_token(parsed: &Parsed, text: &str, token: &str, style: StyleRole) {
    assert_eq!(
        style_at(parsed, text.find(token).unwrap()),
        Some(style),
        "{token}"
    );
}

#[test]
fn every_registered_grammar_and_query_is_compatible_and_captures_are_mapped() {
    for definition in &LANGUAGES {
        let grammar = definition.grammar();
        assert!(
            (tree_sitter::MIN_COMPATIBLE_LANGUAGE_VERSION..=tree_sitter::LANGUAGE_VERSION)
                .contains(&grammar.abi_version())
        );
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&grammar).unwrap();
        for name in definition.query().unwrap().capture_names() {
            assert!(capture_style(name).is_some(), "{:?}: {name}", definition.id);
        }
        definition.injections().unwrap();
    }
}

#[test]
fn file_associations_and_fence_aliases_are_explicit() {
    for (suffixes, language) in [
        (&["rs"][..], LanguageId::Rust),
        (&["js", "mjs", "cjs", "jsx"][..], LanguageId::JavaScript),
        (&["ts", "mts", "cts"][..], LanguageId::TypeScript),
        (&["tsx"][..], LanguageId::Tsx),
        (&["md", "markdown"][..], LanguageId::Markdown),
        (&["html", "htm"][..], LanguageId::Html),
        (&["css"][..], LanguageId::Css),
    ] {
        for suffix in suffixes {
            assert_eq!(
                for_path(Some(Path::new(&format!("sample.{suffix}")))),
                Some(language)
            );
            assert_eq!(for_alias(&suffix.to_uppercase()), Some(language));
        }
    }
    for name in ["file.RS", ".md", "file.md.txt", "untitled", "a.py"] {
        assert_eq!(for_path(Some(Path::new(name))), None);
    }
    assert_eq!(for_path(None), None);
    assert_eq!(for_alias("unknown"), None);
    assert_eq!(for_alias("JavaScript"), Some(LanguageId::JavaScript));
    assert_eq!(for_alias("TypeScript"), Some(LanguageId::TypeScript));
}

#[test]
fn javascript_jsx_typescript_and_tsx_combine_queries() {
    for (path, text) in [
        (
            "a.jsx",
            "const node = <div title=\"hi\">{call(42)}</div>; // 中🙂",
        ),
        (
            "a.tsx",
            "const node: Element = <div title=\"hi\">{call(42)}</div>; // 中🙂",
        ),
    ] {
        let parsed = parsed(path, text);
        assert!(!parsed.layers[0].tree.root_node().has_error());
        for (token, style) in [
            ("const", StyleRole::SyntaxKeyword),
            ("div", StyleRole::SyntaxType),
            ("title", StyleRole::SyntaxVariable),
            ("hi", StyleRole::SyntaxString),
            ("call", StyleRole::SyntaxFunction),
            ("42", StyleRole::SyntaxNumber),
            ("//", StyleRole::SyntaxComment),
            ("<", StyleRole::SyntaxPunctuation),
        ] {
            assert_token(&parsed, text, token, style);
        }
    }
    let text = "interface User { name: string }\nfunction greet(user: User): number { return 42; }";
    let parsed = parsed("a.ts", text);
    for (token, style) in [
        ("interface", StyleRole::SyntaxKeyword),
        ("User", StyleRole::SyntaxType),
        ("string", StyleRole::SyntaxType),
        ("greet", StyleRole::SyntaxFunction),
        ("user:", StyleRole::SyntaxVariable),
        ("number", StyleRole::SyntaxType),
        ("return", StyleRole::SyntaxKeyword),
        ("42", StyleRole::SyntaxNumber),
    ] {
        assert_token(&parsed, text, token, style);
    }
}

#[test]
fn template_substitutions_override_the_enclosing_string() {
    let text = "const s = `hello ${call(42)}!`;";
    let parsed = parsed("a.js", text);
    assert_token(&parsed, text, "hello", StyleRole::SyntaxString);
    assert_token(&parsed, text, "call", StyleRole::SyntaxFunction);
    assert_token(&parsed, text, "42", StyleRole::SyntaxNumber);
    assert_token(&parsed, text, "!", StyleRole::SyntaxString);
}

#[test]
fn html_script_and_style_use_native_injection_ranges() {
    let text = "<!-- 中🙂 --><div class=\"a\">text</div><script>const n = call(42);</script><style>div { color: red; width: 12px; }</style>";
    let parsed = parsed("a.html", text);
    assert_eq!(parsed.layers.len(), 3);
    for (token, style) in [
        ("<!--", StyleRole::SyntaxComment),
        ("div", StyleRole::SyntaxType),
        ("class", StyleRole::SyntaxVariable),
        ("a\"", StyleRole::SyntaxString),
        ("const", StyleRole::SyntaxKeyword),
        ("call", StyleRole::SyntaxFunction),
        ("42", StyleRole::SyntaxNumber),
        ("color", StyleRole::SyntaxVariable),
        ("12", StyleRole::SyntaxNumber),
        ("px", StyleRole::SyntaxType),
    ] {
        assert_token(&parsed, text, token, style);
    }
    assert_eq!(style_at(&parsed, text.find("text").unwrap()), None);
}

#[test]
fn css_representative_syntax_is_colored() {
    let text =
        "/* 中🙂 */ @media screen { .box:hover { width: 12px; color: #fff; content: \"hi\"; } }";
    let parsed = parsed("a.css", text);
    for (token, style) in [
        ("/*", StyleRole::SyntaxComment),
        ("@media", StyleRole::SyntaxKeyword),
        ("box", StyleRole::SyntaxVariable),
        ("hover", StyleRole::SyntaxVariable),
        ("width", StyleRole::SyntaxVariable),
        ("12", StyleRole::SyntaxNumber),
        ("px", StyleRole::SyntaxType),
        ("fff", StyleRole::SyntaxString),
        ("hi", StyleRole::SyntaxString),
    ] {
        assert_token(&parsed, text, token, style);
    }
}

#[test]
fn markdown_blocks_inlines_tables_and_unknown_fences_have_defined_styles() {
    let text = "# Heading\n\n*italic* **bold** [label](https://example.com) `code`\n\n| Header |\n| --- |\n| *cell* |\n\n```unknown\nconst no_color = 42;\n```\n";
    let parsed = parsed("a.md", text);
    for (token, style) in [
        ("Heading", StyleRole::MarkupHeading),
        ("italic", StyleRole::MarkupEmphasis),
        ("bold", StyleRole::MarkupStrong),
        ("label", StyleRole::SyntaxVariable),
        ("https:", StyleRole::SyntaxString),
        ("code", StyleRole::SyntaxString),
        ("cell", StyleRole::MarkupEmphasis),
        ("const", StyleRole::Default),
    ] {
        assert_token(&parsed, text, token, style);
    }
    assert!(
        parsed
            .layers
            .iter()
            .any(|layer| layer.language == LanguageId::MarkdownInline)
    );
}

#[test]
fn markdown_fences_support_every_registered_public_language_and_html_children() {
    for (alias, code, token, style) in [
        ("rust", "fn main() {}", "fn", StyleRole::SyntaxKeyword),
        ("JS", "const n = 42;", "const", StyleRole::SyntaxKeyword),
        ("jsx", "const n = <div/>;", "div", StyleRole::SyntaxType),
        (
            "typescript",
            "interface User {}",
            "interface",
            StyleRole::SyntaxKeyword,
        ),
        (
            "tsx",
            "const n: Element = <div/>;",
            "Element",
            StyleRole::SyntaxType,
        ),
        (
            "css",
            ".box { width: 12px; }",
            "12",
            StyleRole::SyntaxNumber,
        ),
        (
            "html",
            "<script>const n = 42;</script>",
            "const",
            StyleRole::SyntaxKeyword,
        ),
        ("markdown", "# Heading", "Heading", StyleRole::MarkupHeading),
    ] {
        let text = format!("中🙂\n\n```{alias}\n{code}\n```\n");
        let parsed = parsed("a.md", &text);
        assert_token(&parsed, &text, token, style);
    }
}

#[test]
fn markdown_excluded_block_continuations_never_receive_inline_or_code_colors() {
    let text = "> **hello\n> world**\n\n> ```js\n> const n = 42;\n> ```\n";
    let parsed = parsed("a.md", text);
    assert_token(&parsed, text, "hello", StyleRole::MarkupStrong);
    assert_token(&parsed, text, "world", StyleRole::MarkupStrong);
    assert_token(&parsed, text, "const", StyleRole::SyntaxKeyword);
    assert_token(&parsed, text, "42", StyleRole::SyntaxNumber);
    for (offset, _) in text.match_indices("> ") {
        assert_eq!(
            style_at(&parsed, offset),
            Some(StyleRole::SyntaxPunctuation)
        );
    }
}

#[test]
fn multilayer_incremental_results_match_fresh_after_edits_and_history() {
    for (path, initial) in [
        (
            "a.md",
            "# 中🙂\n\n**bold**\n\n```html\n<script>const n = 42;</script><style>a { width: 12px; }</style>\n```\n",
        ),
        (
            "a.html",
            "<div>中🙂</div><script>const n = 42;</script><style>a { width: 12px; }</style>",
        ),
    ] {
        let language = for_path(Some(Path::new(path))).unwrap();
        let mut buffer = buffer(path, initial);
        let mut base = parse(
            language,
            buffer.snapshot(),
            None,
            None,
            &AtomicBool::new(false),
        )
        .unwrap();
        for (from, to) in [
            ("42", "'中🙂'"),
            ("script", "div"),
            ("div", "script"),
            ("12", "123"),
            ("const", "let"),
            ("中🙂", "中文\n"),
        ] {
            let source = buffer.text().to_string();
            let offset = source.find(from).unwrap();
            let change = edit(&mut buffer, offset..offset + from.len(), to);
            base = parse(
                language,
                buffer.snapshot(),
                Some(base),
                Some(change),
                &AtomicBool::new(false),
            )
            .unwrap();
            assert_matches_fresh(&base, &buffer);
        }
        for redo in [false, false, false, true, true, true] {
            let applied = if redo { buffer.redo() } else { buffer.undo() }
                .unwrap()
                .unwrap();
            let change = Change {
                old: applied.old_range.start.0..applied.old_range.end.0,
                new_end: applied.new_range.end.0,
            };
            base = parse(
                language,
                buffer.snapshot(),
                Some(base),
                Some(change),
                &AtomicBool::new(false),
            )
            .unwrap();
            assert_matches_fresh(&base, &buffer);
        }
    }
}

pub(super) fn assert_matches_fresh(base: &Parsed, buffer: &Buffer) {
    let fresh = parse(
        base.language,
        buffer.snapshot(),
        None,
        None,
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(base.highlights.spans, fresh.highlights.spans);
    assert_eq!(base.layers.len(), fresh.layers.len());
    for (old, new) in base.layers.iter().zip(&fresh.layers) {
        assert_eq!(old.language, new.language);
        assert_eq!(
            old.tree.root_node().to_sexp(),
            new.tree.root_node().to_sexp()
        );
    }
}

#[test]
fn cross_language_base_is_never_reused_and_all_languages_honor_cancellation() {
    let buffer = buffer("a.js", "const x = 42;");
    let base = parse(
        LanguageId::JavaScript,
        buffer.snapshot(),
        None,
        None,
        &AtomicBool::new(false),
    )
    .unwrap();
    let parsed = parse(
        LanguageId::Css,
        buffer.snapshot(),
        Some(base),
        None,
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(parsed.layers[0].language, LanguageId::Css);
    assert_matches_fresh(&parsed, &buffer);
    for definition in &LANGUAGES {
        assert!(
            parse(
                definition.id,
                buffer.snapshot(),
                None,
                None,
                &AtomicBool::new(true)
            )
            .is_err()
        );
    }
}

#[test]
fn empty_and_incomplete_documents_remain_available_for_every_language() {
    for definition in &LANGUAGES {
        for text in ["", "中🙂 < { ` \" /*\n"] {
            let buffer = buffer("a.txt", text);
            let result = parse(
                definition.id,
                buffer.snapshot(),
                None,
                None,
                &AtomicBool::new(false),
            )
            .unwrap();
            assert_eq!(result.highlights.snapshot, buffer.snapshot_id());
            let mut end = 0;
            for span in &result.highlights.spans {
                assert!(span.range.start >= end);
                assert!(
                    text.is_char_boundary(span.range.start)
                        && text.is_char_boundary(span.range.end)
                );
                assert!(span.range.start < span.range.end);
                end = span.range.end;
            }
        }
    }
}

#[test]
fn root_and_embedded_parsers_reuse_unchanged_subtrees() {
    for (path, prefix, suffix) in [("a.js", "", ""), ("a.html", "<script>", "</script>")] {
        let text = format!(
            "{prefix}{}{}",
            (0..200)
                .map(|i| format!("function f{i}() {{ return {i}; }}\n"))
                .collect::<String>(),
            suffix
        );
        let language = for_path(Some(Path::new(path))).unwrap();
        let mut buffer = buffer(path, &text);
        let base = parse(
            language,
            buffer.snapshot(),
            None,
            None,
            &AtomicBool::new(false),
        )
        .unwrap();
        let before = base
            .layers
            .last()
            .unwrap()
            .tree
            .root_node()
            .named_child(199)
            .unwrap()
            .id();
        let start = text.find("return 0").unwrap() + "return ".len();
        let change = edit(&mut buffer, start..start + 1, "42");
        let updated = parse(
            language,
            buffer.snapshot(),
            Some(base),
            Some(change),
            &AtomicBool::new(false),
        )
        .unwrap();
        let after = updated
            .layers
            .last()
            .unwrap()
            .tree
            .root_node()
            .named_child(199)
            .unwrap()
            .id();
        assert_eq!(before, after, "{path}: unchanged subtree should be reused");
        assert_matches_fresh(&updated, &buffer);
    }
}
