//! Pending destructive actions and the exit traversal policy.

use std::collections::VecDeque;

use crate::buffer::{BufferId, SnapshotId};

pub(crate) enum Closing {
    Kill(BufferId),
    Exit(ExitFlow),
}

pub(crate) enum AfterSave {
    Message,
    Kill,
    ContinueExit,
    Cancel(Option<&'static str>),
}

impl Closing {
    pub fn is_exit(&self) -> bool {
        matches!(self, Self::Exit(_))
    }

    pub fn exit_mut(&mut self) -> Option<&mut ExitFlow> {
        match self {
            Self::Exit(flow) => Some(flow),
            Self::Kill(_) => None,
        }
    }

    pub fn after_save(&mut self, buffer: BufferId, saved: bool, modified: bool) -> AfterSave {
        match self {
            Self::Kill(target) if *target == buffer => {
                if !saved {
                    AfterSave::Cancel(None)
                } else if modified {
                    AfterSave::Cancel(Some("Buffer still modified; not killed"))
                } else {
                    AfterSave::Kill
                }
            }
            Self::Exit(flow) if flow.is_waiting_for(buffer) => {
                flow.clear_waiting_save();
                if !saved {
                    AfterSave::Cancel(None)
                } else if modified {
                    AfterSave::Cancel(Some("Buffer still modified; exit canceled"))
                } else {
                    AfterSave::ContinueExit
                }
            }
            _ => AfterSave::Message,
        }
    }
}

pub(crate) enum ExitBufferState {
    Clean,
    Modified,
    Saving,
}

pub(crate) enum ExitStep {
    Ask(BufferId),
    Wait,
    CheckRemaining,
}

pub(crate) struct ExitFlow {
    remaining: VecDeque<BufferId>,
    waiting_save: Option<BufferId>,
    pub uncertain_git_confirmed: bool,
    pub modified_confirmed: Vec<SnapshotId>,
    pub commits_confirmed: bool,
}

impl ExitFlow {
    pub fn new(remaining: impl IntoIterator<Item = BufferId>) -> Self {
        Self {
            remaining: remaining.into_iter().collect(),
            waiting_save: None,
            uncertain_git_confirmed: false,
            modified_confirmed: Vec::new(),
            commits_confirmed: false,
        }
    }

    pub fn next(&mut self, mut state: impl FnMut(BufferId) -> Option<ExitBufferState>) -> ExitStep {
        while let Some(buffer) = self.remaining.pop_front() {
            match state(buffer) {
                Some(ExitBufferState::Saving) => {
                    self.waiting_save = Some(buffer);
                    return ExitStep::Wait;
                }
                Some(ExitBufferState::Modified) => return ExitStep::Ask(buffer),
                _ => {}
            }
        }
        ExitStep::CheckRemaining
    }

    pub fn wait_for_save(&mut self, buffer: BufferId) {
        self.waiting_save = Some(buffer);
    }

    pub fn clear_waiting_save(&mut self) {
        self.waiting_save = None;
    }

    pub fn is_waiting(&self) -> bool {
        self.waiting_save.is_some()
    }

    fn is_waiting_for(&self, buffer: BufferId) -> bool {
        self.waiting_save == Some(buffer)
    }
}
