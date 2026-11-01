//! Command identities and default bindings shared by keymaps and command lookup.

#[derive(Clone, Copy, Debug)]
pub struct DefaultBinding {
    pub keys: &'static str,
    pub reference: &'static str,
}

/// Who owns a command. Keymaps are built per scope and combined as layers, so a
/// target only ever sees the scopes its owner gives it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Scope {
    /// Executed by the edit target itself.
    Edit,
    /// Executed by the application shell: `M-x`, files, buffers, exit.
    Application,
    /// Executed by a minibuffer read / query session.
    ReadSession,
}

#[derive(Clone, Copy, Debug)]
pub struct CommandSpec {
    pub command: Command,
    pub name: &'static str,
    pub scope: Scope,
    pub bindings: &'static [DefaultBinding],
}

macro_rules! commands {
    ($($id:ident => $name:literal, $scope:ident, [$($keys:literal => $reference:literal),*];)*) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Command {
            $($id,)*
        }

        pub const COMMANDS: &[CommandSpec] = &[
            $(CommandSpec {
                command: Command::$id,
                name: $name,
                scope: Scope::$scope,
                bindings: &[$(DefaultBinding { keys: $keys, reference: $reference },)*],
            },)*
        ];

        impl Command {
            pub fn name(self) -> &'static str {
                match self {
                    $(Self::$id => $name,)*
                }
            }

            pub fn scope(self) -> Scope {
                match self {
                    $(Self::$id => Scope::$scope,)*
                }
            }

            pub fn named(name: &str) -> Option<Self> {
                COMMANDS.iter().find(|spec| spec.name == name).map(|spec| spec.command)
            }
        }
    };
}

commands! {
    SelfInsert => "self-insert-command", Edit, [];
    ForwardChar => "forward-char", Edit, ["ctrl-f" => "C-f"];
    BackwardChar => "backward-char", Edit, ["ctrl-b" => "C-b"];
    PreviousLine => "previous-line", Edit, ["ctrl-p" => "C-p"];
    NextLine => "next-line", Edit, ["ctrl-n" => "C-n"];
    MoveBeginningOfLine => "move-beginning-of-line", Edit, ["ctrl-a" => "C-a"];
    MoveEndOfLine => "move-end-of-line", Edit, ["ctrl-e" => "C-e"];
    ForwardWord => "forward-word", Edit, ["alt-f" => "M-f"];
    BackwardWord => "backward-word", Edit, ["alt-b" => "M-b"];
    BeginningOfBuffer => "beginning-of-buffer", Edit, ["alt-<" => "M-<"];
    EndOfBuffer => "end-of-buffer", Edit, ["alt->" => "M->"];
    ScrollUp => "scroll-up-command", Edit, ["ctrl-v" => "C-v"];
    ScrollDown => "scroll-down-command", Edit, ["alt-v" => "M-v"];
    Recenter => "recenter-top-bottom", Edit, ["ctrl-l" => "C-l"];
    Newline => "newline", Edit, ["enter" => "RET", "ctrl-m" => "C-m"];
    ElectricNewline => "electric-newline-and-maybe-indent", Edit, ["ctrl-j" => "C-j"];
    Indent => "indent-for-tab-command", Edit, ["tab" => "TAB", "ctrl-i" => "C-i"];
    DeleteChar => "delete-char", Edit, ["ctrl-d" => "C-d"];
    DeleteForwardChar => "delete-forward-char", Edit, ["delete" => "<deletechar>"];
    DeleteBackwardChar => "delete-backward-char", Edit, ["backspace" => "DEL"];
    KillLine => "kill-line", Edit, ["ctrl-k" => "C-k"];
    KillRegion => "kill-region", Edit, ["ctrl-w" => "C-w"];
    KillRingSave => "kill-ring-save", Edit, ["alt-w" => "M-w"];
    Yank => "yank", Edit, ["ctrl-y" => "C-y"];
    SetMark => "set-mark-command", Edit, ["ctrl-space" => "C-SPC", "ctrl-@" => "C-@"];
    KeyboardQuit => "keyboard-quit", Edit, ["ctrl-g" => "C-g"];
    KeyboardEscapeQuit => "keyboard-escape-quit", Edit, ["escape escape escape" => "ESC ESC ESC"];
    Undo => "undo", Edit, ["ctrl-/" => "C-/", "ctrl-_" => "C-_", "ctrl-x u" => "C-x u"];
    UndoRedo => "undo-redo", Edit, ["ctrl-?" => "C-?", "ctrl-alt-_" => "C-M-_"];
    ExecuteExtendedCommand => "execute-extended-command", Application, ["alt-x" => "M-x"];
    FindFile => "find-file", Application, ["ctrl-x ctrl-f" => "C-x C-f"];
    SaveBuffer => "save-buffer", Application, ["ctrl-x ctrl-s" => "C-x C-s"];
    WriteFile => "write-file", Application, ["ctrl-x ctrl-w" => "C-x C-w"];
    SwitchToBuffer => "switch-to-buffer", Application, ["ctrl-x b" => "C-x b"];
    KillBuffer => "kill-buffer", Application, ["ctrl-x k" => "C-x k"];
    SaveBuffersKillTerminal => "save-buffers-kill-terminal", Application, ["ctrl-x ctrl-c" => "C-x C-c"];
    MinibufferComplete => "minibuffer-complete", ReadSession, ["tab" => "TAB", "ctrl-i" => "C-i"];
    MinibufferCompleteAndExit => "minibuffer-complete-and-exit", ReadSession, ["enter" => "RET", "ctrl-m" => "C-m"];
    AbortMinibuffers => "abort-minibuffers", ReadSession, ["ctrl-g" => "C-g"];
}
