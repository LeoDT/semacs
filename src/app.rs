use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::component::button::ButtonVariant;
use gpui_kit::component::dialog::DialogButtonProps;
use gpui_kit::component::{ActiveTheme, WindowExt, v_flex};
use gpui_kit::*;

use crate::buffer::view::BufferView;
use crate::buffer::{Buffer, BufferEvent};
use crate::echo::EchoArea;
use crate::files::{DecodedFile, FileFormat};
use crate::frame;

pub struct StartupDocument {
    path: Option<PathBuf>,
    text: String,
    format: FileFormat,
}

impl StartupDocument {
    pub fn scratch() -> Self {
        Self {
            path: None,
            text: String::new(),
            format: FileFormat::default(),
        }
    }

    pub fn visit(path: PathBuf, file: DecodedFile) -> Self {
        Self {
            path: Some(path),
            text: file.text,
            format: file.format,
        }
    }

    /// Emacs opens a new, empty buffer for a path that does not exist yet.
    pub fn new_file(path: PathBuf) -> Self {
        Self {
            path: Some(path),
            text: String::new(),
            format: FileFormat::default(),
        }
    }

    pub fn title(&self) -> String {
        let name = self
            .path
            .as_deref()
            .and_then(std::path::Path::file_name)
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "*scratch*".to_owned());
        format!("{name} - semacs")
    }
}

pub struct AppShell {
    buffer: Entity<Buffer>,
    view: Entity<BufferView>,
    echo: EchoArea,
    confirming_close: bool,
    _subscriptions: Vec<Subscription>,
}

impl AppShell {
    pub fn new(document: StartupDocument, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let buffer =
            cx.new(|cx| Buffer::new(window, cx, document.path, document.text, document.format));
        let view = cx.new(|cx| BufferView::new(buffer.clone(), window, cx));
        let weak = cx.entity().downgrade();
        let subscriptions = vec![
            cx.subscribe_in(&buffer, window, |_, _, _: &BufferEvent, _, cx| cx.notify()),
            cx.observe_window_activation(window, |app, window, cx| {
                if !window.is_window_active() {
                    app.buffer.update(cx, |buffer, cx| {
                        buffer.cancel_composition(window, cx);
                    });
                }
            }),
            // M0 guard: the native Escape action calls EditorState::unmark_text
            // directly and retains preedit as untracked text. Route Escape
            // through Buffer while a composition is active; M1 replaces this
            // with the real key dispatcher.
            cx.intercept_keystrokes(move |event: &KeystrokeEvent, window, cx| {
                if is_plain_escape(&event.keystroke) {
                    let consumed = weak
                        .update(cx, |app, cx| app.handle_escape(window, cx))
                        .unwrap_or(false);
                    if consumed {
                        cx.stop_propagation();
                    }
                }
            }),
        ];

        let focus = buffer.read(cx).focus_handle(cx);
        window.defer(cx, move |window, cx| focus.focus(window, cx));

        Self {
            buffer,
            view,
            echo: EchoArea::default(),
            confirming_close: false,
            _subscriptions: subscriptions,
        }
    }

    pub fn buffer(&self) -> &Entity<Buffer> {
        &self.buffer
    }

    fn handle_escape(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.buffer
            .update(cx, |buffer, cx| buffer.finish_composition(window, cx))
    }

    /// D19: uncommitted preedit is not input. Cancel it first; only committed
    /// changes can require the unsaved-changes confirmation.
    pub fn should_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.buffer
            .update(cx, |buffer, cx| buffer.cancel_composition(window, cx));

        if !self.buffer.read(cx).is_modified() {
            return true;
        }
        if self.confirming_close {
            return false;
        }

        self.confirming_close = true;
        self.open_discard_dialog(window, cx);
        false
    }

    fn open_discard_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let this = cx.entity().downgrade();
        let focus = self.buffer.read(cx).focus_handle(cx);
        let name = self.buffer.read(cx).display_name();
        let discarded = Rc::new(Cell::new(false));

        let discard_on_ok = discarded.clone();
        let discard_on_close = discarded.clone();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            dialog
                .title("Unsaved changes")
                .description(format!(
                    "{name} has unsaved changes. Discard them and close?"
                ))
                .confirm()
                .button_props(
                    DialogButtonProps::default()
                        .ok_text("Discard")
                        .ok_variant(ButtonVariant::Danger)
                        .cancel_text("Cancel")
                        .show_cancel(true),
                )
                .on_ok({
                    let discarded = discard_on_ok.clone();
                    move |_, _, _| {
                        discarded.set(true);
                        true
                    }
                })
                .on_close({
                    let discarded = discard_on_close.clone();
                    let this = this.clone();
                    let focus = focus.clone();
                    move |_, window, cx| {
                        if discarded.get() {
                            window.remove_window();
                            return;
                        }
                        this.update(cx, |this, cx| {
                            this.confirming_close = false;
                            cx.notify();
                        })
                        .ok();
                        focus.focus(window, cx);
                    }
                })
        });
    }
}

fn is_plain_escape(keystroke: &Keystroke) -> bool {
    keystroke.key == "escape"
        && !keystroke.modifiers.control
        && !keystroke.modifiers.alt
        && !keystroke.modifiers.shift
        && !keystroke.modifiers.platform
}

impl Render for AppShell {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mode_line = frame::mode_line(self.buffer.read(cx), cx);
        let echo_area = frame::echo_area(&self.echo, cx);

        v_flex()
            .id("app")
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(div().flex_1().min_h_0().child(self.view.clone()))
            .child(mode_line)
            .child(echo_area)
    }
}
