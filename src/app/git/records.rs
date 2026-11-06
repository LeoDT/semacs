use crate::buffer::BufferId;
use crate::status::{
    Entry, Group, Head, Repository,
    index::{Operation, Outcome, Request, bounded},
};
use std::collections::VecDeque;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GitWritePhase {
    Accepted,
    Executing,
    Verifying,
    Complete,
}

#[derive(Debug)]
pub struct GitWriteRecord {
    pub id: u64,
    pub repository: Repository,
    pub path: PathBuf,
    pub operation: Operation,
    pub group: Group,
    pub expected_head: Head,
    pub expected_entry: Entry,
    pub buffer: BufferId,
    pub request: u64,
    pub phase: GitWritePhase,
    pub outcome: Option<Outcome>,
    pub diagnostic: String,
    pub verified: Option<bool>,
    pub installed: bool,
}

#[derive(Default)]
pub(in crate::app) struct WriteRecords {
    entries: VecDeque<GitWriteRecord>,
    uncertain: bool,
}

impl WriteRecords {
    pub fn iter(&self) -> impl Iterator<Item = &GitWriteRecord> {
        self.entries.iter()
    }

    pub fn accept(&mut self, id: u64, target: &Request, buffer: BufferId, request: u64) {
        self.entries.push_back(GitWriteRecord {
            id,
            repository: target.repository().clone(),
            path: target.path().to_owned(),
            operation: target.operation(),
            group: target.group(),
            expected_head: target.expected_head().clone(),
            expected_entry: target.expected_entry().clone(),
            buffer,
            request,
            phase: GitWritePhase::Accepted,
            outcome: None,
            diagnostic: String::new(),
            verified: None,
            installed: false,
        });
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut GitWriteRecord> {
        self.entries.iter_mut().find(|record| record.id == id)
    }

    pub fn complete(&mut self, id: u64) {
        if let Some(record) = self.get_mut(id) {
            record.phase = GitWritePhase::Complete;
            if record.outcome == Some(Outcome::Uncertain) {
                self.uncertain = true;
            }
        }
        while self
            .entries
            .iter()
            .filter(|record| record.phase == GitWritePhase::Complete)
            .count()
            > 64
        {
            let oldest = self
                .entries
                .iter()
                .position(|record| record.phase == GitWritePhase::Complete)
                .unwrap();
            self.entries.remove(oldest);
        }
    }

    pub fn uncertain(&self) -> bool {
        self.uncertain
    }
}

impl GitWriteRecord {
    pub(in crate::app) fn summary(&self) -> String {
        let outcome = match self.outcome {
            Some(Outcome::NotExecuted) => "not executed",
            Some(Outcome::Succeeded) => "write succeeded",
            Some(Outcome::Failed) => "execution failed (side effects may have occurred)",
            Some(Outcome::Uncertain) => "result uncertain; do not automatically retry",
            None => "pending",
        };
        let verification = match (self.verified, self.installed) {
            (Some(true), true) => "; status refreshed",
            (Some(true), false) => "; repository checked; original status session unavailable",
            (Some(false), _) => "; refresh failed; old projections are stale",
            _ => "",
        };
        // Leave history space for the terminal summary and subsequent exit-coordination messages.
        let mut lines = self.diagnostic.lines();
        let diagnostic = lines
            .by_ref()
            .take((crate::messages::LOG_LINES / 2) as usize)
            .collect::<Vec<_>>()
            .join("\n");
        let truncated = if lines.next().is_some() {
            "\n[diagnostic lines truncated; bounded diagnostic retained in core record]"
        } else {
            ""
        };
        bounded(format!(
            "Git #{} {:?}: {outcome}{verification}\nTarget {:?}{}{}{truncated}",
            self.id,
            self.operation,
            self.path,
            if diagnostic.is_empty() { "" } else { "\n" },
            diagnostic
        ))
    }
}
