//! Command identities and default bindings shared by keymaps and command lookup.

#[derive(Clone, Copy, Debug)]
pub struct DefaultBinding {
    pub keys: &'static str,
    pub reference: &'static str,
}

#[derive(Clone, Copy, Debug)]
pub struct CommandSpec {
    pub command: Command,
    pub name: &'static str,
    pub bindings: &'static [DefaultBinding],
}

macro_rules! commands {
    ($($id:ident => $name:literal, [$($keys:literal => $reference:literal),*];)*) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Command {
            $($id,)*
        }

        pub const COMMANDS: &[CommandSpec] = &[
            $(CommandSpec {
                command: Command::$id,
                name: $name,
                bindings: &[$(DefaultBinding { keys: $keys, reference: $reference },)*],
            },)*
        ];

        impl Command {
            pub fn name(self) -> &'static str {
                match self {
                    $(Self::$id => $name,)*
                }
            }

            pub fn named(name: &str) -> Option<Self> {
                COMMANDS.iter().find(|spec| spec.name == name).map(|spec| spec.command)
            }
        }
    };
}

commands! {
    SelfInsert => "self-insert-command", [];
    ForwardChar => "forward-char", ["ctrl-f" => "C-f"];
    BackwardChar => "backward-char", ["ctrl-b" => "C-b"];
    PreviousLine => "previous-line", ["ctrl-p" => "C-p"];
    NextLine => "next-line", ["ctrl-n" => "C-n"];
    MoveBeginningOfLine => "move-beginning-of-line", ["ctrl-a" => "C-a"];
    MoveEndOfLine => "move-end-of-line", ["ctrl-e" => "C-e"];
    ForwardWord => "forward-word", ["alt-f" => "M-f"];
    BackwardWord => "backward-word", ["alt-b" => "M-b"];
    BeginningOfBuffer => "beginning-of-buffer", ["alt-<" => "M-<"];
    EndOfBuffer => "end-of-buffer", ["alt->" => "M->"];
    ScrollUp => "scroll-up-command", ["ctrl-v" => "C-v"];
    ScrollDown => "scroll-down-command", ["alt-v" => "M-v"];
    Recenter => "recenter-top-bottom", ["ctrl-l" => "C-l"];
    Newline => "newline", ["enter" => "RET", "ctrl-m" => "C-m"];
    ElectricNewline => "electric-newline-and-maybe-indent", ["ctrl-j" => "C-j"];
    Indent => "indent-for-tab-command", ["tab" => "TAB", "ctrl-i" => "C-i"];
    DeleteChar => "delete-char", ["ctrl-d" => "C-d"];
    DeleteForwardChar => "delete-forward-char", ["delete" => "<deletechar>"];
    DeleteBackwardChar => "delete-backward-char", ["backspace" => "DEL"];
    KillLine => "kill-line", ["ctrl-k" => "C-k"];
    KillRegion => "kill-region", ["ctrl-w" => "C-w"];
    KillRingSave => "kill-ring-save", ["alt-w" => "M-w"];
    Yank => "yank", ["ctrl-y" => "C-y"];
    SetMark => "set-mark-command", ["ctrl-space" => "C-SPC", "ctrl-@" => "C-@"];
    KeyboardQuit => "keyboard-quit", ["ctrl-g" => "C-g"];
    KeyboardEscapeQuit => "keyboard-escape-quit", ["escape escape escape" => "ESC ESC ESC"];
    Undo => "undo", ["ctrl-/" => "C-/", "ctrl-_" => "C-_", "ctrl-x u" => "C-x u"];
    UndoRedo => "undo-redo", ["ctrl-?" => "C-?", "ctrl-alt-_" => "C-M-_"];
}
