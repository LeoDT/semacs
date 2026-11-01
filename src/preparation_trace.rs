//! Opt-in, bounded CPU wall-clock tracing for the single-open diagnostic.

use std::cell::RefCell;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Instant;

const CAPACITY: usize = 16_384;

#[derive(Clone, Debug)]
pub struct Sample {
    pub parent: Option<usize>,
    pub lane: &'static str,
    pub phase: &'static str,
    pub bytes: usize,
    pub start: Instant,
    pub end: Option<Instant>,
}

#[derive(Default)]
struct State {
    samples: Vec<Sample>,
    lost: usize,
    closed: bool,
}

#[derive(Clone, Default)]
pub struct Capture(Arc<Mutex<State>>);

#[derive(Clone)]
pub struct Context {
    capture: Capture,
    lane: &'static str,
    parent: Option<usize>,
}

thread_local! {
    static CURRENT: RefCell<Option<Context>> = const { RefCell::new(None) };
}

pub struct Recording {
    previous: Option<Context>,
    _foreground: PhantomData<Rc<()>>,
}

impl Capture {
    pub fn enter(&self) -> Recording {
        Context {
            capture: self.clone(),
            lane: "foreground",
            parent: None,
        }
        .enter()
    }

    pub fn finish(&self) -> (Vec<Sample>, usize, Instant) {
        let mut state = self.0.lock().unwrap();
        state.closed = true;
        (
            std::mem::take(&mut state.samples),
            state.lost,
            Instant::now(),
        )
    }
}

impl Context {
    pub fn worker(lane: &'static str) -> Option<Self> {
        CURRENT.with(|current| {
            current.borrow().as_ref().map(|context| Self {
                capture: context.capture.clone(),
                lane,
                parent: None,
            })
        })
    }

    pub fn enter(self) -> Recording {
        let previous = CURRENT.with(|current| current.replace(Some(self)));
        Recording {
            previous,
            _foreground: PhantomData,
        }
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        CURRENT.with(|current| current.replace(self.previous.take()));
    }
}

pub struct Span {
    active: Option<(Context, usize)>,
    _foreground: PhantomData<Rc<()>>,
}

impl Span {
    pub fn new(phase: &'static str, bytes: usize) -> Self {
        Self::start(phase, bytes, false)
    }

    pub fn detail(phase: &'static str, bytes: usize) -> Self {
        Self::start(phase, bytes, true)
    }

    fn start(phase: &'static str, bytes: usize, detail: bool) -> Self {
        let active = CURRENT.with(|current| {
            let mut current = current.borrow_mut();
            let context = current.as_mut()?;
            if detail && context.lane != "foreground" {
                return None;
            }
            let mut state = context.capture.0.lock().unwrap();
            if state.closed {
                return None;
            }
            if state.samples.len() == CAPACITY {
                state.lost += 1;
                return None;
            }
            let index = state.samples.len();
            state.samples.push(Sample {
                parent: context.parent,
                lane: context.lane,
                phase,
                bytes,
                start: Instant::now(),
                end: None,
            });
            let previous = context.clone();
            context.parent = Some(index);
            Some((previous, index))
        });
        Self {
            active,
            _foreground: PhantomData,
        }
    }
}

impl Drop for Span {
    fn drop(&mut self) {
        if let Some((context, index)) = self.active.take() {
            let end = Instant::now();
            let mut state = context.capture.0.lock().unwrap();
            if !state.closed {
                state.samples[index].end = Some(end);
            }
            CURRENT.with(|current| {
                if let Some(current) = current.borrow_mut().as_mut() {
                    current.parent = context.parent;
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_spans_workers_and_unfinished_work_keep_their_boundaries() {
        assert!(Context::worker("worker").is_none());
        let capture = Capture::default();
        let recording = capture.enter();
        let prepare = Span::new("prepare", 100);
        {
            let _measure = Span::detail("measure-grapheme", 3);
        }
        let worker = Context::worker("wrap-worker").unwrap();
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (stop_tx, stop_rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let _recording = worker.enter();
            let _wrap = Span::new("wrap-worker", 100);
            let _detail = Span::detail("measure-grapheme", 3);
            started_tx.send(()).unwrap();
            stop_rx.recv().unwrap();
        });
        started_rx.recv().unwrap();
        let (samples, lost, stopped) = capture.finish();
        stop_tx.send(()).unwrap();
        worker.join().unwrap();
        assert_eq!(lost, 0);
        assert_eq!(samples.len(), 3);
        assert_eq!(samples[1].parent, Some(0));
        assert!(samples[0].end.is_none());
        assert!(samples[1].end.unwrap() <= stopped);
        assert_eq!(samples[2].lane, "wrap-worker");
        assert_eq!(samples[2].parent, None);
        assert!(samples[2].end.is_none());
        drop(prepare);
        drop(recording);
        assert!(Context::worker("worker").is_none());
    }

    #[test]
    fn trace_is_bounded_and_nested_captures_restore_the_previous_context() {
        let capture = Capture::default();
        let _recording = capture.enter();
        {
            let nested = Capture::default();
            let _nested = nested.enter();
            let _span = Span::new("nested", 0);
        }
        for _ in 0..=CAPACITY {
            let _span = Span::new("bounded", 0);
        }
        let (samples, lost, _) = capture.finish();
        assert_eq!(samples.len(), CAPACITY);
        assert_eq!(lost, 1);
        assert!(samples.iter().all(|sample| sample.end.is_some()));
    }
}
