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
