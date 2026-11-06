//! Buffer interaction identity, independent of the syntax language.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ModeId(&'static str);

impl ModeId {
    pub const FUNDAMENTAL: Self = Self("fundamental");
    pub const MAGIT_DIFF: Self = Self("magit-diff");
    pub const MAGIT_STATUS: Self = Self("magit-status");

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
];

const MODE_BINDINGS: &[(ModeId, &str, crate::command::Command)] = &[
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
