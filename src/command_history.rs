use std::collections::HashMap;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

use gpui_kit::Global;

use crate::completion::MatchedCandidate;

const CAPACITY: usize = 100;

#[derive(Default)]
pub struct CommandHistory {
    entries: Vec<String>,
    writer: Option<Arc<Writer>>,
}

impl Global for CommandHistory {}

impl CommandHistory {
    pub fn record(&mut self, name: &str) {
        if self.entries.first().is_some_and(|entry| entry == name) {
            return;
        }
        self.entries.insert(0, name.into());
        self.entries.truncate(CAPACITY);
        if let Some(writer) = &self.writer {
            writer.pending.lock().unwrap().text = Some(self.entries.join("\n"));
            writer.changed.notify_one();
        }
    }

    pub fn sort(&self, candidates: &mut [MatchedCandidate]) {
        let mut ranks = HashMap::new();
        for (index, name) in self.entries.iter().enumerate() {
            match ranks.entry(name.as_str()) {
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(if index == 0 {
                        i64::MIN / 2
                    } else {
                        index as i64
                    });
                }
                std::collections::hash_map::Entry::Occupied(mut entry) => {
                    *entry.get_mut() -=
                        (10.0 * (-(index as f64) / 100.0).exp()).round_ties_even() as i64;
                }
            }
        }
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

    fn load(path: &Path) -> io::Result<Self> {
        let file = match std::fs::File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(error) => return Err(error),
        };
        let mut text = String::new();
        file.take(1024 * 1024 + 1).read_to_string(&mut text)?;
        if text.len() > 1024 * 1024 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "command history exceeds 1 MiB",
            ));
        }
        let entries: Vec<_> = text.lines().take(CAPACITY).map(str::to_owned).collect();
        if entries.iter().any(|name| {
            name.is_empty()
                || name.len() > 128
                || !name
                    .bytes()
                    .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == b'-')
        }) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid command history",
            ));
        }
        Ok(Self {
            entries,
            writer: None,
        })
    }
}

#[derive(Default)]
struct Pending {
    text: Option<String>,
    stopping: bool,
}

#[derive(Default)]
struct Writer {
    pending: Mutex<Pending>,
    changed: Condvar,
}

pub struct Persistence {
    writer: Arc<Writer>,
    worker: Option<JoinHandle<()>>,
}

impl Persistence {
    pub fn open(path: PathBuf) -> io::Result<(CommandHistory, Self)> {
        let mut history = CommandHistory::load(&path).unwrap_or_else(|error| {
            eprintln!(
                "semacs: cannot read command history {}: {error}",
                path.display()
            );
            CommandHistory::default()
        });
        let writer = Arc::new(Writer::default());
        let work = writer.clone();
        let worker = std::thread::Builder::new()
            .name("command-history".into())
            .spawn(move || {
                loop {
                    let text = {
                        let mut pending = work.pending.lock().unwrap();
                        while pending.text.is_none() && !pending.stopping {
                            pending = work.changed.wait(pending).unwrap();
                        }
                        match pending.text.take() {
                            Some(text) => text,
                            None => break,
                        }
                    };
                    let result = path
                        .parent()
                        .map_or(Ok(()), std::fs::create_dir_all)
                        .and_then(|()| {
                            crate::files::write_atomic(&path, &text, Default::default()).map(|_| ())
                        });
                    if let Err(error) = result {
                        eprintln!(
                            "semacs: cannot save command history {}: {error}",
                            path.display()
                        );
                    }
                }
            })?;
        history.writer = Some(writer.clone());
        Ok((
            history,
            Self {
                writer,
                worker: Some(worker),
            },
        ))
    }
}

impl Drop for Persistence {
    fn drop(&mut self) {
        self.writer.pending.lock().unwrap().stopping = true;
        self.writer.changed.notify_one();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

pub fn state_path() -> io::Result<PathBuf> {
    state_path_from(
        std::env::var_os("XDG_STATE_HOME").map(PathBuf::from),
        std::env::var_os("HOME").map(PathBuf::from),
    )
}

fn state_path_from(xdg: Option<PathBuf>, home: Option<PathBuf>) -> io::Result<PathBuf> {
    xdg.filter(|path| path.is_absolute())
        .or_else(|| {
            home.filter(|path| path.is_absolute())
                .map(|path| path.join(".local/state"))
        })
        .map(|path| path.join("semacs/command-history"))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "neither XDG_STATE_HOME nor HOME is absolute",
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::completion::Candidate;

    fn sorted(history: &[&str], names: &[&str]) -> Vec<String> {
        let history = CommandHistory {
            entries: history.iter().map(|s| (*s).into()).collect(),
            writer: None,
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

    #[test]
    fn recording_is_bounded_and_only_suppresses_consecutive_duplicates() {
        let mut history = CommandHistory::default();
        for name in ["alpha", "alpha", "beta", "alpha"] {
            history.record(name);
        }
        assert_eq!(history.entries, ["alpha", "beta", "alpha"]);
        for index in 0..200 {
            history.record(&format!("command-{index}"));
        }
        assert_eq!(history.entries.len(), CAPACITY);
        assert_eq!(history.entries[0], "command-199");
    }

    #[test]
    fn paths_do_not_fall_back_to_the_project() {
        assert_eq!(
            state_path_from(Some("/state".into()), None).unwrap(),
            PathBuf::from("/state/semacs/command-history")
        );
        for xdg in [None, Some("".into()), Some("relative".into())] {
            assert_eq!(
                state_path_from(xdg, Some("/home/user".into())).unwrap(),
                PathBuf::from("/home/user/.local/state/semacs/command-history")
            );
        }
        assert!(state_path_from(Some("relative".into()), Some(".".into())).is_err());
    }

    #[gpui_kit::test]
    fn repeated_app_initialization_preserves_shared_history(cx: &mut gpui_kit::TestAppContext) {
        cx.update(crate::app::init);
        cx.update(|cx| cx.global_mut::<CommandHistory>().record("forward-char"));
        cx.update(crate::app::init);
        cx.update(|cx| assert_eq!(cx.global::<CommandHistory>().entries, ["forward-char"]));
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
        let (mut history, persistence) = Persistence::open(root.clone()).unwrap();
        history.record("undo");
        drop(persistence);
        assert_eq!(history.entries, ["undo"]);
        assert!(root.is_dir());
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
        let (mut history, persistence) = Persistence::open(path.clone()).unwrap();
        for name in ["alpha", "beta", "alpha"] {
            history.record(name);
        }
        drop(persistence);
        assert_eq!(
            CommandHistory::load(&path).unwrap().entries,
            ["alpha", "beta", "alpha"]
        );
        std::fs::write(&path, "invalid command").unwrap();
        assert!(CommandHistory::load(&path).is_err());
        std::fs::write(&path, vec![b'a'; 1024 * 1024 + 1]).unwrap();
        assert!(CommandHistory::load(&path).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
