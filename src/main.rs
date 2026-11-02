use std::io::ErrorKind;
use std::path::PathBuf;
use std::process::ExitCode;

use gpui_kit::*;
use semacs::app::AppShell;
use semacs::buffer::Buffer;
use semacs::files::{self, ReadError};

fn main() -> ExitCode {
    let path = match parse_args() {
        Ok(path) => path,
        Err(message) => {
            eprintln!("semacs: {message}");
            return ExitCode::from(2);
        }
    };

    let buffer = match startup_buffer(path) {
        Ok(buffer) => buffer,
        Err(message) => {
            eprintln!("semacs: {message}");
            return ExitCode::FAILURE;
        }
    };

    let title = format!("{} - semacs", buffer.display_name());
    gpui_kit::application().run(move |cx| {
        semacs::app::init(cx);
        let options = WindowOptions {
            titlebar: Some(TitlebarOptions {
                title: Some(title.into()),
                ..Default::default()
            }),
            window_min_size: Some(size(px(480.0), px(320.0))),
            ..Default::default()
        };
        cx.spawn(async move |cx| {
            let path = semacs::scripting::config_path();
            let result = match &path {
                Ok(path) => semacs::scripting::load(path.clone()).await,
                Err(error) => Err(error.clone()),
            };
            cx.update(|cx| {
                gpui_kit::open_window(options, cx, move |window, cx| {
                    cx.new(|cx| AppShell::with_configuration(buffer, path.ok(), result, window, cx))
                })
                .expect("failed to open semacs window");
            });
        })
        .detach();
    });

    ExitCode::SUCCESS
}

fn parse_args() -> Result<Option<PathBuf>, String> {
    let mut args = std::env::args_os().skip(1);
    let Some(first) = args.next() else {
        return Ok(None);
    };
    if first == "-h" || first == "--help" {
        print_help();
        std::process::exit(0);
    }
    if args.next().is_some() {
        return Err("expected at most one file argument\n\nUsage: semacs [FILE]".to_owned());
    }
    Ok(Some(PathBuf::from(first)))
}

fn print_help() {
    println!("Usage: semacs [FILE]");
    println!();
    println!("Load FILE, or prepare an empty *scratch* document.");
    println!("Emacs editing, undo/redo, minibuffer commands, files and buffers are available.");
}

fn startup_buffer(path: Option<PathBuf>) -> Result<Buffer, String> {
    let Some(path) = path else {
        return Ok(Buffer::scratch());
    };
    let path = files::absolute(&path).map_err(|error| format!("{}: {error}", path.display()))?;

    match files::load(&path) {
        Ok(loaded) => Ok(Buffer::visiting(path, loaded.file, loaded.stamp)),
        Err(ReadError::Io(error)) if error.kind() == ErrorKind::NotFound => {
            Ok(Buffer::new_file(path))
        }
        Err(error) => Err(format!("{}: {error}", path.display())),
    }
}
