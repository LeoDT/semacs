use std::collections::{BinaryHeap, HashMap};
use std::ops::{ControlFlow, Range};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use tree_sitter::{
    Node, ParseOptions, Parser, QueryCursor, QueryCursorOptions, StreamingIterator, Tree,
};

use super::languages::{LanguageId, for_alias};
use super::{Change, Highlights, Span};
use crate::buffer::BufferSnapshot;
use crate::text::{Bias, Rope};
use crate::theme::StyleRole;

const MAX_LAYERS: usize = 16_384;
const MAX_DEPTH: usize = 4;

pub(super) struct Layer {
    pub language: LanguageId,
    pub tree: Tree,
}

pub(super) struct Parsed {
    pub snapshot: BufferSnapshot,
    pub language: LanguageId,
    pub layers: Vec<Layer>,
    pub highlights: Arc<Highlights>,
}

fn check(cancelled: &AtomicBool) -> Result<(), String> {
    if cancelled.load(Ordering::Relaxed) {
        Err("syntax cancelled".into())
    } else {
        Ok(())
    }
}

type TreeKey = (LanguageId, Vec<(usize, usize)>);

fn key(language: LanguageId, ranges: &[tree_sitter::Range]) -> TreeKey {
    (
        language,
        ranges
            .iter()
            .map(|range| (range.start_byte, range.end_byte))
            .collect(),
    )
}

struct Request {
    language: LanguageId,
    ranges: Vec<tree_sitter::Range>,
    depth: usize,
    fences: bool,
}

pub(super) fn parse(
    language: LanguageId,
    snapshot: BufferSnapshot,
    base: Option<Parsed>,
    change: Option<Change>,
    cancelled: &AtomicBool,
) -> Result<Parsed, String> {
    check(cancelled)?;
    let text = snapshot.text();
    if text.len() > u32::MAX as usize {
        return Err("text exceeds tree-sitter byte range".into());
    }
    let mut old = HashMap::new();
    let mut old_root = None;
    if let Some(base) = base.filter(|base| base.language == language) {
        let edit = change.map(|change| change.input_edit(base.snapshot.text(), text));
        for (index, mut layer) in base.layers.into_iter().enumerate() {
            check(cancelled)?;
            if let Some(edit) = &edit {
                layer.tree.edit(edit);
            }
            if index == 0 {
                old_root = Some(layer.tree);
            } else {
                old.insert(
                    key(layer.language, &layer.tree.included_ranges()),
                    layer.tree,
                );
            }
        }
    }
    let mut pending = vec![Request {
        language,
        ranges: vec![source_range(text, 0..text.len())],
        depth: 0,
        fences: true,
    }];
    let mut layers = Vec::new();
    let mut host_spans = Vec::new();
    let mut colored = Vec::new();
    let mut parser = Parser::new();
    while let Some(request) = pending.pop() {
        check(cancelled)?;
        let definition = request.language.definition();
        parser
            .set_language(&definition.grammar())
            .map_err(|error| error.to_string())?;
        parser
            .set_included_ranges(if request.depth == 0 {
                &[]
            } else {
                &request.ranges
            })
            .map_err(|error| error.to_string())?;
        let previous = if request.depth == 0 {
            old_root.take()
        } else {
            old.remove(&key(request.language, &request.ranges))
        };
        let mut progress = |_: &tree_sitter::ParseState| {
            if cancelled.load(Ordering::Relaxed) {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        };
        let tree = parser
            .parse_with_options(
                &mut |byte, _| {
                    let start = text.clip_offset(byte, Bias::Left);
                    let chunk = text.chunks_in_range(start..text.len()).next().unwrap_or("");
                    &chunk.as_bytes()[byte - start..]
                },
                previous.as_ref(),
                Some(ParseOptions::new().progress_callback(&mut progress)),
            )
            .ok_or("parse cancelled")?;
        let spans = query(&tree, request.language, text, cancelled)?;
        if request.depth == 0 {
            host_spans = spans;
        } else {
            for span in spans {
                for range in intersect(&request.ranges, span.range) {
                    colored.push((
                        request.depth,
                        request.language == LanguageId::MarkdownInline,
                        Span {
                            range: range.start_byte..range.end_byte,
                            style: span.style,
                            modifiers: span.modifiers.clone(),
                        },
                    ));
                }
            }
        }
        if request.depth < MAX_DEPTH {
            let remaining = MAX_LAYERS.saturating_sub(layers.len() + pending.len() + 1);
            let children = injections(&tree, &request, text, cancelled, remaining)?;
            pending.extend(children.into_iter().rev());
        }
        layers.push(Layer {
            language: request.language,
            tree,
        });
    }
    check(cancelled)?;
    let spans = if colored.is_empty() {
        host_spans
    } else {
        colored.extend(host_spans.into_iter().map(|span| (0, false, span)));
        combine(colored, text.len(), cancelled)?
    };
    Ok(Parsed {
        language,
        highlights: Arc::new(Highlights {
            snapshot: snapshot.id(),
            role_mask: spans.iter().fold(0, |mask, span| {
                span.modifiers
                    .iter()
                    .fold(mask | (1 << span.style as usize), |mask, role| {
                        mask | (1 << *role as usize)
                    })
            }),
            spans,
        }),
        snapshot,
        layers,
    })
}

fn source_range(text: &Rope, range: Range<usize>) -> tree_sitter::Range {
    let point = |offset| {
        let p = text.offset_to_point(offset);
        tree_sitter::Point::new(p.row as usize, p.column as usize)
    };
    tree_sitter::Range {
        start_byte: range.start,
        end_byte: range.end,
        start_point: point(range.start),
        end_point: point(range.end),
    }
}

fn intersect(ranges: &[tree_sitter::Range], clip: Range<usize>) -> Vec<tree_sitter::Range> {
    let start = ranges.partition_point(|range| range.end_byte <= clip.start);
    ranges[start..]
        .iter()
        .take_while(|range| range.start_byte < clip.end)
        .filter_map(|range| {
            let mut result = *range;
            if clip.start > result.start_byte {
                result.start_byte = clip.start;
            }
            if clip.end < result.end_byte {
                result.end_byte = clip.end;
            }
            (result.start_byte < result.end_byte).then_some(result)
        })
        .collect()
}

fn content_ranges(
    node: Node<'_>,
    parent: &[tree_sitter::Range],
    text: &Rope,
) -> Vec<tree_sitter::Range> {
    let mut cursor = node.walk();
    let mut start = node.start_byte();
    let mut pieces = Vec::new();
    for child in node.named_children(&mut cursor) {
        if child.kind() == "block_continuation" {
            pieces.extend(intersect(parent, start..child.start_byte()));
            start = child.end_byte();
        }
    }
    pieces.extend(intersect(parent, start..node.end_byte()));
    pieces
        .into_iter()
        .map(|range| source_range(text, range.start_byte..range.end_byte))
        .collect()
}

fn injections(
    tree: &Tree,
    parent: &Request,
    text: &Rope,
    cancelled: &AtomicBool,
    limit: usize,
) -> Result<Vec<Request>, String> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let Some(query) = parent.language.definition().injections()? else {
        return Ok(Vec::new());
    };
    let mut cursor = QueryCursor::new();
    let mut progress = |_: &tree_sitter::QueryCursorState| {
        if cancelled.load(Ordering::Relaxed) {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    };
    let mut matches = cursor.matches_with_options(
        query,
        tree.root_node(),
        |node: Node| text.chunks_in_range(node.byte_range()).map(str::as_bytes),
        QueryCursorOptions::new().progress_callback(&mut progress),
    );
    let mut children = Vec::new();
    while let Some(matched) = matches.next() {
        check(cancelled)?;
        let mut content = None;
        let mut language = None;
        let mut fence = false;
        for capture in matched.captures() {
            match query.capture_names()[capture.index as usize] {
                "injection.content" => content = Some(capture.node),
                "injection.language" => {
                    fence = true;
                    if capture.node.byte_range().len() <= 128 {
                        language = for_alias(&text.slice(capture.node.byte_range()).to_string());
                    }
                }
                _ => {}
            }
        }
        if fence && !parent.fences {
            continue;
        }
        if !fence {
            language = query
                .property_settings(matched.pattern_index)
                .iter()
                .find(|property| property.key.as_ref() == "injection.language")
                .and_then(|property| property.value.as_deref())
                .and_then(for_alias);
        }
        if let (Some(node), Some(language)) = (content, language) {
            let ranges = content_ranges(node, &parent.ranges, text);
            if !ranges.is_empty() {
                children.push(Request {
                    language,
                    ranges,
                    depth: parent.depth + 1,
                    fences: parent.fences && !fence,
                });
                if children.len() == limit {
                    break;
                }
            }
        }
    }
    drop(matches);
    check(cancelled)?;
    if cursor.did_exceed_match_limit() {
        return Err("injection query match limit exceeded".into());
    }
    Ok(children)
}

pub(super) fn capture_style(name: &str) -> Option<StyleRole> {
    Some(match name {
        "none" | "embedded" => StyleRole::Default,
        "text.title" => StyleRole::MarkupHeading,
        "text.strong" => StyleRole::MarkupStrong,
        "text.emphasis" => StyleRole::MarkupEmphasis,
        "text.reference" => StyleRole::SyntaxVariable,
        "text.uri" | "text.literal" => StyleRole::SyntaxString,
        _ => match name.split('.').next()? {
            "keyword" => StyleRole::SyntaxKeyword,
            "comment" => StyleRole::SyntaxComment,
            "string" | "escape" => StyleRole::SyntaxString,
            "number" => StyleRole::SyntaxNumber,
            "type" | "constructor" | "tag" => StyleRole::SyntaxType,
            "function" => StyleRole::SyntaxFunction,
            "variable" | "property" | "label" | "attribute" => StyleRole::SyntaxVariable,
            "constant" => StyleRole::SyntaxConstant,
            "operator" => StyleRole::SyntaxOperator,
            "punctuation" => StyleRole::SyntaxPunctuation,
            _ => return None,
        },
    })
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct SemanticStyle {
    inherit: bool,
    role: StyleRole,
    modifiers: smallvec::SmallVec<[StyleRole; 3]>,
}

type Active = BinaryHeap<(usize, usize, usize, usize, SemanticStyle)>;

fn emit_until(
    spans: &mut Vec<Span>,
    active: &mut Active,
    position: &mut usize,
    end: usize,
    inherit: bool,
) {
    while *position < end {
        while active.peek().is_some_and(|entry| entry.3 <= *position) {
            active.pop();
        }
        let Some(selected) = active.peek() else {
            *position = end;
            break;
        };
        let style = selected.4.role;
        let mut stop = selected.3.min(end);
        let mut modifiers = selected.4.modifiers.clone();
        if inherit || selected.4.inherit || style >= StyleRole::MarkupHeading {
            let mut enclosing: Vec<_> = active
                .iter()
                .filter(|entry| {
                    entry.3 > *position
                        && (entry.4.role >= StyleRole::MarkupHeading
                            || !entry.4.modifiers.is_empty())
                })
                .collect();
            enclosing.sort_unstable();
            modifiers.clear();
            for entry in enclosing {
                stop = stop.min(entry.3);
                for role in entry
                    .4
                    .modifiers
                    .iter()
                    .copied()
                    .chain((entry.4.role >= StyleRole::MarkupHeading).then_some(entry.4.role))
                {
                    if let Some(index) = modifiers.iter().position(|previous| *previous == role) {
                        modifiers.remove(index);
                    }
                    modifiers.push(role);
                }
            }
        }
        if let Some(last) = spans.last_mut()
            && last.range.end == *position
            && last.style == style
            && last.modifiers == modifiers
        {
            last.range.end = stop;
        } else {
            spans.push(Span {
                range: *position..stop,
                style,
                modifiers,
            });
        }
        *position = stop;
    }
}

fn query(
    tree: &Tree,
    language: LanguageId,
    text: &Rope,
    cancelled: &AtomicBool,
) -> Result<Vec<Span>, String> {
    let query = language.definition().query()?;
    let styles: Vec<_> = query
        .capture_names()
        .iter()
        .map(|name| capture_style(name))
        .collect();
    let inherit_markup = styles
        .iter()
        .flatten()
        .any(|style| *style >= StyleRole::MarkupHeading);
    let mut cursor = QueryCursor::new();
    let mut progress = |_: &tree_sitter::QueryCursorState| {
        if cancelled.load(Ordering::Relaxed) {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    };
    let mut captures = cursor.captures_with_options(
        query,
        tree.root_node(),
        |node: Node| text.chunks_in_range(node.byte_range()).map(str::as_bytes),
        QueryCursorOptions::new().progress_callback(&mut progress),
    );
    let mut active = BinaryHeap::new();
    let mut spans = Vec::new();
    let mut position = 0;
    let mut sequence = 0;
    while let Some((matched, index)) = captures.next() {
        check(cancelled)?;
        let capture = matched.captures()[*index];
        let Some(style) = styles[capture.index as usize] else {
            continue;
        };
        let range = capture.node.byte_range();
        emit_until(
            &mut spans,
            &mut active,
            &mut position,
            range.start,
            inherit_markup,
        );
        // Inner captures beat enclosing strings; later patterns resolve equal ranges.
        active.push((
            usize::MAX - range.len(),
            matched.pattern_index,
            sequence,
            range.end,
            SemanticStyle {
                inherit: false,
                role: style,
                modifiers: smallvec::SmallVec::new(),
            },
        ));
        sequence += 1;
    }
    drop(captures);
    check(cancelled)?;
    if cursor.did_exceed_match_limit() {
        return Err("query match limit exceeded".into());
    }
    emit_until(
        &mut spans,
        &mut active,
        &mut position,
        text.len(),
        inherit_markup,
    );
    Ok(spans)
}

fn combine(
    mut colored: Vec<(usize, bool, Span)>,
    end: usize,
    cancelled: &AtomicBool,
) -> Result<Vec<Span>, String> {
    colored.sort_unstable_by_key(|(_, _, span)| span.range.start);
    check(cancelled)?;
    let mut spans = Vec::new();
    let mut active = BinaryHeap::new();
    let mut position = 0;
    for (sequence, (depth, inherit, span)) in colored.into_iter().enumerate() {
        check(cancelled)?;
        emit_until(
            &mut spans,
            &mut active,
            &mut position,
            span.range.start,
            false,
        );
        active.push((
            depth,
            0,
            sequence,
            span.range.end,
            SemanticStyle {
                inherit,
                role: span.style,
                modifiers: span.modifiers,
            },
        ));
    }
    emit_until(&mut spans, &mut active, &mut position, end, false);
    Ok(spans)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::Buffer;
    use crate::files::{DecodedFile, FileFormat};

    fn buffer(text: &str) -> Buffer {
        Buffer::new(
            Some("limits.md".into()),
            DecodedFile {
                text: text.into(),
                format: FileFormat::default(),
            },
        )
    }

    #[test]
    fn layer_limit_retains_host_and_keeps_every_byte_editable() {
        let text = "*hello*\n\n".repeat(MAX_LAYERS + 2);
        let buffer = buffer(&text);
        let parsed = parse(
            LanguageId::Markdown,
            buffer.snapshot(),
            None,
            None,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(parsed.layers.len(), MAX_LAYERS);
        assert_eq!(buffer.text().to_string(), text);
        assert_eq!(
            parsed.highlights.in_range(1..2).next().unwrap().style,
            StyleRole::MarkupEmphasis
        );
        assert!(
            parsed
                .highlights
                .in_range(text.len() - 8..text.len())
                .next()
                .is_none()
        );
    }

    #[test]
    fn query_injection_and_merge_stages_check_cancellation() {
        let buffer = buffer("**bold**\n\n```js\nconst n = 42;\n```\n");
        let parsed = parse(
            LanguageId::Markdown,
            buffer.snapshot(),
            None,
            None,
            &AtomicBool::new(false),
        )
        .unwrap();
        let cancelled = AtomicBool::new(true);
        assert!(
            query(
                &parsed.layers[0].tree,
                LanguageId::Markdown,
                &buffer.text(),
                &cancelled
            )
            .is_err()
        );
        let request = Request {
            language: LanguageId::Markdown,
            ranges: vec![source_range(&buffer.text(), 0..buffer.text().len())],
            depth: 0,
            fences: true,
        };
        assert!(
            injections(
                &parsed.layers[0].tree,
                &request,
                &buffer.text(),
                &cancelled,
                MAX_LAYERS
            )
            .is_err()
        );
        assert!(combine(Vec::new(), buffer.text().len(), &cancelled).is_err());
    }

    #[test]
    fn markdown_fence_does_not_recursively_expand_more_fences() {
        let text = "~~~~markdown\n# Title\n\n```js\nconst plain = 42;\n```\n~~~~\n";
        let buffer = buffer(text);
        let parsed = parse(
            LanguageId::Markdown,
            buffer.snapshot(),
            None,
            None,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(
            !parsed
                .layers
                .iter()
                .any(|layer| layer.language == LanguageId::JavaScript)
        );
        assert_eq!(
            parsed
                .highlights
                .in_range(text.find("const").unwrap()..text.len())
                .next()
                .unwrap()
                .style,
            StyleRole::Default
        );
        assert!(
            parsed
                .highlights
                .in_range(text.find("Title").unwrap()..text.len())
                .any(|span| span.style == StyleRole::MarkupHeading)
        );
    }
}
