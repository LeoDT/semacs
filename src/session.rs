//! Minibuffer read / query sessions and basic completion.
//!
//! A session is bound to the target that opened it. Only one may exist at a
//! time; `ReadSession::id` isolates late results from a session that has ended.

use gpui_kit::Entity;

use crate::command::{COMMANDS, Command, Scope};
use crate::view::BufferView;

/// What a confirmed session does with the text read in the minibuffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadKind {
    /// `M-x`: read a command name and run it on the origin.
    Command,
}

impl ReadKind {
    pub fn prompt(self) -> &'static str {
        match self {
            ReadKind::Command => "M-x ",
        }
    }

    fn names(self) -> Vec<String> {
        match self {
            // Session commands drive the minibuffer itself and are not commands
            // the user can invoke through M-x.
            ReadKind::Command => {
                let mut names: Vec<_> = COMMANDS
                    .iter()
                    .filter(|spec| spec.scope != Scope::ReadSession)
                    .map(|spec| spec.name.to_owned())
                    .collect();
                names.sort_unstable();
                names
            }
        }
    }
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

/// Prefix completion: the matching names plus the longest common prefix (or the
/// sole candidate). No fuzzy matching, cycling or recursive minibuffers.
pub fn complete(kind: ReadKind, input: &str) -> Completion {
    let candidates: Vec<_> = kind
        .names()
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
    candidates: Vec<String>,
}

impl ReadSession {
    pub fn new(id: u64, kind: ReadKind, origin: Entity<BufferView>) -> Self {
        Self {
            id,
            kind,
            origin,
            candidates: Vec::new(),
        }
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn kind(&self) -> ReadKind {
        self.kind
    }

    pub fn origin(&self) -> &Entity<BufferView> {
        &self.origin
    }

    /// Candidates from the most recent completion request.
    pub fn candidates(&self) -> &[String] {
        &self.candidates
    }

    pub(crate) fn record_completion(&mut self, completion: &Completion) {
        self.candidates = completion.candidates.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_completion_is_sorted_prefix_matching_without_session_commands() {
        let completion = complete(ReadKind::Command, "forward-");
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
        let extended = complete(ReadKind::Command, "forw");
        assert_eq!(extended.replacement.as_deref(), Some("forward-"));
        assert_eq!(
            complete(ReadKind::Command, "nosuch").candidates,
            Vec::<String>::new()
        );
        assert_eq!(complete(ReadKind::Command, "nosuch").message(), "No match");
        let sole = complete(ReadKind::Command, "kill-regi");
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
