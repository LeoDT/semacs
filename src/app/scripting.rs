use std::path::PathBuf;

use gpui_kit::{Context, Entity, Window};

use super::AppShell;
use crate::command_registry::ScriptCommand;
use crate::scripting::{self, LoadedConfiguration};
use crate::view::BufferView;

impl AppShell {
    pub fn with_configuration(
        buffer: crate::buffer::Buffer,
        path: Option<PathBuf>,
        result: Result<LoadedConfiguration, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        if let Ok(loaded) = &result {
            cx.set_global(loaded.configuration.font.clone());
        }
        let mut shell = Self::new(buffer, window, cx);
        shell.config_path = path;
        let origin = shell.active_view().clone();
        match result {
            Ok(loaded) => shell.apply_configuration(loaded, window, cx),
            Err(error) => shell.show_message(
                &origin,
                format!("Configuration failed; using defaults: {error}"),
                cx,
            ),
        }
        shell
    }

    pub(super) fn apply_configuration(
        &mut self,
        loaded: LoadedConfiguration,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.script_task = None;
        self.config_generation = loaded.configuration.generation;
        self.commands = loaded.configuration.commands;
        self.body_keymap = loaded.configuration.body_keymap;
        self.script_host = Some(loaded.host);
        cx.set_global(loaded.configuration.font);
        for view in self.all_views() {
            view.update(cx, |view, cx| view.clear_pending(cx));
        }
        if let Some(mount) = &self.minibuffer {
            mount
                .view
                .read(cx)
                .editor()
                .clone()
                .update(cx, |view, cx| view.clear_pending(cx));
        }
        self.refresh_candidates(window, cx);
        cx.notify();
    }

    pub(super) fn reload_configuration(
        &mut self,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.reload_task.is_some() {
            self.show_message(origin, "Configuration reload is already running", cx);
            return;
        }
        let path = self
            .config_path
            .clone()
            .map(Ok)
            .unwrap_or_else(scripting::config_path);
        let path = match path {
            Ok(path) => path,
            Err(error) => {
                self.show_message(origin, error, cx);
                return;
            }
        };
        self.config_path = Some(path.clone());
        let origin = origin.downgrade();
        self.reload_task = Some(cx.spawn_in(window, async move |shell, cx| {
            let result = scripting::load(path).await;
            shell.update_in(cx, |shell, window, cx| {
                shell.reload_task = None;
                let origin = origin.upgrade().unwrap_or_else(|| shell.active_view().clone());
                match result {
                    Ok(loaded) => {
                        if shell.focused_editor(window, cx).read(cx).display_snapshot().marked_range().is_some() {
                            shell.show_message(&origin, "Configuration unchanged: finish input method composition and reload again", cx);
                            return;
                        }
                        shell.apply_configuration(loaded, window, cx);
                        shell.show_message(&origin, "Configuration reloaded", cx);
                    }
                    Err(error) => shell.show_message(&origin, format!("Configuration unchanged: {error}"), cx),
                }
            }).ok();
        }));
    }

    pub(super) fn execute_script(
        &mut self,
        command: ScriptCommand,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let result = self.start_transform(command, origin, window, cx);
        if let Err(error) = result {
            self.show_message(origin, error, cx);
        }
    }

    fn start_transform(
        &mut self,
        command: ScriptCommand,
        origin: &Entity<BufferView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if self.script_task.is_some() {
            return Err("A script command is already running".into());
        }
        if command.generation != self.config_generation {
            return Err("Script command belongs to an obsolete configuration".into());
        }
        let (target, input) = origin.read(cx).capture_region()?;
        let host = self
            .script_host
            .as_ref()
            .ok_or("JavaScript worker is unavailable")?;
        let result = host.transform(command, input)?;
        let origin = origin.downgrade();
        self.script_task = Some(cx.spawn_in(window, async move |shell, cx| {
            let result = result
                .await
                .unwrap_or_else(|_| Err("JavaScript worker stopped; reload configuration".into()));
            shell
                .update_in(cx, |shell, window, cx| {
                    if shell.config_generation != command.generation {
                        return;
                    }
                    shell.script_task = None;
                    let Some(origin) = origin.upgrade() else {
                        return;
                    };
                    let valid = shell
                        .body_panes
                        .values()
                        .any(|pane| pane.active_view() == &origin);
                    if !valid {
                        shell.show_message(
                            &origin,
                            "Script result discarded: original view is no longer visible",
                            cx,
                        );
                        return;
                    }
                    let result = result.and_then(|output| {
                        origin.update(cx, |view, cx| {
                            view.apply_region_transform(target, &output, window, cx)
                        })
                    });
                    if let Err(error) = result {
                        shell.show_message(&origin, error, cx);
                    }
                })
                .ok();
        }));
        Ok(())
    }
}
