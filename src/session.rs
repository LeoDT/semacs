//! Minibuffer read / query sessions and basic completion.
//!
//! A session is bound to the target that opened it. Only one may exist at a
//! time; `ReadSession::id` isolates late results from a session that has ended.

use std::path::PathBuf;

use crate::keymap::{Keymap, parse_sequence};
use gpui_kit::{Entity, Keystroke};

use crate::command::{COMMANDS, Command, Scope};
use crate::view::BufferView;

/// What a confirmed session does with the text read in the minibuffer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadKind {
    /// `M-x`: read a command name and run it on the origin.
    Command,
    /// `C-x C-f`: read a path and visit it from the origin.
    FindFile,
    /// `C-x C-w`: read a path and write the buffer there.
    WriteFile,
    /// `C-x b`: read an existing buffer name and activate it.
    SwitchToBuffer { default: Option<String> },
    /// A `write-file` target already exists.
    Overwrite(PathBuf),
    /// A visited file changed on disk since it was read or saved.
    ExternalChange(PathBuf),
    /// `C-x k`: read the name of the buffer to kill.
    KillBuffer { default: String },
    /// A modified buffer would be killed.
    KillBufferConfirm { name: String },
    /// Exit coordination asks whether to save one dirty buffer.
    ExitSave { path: Option<PathBuf>, name: String },
    /// Dirty buffers remain after the per-buffer answers.
    ExitModified,
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
        }
    }
}

/// Command candidates shared by `M-x` completion and exact lookup.
pub fn command_names() -> Vec<String> {
    let mut names: Vec<_> = COMMANDS
        .iter()
        .filter(|spec| spec.scope != Scope::ReadSession)
        .map(|spec| spec.name.to_owned())
        .collect();
    names.sort_unstable();
    names
}

/// Completed matches and the input change they imply.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Completion {
    /// Matching candidate names in display order.
    pub candidates: Vec<String>,
    /// Text the input area should become when completion made progress.
    pub replacement: Option<String>,
}

impl Completion {
    /// The message Emacs reports for a completion request.
    pub fn message(&self) -> String {
        match self.candidates.len() {
            0 => "No match".to_owned(),
            1 => "Sole completion".to_owned(),
            count => {
                // The echo area replaces Emacs' *Completions* buffer; keep the
                // remainder countable instead of silently clipping it.
                const SHOWN: usize = 12;
                let mut message = String::from("Candidates:");
                for name in self.candidates.iter().take(SHOWN) {
                    message.push(' ');
                    message.push_str(name);
                }
                if count > SHOWN {
                    message.push_str(&format!(" ... ({count} total)"));
                }
                message
            }
        }
    }
}

/// Prefix completion over an already selected candidate source. The shell
/// chooses command names, paths or buffer names before asking for completion.
pub fn complete(candidates: Vec<String>, input: &str) -> Completion {
    let candidates: Vec<_> = candidates
        .into_iter()
        .filter(|name| name.starts_with(input))
        .collect();
    let replacement = match candidates.len() {
        1 => Some(candidates[0].clone()),
        _ => common_prefix(&candidates).filter(|prefix| prefix.len() > input.len()),
    };
    Completion {
        candidates,
        replacement,
    }
}

/// Resolves a name `M-x` may run; session commands stay internal.
pub fn session_command(name: &str) -> Option<Command> {
    Command::named(name).filter(|command| command.scope() != Scope::ReadSession)
}

pub fn invalid_command_message(name: &str) -> String {
    format!("‘{name}’ is not a valid command name")
}

fn common_prefix(candidates: &[String]) -> Option<String> {
    let first = candidates.first()?;
    let mut end = first.len();
    for candidate in &candidates[1..] {
        let mut shared = 0;
        for (a, b) in first.chars().zip(candidate.chars()) {
            if a != b {
                break;
            }
            shared += a.len_utf8();
        }
        end = end.min(shared);
    }
    Some(first[..end].to_owned())
}

/// One minibuffer read session.
pub struct ReadSession {
    id: u64,
    kind: ReadKind,
    origin: Entity<BufferView>,
    keymap: Keymap,
}

impl ReadSession {
    pub fn new(id: u64, kind: ReadKind, origin: Entity<BufferView>) -> Self {
        let keymap = if kind.is_single_key() {
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
        Self {
            id,
            kind,
            origin,
            keymap,
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_completion_is_sorted_prefix_matching_without_session_commands() {
        let completion = complete(command_names(), "forward-");
        assert_eq!(
            completion.replacement, None,
            "the common prefix is the input"
        );
        assert!(completion.candidates.contains(&"forward-char".into()));
        assert!(completion.candidates.contains(&"forward-word".into()));
        assert!(
            completion
                .candidates
                .windows(2)
                .all(|pair| pair[0] <= pair[1])
        );
        assert!(
            completion
                .candidates
                .iter()
                .all(|name| Command::named(name).is_some_and(|c| c.scope() != Scope::ReadSession))
        );
        assert!(completion.message().starts_with("Candidates: "));
    }

    #[test]
    fn completion_extends_to_the_longest_common_prefix_and_handles_edges() {
        let extended = complete(command_names(), "forw");
        assert_eq!(extended.replacement.as_deref(), Some("forward-"));
        assert_eq!(
            complete(command_names(), "nosuch").candidates,
            Vec::<String>::new()
        );
        assert_eq!(complete(command_names(), "nosuch").message(), "No match");
        let sole = complete(command_names(), "kill-regi");
        assert_eq!(sole.replacement.as_deref(), Some("kill-region"));
        assert_eq!(sole.message(), "Sole completion");
    }

    #[test]
    fn common_prefix_is_unicode_safe() {
        let names = vec!["前a".to_owned(), "前b".to_owned()];
        assert_eq!(common_prefix(&names).as_deref(), Some("前"));
        assert_eq!(common_prefix(&[]), None);
    }

    #[test]
    fn session_commands_are_not_resolvable_through_mx() {
        assert_eq!(session_command("forward-char"), Some(Command::ForwardChar));
        assert_eq!(session_command("save-buffer"), Some(Command::SaveBuffer));
        assert_eq!(session_command("minibuffer-complete"), None);
        assert_eq!(session_command("nosuch"), None);
        assert_eq!(
            invalid_command_message(""),
            "‘’ is not a valid command name"
        );
    }
}
