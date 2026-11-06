//! Minibuffer read / query sessions and their candidate selection.
//!
//! A session is bound to the target that opened it. Only one may exist at a
//! time; `ReadSession::id` isolates late results from a session that has ended.

use std::path::PathBuf;

use crate::keymap::{Keymap, parse_sequence};
use gpui_kit::{Entity, Keystroke};

use crate::command::{CANDIDATE_BINDINGS, Command, Scope};
use crate::completion::CandidateList;
use crate::view::BufferView;

/// What a confirmed session does with the text read in the minibuffer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadKind {
    /// `M-x`: read a command name and run it on the origin.
    Command,
    FontFamily,
    Theme,
    /// `C-x C-f`: read a path and visit it from the origin.
    FindFile,
    /// `C-x C-w`: read a path and write the buffer there.
    WriteFile,
    /// `C-x b`: read an existing buffer name and activate it.
    SwitchToBuffer {
        default: Option<String>,
    },
    /// A `write-file` target already exists.
    Overwrite(PathBuf),
    /// A visited file changed on disk since it was read or saved.
    ExternalChange(PathBuf),
    /// `C-x k`: read the name of the buffer to kill.
    KillBuffer {
        default: String,
    },
    /// A modified buffer would be killed.
    KillBufferConfirm {
        name: String,
    },
    /// Exit coordination asks whether to save one dirty buffer.
    ExitSave {
        path: Option<PathBuf>,
        name: String,
    },
    /// Dirty buffers remain after the per-buffer answers.
    ExitModified,
    ExitUncertainGit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Answer {
    Yes,
    No,
    Save,
    Invalid,
}

impl Answer {
    pub fn full(input: &str) -> Self {
        match input.trim().to_ascii_lowercase().as_str() {
            "yes" => Self::Yes,
            "no" => Self::No,
            _ => Self::Invalid,
        }
    }

    pub fn key(stroke: &Keystroke) -> Self {
        match stroke.key.as_str() {
            "y" | "Y" | "space" => Self::Yes,
            "n" | "N" | "backspace" | "delete" => Self::No,
            "s" | "S" => Self::Save,
            _ => Self::Invalid,
        }
    }
}

impl ReadKind {
    pub fn has_candidates(&self) -> bool {
        matches!(
            self,
            Self::Command
                | Self::Theme
                | Self::FindFile
                | Self::WriteFile
                | Self::SwitchToBuffer { .. }
                | Self::KillBuffer { .. }
        )
    }

    pub fn candidate_default(&self) -> Option<&str> {
        match self {
            Self::SwitchToBuffer { default } => default.as_deref(),
            Self::KillBuffer { default } => Some(default),
            _ => None,
        }
    }

    pub fn is_single_key(&self) -> bool {
        matches!(
            self,
            Self::Overwrite(_) | Self::KillBufferConfirm { .. } | Self::ExitSave { .. }
        )
    }

    pub fn answer_hint(&self) -> &'static str {
        match self {
            Self::KillBufferConfirm { .. } => "Please answer y, n, or s",
            _ if self.is_single_key() => "Please answer y or n",
            _ => "Please answer yes or no",
        }
    }

    pub fn prompt(&self) -> String {
        match self {
            ReadKind::Command => "M-x ".to_owned(),
            ReadKind::FontFamily => "Font family: ".to_owned(),
            ReadKind::Theme => "Load theme: ".to_owned(),
            ReadKind::FindFile => "Find file: ".to_owned(),
            ReadKind::WriteFile => "Write file: ".to_owned(),
            ReadKind::SwitchToBuffer { default } => match default {
                Some(name) => format!("Switch to buffer (default {name}): "),
                None => "Switch to buffer: ".to_owned(),
            },
            ReadKind::Overwrite(path) => {
                format!("File ‘{}’ exists; overwrite? (y or n) ", path.display())
            }
            ReadKind::ExternalChange(path) => format!(
                "{} has changed since visited or saved.  Save anyway? (yes or no) ",
                path.display()
            ),
            ReadKind::KillBuffer { default } => {
                format!("Kill buffer (default {default}): ")
            }
            ReadKind::KillBufferConfirm { name } => {
                format!("Buffer {name} modified; kill anyway? (y, n, s) ")
            }
            ReadKind::ExitSave { path, name } => match path {
                Some(path) => format!("Save file {}? (y or n) ", path.display()),
                None => format!("Save buffer {name}? (y or n) "),
            },
            ReadKind::ExitModified => {
                "Modified buffers exist; exit anyway? (yes or no) ".to_owned()
            }
            ReadKind::ExitUncertainGit => {
                "A Git write result is uncertain; inspect *Messages* and repository. Exit anyway? (yes or no) ".to_owned()
            }
        }
    }
}

/// One minibuffer read session.
pub struct ReadSession {
    id: u64,
    kind: ReadKind,
    origin: Entity<BufferView>,
    keymap: Keymap,
    pub(crate) candidates: Option<CandidateList>,
}

impl ReadSession {
    pub fn new(id: u64, kind: ReadKind, origin: Entity<BufferView>) -> Self {
        let mut keymap = if kind.is_single_key() {
            let mut keymap = Keymap::default();
            for key in ["y", "shift-y", "n", "shift-n", "enter", "ctrl-m"] {
                keymap
                    .bind(
                        &parse_sequence(key).unwrap(),
                        Command::MinibufferCompleteAndExit,
                    )
                    .unwrap();
            }
            let extra = if matches!(kind, ReadKind::KillBufferConfirm { .. }) {
                &["s", "shift-s"][..]
            } else {
                &["space", "backspace", "delete"][..]
            };
            for key in extra {
                keymap
                    .bind(
                        &parse_sequence(key).unwrap(),
                        Command::MinibufferCompleteAndExit,
                    )
                    .unwrap();
            }
            keymap
                .bind(
                    &parse_sequence("ctrl-g").unwrap(),
                    Command::AbortMinibuffers,
                )
                .unwrap();
            keymap
        } else {
            Keymap::for_scope(Scope::ReadSession)
        };
        if kind.has_candidates() {
            for &(key, command) in CANDIDATE_BINDINGS {
                keymap.bind(&parse_sequence(key).unwrap(), command).unwrap();
            }
        }
        let candidates = kind.has_candidates().then(CandidateList::default);
        Self {
            id,
            kind,
            origin,
            keymap,
            candidates,
        }
    }

    pub fn candidates(&self) -> Option<&CandidateList> {
        self.candidates.as_ref()
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn kind(&self) -> &ReadKind {
        &self.kind
    }

    pub fn origin(&self) -> &Entity<BufferView> {
        &self.origin
    }

    pub(crate) fn keymap(&self) -> &Keymap {
        &self.keymap
    }
}
