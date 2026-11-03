use gpui_kit::{Context, Entity, Window, px};

use super::AppShell;
use crate::appearance::FontSettings;
use crate::command::Command;
use crate::view::BufferView;

impl AppShell {
    pub(super) fn confirm_theme(
        &mut self,
        id: u64,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut settings = crate::theme::ThemeSettings::global(cx).clone();
        if let Err(error) = settings.select(name) {
            self.message_minibuffer(error, cx);
            return;
        }
        if let Some(origin) = self.end_session(id, window, cx) {
            settings.install(cx);
            self.show_message(&origin, format!("Theme: {name}"), cx);
        }
    }

    pub(super) fn change_font_size(
        &mut self,
        command: Command,
        origin: &Entity<BufferView>,
        cx: &mut Context<Self>,
    ) {
        let current = cx.global::<FontSettings>();
        let mut settings = current.clone();
        let result = match command {
            Command::IncreaseFontSize => settings.set_size(settings.size() + px(1.)),
            Command::DecreaseFontSize => settings.set_size(settings.size() - px(1.)),
            Command::ResetFontSize => {
                settings.reset_size();
                Ok(())
            }
            _ => unreachable!("not a font size command"),
        };
        match result {
            Ok(()) => {
                let message = format!("Font size: {}px", f32::from(settings.size()));
                if &settings != current {
                    cx.set_global(settings);
                }
                self.show_message(origin, message, cx);
            }
            Err(error) => self.show_message(origin, error.to_string(), cx),
        }
    }

    pub(super) fn confirm_font_family(
        &mut self,
        id: u64,
        input: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current = cx.global::<FontSettings>();
        let mut settings = current.clone();
        if let Err(error) = settings.set_family(input) {
            self.message_minibuffer(error.to_string(), cx);
            return;
        }
        let changed = &settings != current;
        let message = format!(
            "Font family: {} (platform fallback may apply)",
            settings.family()
        );
        if let Some(origin) = self.end_session(id, window, cx) {
            if changed {
                cx.set_global(settings);
            }
            self.show_message(&origin, message, cx);
        }
    }
}
