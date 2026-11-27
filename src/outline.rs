//! Tree-sitter structural outline extraction and project-wide symbol cache.

use std::collections::{HashMap, VecDeque};
use std::ops::{ControlFlow, Range};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use gpui_kit::Global;
use rayon::prelude::*;
use tree_sitter::{Node, ParseOptions, Parser, Tree};

use crate::buffer::BufferSnapshot;
use crate::files::{self, FileStamp};
use crate::project::{self, Project};
use crate::syntax::{LanguageId, language_for_path, supports_path};
use crate::text::{Bias, Point, Rope};
use crate::theme::StyleRole;

pub const MAX_SYMBOLS_PER_FILE: usize = 4_096;
pub const MAX_OUTLINE_FILE_BYTES: u64 = 4 * 1024 * 1024;
pub const MAX_NAME_CHARS: usize = 120;
pub const MAX_PROJECT_OUTLINE_FILES: usize = 10_000;
pub const MAX_PROJECT_SYMBOLS: usize = 200_000;
const MAX_JSON_DEPTH: usize = 4;
const MAX_CACHED_PROJECTS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SymbolKind {
    Function,
    Method,
    Macro,
    Struct,
    Enum,
    Union,
    Trait,
    Impl,
    Module,
    Type,
    Const,
    Static,
    Class,
    Interface,
    Variable,
    Heading(u8),
    Tag,
    Selector,
    Key,
}

impl SymbolKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Function => "fn",
            Self::Method => "method",
            Self::Macro => "macro",
            Self::Struct => "struct",
            Self::Enum => "enum",
            Self::Union => "union",
            Self::Trait => "trait",
            Self::Impl => "impl",
            Self::Module => "mod",
            Self::Type => "type",
            Self::Const => "const",
            Self::Static => "static",
            Self::Class => "class",
            Self::Interface => "interface",
            Self::Variable => "var",
            Self::Heading(1) => "h1",
            Self::Heading(2) => "h2",
            Self::Heading(3) => "h3",
            Self::Heading(4) => "h4",
            Self::Heading(5) => "h5",
            Self::Heading(_) => "h6",
            Self::Tag => "tag",
            Self::Selector => "rule",
            Self::Key => "key",
        }
    }

    pub fn style_role(self) -> StyleRole {
        match self {
            Self::Function => StyleRole::SyntaxFunction,
            Self::Method => StyleRole::SyntaxFunctionMethod,
            Self::Macro => StyleRole::SyntaxFunctionMacro,
            Self::Struct
            | Self::Enum
            | Self::Union
            | Self::Trait
            | Self::Impl
            | Self::Module
            | Self::Type
            | Self::Class
            | Self::Interface => StyleRole::SyntaxType,
            Self::Const | Self::Static => StyleRole::SyntaxConstant,
            Self::Variable => StyleRole::SyntaxVariable,
            Self::Heading(_) => StyleRole::MarkupHeading,
            Self::Tag | Self::Selector => StyleRole::SyntaxTag,
            Self::Key => StyleRole::SyntaxStringSpecialKey,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    pub range: Range<usize>,
    pub selection_start: usize,
    pub line: usize,
    pub column: usize,
    pub children: Vec<Symbol>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlatSymbol {
    pub qualified_name: String,
    pub name: String,
    pub kind: SymbolKind,
    pub line: usize,
    pub column: usize,
    pub byte_offset: usize,
    pub range: Range<usize>,
}

pub fn flatten(symbols: &[Symbol]) -> Vec<FlatSymbol> {
    fn walk(symbols: &[Symbol], prefix: &str, out: &mut Vec<FlatSymbol>) {
        for sym in symbols {
            let qualified_name = if prefix.is_empty() {
                sym.name.clone()
            } else {
                format!("{prefix} / {}", sym.name)
            };
            out.push(FlatSymbol {
                qualified_name: qualified_name.clone(),
                name: sym.name.clone(),
                kind: sym.kind,
                line: sym.line,
                column: sym.column,
                byte_offset: sym.selection_start,
                range: sym.range.clone(),
            });
            if !sym.children.is_empty() {
                walk(&sym.children, &qualified_name, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(symbols, "", &mut out);
    out
}

#[derive(Clone)]
pub(crate) struct OpenFileOverlay {
    pub snapshot: BufferSnapshot,
    pub language: LanguageId,
    pub tree: Option<Tree>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectSymbol {
    pub qualified_name: String,
    pub relative_path: String,
    pub path: PathBuf,
    pub kind: SymbolKind,
    pub line: usize,
    pub column: usize,
}

#[derive(Clone)]
pub struct CachedFileOutline {
    pub stamp: Option<FileStamp>,
    pub symbols: Arc<Vec<FlatSymbol>>,
}

#[derive(Default)]
pub struct ProjectOutlineCache {
    projects: HashMap<PathBuf, HashMap<PathBuf, CachedFileOutline>>,
    order: VecDeque<PathBuf>,
}

impl Global for ProjectOutlineCache {}

impl ProjectOutlineCache {
    pub fn snapshot_for(&self, root: &Path) -> HashMap<PathBuf, CachedFileOutline> {
        self.projects.get(root).cloned().unwrap_or_default()
    }

    pub fn install(&mut self, root: PathBuf, files: HashMap<PathBuf, CachedFileOutline>) {
        self.order.retain(|existing| existing != &root);
        self.order.push_front(root.clone());
        self.projects.insert(root, files);
        while self.order.len() > MAX_CACHED_PROJECTS {
            if let Some(evicted) = self.order.pop_back() {
                self.projects.remove(&evicted);
            }
        }
    }
}

pub struct ProjectScanResult {
    pub symbols: Vec<ProjectSymbol>,
    pub cache: HashMap<PathBuf, CachedFileOutline>,
}

pub(crate) fn extract_from_tree(
    language: LanguageId,
    text: &Rope,
    tree: &Tree,
    cancelled: &AtomicBool,
) -> Result<Vec<Symbol>, String> {
    let source = RopeSource(text);
    extract_with_source(language, &source, tree, cancelled)
}

pub(crate) fn extract_from_rope(
    language: LanguageId,
    text: &Rope,
    cancelled: &AtomicBool,
) -> Result<Vec<Symbol>, String> {
    let tree = parse_rope(language, text, cancelled)?;
    extract_from_tree(language, text, &tree, cancelled)
}

pub(crate) fn extract_from_str(
    language: LanguageId,
    text: &str,
    cancelled: &AtomicBool,
) -> Result<Vec<Symbol>, String> {
    let tree = parse_str(language, text, cancelled)?;
    let source = StrSource::new(text);
    extract_with_source(language, &source, &tree, cancelled)
}

pub(crate) fn extract_from_file(
    path: &Path,
    cancelled: &AtomicBool,
) -> Result<Vec<Symbol>, String> {
    check_cancelled(cancelled)?;
    let Some(language) = language_for_path(Some(path)) else {
        return Ok(Vec::new());
    };
    let metadata =
        std::fs::metadata(path).map_err(|error| format!("{}: {error}", path.display()))?;
    if metadata.len() > MAX_OUTLINE_FILE_BYTES {
        return Err(format!(
            "{}: exceeds outline size limit ({} bytes)",
            path.display(),
            MAX_OUTLINE_FILE_BYTES
        ));
    }
    let loaded = files::load(path).map_err(|error| format!("{}: {error}", path.display()))?;
    extract_from_str(language, &loaded.file.text, cancelled)
}

pub(crate) fn extract_from_overlay(
    overlay: &OpenFileOverlay,
    cancelled: &AtomicBool,
) -> Result<Vec<Symbol>, String> {
    match &overlay.tree {
        Some(tree) => extract_from_tree(overlay.language, overlay.snapshot.text(), tree, cancelled),
        None => extract_from_rope(overlay.language, overlay.snapshot.text(), cancelled),
    }
}

pub(crate) fn scan_project(
    project: &Project,
    open_files: &HashMap<PathBuf, OpenFileOverlay>,
    cached_files: &HashMap<PathBuf, CachedFileOutline>,
    cancelled: &AtomicBool,
) -> Result<ProjectScanResult, String> {
    check_cancelled(cancelled)?;
    let listed = project::list_files(project, cancelled)?;
    let mut targets = Vec::new();
    for relative in listed {
        check_cancelled(cancelled)?;
        let path = project.root.join(&relative);
        if supports_path(&path) {
            targets.push((relative, path));
            if targets.len() >= MAX_PROJECT_OUTLINE_FILES {
                break;
            }
        }
    }

    enum WorkItem {
        Ready {
            relative: String,
            path: PathBuf,
            symbols: Arc<Vec<FlatSymbol>>,
            cache_entry: Option<CachedFileOutline>,
        },
        ParseDisk {
            relative: String,
            path: PathBuf,
            language: LanguageId,
        },
    }

    let mut work_items = Vec::with_capacity(targets.len());
    for (relative, path) in targets {
        check_cancelled(cancelled)?;
        if let Some(overlay) = open_files.get(&path) {
            let symbols = extract_from_overlay(overlay, cancelled)
                .map(|syms| Arc::new(flatten(&syms)))
                .unwrap_or_else(|_| Arc::new(Vec::new()));
            work_items.push(WorkItem::Ready {
                relative,
                path,
                symbols,
                cache_entry: None,
            });
            continue;
        }
        let Some(language) = language_for_path(Some(&path)) else {
            continue;
        };
        if let Ok(Some(stamp)) = files::stamp(&path)
            && stamp.len() <= MAX_OUTLINE_FILE_BYTES
            && let Some(cached) = cached_files.get(&path)
            && cached.stamp == Some(stamp)
        {
            work_items.push(WorkItem::Ready {
                relative,
                path,
                symbols: cached.symbols.clone(),
                cache_entry: Some(cached.clone()),
            });
            continue;
        }
        work_items.push(WorkItem::ParseDisk {
            relative,
            path,
            language,
        });
    }

    struct ParsedProjectFile {
        relative: String,
        path: PathBuf,
        symbols: Arc<Vec<FlatSymbol>>,
        cache_entry: Option<CachedFileOutline>,
    }

    let parsed: Vec<ParsedProjectFile> = work_items
        .into_par_iter()
        .filter_map(|item| {
            if cancelled.load(Ordering::Relaxed) {
                return None;
            }
            match item {
                WorkItem::Ready {
                    relative,
                    path,
                    symbols,
                    cache_entry,
                } => Some(ParsedProjectFile {
                    relative,
                    path,
                    symbols,
                    cache_entry,
                }),
                WorkItem::ParseDisk {
                    relative,
                    path,
                    language,
                } => {
                    let stamp = files::stamp(&path).ok().flatten();
                    if stamp.is_some_and(|stamp| stamp.len() > MAX_OUTLINE_FILE_BYTES) {
                        return None;
                    }
                    let loaded = files::load(&path).ok()?;
                    if loaded.file.text.len() as u64 > MAX_OUTLINE_FILE_BYTES {
                        return None;
                    }
                    let syms = extract_from_str(language, &loaded.file.text, cancelled).ok()?;
                    let flat = Arc::new(flatten(&syms));
                    let entry = CachedFileOutline {
                        stamp: loaded.stamp.or(stamp),
                        symbols: flat.clone(),
                    };
                    Some(ParsedProjectFile {
                        relative,
                        path,
                        symbols: flat,
                        cache_entry: Some(entry),
                    })
                }
            }
        })
        .collect();

    check_cancelled(cancelled)?;

    let mut next_cache = HashMap::new();
    let mut entries = Vec::with_capacity(parsed.len());
    for file in parsed {
        if let Some(entry) = file.cache_entry {
            next_cache.insert(file.path.clone(), entry);
        } else if let Some(existing) = cached_files.get(&file.path) {
            next_cache.insert(file.path.clone(), existing.clone());
        }
        entries.push((file.relative, file.path, file.symbols));
    }
    entries.sort_by(|left, right| left.0.cmp(&right.0));

    let mut symbols = Vec::new();
    'outer: for (relative, path, flat) in entries {
        for sym in flat.iter() {
            if symbols.len() >= MAX_PROJECT_SYMBOLS {
                break 'outer;
            }
            symbols.push(ProjectSymbol {
                qualified_name: sym.qualified_name.clone(),
                relative_path: relative.clone(),
                path: path.clone(),
                kind: sym.kind,
                line: sym.line,
                column: sym.column,
            });
        }
    }

    Ok(ProjectScanResult {
        symbols,
        cache: next_cache,
    })
}

fn check_cancelled(cancelled: &AtomicBool) -> Result<(), String> {
    if cancelled.load(Ordering::Relaxed) {
        Err("Outline extraction cancelled".into())
    } else {
        Ok(())
    }
}

fn parse_rope(language: LanguageId, text: &Rope, cancelled: &AtomicBool) -> Result<Tree, String> {
    check_cancelled(cancelled)?;
    if text.len() > u32::MAX as usize {
        return Err("text exceeds tree-sitter byte range".into());
    }
    let mut parser = Parser::new();
    parser
        .set_language(&language.grammar())
        .map_err(|error| error.to_string())?;
    let mut progress = |_: &tree_sitter::ParseState| {
        if cancelled.load(Ordering::Relaxed) {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    };
    parser
        .parse_with_options(
            &mut |byte, _| {
                let start = text.clip_offset(byte, Bias::Left);
                let chunk = text.chunks_in_range(start..text.len()).next().unwrap_or("");
                &chunk.as_bytes()[byte - start..]
            },
            None,
            Some(ParseOptions::new().progress_callback(&mut progress)),
        )
        .ok_or_else(|| "Outline parse cancelled".to_owned())
}

fn parse_str(language: LanguageId, text: &str, cancelled: &AtomicBool) -> Result<Tree, String> {
    check_cancelled(cancelled)?;
    if text.len() > u32::MAX as usize {
        return Err("text exceeds tree-sitter byte range".into());
    }
    let mut parser = Parser::new();
    parser
        .set_language(&language.grammar())
        .map_err(|error| error.to_string())?;
    let mut progress = |_: &tree_sitter::ParseState| {
        if cancelled.load(Ordering::Relaxed) {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    };
    let bytes = text.as_bytes();
    parser
        .parse_with_options(
            &mut |byte, _| {
                if byte < bytes.len() {
                    &bytes[byte..]
                } else {
                    &[]
                }
            },
            None,
            Some(ParseOptions::new().progress_callback(&mut progress)),
        )
        .ok_or_else(|| "Outline parse cancelled".to_owned())
}

trait TextSource {
    fn slice(&self, range: Range<usize>) -> String;
    fn line_column(&self, offset: usize) -> (usize, usize);
}

struct RopeSource<'a>(&'a Rope);

impl TextSource for RopeSource<'_> {
    fn slice(&self, range: Range<usize>) -> String {
        let end = range.end.min(self.0.len());
        let start = range.start.min(end);
        self.0.slice(start..end).to_string()
    }

    fn line_column(&self, offset: usize) -> (usize, usize) {
        let offset = self.0.clip_offset(offset.min(self.0.len()), Bias::Left);
        let Point { row, column } = self.0.offset_to_point(offset);
        (row as usize + 1, column as usize)
    }
}

struct StrSource<'a> {
    text: &'a str,
    line_starts: Vec<usize>,
}

impl<'a> StrSource<'a> {
    fn new(text: &'a str) -> Self {
        let mut line_starts = vec![0];
        for (idx, byte) in text.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(idx + 1);
            }
        }
        Self { text, line_starts }
    }
}

impl TextSource for StrSource<'_> {
    fn slice(&self, range: Range<usize>) -> String {
        let end = range.end.min(self.text.len());
        let start = range.start.min(end);
        self.text[start..end].to_owned()
    }

    fn line_column(&self, offset: usize) -> (usize, usize) {
        let mut offset = offset.min(self.text.len());
        while !self.text.is_char_boundary(offset) {
            offset -= 1;
        }
        let line_idx = self
            .line_starts
            .partition_point(|&start| start <= offset)
            .saturating_sub(1);
        let column = offset.saturating_sub(self.line_starts[line_idx]);
        (line_idx + 1, column)
    }
}

struct Extractor<'a, S: TextSource> {
    source: &'a S,
    cancelled: &'a AtomicBool,
    count: usize,
}

impl<S: TextSource> Extractor<'_, S> {
    fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    fn can_push(&self) -> bool {
        self.count < MAX_SYMBOLS_PER_FILE && !self.cancelled()
    }

    fn node_text(&self, node: Node<'_>) -> String {
        self.source.slice(node.byte_range())
    }

    fn make_symbol(
        &mut self,
        name: String,
        kind: SymbolKind,
        node: Node<'_>,
        selection: Node<'_>,
        children: Vec<Symbol>,
    ) -> Option<Symbol> {
        if !self.can_push() {
            return None;
        }
        let name = normalize_name(&name);
        if name.is_empty() {
            return None;
        }
        self.count += 1;
        let selection_start = selection.start_byte();
        let (line, column) = self.source.line_column(selection_start);
        Some(Symbol {
            name,
            kind,
            range: node.byte_range(),
            selection_start,
            line,
            column,
            children,
        })
    }
}

fn normalize_name(raw: &str) -> String {
    let mut normalized = String::new();
    for word in raw.split_whitespace() {
        if !normalized.is_empty() {
            normalized.push(' ');
        }
        normalized.push_str(word);
    }
    let cut = normalized
        .char_indices()
        .nth(MAX_NAME_CHARS)
        .map(|(idx, _)| idx);
    if let Some(cut) = cut {
        normalized.truncate(cut);
        normalized.push('…');
    }
    normalized
}

fn strip_quotes(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.len() >= 2
        && ((trimmed.starts_with('"') && trimmed.ends_with('"'))
            || (trimmed.starts_with('\'') && trimmed.ends_with('\'')))
    {
        trimmed[1..trimmed.len() - 1].to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn extract_with_source<S: TextSource>(
    language: LanguageId,
    source: &S,
    tree: &Tree,
    cancelled: &AtomicBool,
) -> Result<Vec<Symbol>, String> {
    check_cancelled(cancelled)?;
    let mut extractor = Extractor {
        source,
        cancelled,
        count: 0,
    };
    let root = tree.root_node();
    let symbols = match language {
        LanguageId::Rust => extract_rust(&mut extractor, root, false),
        LanguageId::JavaScript | LanguageId::TypeScript | LanguageId::Tsx => {
            extract_js_ts(&mut extractor, root)
        }
        LanguageId::Markdown => extract_markdown(&mut extractor, root),
        LanguageId::Html => extract_html(&mut extractor, root),
        LanguageId::Css => extract_css(&mut extractor, root),
        LanguageId::Json => extract_json(&mut extractor, root, 0),
        LanguageId::Json5 => extract_json5(&mut extractor, root, 0),
        LanguageId::MarkdownInline => Vec::new(),
    };
    check_cancelled(cancelled)?;
    Ok(symbols)
}

fn extract_rust<S: TextSource>(
    ext: &mut Extractor<'_, S>,
    container: Node<'_>,
    in_type_or_trait: bool,
) -> Vec<Symbol> {
    let mut out = Vec::new();
    let mut cursor = container.walk();
    for node in container.named_children(&mut cursor) {
        if !ext.can_push() {
            break;
        }
        match node.kind() {
            "function_item" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let kind = if in_type_or_trait {
                        SymbolKind::Method
                    } else {
                        SymbolKind::Function
                    };
                    if let Some(sym) =
                        ext.make_symbol(ext.node_text(name_node), kind, node, name_node, Vec::new())
                    {
                        out.push(sym);
                    }
                }
            }
            "function_signature_item" => {
                if let Some(name_node) = node.child_by_field_name("name")
                    && let Some(sym) = ext.make_symbol(
                        ext.node_text(name_node),
                        SymbolKind::Method,
                        node,
                        name_node,
                        Vec::new(),
                    )
                {
                    out.push(sym);
                }
            }
            "struct_item" | "enum_item" | "union_item" | "type_item" | "const_item"
            | "static_item" | "macro_definition" => {
                let kind = match node.kind() {
                    "struct_item" => SymbolKind::Struct,
                    "enum_item" => SymbolKind::Enum,
                    "union_item" => SymbolKind::Union,
                    "type_item" => SymbolKind::Type,
                    "const_item" => SymbolKind::Const,
                    "static_item" => SymbolKind::Static,
                    _ => SymbolKind::Macro,
                };
                if let Some(name_node) = node.child_by_field_name("name")
                    && let Some(sym) =
                        ext.make_symbol(ext.node_text(name_node), kind, node, name_node, Vec::new())
                {
                    out.push(sym);
                }
            }
            "trait_item" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let children = node
                        .child_by_field_name("body")
                        .map(|body| extract_rust(ext, body, true))
                        .unwrap_or_default();
                    if let Some(sym) = ext.make_symbol(
                        ext.node_text(name_node),
                        SymbolKind::Trait,
                        node,
                        name_node,
                        children,
                    ) {
                        out.push(sym);
                    }
                }
            }
            "impl_item" => {
                if let Some(type_node) = node.child_by_field_name("type") {
                    let name = if let Some(trait_node) = node.child_by_field_name("trait") {
                        format!(
                            "{} for {}",
                            ext.node_text(trait_node),
                            ext.node_text(type_node)
                        )
                    } else {
                        ext.node_text(type_node)
                    };
                    let children = node
                        .child_by_field_name("body")
                        .map(|body| extract_rust(ext, body, true))
                        .unwrap_or_default();
                    if let Some(sym) =
                        ext.make_symbol(name, SymbolKind::Impl, node, type_node, children)
                    {
                        out.push(sym);
                    }
                }
            }
            "mod_item" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let children = node
                        .child_by_field_name("body")
                        .map(|body| extract_rust(ext, body, false))
                        .unwrap_or_default();
                    if let Some(sym) = ext.make_symbol(
                        ext.node_text(name_node),
                        SymbolKind::Module,
                        node,
                        name_node,
                        children,
                    ) {
                        out.push(sym);
                    }
                }
            }
            "foreign_mod_item" => {
                if let Some(body) = node.child_by_field_name("body") {
                    out.extend(extract_rust(ext, body, false));
                }
            }
            _ => {}
        }
    }
    out
}

fn is_js_function_like(node: Node<'_>) -> bool {
    matches!(
        node.kind(),
        "arrow_function" | "function_expression" | "function" | "generator_function"
    )
}

fn extract_js_class_body<S: TextSource>(ext: &mut Extractor<'_, S>, body: Node<'_>) -> Vec<Symbol> {
    let mut out = Vec::new();
    let mut cursor = body.walk();
    for node in body.named_children(&mut cursor) {
        if !ext.can_push() {
            break;
        }
        match node.kind() {
            "method_definition" | "method_signature" | "abstract_method_signature" => {
                if let Some(name_node) = node.child_by_field_name("name")
                    && let Some(sym) = ext.make_symbol(
                        ext.node_text(name_node),
                        SymbolKind::Method,
                        node,
                        name_node,
                        Vec::new(),
                    )
                {
                    out.push(sym);
                }
            }
            "public_field_definition" | "field_definition" => {
                let name_node = node
                    .child_by_field_name("name")
                    .or_else(|| node.child_by_field_name("property"));
                if let (Some(name_node), Some(value_node)) =
                    (name_node, node.child_by_field_name("value"))
                    && is_js_function_like(value_node)
                    && let Some(sym) = ext.make_symbol(
                        ext.node_text(name_node),
                        SymbolKind::Method,
                        node,
                        name_node,
                        Vec::new(),
                    )
                {
                    out.push(sym);
                }
            }
            _ => {}
        }
    }
    out
}

fn extract_js_ts<S: TextSource>(ext: &mut Extractor<'_, S>, container: Node<'_>) -> Vec<Symbol> {
    let mut out = Vec::new();
    let mut cursor = container.walk();
    for node in container.named_children(&mut cursor) {
        if !ext.can_push() {
            break;
        }
        match node.kind() {
            "export_statement" | "ambient_declaration" => {
                out.extend(extract_js_ts(ext, node));
            }
            "expression_statement" => {
                let mut expr_cursor = node.walk();
                for child in node.named_children(&mut expr_cursor) {
                    if matches!(child.kind(), "internal_module" | "module") {
                        out.extend(extract_js_ts(ext, node));
                        break;
                    }
                }
            }
            "function_declaration" | "generator_function_declaration" | "function_signature" => {
                if let Some(name_node) = node.child_by_field_name("name")
                    && let Some(sym) = ext.make_symbol(
                        ext.node_text(name_node),
                        SymbolKind::Function,
                        node,
                        name_node,
                        Vec::new(),
                    )
                {
                    out.push(sym);
                }
            }
            "class_declaration" | "abstract_class_declaration" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let children = node
                        .child_by_field_name("body")
                        .map(|body| extract_js_class_body(ext, body))
                        .unwrap_or_default();
                    if let Some(sym) = ext.make_symbol(
                        ext.node_text(name_node),
                        SymbolKind::Class,
                        node,
                        name_node,
                        children,
                    ) {
                        out.push(sym);
                    }
                }
            }
            "interface_declaration" | "type_alias_declaration" | "enum_declaration" => {
                let kind = match node.kind() {
                    "interface_declaration" => SymbolKind::Interface,
                    "type_alias_declaration" => SymbolKind::Type,
                    _ => SymbolKind::Enum,
                };
                if let Some(name_node) = node.child_by_field_name("name")
                    && let Some(sym) =
                        ext.make_symbol(ext.node_text(name_node), kind, node, name_node, Vec::new())
                {
                    out.push(sym);
                }
            }
            "internal_module" | "module" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let children = node
                        .child_by_field_name("body")
                        .map(|body| extract_js_ts(ext, body))
                        .unwrap_or_default();
                    if let Some(sym) = ext.make_symbol(
                        ext.node_text(name_node),
                        SymbolKind::Module,
                        node,
                        name_node,
                        children,
                    ) {
                        out.push(sym);
                    }
                }
            }
            "lexical_declaration" | "variable_declaration" => {
                let is_const = ext.node_text(node).trim_start().starts_with("const");
                let mut decl_cursor = node.walk();
                for child in node.named_children(&mut decl_cursor) {
                    if !ext.can_push() {
                        break;
                    }
                    if child.kind() != "variable_declarator" {
                        continue;
                    }
                    let Some(name_node) = child.child_by_field_name("name") else {
                        continue;
                    };
                    if name_node.kind() != "identifier" {
                        continue;
                    }
                    let Some(value_node) = child.child_by_field_name("value") else {
                        continue;
                    };
                    if is_js_function_like(value_node) {
                        if let Some(sym) = ext.make_symbol(
                            ext.node_text(name_node),
                            SymbolKind::Function,
                            child,
                            name_node,
                            Vec::new(),
                        ) {
                            out.push(sym);
                        }
                    } else if value_node.kind() == "class" {
                        let children = value_node
                            .child_by_field_name("body")
                            .map(|body| extract_js_class_body(ext, body))
                            .unwrap_or_default();
                        if let Some(sym) = ext.make_symbol(
                            ext.node_text(name_node),
                            SymbolKind::Class,
                            child,
                            name_node,
                            children,
                        ) {
                            out.push(sym);
                        }
                    } else if is_const
                        && let Some(sym) = ext.make_symbol(
                            ext.node_text(name_node),
                            SymbolKind::Const,
                            child,
                            name_node,
                            Vec::new(),
                        )
                    {
                        out.push(sym);
                    }
                }
            }
            _ => {}
        }
    }
    out
}

fn extract_markdown<S: TextSource>(ext: &mut Extractor<'_, S>, root: Node<'_>) -> Vec<Symbol> {
    fn collect<S: TextSource>(
        ext: &mut Extractor<'_, S>,
        node: Node<'_>,
        flat: &mut Vec<(u8, Symbol)>,
    ) {
        if !ext.can_push() {
            return;
        }
        match node.kind() {
            "fenced_code_block" | "indented_code_block" => return,
            "atx_heading" => {
                let mut level = 1u8;
                let mut content = None;
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    match child.kind() {
                        "atx_h1_marker" => level = 1,
                        "atx_h2_marker" => level = 2,
                        "atx_h3_marker" => level = 3,
                        "atx_h4_marker" => level = 4,
                        "atx_h5_marker" => level = 5,
                        "atx_h6_marker" => level = 6,
                        "inline" | "heading_content" => content = Some(child),
                        _ => {}
                    }
                }
                let raw = content.map(|n| ext.node_text(n)).unwrap_or_else(|| {
                    ext.node_text(node)
                        .trim_start_matches('#')
                        .trim_end_matches('#')
                        .to_owned()
                });
                let cleaned = raw.trim().trim_end_matches('#').trim().to_owned();
                let anchor = content.unwrap_or(node);
                if let Some(sym) = ext.make_symbol(
                    cleaned,
                    SymbolKind::Heading(level),
                    node,
                    anchor,
                    Vec::new(),
                ) {
                    flat.push((level, sym));
                }
                return;
            }
            "setext_heading" => {
                let mut level = 1u8;
                let mut content = None;
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    match child.kind() {
                        "setext_h1_underline" => level = 1,
                        "setext_h2_underline" => level = 2,
                        "paragraph" | "inline" | "heading_content" => content = Some(child),
                        _ => {}
                    }
                }
                if let Some(content_node) = content
                    && let Some(sym) = ext.make_symbol(
                        ext.node_text(content_node),
                        SymbolKind::Heading(level),
                        node,
                        content_node,
                        Vec::new(),
                    )
                {
                    flat.push((level, sym));
                }
                return;
            }
            _ => {}
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            collect(ext, child, flat);
        }
    }

    let mut flat = Vec::new();
    collect(ext, root, &mut flat);

    // Nest headings using a level stack so h2 sits under h1, h3 under h2, etc.
    let mut roots: Vec<Symbol> = Vec::new();
    let mut stack: Vec<(u8, Symbol)> = Vec::new();
    for (level, sym) in flat {
        while let Some((top_level, _)) = stack.last() {
            if *top_level < level {
                break;
            }
            let (_, popped) = stack.pop().unwrap();
            if let Some((_, parent)) = stack.last_mut() {
                parent.children.push(popped);
            } else {
                roots.push(popped);
            }
        }
        stack.push((level, sym));
    }
    while let Some((_, popped)) = stack.pop() {
        if let Some((_, parent)) = stack.last_mut() {
            parent.children.push(popped);
        } else {
            roots.push(popped);
        }
    }
    roots
}

fn html_tag_info<'a, S: TextSource>(
    ext: &Extractor<'_, S>,
    element: Node<'a>,
) -> Option<(String, Option<String>, Node<'a>)> {
    let mut cursor = element.walk();
    let start_tag = element
        .named_children(&mut cursor)
        .find(|child| matches!(child.kind(), "start_tag" | "self_closing_tag"))?;
    let mut tag_name = None;
    let mut id_attr = None;
    let mut tag_cursor = start_tag.walk();
    for child in start_tag.named_children(&mut tag_cursor) {
        if child.kind() == "tag_name" {
            tag_name = Some((ext.node_text(child), child));
        } else if child.kind() == "attribute" {
            let mut attr_name = None;
            let mut attr_value = None;
            let mut attr_cursor = child.walk();
            for part in child.named_children(&mut attr_cursor) {
                match part.kind() {
                    "attribute_name" => attr_name = Some(ext.node_text(part)),
                    "quoted_attribute_value" | "attribute_value" => {
                        attr_value = Some(strip_quotes(&ext.node_text(part)));
                    }
                    _ => {}
                }
            }
            if attr_name
                .as_deref()
                .is_some_and(|name| name.eq_ignore_ascii_case("id"))
                && let Some(val) = attr_value.filter(|v| !v.is_empty())
            {
                id_attr = Some(val);
            }
        }
    }
    let (name, name_node) = tag_name?;
    Some((name, id_attr, name_node))
}

fn html_inner_text<S: TextSource>(ext: &Extractor<'_, S>, element: Node<'_>) -> String {
    let mut text = String::new();
    let mut cursor = element.walk();
    for child in element.named_children(&mut cursor) {
        match child.kind() {
            "start_tag" | "end_tag" | "self_closing_tag" => {}
            "text" => {
                if !text.is_empty() {
                    text.push(' ');
                }
                text.push_str(&ext.node_text(child));
            }
            "element" => {
                let inner = html_inner_text(ext, child);
                if !inner.is_empty() {
                    if !text.is_empty() {
                        text.push(' ');
                    }
                    text.push_str(&inner);
                }
            }
            _ => {}
        }
    }
    text
}

fn extract_html<S: TextSource>(ext: &mut Extractor<'_, S>, container: Node<'_>) -> Vec<Symbol> {
    let mut out = Vec::new();
    let mut cursor = container.walk();
    for node in container.named_children(&mut cursor) {
        if !ext.can_push() {
            break;
        }
        if node.kind() != "element" {
            out.extend(extract_html(ext, node));
            continue;
        }
        let child_symbols = extract_html(ext, node);
        if let Some((tag, id_attr, anchor)) = html_tag_info(ext, node) {
            let lower = tag.to_ascii_lowercase();
            let heading_level = match lower.as_str() {
                "h1" => Some(1),
                "h2" => Some(2),
                "h3" => Some(3),
                "h4" => Some(4),
                "h5" => Some(5),
                "h6" => Some(6),
                _ => None,
            };
            let entry = if let Some(level) = heading_level {
                let text = normalize_name(&html_inner_text(ext, node));
                let name = if text.is_empty() {
                    if let Some(id) = id_attr {
                        format!("{lower}#{id}")
                    } else {
                        lower
                    }
                } else {
                    text
                };
                Some((name, SymbolKind::Heading(level)))
            } else if lower == "title" {
                let text = normalize_name(&html_inner_text(ext, node));
                let name = if text.is_empty() { lower } else { text };
                Some((name, SymbolKind::Tag))
            } else {
                id_attr.map(|id| (format!("{lower}#{id}"), SymbolKind::Tag))
            };
            if let Some((name, kind)) = entry {
                let mut children = child_symbols;
                if let Some(sym) =
                    ext.make_symbol(name, kind, node, anchor, std::mem::take(&mut children))
                {
                    out.push(sym);
                } else {
                    out.extend(children);
                }
                continue;
            }
        }
        out.extend(child_symbols);
    }
    out
}

fn extract_css<S: TextSource>(ext: &mut Extractor<'_, S>, container: Node<'_>) -> Vec<Symbol> {
    let mut out = Vec::new();
    let mut cursor = container.walk();
    for node in container.named_children(&mut cursor) {
        if !ext.can_push() {
            break;
        }
        match node.kind() {
            "rule_set" => {
                let mut child_cursor = node.walk();
                if let Some(selectors) = node
                    .named_children(&mut child_cursor)
                    .find(|child| child.kind() == "selectors")
                    && let Some(sym) = ext.make_symbol(
                        ext.node_text(selectors),
                        SymbolKind::Selector,
                        node,
                        selectors,
                        Vec::new(),
                    )
                {
                    out.push(sym);
                }
            }
            "media_statement"
            | "supports_statement"
            | "keyframes_statement"
            | "layer_statement" => {
                let mut child_cursor = node.walk();
                let block = node
                    .named_children(&mut child_cursor)
                    .find(|child| matches!(child.kind(), "block" | "keyframe_block_list"));
                let header_end = block.map_or(node.end_byte(), |b| b.start_byte());
                let header = ext
                    .source
                    .slice(node.start_byte()..header_end)
                    .trim()
                    .trim_end_matches('{')
                    .trim()
                    .to_owned();
                let children = block.map(|b| extract_css(ext, b)).unwrap_or_default();
                if let Some(sym) = ext.make_symbol(header, SymbolKind::Module, node, node, children)
                {
                    out.push(sym);
                }
            }
            "keyframe_block" => {
                let mut child_cursor = node.walk();
                let block = node
                    .named_children(&mut child_cursor)
                    .find(|child| child.kind() == "block");
                let header_end = block.map_or(node.end_byte(), |b| b.start_byte());
                let header = ext
                    .source
                    .slice(node.start_byte()..header_end)
                    .trim()
                    .trim_end_matches('{')
                    .trim()
                    .to_owned();
                if let Some(sym) =
                    ext.make_symbol(header, SymbolKind::Selector, node, node, Vec::new())
                {
                    out.push(sym);
                }
            }
            _ => {}
        }
    }
    out
}

fn extract_json<S: TextSource>(
    ext: &mut Extractor<'_, S>,
    node: Node<'_>,
    depth: usize,
) -> Vec<Symbol> {
    if depth >= MAX_JSON_DEPTH || !ext.can_push() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if !ext.can_push() {
            break;
        }
        match child.kind() {
            "object" => out.extend(extract_json(ext, child, depth)),
            "pair" => {
                if let Some(key_node) = child.child_by_field_name("key") {
                    let name = strip_quotes(&ext.node_text(key_node));
                    let children = child
                        .child_by_field_name("value")
                        .filter(|v| v.kind() == "object")
                        .map(|obj| extract_json(ext, obj, depth + 1))
                        .unwrap_or_default();
                    if let Some(sym) =
                        ext.make_symbol(name, SymbolKind::Key, child, key_node, children)
                    {
                        out.push(sym);
                    }
                }
            }
            _ => {}
        }
    }
    out
}

fn extract_json5<S: TextSource>(
    ext: &mut Extractor<'_, S>,
    node: Node<'_>,
    depth: usize,
) -> Vec<Symbol> {
    if depth >= MAX_JSON_DEPTH || !ext.can_push() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if !ext.can_push() {
            break;
        }
        match child.kind() {
            "object" => out.extend(extract_json5(ext, child, depth)),
            "member" => {
                if let Some(name_node) = child.child_by_field_name("name") {
                    let name = strip_quotes(&ext.node_text(name_node));
                    let children = child
                        .child_by_field_name("value")
                        .filter(|v| v.kind() == "object")
                        .map(|obj| extract_json5(ext, obj, depth + 1))
                        .unwrap_or_default();
                    if let Some(sym) =
                        ext.make_symbol(name, SymbolKind::Key, child, name_node, children)
                    {
                        out.push(sym);
                    }
                }
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::AtomicU64;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
                ".pi/tmp/outline-{tag}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&root).unwrap();
            Self(root)
        }

        fn write(&self, rel: &str, content: &str) -> PathBuf {
            let path = self.0.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, content).unwrap();
            path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn extracts_rust_symbols_and_nested_methods() {
        let code = r#"
const MAX: usize = 10;
static COUNT: u32 = 0;
type Alias = Vec<u8>;
macro_rules! my_macro { () => {}; }
struct Foo { x: i32 }
enum Bar { A, B }
union U { a: u8, b: i8 }
trait Greet {
    fn hello(&self);
}
impl Greet for Foo {
    fn hello(&self) {}
}
impl Foo {
    fn helper(&self) {}
}
mod inner {
    pub fn nested_fn() {}
}
fn top_level() {}
"#;
        let syms = extract_from_str(LanguageId::Rust, code, &AtomicBool::new(false)).unwrap();
        let flat = flatten(&syms);
        let summary: Vec<_> = flat
            .iter()
            .map(|s| (s.qualified_name.as_str(), s.kind))
            .collect();
        assert_eq!(
            summary,
            vec![
                ("MAX", SymbolKind::Const),
                ("COUNT", SymbolKind::Static),
                ("Alias", SymbolKind::Type),
                ("my_macro", SymbolKind::Macro),
                ("Foo", SymbolKind::Struct),
                ("Bar", SymbolKind::Enum),
                ("U", SymbolKind::Union),
                ("Greet", SymbolKind::Trait),
                ("Greet / hello", SymbolKind::Method),
                ("Greet for Foo", SymbolKind::Impl),
                ("Greet for Foo / hello", SymbolKind::Method),
                ("Foo", SymbolKind::Impl),
                ("Foo / helper", SymbolKind::Method),
                ("inner", SymbolKind::Module),
                ("inner / nested_fn", SymbolKind::Function),
                ("top_level", SymbolKind::Function),
            ]
        );
    }

    #[test]
    fn extracts_js_ts_tsx_symbols() {
        let js = r#"
export const ANSWER = 42;
export const arrowFn = () => 1;
export function namedFn() {}
class Widget {
  render() {}
  onClick = () => {};
}
"#;
        let js_syms = flatten(
            &extract_from_str(LanguageId::JavaScript, js, &AtomicBool::new(false)).unwrap(),
        );
        assert_eq!(
            js_syms
                .iter()
                .map(|s| (s.qualified_name.as_str(), s.kind))
                .collect::<Vec<_>>(),
            vec![
                ("ANSWER", SymbolKind::Const),
                ("arrowFn", SymbolKind::Function),
                ("namedFn", SymbolKind::Function),
                ("Widget", SymbolKind::Class),
                ("Widget / render", SymbolKind::Method),
                ("Widget / onClick", SymbolKind::Method),
            ]
        );

        let ts = r#"
interface Props { id: string; }
type ID = string;
enum Mode { Read, Write }
namespace Core {
  export function init() {}
}
abstract class Base {
  abstract run(): void;
}
"#;
        let ts_syms = flatten(
            &extract_from_str(LanguageId::TypeScript, ts, &AtomicBool::new(false)).unwrap(),
        );
        assert_eq!(
            ts_syms
                .iter()
                .map(|s| (s.qualified_name.as_str(), s.kind))
                .collect::<Vec<_>>(),
            vec![
                ("Props", SymbolKind::Interface),
                ("ID", SymbolKind::Type),
                ("Mode", SymbolKind::Enum),
                ("Core", SymbolKind::Module),
                ("Core / init", SymbolKind::Function),
                ("Base", SymbolKind::Class),
                ("Base / run", SymbolKind::Method),
            ]
        );

        let tsx = r#"export const App = () => <div/>;"#;
        let tsx_syms =
            flatten(&extract_from_str(LanguageId::Tsx, tsx, &AtomicBool::new(false)).unwrap());
        assert_eq!(tsx_syms.len(), 1);
        assert_eq!(tsx_syms[0].qualified_name, "App");
        assert_eq!(tsx_syms[0].kind, SymbolKind::Function);
    }

    #[test]
    fn extracts_markdown_nested_headings_and_skips_code_blocks() {
        let md = r#"# Title
```markdown
# Not a heading
```
## Section A
### Sub A1
## Section B
Setext Heading
==============
"#;
        let syms = extract_from_str(LanguageId::Markdown, md, &AtomicBool::new(false)).unwrap();
        let flat = flatten(&syms);
        assert_eq!(
            flat.iter()
                .map(|s| (s.qualified_name.as_str(), s.kind))
                .collect::<Vec<_>>(),
            vec![
                ("Title", SymbolKind::Heading(1)),
                ("Title / Section A", SymbolKind::Heading(2)),
                ("Title / Section A / Sub A1", SymbolKind::Heading(3)),
                ("Title / Section B", SymbolKind::Heading(2)),
                ("Setext Heading", SymbolKind::Heading(1)),
            ]
        );
    }

    #[test]
    fn extracts_html_css_json_json5_symbols() {
        let html = r#"<html><head><title>Doc</title></head><body><main id="app"><h1>Hello</h1></main></body></html>"#;
        let html_syms =
            flatten(&extract_from_str(LanguageId::Html, html, &AtomicBool::new(false)).unwrap());
        assert_eq!(
            html_syms
                .iter()
                .map(|s| (s.qualified_name.as_str(), s.kind))
                .collect::<Vec<_>>(),
            vec![
                ("Doc", SymbolKind::Tag),
                ("main#app", SymbolKind::Tag),
                ("main#app / Hello", SymbolKind::Heading(1)),
            ]
        );

        let css = r#"
.btn { color: red; }
@media (min-width: 600px) {
  .btn:hover { color: blue; }
}
"#;
        let css_syms =
            flatten(&extract_from_str(LanguageId::Css, css, &AtomicBool::new(false)).unwrap());
        assert_eq!(
            css_syms
                .iter()
                .map(|s| (s.qualified_name.as_str(), s.kind))
                .collect::<Vec<_>>(),
            vec![
                (".btn", SymbolKind::Selector),
                ("@media (min-width: 600px)", SymbolKind::Module),
                (
                    "@media (min-width: 600px) / .btn:hover",
                    SymbolKind::Selector
                ),
            ]
        );

        let json = r#"{"name": "semacs", "scripts": {"build": "cargo build"}}"#;
        let json_syms =
            flatten(&extract_from_str(LanguageId::Json, json, &AtomicBool::new(false)).unwrap());
        assert_eq!(
            json_syms
                .iter()
                .map(|s| s.qualified_name.as_str())
                .collect::<Vec<_>>(),
            vec!["name", "scripts", "scripts / build"]
        );

        let json5 = r#"{ // comment
  server: { port: 8080 },
  "quoted": true
}"#;
        let json5_syms =
            flatten(&extract_from_str(LanguageId::Json5, json5, &AtomicBool::new(false)).unwrap());
        assert_eq!(
            json5_syms
                .iter()
                .map(|s| s.qualified_name.as_str())
                .collect::<Vec<_>>(),
            vec!["server", "server / port", "quoted"]
        );
    }

    #[test]
    fn cancelled_extraction_returns_error() {
        let cancelled = AtomicBool::new(true);
        assert!(extract_from_str(LanguageId::Rust, "fn main() {}", &cancelled).is_err());
    }

    #[test]
    fn project_scan_uses_cache_and_open_overlays_and_respects_gitignore() {
        let dir = TempDir::new("project-scan");
        let status = std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&dir.0)
            .status()
            .unwrap();
        assert!(status.success());
        dir.write(".gitignore", "ignored/\n");
        let a_rs = dir.write("src/a.rs", "fn alpha() {}\n");
        let b_rs = dir.write("src/b.rs", "fn beta() {}\n");
        dir.write("ignored/c.rs", "fn ignored_fn() {}\n");
        dir.write("notes.txt", "plain text\n");

        let project = Project::git(dir.0.clone());
        let cancelled = AtomicBool::new(false);
        let cache = HashMap::new();
        let open = HashMap::new();
        let first = scan_project(&project, &open, &cache, &cancelled).unwrap();
        let names: Vec<_> = first
            .symbols
            .iter()
            .map(|s| (s.relative_path.as_str(), s.qualified_name.as_str()))
            .collect();
        assert_eq!(names, vec![("src/a.rs", "alpha"), ("src/b.rs", "beta")]);
        assert!(first.cache.contains_key(&a_rs));
        assert!(first.cache.contains_key(&b_rs));

        // Open overlay overrides disk content for src/a.rs, while src/b.rs hits cache.
        let mut open = HashMap::new();
        let buffer = crate::buffer::Buffer::visiting(
            a_rs.clone(),
            files::DecodedFile {
                text: "fn unsaved_alpha() {}\n".into(),
                format: Default::default(),
            },
            files::stamp(&a_rs).unwrap(),
        );
        open.insert(
            a_rs.clone(),
            OpenFileOverlay {
                snapshot: buffer.snapshot(),
                language: LanguageId::Rust,
                tree: None,
            },
        );
        let second = scan_project(&project, &open, &first.cache, &cancelled).unwrap();
        let names: Vec<_> = second
            .symbols
            .iter()
            .map(|s| (s.relative_path.as_str(), s.qualified_name.as_str()))
            .collect();
        assert_eq!(
            names,
            vec![("src/a.rs", "unsaved_alpha"), ("src/b.rs", "beta")]
        );
    }
}
