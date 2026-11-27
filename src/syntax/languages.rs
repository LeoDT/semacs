use std::path::Path;
use std::sync::OnceLock;

use tree_sitter::{Language, Query};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(usize)]
pub(crate) enum LanguageId {
    Rust,
    JavaScript,
    TypeScript,
    Tsx,
    Markdown,
    Html,
    Css,
    Json,
    Json5,
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

    pub(crate) fn grammar(&self) -> Language {
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

const JS_PARAM_HIGHLIGHTS: &str = r#"
(formal_parameters
  [
    (identifier) @variable.parameter
    (array_pattern
      (identifier) @variable.parameter)
    (object_pattern
      [
        (pair_pattern value: (identifier) @variable.parameter)
        (shorthand_property_identifier_pattern) @variable.parameter
      ])
  ])
"#;
const JS_EXTRA_HIGHLIGHTS: &str = r#"
[(this) (super)] @variable.special
(arrow_function
  parameter: (identifier) @variable.parameter)
(call_expression
  function: (identifier) @function.call)
(call_expression
  function: (member_expression
    property: (property_identifier) @function.method.call))
((call_expression
  function: (identifier) @function.builtin)
 (#eq? @function.builtin "require")
 (#is-not? local))
"#;
const JSON_EXTRA_HIGHLIGHTS: &str = r#"
(pair
  key: (_) @string.special.key)
[ "{" "}" "[" "]" ] @punctuation.bracket
[ "," ":" ] @punctuation.delimiter
"#;
pub(super) static LANGUAGES: [Definition; 10] = [
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
            JS_PARAM_HIGHLIGHTS,
            JS_EXTRA_HIGHLIGHTS,
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
            JS_EXTRA_HIGHLIGHTS,
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
            JS_EXTRA_HIGHLIGHTS,
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
        Json,
        &["json"],
        &["json"],
        || tree_sitter_json::LANGUAGE.into(),
        &[tree_sitter_json::HIGHLIGHTS_QUERY, JSON_EXTRA_HIGHLIGHTS],
        "",
    ),
    Definition::new(
        Json5,
        &["json5", "jsonc"],
        &["json5", "jsonc"],
        || tree_sitter_json5_orchard::LANGUAGE.into(),
        &[include_str!("json5.scm")],
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
    pub(super) fn definition(self) -> &'static Definition {
        &LANGUAGES[self as usize]
    }

    pub(crate) fn grammar(self) -> Language {
        self.definition().grammar()
    }
}

pub(crate) fn for_path(path: Option<&Path>) -> Option<LanguageId> {
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
