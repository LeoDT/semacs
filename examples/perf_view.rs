//! CPU-only GPUI TestPlatform measurements, not native-font or GPU timings.

use std::path::PathBuf;
use std::time::Instant;

use gpui_kit::{Entity, Keystroke, TestAppContext, VisualTestContext, font, point, px, size};
use semacs::app::AppShell;
use semacs::buffer::Buffer;
use semacs::files::{self, DecodedFile, FileFormat};
use semacs::view::BufferView;

#[path = "support/samples.rs"]
mod samples;

#[path = "support/headless.rs"]
mod headless;

use headless::{draw, ms, ready};

fn input_measurement(
    cx: &mut VisualTestContext,
    editor: &Entity<BufferView>,
    key: &'static str,
    expected: &str,
) -> (f64, f64) {
    let key = Keystroke::parse(key).unwrap();
    let start = Instant::now();
    cx.update(|window, cx| {
        window.dispatch_keystroke(key, cx);
    });
    draw(cx, false);
    let front_ms = ms(start);
    cx.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(
            view.wrap_index().unwrap().key().display,
            view.display_snapshot().id()
        );
        assert!(view.wrap_index().unwrap().scanned_bytes() <= 8192);
    });
    ready(cx, editor);
    let ready_ms = ms(start);
    cx.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.buffer().text().to_string(), expected);
        assert_eq!(view.wrap_index().unwrap().indexed_bytes(), expected.len());
        assert!(view.buffer().is_modified());
    });
    (front_ms, ready_ms)
}

fn measure(sample: &str, text: &str, run: usize) {
    let start = Instant::now();
    let buffer = Buffer::new(
        None,
        DecodedFile {
            text: text.to_owned(),
            format: FileFormat::default(),
        },
    );
    let model_ms = ms(start);
    let dispatcher = gpui_kit::TestDispatcher::new(0);
    let mut app = TestAppContext::build(dispatcher.clone(), Some("perf_view"));
    app.update(semacs::app::init);
    let start = Instant::now();
    let window = app.open_window(size(px(960.), px(640.)), |window, cx| {
        AppShell::new(buffer, window, cx)
    });
    let editor = window
        .read_with(&app, |shell, _| shell.editor().clone())
        .unwrap();
    let mut cx = VisualTestContext::from_window(window.into(), &app);
    draw(&mut cx, false);
    let first_ms = ms(start);
    let (first_rows, scan_bytes) = cx.read(|cx| {
        let view = editor.read(cx);
        (
            view.viewport_layout().unwrap().visible_row_count(),
            view.wrap_index().unwrap().scanned_bytes(),
        )
    });
    assert!(first_rows > 0, "first frame must contain body text");
    ready(&mut cx, &editor);
    let ready_ms = ms(start);
    let (rows, bounds) = cx.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.buffer().text().len(), text.len());
        assert_eq!(view.wrap_index().unwrap().indexed_bytes(), text.len());
        (
            view.wrap_index().unwrap().row_count(),
            view.viewport_layout().unwrap().bounds(),
        )
    });

    let start = Instant::now();
    cx.simulate_event(gpui_kit::ScrollEvent {
        position: point(bounds.left() + px(8.), bounds.top() + px(8.)),
        delta: gpui_kit::ScrollDelta::Pixels(point(px(0.), px(-240.))),
        modifiers: Default::default(),
        touch_phase: gpui_kit::TouchPhase::Moved,
        source: gpui_kit::ScrollSource::Pointer,
    });
    draw(&mut cx, false);
    ready(&mut cx, &editor);
    let scroll_ms = ms(start);
    cx.read(|cx| assert!(editor.read(cx).viewport_layout().unwrap().scroll().y > px(0.)));

    let start = Instant::now();
    cx.simulate_resize(size(px(720.), px(640.)));
    draw(&mut cx, false);
    ready(&mut cx, &editor);
    let resize_ms = ms(start);
    cx.read(|cx| {
        assert!(
            editor
                .read(cx)
                .wrap_index()
                .unwrap()
                .key()
                .settings
                .wrap_width
                < bounds.size.width
        )
    });

    let start = Instant::now();
    cx.update(|_, cx| {
        editor.update(cx, |view, cx| {
            view.set_font(font("monospace").bold(), px(18.), px(27.), cx)
        })
    });
    draw(&mut cx, false);
    ready(&mut cx, &editor);
    let font_ms = ms(start);
    let key = cx.read(|cx| editor.read(cx).wrap_index().unwrap().key().clone());
    let start = Instant::now();
    for _ in 0..30 {
        draw(&mut cx, true);
    }
    let forced_frame_ms = ms(start) / 30.;
    let start = Instant::now();
    for _ in 0..30 {
        draw(&mut cx, false);
    }
    let empty_draw_ms = ms(start) / 30.;
    cx.read(|cx| assert_eq!(editor.read(cx).wrap_index().unwrap().key(), &key));

    let click = cx.read(|cx| {
        let bounds = editor.read(cx).viewport_layout().unwrap().bounds();
        point(bounds.left() + px(1.), bounds.top() + px(4.))
    });
    cx.simulate_click(click, Default::default());
    draw(&mut cx, false);
    let point = cx.read(|cx| editor.read(cx).buffer().point().0);
    let mut expected = text.to_owned();
    expected.insert(point, 'x');
    let (insert_front, insert_ready) = input_measurement(&mut cx, &editor, "x", &expected);
    let (delete_front, delete_ready) = input_measurement(&mut cx, &editor, "backspace", text);
    expected = text.to_owned();
    expected.insert(point, '\n');
    let (newline_front, newline_ready) = input_measurement(&mut cx, &editor, "enter", &expected);
    let (join_front, join_ready) = input_measurement(&mut cx, &editor, "backspace", text);

    println!(
        "{sample},{run},{},{rows},{first_rows},{scan_bytes},{model_ms:.3},{first_ms:.3},{ready_ms:.3},{scroll_ms:.3},{resize_ms:.3},{font_ms:.3},{forced_frame_ms:.3},{empty_draw_ms:.3},{insert_front:.3},{insert_ready:.3},{delete_front:.3},{delete_ready:.3},{newline_front:.3},{newline_ready:.3},{join_front:.3},{join_ready:.3}",
        text.len()
    );
    cx.run_until_parked();
    cx.update(|_, cx| {
        cx.background_executor().forbid_parking();
        cx.quit();
    });
    cx.run_until_parked();
    drop(editor);
    drop(cx);
    drop(app);
    dispatcher.drain_tasks();
    dispatcher.scheduler().end_test();
}

fn main() {
    let path = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .expect("Usage: cargo run --release --example perf_view -- MIXED_SAMPLE");
    let mixed = files::read(&path)
        .expect("read deterministic mixed sample")
        .text;
    let runs = std::env::var("RUNS").map_or(3, |value| {
        value
            .parse::<usize>()
            .expect("RUNS must be a positive integer")
    });
    assert!(runs > 0);
    println!("# GPUI TestPlatform + TestTextSystem; no native fonts, display, IME or GPU");
    println!(
        "# Initial 960x640, resize 720x640, monospace 16px/24px -> bold 18px/27px; soft wrap enabled"
    );
    println!(
        "sample,run,bytes,wrap_rows,first_rows,preview_scan_bytes,model_ms,first_ms,ready_ms,scroll_ms,resize_ready_ms,font_ready_ms,forced_frame_ms,empty_draw_ms,insert_front_ms,insert_ready_ms,delete_front_ms,delete_ready_ms,newline_front_ms,newline_ready_ms,join_front_ms,join_ready_ms"
    );
    for sample in samples::NAMES {
        let text = samples::build(sample, &mixed);
        for run in 1..=runs {
            measure(sample, &text, run);
        }
    }
}
