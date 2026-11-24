use std::sync::{Arc, atomic::AtomicBool};

use gpui_kit::{Context, Entity, Keystroke, Window};

use super::{
    AppShell,
    git::{GitRead, GitTask, operation_id},
};
use crate::command::Command;
use crate::push::{PushContext, parse_remote_target, set_push_remote};
use crate::session::ReadKind;
use crate::view::BufferView;

#[cfg(test)]
#[path = "push_tests.rs"]
mod tests;

#[derive(Clone, Copy)]
enum PushTargetAction {
    Menu,
    PushRemote,
    Upstream,
    Elsewhere,
}

impl AppShell {
    pub(super) fn push_command(
        &mut self,
        command: Command,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let action = match command {
            Command::MagitPush => PushTargetAction::Menu,
            Command::MagitPushCurrentToPushremote => PushTargetAction::PushRemote,
            Command::MagitPushCurrentToUpstream => PushTargetAction::Upstream,
            Command::MagitPushCurrent => PushTargetAction::Elsewhere,
            _ => return,
        };
        if !self.git_available(origin, cx)
            || !origin.read(cx).git_open_ready()
            || self.minibuffer.is_some()
        {
            return;
        }
        let repository = self
            .commit_editor_repository(origin.read(cx).buffer().snapshot_id().buffer)
            .or_else(|| origin.read(cx).buffer().repository());
        let directory = self.default_directory_for(origin, cx);
        let source = origin.read(cx).display_snapshot().id();
        let origin = origin.clone();
        let operation = operation_id();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = cancelled.clone();
        let task = cx.spawn_in(window, async move |shell, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let repository = match repository {
                        Some(repository) => repository,
                        None => crate::git::ReadEnvironment::Commit
                            .discover(&directory, &worker_cancelled)?,
                    };
                    PushContext::load(repository, &worker_cancelled)
                })
                .await;
            shell
                .update_in(cx, |shell, window, cx| {
                    if !shell.finish_git_read(operation, window, cx)
                        || shell.exiting()
                        || shell.minibuffer.is_some()
                        || shell.active_view() != &origin
                        || shell.buffer_index(&origin, cx).is_none()
                        || origin.read(cx).display_snapshot().id() != source
                        || !origin.read(cx).git_open_ready()
                    {
                        return;
                    }
                    match result {
                        Ok(context) => {
                            shell.dispatch_push_context(action, context, &origin, window, cx)
                        }
                        Err(error) => shell.show_message(&origin, error, cx),
                    }
                })
                .ok();
        });
        self.git_task = Some(GitTask::Read(GitRead {
            operation,
            cancelled,
            _task: task,
        }));
    }

    fn dispatch_push_context(
        &mut self,
        action: PushTargetAction,
        context: PushContext,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Err(error) = context.validate_current_branch() {
            self.show_message(origin, error, cx);
            return;
        }
        match action {
            PushTargetAction::Menu => {
                self.begin_read(ReadKind::PushMenu { context }, origin, "", window, cx);
            }
            PushTargetAction::PushRemote => {
                if let Some(remote) = context.push_remote.clone() {
                    self.launch_push_remote(&context, &remote, origin, window, cx);
                } else if context.remotes.is_empty() {
                    self.show_message(origin, "No remotes configured", cx);
                } else {
                    self.begin_read(ReadKind::PushSetRemote { context }, origin, "", window, cx);
                }
            }
            PushTargetAction::Upstream => {
                if let Some(upstream) = context.upstream.clone() {
                    match context.upstream_args(&upstream.remote, &upstream.merge) {
                        Ok((args, label)) => {
                            self.launch_push_process(
                                context.repository,
                                args,
                                label,
                                origin,
                                window,
                                cx,
                            );
                        }
                        Err(error) => self.show_message(origin, error, cx),
                    }
                } else if context.remotes.is_empty() {
                    self.show_message(origin, "No remotes configured", cx);
                } else {
                    self.begin_read(
                        ReadKind::PushSetUpstream { context },
                        origin,
                        "",
                        window,
                        cx,
                    );
                }
            }
            PushTargetAction::Elsewhere => {
                if context.remotes.is_empty() {
                    self.show_message(origin, "No remotes configured", cx);
                } else {
                    self.begin_read(ReadKind::PushElsewhere { context }, origin, "", window, cx);
                }
            }
        }
    }

    pub(super) fn answer_push_choice(
        &mut self,
        stroke: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(session) = &self.session else {
            return false;
        };
        let id = session.id();
        let ReadKind::PushMenu { context } = session.kind().clone() else {
            return false;
        };
        let action = match stroke.key.as_str() {
            "p" => PushTargetAction::PushRemote,
            "u" => PushTargetAction::Upstream,
            "e" => PushTargetAction::Elsewhere,
            _ => {
                self.message_minibuffer(session.kind().answer_hint(), cx);
                return true;
            }
        };
        if let Some(origin) = self.end_session(id, window, cx) {
            self.dispatch_push_context(action, context, &origin, window, cx);
        }
        true
    }

    pub(super) fn confirm_push_read(
        &mut self,
        kind: ReadKind,
        id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = self.session_input(cx).unwrap_or_default();
        match kind {
            ReadKind::PushSetRemote { context } => {
                let remote = input.trim().to_owned();
                if !context.remotes.iter().any(|known| known == &remote) {
                    self.message_minibuffer(
                        format!(
                            "Unknown remote {remote:?}; choose one of: {}",
                            context.remotes.join(", ")
                        ),
                        cx,
                    );
                    return;
                }
                let Some(origin) = self.end_session(id, window, cx) else {
                    return;
                };
                if !self.git_available(&origin, cx) {
                    return;
                }
                let Ok(branch) = context.validate_current_branch().map(ToOwned::to_owned) else {
                    return;
                };
                let repository = context.repository.clone();
                let operation = operation_id();
                let cancelled = Arc::new(AtomicBool::new(false));
                let worker_cancelled = cancelled.clone();
                let task = cx.spawn_in(window, async move |shell, cx| {
                    let result = cx
                        .background_executor()
                        .spawn(async move {
                            set_push_remote(&repository, &branch, &remote, &worker_cancelled)?;
                            Ok::<_, String>(remote)
                        })
                        .await;
                    shell
                        .update_in(cx, |shell, window, cx| {
                            if !shell.finish_git_read(operation, window, cx) || shell.exiting() {
                                return;
                            }
                            match result {
                                Ok(remote) => {
                                    shell.launch_push_remote(&context, &remote, &origin, window, cx)
                                }
                                Err(error) => shell.show_message(&origin, error, cx),
                            }
                        })
                        .ok();
                });
                self.git_task = Some(GitTask::Read(GitRead {
                    operation,
                    cancelled,
                    _task: task,
                }));
            }
            ReadKind::PushSetUpstream { context } => {
                let (remote, target_branch) = match parse_remote_target(&input, &context.remotes) {
                    Ok(parsed) => parsed,
                    Err(error) => {
                        self.message_minibuffer(error, cx);
                        return;
                    }
                };
                let Some(origin) = self.end_session(id, window, cx) else {
                    return;
                };
                match context.set_upstream_args(&remote, &target_branch) {
                    Ok((args, label)) => {
                        self.launch_push_process(
                            context.repository,
                            args,
                            label,
                            &origin,
                            window,
                            cx,
                        );
                    }
                    Err(error) => self.show_message(&origin, error, cx),
                }
            }
            ReadKind::PushElsewhere { context } => {
                let (remote, target_branch) = match parse_remote_target(&input, &context.remotes) {
                    Ok(parsed) => parsed,
                    Err(error) => {
                        self.message_minibuffer(error, cx);
                        return;
                    }
                };
                let Some(origin) = self.end_session(id, window, cx) else {
                    return;
                };
                match context.elsewhere_args(&remote, &target_branch) {
                    Ok((args, label)) => {
                        self.launch_push_process(
                            context.repository,
                            args,
                            label,
                            &origin,
                            window,
                            cx,
                        );
                    }
                    Err(error) => self.show_message(&origin, error, cx),
                }
            }
            _ => {}
        }
    }

    fn launch_push_remote(
        &mut self,
        context: &PushContext,
        remote: &str,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match context.pushremote_args(remote) {
            Ok((args, label)) => {
                self.launch_push_process(
                    context.repository.clone(),
                    args,
                    label,
                    origin,
                    window,
                    cx,
                );
            }
            Err(error) => self.show_message(origin, error, cx),
        }
    }

    fn launch_push_process(
        &mut self,
        repository: crate::git::Repository,
        args: Vec<String>,
        label: String,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.start_push_process(repository, args, label, window, cx);
        self.show_message(origin, "Pushing...", cx);
    }
}
