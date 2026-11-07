use super::Control;
use std::fs::File;
use std::io::{self, Write};
use std::os::fd::AsRawFd;
use std::sync::Weak;

pub struct Input {
    pub(super) file: File,
    pub(super) control: Weak<Control>,
}

impl Input {
    pub fn write_all(mut self, mut bytes: &[u8]) -> io::Result<()> {
        while !bytes.is_empty() {
            let control = self.control.upgrade().ok_or_else(disconnected)?;
            let result = {
                let child = control.child.lock().unwrap();
                if child.is_none() || control.abandoned.load(std::sync::atomic::Ordering::Acquire) {
                    return Err(disconnected());
                }
                self.file.write(bytes)
            };
            match result {
                Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
                Ok(count) => bytes = &bytes[count..],
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    let mut descriptor = libc::pollfd {
                        fd: self.file.as_raw_fd(),
                        events: libc::POLLOUT,
                        revents: 0,
                    };
                    if unsafe { libc::poll(&mut descriptor, 1, 50) } < 0 {
                        let error = io::Error::last_os_error();
                        if error.kind() != io::ErrorKind::Interrupted {
                            return Err(error);
                        }
                    }
                }
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }
}

fn disconnected() -> io::Error {
    io::Error::new(io::ErrorKind::NotConnected, "Git is no longer running")
}

pub fn login_name() -> String {
    let mut size = 1024;
    loop {
        let mut buffer = vec![0; size];
        let mut entry = std::mem::MaybeUninit::uninit();
        let mut result = std::ptr::null_mut();
        let error = unsafe {
            libc::getpwuid_r(
                libc::geteuid(),
                entry.as_mut_ptr(),
                buffer.as_mut_ptr(),
                buffer.len(),
                &mut result,
            )
        };
        if error == libc::ERANGE {
            size *= 2;
            continue;
        }
        if error != 0 || result.is_null() {
            return String::new();
        }
        return unsafe { std::ffi::CStr::from_ptr((*result).pw_name) }
            .to_string_lossy()
            .into_owned();
    }
}
