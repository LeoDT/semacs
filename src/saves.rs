//! Per-buffer save ordering, authorization and disk work.

use std::io;
use std::path::{Path, PathBuf};

use crate::buffer::{BufferId, SavePoint};
use crate::files::{self, FileFormat, FileStamp};
use crate::text::Rope;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum SaveIntent {
    Current,
    WriteFile(PathBuf),
}

pub(crate) struct SaveRequest {
    pub generation: u64,
    pub save_point: SavePoint,
    pub intent: SaveIntent,
    pub confirmed: bool,
    pub format: FileFormat,
    pub text: Rope,
}

pub(crate) enum SaveOutcome {
    Written(FileStamp),
    Confirm,
}

impl SaveRequest {
    pub fn write(&self, path: &Path, stamp: Option<FileStamp>) -> io::Result<SaveOutcome> {
        if !self.confirmed {
            let needs_confirmation = match self.intent {
                SaveIntent::Current => disk_changed(path, stamp)?,
                SaveIntent::WriteFile(_) => files::stamp(path)?.is_some(),
            };
            if needs_confirmation {
                return Ok(SaveOutcome::Confirm);
            }
        }
        files::write_atomic(path, &self.text.to_string(), self.format).map(SaveOutcome::Written)
    }
}

pub(crate) struct SaveCompletion {
    pub buffer: BufferId,
    pub request: SaveRequest,
    pub path: PathBuf,
    pub result: io::Result<SaveOutcome>,
}

enum SavePhase {
    Idle,
    Writing {
        generation: u64,
        save_point: SavePoint,
    },
    Confirming(SaveRequest),
}

pub(crate) struct SaveState {
    next_generation: u64,
    phase: SavePhase,
    pending: Option<SaveRequest>,
}

impl Default for SaveState {
    fn default() -> Self {
        Self {
            next_generation: 0,
            phase: SavePhase::Idle,
            pending: None,
        }
    }
}

impl SaveState {
    pub fn is_saving(&self) -> bool {
        !matches!(self.phase, SavePhase::Idle)
    }

    pub(crate) fn next_generation(&mut self) -> u64 {
        self.next_generation = self
            .next_generation
            .checked_add(1)
            .expect("save generation exhausted");
        self.next_generation
    }

    pub(crate) fn start(&mut self, request: &SaveRequest) {
        debug_assert!(matches!(self.phase, SavePhase::Idle));
        self.phase = SavePhase::Writing {
            generation: request.generation,
            save_point: request.save_point,
        };
    }

    pub(crate) fn take_matching(&mut self, request: &SaveRequest) -> bool {
        if matches!(self.phase, SavePhase::Writing { generation, save_point }
            if generation == request.generation && save_point == request.save_point)
        {
            self.phase = SavePhase::Idle;
            true
        } else {
            false
        }
    }

    pub(crate) fn queue(&mut self, mut request: SaveRequest) {
        if request.intent == SaveIntent::Current
            && let Some(previous) = self.pending.take()
        {
            request.intent = previous.intent;
            request.confirmed = previous.confirmed;
        }
        self.pending = Some(request);
    }

    pub(crate) fn take_pending(&mut self) -> Option<SaveRequest> {
        self.pending.take()
    }

    pub(crate) fn await_confirmation(&mut self, request: SaveRequest) {
        debug_assert!(matches!(self.phase, SavePhase::Idle));
        self.phase = SavePhase::Confirming(request);
    }

    pub(crate) fn confirm(&mut self) -> Option<SaveRequest> {
        match std::mem::replace(&mut self.phase, SavePhase::Idle) {
            SavePhase::Confirming(mut request) => {
                request.confirmed = true;
                Some(request)
            }
            phase => {
                self.phase = phase;
                None
            }
        }
    }

    pub(crate) fn cancel_confirmation(&mut self) -> bool {
        if matches!(self.phase, SavePhase::Confirming(_)) {
            self.phase = SavePhase::Idle;
            self.pending = None;
            true
        } else {
            false
        }
    }
}

fn disk_changed(path: &Path, stamp: Option<FileStamp>) -> io::Result<bool> {
    Ok(matches!(files::stamp(path)?, Some(current) if Some(current) != stamp))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(
        state: &mut SaveState,
        point: SavePoint,
        intent: SaveIntent,
        text: &str,
    ) -> SaveRequest {
        SaveRequest {
            generation: state.next_generation(),
            save_point: point,
            intent,
            confirmed: false,
            format: FileFormat::default(),
            text: Rope::from(text),
        }
    }

    #[test]
    fn queue_coalesces_snapshots_without_losing_a_pending_save_as() {
        let point = crate::buffer::Buffer::scratch().begin_save();
        let mut state = SaveState::default();
        let first = request(&mut state, point, SaveIntent::Current, "a");
        state.start(&first);
        let newer = request(&mut state, point, SaveIntent::Current, "b");
        assert!(!state.take_matching(&newer));
        assert!(state.take_matching(&first));
        assert!(!state.is_saving());

        let rename = request(
            &mut state,
            point,
            SaveIntent::WriteFile(PathBuf::from("/tmp/b")),
            "b",
        );
        state.queue(rename);
        let newest = request(&mut state, point, SaveIntent::Current, "c");
        state.queue(newest);
        let pending = state.take_pending().unwrap();
        assert_eq!(pending.text.to_string(), "c");
        assert_eq!(
            pending.intent,
            SaveIntent::WriteFile(PathBuf::from("/tmp/b"))
        );
        state.await_confirmation(pending);
        assert!(state.is_saving());
        assert!(state.confirm().unwrap().confirmed);
        assert!(!state.is_saving());
    }
}
