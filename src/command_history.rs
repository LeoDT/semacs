//! Command-name history used to order `M-x` candidates.

use std::io;
use std::path::PathBuf;

use gpui_kit::Global;

use crate::completion::MatchedCandidate;
use crate::persisted::{self, List};

const CAPACITY: usize = 100;

pub struct CommandHistory {
    list: List,
}

impl Default for CommandHistory {
    fn default() -> Self {
        Self {
            list: List::new(CAPACITY),
        }
    }
}

impl Global for CommandHistory {}

impl CommandHistory {
    pub fn record(&mut self, name: &str) {
        self.list.record(name);
    }

    pub fn sort(&self, candidates: &mut [MatchedCandidate]) {
        let entries = self.list.entries();
        let ranks = crate::completion::history_ranks(entries, Some);
        candidates.sort_by(|a, b| {
            let key = |item: &MatchedCandidate| {
                ranks
                    .get(item.candidate.value.as_str())
                    .copied()
                    .unwrap_or(i64::MAX)
            };
            key(a)
                .cmp(&key(b))
                .then_with(|| {
                    a.candidate
                        .label
                        .chars()
                        .count()
                        .cmp(&b.candidate.label.chars().count())
                })
                .then_with(|| a.candidate.label.cmp(&b.candidate.label))
                .then_with(|| a.candidate.value.cmp(&b.candidate.value))
        });
    }
}

/// Opens the command history stored at `path`, starting empty when the file is
/// missing or unreadable.
pub fn open(path: PathBuf) -> io::Result<(CommandHistory, persisted::Persistence)> {
    let (list, persistence) = persisted::open(path, CAPACITY, valid)?;
    Ok((CommandHistory { list }, persistence))
}

fn valid(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::completion::Candidate;

    fn sorted(history: &[&str], names: &[&str]) -> Vec<String> {
        let history = CommandHistory {
            list: List::from_entries(history),
        };
        let mut candidates: Vec<_> = names
            .iter()
            .map(|name| MatchedCandidate {
                candidate: Candidate::named((*name).into(), String::new()),
                highlights: Vec::new(),
            })
            .collect();
        history.sort(&mut candidates);
        candidates
            .into_iter()
            .map(|item| item.candidate.value)
            .collect()
    }

    #[test]
    fn recent_is_first_duplicates_are_weighted_and_unknowns_use_length_then_name() {
        assert_eq!(
            sorted(
                &["recent", "once", "frequent", "frequent"],
                &[
                    "long-unknown",
                    "bb",
                    "a",
                    "ab",
                    "once",
                    "frequent",
                    "recent"
                ]
            ),
            [
                "recent",
                "frequent",
                "once",
                "a",
                "ab",
                "bb",
                "long-unknown"
            ]
        );
        let mut entries = vec!["recent", "once", "frequent"];
        entries.extend(std::iter::repeat_n("other", 500));
        entries.push("frequent");
        assert_eq!(
            sorted(&entries, &["once", "frequent", "recent"]),
            ["recent", "once", "frequent"]
        );
    }

    #[gpui_kit::test]
    fn repeated_app_initialization_preserves_shared_history(cx: &mut gpui_kit::TestAppContext) {
        cx.update(crate::app::init);
        cx.update(|cx| cx.global_mut::<CommandHistory>().record("forward-char"));
        cx.update(crate::app::init);
        cx.update(|cx| {
            assert_eq!(
                cx.global::<CommandHistory>().list.entries(),
                ["forward-char"]
            )
        });
    }

    #[test]
    fn failed_persistence_does_not_prevent_in_memory_history() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
            ".pi/tmp/history-failure-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let (mut history, persistence) = open(root.clone()).unwrap();
        history.record("undo");
        drop(persistence);
        assert_eq!(history.list.entries(), ["undo"]);
        std::fs::remove_dir(root).unwrap();
    }

    #[test]
    fn persistence_flushes_on_shutdown_and_roundtrips_duplicates() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
            ".pi/tmp/command-history-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = root.join("state/history");
        let (mut history, persistence) = open(path.clone()).unwrap();
        for name in ["alpha", "beta", "alpha"] {
            history.record(name);
        }
        drop(persistence);
        let (history, persistence) = open(path.clone()).unwrap();
        assert_eq!(history.list.entries(), ["alpha", "beta", "alpha"]);
        drop(persistence);
        std::fs::remove_dir_all(root).unwrap();
    }
}
