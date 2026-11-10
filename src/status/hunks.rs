use super::{Group, MAX_BYTES, Record, Status};
use crate::diff::{Comparison, Diff, DiffKind, ReadEnvironment};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

impl Status {
    pub(super) fn load_diffs(&mut self, cancelled: &AtomicBool) -> Result<(), String> {
        let mut budget = MAX_BYTES;
        let mut lines = 100_000usize;
        for (group, kind) in [
            (Group::Unstaged, DiffKind::Unstaged),
            (Group::Staged, DiffKind::Staged),
        ] {
            let targets: std::collections::HashSet<_> = self
                .group(group)
                .map(|(_, entry)| entry.path.clone())
                .collect();
            let mut batches: Vec<Vec<PathBuf>> = Vec::new();
            let mut size = 0;
            for (_, entry) in self.group(group) {
                if entry.submodule.is_submodule || !matches!(entry.record, Record::Tracked { .. }) {
                    continue;
                }
                let mut paths = vec![entry.path.clone()];
                if let Record::Tracked {
                    rename: Some(rename),
                    ..
                } = &entry.record
                {
                    paths.push(rename.source.clone());
                }
                let bytes = paths
                    .iter()
                    .map(|path| path.as_os_str().len() + 1)
                    .sum::<usize>();
                if batches.is_empty() || size + bytes > 32 * 1024 {
                    batches.push(Vec::new());
                    size = 0;
                }
                batches.last_mut().unwrap().extend(paths);
                size += bytes;
            }
            for paths in batches {
                let (diff, bytes) = Diff::load_status(
                    Comparison {
                        repository: self.repository.clone(),
                        kind,
                        environment: ReadEnvironment::Projection,
                    },
                    &paths,
                    cancelled,
                )?;
                budget = budget
                    .checked_sub(bytes)
                    .ok_or("Status diffs exceed the 16 MiB input limit")?;
                for file in diff.files {
                    let count = file.hunks.iter().map(|hunk| hunk.lines.len() + 1).sum();
                    lines = lines
                        .checked_sub(count)
                        .ok_or("Status diffs exceed the 100000 patch line limit")?;
                    if targets.contains(&file.new_path) {
                        self.diffs
                            .entry(group)
                            .or_default()
                            .insert(file.new_path.clone(), std::sync::Arc::new(file));
                    }
                }
            }
        }
        Ok(())
    }
}
