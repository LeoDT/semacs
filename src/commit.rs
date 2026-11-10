use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{
    self, Receiver, RecvError, RecvTimeoutError, SyncSender, TryRecvError, TrySendError,
};
use std::sync::{Arc, Mutex, Weak};
use std::task::{Poll, Waker};
use std::thread::{self, JoinHandle};
use std::time::Duration;

mod editor;
#[cfg(test)]
mod event_tests;
mod input;
pub mod message;
mod preparation;
pub mod prompt;
mod terminal;

pub use input::{Input, login_name};

pub use crate::git::Repository;
use editor::Bridge;
pub use editor::{EditorOutcome, EditorRequest, run_helper_if_requested};
use terminal::{Terminal, signal_group};

#[derive(Clone, Debug)]
pub struct Runtime {
    pub executable: PathBuf,
    pub directory: PathBuf,
}

impl Runtime {
    pub fn current() -> io::Result<Self> {
        Ok(Self {
            executable: std::env::current_exe()?,
            directory: std::env::temp_dir(),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Staged,
    All,
}

pub(crate) use preparation::{Preparation, check_target, prepare};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Intent {
    Create,
    Amend,
    Reword,
    Fixup(String),
    Squash(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetKind {
    Fixup,
    Squash,
}

impl TargetKind {
    pub(crate) fn intent(self, target: String) -> Intent {
        match self {
            Self::Fixup => Intent::Fixup(target),
            Self::Squash => Intent::Squash(target),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Create,
    Amend,
    Reword,
    Target(TargetKind),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub repository: Repository,
    pub action: Action,
    pub initial: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub repository: Repository,
    pub scope: Scope,
    pub intent: Intent,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Continuation {
    Select {
        repository: Repository,
        scope: Scope,
        kind: TargetKind,
        initial: Option<String>,
    },
    Launch(Plan),
}

#[derive(Debug)]
pub enum Event {
    Output(Vec<u8>),
    Edit(EditorRequest),
    Diagnostic(String),
    Exited(ExitStatus),
    WaitFailed(String),
}

#[derive(Debug)]
pub struct StartFailure {
    pub started: bool,
    pub diagnostic: String,
}

pub struct Commit {
    control: Arc<Control>,
    input: File,
    worker: Option<JoinHandle<()>>,
}

pub struct Events {
    receive: Receiver<Event>,
    control: Weak<Control>,
    notification: Arc<Notification>,
}

#[derive(Default)]
struct Notification(Mutex<Option<Waker>>);

impl Notification {
    fn wake(&self) {
        let waker = self.0.lock().unwrap().take();
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

struct EventSender {
    send: Option<SyncSender<Event>>,
    notification: Arc<Notification>,
}

impl Drop for EventSender {
    fn drop(&mut self) {
        self.send.take();
        self.notification.wake();
    }
}

impl Events {
    pub async fn next(&mut self) -> Result<Event, RecvError> {
        std::future::poll_fn(|cx| {
            *self.notification.0.lock().unwrap() = Some(cx.waker().clone());
            match self.receive.try_recv() {
                Ok(event) => Poll::Ready(Ok(event)),
                Err(TryRecvError::Disconnected) => Poll::Ready(Err(RecvError)),
                Err(TryRecvError::Empty) => Poll::Pending,
            }
        })
        .await
    }

    pub fn recv(&self) -> Result<Event, RecvError> {
        self.receive.recv()
    }

    pub fn recv_timeout(&self, timeout: Duration) -> Result<Event, RecvTimeoutError> {
        self.receive.recv_timeout(timeout)
    }
}

impl Drop for Events {
    fn drop(&mut self) {
        if let Some(control) = self.control.upgrade() {
            control.abandon();
        }
    }
}

struct Control {
    child: Mutex<Option<Child>>,
    abandoned: AtomicBool,
}

impl Control {
    fn abandon(&self) {
        self.abandoned.store(true, Ordering::Release);
        let _ = self.signal(libc::SIGKILL);
    }

    fn signal(&self, signal: libc::c_int) -> io::Result<()> {
        let child = self.child.lock().unwrap();
        match child.as_ref() {
            Some(child) => signal_group(child, signal),
            None => Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "Git is no longer running",
            )),
        }
    }

    fn try_wait(&self) -> io::Result<Option<ExitStatus>> {
        let mut child = self.child.lock().unwrap();
        let result = child.as_mut().unwrap().try_wait();
        if matches!(&result, Ok(Some(_)))
            || matches!(&result, Err(error) if error.raw_os_error() == Some(libc::ECHILD))
        {
            *child = None;
        }
        match result {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => Ok(None),
            result => result,
        }
    }

    fn stop(&self) {
        if let Some(mut child) = self.child.lock().unwrap().take() {
            let _ = signal_group(&child, libc::SIGKILL);
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for Control {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Commit {
    pub fn start(
        repository: &Repository,
        editor_executable: &Path,
        runtime_directory: &Path,
    ) -> Result<(Self, Events), StartFailure> {
        Self::start_with_scope(
            repository,
            editor_executable,
            runtime_directory,
            Scope::Staged,
        )
    }

    pub fn start_with_scope(
        repository: &Repository,
        editor_executable: &Path,
        runtime_directory: &Path,
        scope: Scope,
    ) -> Result<(Self, Events), StartFailure> {
        Self::start_with_intent(
            repository,
            editor_executable,
            runtime_directory,
            scope,
            &Intent::Create,
        )
    }

    pub fn start_with_intent(
        repository: &Repository,
        editor_executable: &Path,
        runtime_directory: &Path,
        scope: Scope,
        intent: &Intent,
    ) -> Result<(Self, Events), StartFailure> {
        let before_start = |error: io::Error| StartFailure {
            started: false,
            diagnostic: error.to_string(),
        };
        let bridge = Bridge::bind(runtime_directory).map_err(before_start)?;
        let terminal = Terminal::open().map_err(before_start)?;
        let input = terminal.master.try_clone().map_err(before_start)?;
        let mut command = Command::new("git");
        command
            .current_dir(&repository.root)
            .arg("--git-dir")
            .arg(&repository.git_dir)
            .arg("--work-tree")
            .arg(&repository.root)
            .arg("commit")
            .arg("--verbose");
        if scope == Scope::All {
            command.arg("--all");
        }
        match intent {
            Intent::Create => {}
            Intent::Amend => {
                command.arg("--amend");
            }
            Intent::Reword => {
                command.args(["--amend", "--only", "--allow-empty"]);
            }
            Intent::Fixup(target) | Intent::Squash(target) => {
                if !crate::git::valid_oid(target) {
                    return Err(before_start(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "Expected a complete target commit identity",
                    )));
                }
                command
                    .arg(format!(
                        "--{}={target}",
                        if matches!(intent, Intent::Fixup(_)) {
                            "fixup"
                        } else {
                            "squash"
                        }
                    ))
                    .arg("--no-edit");
            }
        }
        command.env("GIT_EDITOR", bridge.editor_command(editor_executable));
        let child = terminal.spawn(&mut command).map_err(before_start)?;
        let control = Arc::new(Control {
            child: Mutex::new(Some(child)),
            abandoned: AtomicBool::new(false),
        });
        let (send, receive) = mpsc::sync_channel(16);
        let notification = Arc::new(Notification::default());
        let send = EventSender {
            send: Some(send),
            notification: notification.clone(),
        };
        let event_control = Arc::downgrade(&control);
        let state = control.clone();
        let master = terminal.master;
        let worker = thread::Builder::new()
            .name("git-commit".into())
            .spawn(move || {
                run(master, bridge, &state, &send);
            })
            .map_err(|error| StartFailure {
                started: true,
                diagnostic: error.to_string(),
            })?;
        Ok((
            Self {
                control,
                input,
                worker: Some(worker),
            },
            Events {
                receive,
                control: event_control,
                notification,
            },
        ))
    }

    pub fn hangup(&self) -> io::Result<()> {
        self.control.signal(libc::SIGHUP)
    }

    pub fn interrupt(&self) -> io::Result<()> {
        self.control.signal(libc::SIGINT)
    }

    pub fn kill(&self) -> io::Result<()> {
        self.control.signal(libc::SIGKILL)
    }

    pub fn input(&self) -> io::Result<Input> {
        Ok(Input {
            file: self.input.try_clone()?,
            control: Arc::downgrade(&self.control),
        })
    }

    pub fn try_write_input(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let child = self.control.child.lock().unwrap();
        if child.is_none() {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "Git is no longer running",
            ));
        }
        self.input.write(bytes)
    }

    pub fn join(mut self) -> thread::Result<()> {
        self.worker.take().unwrap().join()
    }
}

impl Drop for Commit {
    fn drop(&mut self) {
        if self.worker.is_some() {
            self.control.abandon();
        }
    }
}

fn send_event(send: &EventSender, control: &Control, mut event: Event) -> bool {
    loop {
        if control.abandoned.load(Ordering::Acquire) {
            return false;
        }
        match send.send.as_ref().unwrap().try_send(event) {
            Ok(()) => {
                send.notification.wake();
                return true;
            }
            Err(TrySendError::Disconnected(_)) => return false,
            Err(TrySendError::Full(pending)) => {
                event = pending;
                thread::sleep(Duration::from_millis(2));
            }
        }
    }
}

enum Drain {
    Open,
    Closed,
    Disconnected,
}

fn output(master: &mut File, control: &Control, send: &EventSender) -> io::Result<Drain> {
    for _ in 0..16 {
        let mut bytes = [0; 8192];
        match master.read(&mut bytes) {
            Ok(0) => return Ok(Drain::Closed),
            Ok(count) => {
                if !send_event(send, control, Event::Output(bytes[..count].to_vec())) {
                    return Ok(Drain::Disconnected);
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
            Err(error) if error.raw_os_error() == Some(libc::EIO) => return Ok(Drain::Closed),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    Ok(Drain::Open)
}

fn run(mut master: File, bridge: Bridge, control: &Control, send: &EventSender) {
    let mut bridge = Some(bridge);
    let mut readable = true;
    loop {
        if control.abandoned.load(Ordering::Acquire) {
            control.stop();
            return;
        }
        if readable {
            match output(&mut master, control, send) {
                Ok(Drain::Open) => {}
                Ok(Drain::Closed) => readable = false,
                Ok(Drain::Disconnected) => {
                    control.stop();
                    return;
                }
                Err(error) => {
                    readable = false;
                    if !send_event(
                        send,
                        control,
                        Event::Diagnostic(format!("Git output read failed: {error}")),
                    ) {
                        control.stop();
                        return;
                    }
                }
            }
        }
        if let Some(active) = bridge.as_mut() {
            match active.requests() {
                Ok(requests) => {
                    for request in requests {
                        if !send_event(send, control, Event::Edit(request)) {
                            control.stop();
                            return;
                        }
                    }
                }
                Err(error) => {
                    bridge = None;
                    if !send_event(
                        send,
                        control,
                        Event::Diagnostic(format!("Git editor bridge failed: {error}")),
                    ) {
                        control.stop();
                        return;
                    }
                }
            }
        }
        match control.try_wait() {
            Ok(Some(status)) => {
                if readable && let Err(error) = output(&mut master, control, send) {
                    send_event(
                        send,
                        control,
                        Event::Diagnostic(format!("Git output read failed: {error}")),
                    );
                }
                drop(bridge);
                send_event(send, control, Event::Exited(status));
                return;
            }
            Ok(None) => {}
            Err(error) => {
                control.stop();
                drop(bridge);
                send_event(send, control, Event::WaitFailed(error.to_string()));
                return;
            }
        }
        let mut descriptors = Vec::new();
        if readable {
            descriptors.push(editor::descriptor(master.as_raw_fd()));
        }
        if let Some(bridge) = &bridge {
            bridge.poll_descriptors(&mut descriptors);
        }
        let result = unsafe {
            libc::poll(
                descriptors.as_mut_ptr(),
                descriptors.len() as libc::nfds_t,
                50,
            )
        };
        if result < 0 {
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted {
                control.stop();
                drop(bridge);
                send_event(send, control, Event::WaitFailed(error.to_string()));
                return;
            }
        }
    }
}
