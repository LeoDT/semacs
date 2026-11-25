use std::collections::HashMap;
use std::path::PathBuf;

use gpui_kit::component::{ActiveTheme, v_flex};
use gpui_kit::*;

use crate::buffer::Buffer;
use crate::buffers::{BufferSet, BufferSlot};
use crate::command::{Command, Scope};
use crate::command_registry::{CommandId, CommandRegistry};
use crate::editing::KillRing;
use crate::exit::Closing;
use crate::frame;
use crate::keymap::Keymap;
use crate::minibuffer::Minibuffer;
use crate::session::{Answer, ReadKind, ReadSession};
use crate::view::{BufferView, KeyLayers, KeyRoute};

mod appearance;
mod buffer_commands;
mod candidates;
mod closing;
#[cfg(unix)]
mod commit_editor;
#[cfg(unix)]
mod commit_exit;
#[cfg(unix)]
mod commit_process;
#[cfg(unix)]
mod commit_prompt;
#[cfg(unix)]
mod commit_start;
#[cfg(unix)]
mod commits;
#[cfg(unix)]
pub use commits::{CommitPhase, CommitRecord};
#[cfg(unix)]
mod commit_actions;
mod diff;
mod documents;
mod git;
#[cfg(test)]
mod git_test_support;
mod history;
mod index_write;
mod isearch;
#[cfg(unix)]
mod push;
pub use git::records::{GitWritePhase, GitWriteRecord};
pub use index_write::StatusWriteTarget;
mod messages;
mod project;
mod reads;
mod scripting;
#[cfg(test)]
mod scripting_tests;
mod search;
mod status;
mod temporary;
mod tree;
mod windows;

impl Global for KillRing {}

pub fn init(cx: &mut App) {
    crate::appearance::load_bundled_fonts(cx.text_system())
        .expect("failed to load bundled Lilex fonts");
    gpui_kit::init(cx);
    crate::theme::ThemeSettings::default().install(cx);
    cx.default_global::<crate::command_history::CommandHistory>();
    cx.default_global::<crate::file_history::FileHistory>();
    cx.default_global::<crate::project::ProjectList>();
    cx.default_global::<crate::isearch::IsearchRing>();
}

struct MinibufferMount {
    view: Entity<Minibuffer>,
    completion: Option<candidates::CompletionWork>,
    project_files_work: Option<candidates::CompletionWork>,
    project_files: Option<std::sync::Arc<Vec<String>>>,
    candidate_snapshot: Option<crate::input::DisplayId>,
    pending_candidate_action: Option<crate::completion::CandidateAction>,
    _subscription: Subscription,
    _feedback_subscription: Subscription,
}

/// Window-level ownership, command routing and UI composition. Session targets
/// may name inactive buffers; focus always returns to the active body view.
pub struct AppShell {
    buffers: BufferSet,
    messages: crate::messages::Messages,
    echo: Option<(EntityId, SharedString)>,
    panes: crate::panes::Panes<u64>,
    body_panes: HashMap<u64, windows::BodyPane>,
    next_pane: u64,
    tree_window: tree::TreeWindow,
    tree_work: Option<tree::TreeWork>,
    tree_git_work: Option<tree::TreeGitWork>,
    project_scan_task: Option<project::ProjectScanWork>,
    search_work: Option<search::SearchWork>,
    last_search_buffer: Option<crate::buffer::BufferId>,
    isearch: Option<isearch::IsearchSession>,
    body_keymap: Keymap,
    mode_keymaps: HashMap<crate::mode::ModeId, Keymap>,
    commands: CommandRegistry,
    script_host: Option<crate::scripting::ScriptHost>,
    config_generation: u64,
    config_path: Option<PathBuf>,
    reload_task: Option<Task<()>>,
    script_task: Option<Task<()>>,
    git_task: Option<git::GitTask>,
    git_writes: git::records::WriteRecords,
    git_changes: HashMap<crate::git::Repository, u64>,
    git_refreshes: std::collections::VecDeque<git::refresh::Target>,
    #[cfg(unix)]
    commit_editors: HashMap<crate::buffer::BufferId, commit_editor::Editor>,
    #[cfg(unix)]
    commits: Vec<commits::Session>,
    #[cfg(unix)]
    commit_selections: HashMap<crate::buffer::BufferId, commit_actions::Selection>,
    #[cfg(unix)]
    commit_runtime: Option<crate::commit::Runtime>,
    #[cfg(test)]
    save_gate: Option<futures_channel::oneshot::Receiver<()>>,
    #[cfg(test)]
    index_write_gate: Option<futures_channel::oneshot::Receiver<()>>,
    #[cfg(test)]
    index_verify_gate: Option<futures_channel::oneshot::Receiver<()>>,
    #[cfg(test)]
    status_read_gate: Option<futures_channel::oneshot::Receiver<()>>,
    #[cfg(test)]
    history_read_gate: Option<futures_channel::oneshot::Receiver<()>>,
    #[cfg(test)]
    commit_start_gate: Option<futures_channel::oneshot::Receiver<()>>,
    #[cfg(test)]
    repository_refresh_gate: Option<futures_channel::oneshot::Receiver<()>>,
    #[cfg(test)]
    workflow_return_gate: Option<futures_channel::oneshot::Receiver<()>>,
    #[cfg(test)]
    tree_read_gate: Option<futures_channel::oneshot::Receiver<()>>,
    #[cfg(test)]
    project_files_gate: Option<futures_channel::oneshot::Receiver<()>>,
    startup_directory: PathBuf,
    minibuffer: Option<MinibufferMount>,
    session: Option<ReadSession>,
    next_session_id: u64,
    closing: Option<Closing>,
    file_task: Option<Task<()>>,
    opening: Option<documents::Opening>,
    _keys_subscription: Subscription,
    _activation_subscription: Subscription,
}

impl AppShell {
    pub fn new(buffer: Buffer, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::with_configuration(
            buffer,
            None,
            gpui_kit::block_on(crate::scripting::load_defaults()),
            window,
            cx,
        )
    }

    fn new_configured(
        buffer: Buffer,
        loaded: Option<crate::scripting::LoadedConfiguration>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let registry_buffer = buffer.new_view();
        let buffer_id = buffer.snapshot_id().buffer;
        let view = Self::create_view(buffer, window, cx);
        let subscription = cx.observe(&view, Self::view_changed);
        let feedback = cx.subscribe(&view, Self::feedback);
        let pane = windows::BodyPane::new(buffer_id, view.clone(), subscription, feedback);
        let owner = window.window_handle();
        let shell = cx.weak_entity();
        let keys_shell = shell.clone();
        let keys = cx.intercept_keystrokes(move |event, window, cx| {
            if window.window_handle() != owner {
                return;
            }
            let consumed = keys_shell
                .update(cx, |shell, cx| {
                    shell.route_keystroke(
                        &event.keystroke,
                        event.input_preference == InputPreference::CharacterInput,
                        window,
                        cx,
                    )
                })
                .unwrap_or(false);
            if consumed {
                cx.stop_propagation();
            }
        });
        window.on_window_should_close(cx, move |window, cx| {
            shell
                .update(cx, |shell, cx| shell.request_close(window, cx))
                .unwrap_or(true)
        });
        let mut was_active = window.is_window_active();
        let activation = cx.observe_window_activation(window, move |shell, window, cx| {
            let active = window.is_window_active();
            let regained = !was_active && active;
            was_active = active;
            if regained && shell.panes.side().is_some() {
                shell.schedule_tree_git_refresh(200, window, cx);
            }
        });
        view.update(cx, |view, cx| view.restore_input(window, cx));
        let (configuration, script_host) = match loaded {
            Some(loaded) => (loaded.configuration, Some(loaded.host)),
            None => (crate::scripting::Configuration::default(), None),
        };
        Self {
            buffers: BufferSet::new(BufferSlot::new(registry_buffer)),
            messages: crate::messages::Messages::default(),
            echo: None,
            panes: crate::panes::Panes::new(0),
            body_panes: HashMap::from([(0, pane)]),
            next_pane: 1,
            tree_window: tree::TreeWindow::default(),
            tree_work: None,
            tree_git_work: None,
            project_scan_task: None,
            search_work: None,
            last_search_buffer: None,
            isearch: None,
            body_keymap: configuration.body_keymap,
            mode_keymaps: configuration.mode_keymaps,
            commands: configuration.commands,
            script_host,
            config_generation: configuration.generation,
            config_path: None,
            reload_task: None,
            script_task: None,
            git_task: None,
            git_writes: Default::default(),
            git_changes: HashMap::new(),
            git_refreshes: Default::default(),
            #[cfg(unix)]
            commit_editors: HashMap::new(),
            #[cfg(unix)]
            commits: Vec::new(),
            #[cfg(unix)]
            commit_selections: HashMap::new(),
            #[cfg(unix)]
            commit_runtime: None,
            #[cfg(test)]
            save_gate: None,
            #[cfg(test)]
            index_write_gate: None,
            #[cfg(test)]
            index_verify_gate: None,
            #[cfg(test)]
            status_read_gate: None,
            #[cfg(test)]
            history_read_gate: None,
            #[cfg(test)]
            commit_start_gate: None,
            #[cfg(test)]
            repository_refresh_gate: None,
            #[cfg(test)]
            workflow_return_gate: None,
            #[cfg(test)]
            tree_read_gate: None,
            #[cfg(test)]
            project_files_gate: None,
            startup_directory: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            minibuffer: None,
            session: None,
            next_session_id: 0,
            closing: None,
            file_task: None,
            opening: None,
            _keys_subscription: keys,
            _activation_subscription: activation,
        }
    }

    fn create_view(
        buffer: Buffer,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<BufferView> {
        let view = cx.new(|cx| BufferView::new(buffer, cx));
        view.update(cx, |view, cx| {
            view.install_input_lifecycle(window, cx);
            view.suspend_input(cx);
        });
        view
    }

    /// Keyboard routing for the window: the focused target supplies the edit
    /// layer, the shell supplies the layer above it. Commands owned by that
    /// higher layer come back as requests and are executed here, never by the
    /// edit target.
    fn route_keystroke(
        &mut self,
        stroke: &Keystroke,
        prefer_character_input: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.tree_input_activity(window, cx);
        if self.session.is_none()
            && self.minibuffer.is_none()
            && let Some(consumed) =
                self.route_isearch_keystroke(stroke, prefer_character_input, window, cx)
        {
            return consumed;
        }
        if let Some(mount) = &mut self.minibuffer {
            mount.pending_candidate_action = None;
        }
        // A read session owns the keyboard even if focus drifted back to a
        // body view. Re-focusing here keeps platform text input on the
        // minibuffer and makes RET reach the session layer.
        let (view, outer, require_focus) = if let Some(session) = &self.session {
            let Some(minibuffer) = &self.minibuffer else {
                return false;
            };
            let editor = minibuffer.view.read(cx).editor().clone();
            if !minibuffer.view.focus_handle(cx).is_focused(window) {
                editor.update(cx, |view, cx| view.restore_input(window, cx));
            }
            (editor, Some(session.keymap()), false)
        } else if let Some(minibuffer) = self
            .minibuffer
            .as_ref()
            .filter(|minibuffer| minibuffer.view.focus_handle(cx).is_focused(window))
        {
            (minibuffer.view.read(cx).editor().clone(), None, true)
        } else {
            (self.active_view().clone(), Some(&self.body_keymap), true)
        };
        let single_key = self
            .session
            .as_ref()
            .is_some_and(|session| session.kind().is_single_key());
        let layers = if single_key {
            KeyLayers::Exclusive(outer.unwrap())
        } else if self.session.is_none()
            && self
                .minibuffer
                .as_ref()
                .is_none_or(|mount| !mount.view.focus_handle(cx).is_focused(window))
        {
            KeyLayers::Body {
                local: &self.mode_keymaps[&view.read(cx).buffer().mode()],
                global: &self.body_keymap,
            }
        } else {
            KeyLayers::Edit(outer)
        };
        let route = view.update(cx, |view, cx| {
            view.route_key(
                stroke,
                prefer_character_input,
                layers,
                require_focus,
                window,
                cx,
            )
        });
        if single_key && !matches!(route, KeyRoute::Requested(_)) {
            let hint = self.session.as_ref().unwrap().kind().answer_hint();
            self.message_minibuffer(hint, cx);
            return true;
        }
        match route {
            KeyRoute::Unhandled => false,
            KeyRoute::Handled => true,
            KeyRoute::Quit => {
                #[cfg(unix)]
                self.cancel_commit_preparation(window, cx);
                self.cancel_commit_exit(window, cx);
                true
            }
            KeyRoute::Requested(command) => {
                if let CommandId::Native(command) = command
                    && command.scope() == Scope::ReadSession
                {
                    if single_key && command == Command::MinibufferCompleteAndExit {
                        if self.answer_project_switch_menu(stroke, window, cx) {
                            return true;
                        }
                        if self.answer_diff_menu(stroke, window, cx) {
                            return true;
                        }
                        if self.answer_status_jump_menu(stroke, window, cx) {
                            return true;
                        }
                        #[cfg(unix)]
                        if self.answer_commit_choice(stroke, window, cx) {
                            return true;
                        }
                        #[cfg(unix)]
                        if self.answer_push_choice(stroke, window, cx) {
                            return true;
                        }
                        self.answer_query(Answer::key(stroke), window, cx);
                    } else {
                        self.execute_session(command, window, cx);
                    }
                } else {
                    self.run_command(command, &view, window, cx);
                }
                true
            }
        }
    }

    fn execute_application(
        &mut self,
        command: Command,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match command {
            Command::ExecuteExtendedCommand => {
                self.begin_read(ReadKind::Command, origin, "", window, cx);
            }
            Command::MagitStatus => self.open_status(origin, window, cx),
            Command::MagitStatusJump => {
                self.begin_read(
                    ReadKind::StatusJumpMenu { prefix: None },
                    origin,
                    "",
                    window,
                    cx,
                );
            }
            Command::Treemacs => self.toggle_tree(origin, window, cx),
            Command::TreemacsSelectWindow => self.select_tree_window(origin, window, cx),
            Command::TreemacsFindFile => self.find_tree_file(origin, window, cx),
            Command::ProjectSwitchProject
            | Command::ProjectFindFile
            | Command::ProjectSwitchToBuffer
            | Command::ProjectKillBuffers
            | Command::ProjectRememberProject
            | Command::ProjectForgetProject
            | Command::ProjectForgetZombieProjects
            | Command::ProjectRememberProjectsUnder => {
                self.project_command(command, origin, window, cx);
            }
            Command::IsearchForward
            | Command::IsearchBackward
            | Command::IsearchForwardRegexp
            | Command::IsearchBackwardRegexp
            | Command::IsearchForwardWord
            | Command::IsearchForwardSymbol
            | Command::IsearchForwardSymbolAtPoint => {
                self.isearch_command(command, origin, window, cx);
            }
            Command::ProjectFindRegexp
            | Command::Rg
            | Command::RgProject
            | Command::NextError
            | Command::PreviousError
            | Command::CompileGotoError
            | Command::CompilationDisplayError
            | Command::NextErrorNoSelect
            | Command::PreviousErrorNoSelect
            | Command::CompilationNextError
            | Command::CompilationPreviousError
            | Command::CompilationNextFile
            | Command::CompilationPreviousFile
            | Command::Recompile
            | Command::RgRerunChangeRegexp
            | Command::RgRerunChangeLiteral
            | Command::RgRerunChangeDir
            | Command::RgRerunChangeFiles
            | Command::RgRerunToggleCase
            | Command::RgRerunToggleIgnore
            | Command::KillCompilation
            | Command::QuitWindow => {
                self.search_command(command, origin, window, cx);
            }
            Command::TreemacsToggle
            | Command::TreemacsRefresh
            | Command::TreemacsReturn
            | Command::TreemacsReturnStay
            | Command::TreemacsReturnClose
            | Command::TreemacsCollapse
            | Command::TreemacsCollapseParent
            | Command::TreemacsParent
            | Command::TreemacsNext
            | Command::TreemacsPrevious
            | Command::TreemacsNextNeighbour
            | Command::TreemacsPreviousNeighbour
            | Command::TreemacsQuit
            | Command::TreemacsKill
            | Command::TreemacsHidden
            | Command::TreemacsFollow
            | Command::TreemacsWidth
            | Command::TreemacsNarrow
            | Command::TreemacsWiden
            | Command::TreemacsFit
            | Command::TreemacsVisit
            | Command::TreemacsVisitHorizontal
            | Command::TreemacsVisitVertical
            | Command::TreemacsVisitRecent
            | Command::TreemacsVisitClose => {
                self.tree_command(command, origin, window, cx);
            }
            #[cfg(unix)]
            Command::MagitCommitCreate => self.prepare_commit(origin, window, cx),
            #[cfg(unix)]
            Command::MagitCommit
            | Command::MagitCommitAmend
            | Command::MagitCommitReword
            | Command::MagitCommitFixup
            | Command::MagitCommitSquash
            | Command::MagitLogSelectPick
            | Command::MagitLogSelectQuit => {
                self.commit_action_command(command, origin, window, cx)
            }
            #[cfg(unix)]
            Command::MagitPush
            | Command::MagitPushCurrentToPushremote
            | Command::MagitPushCurrentToUpstream
            | Command::MagitPushCurrent => self.push_command(command, origin, window, cx),
            Command::MagitVisitThing
            | Command::MagitDiffVisit
            | Command::MagitDiffVisitOtherWindow
                if origin.read(cx).buffer().status_repository().is_some() =>
            {
                self.visit_status(
                    command == Command::MagitVisitThing,
                    command == Command::MagitDiffVisitOtherWindow,
                    origin,
                    window,
                    cx,
                )
            }
            Command::MagitDiffVisit | Command::MagitDiffVisitOtherWindow
                if origin.read(cx).buffer().history_query().is_some() =>
            {
                self.history_command(command, origin, window, cx)
            }
            Command::MagitLogCurrent
            | Command::MagitVisitThing
            | Command::MagitShowCommit
            | Command::MagitLogMore
            | Command::MagitLogLess
            | Command::MagitDiffShowOrScrollUp
            | Command::MagitDiffShowOrScrollDown
            | Command::MagitVisitFileVersion => self.history_command(command, origin, window, cx),
            #[cfg(unix)]
            Command::MagitProcessBuffer => self.view_commit_process(origin, window, cx),
            #[cfg(unix)]
            Command::MagitProcessKill => self.request_commit_interrupt(origin, window, cx),
            #[cfg(unix)]
            Command::CommitFinish
            | Command::CommitCancel
            | Command::CommitSaveMessage
            | Command::CommitPreviousMessage
            | Command::CommitNextMessage => {
                self.commit_editor_command(command, origin, window, cx);
            }
            Command::MagitDiff
            | Command::MagitDiffDwim
            | Command::MagitDiffRange
            | Command::MagitDiffWorkingTree
            | Command::MagitDiffRefreshMenu
            | Command::MagitDiffSwitchRangeType
            | Command::MagitDiffFlipRevs
            | Command::MagitDiffStaged
            | Command::MagitDiffUnstaged
            | Command::SectionToggle
            | Command::SectionCycle
            | Command::SectionCycleDiffs
            | Command::SectionCycleGlobal
            | Command::SectionForward
            | Command::SectionBackward
            | Command::SectionForwardSibling
            | Command::SectionBackwardSibling
            | Command::SectionUp
            | Command::SectionShowLevel1
            | Command::SectionShowLevel2
            | Command::SectionShowLevel3
            | Command::SectionShowLevel4
            | Command::SectionShowLevel1All
            | Command::SectionShowLevel2All
            | Command::SectionShowLevel3All
            | Command::SectionShowLevel4All
            | Command::MagitDiffJump
            | Command::MagitRefresh
            | Command::MagitDiffVisit
            | Command::MagitDiffVisitOtherWindow
            | Command::MagitDiffVisitWorktree
            | Command::MagitCopySectionValue
            | Command::ScrollLeft
            | Command::ScrollRight => self.diff_command(command, origin, window, cx),
            Command::MagitModeBuryBuffer => self.quit_temporary(window, cx),
            Command::ViewEchoAreaMessages => self.view_messages(window, cx),
            Command::ReloadConfiguration => self.reload_configuration(origin, window, cx),
            Command::LoadTheme => {
                self.begin_read(ReadKind::Theme, origin, "", window, cx);
            }
            Command::SetFontFamily => {
                self.begin_read(ReadKind::FontFamily, origin, "", window, cx);
            }
            Command::IncreaseFontSize | Command::DecreaseFontSize | Command::ResetFontSize => {
                self.change_font_size(command, origin, cx);
            }
            Command::FindFile => {
                self.begin_file_read(ReadKind::FindFile, origin, window, cx);
            }
            Command::SaveBuffer => {
                self.save_buffer(origin, window, cx);
            }
            Command::WriteFile => {
                self.begin_file_read(ReadKind::WriteFile, origin, window, cx);
            }
            Command::SwitchToBuffer => {
                let default = self
                    .buffers
                    .other_buffer(self.active_buffer_id())
                    .and_then(|id| self.buffers.index_of(id))
                    .map(|index| self.buffers[index].buffer.display_name());
                self.begin_read(ReadKind::SwitchToBuffer { default }, origin, "", window, cx);
            }
            Command::PreviousBuffer => self.cycle_buffer(false, window, cx),
            Command::NextBuffer => self.cycle_buffer(true, window, cx),
            Command::KillBuffer => {
                #[cfg(unix)]
                if self
                    .commit_editors
                    .contains_key(&origin.read(cx).buffer().snapshot_id().buffer)
                {
                    self.finish_commit_editor(
                        origin,
                        crate::commit::EditorOutcome::Cancel,
                        true,
                        window,
                        cx,
                    );
                    return;
                }
                let default = origin.read(cx).buffer().display_name();
                self.begin_read(ReadKind::KillBuffer { default }, origin, "", window, cx);
            }
            Command::KillCurrentBuffer => {
                if self.session.is_some() {
                    self.cancel_session(window, cx);
                    return;
                }
                let id = origin.read(cx).buffer().snapshot_id().buffer;
                #[cfg(unix)]
                if self.commit_editors.contains_key(&id) {
                    self.finish_commit_editor(
                        origin,
                        crate::commit::EditorOutcome::Cancel,
                        true,
                        window,
                        cx,
                    );
                    return;
                }
                if let Some(index) = self.buffers.index_of(id) {
                    self.kill_buffer_at(index, window, cx);
                }
            }
            Command::SplitWindowBelow
            | Command::SplitWindowRight
            | Command::OtherWindow
            | Command::DeleteWindow
            | Command::DeleteOtherWindows => {
                self.window_command(command, window, cx);
            }
            Command::SaveBuffersKillTerminal => {
                if self.begin_exit(window, cx) {
                    window.remove_window();
                }
            }
            _ => debug_assert!(false, "{} is not an application command", command.name()),
        }
    }

    fn run_command(
        &mut self,
        command: CommandId,
        target: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let command = match command {
            CommandId::Native(command) => command,
            CommandId::Script(command) => {
                self.execute_script(command, target, window, cx);
                return;
            }
        };
        if !command.available_in(target.read(cx).buffer().mode()) {
            self.show_message(
                target,
                format!("{} is unavailable in this mode", command.name()),
                cx,
            );
            return;
        }
        match command.scope() {
            Scope::Edit => {
                if target.update(cx, |view, cx| view.execute_command(command, window, cx))
                    == KeyRoute::Quit
                {
                    #[cfg(unix)]
                    self.cancel_commit_preparation(window, cx);
                    self.cancel_commit_exit(window, cx);
                }
            }
            Scope::Application => self.execute_application(command, target, window, cx),
            Scope::ReadSession => debug_assert!(false, "{} is not runnable here", command.name()),
        }
    }

    /// Async results remain visible without changing the selected view or read session.
    fn show_message(
        &mut self,
        target: &Entity<BufferView>,
        message: impl Into<String>,
        cx: &mut Context<Self>,
    ) {
        let view = self
            .minibuffer
            .as_ref()
            .map(|mount| mount.view.read(cx).editor().clone())
            .unwrap_or_else(|| {
                if target == self.active_view() {
                    target.clone()
                } else {
                    self.active_view().clone()
                }
            });
        view.update(cx, |view, cx| view.set_message(message, cx));
    }

    pub(crate) fn echo_message(&self, _window: &Window, _cx: &App) -> String {
        self.echo
            .as_ref()
            .map(|(_, text)| text.to_string())
            .unwrap_or_default()
    }

    #[cfg(test)]
    pub(crate) fn echo_text(&self) -> &str {
        self.echo
            .as_ref()
            .map(|(_, text)| text.as_ref())
            .unwrap_or("")
    }
}

impl Render for AppShell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = self.render_body(window, cx);
        let message = self.echo_message(window, cx);
        let echo_area = frame::echo_area(&message, window.viewport_size().height * 0.25, cx);
        let minibuffer = self
            .minibuffer
            .as_ref()
            .map(|minibuffer| minibuffer.view.clone());
        v_flex()
            .id("app")
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(body)
            .overflow_hidden()
            .child(match minibuffer {
                Some(minibuffer) => minibuffer.into_any_element(),
                None => echo_area.into_any_element(),
            })
    }
}
