//! Bounded, shell-free Git reads. Never invokes diff drivers or text converters.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

const OUTPUT_LIMIT: usize = 16 * 1024 * 1024;
const ERROR_LIMIT: usize = 64 * 1024;
const TIMEOUT: Duration = Duration::from_secs(15);

fn collect(mut stream: impl Read, limit: usize, exceeded: &AtomicBool) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    stream
        .by_ref()
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > limit {
        exceeded.store(true, Ordering::Relaxed);
        return Err("Git output exceeds its resource limit".into());
    }
    Ok(bytes)
}

pub(crate) fn read(
    directory: &Path,
    git_dir: Option<&Path>,
    args: &[&std::ffi::OsStr],
    cancelled: &AtomicBool,
) -> Result<Vec<u8>, String> {
    if cancelled.load(Ordering::Relaxed) {
        return Err("Git request cancelled".into());
    }
    let mut command = Command::new("git");
    command
        .args([
            "--no-optional-locks",
            "--literal-pathspecs",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.untrackedCache=false",
            "-c",
            "diff.suppressBlankEmpty=false",
            "-c",
            "core.quotePath=true",
            "-C",
        ])
        .arg(directory);
    if let Some(git_dir) = git_dir {
        command
            .arg("--git-dir")
            .arg(git_dir)
            .arg("--work-tree")
            .arg(directory);
    }
    let mut child = command
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Cannot start Git: {error}"))?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let exceeded = Arc::new(AtomicBool::new(false));
    std::thread::scope(|scope| {
        let output = scope.spawn(|| collect(stdout, OUTPUT_LIMIT, &exceeded));
        let diagnostic = scope.spawn(|| collect(stderr, ERROR_LIMIT, &exceeded));
        let start = Instant::now();
        let status = loop {
            if cancelled.load(Ordering::Relaxed)
                || exceeded.load(Ordering::Relaxed)
                || start.elapsed() >= TIMEOUT
            {
                let _ = child.kill();
                let _ = child.wait();
                break Err(if cancelled.load(Ordering::Relaxed) {
                    "Git request cancelled"
                } else if exceeded.load(Ordering::Relaxed) {
                    "Git output exceeds its resource limit"
                } else {
                    "Git request exceeded its 15 second limit"
                }
                .to_owned());
            }
            match child.try_wait() {
                Ok(Some(status)) => break Ok(status),
                Ok(None) => std::thread::sleep(Duration::from_millis(10)),
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break Err(error.to_string());
                }
            }
        };
        let bytes = output.join().map_err(|_| "Git output reader stopped")??;
        let diagnostic = diagnostic
            .join()
            .map_err(|_| "Git diagnostic reader stopped")??;
        let status = status?;
        if !status.success() {
            return Err(format!(
                "Git failed ({status}): {}",
                String::from_utf8_lossy(&diagnostic).trim()
            ));
        }
        Ok(bytes)
    })
}

pub(crate) fn discover(
    directory: &Path,
    cancelled: &AtomicBool,
) -> Result<(PathBuf, PathBuf), String> {
    let root = output_path(read(
        directory,
        None,
        &["rev-parse".as_ref(), "--show-toplevel".as_ref()],
        cancelled,
    )?)?;
    let git_dir = output_path(read(
        directory,
        None,
        &["rev-parse".as_ref(), "--absolute-git-dir".as_ref()],
        cancelled,
    )?)?;
    Ok((root, git_dir))
}

fn output_path(mut bytes: Vec<u8>) -> Result<PathBuf, String> {
    if bytes.last() == Some(&b'\n') {
        bytes.pop();
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        Ok(std::ffi::OsString::from_vec(bytes).into())
    }
    #[cfg(not(unix))]
    {
        Ok(String::from_utf8(bytes)
            .map_err(|_| "Repository path is not UTF-8")?
            .into())
    }
}

pub(crate) fn blob(
    root: &Path,
    git_dir: &Path,
    oid: &str,
    cancelled: &AtomicBool,
) -> Result<String, String> {
    if !valid_oid(oid) || oid.bytes().all(|byte| byte == b'0') {
        return Err("No file version exists on this side".into());
    }
    let bytes = read(
        root,
        Some(git_dir),
        &["cat-file".as_ref(), "blob".as_ref(), oid.as_ref()],
        cancelled,
    )?;
    if bytes.contains(&0) {
        return Err("Binary file versions are not supported".into());
    }
    String::from_utf8(bytes).map_err(|_| "File version is not UTF-8".into())
}

pub(crate) fn valid_oid(oid: &str) -> bool {
    matches!(oid.len(), 40 | 64) && oid.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stream_limits_fail_without_retaining_unbounded_output() {
        let exceeded = AtomicBool::new(false);
        assert_eq!(
            collect(std::io::Cursor::new(b"four"), 4, &exceeded).unwrap(),
            b"four"
        );
        assert!(!exceeded.load(Ordering::Relaxed));
        assert!(collect(std::io::Cursor::new(b"too much data"), 4, &exceeded).is_err());
        assert!(exceeded.load(Ordering::Relaxed));
    }
}
