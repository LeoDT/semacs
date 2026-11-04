use super::*;
use crate::buffer::{Buffer, Edit, EditSource};
use crate::files::{DecodedFile, FileFormat};
use crate::text::ByteOffset;
use gpui_kit::{AppContext, TestAppContext};
use std::sync::atomic::AtomicUsize;

struct DropProbe {
    executor: BackgroundExecutor,
    dropped_on: Arc<AtomicUsize>,
}

impl Drop for DropProbe {
    fn drop(&mut self) {
        self.dropped_on.store(
            if self.executor.is_main_thread() { 1 } else { 2 },
            Ordering::SeqCst,
        );
    }
}

#[test]
fn completed_but_unreceived_results_and_queued_input_are_released_on_the_background_lane() {
    let dispatcher = gpui_kit::TestDispatcher::new(0);
    let cx = TestAppContext::build(dispatcher.clone(), Some("syntax-background-release"));
    let executor = cx.executor();
    for completed in [false, true] {
        let dropped_on = Arc::new(AtomicUsize::new(0));
        let probe = DropProbe {
            executor: executor.clone(),
            dropped_on: dropped_on.clone(),
        };
        let parsed = parse(
            LanguageId::Rust,
            buffer().snapshot(),
            None,
            None,
            &AtomicBool::new(false),
        )
        .unwrap();
        let guarded = BackgroundOwned::new((parsed, probe), executor.clone());
        let finished = Arc::new(AtomicBool::new(false));
        let worker_finished = finished.clone();
        let worker = executor.spawn(async move {
            worker_finished.store(true, Ordering::SeqCst);
            guarded
        });
        let receiver = cx.foreground_executor().spawn(async move {
            drop(worker.await);
        });
        if completed {
            while dispatcher.tick(true) {}
            assert!(finished.load(Ordering::SeqCst));
            assert_eq!(dropped_on.load(Ordering::SeqCst), 0);
        }
        drop(receiver);
        assert_eq!(dropped_on.load(Ordering::SeqCst), 0);
        dispatcher.run_until_parked();
        assert_eq!(dropped_on.load(Ordering::SeqCst), 2);
    }
    let dropped_on = Arc::new(AtomicUsize::new(0));
    let guarded = BackgroundOwned::new(
        DropProbe {
            executor: executor.clone(),
            dropped_on: dropped_on.clone(),
        },
        executor,
    );
    drop(guarded);
    assert_eq!(dropped_on.load(Ordering::SeqCst), 0);
    dispatcher.run_until_parked();
    assert_eq!(dropped_on.load(Ordering::SeqCst), 2);
    cx.quit();
    drop(cx);
    dispatcher.drain_tasks();
    dispatcher.scheduler().end_test();
}

fn buffer() -> Buffer {
    Buffer::new(
        Some("worker.rs".into()),
        DecodedFile {
            text: "fn main() {}\n".into(),
            format: FileFormat::default(),
        },
    )
}

#[gpui_kit::test]
fn coalesces_in_flight_edits_into_one_followup_and_only_exposes_current_results(
    cx: &mut TestAppContext,
) {
    let mut buffer = buffer();
    let syntax = Syntax::new(buffer.snapshot(), Some(Path::new("worker.rs")));
    let observer = cx.new(|_| ());
    cx.update(|cx| syntax.prepare(observer.downgrade().into(), cx));
    let cancel = syntax.0.borrow().work.as_ref().unwrap().cancelled.clone();
    cx.update(|cx| syntax.prepare(observer.downgrade().into(), cx));
    assert!(Arc::ptr_eq(
        &cancel,
        &syntax.0.borrow().work.as_ref().unwrap().cancelled
    ));
    for i in 0..64 {
        let start = buffer.text().len();
        let text = format!("const C{i}: i32 = {i}; // 中🙂\n");
        buffer
            .edit(Edit {
                snapshot: buffer.snapshot_id(),
                range: ByteOffset(start)..ByteOffset(start),
                text: &text,
                source: EditSource::Command,
            })
            .unwrap();
        syntax.edited(buffer.snapshot(), start..start, start + text.len());
        assert!(syntax.highlights().is_none());
    }
    assert!(syntax.0.borrow().pending.is_some());
    cx.run_until_parked();
    let result = syntax.highlights().unwrap();
    assert_eq!(result.snapshot, buffer.snapshot_id());
    assert!(syntax.0.borrow().pending.is_none());
    assert!(syntax.0.borrow().work.is_none());
    let fresh = parse(
        LanguageId::Rust,
        buffer.snapshot(),
        None,
        None,
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(result.spans, fresh.highlights.spans);
    assert!(cancel.load(Ordering::Relaxed));
}

#[gpui_kit::test]
fn language_epoch_cancels_old_work_even_when_text_identity_is_unchanged(cx: &mut TestAppContext) {
    let buffer = buffer();
    let syntax = Syntax::new(buffer.snapshot(), Some(Path::new("worker.rs")));
    let observer = cx.new(|_| ());
    cx.update(|cx| syntax.prepare(observer.downgrade().into(), cx));
    let cancelled = syntax.0.borrow().work.as_ref().unwrap().cancelled.clone();
    syntax.set_path(Path::new("worker.html"));
    assert!(cancelled.load(Ordering::Relaxed));
    assert!(!syntax.ready());
    assert!(syntax.highlights().is_none());
    syntax.set_path(Path::new("again.md"));
    assert_eq!(syntax.0.borrow().generation, 2);
    cx.update(|cx| syntax.prepare(observer.downgrade().into(), cx));
    cx.run_until_parked();
    assert_eq!(syntax.highlights().unwrap().snapshot, buffer.snapshot_id());
    assert_eq!(syntax.0.borrow().generation, 2);
    assert_eq!(
        syntax.0.borrow().base.as_ref().unwrap().language,
        LanguageId::Markdown
    );
    let highlights = syntax.highlights().unwrap();
    syntax.set_path(Path::new("alias.markdown"));
    assert_eq!(syntax.0.borrow().generation, 2);
    assert!(Arc::ptr_eq(&highlights, &syntax.highlights().unwrap()));
    syntax.set_path(Path::new("plain.txt"));
    assert!(syntax.ready());
    assert!(syntax.highlights().is_none());
}

#[gpui_kit::test]
fn work_outlives_its_initiating_view_but_not_the_shared_document(cx: &mut TestAppContext) {
    let syntax = Syntax::new(buffer().snapshot(), Some(Path::new("worker.rs")));
    let first = cx.new(|_| ());
    let second = cx.new(|_| ());
    cx.update(|cx| {
        syntax.prepare(first.downgrade().into(), cx);
        syntax.prepare(second.downgrade().into(), cx);
    });
    drop(first);
    cx.run_until_parked();
    assert!(syntax.ready());
    assert_eq!(syntax.0.borrow().observers.len(), 1);
    let other = Syntax::new(buffer().snapshot(), Some(Path::new("other.rs")));
    cx.update(|cx| other.prepare(second.downgrade().into(), cx));
    let weak = Rc::downgrade(&other.0);
    let cancelled = other.0.borrow().work.as_ref().unwrap().cancelled.clone();
    drop(other);
    assert!(weak.upgrade().is_none());
    assert!(cancelled.load(Ordering::Relaxed));
    cx.run_until_parked();
}
