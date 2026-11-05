use super::{InitialPreparation, InitialViewport};
use crate::app::AppShell;
use crate::buffer::Buffer;
use crate::files::{DecodedFile, FileFormat};
use crate::text::ByteOffset;
use crate::view::{BufferView, PREVIEW_BYTES};
use gpui_kit as gpui;
use gpui_kit::{
    AppContext, Bounds, Entity, Pixels, TestAppContext, WindowHandle, font, point, px, size,
};
use std::sync::{Arc, atomic::Ordering};

fn setup(
    cx: &mut TestAppContext,
    text: &str,
) -> (WindowHandle<AppShell>, Entity<BufferView>, Bounds<Pixels>) {
    cx.update(crate::app::init);
    let window = cx.open_window(size(px(400.), px(240.)), |window, cx| {
        AppShell::new(Buffer::scratch(), window, cx)
    });
    let view = cx.update(|cx| {
        cx.new(|cx| {
            BufferView::new(
                Buffer::new(
                    None,
                    DecodedFile {
                        text: text.into(),
                        format: FileFormat::default(),
                    },
                ),
                cx,
            )
        })
    });
    let bounds = Bounds::new(point(px(12.), px(20.)), size(px(380.), px(170.)));
    (window, view, bounds)
}

fn request(
    cx: &mut TestAppContext,
    window: WindowHandle<AppShell>,
    view: &Entity<BufferView>,
    bounds: Bounds<Pixels>,
) -> (InitialViewport, InitialPreparation) {
    cx.update_window(window.into(), |_, window, cx| {
        let viewport = view.read(cx).initial_viewport(bounds, window, cx);
        let work = view.read(cx).prepare_initial(viewport.clone(), cx);
        (viewport, work)
    })
    .unwrap()
}

#[gpui::test]
async fn first_prepaint_reuses_background_rows_and_completes_the_remaining_index(
    cx: &mut TestAppContext,
) {
    let (window, view, bounds) = setup(cx, &"中🙂e\u{301}\tabcdefghijklmnop\n".repeat(20_000));
    let (viewport, work) = request(cx, window, &view, bounds);
    assert!(view.read_with(cx, |view, _| view.index.is_none() && view.metrics.is_none()));
    let prepared = work.ready().await.unwrap();
    assert!(!prepared.index.is_complete());
    assert!(prepared.index.scanned_bytes() <= PREVIEW_BYTES);
    let rows = prepared.rows.clone();
    assert!(!rows.is_empty());
    cx.update_window(window.into(), |_, window, cx| {
        view.update(cx, |view, cx| {
            assert!(view.install_initial(prepared, &viewport, cx));
            view.restore_input(window, cx);
            #[cfg(feature = "native-profiler")]
            let capture = crate::preparation_trace::Capture::default();
            #[cfg(feature = "native-profiler")]
            let recording = capture.enter();
            view.prepare(bounds, window, cx);
            #[cfg(feature = "native-profiler")]
            {
                drop(recording);
                let (samples, lost, _) = capture.finish();
                assert_eq!(lost, 0);
                assert!(samples.iter().any(|sample| sample.phase == "prepare"));
                assert!(samples.iter().all(|sample| !matches!(
                    sample.phase,
                    "font-init" | "wrap-preview" | "shape-row" | "shape-line" | "measure-grapheme"
                )));
            }
            let layout = view.viewport_layout().unwrap();
            assert_eq!(layout.rows.len(), layout.requested_rows);
            assert_eq!(layout.rows.len(), rows.len());
            for row in &layout.rows {
                assert!(Arc::ptr_eq(row, &rows[&row.source.index]));
            }
        });
    })
    .unwrap();
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        let index = view.index.as_ref().unwrap();
        assert!(index.is_complete());
        assert_eq!(index.indexed_bytes(), view.buffer().text().len());
        assert_eq!(index.key().display, view.display_snapshot().id());
    });
}

#[gpui::test]
async fn stale_text_session_font_width_and_height_results_are_not_installed(
    cx: &mut TestAppContext,
) {
    for change in ["text", "session", "font", "width", "height"] {
        let (window, view, bounds) = setup(cx, "abc中🙂\tlast");
        let (mut viewport, work) = request(cx, window, &view, bounds);
        let prepared = work.ready().await.unwrap();
        view.update(cx, |view, cx| {
            match change {
                "text" => {
                    view.target
                        .replace_text_range(view.target.token(), ByteOffset(0)..ByteOffset(1), "X")
                        .unwrap();
                }
                "session" => view.end_input(cx),
                "font" => view.set_font(font("other"), px(20.), px(30.), cx),
                "width" => {
                    viewport.size.width += px(20.);
                    viewport.settings.wrap_width = viewport.size.width;
                }
                "height" => viewport.size.height += px(24.),
                _ => unreachable!(),
            }
            assert!(
                !view.install_initial(prepared, &viewport, cx),
                "accepted stale {change}"
            );
            assert!(view.index.is_none());
            assert!(view.metrics.is_none());
            assert!(view.cache.is_empty());
        });
    }
}

#[gpui::test]
async fn empty_and_oversized_grapheme_preparations_always_have_a_real_first_row(
    cx: &mut TestAppContext,
) {
    for text in [
        String::new(),
        format!("a{}\nlast", "\u{301}".repeat(PREVIEW_BYTES * 2)),
    ] {
        let (window, view, bounds) = setup(cx, &text);
        let (_, work) = request(cx, window, &view, bounds);
        let prepared = work.ready().await.unwrap();
        assert!(!prepared.rows.is_empty());
        assert!(prepared.index.is_complete());
        assert_eq!(prepared.index.indexed_bytes(), text.len());
        assert!(prepared.rows.contains_key(&0));
    }
}

#[gpui::test]
async fn initial_preparation_is_reusable_after_a_theme_change(cx: &mut TestAppContext) {
    let (window, view, bounds) = setup(cx, "abc中🙂\tlast");
    let (viewport, work) = request(cx, window, &view, bounds);
    let prepared = work.ready().await.unwrap();
    let rows = prepared.rows.clone();
    cx.update(|cx| {
        let mut settings = crate::theme::ThemeSettings::default();
        settings.select("dark").unwrap();
        settings.install(cx);
    });
    cx.update_window(window.into(), |_, window, cx| {
        assert!(viewport.matches_window(window));
        view.update(cx, |view, cx| {
            assert!(view.install_initial(prepared, &viewport, cx));
            view.restore_input(window, cx);
            view.prepare(bounds, window, cx);
            assert!(
                view.viewport_layout()
                    .unwrap()
                    .rows
                    .iter()
                    .all(|row| Arc::ptr_eq(row, &rows[&row.source.index]))
            );
        });
    })
    .unwrap();
}

#[gpui::test]
fn dropping_preparation_cancels_the_worker_without_publishing(cx: &mut TestAppContext) {
    let (window, view, bounds) = setup(cx, &"x".repeat(1024 * 1024));
    let (_, work) = request(cx, window, &view, bounds);
    let cancelled = work.cancelled.clone();
    drop(work);
    assert!(cancelled.load(Ordering::Relaxed));
    cx.run_until_parked();
    assert!(view.read_with(cx, |view, _| view.index.is_none() && view.cache.is_empty()));
}
