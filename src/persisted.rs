//! A bounded newest-first list of strings mirrored to a state file.
//!
//! Command and file-name histories only differ in validation and ordering, so
//! loading, truncation and the background atomic write live here.

use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

const MAX_FILE_BYTES: u64 = 1024 * 1024;

pub struct List {
    entries: Vec<String>,
    capacity: usize,
    writer: Option<Arc<Writer>>,
}

impl List {
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: Vec::new(),
            capacity,
            writer: None,
        }
    }

    pub fn entries(&self) -> &[String] {
        &self.entries
    }

    /// Entries exactly as given, newest first, without `record`'s de-duplication.
    #[cfg(test)]
    pub(crate) fn from_entries(entries: &[&str]) -> Self {
        Self {
            entries: entries.iter().map(|entry| (*entry).to_owned()).collect(),
            capacity: entries.len().max(1),
            writer: None,
        }
    }

    /// Emacs `add-to-history` semantics: skip empty entries and a repeat of the
    /// most recent one, keep other duplicates, and drop the oldest overflow.
    pub fn record(&mut self, entry: &str) {
        if entry.is_empty() || self.entries.first().is_some_and(|first| first == entry) {
            return;
        }
        self.entries.insert(0, entry.to_owned());
        self.entries.truncate(self.capacity);
        if let Some(writer) = &self.writer {
            writer.pending.lock().unwrap().text = Some(self.entries.join("\n"));
            writer.changed.notify_one();
        }
    }

    fn load(path: &Path, capacity: usize, valid: fn(&str) -> bool) -> io::Result<Self> {
        let file = match std::fs::File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(Self::new(capacity));
            }
            Err(error) => return Err(error),
        };
        let mut text = String::new();
        file.take(MAX_FILE_BYTES + 1).read_to_string(&mut text)?;
        if text.len() as u64 > MAX_FILE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "state file exceeds 1 MiB",
            ));
        }
        let entries: Vec<_> = text.lines().take(capacity).map(str::to_owned).collect();
        if entries.iter().any(|entry| !valid(entry)) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid state file entry",
            ));
        }
        Ok(Self {
            entries,
            capacity,
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

/// Owns the writer thread; dropping it flushes the last text and joins.
pub struct Persistence {
    writer: Arc<Writer>,
    worker: Option<JoinHandle<()>>,
}

/// Opens (or starts empty at) `path` and starts mirroring records to it.
pub fn open(
    path: PathBuf,
    capacity: usize,
    valid: fn(&str) -> bool,
) -> io::Result<(List, Persistence)> {
    let mut list = List::load(&path, capacity, valid).unwrap_or_else(|error| {
        eprintln!("semacs: cannot read state file {}: {error}", path.display());
        List::new(capacity)
    });
    let writer = Arc::new(Writer::default());
    let work = writer.clone();
    let worker = std::thread::Builder::new()
        .name("state-file".into())
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
                    eprintln!("semacs: cannot save state file {}: {error}", path.display());
                }
            }
        })?;
    list.writer = Some(writer.clone());
    Ok((
        list,
        Persistence {
            writer,
            worker: Some(worker),
        },
    ))
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

pub fn state_path(name: &str) -> io::Result<PathBuf> {
    state_path_from(
        std::env::var_os("XDG_STATE_HOME").map(PathBuf::from),
        std::env::var_os("HOME").map(PathBuf::from),
        name,
    )
}

fn state_path_from(xdg: Option<PathBuf>, home: Option<PathBuf>, name: &str) -> io::Result<PathBuf> {
    xdg.filter(|path| path.is_absolute())
        .or_else(|| {
            home.filter(|path| path.is_absolute())
                .map(|path| path.join(".local/state"))
        })
        .map(|path| path.join("semacs").join(name))
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

    fn temp_root(tag: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
            ".pi/tmp/persisted-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    fn valid(entry: &str) -> bool {
        !entry.is_empty()
    }

    #[test]
    fn recording_skips_empties_and_consecutive_duplicates_and_drops_the_oldest() {
        let mut list = List::new(3);
        for entry in ["", "alpha", "alpha", "beta", "alpha"] {
            list.record(entry);
        }
        assert_eq!(list.entries(), ["alpha", "beta", "alpha"]);
        list.record("gamma");
        assert_eq!(list.entries(), ["gamma", "alpha", "beta"]);
    }

    #[test]
    fn opens_empty_when_missing_and_roundtrips_through_the_state_file() {
        let root = temp_root("roundtrip");
        let path = root.join("state/history");
        let (mut list, persistence) = open(path.clone(), 100, valid).unwrap();
        assert!(list.entries().is_empty());
        for entry in ["alpha", "beta", "alpha"] {
            list.record(entry);
        }
        drop(persistence);
        assert_eq!(
            List::load(&path, 100, valid).unwrap().entries(),
            ["alpha", "beta", "alpha"]
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn invalid_entries_and_oversized_files_fall_back_to_an_empty_list() {
        let root = temp_root("invalid");
        let path = root.join("state/history");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        for text in [
            String::new(),
            "\n\n".to_owned(),
            vec![b'a'; MAX_FILE_BYTES as usize + 1]
                .into_iter()
                .map(char::from)
                .collect::<String>(),
        ] {
            std::fs::write(&path, &text).unwrap();
            let (list, persistence) = open(path.clone(), 100, valid).unwrap();
            assert!(list.entries().is_empty());
            drop(persistence);
        }
        std::fs::write(&path, "alpha\nbeta").unwrap();
        let (list, persistence) = open(path.clone(), 1, valid).unwrap();
        assert_eq!(list.entries(), ["alpha"]);
        drop(persistence);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn state_paths_do_not_fall_back_to_the_project() {
        assert_eq!(
            state_path_from(Some("/state".into()), None, "history").unwrap(),
            PathBuf::from("/state/semacs/history")
        );
        for xdg in [None, Some("".into()), Some("relative".into())] {
            assert_eq!(
                state_path_from(xdg, Some("/home/user".into()), "history").unwrap(),
                PathBuf::from("/home/user/.local/state/semacs/history")
            );
        }
        assert!(state_path_from(Some("relative".into()), Some(".".into()), "history").is_err());
    }
}
