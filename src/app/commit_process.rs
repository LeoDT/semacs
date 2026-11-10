use super::{AppShell, CommitPhase, CommitRecord};
use crate::buffer::Buffer;
use crate::commit::Repository;
use crate::section::{Section, Sections};
use crate::session::ReadKind;
use crate::text::{ByteOffset, Rope};
use crate::view::BufferView;
use gpui_kit::{Context, Entity, Window};
use std::fmt::Write;

#[cfg(test)]
#[path = "commit_process_tests.rs"]
mod tests;

impl AppShell {
    pub(super) fn view_commit_process(
        &mut self,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let buffer = origin.read(cx).buffer();
        if let Some(repository) = self
            .commit_editor_repository(buffer.snapshot_id().buffer)
            .or_else(|| buffer.repository())
        {
            self.display_commit_process(repository, window, cx);
            return;
        }
        let directory = self.default_directory_for(origin, cx);
        let input = origin.read(cx).display_snapshot().id().session;
        let origin = origin.clone();
        cx.spawn_in(window, async move |shell, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    crate::git::ReadEnvironment::Commit
                        .discover(&directory, &std::sync::atomic::AtomicBool::new(false))
                })
                .await;
            shell
                .update_in(cx, |shell, window, cx| {
                    if shell.minibuffer.is_some()
                        || shell.active_view() != &origin
                        || shell.buffer_index(&origin, cx).is_none()
                        || origin.read(cx).display_snapshot().id().session != input
                    {
                        return;
                    }
                    match result {
                        Ok(repository) => shell.display_commit_process(repository, window, cx),
                        Err(error) => shell.show_message(&origin, error, cx),
                    }
                })
                .ok();
        })
        .detach();
    }

    fn display_commit_process(
        &mut self,
        repository: Repository,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let index = self
            .buffers
            .iter()
            .position(|slot| slot.buffer.process_repository().as_ref() == Some(&repository));
        let (index, new) = match index {
            Some(index) => (index, false),
            None => (
                self.create_buffer(Buffer::process(repository.clone()), window, cx),
                true,
            ),
        };
        self.refresh_commit_process(&repository, cx);
        self.display_temporary(index, window, cx);
        if self.active_buffer_id() == self.buffers[index].id()
            && (new
                || self
                    .active_view()
                    .read(cx)
                    .buffer()
                    .process_at_point()
                    .is_none())
        {
            let sections = self.buffers[index].buffer.sections().unwrap();
            let point = sections.nodes().last().unwrap().range.start;
            self.active_view().clone().update(cx, |view, cx| {
                view.show_location(point, window, cx);
            });
        }
    }

    pub(super) fn refresh_commit_process(
        &mut self,
        repository: &Repository,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self
            .buffers
            .iter()
            .position(|slot| slot.buffer.process_repository().as_ref() == Some(repository))
        else {
            return;
        };
        let buffer = self.buffers[index].id();
        let end = self.buffers[index].buffer.text().len();
        let views: Vec<_> = self
            .all_views()
            .into_iter()
            .filter(|view| view.read(cx).buffer().snapshot_id().buffer == buffer)
            .map(|view| {
                let follow = view.read(cx).buffer().point().0 == end;
                let anchor = view.read(cx).section_scroll_anchor();
                (view, follow, anchor)
            })
            .collect();
        let (text, sections, operations) = render(
            repository,
            self.commit_records()
                .filter(|record| &record.repository == repository),
        );
        self.buffers[index]
            .buffer
            .install_process(&text, sections, operations);
        self.refresh_buffer(buffer, cx);
        for (view, follow, anchor) in views {
            view.update(cx, |view, cx| {
                if follow {
                    view.show_end(cx);
                } else {
                    view.restore_section_scroll(anchor, cx);
                }
            });
        }
    }

    pub(super) fn request_commit_interrupt(
        &mut self,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(operation) = origin.read(cx).buffer().process_at_point() else {
            self.show_message(origin, "No process at point", cx);
            return;
        };
        let Some(session) = self
            .commits
            .iter()
            .find(|s| s.record.operation == operation)
        else {
            self.show_message(origin, "Process no longer running", cx);
            return;
        };
        if !session.record.phase.is_active() {
            self.show_message(origin, "Process no longer running", cx);
        } else if session.interrupted {
            self.signal_commit(operation, true, origin, cx);
        } else {
            self.begin_read(
                ReadKind::CommitInterrupt { operation },
                origin,
                "",
                window,
                cx,
            );
        }
    }

    pub(super) fn signal_commit(
        &mut self,
        operation: u64,
        kill: bool,
        origin: &Entity<BufferView>,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self
            .commits
            .iter_mut()
            .find(|s| s.record.operation == operation)
        else {
            self.show_message(origin, "Process no longer running", cx);
            return;
        };
        if !session.record.phase.is_active() {
            self.show_message(origin, "Process no longer running", cx);
            return;
        }
        let result = session.process.as_ref().map_or_else(
            || {
                Err(std::io::Error::new(
                    std::io::ErrorKind::NotConnected,
                    "Git is still starting",
                ))
            },
            |process| {
                if kill {
                    process.kill()
                } else {
                    process.interrupt()
                }
            },
        );
        let message = match result {
            Ok(()) => {
                session.interrupted = true;
                if kill {
                    "Killing Git process".to_owned()
                } else {
                    "Interrupting Git process".to_owned()
                }
            }
            Err(error) => format!("Cannot signal Git process: {error}"),
        };
        self.show_message(origin, message, cx);
    }
}

fn render<'a>(
    repository: &Repository,
    records: impl Iterator<Item = &'a CommitRecord>,
) -> (String, Sections, Vec<u64>) {
    let mut text = format!(
        "Git processes: {}\n\n",
        repository.root.to_string_lossy().escape_debug()
    );
    let body = ByteOffset(text.len());
    let mut nodes = vec![Section {
        key: "processes".into(),
        parent: None,
        range: ByteOffset(0)..body,
        body,
        collapsed: false,
    }];
    let mut operations = Vec::new();
    for record in records {
        let start = ByteOffset(text.len());
        let phase = match &record.phase {
            CommitPhase::Starting => "starting".into(),
            CommitPhase::Running => "running".into(),
            CommitPhase::Exited(status) => status.to_string(),
            CommitPhase::Failed { started, .. } => {
                if *started {
                    "failed after start".into()
                } else {
                    "not started".into()
                }
            }
        };
        writeln!(text, "{phase}  git commit  [{}]", record.operation).unwrap();
        let body = ByteOffset(text.len());
        append_output(&mut text, &record.output);
        if !text.ends_with('\n')
            && (!record.diagnostics.is_empty()
                || matches!(record.phase, CommitPhase::Failed { .. }))
        {
            text.push('\n');
        }
        for diagnostic in &record.diagnostics {
            append_output(&mut text, format!("Diagnostic: {diagnostic}\n").as_bytes());
        }
        if let CommitPhase::Failed { diagnostic, .. } = &record.phase {
            append_output(&mut text, format!("{diagnostic}\n").as_bytes());
        }
        if !text.ends_with('\n') {
            text.push('\n');
        }
        text.push('\n');
        nodes.push(Section {
            key: format!("commit:{}", record.operation),
            parent: Some(0),
            range: start..ByteOffset(text.len()),
            body,
            collapsed: false,
        });
        operations.push(record.operation);
    }
    nodes[0].range.end = ByteOffset(text.len());
    let sections = Sections::new(&Rope::from(text.as_str()), nodes)
        .expect("process sections have line-aligned non-overlapping boundaries");
    (text, sections, operations)
}

fn append_output(text: &mut String, bytes: &[u8]) {
    let decoded = String::from_utf8_lossy(bytes).replace("\r\n", "\n");
    for ch in decoded.chars() {
        if ch.is_control() && !matches!(ch, '\n' | '\t') {
            write!(text, "{}", ch.escape_default()).unwrap();
        } else {
            text.push(ch);
        }
    }
}
