//! Opt-in native input diagnostics over the production AppShell.
//! Usage: cargo run --example native_input -- INPUT_FILE NEW_STATE_LOG
//! State contains the fixture's text; use disposable inputs and explicit consent.

use std::cell::RefCell;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Instant;

use gpui_kit::*;
use semacs::app::AppShell;
use semacs::buffer::Buffer;
use semacs::files;
use semacs::view::BufferView;

fn hex(text: &str) -> String {
    use std::fmt::Write;
    let mut encoded = String::with_capacity(text.len() * 2);
    for byte in text.bytes() {
        write!(&mut encoded, "{byte:02x}").unwrap();
    }
    encoded
}

fn record(view: &BufferView, start: Instant, log: &mut File) {
    let display = view.display_snapshot();
    let selection = display.selected_range();
    writeln!(
        log,
        "{:.3}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
        start.elapsed().as_secs_f64() * 1000.,
        view.buffer().point().0,
        view.buffer().is_modified(),
        display.marked_range().is_some(),
        selection.start.0,
        selection.end.0,
        view.layout_ready(),
        hex(&view.buffer().text().to_string()),
        hex(&display.text().to_string()),
        hex(view.message().unwrap_or("")),
    )
    .expect("write native input state");
}

fn main() {
    let mut args = std::env::args_os().skip(1);
    let input = PathBuf::from(args.next().expect("expected INPUT_FILE"));
    let output = PathBuf::from(args.next().expect("expected NEW_STATE_LOG"));
    assert!(args.next().is_none(), "expected exactly two arguments");
    let buffer = Buffer::new(
        Some(input.clone()),
        files::read(&input).expect("read fixture"),
    );
    let mut log = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .expect("create a new state log; refusing to overwrite");
    writeln!(log, "elapsed_ms\tpoint\tmodified\tmarked\tselected_start\tselected_end\tready\tcommitted_hex\tdisplay_hex\tmessage_hex")
        .expect("write header");
    let log = Rc::new(RefCell::new(log));
    let start = Instant::now();

    gpui_kit::application().run(move |cx| {
        semacs::app::init(cx);
        let (window, shell) = gpui_kit::open_window(
            WindowOptions {
                titlebar: Some(TitlebarOptions {
                    title: Some("semacs native input probe".into()),
                    ..Default::default()
                }),
                window_min_size: Some(size(px(480.), px(320.))),
                ..Default::default()
            },
            cx,
            move |window, cx| cx.new(|cx| AppShell::new(buffer, window, cx)),
        )
        .expect("open native probe");
        window
            .update(cx, |_, window, _| {
                eprintln!("Native probe graphics: {:?}", window.gpu_specs());
            })
            .expect("inspect native graphics");
        let editor = shell.read(cx).active_view().clone();
        record(editor.read(cx), start, &mut log.borrow_mut());
        cx.observe(&editor, move |editor, cx| {
            record(editor.read(cx), start, &mut log.borrow_mut());
        })
        .detach();
    });
}
