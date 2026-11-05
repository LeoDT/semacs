//! Multi-view CPU baselines and production-input assertions, not native acceptance.

use std::path::PathBuf;
use std::time::Instant;

use gpui_kit::{Entity, Keystroke, TestAppContext, VisualTestContext, px, size};
use semacs::app::AppShell;
use semacs::buffer::Buffer;
use semacs::files::{self, DecodedFile, FileFormat};
use semacs::view::BufferView;

#[path = "support/headless.rs"]
mod headless;
#[path = "support/samples.rs"]
mod samples;

use headless::{draw, ms, ready};

fn keys(cx: &mut VisualTestContext, sequence: &[&'static str]) {
    cx.update(|window, cx| {
        for key in sequence {
            window.dispatch_keystroke(Keystroke::parse(key).unwrap(), cx);
        }
    });
}

fn settle(cx: &mut VisualTestContext, views: &[Entity<BufferView>]) {
    for view in views {
        ready(cx, view);
    }
    cx.read(|cx| {
        for view in views {
            let view = view.read(cx);
            assert!(view.layout_ready());
            assert!(view.viewport_layout().unwrap().visible_row_count() > 0);
            assert_eq!(
                view.wrap_index().unwrap().key().display,
                view.display_snapshot().id()
            );
        }
    });
}

fn measurement(
    cx: &mut VisualTestContext,
    views: &[Entity<BufferView>],
    label: &str,
    action: impl FnOnce(&mut VisualTestContext),
) {
    let start = Instant::now();
    action(cx);
    let action_ms = ms(start);
    draw(cx, false);
    let front_ms = ms(start);
    cx.read(|cx| {
        for view in views {
            assert!(
                view.read(cx)
                    .paint_layout()
                    .is_some_and(|layout| layout.visible_row_count() > 0),
                "blank body after {label}"
            );
        }
    });
    settle(cx, views);
    let all_ready_ms = ms(start);
    println!("{label},{action_ms:.3},{front_ms:.3},{all_ready_ms:.3}");
}

fn assert_text(cx: &VisualTestContext, views: &[Entity<BufferView>], expected: &str) {
    cx.read(|cx| {
        let snapshot = views[0].read(cx).buffer().snapshot_id();
        for view in views {
            let view = view.read(cx);
            assert_eq!(view.buffer().snapshot_id(), snapshot);
            assert_eq!(view.buffer().text().to_string(), expected);
            assert_eq!(view.wrap_index().unwrap().indexed_bytes(), expected.len());
        }
    });
}

fn measure(sample: &str, text: &str, run: usize, direction: &str) {
    let buffer = Buffer::new(
        None,
        DecodedFile {
            text: text.to_owned(),
            format: FileFormat::default(),
        },
    );
    let dispatcher = gpui_kit::TestDispatcher::new(0);
    let mut app = TestAppContext::build(dispatcher.clone(), Some("perf_split"));
    app.update(semacs::app::init);
    let window = app.open_window(size(px(960.), px(640.)), |window, cx| {
        AppShell::new(buffer, window, cx)
    });
    let shell = window.entity(&app).unwrap();
    let first = shell.read_with(&app, |shell, _| shell.active_view().clone());
    let mut cx = VisualTestContext::from_window(window.into(), &app);
    draw(&mut cx, false);
    ready(&mut cx, &first);

    let start = Instant::now();
    keys(
        &mut cx,
        &["ctrl-x", if direction == "right" { "3" } else { "2" }],
    );
    draw(&mut cx, false);
    keys(&mut cx, &["ctrl-x", "o"]);
    let second = cx.read(|cx| shell.read(cx).active_view().clone());
    assert_ne!(first, second, "split must create another view");
    let views = [first.clone(), second.clone()];
    settle(&mut cx, &views);
    let split_ready_ms = ms(start);
    println!("{sample},{run},{direction},split-and-select,,,{split_ready_ms:.3}");

    keys(&mut cx, &["ctrl-v"]);
    settle(&mut cx, &views);
    let (peer_point, peer_scroll) = cx.read(|cx| {
        let view = second.read(cx);
        (
            view.buffer().point(),
            view.viewport_layout().unwrap().scroll(),
        )
    });
    keys(&mut cx, &["ctrl-x", "o"]);
    settle(&mut cx, &views);
    let label = |phase: &str| format!("{sample},{run},{direction},{phase}");
    measurement(&mut cx, &views, &label("insert"), |cx| keys(cx, &["x"]));
    assert_text(&cx, &views, &format!("x{text}"));
    cx.read(|cx| {
        let peer = second.read(cx);
        assert_eq!(peer.buffer().point().0, peer_point.0 + 1);
        assert_eq!(peer.viewport_layout().unwrap().scroll(), peer_scroll);
    });
    measurement(&mut cx, &views, &label("delete"), |cx| {
        keys(cx, &["backspace"])
    });
    assert_text(&cx, &views, text);
    measurement(&mut cx, &views, &label("newline"), |cx| {
        keys(cx, &["enter"])
    });
    assert_text(&cx, &views, &format!("\n{text}"));
    measurement(&mut cx, &views, &label("join"), |cx| {
        keys(cx, &["backspace"])
    });
    assert_text(&cx, &views, text);

    keys(&mut cx, &["x", "ctrl-x", "o"]);
    settle(&mut cx, &views);
    measurement(&mut cx, &views, &label("undo-other-view"), |cx| {
        keys(cx, &["ctrl-/"])
    });
    assert_text(&cx, &views, text);
    measurement(&mut cx, &views, &label("redo-other-view"), |cx| {
        keys(cx, &["ctrl-?"])
    });
    assert_text(&cx, &views, &format!("x{text}"));

    measurement(&mut cx, &views, &label("burst-32"), |cx| {
        cx.update(|window, cx| {
            for _ in 0..32 {
                window.dispatch_keystroke(Keystroke::parse("z").unwrap(), cx);
            }
        });
    });
    assert_text(&cx, &views, &format!("{}x{text}", "z".repeat(32)));
    measurement(&mut cx, &views, &label("resize"), |cx| {
        cx.simulate_resize(size(px(720.), px(480.)));
    });
    measurement(&mut cx, &views, &label("font"), |cx| {
        cx.update(|_, cx| {
            for view in &views {
                view.update(cx, |view, cx| {
                    view.set_font(
                        semacs::appearance::FontSettings::default().font().bold(),
                        px(18.),
                        px(27.),
                        cx,
                    );
                });
            }
        });
    });
    let start = Instant::now();
    for _ in 0..30 {
        draw(&mut cx, true);
    }
    let forced_ms = ms(start) / 30.;
    let start = Instant::now();
    for _ in 0..30 {
        draw(&mut cx, false);
    }
    let empty_ms = ms(start) / 30.;
    println!("{sample},{run},{direction},forced-frame,,{forced_ms:.3},");
    println!("{sample},{run},{direction},empty-draw,,{empty_ms:.3},");

    cx.run_until_parked();
    cx.update(|_, cx| {
        cx.background_executor().forbid_parking();
        cx.quit();
    });
    cx.run_until_parked();
    drop(views);
    drop(first);
    drop(second);
    drop(shell);
    drop(cx);
    drop(app);
    dispatcher.drain_tasks();
    dispatcher.scheduler().end_test();
}

fn main() {
    let path = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .expect("Usage: perf_split MIXED_SAMPLE");
    let mixed = files::read(&path).expect("read mixed sample").text;
    let runs =
        std::env::var("RUNS").map_or(3, |value| value.parse::<usize>().expect("positive RUNS"));
    assert!(runs > 0);
    println!(
        "# release={}; TestPlatform / TestTextSystem; two shared-buffer views; 960x640 -> 720x480; soft wrap enabled",
        !cfg!(debug_assertions)
    );
    println!(
        "# all_ready includes both views; new multi-view aggregate baselines, no new numeric thresholds; not GPU/native/IME acceptance"
    );
    println!("sample,run,direction,phase,action_cpu_ms,front_ms,all_ready_ms");
    for sample in samples::NAMES {
        let text = samples::build(sample, &mixed);
        for direction in ["right", "below"] {
            for run in 1..=runs {
                measure(sample, &text, run, direction);
            }
        }
    }
}
