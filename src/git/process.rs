use std::ffi::OsStr;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

pub(super) const OUTPUT_LIMIT: usize = 16 * 1024 * 1024;
pub(super) const ERROR_LIMIT: usize = 64 * 1024;
const TIMEOUT: Duration = Duration::from_secs(15);

pub(crate) enum Access<'a> {
    Read(&'a AtomicBool),
    IndexWrite {
        hooks: &'a Path,
        started: &'a mut dyn FnMut(),
    },
}

pub(crate) struct Output {
    pub status: ExitStatus,
    pub bytes: Vec<u8>,
    pub diagnostic: String,
}

pub(crate) struct Failure {
    pub started: bool,
    pub diagnostic: String,
}

pub(crate) fn write_environment() -> Result<(), String> {
    for name in [
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_QUARANTINE_PATH",
    ] {
        if std::env::var_os(name).is_some() {
            return Err(format!("Index write refused: environment override {name}"));
        }
    }
    Ok(())
}

pub(super) fn collect(
    mut stream: impl Read,
    limit: usize,
    exceeded: &AtomicBool,
) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    stream
        .by_ref()
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > limit {
        exceeded.store(true, Ordering::Relaxed);
        return Err("Git output exceeds its resource limit [truncated]".into());
    }
    Ok(bytes)
}

pub(crate) fn execute(
    directory: &Path,
    git_dir: Option<&Path>,
    args: &[&OsStr],
    input: Option<&[u8]>,
    access: Access<'_>,
) -> Result<Output, Failure> {
    let not_started = |diagnostic| Failure {
        started: false,
        diagnostic,
    };
    if input.is_some_and(|bytes| bytes.len() > OUTPUT_LIMIT) {
        return Err(not_started(
            "Git input exceeds its 16 MiB resource limit".into(),
        ));
    }
    let mut command = Command::new("git");
    for name in [
        "GIT_CONFIG",
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_QUARANTINE_PATH",
        "GIT_NAMESPACE",
        "GIT_PREFIX",
        "GIT_SHALLOW_FILE",
        "GIT_GRAFT_FILE",
        "GIT_IMPLICIT_WORK_TREE",
    ] {
        command.env_remove(name);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    command.args([
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
    ]);
    let (cancelled, started) = match access {
        Access::Read(cancelled) => (Some(cancelled), None),
        Access::IndexWrite { hooks, started } => {
            write_environment().map_err(not_started)?;
            command.arg("-c").arg({
                let mut setting = std::ffi::OsString::from("core.hooksPath=");
                setting.push(hooks);
                setting
            });
            (None, Some(started))
        }
    };
    if cancelled.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
        return Err(not_started("Git request cancelled".into()));
    }
    command.arg("-C").arg(directory);
    if let Some(git_dir) = git_dir {
        command
            .arg("--git-dir")
            .arg(git_dir)
            .arg("--work-tree")
            .arg(directory);
    }
    command
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("LC_ALL", "C");
    run(
        command,
        input,
        cancelled,
        TIMEOUT,
        OUTPUT_LIMIT,
        ERROR_LIMIT,
        started,
    )
}

fn run(
    mut command: Command,
    input: Option<&[u8]>,
    cancelled: Option<&AtomicBool>,
    timeout: Duration,
    output_limit: usize,
    error_limit: usize,
    started: Option<&mut dyn FnMut()>,
) -> Result<Output, Failure> {
    let mut child = command
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| Failure {
            started: false,
            diagnostic: format!("Cannot start Git: {error}"),
        })?;
    if let Some(started) = started {
        started();
    }
    let stdin = child.stdin.take();
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let exceeded = Arc::new(AtomicBool::new(false));
    let result = std::thread::scope(|scope| -> Result<Output, String> {
        let writer = scope.spawn(move || -> Result<(), String> {
            if let (Some(mut stdin), Some(input)) = (stdin, input) {
                stdin
                    .write_all(input)
                    .map_err(|error| format!("Git input failed: {error}"))?;
            }
            Ok(())
        });
        let output = scope.spawn(|| collect(stdout, output_limit, &exceeded));
        let diagnostic = scope.spawn(|| collect(stderr, error_limit, &exceeded));
        let start = Instant::now();
        let status = loop {
            if cancelled.is_some_and(|flag| flag.load(Ordering::Relaxed))
                || exceeded.load(Ordering::Relaxed)
                || start.elapsed() >= timeout
            {
                break Err(
                    if cancelled.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
                        "Git request cancelled"
                    } else if exceeded.load(Ordering::Relaxed) {
                        "Git output exceeds its resource limit [truncated]"
                    } else {
                        "Git request exceeded its time limit"
                    }
                    .to_owned(),
                );
            }
            match child.try_wait() {
                Ok(Some(status)) => break Ok(status),
                Ok(None) => std::thread::sleep(Duration::from_millis(10)),
                Err(error) => break Err(error.to_string()),
            }
        };
        stop(&mut child);
        let written = writer.join().map_err(|_| "Git input writer stopped")?;
        let bytes = output.join().map_err(|_| "Git output reader stopped")??;
        let diagnostic = diagnostic
            .join()
            .map_err(|_| "Git diagnostic reader stopped")??;
        let status = status?;
        written?;
        Ok(Output {
            status,
            bytes,
            diagnostic: String::from_utf8_lossy(&diagnostic).trim().to_owned(),
        })
    });
    result.map_err(|diagnostic| Failure {
        started: true,
        diagnostic,
    })
}

fn stop(child: &mut std::process::Child) {
    #[cfg(unix)]
    // Descendants may still hold pipes after Git itself exits.
    unsafe {
        libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
    }
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outcomes_distinguish_spawn_exit_signal_timeout_and_output_loss() {
        let missing = Command::new("/nonexistent-semacs-test-command");
        assert!(
            !run(
                missing,
                None,
                None,
                TIMEOUT,
                OUTPUT_LIMIT,
                ERROR_LIMIT,
                None
            )
            .err()
            .unwrap()
            .started
        );
        for (arguments, success) in [(["--version"], true), (["--invalid-semacs-option"], false)] {
            let mut command = Command::new("git");
            command.arg(arguments[0]);
            let output = run(
                command,
                None,
                None,
                TIMEOUT,
                OUTPUT_LIMIT,
                ERROR_LIMIT,
                None,
            )
            .ok()
            .unwrap();
            assert_eq!(output.status.success(), success);
        }
        let mut command = Command::new("git");
        command.arg("--version");
        let failure = run(command, None, None, TIMEOUT, 1, ERROR_LIMIT, None)
            .err()
            .unwrap();
        assert!(failure.started);
        assert!(failure.diagnostic.contains("truncated"));
    }

    #[cfg(unix)]
    #[test]
    fn timeout_and_signal_do_not_claim_a_known_write_failure() {
        use std::os::unix::process::CommandExt;
        for (signal, timeout) in [(false, Duration::ZERO), (true, TIMEOUT)] {
            let mut command = Command::new(std::env::current_exe().unwrap());
            command
                .args([
                    "--exact",
                    "git::process::tests::process_barrier",
                    "--nocapture",
                ])
                .env("SEMACS_PROCESS_BARRIER", "1")
                .process_group(0);
            if signal {
                command.env("SEMACS_PROCESS_SIGNAL", "1");
            }
            let result = run(
                command,
                None,
                None,
                timeout,
                OUTPUT_LIMIT,
                ERROR_LIMIT,
                None,
            );
            if signal {
                let output = result.ok().unwrap();
                assert!(output.status.code().is_none());
            } else {
                assert!(result.err().unwrap().started);
            }
        }
    }

    #[test]
    fn process_barrier() {
        if std::env::var_os("SEMACS_PROCESS_BARRIER").is_none() {
            return;
        }
        #[cfg(unix)]
        if std::env::var_os("SEMACS_PROCESS_SIGNAL").is_some() {
            unsafe {
                libc::raise(libc::SIGKILL);
            }
        }
        loop {
            std::thread::park();
        }
    }
}
