//! Buffer interaction identity, independent of the syntax language.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ModeId(&'static str);

impl ModeId {
    pub const FUNDAMENTAL: Self = Self("fundamental");
    pub const MAGIT_DIFF: Self = Self("magit-diff");
    pub const MAGIT_STATUS: Self = Self("magit-status");
    pub const GIT_COMMIT: Self = Self("git-commit");
    pub const MAGIT_PROCESS: Self = Self("magit-process");
    pub const MAGIT_LOG: Self = Self("magit-log");
    pub const MAGIT_LOG_SELECT: Self = Self("magit-log-select");
    pub const MAGIT_REVISION: Self = Self("magit-revision");

    pub fn name(self) -> &'static str {
        self.0
    }

    pub(crate) fn bindings(self) -> impl Iterator<Item = (&'static str, crate::command::Command)> {
        MODE_BINDINGS
            .iter()
            .filter(move |(mode, _, _)| *mode == self)
            .map(|(_, keys, command)| (*keys, *command))
    }

    pub fn named(name: &str) -> Option<Self> {
        MODES.iter().copied().find(|mode| mode.name() == name)
    }
}

impl Default for ModeId {
    fn default() -> Self {
        Self::FUNDAMENTAL
    }
}

pub const MODES: &[ModeId] = &[
    ModeId::FUNDAMENTAL,
    ModeId::MAGIT_DIFF,
    ModeId::MAGIT_STATUS,
    ModeId::GIT_COMMIT,
    ModeId::MAGIT_PROCESS,
    ModeId::MAGIT_LOG,
    ModeId::MAGIT_LOG_SELECT,
    ModeId::MAGIT_REVISION,
];

const MODE_BINDINGS: &[(ModeId, &str, crate::command::Command)] = &[
    (
        ModeId::MAGIT_LOG,
        "enter",
        crate::command::Command::MagitVisitThing,
    ),
    (
        ModeId::MAGIT_LOG,
        "ctrl-m",
        crate::command::Command::MagitVisitThing,
    ),
    (
        ModeId::MAGIT_LOG_SELECT,
        "enter",
        crate::command::Command::MagitVisitThing,
    ),
    (
        ModeId::MAGIT_LOG_SELECT,
        "ctrl-m",
        crate::command::Command::MagitVisitThing,
    ),
    (
        ModeId::MAGIT_REVISION,
        "enter",
        crate::command::Command::MagitVisitThing,
    ),
    (
        ModeId::MAGIT_REVISION,
        "ctrl-m",
        crate::command::Command::MagitVisitThing,
    ),
    (
        ModeId::MAGIT_REVISION,
        "space",
        crate::command::Command::ScrollUp,
    ),
    (
        ModeId::MAGIT_REVISION,
        "backspace",
        crate::command::Command::ScrollDown,
    ),
    (
        ModeId::MAGIT_REVISION,
        "shift-space",
        crate::command::Command::ScrollDown,
    ),
    (
        ModeId::MAGIT_STATUS,
        "c",
        crate::command::Command::MagitCommit,
    ),
    (
        ModeId::MAGIT_DIFF,
        "c",
        crate::command::Command::MagitCommit,
    ),
    (
        ModeId::MAGIT_PROCESS,
        "c",
        crate::command::Command::MagitCommit,
    ),
    (ModeId::MAGIT_LOG, "c", crate::command::Command::MagitCommit),
    (
        ModeId::MAGIT_REVISION,
        "c",
        crate::command::Command::MagitCommit,
    ),
    (
        ModeId::MAGIT_LOG_SELECT,
        "c",
        crate::command::Command::MagitCommit,
    ),
    (
        ModeId::MAGIT_STATUS,
        "l l",
        crate::command::Command::MagitLogCurrent,
    ),
    (
        ModeId::MAGIT_DIFF,
        "l l",
        crate::command::Command::MagitLogCurrent,
    ),
    (
        ModeId::MAGIT_LOG,
        "l l",
        crate::command::Command::MagitLogCurrent,
    ),
    (
        ModeId::MAGIT_REVISION,
        "l l",
        crate::command::Command::MagitLogCurrent,
    ),
    (
        ModeId::MAGIT_LOG_SELECT,
        "ctrl-c ctrl-c",
        crate::command::Command::MagitLogSelectPick,
    ),
    (
        ModeId::MAGIT_LOG_SELECT,
        ".",
        crate::command::Command::MagitLogSelectPick,
    ),
    (
        ModeId::MAGIT_LOG_SELECT,
        "e",
        crate::command::Command::MagitLogSelectPick,
    ),
    (
        ModeId::MAGIT_LOG_SELECT,
        "ctrl-c ctrl-k",
        crate::command::Command::MagitLogSelectQuit,
    ),
    (
        ModeId::MAGIT_LOG_SELECT,
        "q",
        crate::command::Command::MagitLogSelectQuit,
    ),
    (
        ModeId::MAGIT_LOG,
        "$",
        crate::command::Command::MagitProcessBuffer,
    ),
    (
        ModeId::MAGIT_REVISION,
        "$",
        crate::command::Command::MagitProcessBuffer,
    ),
    (
        ModeId::MAGIT_LOG_SELECT,
        "$",
        crate::command::Command::MagitProcessBuffer,
    ),
    (
        ModeId::MAGIT_DIFF,
        "$",
        crate::command::Command::MagitProcessBuffer,
    ),
    (
        ModeId::MAGIT_STATUS,
        "$",
        crate::command::Command::MagitProcessBuffer,
    ),
    (
        ModeId::MAGIT_PROCESS,
        "$",
        crate::command::Command::MagitProcessBuffer,
    ),
    (
        ModeId::MAGIT_PROCESS,
        "space",
        crate::command::Command::ScrollUp,
    ),
    (
        ModeId::MAGIT_PROCESS,
        "backspace",
        crate::command::Command::ScrollDown,
    ),
    (
        ModeId::MAGIT_PROCESS,
        "shift-space",
        crate::command::Command::ScrollDown,
    ),
    (
        ModeId::MAGIT_STATUS,
        "space",
        crate::command::Command::ScrollUp,
    ),
    (
        ModeId::MAGIT_STATUS,
        "backspace",
        crate::command::Command::ScrollDown,
    ),
    (
        ModeId::MAGIT_STATUS,
        "shift-space",
        crate::command::Command::ScrollDown,
    ),
    (
        ModeId::MAGIT_DIFF,
        "space",
        crate::command::Command::ScrollUp,
    ),
    (
        ModeId::MAGIT_DIFF,
        "backspace",
        crate::command::Command::ScrollDown,
    ),
    (
        ModeId::MAGIT_DIFF,
        "shift-space",
        crate::command::Command::ScrollDown,
    ),
];
