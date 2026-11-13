//! `file-name-history` used to order `find-file` / `write-file` candidates.
//!
//! Entries keep the text the user typed, exactly as Emacs stores the minibuffer
//! contents, so `~/` and absolute spellings are distinct history entries.

use std::io;
use std::path::PathBuf;

use gpui_kit::Global;

use crate::persisted::{self, List};

/// Emacs `history-length` default.
const CAPACITY: usize = 100;
const MAX_ENTRY_BYTES: usize = 4096;

pub struct FileHistory {
    list: List,
}

impl Default for FileHistory {
    fn default() -> Self {
        Self {
            list: List::new(CAPACITY),
        }
    }
}

impl Global for FileHistory {}

impl FileHistory {
    pub fn record(&mut self, entry: &str) {
        if valid(entry) {
            self.list.record(entry);
        }
    }

    pub fn entries(&self) -> &[String] {
        self.list.entries()
    }
}

/// Opens the file-name history stored at `path`, starting empty when the file
/// is missing or unreadable.
pub fn open(path: PathBuf) -> io::Result<(FileHistory, persisted::Persistence)> {
    let (list, persistence) = persisted::open(path, CAPACITY, valid)?;
    Ok((FileHistory { list }, persistence))
}

fn valid(entry: &str) -> bool {
    !entry.is_empty() && entry.len() <= MAX_ENTRY_BYTES && !entry.contains(['\n', '\r'])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_raw_input_and_keeps_non_consecutive_duplicates() {
        let mut history = FileHistory::default();
        for entry in ["~/src/a.rs", "~/src/a.rs", "/tmp/b.rs", "~/src/a.rs"] {
            history.record(entry);
        }
        assert_eq!(history.entries(), ["~/src/a.rs", "/tmp/b.rs", "~/src/a.rs"]);
    }

    #[test]
    fn rejects_entries_that_cannot_roundtrip_through_the_state_file() {
        assert!(valid("~/src/a.rs"));
        assert!(valid("a b\tc"));
        for entry in ["", "a\nb", "a\rb", &"a".repeat(MAX_ENTRY_BYTES + 1)] {
            assert!(!valid(entry), "{entry:?}");
        }
    }

    #[gpui_kit::test]
    fn repeated_app_initialization_preserves_shared_history(cx: &mut gpui_kit::TestAppContext) {
        cx.update(crate::app::init);
        cx.update(|cx| cx.global_mut::<FileHistory>().record("~/src/a.rs"));
        cx.update(crate::app::init);
        cx.update(|cx| {
            assert_eq!(cx.global::<FileHistory>().entries(), ["~/src/a.rs"]);
        });
    }

    #[test]
    fn persistence_roundtrips_entries() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
            ".pi/tmp/file-history-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = root.join("state/file-name-history");
        let (mut history, persistence) = open(path.clone()).unwrap();
        for entry in ["~/src/a.rs", "/tmp/b.rs", "~/src/a.rs"] {
            history.record(entry);
        }
        drop(persistence);
        let (history, persistence) = open(path).unwrap();
        assert_eq!(history.entries(), ["~/src/a.rs", "/tmp/b.rs", "~/src/a.rs"]);
        drop(persistence);
        std::fs::remove_dir_all(root).unwrap();
    }
}
