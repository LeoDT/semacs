use std::path::Path;
use std::sync::OnceLock;

use tree_sitter::{Language, Query};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(usize)]
pub(super) enum LanguageId {
    Rust,
    JavaScript,
    TypeScript,
    Tsx,
    Markdown,
    Html,
    Css,
    MarkdownInline,
}

pub(super) struct Definition {
    pub id: LanguageId,
    pub extensions: &'static [&'static str],
    pub aliases: &'static [&'static str],
    grammar: fn() -> Language,
    highlights: &'static [&'static str],
    injections: &'static str,
    query: OnceLock<Result<Query, String>>,
    injection_query: OnceLock<Result<Query, String>>,
}

impl Definition {
    const fn new(
        id: LanguageId,
        extensions: &'static [&'static str],
        aliases: &'static [&'static str],
        grammar: fn() -> Language,
        highlights: &'static [&'static str],
        injections: &'static str,
    ) -> Self {
        Self {
            id,
            extensions,
            aliases,
            grammar,
            highlights,
            injections,
            query: OnceLock::new(),
            injection_query: OnceLock::new(),
        }
    }

    pub fn grammar(&self) -> Language {
        (self.grammar)()
    }

    pub fn query(&self) -> Result<&Query, String> {
        self.query
            .get_or_init(|| {
                Query::new(&self.grammar(), &self.highlights.join("\n"))
                    .map_err(|error| format!("{:?} highlights: {error}", self.id))
            })
            .as_ref()
            .map_err(Clone::clone)
    }

    pub fn injections(&self) -> Result<Option<&Query>, String> {
        if self.injections.is_empty() {
            return Ok(None);
        }
        self.injection_query
            .get_or_init(|| {
                Query::new(&self.grammar(), self.injections)
                    .map_err(|error| format!("{:?} injections: {error}", self.id))
            })
            .as_ref()
            .map(Some)
            .map_err(Clone::clone)
    }
}

use LanguageId::*;
pub(super) static LANGUAGES: [Definition; 8] = [
    Definition::new(
        Rust,
        &["rs"],
        &["rust", "rs"],
        || tree_sitter_rust::LANGUAGE.into(),
        &[
            "(identifier) @variable",
            tree_sitter_rust::HIGHLIGHTS_QUERY,
            include_str!("rust.scm"),
        ],
        "",
    ),
    Definition::new(
        JavaScript,
        &["js", "mjs", "cjs", "jsx"],
        &["javascript", "js", "jsx", "mjs", "cjs"],
        || tree_sitter_javascript::LANGUAGE.into(),
        &[
            tree_sitter_javascript::HIGHLIGHT_QUERY,
            tree_sitter_javascript::JSX_HIGHLIGHT_QUERY,
        ],
        "",
    ),
    Definition::new(
        TypeScript,
        &["ts", "mts", "cts"],
        &["typescript", "ts", "mts", "cts"],
        || tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        &[
            tree_sitter_javascript::HIGHLIGHT_QUERY,
            tree_sitter_typescript::HIGHLIGHTS_QUERY,
        ],
        "",
    ),
    Definition::new(
        Tsx,
        &["tsx"],
        &["tsx"],
        || tree_sitter_typescript::LANGUAGE_TSX.into(),
        &[
            tree_sitter_javascript::HIGHLIGHT_QUERY,
            tree_sitter_typescript::HIGHLIGHTS_QUERY,
            tree_sitter_javascript::JSX_HIGHLIGHT_QUERY,
        ],
        "",
    ),
    Definition::new(
        Markdown,
        &["md", "markdown"],
        &["markdown", "md"],
        || tree_sitter_md::LANGUAGE.into(),
        &[tree_sitter_md::HIGHLIGHT_QUERY_BLOCK],
        include_str!("markdown-injections.scm"),
    ),
    Definition::new(
        Html,
        &["html", "htm"],
        &["html", "htm"],
        || tree_sitter_html::LANGUAGE.into(),
        &[tree_sitter_html::HIGHLIGHTS_QUERY],
        tree_sitter_html::INJECTIONS_QUERY,
    ),
    Definition::new(
        Css,
        &["css"],
        &["css"],
        || tree_sitter_css::LANGUAGE.into(),
        &[tree_sitter_css::HIGHLIGHTS_QUERY],
        "",
    ),
    Definition::new(
        MarkdownInline,
        &[],
        &["markdown_inline"],
        || tree_sitter_md::INLINE_LANGUAGE.into(),
        &[tree_sitter_md::HIGHLIGHT_QUERY_INLINE],
        "",
    ),
];

impl LanguageId {
    pub fn definition(self) -> &'static Definition {
        &LANGUAGES[self as usize]
    }
}

pub(super) fn for_path(path: Option<&Path>) -> Option<LanguageId> {
    let extension = path?.extension()?.to_str()?;
    LANGUAGES
        .iter()
        .find(|language| language.extensions.contains(&extension))
        .map(|l| l.id)
}

pub(super) fn for_alias(alias: &str) -> Option<LanguageId> {
    let alias = alias.trim().to_ascii_lowercase();
    LANGUAGES
        .iter()
        .find(|language| language.aliases.contains(&alias.as_str()))
        .map(|l| l.id)
}
