//! Shared, version-bound syntax work. Text and geometry remain owned by their existing models.

use std::cell::RefCell;
use std::collections::BinaryHeap;
use std::ops::{ControlFlow, Range};
use std::path::Path;
use std::rc::{Rc, Weak};
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, Ordering},
};

use gpui_kit::{AnyWeakEntity, App, BackgroundExecutor, Task};
use tree_sitter::{
    InputEdit, ParseOptions, Parser, Point, Query, QueryCursor, QueryCursorOptions,
    StreamingIterator, Tree,
};

use crate::buffer::{BufferSnapshot, SnapshotId};
use crate::text::{Bias, Rope};
use crate::theme::StyleRole;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Span {
    pub range: Range<usize>,
    pub style: StyleRole,
}

pub(crate) struct Highlights {
    pub snapshot: SnapshotId,
    spans: Vec<Span>,
}

impl Highlights {
    pub fn in_range(&self, range: Range<usize>) -> impl Iterator<Item = Span> + '_ {
        let start = self
            .spans
            .partition_point(|span| span.range.end <= range.start);
        self.spans[start..]
            .iter()
            .take_while(move |span| range.start < range.end && span.range.start < range.end)
            .map(move |span| Span {
                range: span.range.start.max(range.start)..span.range.end.min(range.end),
                style: span.style,
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

#[derive(Clone, Debug)]
struct Change {
    old: Range<usize>,
    new_end: usize,
}

impl Change {
    fn compose(&mut self, next: Self) {
        self.old.start = self.old.start.min(next.old.start);
        self.old.end += next.old.end.saturating_sub(self.new_end);
        self.new_end = next.new_end + self.new_end.saturating_sub(next.old.end);
    }

    fn input_edit(&self, before: &Rope, after: &Rope) -> InputEdit {
        let point = |text: &Rope, offset| {
            let p = text.offset_to_point(offset);
            Point::new(p.row as usize, p.column as usize)
        };
        InputEdit {
            start_byte: self.old.start,
            old_end_byte: self.old.end,
            new_end_byte: self.new_end,
            start_position: point(before, self.old.start),
            old_end_position: point(before, self.old.end),
            new_end_position: point(after, self.new_end),
        }
    }
}

struct Parsed {
    snapshot: BufferSnapshot,
    tree: Tree,
    highlights: Arc<Highlights>,
}

struct Work {
    cancelled: Arc<AtomicBool>,
    _task: Task<()>,
}

impl Drop for Work {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

// Cancellation may drop queued inputs or completed task outputs on the caller's thread.
struct BackgroundOwned<T: Send + 'static> {
    value: Option<T>,
    executor: BackgroundExecutor,
}

impl<T: Send + 'static> BackgroundOwned<T> {
    fn new(value: T, executor: BackgroundExecutor) -> Self {
        Self {
            value: Some(value),
            executor,
        }
    }

    fn into_inner(mut self) -> T {
        self.value.take().expect("owned background value")
    }
}

impl<T: Send + 'static> std::ops::Deref for BackgroundOwned<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.value.as_ref().expect("owned background value")
    }
}

impl<T: Send + 'static> Drop for BackgroundOwned<T> {
    fn drop(&mut self) {
        if let Some(value) = self.value.take() {
            self.executor
                .spawn(async move {
                    drop(value);
                })
                .detach();
        }
    }
}

struct State {
    enabled: bool,
    generation: u64,
    latest: BufferSnapshot,
    base: Option<BackgroundOwned<Parsed>>,
    pending: Option<Change>,
    work: Option<Work>,
    failed: Option<SnapshotId>,
    observers: Vec<AnyWeakEntity>,
}

pub(crate) struct Syntax(Rc<RefCell<State>>);

impl Syntax {
    pub fn new(snapshot: BufferSnapshot, path: Option<&Path>) -> Self {
        Self(Rc::new(RefCell::new(State {
            enabled: is_rust(path),
            generation: 0,
            latest: snapshot,
            base: None,
            pending: None,
            work: None,
            failed: None,
            observers: Vec::new(),
        })))
    }

    pub fn set_path(&self, path: &Path) {
        let mut state = self.0.borrow_mut();
        let enabled = is_rust(Some(path));
        if enabled == state.enabled {
            return;
        }
        state.enabled = enabled;
        state.generation = state
            .generation
            .checked_add(1)
            .expect("syntax generation exhausted");
        state.work = None;
        state.base = None;
        state.pending = None;
        state.failed = None;
    }

    pub fn edited(&self, snapshot: BufferSnapshot, old: Range<usize>, new_end: usize) {
        let mut state = self.0.borrow_mut();
        state.latest = snapshot;
        if state.enabled {
            let change = Change { old, new_end };
            if let Some(pending) = &mut state.pending {
                pending.compose(change);
            } else {
                state.pending = Some(change);
            }
        }
    }

    pub fn highlights(&self) -> Option<Arc<Highlights>> {
        let state = self.0.borrow();
        state
            .base
            .as_ref()
            .filter(|base| state.enabled && base.snapshot.id() == state.latest.id())
            .map(|base| base.highlights.clone())
    }

    pub fn ready(&self) -> bool {
        !self.0.borrow().enabled || self.highlights().is_some()
    }

    pub fn prepare(&self, observer: AnyWeakEntity, cx: &mut App) {
        {
            let mut state = self.0.borrow_mut();
            if !state.enabled {
                return;
            }
            state.observers.retain(AnyWeakEntity::is_upgradable);
            if !state
                .observers
                .iter()
                .any(|old| old.entity_id() == observer.entity_id())
            {
                state.observers.push(observer);
            }
        }
        start(&self.0, cx);
    }
}

fn is_rust(path: Option<&Path>) -> bool {
    path.and_then(Path::extension)
        .is_some_and(|extension| extension == "rs")
}

fn start(shared: &Rc<RefCell<State>>, cx: &mut App) {
    let mut state = shared.borrow_mut();
    if !state.enabled
        || state.work.is_some()
        || state.failed == Some(state.latest.id())
        || state
            .base
            .as_ref()
            .is_some_and(|base| base.snapshot.id() == state.latest.id())
    {
        return;
    }
    let snapshot = state.latest.clone();
    let id = snapshot.id();
    let generation = state.generation;
    let base = state.base.take();
    let change = state.pending.take();
    let cancelled = Arc::new(AtomicBool::new(false));
    let worker_cancelled = cancelled.clone();
    let executor = cx.background_executor().clone();
    let worker = executor.clone().spawn(async move {
        parse(
            snapshot,
            base.map(BackgroundOwned::into_inner),
            change,
            &worker_cancelled,
        )
        .map(|parsed| BackgroundOwned::new(parsed, executor))
    });
    let weak: Weak<RefCell<State>> = Rc::downgrade(shared);
    let task = cx.spawn(async move |cx| {
        let result = worker.await;
        cx.update(|cx| {
            let Some(shared) = weak.upgrade() else {
                return;
            };
            {
                let mut state = shared.borrow_mut();
                if state.generation != generation {
                    return;
                }
                state.work = None;
                match result {
                    Ok(parsed) => state.base = Some(parsed),
                    Err(error) => {
                        log::warn!("Rust syntax unavailable: {error}");
                        state.failed = Some(id);
                    }
                }
                if state.latest.id() == id {
                    state.observers.retain(|observer| {
                        if let Some(entity) = observer.upgrade() {
                            cx.notify(entity.entity_id());
                            true
                        } else {
                            false
                        }
                    });
                }
            }
            start(&shared, cx);
        });
    });
    state.work = Some(Work {
        cancelled,
        _task: task,
    });
}

fn rust_query() -> &'static Query {
    static QUERY: OnceLock<Query> = OnceLock::new();
    QUERY.get_or_init(|| {
        let source = format!(
            "(identifier) @variable\n{}\n{}",
            tree_sitter_rust::HIGHLIGHTS_QUERY,
            include_str!("syntax/rust.scm")
        );
        Query::new(&tree_sitter_rust::LANGUAGE.into(), &source).expect("bundled Rust query")
    })
}

fn capture_style(name: &str) -> Option<StyleRole> {
    Some(match name.split('.').next()? {
        "keyword" => StyleRole::SyntaxKeyword,
        "comment" => StyleRole::SyntaxComment,
        "string" | "escape" => StyleRole::SyntaxString,
        "number" => StyleRole::SyntaxNumber,
        "type" | "constructor" => StyleRole::SyntaxType,
        "function" => StyleRole::SyntaxFunction,
        "variable" | "property" | "label" => StyleRole::SyntaxVariable,
        "constant" => StyleRole::SyntaxConstant,
        "operator" => StyleRole::SyntaxOperator,
        "punctuation" => StyleRole::SyntaxPunctuation,
        _ => return None,
    })
}

fn parse(
    snapshot: BufferSnapshot,
    mut base: Option<Parsed>,
    change: Option<Change>,
    cancelled: &AtomicBool,
) -> Result<Parsed, &'static str> {
    if cancelled.load(Ordering::Relaxed) {
        return Err("cancelled");
    }
    let text = snapshot.text();
    if text.len() > u32::MAX as usize {
        return Err("text exceeds tree-sitter byte range");
    }
    if let (Some(base), Some(change)) = (&mut base, change) {
        base.tree
            .edit(&change.input_edit(base.snapshot.text(), text));
    }
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .expect("bundled Rust grammar");
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
            base.as_ref().map(|base| &base.tree),
            Some(ParseOptions::new().progress_callback(&mut progress)),
        )
        .ok_or("parse cancelled")?;
    let spans = query(&tree, text, cancelled)?;
    Ok(Parsed {
        highlights: Arc::new(Highlights {
            snapshot: snapshot.id(),
            spans,
        }),
        snapshot,
        tree,
    })
}

fn append_span(spans: &mut Vec<Span>, range: Range<usize>, style: StyleRole) {
    if let Some(last) = spans.last_mut()
        && last.range.end == range.start
        && last.style == style
    {
        last.range.end = range.end;
    } else {
        spans.push(Span { range, style });
    }
}

fn emit_until(
    spans: &mut Vec<Span>,
    active: &mut BinaryHeap<(usize, usize, usize, StyleRole)>,
    position: &mut usize,
    end: usize,
) {
    while *position < end {
        while active.peek().is_some_and(|entry| entry.2 <= *position) {
            active.pop();
        }
        let Some(&(_, _, stop, style)) = active.peek() else {
            *position = end;
            break;
        };
        let stop = stop.min(end);
        append_span(spans, *position..stop, style);
        *position = stop;
    }
}

fn query(tree: &Tree, text: &Rope, cancelled: &AtomicBool) -> Result<Vec<Span>, &'static str> {
    let query = rust_query();
    let styles: Vec<_> = query
        .capture_names()
        .iter()
        .map(|name| capture_style(name))
        .collect();
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
        |node: tree_sitter::Node| text.chunks_in_range(node.byte_range()).map(str::as_bytes),
        QueryCursorOptions::new().progress_callback(&mut progress),
    );
    let mut active = BinaryHeap::new();
    let mut spans = Vec::new();
    let mut position = 0;
    let mut sequence = 0;
    while let Some((matched, index)) = captures.next() {
        if cancelled.load(Ordering::Relaxed) {
            return Err("query cancelled");
        }
        let capture = matched.captures()[*index];
        let Some(style) = styles[capture.index as usize] else {
            continue;
        };
        let range = capture.node.byte_range();
        emit_until(&mut spans, &mut active, &mut position, range.start);
        active.push((matched.pattern_index, sequence, range.end, style));
        sequence += 1;
    }
    drop(captures);
    if cancelled.load(Ordering::Relaxed) {
        return Err("query cancelled");
    }
    if cursor.did_exceed_match_limit() {
        return Err("query match limit exceeded");
    }
    emit_until(&mut spans, &mut active, &mut position, text.len());
    Ok(spans)
}

#[cfg(test)]
mod property_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod worker_tests;
