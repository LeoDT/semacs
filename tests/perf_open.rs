#![recursion_limit = "256"]

//! M0 application-level performance baseline.
//!
//! Run with a warm file cache and a release build:
//!
//!   SEMACS_PERF_SAMPLE=.pi/tmp/perf-sample.txt \
//!     cargo test --release --test perf_open -- --ignored --nocapture

use std::path::PathBuf;
use std::time::Instant;

use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AppContext as _, Point, ScrollDelta, TestAppContext, WindowOptions, px};
use semacs::app::{AppShell, StartupDocument};
use semacs::files;

#[gpui_kit::test]
#[ignore = "performance baseline, run explicitly in release mode"]
fn open_file_and_scroll(cx: &mut TestAppContext) {
    let path = PathBuf::from(
        std::env::var("SEMACS_PERF_SAMPLE").unwrap_or_else(|_| ".pi/tmp/perf-sample.txt".into()),
    );

    let load_start = Instant::now();
    let file = files::read(&path).expect("read perf sample");
    let text_len = file.text.len();
    let load_time = load_start.elapsed();

    cx.update(gpui_kit::init);
    let open_start = Instant::now();
    let (handle, app) = cx.update(|cx| {
        let open_path = path.clone();
        gpui_kit::open_window(WindowOptions::default(), cx, move |window, cx| {
            cx.new(|cx| AppShell::new(StartupDocument::visit(open_path, file), window, cx))
        })
        .expect("open window")
    });

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let first_frame_time = open_start.elapsed();

        let before = app.read(cx).buffer().read(cx).scroll_offset(cx);
        let scroll_start = Instant::now();
        const SCROLLS: usize = 10;
        for _ in 0..SCROLLS {
            // Each helper call renders once before lookup and once after
            // dispatch; report the whole call rather than calling it a frame.
            window.within("buffer-view").scroll(
                "input-state",
                ScrollDelta::Lines(Point { x: 0.0, y: -20.0 }),
                cx,
            );
        }
        let scroll_time = scroll_start.elapsed();

        let render_start = Instant::now();
        const RENDERS: usize = 10;
        for _ in 0..RENDERS {
            window.render_frame(cx);
        }
        let render_time = render_start.elapsed();

        let after = app.read(cx).buffer().read(cx).scroll_offset(cx);
        let loaded_len = app.read(cx).buffer().read(cx).len(cx);
        assert_eq!(before.y, px(0.0), "editor should start at the top");
        assert!(
            before.y - after.y >= px(1000.0),
            "scroll must move the editor substantially, before={:?} after={:?}",
            before.y,
            after.y
        );
        assert!(text_len > 1_000_000, "perf sample should be non-trivial");
        assert_eq!(loaded_len, text_len, "the editor must hold the whole sample");
        println!(
            "file={} file_and_decode_ms={:.3} window_and_first_frame_ms={:.3} scroll_calls={} scroll_call_avg_ms={:.3} idle_render_avg_ms={:.3} scroll_before_y={:.1} scroll_after_y={:.1}",
            path.display(),
            load_time.as_secs_f64() * 1000.0,
            first_frame_time.as_secs_f64() * 1000.0,
            SCROLLS,
            scroll_time.as_secs_f64() * 1000.0 / SCROLLS as f64,
            render_time.as_secs_f64() * 1000.0 / RENDERS as f64,
            before.y.as_f32(),
            after.y.as_f32(),
        );
    })
    .expect("window");
}
