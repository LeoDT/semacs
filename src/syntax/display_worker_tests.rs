use super::*;
use crate::buffer::{Buffer, Edit, EditSource};
use crate::files::{DecodedFile, FileFormat};
use crate::text::ByteOffset;
use gpui_kit::{AppContext, TestAppContext};

fn buffer() -> Buffer {
    Buffer::new(
        Some("worker.rs".into()),
        DecodedFile {
            text: "fn main() {}\n".into(),
            format: FileFormat::default(),
        },
    )
}

fn insert(buffer: &mut Buffer, syntax: &Syntax, offset: usize, text: &str) {
    buffer
        .edit(Edit {
            snapshot: buffer.snapshot_id(),
            range: ByteOffset(offset)..ByteOffset(offset),
            text,
            source: EditSource::Command,
        })
        .unwrap();
    syntax.edited(buffer.snapshot(), offset..offset, offset + text.len());
}

#[gpui_kit::test]
fn display_survives_worker_transfer_and_history_without_claiming_readiness(
    cx: &mut TestAppContext,
) {
    let mut buffer = buffer();
    let len = buffer.text().len();
    let syntax = Syntax::new(buffer.snapshot(), Some(Path::new("worker.rs")));
    let observer = cx.new(|_| ());
    cx.update(|cx| syntax.prepare(observer.downgrade().into(), cx));
    cx.run_until_parked();
    let original = syntax.display_highlights().unwrap();
    for _ in 0..32 {
        let start = buffer.text().len();
        insert(&mut buffer, &syntax, start, "中🙂");
        cx.update(|cx| syntax.prepare(observer.downgrade().into(), cx));
        let display = syntax.display_highlights().unwrap();
        assert_eq!(display.snapshot, buffer.snapshot_id());
        assert_eq!(
            display.in_range(0..2).next().unwrap().style,
            StyleRole::SyntaxKeyword
        );
        assert!(display.in_range(len..buffer.text().len()).next().is_none());
        assert!(!syntax.ready());
        assert!(syntax.highlights().is_none());
        assert!(syntax.0.borrow().base.is_none());
    }
    for redo in [false, false, true, true] {
        let edit = if redo { buffer.redo() } else { buffer.undo() }
            .unwrap()
            .unwrap();
        syntax.edited(
            buffer.snapshot(),
            edit.old_range.start.0..edit.old_range.end.0,
            edit.new_range.end.0,
        );
        let display = syntax.display_highlights().unwrap();
        assert_eq!(display.snapshot, buffer.snapshot_id());
        assert_eq!(
            display.in_range(0..2).next().unwrap().style,
            StyleRole::SyntaxKeyword
        );
        assert!(!syntax.ready());
    }
    cx.run_until_parked();
    let current = syntax.display_highlights().unwrap();
    assert!(syntax.ready());
    assert!(!Arc::ptr_eq(&original, &current));
    assert_eq!(
        current.in_range(0..buffer.text().len()).collect::<Vec<_>>(),
        syntax.highlights().unwrap().spans
    );
    syntax.set_path(Path::new("worker.html"));
    assert!(syntax.display_highlights().is_none());
    assert!(!syntax.ready());
}

#[gpui_kit::test]
fn failed_parse_discards_display_and_next_edit_recovers(cx: &mut TestAppContext) {
    let mut buffer = buffer();
    let syntax = Syntax::new(buffer.snapshot(), Some(Path::new("worker.rs")));
    let observer = cx.new(|_| ());
    cx.update(|cx| syntax.prepare(observer.downgrade().into(), cx));
    cx.run_until_parked();
    for fail in [true, false] {
        insert(&mut buffer, &syntax, 0, "// comment\n");
        cx.update(|cx| syntax.prepare(observer.downgrade().into(), cx));
        if fail {
            assert!(syntax.display_highlights().is_some());
            syntax
                .0
                .borrow()
                .work
                .as_ref()
                .unwrap()
                .cancelled
                .store(true, Ordering::Relaxed);
        }
        cx.run_until_parked();
        assert_eq!(syntax.ready(), !fail);
        assert_eq!(syntax.display_highlights().is_some(), !fail);
        assert_eq!(syntax.highlights().is_some(), !fail);
    }
}

#[gpui_kit::test]
fn display_cache_releases_its_last_highlight_owner_on_background(cx: &mut TestAppContext) {
    let syntax = Syntax::new(buffer().snapshot(), Some(Path::new("worker.rs")));
    let observer = cx.new(|_| ());
    cx.update(|cx| syntax.prepare(observer.downgrade().into(), cx));
    cx.run_until_parked();
    let weak = Arc::downgrade(&syntax.highlights().unwrap());
    drop(syntax);
    assert!(weak.upgrade().is_some());
    cx.run_until_parked();
    assert!(weak.upgrade().is_none());
}
