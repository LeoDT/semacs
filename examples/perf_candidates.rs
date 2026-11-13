//! Headless candidate-update / navigation baselines, without new latency limits.

use std::path::PathBuf;
use std::time::Instant;

use gpui_kit::{Entity, Keystroke, TestAppContext, VisualTestContext, px, size};
use semacs::app::AppShell;
use semacs::buffer::Buffer;

fn key(visual: &mut VisualTestContext, key: &'static str) {
    visual.update(|window, cx| window.dispatch_keystroke(Keystroke::parse(key).unwrap(), cx));
}

fn settle(visual: &mut VisualTestContext, shell: &Entity<AppShell>) {
    for _ in 0..16 {
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        if visual.read(|cx| {
            let shell = shell.read(cx);
            shell
                .session()
                .and_then(|session| session.candidates())
                .is_some_and(|list| !list.loading)
                && shell
                    .minibuffer()
                    .unwrap()
                    .read(cx)
                    .editor()
                    .read(cx)
                    .layout_ready()
        }) {
            return;
        }
    }
    panic!("candidate layout did not settle");
}

fn measure(
    visual: &mut VisualTestContext,
    shell: &Entity<AppShell>,
    phase: &str,
    run: usize,
    stroke: &'static str,
) {
    let start = Instant::now();
    key(visual, stroke);
    let action = start.elapsed().as_secs_f64() * 1000.;
    settle(visual, shell);
    let ready = start.elapsed().as_secs_f64() * 1000.;
    let count = visual.read(|cx| {
        shell
            .read(cx)
            .session()
            .unwrap()
            .candidates()
            .unwrap()
            .items
            .len()
    });
    println!("{phase},{run},{action:.3},{ready:.3},{count}");
}

fn main() {
    let directory = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .expect("Usage: perf_candidates NEW_FIXTURE_DIRECTORY");
    std::fs::create_dir(&directory).expect("fixture directory must not exist");
    for index in 0..2000 {
        std::fs::write(directory.join(format!("entry-{index:04}.txt")), b"").unwrap();
    }
    let directory = std::fs::canonicalize(directory).unwrap();
    let dispatcher = gpui_kit::TestDispatcher::new(0);
    let mut app = TestAppContext::build(dispatcher.clone(), Some("perf_candidates"));
    app.update(semacs::app::init);
    let window = app.open_window(size(px(960.), px(640.)), |window, cx| {
        AppShell::new(Buffer::scratch(), window, cx)
    });
    let shell = window.entity(&app).unwrap();
    let mut visual = VisualTestContext::from_window(window.into(), &app);
    visual.update(|window, _| window.activate_window());
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    println!(
        "# release={}; TestPlatform / TestTextSystem; 960x640; 2000 directory entries; new candidate metrics have NO approved latency limits; no native fonts / GPU / IME",
        !cfg!(debug_assertions)
    );
    println!("phase,run,action_ms,ready_ms,candidates");
    for run in 1..=3 {
        key(&mut visual, "alt-x");
        visual.simulate_input("s");
        settle(&mut visual, &shell);
        measure(&mut visual, &shell, "command-filter", run, "b");
        measure(&mut visual, &shell, "command-next", run, "ctrl-n");
        key(&mut visual, "ctrl-g");
        key(&mut visual, "ctrl-x");
        key(&mut visual, "ctrl-f");
        key(&mut visual, "ctrl-a");
        key(&mut visual, "ctrl-k");
        visual.simulate_input(&format!("{}/entry", directory.display()));
        settle(&mut visual, &shell);
        measure(&mut visual, &shell, "file-filter", run, "-");
        assert_eq!(
            visual.read(|cx| shell
                .read(cx)
                .session()
                .unwrap()
                .candidates()
                .unwrap()
                .items
                .len()),
            2000
        );
        measure(&mut visual, &shell, "file-next", run, "ctrl-n");
        key(&mut visual, "ctrl-g");
    }
    visual.run_until_parked();
    visual.update(|_, cx| {
        cx.background_executor().forbid_parking();
        cx.quit();
    });
    visual.run_until_parked();
    drop(shell);
    drop(visual);
    drop(app);
    dispatcher.drain_tasks();
    dispatcher.scheduler().end_test();
    println!("# completed=true; original 2000-entry fixture preserved");
}
