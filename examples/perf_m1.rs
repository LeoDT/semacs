//! Bounded M1 command-chain and unsettled-input CPU regression, not native acceptance.

use std::path::PathBuf;
use std::time::Instant;

use gpui_kit::{AppContext, Keystroke, TestAppContext, VisualTestContext, px, size};
use semacs::app::AppShell;
use semacs::buffer::Buffer;
use semacs::files::{self, DecodedFile, FileFormat};

#[path = "support/headless.rs"]
mod headless;
#[path = "support/samples.rs"]
mod samples;

use headless::{draw, ms, ready};

#[path = "support/m1_workload.rs"]
mod workload;

use workload::CHAIN;
fn press(cx: &mut VisualTestContext, key: Keystroke) -> (f64, f64) {
    let start = Instant::now();
    cx.update(|window, cx| {
        window.dispatch_keystroke(key, cx);
    });
    let action = ms(start);
    draw(cx, false);
    (action, ms(start))
}

fn check_budget(front: f64, ready: Option<f64>) {
    assert!(
        front <= 2.,
        "STOP: front {front:.3} ms exceeds 2 ms; do not rerun automatically"
    );
    if let Some(ready) = ready {
        assert!(
            ready <= 300.,
            "STOP: ready {ready:.3} ms exceeds 300 ms; do not rerun automatically"
        );
    }
}

fn measure(sample: &str, text: &str, run: usize) {
    let buffer = Buffer::new(
        None,
        DecodedFile {
            text: text.to_owned(),
            format: FileFormat::default(),
        },
    );
    let dispatcher = gpui_kit::TestDispatcher::new(0);
    let mut app = TestAppContext::build(dispatcher.clone(), Some("perf_m1"));
    app.update(semacs::app::init);
    let window = app.open_window(size(px(960.), px(640.)), |window, cx| {
        let shell = cx.new(|cx| AppShell::new(buffer, window, cx));
        gpui_kit::base::Root::new(shell, window, cx)
    });
    let editor = window
        .read_with(&app, |root, cx| {
            root.view()
                .clone()
                .downcast::<AppShell>()
                .unwrap()
                .read(cx)
                .editor()
                .clone()
        })
        .unwrap();
    let mut cx = VisualTestContext::from_window(window.into(), &app);
    draw(&mut cx, false);
    ready(&mut cx, &editor);

    for (index, &(key, prefix, point)) in CHAIN.iter().enumerate() {
        let expected = format!("{prefix}{text}");
        let key = Keystroke::parse(key).unwrap();
        let start = Instant::now();
        let (action_ms, front_ms) = press(&mut cx, key);
        ready(&mut cx, &editor);
        let ready_ms = ms(start);
        println!("{sample},{run},chain,{index},1,{action_ms:.3},{front_ms:.3},{ready_ms:.3}");
        check_budget(front_ms, Some(ready_ms));
        cx.read(|cx| {
            let view = editor.read(cx);
            assert_eq!(
                view.buffer().text().to_string(),
                expected,
                "chain step {index}"
            );
            assert_eq!(view.buffer().point().0, point, "chain step {index}");
            assert_eq!(view.wrap_index().unwrap().indexed_bytes(), expected.len());
        });
    }

    let insert = Keystroke::parse("x").unwrap();
    let delete = Keystroke::parse("backspace").unwrap();
    let mut action_max: f64 = 0.;
    let mut front_max: f64 = 0.;
    let mut final_start = Instant::now();
    // Do not drain background work between keys: this isolates overlapping foreground updates.
    for index in 0..1024 {
        let key = if workload::stream_key(index) == "x" {
            insert.clone()
        } else {
            delete.clone()
        };
        final_start = Instant::now();
        let (action_ms, front_ms) = press(&mut cx, key);
        action_max = action_max.max(action_ms);
        front_max = front_max.max(front_ms);
        if front_ms > 2. {
            println!("{sample},{run},unsettled-input,{index},1,{action_ms:.3},{front_ms:.3},");
            check_budget(front_ms, None);
        }
        cx.read(|cx| {
            let view = editor.read(cx);
            assert_eq!(
                view.wrap_index().unwrap().key().display,
                view.display_snapshot().id()
            );
            assert!(view.paint_layout().unwrap().visible_row_count() > 0);
            let pending = workload::pending_bytes(index + 1);
            assert_eq!(view.buffer().point().0, pending);
            assert_eq!(view.buffer().text().len(), text.len() + pending);
        });
    }
    ready(&mut cx, &editor);
    let final_ready = ms(final_start);
    println!(
        "{sample},{run},unsettled-input,0,1024,{action_max:.3},{front_max:.3},{final_ready:.3}"
    );
    check_budget(front_max, Some(final_ready));
    cx.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.buffer().text().to_string(), text);
        assert_eq!(view.buffer().point().0, 0);
        assert_eq!(view.wrap_index().unwrap().indexed_bytes(), text.len());
    });

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
        .expect("Usage: perf_m1 MIXED_SAMPLE");
    let mixed = files::read(&path).expect("read mixed sample").text;
    let runs =
        std::env::var("RUNS").map_or(3, |value| value.parse::<usize>().expect("positive RUNS"));
    assert!(runs > 0);
    println!(
        "# release={}; GPUI TestPlatform / TestTextSystem with Base Root; initial 960x640; monospace 16px/24px; soft wrap enabled",
        !cfg!(debug_assertions)
    );
    println!(
        "# chain ready is per command; unsettled-input ready is final-key recovery only, NOT per-key ready or long-duration native input"
    );
    println!("sample,run,phase,step,keys,action_cpu_ms,front_ms,ready_ms");
    for sample in samples::NAMES {
        let text = samples::build(sample, &mixed);
        for run in 1..=runs {
            measure(sample, &text, run);
        }
    }
    println!("# completed=true; native_sustained_input=unmeasured");
}
