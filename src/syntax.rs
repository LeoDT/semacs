//! Shared, version-bound syntax work. Text and geometry remain owned by their existing models.

use std::cell::RefCell;
use std::ops::Range;
use std::path::Path;
use std::rc::{Rc, Weak};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use gpui_kit::{AnyWeakEntity, App, BackgroundExecutor, Task};
use tree_sitter::{InputEdit, Point};

mod display;
mod engine;
pub(crate) use display::DisplayHighlights;
mod languages;
use engine::{Parsed, parse};
use languages::{LanguageId, for_path};

use crate::buffer::{BufferSnapshot, SnapshotId};
use crate::text::Rope;
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
    language: Option<LanguageId>,
    generation: u64,
    latest: BufferSnapshot,
    base: Option<BackgroundOwned<Parsed>>,
    display: Option<BackgroundOwned<Arc<DisplayHighlights>>>,
    pending: Option<Change>,
    work: Option<Work>,
    failed: Option<SnapshotId>,
    observers: Vec<AnyWeakEntity>,
}

pub(crate) struct Syntax(Rc<RefCell<State>>);

impl Syntax {
    pub fn new(snapshot: BufferSnapshot, path: Option<&Path>) -> Self {
        Self(Rc::new(RefCell::new(State {
            language: for_path(path),
            generation: 0,
            latest: snapshot,
            base: None,
            display: None,
            pending: None,
            work: None,
            failed: None,
            observers: Vec::new(),
        })))
    }

    pub fn set_path(&self, path: &Path) {
        let mut state = self.0.borrow_mut();
        let language = for_path(Some(path));
        if language == state.language {
            return;
        }
        state.language = language;
        state.generation = state
            .generation
            .checked_add(1)
            .expect("syntax generation exhausted");
        state.work = None;
        state.base = None;
        state.display = None;
        state.pending = None;
        state.failed = None;
    }

    pub fn edited(&self, snapshot: BufferSnapshot, old: Range<usize>, new_end: usize) {
        let mut state = self.0.borrow_mut();
        if let Some(display) = state.display.take() {
            state.display = Some(BackgroundOwned::new(
                Arc::new(display.edited(snapshot.id(), old.clone(), new_end)),
                display.executor.clone(),
            ));
        }
        state.latest = snapshot;
        if state.language.is_some() {
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
            .filter(|base| {
                Some(base.language) == state.language && base.snapshot.id() == state.latest.id()
            })
            .map(|base| base.highlights.clone())
    }

    pub fn display_highlights(&self) -> Option<Arc<DisplayHighlights>> {
        self.0
            .borrow()
            .display
            .as_ref()
            .map(|display| Arc::clone(display))
    }

    pub fn ready(&self) -> bool {
        self.0.borrow().language.is_none() || self.highlights().is_some()
    }

    pub fn prepare(&self, observer: AnyWeakEntity, cx: &mut App) {
        {
            let mut state = self.0.borrow_mut();
            if state.language.is_none() {
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

fn start(shared: &Rc<RefCell<State>>, cx: &mut App) {
    let mut state = shared.borrow_mut();
    let Some(language) = state.language else {
        return;
    };
    if state.work.is_some()
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
            language,
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
                    Ok(parsed) => {
                        if state.latest.id() == id {
                            state.display = Some(BackgroundOwned::new(
                                Arc::new(DisplayHighlights::new(
                                    parsed.highlights.clone(),
                                    parsed.snapshot.text().len(),
                                )),
                                parsed.executor.clone(),
                            ));
                        }
                        state.base = Some(parsed);
                    }
                    Err(error) => {
                        log::warn!("{language:?} syntax unavailable: {error}");
                        state.failed = Some(id);
                        state.display = None;
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

#[cfg(test)]
mod display_worker_tests;
#[cfg(test)]
mod language_tests;
#[cfg(test)]
mod property_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod worker_tests;
