use std::ffi::{OsStr, OsString};
use std::fs::DirBuilder;
use std::io::{self, Read, Write};
use std::net::Shutdown;
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::DirBuilderExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

const HELPER: &str = "--internal-git-editor";
const MAGIC: &[u8; 4] = b"SME1";
const MAX_PATH: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorOutcome {
    Finish,
    Cancel,
}

#[derive(Debug)]
pub struct EditorRequest {
    pub path: PathBuf,
    reply: Arc<Reply>,
}

impl EditorRequest {
    #[cfg(test)]
    pub(crate) fn test_pair(path: PathBuf) -> (Self, UnixStream) {
        let (stream, peer) = UnixStream::pair().unwrap();
        peer.set_nonblocking(true).unwrap();
        (
            Self {
                path,
                reply: Arc::new(Reply(Mutex::new(Some(stream)))),
            },
            peer,
        )
    }

    pub fn respond(self, outcome: EditorOutcome) -> io::Result<()> {
        let mut stream = self.reply.0.lock().unwrap().take().ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotConnected, "Git editor session has ended")
        })?;
        stream.write_all(&[match outcome {
            EditorOutcome::Finish => 0,
            EditorOutcome::Cancel => 1,
        }])
    }
}

impl Drop for EditorRequest {
    fn drop(&mut self) {
        self.reply.close();
    }
}

#[derive(Debug)]
struct Reply(Mutex<Option<UnixStream>>);

impl Reply {
    fn close(&self) {
        if let Some(stream) = self.0.lock().unwrap().take() {
            let _ = stream.shutdown(Shutdown::Both);
        }
    }
}

struct Incoming {
    stream: UnixStream,
    bytes: Vec<u8>,
}

pub(super) struct Bridge {
    directory: PathBuf,
    socket: PathBuf,
    listener: UnixListener,
    incoming: Vec<Incoming>,
    replies: Vec<Arc<Reply>>,
}

impl Bridge {
    pub fn bind(parent: &Path) -> io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let parent = parent.canonicalize()?;
        let directory = loop {
            let path = parent.join(format!(
                ".semacs-editor-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match DirBuilder::new().mode(0o700).create(&path) {
                Ok(()) => break path,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        };
        let socket = directory.join("edit.sock");
        let listener = match UnixListener::bind(&socket).and_then(|listener| {
            listener.set_nonblocking(true)?;
            Ok(listener)
        }) {
            Ok(listener) => listener,
            Err(error) => {
                let _ = std::fs::remove_file(&socket);
                let _ = std::fs::remove_dir(&directory);
                return Err(error);
            }
        };
        Ok(Self {
            directory,
            socket,
            listener,
            incoming: Vec::new(),
            replies: Vec::new(),
        })
    }

    pub fn editor_command(&self, executable: &Path) -> OsString {
        let mut command = quote(executable.as_os_str());
        command.extend_from_slice(format!(" {HELPER} ").as_bytes());
        command.extend(quote(self.socket.as_os_str()));
        OsString::from_vec(command)
    }

    pub fn poll_descriptors(&self, out: &mut Vec<libc::pollfd>) {
        for fd in std::iter::once(self.listener.as_raw_fd()).chain(
            self.incoming
                .iter()
                .map(|request| request.stream.as_raw_fd()),
        ) {
            out.push(descriptor(fd));
        }
    }

    pub fn requests(&mut self) -> io::Result<Vec<EditorRequest>> {
        for _ in 0..16 {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    stream.set_nonblocking(true)?;
                    self.incoming.push(Incoming {
                        stream,
                        bytes: Vec::new(),
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            }
        }
        let mut requests = Vec::new();
        let mut index = 0;
        while index < self.incoming.len() {
            let incoming = &mut self.incoming[index];
            let mut bytes = [0; 8192];
            let read = incoming.stream.read(&mut bytes);
            let remove = match read {
                Ok(0) => true,
                Ok(count) => {
                    incoming.bytes.extend_from_slice(&bytes[..count]);
                    match decode_path(&incoming.bytes) {
                        Ok(Some(path)) => {
                            let incoming = self.incoming.swap_remove(index);
                            let reply = Arc::new(Reply(Mutex::new(Some(incoming.stream))));
                            self.replies.push(reply.clone());
                            requests.push(EditorRequest { path, reply });
                            continue;
                        }
                        Ok(None) => false,
                        Err(_) => true,
                    }
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) =>
                {
                    false
                }
                Err(_) => true,
            };
            if remove {
                self.incoming.swap_remove(index);
            } else {
                index += 1;
            }
        }
        self.replies
            .retain(|reply| reply.0.lock().unwrap().is_some());
        Ok(requests)
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        for reply in &self.replies {
            reply.close();
        }
        let _ = std::fs::remove_file(&self.socket);
        let _ = std::fs::remove_dir(&self.directory);
    }
}

pub(super) fn descriptor(fd: RawFd) -> libc::pollfd {
    libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    }
}

fn quote(value: &OsStr) -> Vec<u8> {
    let mut quoted = vec![b'\''];
    for byte in value.as_bytes() {
        if *byte == b'\'' {
            quoted.extend_from_slice(b"'\\''");
        } else {
            quoted.push(*byte);
        }
    }
    quoted.push(b'\'');
    quoted
}

fn decode_path(bytes: &[u8]) -> io::Result<Option<PathBuf>> {
    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "Invalid Git editor request");
    if bytes.len() < 8 {
        return Ok(None);
    }
    let length = u32::from_be_bytes(bytes[4..8].try_into().unwrap()) as usize;
    if &bytes[..4] != MAGIC || length == 0 || length > MAX_PATH || bytes.len() > length + 8 {
        return Err(invalid());
    }
    if bytes.len() < length + 8 {
        return Ok(None);
    }
    let path = PathBuf::from(OsString::from_vec(bytes[8..].to_vec()));
    if !path.is_absolute() || bytes[8..].contains(&0) {
        return Err(invalid());
    }
    Ok(Some(path))
}

pub fn run_helper_if_requested(args: impl IntoIterator<Item = OsString>) -> Option<ExitCode> {
    let mut args = args.into_iter();
    if args.next().as_deref() != Some(OsStr::new(HELPER)) {
        return None;
    }
    let result = (|| {
        let socket = args
            .next()
            .ok_or_else(|| io::Error::other("Missing editor socket"))?;
        let path = args
            .next()
            .ok_or_else(|| io::Error::other("Missing editor file"))?;
        if args.next().is_some() {
            return Err(io::Error::other("Expected one Git editor file"));
        }
        let path = PathBuf::from(path);
        let path = if path.is_absolute() {
            path
        } else {
            std::env::current_dir()?.join(path)
        };
        let path = path.as_os_str().as_bytes();
        if path.is_empty() || path.len() > MAX_PATH {
            return Err(io::Error::other("Invalid editor file length"));
        }
        let mut stream = UnixStream::connect(socket)?;
        stream.write_all(MAGIC)?;
        stream.write_all(&(path.len() as u32).to_be_bytes())?;
        stream.write_all(path)?;
        let mut response = [0];
        stream.read_exact(&mut response)?;
        match response[0] {
            0 => Ok(ExitCode::SUCCESS),
            1 => Ok(ExitCode::FAILURE),
            _ => Err(io::Error::other("Invalid editor response")),
        }
    })();
    Some(match result {
        Ok(status) => status,
        Err(error) => {
            eprintln!("semacs Git editor: {error}");
            ExitCode::FAILURE
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_protocol_preserves_bytes_and_rejects_invalid_frames() {
        let path = b"/a/'\n\xff/message";
        let mut frame = MAGIC.to_vec();
        frame.extend_from_slice(&(path.len() as u32).to_be_bytes());
        frame.extend_from_slice(path);
        for end in 0..frame.len() {
            assert!(decode_path(&frame[..end]).unwrap().is_none());
        }
        assert_eq!(
            decode_path(&frame).unwrap().unwrap().as_os_str().as_bytes(),
            path
        );
        for invalid in [
            b"NOPE\0\0\0\x01x".as_slice(),
            b"SME1\0\0\0\x01x",
            b"SME1\0\0\0\0",
            b"SME1\xff\xff\xff\xff",
        ] {
            assert!(decode_path(invalid).is_err());
        }
        frame.push(0);
        assert!(decode_path(&frame).is_err());
    }
}
