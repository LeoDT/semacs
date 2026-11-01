//! Pending destructive actions and the exit traversal policy.

use std::collections::VecDeque;

use gpui_kit::Entity;

use crate::view::BufferView;

pub(crate) enum Closing {
    Kill(Entity<BufferView>),
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

    pub fn after_save(
        &mut self,
        view: &Entity<BufferView>,
        saved: bool,
        modified: bool,
    ) -> AfterSave {
        match self {
            Self::Kill(target) if target == view => {
                if !saved {
                    AfterSave::Cancel(None)
                } else if modified {
                    AfterSave::Cancel(Some("Buffer still modified; not killed"))
                } else {
                    AfterSave::Kill
                }
            }
            Self::Exit(flow) if flow.is_waiting_for(view) => {
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
    Ask(Entity<BufferView>),
    Wait,
    CheckRemaining,
}

pub(crate) struct ExitFlow {
    remaining: VecDeque<Entity<BufferView>>,
    waiting_save: Option<Entity<BufferView>>,
}

impl ExitFlow {
    pub fn new(remaining: impl IntoIterator<Item = Entity<BufferView>>) -> Self {
        Self {
            remaining: remaining.into_iter().collect(),
            waiting_save: None,
        }
    }

    pub fn next(
        &mut self,
        mut state: impl FnMut(&Entity<BufferView>) -> Option<ExitBufferState>,
    ) -> ExitStep {
        while let Some(view) = self.remaining.pop_front() {
            match state(&view) {
                Some(ExitBufferState::Saving) => {
                    self.waiting_save = Some(view);
                    return ExitStep::Wait;
                }
                Some(ExitBufferState::Modified) => return ExitStep::Ask(view),
                _ => {}
            }
        }
        ExitStep::CheckRemaining
    }

    pub fn wait_for_save(&mut self, view: Entity<BufferView>) {
        self.waiting_save = Some(view);
    }

    pub fn clear_waiting_save(&mut self) {
        self.waiting_save = None;
    }

    fn is_waiting_for(&self, view: &Entity<BufferView>) -> bool {
        self.waiting_save.as_ref() == Some(view)
    }
}
