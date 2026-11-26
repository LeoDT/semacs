use super::*;

#[test]
fn descriptors_are_close_on_exec_and_the_master_is_nonblocking() {
    let terminal = Terminal::open().unwrap();
    for file in [&terminal.master, &terminal.slave] {
        let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFD) };
        assert!(flags >= 0);
        assert_ne!(flags & libc::FD_CLOEXEC, 0);
    }
    let flags = unsafe { libc::fcntl(terminal.master.as_raw_fd(), libc::F_GETFL) };
    assert!(flags >= 0);
    assert_ne!(flags & libc::O_NONBLOCK, 0);
}

#[test]
fn spawn_sets_gpg_tty_to_child_controlling_terminal() {
    use std::io::Read;

    let mut terminal = Terminal::open().unwrap();
    let expected = terminal.slave_path.clone().unwrap();
    let mut cmd = Command::new("sh");
    cmd.args(["-c", "printf '%s\\n' \"$GPG_TTY\""]);
    let mut child = terminal.spawn(&mut cmd).unwrap();
    let status = child.wait().unwrap();
    assert!(status.success());

    let mut output = Vec::new();
    let mut buf = [0u8; 1024];
    loop {
        match terminal.master.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => output.extend_from_slice(&buf[..n]),
            Err(err)
                if err.kind() == io::ErrorKind::WouldBlock
                    || err.raw_os_error() == Some(libc::EIO) =>
            {
                break;
            }
            Err(err) => panic!("unexpected master read error: {err}"),
        }
    }
    let actual = String::from_utf8(output).unwrap();
    assert_eq!(actual.trim(), expected.to_str().unwrap());
}
