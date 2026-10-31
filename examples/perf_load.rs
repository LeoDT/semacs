//! File-layer load and decode timing for the performance baseline.
//!
//! Usage: cargo run --release --example perf_load -- <file>

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use semacs::files;

fn main() -> ExitCode {
    let Some(path) = std::env::args_os().nth(1).map(PathBuf::from) else {
        eprintln!("usage: perf_load <file>");
        return ExitCode::from(2);
    };

    let read_start = Instant::now();
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("{}: {error}", path.display());
            return ExitCode::FAILURE;
        }
    };
    let read_time = read_start.elapsed();

    let decode_start = Instant::now();
    let decoded = match files::decode(&bytes) {
        Ok(decoded) => decoded,
        Err(error) => {
            eprintln!("{}: {error}", path.display());
            return ExitCode::FAILURE;
        }
    };
    let decode_time = decode_start.elapsed();

    let text = &decoded.text;
    let mut ascii = 0usize;
    let mut cjk = 0usize;
    let mut other = 0usize;
    for character in text.chars() {
        if character.is_ascii() {
            ascii += 1;
        } else if matches!(character as u32, 0x3400..=0x4DBF | 0x4E00..=0x9FFF) {
            cjk += 1;
        } else {
            other += 1;
        }
    }

    let mut lines = 0usize;
    let mut max_line_chars = 0usize;
    for line in text.split('\n') {
        lines += 1;
        max_line_chars = max_line_chars.max(line.chars().count());
    }

    println!(
        "file={} bytes={} format={} text_bytes={} lines={} max_line_chars={} chars_ascii={} chars_cjk={} chars_other={} read_ms={:.3} decode_ms={:.3}",
        path.display(),
        bytes.len(),
        decoded.format.as_str(),
        text.len(),
        lines,
        max_line_chars,
        ascii,
        cjk,
        other,
        read_time.as_secs_f64() * 1000.0,
        decode_time.as_secs_f64() * 1000.0,
    );
    ExitCode::SUCCESS
}
