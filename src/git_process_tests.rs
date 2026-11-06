use super::*;
use crate::diff::tests::Repository as Fixture;
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[test]
fn blocking_git_helper() {
    let Some(path) = std::env::var_os("SEMACS_GIT_HELPER_SOCKET") else {
        return;
    };
    let mut socket = UnixStream::connect(&path).unwrap();
    socket.write_all(b"R").unwrap();
    let mut child = if std::env::var_os("SEMACS_GIT_HELPER_CHILD").is_none() {
        Some(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "git::process_tests::blocking_git_helper",
                    "--nocapture",
                ])
                .env("SEMACS_GIT_HELPER_CHILD", "1")
                .spawn()
                .unwrap(),
        )
    } else {
        None
    };
    let _ = socket.read(&mut [0]);
    if let Some(child) = child.as_mut() {
        let _ = child.kill();
        let _ = child.wait();
    }
}

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[test]
fn cancellation_reclaims_git_descendants_and_their_inherited_pipes() {
    let repo = Fixture::new();
    let socket_path = repo.0.join("gate.sock");
    let listener = UnixListener::bind(&socket_path).unwrap();
    listener.set_nonblocking(true).unwrap();
    let cancel = Arc::new(AtomicBool::new(false));
    let worker_cancel = cancel.clone();
    let alias = format!(
        "alias.block=!env SEMACS_GIT_HELPER_SOCKET={} {} --exact git::process_tests::blocking_git_helper --nocapture",
        quote(socket_path.to_str().unwrap()),
        quote(std::env::current_exe().unwrap().to_str().unwrap())
    );
    let root = repo.0.clone();
    let (send, receive) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        send.send(read(
            &root,
            Some(&root.join(".git")),
            &["-c".as_ref(), alias.as_ref(), "block".as_ref()],
            &worker_cancel,
        ))
        .unwrap();
    });
    let mut sockets = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(5);
    while sockets.len() < 2 {
        match listener.accept() {
            Ok((mut socket, _)) => {
                socket
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                socket.read_exact(&mut [0]).unwrap();
                sockets.push(socket);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    cancel.store(true, Ordering::Relaxed);
                    panic!("Git helper did not reach its barrier");
                }
                std::thread::yield_now();
            }
            Err(error) => panic!("Git helper socket: {error}"),
        }
    }
    cancel.store(true, Ordering::Relaxed);
    assert!(
        receive
            .recv_timeout(Duration::from_secs(3))
            .unwrap()
            .unwrap_err()
            .contains("cancelled")
    );
    worker.join().unwrap();
    for mut socket in sockets {
        assert_eq!(socket.read(&mut [0]).unwrap(), 0);
    }
}

#[test]
fn oversized_input_fails_before_starting_git() {
    let input = vec![0; OUTPUT_LIMIT + 1];
    let error = read_input(
        Path::new("/nonexistent-semacs-fixture"),
        None,
        &["check-attr".as_ref()],
        Some(&input),
        &AtomicBool::new(false),
    )
    .unwrap_err();
    assert!(error.contains("input exceeds"));
}
