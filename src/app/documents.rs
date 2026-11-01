use super::AppShell;
use crate::buffer::Buffer;
use crate::files::{self, ReadError};
use crate::saves::{SaveCompletion, SaveIntent, SaveOutcome, SaveRequest};
use crate::session::ReadKind;
use crate::view::BufferView;
use gpui_kit::{Context, Entity, Window};
use std::io;
use std::path::{Path, PathBuf};

impl AppShell {
    pub(super) fn visit_file(
        &mut self,
        path: PathBuf,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let existing = self
            .buffers
            .iter()
            .position(|slot| slot.view.read(cx).buffer().path() == Some(path.as_path()));
        if let Some(index) = existing {
            self.activate(index, window, cx);
            return;
        }
        let origin = origin.clone();
        self.file_task = Some(cx.spawn_in(window, async move |shell, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let buffer = match files::load(&path) {
                        Ok(loaded) => Buffer::visiting(path.clone(), loaded.file, loaded.stamp),
                        Err(ReadError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
                            Buffer::new_file(path.clone())
                        }
                        Err(error) => return Err(format!("{}: {error}", path.display())),
                    };
                    Ok(buffer)
                })
                .await;
            shell
                .update_in(cx, |shell, window, cx| {
                    if shell.session.is_some()
                        || shell.active_view() != &origin
                        || shell.buffers.index_of(&origin).is_none()
                    {
                        return;
                    }
                    match result {
                        Ok(buffer) => {
                            let identity = buffer.identity();
                            let existing = shell.buffers.iter().position(|slot| {
                                let other = slot.view.read(cx).buffer();
                                other.path() == buffer.path()
                                    || (identity.is_some() && other.identity() == identity)
                            });
                            let index =
                                existing.unwrap_or_else(|| shell.create_buffer(buffer, window, cx));
                            shell.activate(index, window, cx);
                        }
                        Err(message) => shell.show_message(&origin, message, cx),
                    }
                })
                .ok();
        }));
    }

    pub(super) fn write_file(
        &mut self,
        target: &Entity<BufferView>,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.enqueue_save(target, SaveIntent::WriteFile(path), window, cx);
    }

    pub(super) fn save_buffer(
        &mut self,
        target: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(index) = self.buffers.index_of(target) else {
            return false;
        };
        let saving = self.buffers[index].save.is_saving();
        let buffer = target.read(cx).buffer();
        if !saving && !buffer.is_modified() {
            self.show_message(target, "No changes need to be saved", cx);
            return false;
        }
        if !saving && buffer.path().is_none() {
            return self
                .begin_read(ReadKind::WriteFile, target, "", window, cx)
                .is_some();
        }
        self.enqueue_save(target, SaveIntent::Current, window, cx);
        true
    }

    fn enqueue_save(
        &mut self,
        target: &Entity<BufferView>,
        intent: SaveIntent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.buffers.index_of(target) else {
            return;
        };
        let save_point = target.update(cx, |view, _| view.begin_save());
        let buffer = target.read(cx).buffer();
        let request = SaveRequest {
            generation: self.buffers[index].save.next_generation(),
            save_point,
            intent,
            confirmed: false,
            format: buffer.format(),
            text: buffer.text().clone(),
        };
        if self.buffers[index].save.is_saving() {
            self.buffers[index].save.queue(request);
        } else {
            self.start_save(index, request, window, cx);
        }
    }

    fn start_save(
        &mut self,
        index: usize,
        request: SaveRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = self.buffers[index].view.clone();
        let buffer = view.read(cx).buffer();
        let path = match &request.intent {
            SaveIntent::Current => buffer.path().map(Path::to_path_buf),
            SaveIntent::WriteFile(path) => Some(path.clone()),
        };
        let Some(path) = path else {
            self.buffers[index].save.take_pending();
            self.after_save(
                &view,
                false,
                "No visited file; save canceled".to_owned(),
                window,
                cx,
            );
            return;
        };
        let stamp = buffer.disk_stamp();
        self.buffers[index].save.start(&request);
        cx.spawn_in(window, async move |shell, cx| {
            let completion = cx
                .background_executor()
                .spawn(async move {
                    let result = request.write(&path, stamp);
                    SaveCompletion {
                        view,
                        request,
                        path,
                        result,
                    }
                })
                .await;
            shell
                .update_in(cx, |shell, window, cx| {
                    shell.finish_save(completion, window, cx)
                })
                .ok();
        })
        .detach();
    }

    pub(super) fn resume_save(
        &mut self,
        target: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(index) = self.buffers.index_of(target)
            && let Some(request) = self.buffers[index].save.confirm()
        {
            self.start_save(index, request, window, cx);
        }
    }

    pub(super) fn cancel_save_confirmation(&mut self) {
        if let Some(target) = self
            .session
            .as_ref()
            .map(|session| session.origin().clone())
            && let Some(index) = self.buffers.index_of(&target)
        {
            self.buffers[index].save.cancel_confirmation();
        }
    }

    fn finish_save(
        &mut self,
        completion: SaveCompletion,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let SaveCompletion {
            view,
            request,
            path,
            result,
        } = completion;
        let Some(index) = self.buffers.index_of(&view) else {
            return;
        };
        if !self.buffers[index].save.take_matching(&request) {
            return;
        }
        let saved = matches!(result, Ok(SaveOutcome::Written(_)));
        let message = match result {
            Ok(SaveOutcome::Confirm) => {
                let kind = match request.intent {
                    SaveIntent::Current => ReadKind::ExternalChange(path),
                    SaveIntent::WriteFile(_) => ReadKind::Overwrite(path),
                };
                if self.begin_read(kind, &view, "", window, cx).is_some() {
                    self.buffers[index].save.await_confirmation(request);
                    return;
                }
                "Cannot confirm file change; save canceled".to_owned()
            }
            Ok(SaveOutcome::Written(stamp)) => {
                let update_path = matches!(request.intent, SaveIntent::WriteFile(_));
                view.update(cx, |view, cx| {
                    if update_path {
                        view.set_file_path(path.clone(), cx);
                    }
                    view.set_disk_stamp(Some(stamp), cx);
                    let _ = view.mark_saved_at(request.save_point, cx);
                });
                if update_path {
                    self.rename_buffer(index, cx);
                }
                if let Some(pending) = self.buffers[index].save.take_pending() {
                    self.start_save(index, pending, window, cx);
                    return;
                }
                format!("Wrote {}", path.display())
            }
            Err(error) => format!("Save failed: {error}"),
        };
        if !saved {
            self.buffers[index].save.take_pending();
        }
        self.after_save(&view, saved, message, window, cx);
        cx.notify();
    }
}
