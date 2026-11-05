use super::AppShell;
use crate::buffer::Buffer;
use crate::files::{self, ReadError};
use crate::saves::{SaveCompletion, SaveIntent, SaveOutcome, SaveRequest};
use crate::session::ReadKind;
use crate::view::{BufferView, InitialViewport};
use gpui_kit::{App, AppContext, Bounds, Context, Entity, Pixels, Window};
use std::io;
use std::path::PathBuf;

#[cfg(test)]
#[path = "opening_tests.rs"]
mod tests;

pub(super) struct Opening {
    origin: Entity<BufferView>,
    view: Entity<BufferView>,
    viewport: Option<InitialViewport>,
}

impl AppShell {
    pub(super) fn cancel_file_open(&mut self) {
        self.file_task = None;
        self.opening = None;
    }

    pub(super) fn prepare_opening(
        &mut self,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(opening) = &self.opening else { return };
        if self.minibuffer.is_some() || self.active_view() != &opening.origin {
            self.cancel_file_open();
            return;
        }
        let view = opening.view.clone();
        let viewport = view.read(cx).initial_viewport(bounds, window, cx);
        if opening.viewport.as_ref() == Some(&viewport) {
            return;
        }
        self.file_task = None;
        self.opening.as_mut().unwrap().viewport = Some(viewport.clone());
        let preparation = view.read(cx).prepare_initial(viewport.clone(), cx);
        self.file_task = Some(cx.spawn_in(window, async move |shell, cx| {
            let Some(prepared) = preparation.ready().await else {
                return;
            };
            let mut prepared = Some(prepared);
            shell
                .update_in(cx, |shell, window, cx| {
                    let Some(opening) = &shell.opening else {
                        return;
                    };
                    if opening.view != view || opening.viewport.as_ref() != Some(&viewport) {
                        return;
                    }
                    if shell.minibuffer.is_some()
                        || shell.active_view() != &opening.origin
                        || shell.buffer_index(&opening.origin, cx).is_none()
                    {
                        shell.cancel_file_open();
                        return;
                    }
                    if !viewport.matches_window(window) {
                        shell.opening.as_mut().unwrap().viewport = None;
                        cx.notify();
                        return;
                    }
                    let buffer = view.read(cx).buffer();
                    let existing = shell.buffers.iter().position(|slot| {
                        let other = &slot.buffer;
                        other.path() == buffer.path()
                            || (buffer.identity().is_some()
                                && other.identity() == buffer.identity())
                    });
                    if let Some(index) = existing {
                        shell.activate(index, window, cx);
                        return;
                    }
                    if !view.update(cx, |view, cx| {
                        view.install_initial(prepared.take().unwrap(), &viewport, cx)
                    }) {
                        shell.opening.as_mut().unwrap().viewport = None;
                        cx.notify();
                        return;
                    }
                    view.update(cx, |view, cx| view.install_input_lifecycle(window, cx));
                    let index = shell.insert_view(view, cx);
                    shell.activate(index, window, cx);
                })
                .ok();
            if let Some(prepared) = prepared {
                cx.background_executor()
                    .spawn(async move {
                        drop(prepared);
                    })
                    .detach();
            }
        }));
    }

    pub(super) fn visit_file(
        &mut self,
        path: PathBuf,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cancel_file_open();
        let existing = self
            .buffers
            .iter()
            .position(|slot| slot.buffer.path().as_deref() == Some(path.as_path()));
        if let Some(index) = existing {
            self.activate(index, window, cx);
            return;
        }
        let origin = origin.clone();
        let load_path = path.clone();
        self.file_task = Some(cx.spawn_in(window, async move |shell, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    match files::load(&load_path) {
                        Ok(loaded) => Ok((
                            crate::text::Rope::from(loaded.file.text.as_str()),
                            loaded.file.format,
                            loaded.stamp,
                        )),
                        Err(ReadError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
                            Ok((crate::text::Rope::new(), Default::default(), None))
                        }
                        Err(error) => Err(format!("{}: {error}", load_path.display())),
                    }
                })
                .await;
            shell
                .update_in(cx, |shell, window, cx| {
                    if shell.minibuffer.is_some()
                        || shell.active_view() != &origin
                        || shell.buffer_index(&origin, cx).is_none()
                    {
                        return;
                    }
                    match result {
                        Ok((text, format, stamp)) => {
                            let buffer =
                                Buffer::from_text(Some(path), text, format).with_disk_stamp(stamp);
                            let identity = buffer.identity();
                            let existing = shell.buffers.iter().position(|slot| {
                                let other = &slot.buffer;
                                other.path() == buffer.path()
                                    || (identity.is_some() && other.identity() == identity)
                            });
                            if let Some(index) = existing {
                                shell.activate(index, window, cx);
                            } else {
                                let view = cx.new(|cx| BufferView::new(buffer, cx));
                                shell.opening = Some(Opening {
                                    origin: origin.clone(),
                                    view,
                                    viewport: None,
                                });
                                cx.notify();
                            }
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
        let Some(index) = self.buffer_index(target, cx) else {
            return false;
        };
        let saving = self.buffers[index].save.is_saving();
        let buffer = &self.buffers[index].buffer;
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
        let Some(index) = self.buffer_index(target, cx) else {
            return;
        };
        let save_point = self.buffers[index].buffer.begin_save();
        let generation = self.buffers[index].save.next_generation();
        let buffer = &self.buffers[index].buffer;
        let request = SaveRequest {
            generation,
            save_point,
            intent,
            confirmed: false,
            format: buffer.format(),
            text: buffer.text(),
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
        let buffer_id = self.buffers[index].id();
        let buffer = &self.buffers[index].buffer;
        let path = match &request.intent {
            SaveIntent::Current => buffer.path(),
            SaveIntent::WriteFile(path) => Some(path.clone()),
        };
        let Some(path) = path else {
            self.buffers[index].save.take_pending();
            let view = self.buffer_target(index, window, cx);
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
                        buffer: buffer_id,
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
        if let Some(index) = self.buffer_index(target, cx)
            && let Some(request) = self.buffers[index].save.confirm()
        {
            self.start_save(index, request, window, cx);
        }
    }

    pub(super) fn cancel_save_confirmation(&mut self, cx: &App) {
        if let Some(target) = self
            .session
            .as_ref()
            .map(|session| session.origin().clone())
            && let Some(index) = self.buffer_index(&target, cx)
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
            buffer,
            request,
            path,
            result,
        } = completion;
        let Some(index) = self.buffers.index_of(buffer) else {
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
                let view = self.buffer_target(index, window, cx);
                if self.begin_read(kind, &view, "", window, cx).is_some() {
                    self.buffers[index].save.await_confirmation(request);
                    return;
                }
                "Cannot confirm file change; save canceled".to_owned()
            }
            Ok(SaveOutcome::Written(stamp)) => {
                let update_path = matches!(request.intent, SaveIntent::WriteFile(_));
                {
                    let buffer = &mut self.buffers[index].buffer;
                    if update_path {
                        buffer.set_path(path.clone());
                    }
                    buffer.set_disk_stamp(Some(stamp));
                    let _ = buffer.mark_saved_at(request.save_point);
                    if update_path && buffer.is_message_log() {
                        buffer.detach_message_log();
                    }
                }
                self.refresh_buffer(self.buffers[index].id(), cx);
                if update_path {
                    self.reset_message_log(buffer);
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
        let view = self.buffer_target(index, window, cx);
        self.after_save(&view, saved, message, window, cx);
        cx.notify();
    }
}
