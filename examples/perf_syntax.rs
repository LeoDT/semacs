//! Syntax baselines on TestPlatform; no native, GPU or new latency-budget claims.

use gpui_kit::{AppContext, Entity, Keystroke, TestAppContext, VisualTestContext, px, size};
use semacs::app::AppShell;
use semacs::buffer::Buffer;
use semacs::files::{DecodedFile, FileFormat};
use semacs::theme::ThemeSettings;
use semacs::view::BufferView;
use std::time::Instant;

#[path = "support/headless.rs"]
mod headless;
use headless::{draw, ms, ready};

fn settle(cx: &mut VisualTestContext, view: &Entity<BufferView>) {
    for _ in 0..8 {
        cx.run_until_parked();
        draw(cx, false);
        if cx.read(|cx| view.read(cx).layout_ready() && view.read(cx).syntax_ready()) {
            break;
        }
    }
    cx.read(|cx| {
        let view = view.read(cx);
        assert!(view.layout_ready());
        assert_eq!(view.syntax_snapshot(), Some(view.buffer().snapshot_id()));
        assert!(view.paint_layout().unwrap().visible_row_count() > 0);
    });
}

fn keys(cx: &mut VisualTestContext, keys: &[&'static str]) {
    cx.update(|window, cx| {
        for key in keys {
            window.dispatch_keystroke(Keystroke::parse(key).unwrap(), cx);
        }
    });
}

fn action(
    cx: &mut VisualTestContext,
    view: &Entity<BufferView>,
    label: &str,
    f: impl FnOnce(&mut VisualTestContext),
) {
    let start = Instant::now();
    f(cx);
    let sync = ms(start);
    draw(cx, false);
    let front = ms(start);
    settle(cx, view);
    println!("{label},{sync:.3},{front:.3},{:.3}", ms(start));
}

fn sample(shape: &str, bytes: usize) -> String {
    match shape {
        "rust" => {
            let unit = "// 中🙂\nconst ANSWER: usize = 42;\nfn greet(name: &str) -> usize { let text = r#\"hello\"#; name.len() + ANSWER }\n";
            unit.repeat(bytes.div_ceil(unit.len()))
        }
        "markdown" => {
            let unit = "# Heading 中🙂\n\n**bold** and [link](https://example.com).\n\n```html\n<script>const n = call(42);</script><style>.box { width: 12px; }</style>\n```\n\n";
            unit.repeat(bytes.div_ceil(unit.len()))
        }
        "html" => {
            let unit = "<div class=\"box\">中🙂</div><script>const n = call(42);</script><style>.box { width: 12px; }</style>\n";
            unit.repeat(bytes.div_ceil(unit.len()))
        }
        "tsx" => {
            let unit = "// 中🙂\nconst node: Element = <div title=\"hello\">{call(42)}</div>;\n";
            unit.repeat(bytes.div_ceil(unit.len()))
        }
        "long-string" => format!(
            "const TEXT: &str = r#\"{}\"#;\n",
            "abc 中🙂 ".repeat(bytes.div_ceil("abc 中🙂 ".len()))
        ),
        _ => unreachable!(),
    }
}

fn measure(shape: &str, bytes: usize, run: usize) {
    let text = sample(shape, bytes);
    let buffer = Buffer::new(
        Some(
            match shape {
                "markdown" => "baseline.md",
                "html" => "baseline.html",
                "tsx" => "baseline.tsx",
                _ => "baseline.rs",
            }
            .into(),
        ),
        DecodedFile {
            text: text.clone(),
            format: FileFormat::default(),
        },
    );
    let dispatcher = gpui_kit::TestDispatcher::new(0);
    let mut cx = TestAppContext::build(dispatcher.clone(), Some("perf_syntax"));
    cx.update(semacs::app::init);
    let start = Instant::now();
    let window = cx.open_window(size(px(960.), px(640.)), |window, cx| {
        let shell = cx.new(|cx| AppShell::new(buffer, window, cx));
        gpui_kit::base::Root::new(shell, window, cx)
    });
    let view = window
        .read_with(&cx, |root, cx| {
            root.view()
                .clone()
                .downcast::<AppShell>()
                .unwrap()
                .read(cx)
                .active_view()
                .clone()
        })
        .unwrap();
    let mut visual = VisualTestContext::from_window(window.into(), &cx);
    visual.update(|window, _| window.activate_window());
    draw(&mut visual, false);
    let first = ms(start);
    settle(&mut visual, &view);
    println!("sample,{shape},bytes,{},run,{run}", text.len());
    println!("action,sync_ms,front_ms,syntax_and_layout_ready_ms");
    println!("cold,0,{first:.3},{:.3}", ms(start));
    action(&mut visual, &view, "insert", |cx| keys(cx, &["X"]));
    action(&mut visual, &view, "undo", |cx| keys(cx, &["ctrl-/"]));
    action(&mut visual, &view, "redo", |cx| keys(cx, &["ctrl-?"]));
    visual.read(|cx| assert_eq!(view.read(cx).buffer().point(), semacs::text::ByteOffset(0)));
    action(&mut visual, &view, "burst32", |cx| keys(cx, &["x"; 32]));
    visual.read(|cx| {
        assert!(
            view.read(cx).buffer().text().to_string() == format!("{}X{}", "x".repeat(32), text),
            "burst text differs from the redo-at-edit-start fixture"
        );
    });
    let snapshot = visual.read(|cx| view.read(cx).buffer().snapshot_id());
    action(&mut visual, &view, "theme", |cx| {
        cx.update(|_, cx| {
            let mut theme = ThemeSettings::global(cx).clone();
            theme.select("dark").unwrap();
            theme.install(cx);
        })
    });
    assert_eq!(
        snapshot,
        visual.read(|cx| view.read(cx).buffer().snapshot_id())
    );
    let start = Instant::now();
    for _ in 0..100 {
        draw(&mut visual, true);
    }
    println!("forced100_mean,0,{:.3},0", ms(start) / 100.);
    ready(&mut visual, &view);
    visual.update(|_, cx| {
        cx.background_executor().forbid_parking();
        cx.quit();
    });
    visual.run_until_parked();
    drop(view);
    drop(visual);
    drop(cx);
    dispatcher.drain_tasks();
    dispatcher.scheduler().end_test();
}

#[test]
fn headless_workload_obeys_production_redo_point_and_final_text() {
    for shape in ["rust", "long-string", "tsx", "html", "markdown"] {
        measure(shape, 1024, 1);
    }
}

fn main() {
    let runs: usize = std::env::var("RUNS")
        .unwrap_or_else(|_| "3".into())
        .parse()
        .unwrap();
    assert!(runs > 0);
    let shapes =
        std::env::var("SHAPES").unwrap_or_else(|_| "rust,long-string,tsx,html,markdown".into());
    for shape in shapes.split(',') {
        let bytes = if matches!(shape, "markdown" | "html" | "tsx") {
            256 * 1024
        } else {
            8 * 1024 * 1024
        };
        for run in 1..=runs {
            measure(shape, bytes, run);
        }
    }
}
