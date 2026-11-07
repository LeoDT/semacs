use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};

#[cfg(test)]
#[path = "terminal_tests.rs"]
mod tests;

pub(super) struct Terminal {
    pub master: File,
    slave: File,
}

impl Terminal {
    pub fn open() -> io::Result<Self> {
        let (master, slave) = open_pair()?;
        let mut termios = std::mem::MaybeUninit::uninit();
        if unsafe { libc::tcgetattr(slave.as_raw_fd(), termios.as_mut_ptr()) } < 0 {
            return Err(io::Error::last_os_error());
        }
        let mut termios = unsafe { termios.assume_init() };
        termios.c_lflag &= !(libc::ECHO | libc::ECHONL);
        if unsafe { libc::tcsetattr(slave.as_raw_fd(), libc::TCSANOW, &termios) } < 0 {
            return Err(io::Error::last_os_error());
        }
        let flags = unsafe { libc::fcntl(master.as_raw_fd(), libc::F_GETFL) };
        if flags < 0
            || unsafe { libc::fcntl(master.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) }
                < 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            master: master.into(),
            slave: slave.into(),
        })
    }

    pub fn spawn(&self, command: &mut Command) -> io::Result<Child> {
        command
            .stdin(Stdio::from(self.slave.try_clone()?))
            .stdout(Stdio::from(self.slave.try_clone()?))
            .stderr(Stdio::from(self.slave.try_clone()?));
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() < 0 || libc::ioctl(0, libc::TIOCSCTTY, 0) < 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        command.spawn()
    }
}

#[cfg(target_os = "linux")]
fn open_pair() -> io::Result<(OwnedFd, OwnedFd)> {
    let flags = libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC;
    let master = unsafe { libc::posix_openpt(flags) };
    if master < 0 {
        return Err(io::Error::last_os_error());
    }
    let master = unsafe { OwnedFd::from_raw_fd(master) };
    if unsafe { libc::grantpt(master.as_raw_fd()) } < 0
        || unsafe { libc::unlockpt(master.as_raw_fd()) } < 0
    {
        return Err(io::Error::last_os_error());
    }
    let mut name = [0; libc::PATH_MAX as usize];
    let error = unsafe { libc::ptsname_r(master.as_raw_fd(), name.as_mut_ptr(), name.len()) };
    if error != 0 {
        return Err(if error < 0 {
            io::Error::last_os_error()
        } else {
            io::Error::from_raw_os_error(error)
        });
    }
    let slave = unsafe { libc::open(name.as_ptr(), flags) };
    if slave < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((master, unsafe { OwnedFd::from_raw_fd(slave) }))
}

#[cfg(not(target_os = "linux"))]
fn open_pair() -> io::Result<(OwnedFd, OwnedFd)> {
    let mut master = -1;
    let mut slave = -1;
    if unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
        )
    } < 0
    {
        return Err(io::Error::last_os_error());
    }
    let master = unsafe { OwnedFd::from_raw_fd(master) };
    let slave = unsafe { OwnedFd::from_raw_fd(slave) };
    for fd in [&master, &slave] {
        if unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok((master, slave))
}

pub(super) fn signal_group(child: &Child, signal: libc::c_int) -> io::Result<()> {
    if unsafe { libc::kill(-(child.id() as libc::pid_t), signal) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
