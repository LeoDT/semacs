//! Production-shell diff CPU baselines on TestPlatform / TestTextSystem.
use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui_kit::{TestAppContext, VisualTestContext, px, size};
use semacs::app::AppShell;

#[path = "support/headless.rs"]
mod headless;
#[path = "support/diff_workload.rs"]
mod workload;

fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    assert_eq!(args.len(), 2, "perf_diff FIXTURE NEW_REPORT");
    let fixture = PathBuf::from(&args[0]).canonicalize().unwrap();
    let report = PathBuf::from(&args[1]);
    if run(fixture, report).is_err() {
        std::process::exit(2);
    }
}

fn run(fixture: PathBuf, report: PathBuf) -> Result<(), String> {
    let dispatcher = gpui_kit::TestDispatcher::new(0);
    let mut app = TestAppContext::build(dispatcher.clone(), Some("perf_diff"));
    app.background_executor.allow_parking();
    app.update(semacs::app::init);
    let buffer = workload::buffer(&fixture);
    let window = app.open_window(size(px(960.), px(640.)), |window, cx| {
        AppShell::new(buffer, window, cx)
    });
    let shell = window.entity(&app).unwrap();
    let mut cx = VisualTestContext::from_window(window.into(), &app);
    headless::draw(&mut cx, false);
    let mut work = cx.read(|cx| workload::Workload::new(shell, fixture, &report, "headless", cx));
    work.context("platform=TestPlatform; fonts=TestTextSystem; front observed after draw; no native idle observation");
    headless::ready(&mut cx, &work.views[0]);
    let result = (|| -> Result<(), String> {
        while !work.done() {
            if workload::ACTIONS[work.step % workload::ACTIONS.len()] == "idle" {
                work.step += 1;
                continue;
            }
            cx.update(|window, cx| work.prepare(window, cx));
            headless::draw(&mut cx, false);
            let mut reading = cx.update(|window, cx| work.begin(window, cx));
            loop {
                cx.run_until_parked();
                let start = Instant::now();
                headless::draw(&mut cx, false);
                let draw_ms = headless::ms(start);
                let scene = cx.read(|cx| workload::scene(&work.shell, &work.views, cx));
                reading.accepts(scene.at, draw_ms, Some(scene));
                if let Some(error) = reading.error(false) {
                    let trace_error = work.record(&reading, false, &[]).err();
                    return Err(format!(
                        "{}/{}: {error}; trace_error={trace_error:?}",
                        reading.kind, reading.phase
                    ));
                }
                if reading.complete() {
                    let result = cx.read(|cx| work.verify(&reading, cx));
                    work.record(&reading, result.is_ok(), &[])?;
                    result?;
                    break;
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            work.step += 1;
        }
        Ok(())
    })();
    work.finish(result.as_ref().err().map(String::as_str));
    cx.run_until_parked();
    cx.update(|_, cx| {
        cx.background_executor().forbid_parking();
        cx.quit();
    });
    cx.run_until_parked();
    drop(work);
    drop(cx);
    drop(app);
    dispatcher.drain_tasks();
    dispatcher.scheduler().end_test();
    result
}

#[cfg(test)]
#[path = "support/diff_headless_tests.rs"]
mod tests;
