use std::collections::HashSet;
use std::sync::atomic::AtomicBool;

use gpui_kit as gpui;
use gpui_kit::{TestAppContext, font, px};
use rand::{Rng, SeedableRng, rngs::StdRng};
use unicode_segmentation::UnicodeSegmentation;

use semacs::buffer::{Buffer, Edit, EditSource};
use semacs::display::{DisplayChange, FontMeasure, LayoutSettings, WrapIndex, WrapJob};
use semacs::files::{DecodedFile, FileFormat};
use semacs::input::{DisplaySnapshot, InputSession};
use semacs::text::{ByteOffset, OffsetUtf16};

fn buffer(text: &str) -> Buffer {
    Buffer::new(
        None,
        DecodedFile {
            text: text.into(),
            format: FileFormat::default(),
        },
    )
}

fn settings(cx: &TestAppContext, columns: f32) -> LayoutSettings {
    let mut settings = LayoutSettings {
        font: font("monospace"),
        font_size: px(16.),
        line_height: px(24.),
        wrap_width: px(100.),
        soft_wrap: true,
        tab_size: 8,
        typography: Default::default(),
        visible_lines: None,
    };
    let mut metrics = FontMeasure::new(cx.text_system().clone(), settings.clone());
    settings.wrap_width = metrics.width("x") * columns;
    settings
}

fn finish(buffer: &Buffer, settings: LayoutSettings, cx: &TestAppContext) -> WrapIndex {
    let snapshot = InputSession::new(buffer).display_snapshot(buffer).unwrap();
    let mut metrics = FontMeasure::new(cx.text_system().clone(), settings.clone());
    WrapJob::new(snapshot, settings, None, None)
        .finish(&mut metrics, &AtomicBool::new(false))
        .unwrap()
}

fn assert_exact(index: &WrapIndex) {
    assert!(index.is_complete());
    assert_eq!(index.indexed_bytes(), index.text().len());
    let text = index.text().to_string();
    let boundaries: HashSet<_> = text
        .grapheme_indices(true)
        .map(|(i, _)| i)
        .chain([text.len()])
        .collect();
    let mut byte = 0;
    for row in index.visible_rows(0..index.row_count()) {
        assert_eq!(row.start, ByteOffset(byte));
        assert!(boundaries.contains(&byte));
        assert!(boundaries.contains(&(byte + row.row.content_len)));
        assert!(row.row.byte_len >= row.row.content_len);
        assert!(row.row.byte_len <= row.row.content_len + 1);
        if row.row.byte_len > row.row.content_len {
            assert_eq!(
                &text[byte + row.row.content_len..byte + row.row.byte_len],
                "\n"
            );
        }
        byte += row.row.byte_len;
        if row.row.byte_len > 0 {
            assert_eq!(index.row_for_offset(row.start).unwrap().index, row.index);
        }
    }
    assert_eq!(byte, text.len());
    assert_eq!(
        index.row_for_offset(ByteOffset(text.len())).unwrap().index,
        index.row_count() - 1
    );
    assert!(index.row_for_offset(ByteOffset(text.len() + 1)).is_none());
}

#[gpui::test]
fn truncation_uses_logical_rows_without_scanning_or_measuring_the_text(cx: &mut TestAppContext) {
    for text in ["", "abc", "中🙂e\u{301}\t\n\nend", "abc\n"] {
        let buffer = buffer(text);
        let mut settings = settings(cx, 1.);
        settings.soft_wrap = false;
        let index = finish(&buffer, settings, cx);
        assert_exact(&index);
        assert_eq!(
            index.row_count(),
            buffer.text().max_point().row as usize + 1
        );
        assert_eq!(index.scanned_bytes(), 0);
    }
    let buffer = buffer(&"中🙂a\t".repeat(150_000));
    let mut settings = settings(cx, 1.);
    settings.soft_wrap = false;
    let mut metrics = FontMeasure::new(cx.text_system().clone(), settings.clone());
    let snapshot = InputSession::new(&buffer)
        .display_snapshot(&buffer)
        .unwrap();
    let index = WrapJob::new(snapshot, settings, None, None).preview(&mut metrics, 1, 1);
    assert!(index.is_complete());
    assert_eq!(index.row_count(), 1);
    assert_eq!(index.row(0).unwrap().row.content_len, buffer.text().len());
    assert_eq!(index.scanned_bytes(), 0);
    assert_exact(&index);
}

#[gpui::test]
fn wrapping_mode_changes_cannot_reuse_incompatible_rows(cx: &mut TestAppContext) {
    let buffer = buffer("abcdefghij\n0123456789");
    let snapshot = InputSession::new(&buffer)
        .display_snapshot(&buffer)
        .unwrap();
    let soft = settings(cx, 4.);
    let wrapped = finish(&buffer, soft.clone(), cx);
    assert_eq!(wrapped.row_count(), 6);
    let mut truncated = soft.clone();
    truncated.soft_wrap = false;
    let mut metrics = FontMeasure::new(cx.text_system().clone(), truncated.clone());
    let logical = WrapJob::new(snapshot.clone(), truncated, Some(&wrapped), None)
        .finish(&mut metrics, &AtomicBool::new(false))
        .unwrap();
    assert_eq!(logical.row_count(), 2);
    let mut metrics = FontMeasure::new(cx.text_system().clone(), soft.clone());
    let restored = WrapJob::new(snapshot, soft, Some(&logical), None)
        .finish(&mut metrics, &AtomicBool::new(false))
        .unwrap();
    assert_exact(&restored);
    assert_eq!(restored.row_count(), wrapped.row_count());
    for row in 0..wrapped.row_count() {
        assert_eq!(
            restored.row(row).unwrap().row,
            wrapped.row(row).unwrap().row
        );
    }
}

#[gpui::test]
fn soft_wrap_index_covers_unicode_tabs_blank_lines_and_eof(cx: &mut TestAppContext) {
    for text in [
        "",
        "\n",
        "ab\n\n",
        "中🙂e\u{301}\t末尾",
        "abcdefghijklmno",
        "👩‍💻🇨🇳x\n\t🙂",
    ] {
        let index = finish(&buffer(text), settings(cx, 4.), cx);
        assert_exact(&index);
        assert_eq!(index.text().to_string(), text);
    }
    let index = finish(&buffer("abcdefghij"), settings(cx, 4.), cx);
    assert_eq!(index.row_count(), 3);
    assert_eq!(index.row(0).unwrap().row.content_len, 4);
    assert_eq!(index.row(1).unwrap().row.content_len, 4);
    assert_eq!(index.row(2).unwrap().row.content_len, 2);
}

#[gpui::test]
fn preview_is_bounded_and_never_publishes_an_unfinished_row(cx: &mut TestAppContext) {
    let buffer = buffer(&"x".repeat(1024 * 1024));
    let snapshot = InputSession::new(&buffer)
        .display_snapshot(&buffer)
        .unwrap();
    let settings = settings(cx, 80.);
    let mut metrics = FontMeasure::new(cx.text_system().clone(), settings.clone());
    let job = WrapJob::new(snapshot, settings.clone(), None, None);
    let preview = job.preview(&mut metrics, 3, 8192);
    assert!(!preview.is_complete());
    assert_eq!(preview.row_count(), 3);
    assert!(preview.indexed_bytes() <= 8192);
    assert!(preview.scanned_bytes() <= 8192);
    assert!(
        preview
            .row_for_offset(ByteOffset(preview.indexed_bytes()))
            .is_none()
    );
    let partial = job.preview(&mut metrics, usize::MAX, 96);
    assert_eq!(partial.row_count(), 1);
    assert_eq!(partial.indexed_bytes(), 80);
    assert!(job.finish(&mut metrics, &AtomicBool::new(true)).is_none());

    let text = format!("a{}z", "\u{301}".repeat(16_384));
    let buffer = self::buffer(&text);
    let snapshot = InputSession::new(&buffer)
        .display_snapshot(&buffer)
        .unwrap();
    let job = WrapJob::new(snapshot, settings, None, None);
    let partial = job.preview(&mut metrics, 30, 8192);
    assert_eq!(partial.row_count(), 0);
    assert_eq!(partial.indexed_bytes(), 0);
    assert!(!partial.is_complete());
    assert!(partial.scanned_bytes() <= 8192);
}

#[gpui::test]
fn rope_fragment_seams_never_split_graphemes(cx: &mut TestAppContext) {
    let text = "x👩‍💻e\u{301}🇨🇳中🙂\t\n".repeat(300);
    let index = finish(&buffer(&text), settings(cx, 9.), cx);
    assert_exact(&index);
}

#[gpui::test]
fn incremental_reflow_matches_full_index_for_insert_delete_join_and_eof(cx: &mut TestAppContext) {
    let settings = settings(cx, 12.);
    for (initial, start, end, inserted) in [
        ("first line\nsecond line\nlast\n", 0, 1, "x"),
        ("first line\nsecond line\nlast\n", 10, 11, ""),
        ("first line\nsecond line\nlast\n", 11, 11, "中🙂\n"),
        ("first line\nsecond line\nlast\n", 27, 27, "EOF"),
        ("abc", 3, 3, "\n"),
        ("abc\n", 4, 4, "z"),
        ("abc\n", 0, 4, ""),
        ("", 0, 0, "\n"),
    ] {
        let mut buffer = buffer(initial);
        let input = InputSession::new(&buffer);
        let before = input.display_snapshot(&buffer).unwrap();
        let mut metrics = FontMeasure::new(cx.text_system().clone(), settings.clone());
        let old = WrapJob::new(before.clone(), settings.clone(), None, None)
            .finish(&mut metrics, &AtomicBool::new(false))
            .unwrap();
        buffer
            .edit(Edit {
                snapshot: buffer.snapshot_id(),
                range: ByteOffset(start)..ByteOffset(end),
                text: inserted,
                source: EditSource::Command,
            })
            .unwrap();
        let after = InputSession::new(&buffer)
            .display_snapshot(&buffer)
            .unwrap();
        let change = DisplayChange {
            before: before.id(),
            after: after.id(),
            old_range: ByteOffset(start)..ByteOffset(end),
            new_range: ByteOffset(start)..ByteOffset(start + inserted.len()),
        };
        let job = WrapJob::new(after.clone(), settings.clone(), Some(&old), Some(&change));
        let incremental = job.finish(&mut metrics, &AtomicBool::new(false)).unwrap();
        let full = WrapJob::new(after, settings.clone(), None, None)
            .finish(&mut metrics, &AtomicBool::new(false))
            .unwrap();
        assert_exact(&incremental);
        assert_eq!(
            incremental
                .visible_rows(0..incremental.row_count())
                .iter()
                .map(|row| &row.row)
                .collect::<Vec<_>>(),
            full.visible_rows(0..full.row_count())
                .iter()
                .map(|row| &row.row)
                .collect::<Vec<_>>(),
            "{initial:?} {start}..{end} {inserted:?}"
        );
        if start >= 11 && initial.len() > 11 {
            assert!(job.reflow_range().start.0 >= 11);
        }
    }
}

#[gpui::test]
fn checkpoints_converge_on_unchanged_long_line_suffix_and_bound_lookbehind(
    cx: &mut TestAppContext,
) {
    let settings = settings(cx, 80.);
    let mut metrics = FontMeasure::new(cx.text_system().clone(), settings.clone());
    for initial in ["x".repeat(1024 * 1024), "🇨🇳".repeat(8192)] {
        let mut buffer = buffer(&initial);
        let before = InputSession::new(&buffer)
            .display_snapshot(&buffer)
            .unwrap();
        let old = WrapJob::new(before.clone(), settings.clone(), None, None)
            .finish(&mut metrics, &AtomicBool::new(false))
            .unwrap();
        let start = if initial.is_ascii() {
            0
        } else {
            initial.len() - 16
        };
        let end = start + if initial.is_ascii() { 1 } else { 8 };
        let inserted = if initial.is_ascii() { "z" } else { "🇯🇵" };
        buffer
            .edit(Edit {
                snapshot: buffer.snapshot_id(),
                range: ByteOffset(start)..ByteOffset(end),
                text: inserted,
                source: EditSource::Command,
            })
            .unwrap();
        let after = InputSession::new(&buffer)
            .display_snapshot(&buffer)
            .unwrap();
        let change = DisplayChange {
            before: before.id(),
            after: after.id(),
            old_range: ByteOffset(start)..ByteOffset(end),
            new_range: ByteOffset(start)..ByteOffset(start + inserted.len()),
        };
        let job = WrapJob::new(after.clone(), settings.clone(), Some(&old), Some(&change));
        let preview = job.preview(&mut metrics, 3, 8192);
        assert!(preview.scanned_bytes() <= 8192);
        if initial.is_ascii() {
            assert!(
                preview.is_complete(),
                "unchanged suffix should converge within the preview"
            );
            assert!(preview.scanned_bytes() < 256);
            assert_eq!(preview.row_count(), old.row_count());
        }
        let result = job.finish(&mut metrics, &AtomicBool::new(false)).unwrap();
        assert_exact(&result);
        let full = WrapJob::new(after, settings.clone(), None, None)
            .finish(&mut metrics, &AtomicBool::new(false))
            .unwrap();
        assert_eq!(
            result
                .visible_rows(0..result.row_count())
                .into_iter()
                .map(|r| r.row)
                .collect::<Vec<_>>(),
            full.visible_rows(0..full.row_count())
                .into_iter()
                .map(|r| r.row)
                .collect::<Vec<_>>()
        );
    }
}

#[gpui::test]
fn different_targets_never_reuse_a_previous_buffers_index(cx: &mut TestAppContext) {
    let settings = settings(cx, 4.);
    let a = buffer("aaaa\nbbbb\ncccc");
    let before = InputSession::new(&a).display_snapshot(&a).unwrap();
    let b = buffer("dddd\neeee\nffff");
    let after = InputSession::new(&b).display_snapshot(&b).unwrap();
    let mut metrics = FontMeasure::new(cx.text_system().clone(), settings.clone());
    let old = WrapJob::new(before.clone(), settings.clone(), None, None)
        .finish(&mut metrics, &AtomicBool::new(false))
        .unwrap();
    let change = DisplayChange {
        before: before.id(),
        after: after.id(),
        old_range: ByteOffset(6)..ByteOffset(7),
        new_range: ByteOffset(6)..ByteOffset(7),
    };
    let job = WrapJob::new(after, settings, Some(&old), Some(&change));
    assert_eq!(
        job.reflow_range(),
        ByteOffset(0)..ByteOffset(b.text().len())
    );
    assert_exact(&job.finish(&mut metrics, &AtomicBool::new(false)).unwrap());
}

fn check_projection_reflow(
    snapshot: DisplaySnapshot,
    previous: &WrapIndex,
    change: &DisplayChange,
    metrics: &mut FontMeasure,
) -> WrapIndex {
    let settings = metrics.settings().clone();
    let job = WrapJob::new(
        snapshot.clone(),
        settings.clone(),
        Some(previous),
        Some(change),
    );
    assert!(
        job.reflow_range().start.0 > 0,
        "must reuse an unchanged prefix"
    );
    let incremental = job.finish(metrics, &AtomicBool::new(false)).unwrap();
    let full = WrapJob::new(snapshot.clone(), settings.clone(), None, None)
        .finish(metrics, &AtomicBool::new(false))
        .unwrap();
    assert_exact(&incremental);
    assert_eq!(incremental.text().to_string(), snapshot.text().to_string());
    let rows = |index: &WrapIndex| {
        index
            .visible_rows(0..index.row_count())
            .into_iter()
            .map(|row| (row.start, row.row))
            .collect::<Vec<_>>()
    };
    assert_eq!(rows(&incremental), rows(&full));

    let mut wrong_after = change.clone();
    wrong_after.after = change.before;
    let stale = WrapJob::new(snapshot, settings, Some(previous), Some(&wrong_after));
    assert_eq!(
        stale.reflow_range(),
        ByteOffset(0)..ByteOffset(full.text().len()),
    );
    incremental
}

#[gpui::test]
fn input_projection_deltas_drive_incremental_reflow_through_commit_and_cancel(
    cx: &mut TestAppContext,
) {
    let prefix = "prefix\n".repeat(30);
    let initial = format!("{prefix}0123456789\t中🙂\n{}", "suffix\t中🙂\n".repeat(200));
    let start = prefix.encode_utf16().count();
    let settings = settings(cx, 12.);
    for finish in 0..3 {
        let mut buffer = buffer(&initial);
        let mut input = InputSession::new(&buffer);
        let token = input.token();
        let before = input.display_snapshot(&buffer).unwrap();
        let mut metrics = FontMeasure::new(cx.text_system().clone(), settings.clone());
        let original = WrapJob::new(before.clone(), settings.clone(), None, None)
            .finish(&mut metrics, &AtomicBool::new(false))
            .unwrap();
        let mut index = original.clone();
        let mut cumulative: Option<DisplayChange> = None;
        for (range, text) in [
            (Some(start + 3..start + 6), "ABCD"),
            (Some(start + 4..start + 6), "中\n🙂"),
            (Some(start..start + 1), "x"),
            (None, "y"),
        ] {
            let range = range.map(|r| OffsetUtf16(r.start)..OffsetUtf16(r.end));
            let update = input
                .replace_and_mark(&mut buffer, token, range, text, None)
                .unwrap();
            let after = input.display_snapshot(&buffer).unwrap();
            index = check_projection_reflow(after.clone(), &index, &update.change, &mut metrics);
            let composed = cumulative.as_ref().map_or_else(
                || update.change.clone(),
                |change| change.compose(&update.change),
            );
            check_projection_reflow(after, &original, &composed, &mut metrics);
            cumulative = Some(composed);
            assert!(buffer.is_modified());
        }
        let change = match finish {
            0 => input.cancel(&buffer, token).unwrap().1,
            1 => {
                input
                    .replace_text(
                        &mut buffer,
                        token,
                        Some(OffsetUtf16(start + 1)..OffsetUtf16(start + 2)),
                        "",
                    )
                    .unwrap()
                    .change
            }
            _ => input.commit_preedit(&mut buffer).unwrap().unwrap().change,
        };
        let after = input.display_snapshot(&buffer).unwrap();
        check_projection_reflow(after.clone(), &index, &change, &mut metrics);
        check_projection_reflow(
            after,
            &original,
            &cumulative.unwrap().compose(&change),
            &mut metrics,
        );
        assert!(buffer.is_modified());
    }
}

#[gpui::test]
fn randomized_local_reflow_and_stale_change_fallback(cx: &mut TestAppContext) {
    let settings = settings(cx, 10.);
    let mut metrics = FontMeasure::new(cx.text_system().clone(), settings.clone());
    let mut buffer = buffer(&"中🙂e\u{301}\t\nabcdef\n".repeat(15));
    let mut rng = StdRng::seed_from_u64(264299);
    let mut before = InputSession::new(&buffer)
        .display_snapshot(&buffer)
        .unwrap();
    let mut index = WrapJob::new(before.clone(), settings.clone(), None, None)
        .finish(&mut metrics, &AtomicBool::new(false))
        .unwrap();
    for _ in 0..150 {
        let text = buffer.text().to_string();
        let boundaries: Vec<_> = text
            .char_indices()
            .map(|(i, _)| i)
            .chain([text.len()])
            .collect();
        let a = rng.random_range(0..boundaries.len());
        let b = rng.random_range(a..boundaries.len());
        let start = boundaries[a];
        let end = boundaries[b];
        let inserted = ["x", "中🙂", "\n", "\t", "e\u{301}", ""][rng.random_range(0..6)];
        buffer
            .edit(Edit {
                snapshot: buffer.snapshot_id(),
                range: ByteOffset(start)..ByteOffset(end),
                text: inserted,
                source: EditSource::Command,
            })
            .unwrap();
        let after = InputSession::new(&buffer)
            .display_snapshot(&buffer)
            .unwrap();
        let change = DisplayChange {
            before: before.id(),
            after: after.id(),
            old_range: ByteOffset(start)..ByteOffset(end),
            new_range: ByteOffset(start)..ByteOffset(start + inserted.len()),
        };
        let job = WrapJob::new(after.clone(), settings.clone(), Some(&index), Some(&change));
        index = job.finish(&mut metrics, &AtomicBool::new(false)).unwrap();
        let full = WrapJob::new(after.clone(), settings.clone(), None, None)
            .finish(&mut metrics, &AtomicBool::new(false))
            .unwrap();
        assert_exact(&index);
        let rows = |index: &WrapIndex| {
            index
                .visible_rows(0..index.row_count())
                .into_iter()
                .map(|row| row.row)
                .collect::<Vec<_>>()
        };
        assert_eq!(rows(&index), rows(&full));
        let stale = WrapJob::new(after.clone(), settings.clone(), Some(&index), Some(&change));
        assert_eq!(
            stale.reflow_range(),
            ByteOffset(0)..ByteOffset(buffer.text().len())
        );
        before = after;
    }
}
