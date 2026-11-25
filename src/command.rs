//! Command identities and default bindings shared by keymaps and command lookup.

pub const CANDIDATE_BINDINGS: &[(&str, Command)] = &[
    ("ctrl-n", Command::CandidateNext),
    ("down", Command::CandidateNext),
    ("ctrl-p", Command::CandidatePrevious),
    ("up", Command::CandidatePrevious),
    ("alt-enter", Command::CandidateExitInput),
];

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
    pub modes: &'static [&'static str],
}

macro_rules! command_mode {
    () => {
        &[]
    };
    ($($mode:literal),+) => {
        &[$($mode),+]
    };
}

macro_rules! commands {
    ($($id:ident => $name:literal, $scope:ident $(($($mode:literal),+))?, [$($keys:literal => $reference:literal),*];)*) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Command {
            $($id,)*
        }

        pub const COMMANDS: &[CommandSpec] = &[
            $(CommandSpec {
                command: Command::$id,
                name: $name,
                scope: Scope::$scope,
                modes: command_mode!($($($mode),+)?),
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

            pub fn available_in(self, mode: crate::mode::ModeId) -> bool {
                let modes = COMMANDS.iter().find(|spec| spec.command == self).unwrap().modes;
                modes.is_empty() || modes.contains(&mode.name())
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
    ForwardParagraph => "forward-paragraph", Edit, [];
    BackwardParagraph => "backward-paragraph", Edit, [];
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
    KillWord => "kill-word", Edit, ["alt-d" => "M-d"];
    BackwardKillWord => "backward-kill-word", Edit, ["alt-backspace" => "M-DEL"];
    KillLine => "kill-line", Edit, ["ctrl-k" => "C-k"];
    KillRegion => "kill-region", Edit, ["ctrl-w" => "C-w"];
    KillRingSave => "kill-ring-save", Edit, ["alt-w" => "M-w"];
    Yank => "yank", Edit, ["ctrl-y" => "C-y"];
    SetMark => "set-mark-command", Edit, ["ctrl-t" => "C-t", "ctrl-@" => "C-@"];
    KeyboardQuit => "keyboard-quit", Edit, ["ctrl-g" => "C-g"];
    KeyboardEscapeQuit => "keyboard-escape-quit", Edit, ["escape escape escape" => "ESC ESC ESC"];
    Undo => "undo", Edit, ["ctrl-/" => "C-/", "ctrl-_" => "C-_", "ctrl-x u" => "C-x u"];
    UndoRedo => "undo-redo", Edit, ["ctrl-?" => "C-?", "ctrl-alt-_" => "C-M-_"];
    McMarkNextLikeThis => "mc/mark-next-like-this", Edit, ["ctrl->" => "C->"];
    McMarkPreviousLikeThis => "mc/mark-previous-like-this", Edit, ["ctrl-<" => "C-<"];
    McMarkAllLikeThis => "mc/mark-all-like-this", Edit, ["ctrl-c ctrl-<" => "C-c C-<"];
    McEditLines => "mc/edit-lines", Edit, ["ctrl-shift-c ctrl-shift-c" => "C-S-c C-S-c"];
    McSkipToNextLikeThis => "mc/skip-to-next-like-this", Edit, [];
    McSkipToPreviousLikeThis => "mc/skip-to-previous-like-this", Edit, [];
    McUnmarkNextLikeThis => "mc/unmark-next-like-this", Edit, [];
    McUnmarkPreviousLikeThis => "mc/unmark-previous-like-this", Edit, [];
    ExecuteExtendedCommand => "execute-extended-command", Application, ["alt-x" => "M-x"];
    ViewEchoAreaMessages => "view-echo-area-messages", Application, ["ctrl-h e" => "C-h e"];
    ReloadConfiguration => "reload-configuration", Application, [];
    MagitStatus => "magit-status", Application, ["ctrl-x g" => "C-x g"];
    Treemacs => "treemacs", Application, [];
    TreemacsSelectWindow => "treemacs-select-window", Application, [];
    TreemacsFindFile => "treemacs-find-file", Application, [];
    ProjectSwitchProject => "project-switch-project", Application, ["ctrl-x p p" => "C-x p p"];
    ProjectFindFile => "project-find-file", Application, ["ctrl-x p f" => "C-x p f"];
    ProjectFindRegexp => "project-find-regexp", Application, ["ctrl-x p g" => "C-x p g"];
    ProjectSwitchToBuffer => "project-switch-to-buffer", Application, ["ctrl-x p b" => "C-x p b"];
    ProjectKillBuffers => "project-kill-buffers", Application, ["ctrl-x p k" => "C-x p k"];
    ProjectRememberProject => "project-remember-project", Application, [];
    ProjectForgetProject => "project-forget-project", Application, [];
    ProjectForgetZombieProjects => "project-forget-zombie-projects", Application, [];
    ProjectRememberProjectsUnder => "project-remember-projects-under", Application, [];
    Rg => "rg", Application, [];
    RgProject => "rg-project", Application, [];
    IsearchForward => "isearch-forward", Application, ["ctrl-s" => "C-s"];
    IsearchBackward => "isearch-backward", Application, ["ctrl-r" => "C-r"];
    IsearchForwardRegexp => "isearch-forward-regexp", Application, ["ctrl-alt-s" => "C-M-s"];
    IsearchBackwardRegexp => "isearch-backward-regexp", Application, ["ctrl-alt-r" => "C-M-r"];
    IsearchForwardWord => "isearch-forward-word", Application, ["alt-s w" => "M-s w"];
    IsearchForwardSymbol => "isearch-forward-symbol", Application, ["alt-s _" => "M-s _"];
    IsearchForwardSymbolAtPoint => "isearch-forward-symbol-at-point", Application, ["alt-s ." => "M-s ."];
    NextError => "next-error", Application, ["alt-g n" => "M-g n", "alt-g alt-n" => "M-g M-n", "ctrl-x `" => "C-x `"];
    PreviousError => "previous-error", Application, ["alt-g p" => "M-g p", "alt-g alt-p" => "M-g M-p"];
    CompileGotoError => "compile-goto-error", Application("rg"), ["enter" => "RET", "ctrl-m" => "C-m"];
    CompilationDisplayError => "compilation-display-error", Application("rg"), ["ctrl-o" => "C-o"];
    NextErrorNoSelect => "next-error-no-select", Application("rg"), ["n" => "n"];
    PreviousErrorNoSelect => "previous-error-no-select", Application("rg"), ["p" => "p"];
    CompilationNextError => "compilation-next-error", Application("rg"), ["alt-n" => "M-n"];
    CompilationPreviousError => "compilation-previous-error", Application("rg"), ["alt-p" => "M-p"];
    CompilationNextFile => "compilation-next-file", Application("rg"), ["}" => "}", "alt-}" => "M-}"];
    CompilationPreviousFile => "compilation-previous-file", Application("rg"), ["{" => "{", "alt-{" => "M-{"];
    Recompile => "recompile", Application("rg"), ["g" => "g"];
    RgRerunChangeRegexp => "rg-rerun-change-regexp", Application("rg"), ["r" => "r"];
    RgRerunChangeLiteral => "rg-rerun-change-literal", Application("rg"), ["t" => "t"];
    RgRerunChangeDir => "rg-rerun-change-dir", Application("rg"), ["d" => "d"];
    RgRerunChangeFiles => "rg-rerun-change-files", Application("rg"), ["f" => "f"];
    RgRerunToggleCase => "rg-rerun-toggle-case", Application("rg"), ["c" => "c"];
    RgRerunToggleIgnore => "rg-rerun-toggle-ignore", Application("rg"), ["i" => "i"];
    KillCompilation => "kill-compilation", Application("rg"), ["ctrl-c ctrl-k" => "C-c C-k"];
    QuitWindow => "quit-window", Application("rg"), ["q" => "q"];
    TreemacsToggle => "treemacs-TAB-action", Application("treemacs"), ["tab" => "TAB", "ctrl-i" => "C-i"];
    TreemacsRefresh => "treemacs-refresh", Application("treemacs"), ["g" => "g", "r" => "r"];
    TreemacsReturn => "treemacs-RET-action", Application("treemacs"), ["enter" => "RET", "ctrl-m" => "C-m", "l" => "l"];
    TreemacsReturnStay => "semacs-treemacs-RET-stay", Application("treemacs"), ["ctrl-u enter" => "C-u RET", "ctrl-u ctrl-m" => "C-u C-m", "ctrl-u l" => "C-u l"];
    TreemacsReturnClose => "semacs-treemacs-RET-close", Application("treemacs"), ["ctrl-u ctrl-u enter" => "C-u C-u RET", "ctrl-u ctrl-u ctrl-m" => "C-u C-u C-m", "ctrl-u ctrl-u l" => "C-u C-u l"];
    TreemacsCollapse => "treemacs-COLLAPSE-action", Application("treemacs"), ["h" => "h"];
    TreemacsCollapseParent => "treemacs-collapse-parent-node", Application("treemacs"), ["H" => "H"];
    TreemacsNext => "treemacs-next-line", Application("treemacs"), ["n" => "n"];
    TreemacsPrevious => "treemacs-previous-line", Application("treemacs"), ["p" => "p"];
    TreemacsNextNeighbour => "treemacs-next-neighbour", Application("treemacs"), ["alt-n" => "M-n"];
    TreemacsPreviousNeighbour => "treemacs-previous-neighbour", Application("treemacs"), ["alt-p" => "M-p"];
    TreemacsParent => "treemacs-goto-parent-node", Application("treemacs"), ["u" => "u"];
    TreemacsQuit => "treemacs-quit", Application("treemacs"), ["q" => "q"];
    TreemacsKill => "treemacs-kill-buffer", Application("treemacs"), ["Q" => "Q"];
    TreemacsHidden => "treemacs-toggle-show-dotfiles", Application("treemacs"), ["t h" => "t h"];
    TreemacsFollow => "treemacs-follow-mode", Application("treemacs"), ["t f" => "t f"];
    TreemacsWidth => "treemacs-set-width", Application("treemacs"), ["w" => "w"];
    TreemacsNarrow => "treemacs-decrease-width", Application("treemacs"), ["<" => "<"];
    TreemacsWiden => "treemacs-increase-width", Application("treemacs"), [">" => ">"];
    TreemacsFit => "treemacs-fit-window-width", Application("treemacs"), ["=" => "="];
    TreemacsVisit => "treemacs-visit-node-no-split", Application("treemacs"), ["o o" => "o o"];
    TreemacsVisitHorizontal => "treemacs-visit-node-horizontal-split", Application("treemacs"), ["o h" => "o h"];
    TreemacsVisitVertical => "treemacs-visit-node-vertical-split", Application("treemacs"), ["o v" => "o v"];
    TreemacsVisitRecent => "treemacs-visit-node-in-most-recently-used-window", Application("treemacs"), ["o r" => "o r"];
    TreemacsVisitClose => "treemacs-visit-node-close-treemacs", Application("treemacs"), ["o c" => "o c"];
    MagitCommit => "magit-commit", Application, [];
    MagitCommitCreate => "magit-commit-create", Application, [];
    MagitCommitAmend => "magit-commit-amend", Application, [];
    MagitCommitReword => "magit-commit-reword", Application, [];
    MagitCommitFixup => "magit-commit-fixup", Application, [];
    MagitCommitSquash => "magit-commit-squash", Application, [];
    MagitPush => "magit-push", Application, [];
    MagitPushCurrentToPushremote => "magit-push-current-to-pushremote", Application, [];
    MagitPushCurrentToUpstream => "magit-push-current-to-upstream", Application, [];
    MagitPushCurrent => "magit-push-current", Application, [];
    MagitLogCurrent => "magit-log-current", Application, [];
    MagitShowCommit => "magit-show-commit", Application, [];
    MagitVisitThing => "magit-visit-thing", Application("magit-status", "magit-log", "magit-log-select", "magit-revision"), [];
    MagitLogMore => "magit-log-double-commit-limit", Application("magit-log", "magit-log-select"), ["+" => "+"];
    MagitLogLess => "magit-log-half-commit-limit", Application("magit-log", "magit-log-select"), ["-" => "-"];
    MagitLogSelectPick => "magit-log-select-pick", Application("magit-log-select"), [];
    MagitLogSelectQuit => "magit-log-select-quit", Application("magit-log-select"), [];
    MagitVisitFileVersion => "semacs-magit-visit-file-version", Application("magit-revision"), [];
    MagitProcessBuffer => "magit-process-buffer", Application, [];
    MagitProcessKill => "magit-process-kill", Application("magit-process"), ["k" => "k"];
    CommitFinish => "with-editor-finish", Application("git-commit"), ["ctrl-c ctrl-c" => "C-c C-c"];
    CommitCancel => "with-editor-cancel", Application("git-commit"), ["ctrl-c ctrl-k" => "C-c C-k"];
    CommitSaveMessage => "git-commit-save-message", Application("git-commit"), ["ctrl-c alt-s" => "C-c M-s"];
    CommitPreviousMessage => "git-commit-prev-message", Application("git-commit"), ["alt-p" => "M-p"];
    CommitNextMessage => "git-commit-next-message", Application("git-commit"), ["alt-n" => "M-n"];
    MagitDiff => "magit-diff", Application, [];
    MagitDiffDwim => "magit-diff-dwim", Application, [];
    MagitDiffRange => "magit-diff-range", Application, [];
    MagitDiffWorkingTree => "magit-diff-working-tree", Application, [];
    MagitDiffRefreshMenu => "magit-diff-refresh", Application("magit-diff"), ["D" => "D"];
    MagitDiffSwitchRangeType => "magit-diff-switch-range-type", Application("magit-diff"), [];
    MagitDiffFlipRevs => "magit-diff-flip-revs", Application("magit-diff"), [];
    MagitDiffUnstaged => "magit-diff-unstaged", Application, [];
    MagitDiffStaged => "magit-diff-staged", Application, [];
    SectionCycleGlobal => "magit-section-cycle-global", Application("magit-diff", "magit-status", "magit-process", "magit-log", "magit-log-select", "magit-revision", "rg"), ["shift-tab" => "<backtab>"];
    SectionCycle => "magit-section-cycle", Application("magit-diff", "magit-status", "magit-process", "magit-log", "magit-log-select", "magit-revision"), ["ctrl-c tab" => "C-c TAB", "ctrl-c ctrl-i" => "C-c C-i", "ctrl-tab" => "C-<tab>"];
    SectionCycleDiffs => "magit-section-cycle-diffs", Application("magit-diff", "magit-status"), ["alt-tab" => "M-<tab>"];
    SectionToggle => "section-toggle", Application("magit-diff", "magit-status", "magit-process", "magit-log", "magit-log-select", "magit-revision", "rg"), ["tab" => "TAB", "ctrl-i" => "C-i"];
    SectionForward => "section-forward", Application("magit-diff", "magit-status", "magit-process", "magit-log", "magit-log-select", "magit-revision"), ["n" => "n"];
    SectionBackward => "section-backward", Application("magit-diff", "magit-status", "magit-process", "magit-log", "magit-log-select", "magit-revision"), ["p" => "p"];
    SectionForwardSibling => "magit-section-forward-sibling", Application("magit-diff", "magit-status", "magit-process", "magit-log", "magit-log-select", "magit-revision"), ["alt-n" => "M-n"];
    SectionBackwardSibling => "magit-section-backward-sibling", Application("magit-diff", "magit-status", "magit-process", "magit-log", "magit-log-select", "magit-revision"), ["alt-p" => "M-p"];
    SectionUp => "section-up", Application("magit-diff", "magit-status", "magit-process", "magit-log", "magit-log-select", "magit-revision"), ["^" => "^"];
    SectionShowLevel1 => "magit-section-show-level-1", Application("magit-diff", "magit-status", "magit-process", "magit-log", "magit-log-select", "magit-revision"), ["1" => "1"];
    SectionShowLevel2 => "magit-section-show-level-2", Application("magit-diff", "magit-status", "magit-process", "magit-log", "magit-log-select", "magit-revision"), ["2" => "2"];
    SectionShowLevel3 => "magit-section-show-level-3", Application("magit-diff", "magit-status", "magit-process", "magit-log", "magit-log-select", "magit-revision"), ["3" => "3"];
    SectionShowLevel4 => "magit-section-show-level-4", Application("magit-diff", "magit-status", "magit-process", "magit-log", "magit-log-select", "magit-revision"), ["4" => "4"];
    SectionShowLevel1All => "magit-section-show-level-1-all", Application("magit-diff", "magit-status", "magit-process", "magit-log", "magit-log-select", "magit-revision"), ["alt-1" => "M-1"];
    SectionShowLevel2All => "magit-section-show-level-2-all", Application("magit-diff", "magit-status", "magit-process", "magit-log", "magit-log-select", "magit-revision"), ["alt-2" => "M-2"];
    SectionShowLevel3All => "magit-section-show-level-3-all", Application("magit-diff", "magit-status", "magit-process", "magit-log", "magit-log-select", "magit-revision"), ["alt-3" => "M-3"];
    SectionShowLevel4All => "magit-section-show-level-4-all", Application("magit-diff", "magit-status", "magit-process", "magit-log", "magit-log-select", "magit-revision"), ["alt-4" => "M-4"];
    MagitStatusJump => "magit-status-jump", Application("magit-status"), ["j" => "j"];
    MagitDiffJump => "magit-diff-jump", Application("magit-diff"), ["j" => "j"];
    MagitRefresh => "magit-refresh", Application("magit-diff", "magit-status", "magit-log", "magit-log-select", "magit-revision"), ["g" => "g"];
    MagitModeBuryBuffer => "magit-mode-bury-buffer", Application("magit-diff", "magit-status", "magit-process", "magit-log", "magit-log-select", "magit-revision"), ["q" => "q"];
    MagitDiffVisit => "magit-diff-visit-file", Application("magit-diff", "magit-status", "magit-revision"), ["enter" => "RET", "ctrl-m" => "C-m"];
    MagitDiffVisitOtherWindow => "magit-diff-visit-file-other-window", Application("magit-diff", "magit-status", "magit-revision"), ["ctrl-x 4 enter" => "C-x 4 RET", "ctrl-x 4 ctrl-m" => "C-x 4 C-m"];
    MagitDiffVisitWorktree => "magit-diff-visit-worktree-file", Application("magit-diff", "magit-status", "magit-revision"), ["ctrl-enter" => "C-<return>", "ctrl-j" => "C-j"];
    MagitDiffShowOrScrollUp => "magit-diff-show-or-scroll-up", Application("magit-status", "magit-log", "magit-log-select"), [];
    MagitDiffShowOrScrollDown => "magit-diff-show-or-scroll-down", Application("magit-status", "magit-log", "magit-log-select"), [];
    MagitCopySectionValue => "magit-copy-section-value", Application("magit-status", "magit-diff", "magit-log", "magit-log-select", "magit-revision"), ["ctrl-w" => "C-w"];
    ScrollLeft => "scroll-left", Application("magit-diff", "magit-status", "magit-process", "magit-log", "magit-log-select", "magit-revision", "rg"), ["alt-right" => "M-<right>"];
    ScrollRight => "scroll-right", Application("magit-diff", "magit-status", "magit-process", "magit-log", "magit-log-select", "magit-revision", "rg"), ["alt-left" => "M-<left>"];
    LoadTheme => "load-theme", Application, [];
    SetFontFamily => "set-font-family", Application, [];
    IncreaseFontSize => "increase-font-size", Application, [];
    DecreaseFontSize => "decrease-font-size", Application, [];
    ResetFontSize => "reset-font-size", Application, [];
    FindFile => "find-file", Application, ["ctrl-x ctrl-f" => "C-x C-f"];
    SaveBuffer => "save-buffer", Application, ["ctrl-x ctrl-s" => "C-x C-s"];
    WriteFile => "write-file", Application, ["ctrl-x ctrl-w" => "C-x C-w"];
    SwitchToBuffer => "switch-to-buffer", Application, ["ctrl-x b" => "C-x b"];
    PreviousBuffer => "previous-buffer", Application, ["ctrl-x left" => "C-x <left>", "ctrl-x ctrl-left" => "C-x C-<left>"];
    NextBuffer => "next-buffer", Application, ["ctrl-x right" => "C-x <right>", "ctrl-x ctrl-right" => "C-x C-<right>"];
    KillBuffer => "kill-buffer", Application, ["ctrl-x k" => "C-x k"];
    KillCurrentBuffer => "kill-current-buffer", Application, ["ctrl-x ctrl-k" => "C-x C-k"];
    SplitWindowBelow => "split-window-below", Application, ["ctrl-x 2" => "C-x 2"];
    SplitWindowRight => "split-window-right", Application, ["ctrl-x 3" => "C-x 3"];
    OtherWindow => "other-window", Application, ["ctrl-x o" => "C-x o"];
    DeleteWindow => "delete-window", Application, ["ctrl-x 0" => "C-x 0"];
    DeleteOtherWindows => "delete-other-windows", Application, ["ctrl-x 1" => "C-x 1"];
    SaveBuffersKillTerminal => "save-buffers-kill-terminal", Application, ["ctrl-x ctrl-c" => "C-x C-c"];
    MinibufferComplete => "minibuffer-complete", ReadSession, ["tab" => "TAB", "ctrl-i" => "C-i"];
    MinibufferCompleteAndExit => "minibuffer-complete-and-exit", ReadSession, ["enter" => "RET", "ctrl-m" => "C-m"];
    AbortMinibuffers => "abort-minibuffers", ReadSession, ["ctrl-g" => "C-g"];
    CandidateNext => "semacs-candidate-next", ReadSession, [];
    CandidatePrevious => "semacs-candidate-previous", ReadSession, [];
    CandidateExitInput => "semacs-candidate-exit-input", ReadSession, [];
    IsearchEditForward => "isearch-forward-exit-minibuffer", ReadSession, [];
    IsearchEditBackward => "isearch-reverse-exit-minibuffer", ReadSession, [];
}
