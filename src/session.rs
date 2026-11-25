//! Minibuffer read / query sessions and their candidate selection.
//!
//! A session is bound to the target that opened it. Only one may exist at a
//! time; `ReadSession::id` isolates late results from a session that has ended.

use std::path::PathBuf;

use crate::keymap::{Keymap, parse_sequence};
use gpui_kit::{Entity, Keystroke};

use crate::command::{CANDIDATE_BINDINGS, Command, Scope};
use crate::completion::CandidateList;
use crate::view::BufferView;

/// What a confirmed project prompt executes once a project root is chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectAction {
    SwitchMenu,
    FindFile,
    FindRegexp,
    SwitchToBuffer,
    KillBuffers,
    Remember,
    Forget,
}

/// Destination of a confirmed search pattern read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SearchReadTarget {
    Project(crate::project::Project),
    Directory,
    RerunRegexp(crate::buffer::BufferId),
    RerunLiteral(crate::buffer::BufferId),
}

/// What a confirmed session does with the text read in the minibuffer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadKind {
    /// `M-x`: read a command name and run it on the origin.
    Command,
    FontFamily,
    Theme,
    CommitMenu,
    DiffMenu,
    DiffRefreshMenu,
    StatusJumpMenu {
        prefix: Option<char>,
    },
    MagitDiffRange {
        repository: crate::git::Repository,
        environment: crate::git::ReadEnvironment,
        refs: Vec<String>,
        default: Option<String>,
    },
    MagitShowCommit {
        repository: crate::git::Repository,
        environment: crate::git::ReadEnvironment,
        refs: Vec<String>,
        default: Option<String>,
    },
    ProjectSelect {
        action: ProjectAction,
        prompt: String,
        default: Option<String>,
        require_known: bool,
    },
    ProjectDirectory {
        action: ProjectAction,
    },
    ProjectScanDirectory,
    ProjectSwitchMenu {
        project: crate::project::Project,
    },
    ProjectFindFile {
        project: crate::project::Project,
    },
    ProjectSwitchToBuffer {
        project: crate::project::Project,
        default: Option<String>,
    },
    ProjectKillBuffersConfirm {
        project_name: String,
        buffers: Vec<crate::buffer::BufferId>,
    },
    SearchPattern {
        target: SearchReadTarget,
        default: Option<String>,
        prompt: String,
    },
    SearchDirectory {
        query: crate::search::SearchQuery,
        rerun_buffer: Option<crate::buffer::BufferId>,
    },
    SearchFiles {
        buffer: crate::buffer::BufferId,
        query: crate::search::SearchQuery,
    },
    IsearchEdit {
        forward: bool,
        nonincremental: bool,
        prompt: String,
    },
    HistoryFileVersion {
        versions: Vec<crate::history::FileVersion>,
    },
    #[cfg(unix)]
    CommitPublished {
        next: crate::commit::Continuation,
        merges: bool,
        branches: Vec<String>,
        amending: bool,
    },
    #[cfg(unix)]
    CommitTargetMerges {
        next: crate::commit::Continuation,
    },
    /// `C-x C-f`: read a path and visit it from the origin.
    FindFile,
    TreeRoot,
    TreeFile,
    TreeWidth,
    /// `C-x C-w`: read a path and write the buffer there.
    WriteFile,
    /// `C-x b`: read an existing buffer name and activate it.
    SwitchToBuffer {
        default: Option<String>,
    },
    /// A `write-file` target already exists.
    Overwrite(PathBuf),
    /// A visited file changed on disk since it was read or saved.
    ExternalChange(PathBuf),
    /// `M-x revert-buffer`: confirm rereading the visited file from disk.
    RevertBuffer {
        path: PathBuf,
        modified: bool,
    },
    /// `C-x k`: read the name of the buffer to kill.
    KillBuffer {
        default: String,
    },
    /// A modified buffer would be killed.
    KillBufferConfirm {
        name: String,
        batch: bool,
    },
    /// Exit coordination asks whether to save one dirty buffer.
    ExitSave {
        path: Option<PathBuf>,
        name: String,
    },
    /// Dirty buffers remain after the per-buffer answers.
    ExitModified,
    ExitUncertainGit,
    ExitActiveCommits,
    CommitInterrupt {
        operation: u64,
    },
    CommitSecondLine,
    #[cfg(unix)]
    CommitReload {
        snapshot: crate::buffer::SnapshotId,
        modified: bool,
        outcome: Option<crate::commit::EditorOutcome>,
    },
    CommitAll {
        #[cfg(unix)]
        request: crate::commit::Request,
        pane: u64,
        buffer: crate::buffer::BufferId,
    },
    CommitMerge {
        #[cfg(unix)]
        request: crate::commit::Request,
    },
    CommitYesNo {
        operation: u64,
        request: u64,
        prompt: String,
    },
    CommitUsername {
        operation: u64,
        request: u64,
        prompt: String,
    },
    CommitPassword {
        operation: u64,
        request: u64,
        prompt: String,
    },
    #[cfg(unix)]
    PushMenu {
        context: crate::push::PushContext,
    },
    #[cfg(unix)]
    PushSetRemote {
        context: crate::push::PushContext,
    },
    #[cfg(unix)]
    PushSetUpstream {
        context: crate::push::PushContext,
    },
    #[cfg(unix)]
    PushElsewhere {
        context: crate::push::PushContext,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Answer {
    Yes,
    No,
    Save,
    Invalid,
}

impl Answer {
    pub fn full(input: &str) -> Self {
        match input.trim().to_ascii_lowercase().as_str() {
            "yes" => Self::Yes,
            "no" => Self::No,
            _ => Self::Invalid,
        }
    }

    pub fn key(stroke: &Keystroke) -> Self {
        match stroke.key.as_str() {
            "y" | "Y" | "space" => Self::Yes,
            "n" | "N" | "backspace" | "delete" => Self::No,
            "s" | "S" => Self::Save,
            _ => Self::Invalid,
        }
    }
}

impl ReadKind {
    pub fn has_candidates(&self) -> bool {
        matches!(
            self,
            Self::Command
                | Self::HistoryFileVersion { .. }
                | Self::MagitDiffRange { .. }
                | Self::MagitShowCommit { .. }
                | Self::Theme
                | Self::ProjectSelect { .. }
                | Self::ProjectDirectory { .. }
                | Self::ProjectScanDirectory
                | Self::ProjectFindFile { .. }
                | Self::ProjectSwitchToBuffer { .. }
                | Self::SearchDirectory { .. }
                | Self::FindFile
                | Self::TreeRoot
                | Self::TreeFile
                | Self::WriteFile
                | Self::SwitchToBuffer { .. }
                | Self::KillBuffer { .. }
        ) || match self {
            #[cfg(unix)]
            Self::PushSetRemote { .. }
            | Self::PushSetUpstream { .. }
            | Self::PushElsewhere { .. } => true,
            _ => false,
        }
    }

    pub fn candidate_default(&self) -> Option<&str> {
        match self {
            Self::SwitchToBuffer { default }
            | Self::ProjectSwitchToBuffer { default, .. }
            | Self::ProjectSelect { default, .. }
            | Self::MagitDiffRange { default, .. }
            | Self::MagitShowCommit { default, .. } => default.as_deref(),
            Self::KillBuffer { default } => Some(default),
            #[cfg(unix)]
            Self::PushSetRemote { context } => context.remotes.first().map(String::as_str),
            #[cfg(unix)]
            Self::PushSetUpstream { context } | Self::PushElsewhere { context } => {
                context.targets.first().map(String::as_str)
            }
            _ => None,
        }
    }

    pub fn is_secret(&self) -> bool {
        matches!(self, Self::CommitPassword { .. })
    }

    pub fn is_single_key(&self) -> bool {
        matches!(
            self,
            Self::Overwrite(_)
                | Self::KillBufferConfirm { .. }
                | Self::ExitSave { .. }
                | Self::CommitSecondLine
                | Self::CommitAll { .. }
                | Self::CommitMerge { .. }
                | Self::CommitInterrupt { .. }
                | Self::CommitMenu
                | Self::DiffMenu
                | Self::DiffRefreshMenu
                | Self::StatusJumpMenu { .. }
                | Self::ProjectSwitchMenu { .. }
        ) || match self {
            #[cfg(unix)]
            Self::CommitPublished { .. }
            | Self::CommitTargetMerges { .. }
            | Self::PushMenu { .. } => true,
            _ => false,
        }
    }

    pub fn answer_hint(&self) -> &'static str {
        match self {
            Self::KillBufferConfirm { .. } => "Please answer y, n, or s",
            Self::CommitMenu => "Choose c, a, w, f, s; C-g cancels",
            #[cfg(unix)]
            Self::PushMenu { .. } => "Choose p, u, e; C-g cancels",
            Self::DiffMenu => "Choose d, r, u, s, w, c; C-g cancels",
            Self::DiffRefreshMenu => "Choose r, f, g; C-g cancels",
            Self::StatusJumpMenu { prefix: None } => "Choose n, u, s, m, z, p, f; C-g cancels",
            Self::StatusJumpMenu { prefix: Some(_) } => "Choose u, p; C-g cancels",
            Self::ProjectSwitchMenu { .. } => {
                "Unrecognized input; choose f, g, b, v, t; C-g cancels"
            }
            #[cfg(unix)]
            Self::CommitTargetMerges { .. } => "Choose c (continue), s (select other), a (abort)",
            _ if self.is_single_key() => "Please answer y or n",
            _ => "Please answer yes or no",
        }
    }

    pub fn prompt(&self) -> String {
        match self {
            ReadKind::Command => "M-x ".to_owned(),
            ReadKind::FontFamily => "Font family: ".to_owned(),
            ReadKind::Theme => "Load theme: ".to_owned(),
            ReadKind::CommitMenu => "Commit: [c] create  [a] amend  [w] reword  [f] fixup  [s] squash; C-g cancels: ".into(),
            #[cfg(unix)]
            ReadKind::PushMenu { context } => {
                let branch = context.branch.as_deref().unwrap_or("(detached)");
                let push_remote = context.push_remote.as_deref().unwrap_or("unset");
                let upstream = context
                    .upstream
                    .as_ref()
                    .map_or("unset", |upstream| upstream.short.as_str());
                format!(
                    "Push {branch}: [p] pushRemote ({push_remote})  [u] upstream ({upstream})  [e] elsewhere; C-g cancels: "
                )
            }
            #[cfg(unix)]
            ReadKind::PushSetRemote { context } => format!(
                "Set branch.{}.pushRemote and push there: ",
                context.branch.as_deref().unwrap_or("")
            ),
            #[cfg(unix)]
            ReadKind::PushSetUpstream { context } => format!(
                "Set upstream of {} and push there: ",
                context.branch.as_deref().unwrap_or("")
            ),
            #[cfg(unix)]
            ReadKind::PushElsewhere { context } => format!(
                "Push {} to: ",
                context.branch.as_deref().unwrap_or("")
            ),
            ReadKind::DiffMenu => "Diff: [d] dwim  [r] range  [u] unstaged  [s] staged  [w] worktree  [c] show commit; C-g cancels: ".into(),
            ReadKind::DiffRefreshMenu => "Diff refresh: [r] switch range type  [f] flip revisions  [g] refresh; C-g cancels: ".into(),
            ReadKind::StatusJumpMenu { prefix: None } => "Jump to [n] untracked  [u] unstaged  [s] staged  [m] unmerged  [z] stashes  [p] unpushed/recent  [f] unpulled; C-g cancels: ".into(),
            ReadKind::StatusJumpMenu { prefix: Some('p') } => "Jump to unpushed: [u] upstream/recent  [p] pushremote; C-g cancels: ".into(),
            ReadKind::StatusJumpMenu { prefix: Some(_) } => "Jump to unpulled: [u] upstream  [p] pushremote; C-g cancels: ".into(),
            ReadKind::MagitDiffRange { default, .. } => match default {
                Some(name) => format!("Diff for range (default {name}): "),
                None => "Diff for range: ".to_owned(),
            },
            ReadKind::MagitShowCommit { default, .. } => match default {
                Some(name) => format!("Show commit (default {name}): "),
                None => "Show commit: ".to_owned(),
            },
            ReadKind::ProjectSelect { prompt, .. } => prompt.clone(),
            ReadKind::ProjectDirectory { .. } => "Select directory: ".to_owned(),
            ReadKind::ProjectScanDirectory => "Directory: ".to_owned(),
            ReadKind::ProjectSwitchMenu { project } => format!(
                "Command in `{}': [f] Find file  [g] Find regexp  [b] Switch buffer  [v] Magit status  [t] Treemacs ",
                crate::files::directory_input(&project.root)
            ),
            ReadKind::ProjectFindFile { project } => format!(
                "Find file in {}: ",
                crate::files::directory_input(&project.root)
            ),
            ReadKind::ProjectSwitchToBuffer { default, .. } => match default {
                Some(name) => format!("Switch to buffer (default {name}): "),
                None => "Switch to buffer: ".to_owned(),
            },
            ReadKind::ProjectKillBuffersConfirm {
                project_name,
                buffers,
            } => format!(
                "Kill {} buffers in {}? (yes or no) ",
                buffers.len(),
                project_name
            ),
            ReadKind::SearchPattern { prompt, .. } => prompt.clone(),
            ReadKind::SearchDirectory { .. } => "Search in directory: ".to_owned(),
            ReadKind::SearchFiles { .. } => {
                "Files glob (empty for all, e.g. *.rs !target/*): ".to_owned()
            }
            ReadKind::IsearchEdit { prompt, .. } => prompt.clone(),
            ReadKind::HistoryFileVersion { .. } => "Visit file version: ".into(),
            #[cfg(unix)]
            ReadKind::CommitPublished { branches, amending, .. } => format!("{} already been published to {}. Do you really want to modify {}? (y or n) ", if *amending { "This commit has" } else { "Some of these commits have" }, branches.join(", "), if *amending { "it" } else { "them" }),
            #[cfg(unix)]
            ReadKind::CommitTargetMerges { .. } => "Proceed despite merge in rebase range? [c] continue, [s] select other, [a] abort: ".into(),
            ReadKind::FindFile => "Find file: ".to_owned(),
            ReadKind::TreeRoot => "Project root: ".to_owned(),
            ReadKind::TreeFile => "File to find: ".to_owned(),
            ReadKind::TreeWidth => "New width: ".to_owned(),
            ReadKind::WriteFile => "Write file: ".to_owned(),
            ReadKind::SwitchToBuffer { default } => match default {
                Some(name) => format!("Switch to buffer (default {name}): "),
                None => "Switch to buffer: ".to_owned(),
            },
            ReadKind::Overwrite(path) => {
                format!("File ‘{}’ exists; overwrite? (y or n) ", path.display())
            }
            ReadKind::ExternalChange(path) => format!(
                "{} has changed since visited or saved.  Save anyway? (yes or no) ",
                path.display()
            ),
            ReadKind::RevertBuffer { path, modified } => {
                if *modified {
                    format!(
                        "Discard edits and reread from {}? (yes or no) ",
                        path.display()
                    )
                } else {
                    format!("Revert buffer from file {}? (yes or no) ", path.display())
                }
            }
            ReadKind::KillBuffer { default } => {
                format!("Kill buffer (default {default}): ")
            }
            ReadKind::KillBufferConfirm { name, .. } => {
                format!("Buffer {name} modified; kill anyway? (y, n, s) ")
            }
            ReadKind::ExitSave { path, name } => match path {
                Some(path) => format!("Save file {}? (y or n) ", path.display()),
                None => format!("Save buffer {name}? (y or n) "),
            },
            ReadKind::CommitYesNo { prompt, .. }
            | ReadKind::CommitUsername { prompt, .. }
            | ReadKind::CommitPassword { prompt, .. } => prompt.clone(),
            ReadKind::CommitInterrupt { .. } => "Kill process? (y or n) ".to_owned(),
            ReadKind::CommitSecondLine => "Second line is not empty. Commit anyway? (y or n) ".to_owned(),
            #[cfg(unix)]
            ReadKind::CommitReload { modified, .. } => {
                if *modified {
                    "Git editor file changed on disk. Discard your edits and reread? (yes or no) "
                } else {
                    "Git editor file changed on disk. Reread from disk? (yes or no) "
                }.to_owned()
            }
            ReadKind::CommitAll { .. } => "Nothing staged.  Commit all uncommitted changes? (y or n) ".to_owned(),
            ReadKind::CommitMerge { .. } => "Proceed with merge despite unstaged changes? (y or n) ".to_owned(),
            ReadKind::ExitModified => {
                "Modified buffers exist; exit anyway? (yes or no) ".to_owned()
            }
            ReadKind::ExitActiveCommits => {
                "Active Git processes exist; kill them and exit anyway? (yes or no) ".to_owned()
            }
            ReadKind::ExitUncertainGit => {
                "A Git write result is uncertain; inspect *Messages* and repository. Exit anyway? (yes or no) ".to_owned()
            }
        }
    }
}

/// One minibuffer read session.
pub struct ReadSession {
    id: u64,
    kind: ReadKind,
    origin: Entity<BufferView>,
    keymap: Keymap,
    /// Input the session opened with, used to recognise an unedited answer
    /// such as the default directory of a file read.
    initial: String,
    pub(crate) candidates: Option<CandidateList>,
}

impl ReadSession {
    pub fn new(id: u64, kind: ReadKind, origin: Entity<BufferView>, initial: String) -> Self {
        let mut keymap = if kind.is_single_key() {
            let mut keymap = Keymap::default();
            let keys: &[&str] = match &kind {
                ReadKind::CommitMenu => &["c", "a", "w", "f", "s"],
                #[cfg(unix)]
                ReadKind::PushMenu { .. } => &["p", "u", "e"],
                ReadKind::DiffMenu => &["d", "r", "u", "s", "w", "c"],
                ReadKind::DiffRefreshMenu => &["r", "f", "g"],
                ReadKind::StatusJumpMenu { prefix: None } => &["n", "u", "s", "m", "z", "p", "f"],
                ReadKind::StatusJumpMenu { prefix: Some(_) } => &["u", "p"],
                ReadKind::ProjectSwitchMenu { .. } => &["f", "g", "b", "v", "t"],
                #[cfg(unix)]
                ReadKind::CommitTargetMerges { .. } => &["c", "s", "a"],
                _ => &["y", "shift-y", "n", "shift-n", "enter", "ctrl-m"],
            };
            for key in keys {
                keymap
                    .bind(
                        &parse_sequence(key).unwrap(),
                        Command::MinibufferCompleteAndExit,
                    )
                    .unwrap();
            }
            let extra: &[&str] = match &kind {
                ReadKind::KillBufferConfirm { .. } => &["s", "shift-s"],
                ReadKind::CommitMenu
                | ReadKind::DiffMenu
                | ReadKind::DiffRefreshMenu
                | ReadKind::StatusJumpMenu { .. }
                | ReadKind::ProjectSwitchMenu { .. } => &[],
                #[cfg(unix)]
                ReadKind::CommitTargetMerges { .. } | ReadKind::PushMenu { .. } => &[],
                _ => &["space", "backspace", "delete"],
            };
            for key in extra {
                keymap
                    .bind(
                        &parse_sequence(key).unwrap(),
                        Command::MinibufferCompleteAndExit,
                    )
                    .unwrap();
            }
            keymap
                .bind(
                    &parse_sequence("ctrl-g").unwrap(),
                    Command::AbortMinibuffers,
                )
                .unwrap();
            keymap
        } else {
            Keymap::for_scope(Scope::ReadSession)
        };
        keymap
            .bind(
                &parse_sequence("ctrl-x ctrl-k").unwrap(),
                Command::KillCurrentBuffer,
            )
            .unwrap();
        if kind.has_candidates() {
            for &(key, command) in CANDIDATE_BINDINGS {
                keymap.bind(&parse_sequence(key).unwrap(), command).unwrap();
            }
        }
        if matches!(kind, ReadKind::IsearchEdit { .. }) {
            keymap
                .bind(
                    &parse_sequence("ctrl-s").unwrap(),
                    Command::IsearchEditForward,
                )
                .unwrap();
            keymap
                .bind(
                    &parse_sequence("ctrl-r").unwrap(),
                    Command::IsearchEditBackward,
                )
                .unwrap();
        }
        let candidates = kind.has_candidates().then(CandidateList::default);
        Self {
            id,
            kind,
            origin,
            keymap,
            initial,
            candidates,
        }
    }

    pub fn candidates(&self) -> Option<&CandidateList> {
        self.candidates.as_ref()
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn kind(&self) -> &ReadKind {
        &self.kind
    }

    pub fn origin(&self) -> &Entity<BufferView> {
        &self.origin
    }

    pub fn initial(&self) -> &str {
        &self.initial
    }

    pub(crate) fn keymap(&self) -> &Keymap {
        &self.keymap
    }
}
