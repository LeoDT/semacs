use std::cell::Cell;
use std::fs::File;
use std::io::{ErrorKind, Read};
use std::path::Path;
use std::rc::Rc;
use std::time::Instant;

use gpui_kit::px;
use rquickjs::{Array, Context, Ctx, Function, Object, Persistent, Runtime, Value};

use super::{
    COMMAND_TIMEOUT, CONFIG_TIMEOUT, Configuration, MAX_SOURCE_BYTES, MAX_TEXT_BYTES, MEMORY_LIMIT,
    STACK_LIMIT,
};
use crate::appearance::FontSettings;
use crate::command_registry::{CommandRegistry, ScriptCommand};
use crate::keymap::Keymap;
use crate::theme::{StyleOverride, StyleRole, ThemeSettings};

pub(super) fn read_source(path: &Path) -> Result<String, String> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(String::new()),
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };
    let mut bytes = Vec::new();
    file.take((MAX_SOURCE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    if bytes.len() > MAX_SOURCE_BYTES {
        return Err("init.js exceeds the 1 MiB source limit".into());
    }
    String::from_utf8(bytes).map_err(|error| format!("init.js is not UTF-8: {error}"))
}

pub(super) struct Engine {
    functions: Vec<Persistent<Function<'static>>>,
    context: Context,
    runtime: Runtime,
    deadline: Rc<Cell<Instant>>,
    generation: u64,
    asynchronous: bool,
}

fn js_error(ctx: &Ctx<'_>, error: rquickjs::Error) -> String {
    let message = rquickjs::CaughtError::from_error(ctx, error).to_string();
    message.chars().take(1024).collect()
}

impl Engine {
    pub(super) fn new(
        source: &str,
        path: &Path,
        generation: u64,
    ) -> Result<(Self, Configuration), String> {
        let runtime = Runtime::new().map_err(|error| error.to_string())?;
        runtime.set_memory_limit(MEMORY_LIMIT);
        runtime.set_max_stack_size(STACK_LIMIT);
        let deadline = Rc::new(Cell::new(Instant::now() + CONFIG_TIMEOUT));
        let interrupt = deadline.clone();
        runtime.set_interrupt_handler(Some(Box::new(move || Instant::now() >= interrupt.get())));
        let context = Context::full(&runtime).map_err(|error| error.to_string())?;
        let mut functions = Vec::new();
        let configuration = context.with(|ctx| {
            let result = (|| -> rquickjs::Result<Object<'_>> {
                let builder: Object = ctx.eval(include_str!("bootstrap.js"))?;
                ctx.globals()
                    .set("semacs", builder.get::<_, Object>("api")?)?;
                let mut options = rquickjs::context::EvalOptions::default();
                options.filename = Some(path.to_string_lossy().into_owned());
                let result: Value = ctx.eval_with_options(source, options)?;
                if result.is_promise() {
                    return Err(rquickjs::Error::new_from_js_message(
                        "Promise",
                        "configuration",
                        "Asynchronous configuration is not supported",
                    ));
                }
                let finish: Function = builder.get("finish")?;
                finish.call(())
            })()
            .map_err(|error| js_error(&ctx, error))?;
            Self::configuration(&ctx, result, generation, &mut functions)
        });
        if Instant::now() >= deadline.get() {
            return Err("JavaScript configuration exceeded its 500 ms execution limit".into());
        }
        if runtime.is_job_pending() {
            return Err("Asynchronous configuration is not supported".into());
        }
        let configuration = configuration?;
        Ok((
            Self {
                functions,
                context,
                runtime,
                deadline,
                generation,
                asynchronous: false,
            },
            configuration,
        ))
    }

    fn configuration<'js>(
        ctx: &Ctx<'js>,
        manifest: Object<'js>,
        generation: u64,
        functions: &mut Vec<Persistent<Function<'static>>>,
    ) -> Result<Configuration, String> {
        let parse = || -> rquickjs::Result<(Array<'js>, Array<'js>, Object<'js>)> {
            Ok((
                manifest.get("commands")?,
                manifest.get("bindings")?,
                manifest.get("font")?,
            ))
        };
        let (commands, bindings, font) = parse().map_err(|error| js_error(ctx, error))?;
        if commands.len() > 256 || bindings.len() > 1024 {
            return Err("Configuration exceeds its command or key binding limit".into());
        }
        let mut registry = CommandRegistry::default();
        for (index, command) in commands.iter::<Object>().enumerate() {
            let command = command.map_err(|error| js_error(ctx, error))?;
            let name: String = command.get("name").map_err(|error| js_error(ctx, error))?;
            registry.register_script(&name, ScriptCommand { generation, index })?;
            let function: Function = command.get("run").map_err(|error| js_error(ctx, error))?;
            functions.push(Persistent::save(ctx, function));
        }
        let family: Option<String> = font.get("family").map_err(|error| js_error(ctx, error))?;
        let size: Option<f32> = font.get("size").map_err(|error| js_error(ctx, error))?;
        let default_font = FontSettings::default();
        let font = FontSettings::new(
            family.as_deref().unwrap_or(default_font.family()),
            size.map(px).unwrap_or(default_font.base_size()),
        )
        .map_err(|error| error.to_string())?;

        let bindings = bindings
            .iter::<Object>()
            .map(|binding| {
                let binding = binding?;
                Ok((
                    binding.get::<_, String>("keys")?,
                    binding.get::<_, Option<String>>("command")?,
                    binding.get::<_, Option<String>>("mode")?,
                ))
            })
            .collect::<rquickjs::Result<Vec<_>>>()
            .map_err(|error| js_error(ctx, error))?;
        let mut body = Vec::new();
        let mut local = std::collections::HashMap::<_, Vec<_>>::new();
        for (keys, command, mode) in bindings {
            if let Some(name) = mode {
                let mode = crate::mode::ModeId::named(&name)
                    .ok_or_else(|| format!("Unknown mode: {name}"))?;
                local.entry(mode).or_default().push((keys, command));
            } else {
                body.push((keys, command));
            }
        }
        let body_keymap = Keymap::configured_body(&mut registry, body)?;
        let mut mode_keymaps = std::collections::HashMap::new();
        for mode in crate::mode::MODES {
            let map = Keymap::configured_mode(
                &mut registry,
                local.remove(mode).unwrap_or_default(),
                *mode,
            )?;
            registry.annotate_mode(*mode, Keymap::annotations(&[&map, &body_keymap]));
            mode_keymaps.insert(*mode, map);
        }
        let theme = Self::theme(ctx, &manifest)?;
        Ok(Configuration {
            generation,
            commands: registry,
            body_keymap,
            mode_keymaps,
            font,
            theme,
        })
    }

    fn theme(ctx: &Ctx<'_>, manifest: &Object<'_>) -> Result<ThemeSettings, String> {
        let mut settings = ThemeSettings::default();
        let definitions: Array = manifest
            .get("themes")
            .map_err(|error| js_error(ctx, error))?;
        for definition in definitions.iter::<Object>() {
            let definition = definition.map_err(|error| js_error(ctx, error))?;
            let name: String = definition
                .get("name")
                .map_err(|error| js_error(ctx, error))?;
            let appearance: String = definition
                .get("appearance")
                .map_err(|error| js_error(ctx, error))?;
            let styles = Self::styles(
                ctx,
                definition
                    .get("styles")
                    .map_err(|error| js_error(ctx, error))?,
            )?;
            settings.define(&name, &appearance, styles)?;
        }
        let overrides = Self::styles(
            ctx,
            manifest
                .get("styles")
                .map_err(|error| js_error(ctx, error))?,
        )?;
        for (role, colors) in overrides {
            settings.set_style(role, colors);
        }
        let active: String = manifest
            .get("theme")
            .map_err(|error| js_error(ctx, error))?;
        settings.select(&active)?;
        Ok(settings)
    }

    fn styles(ctx: &Ctx<'_>, styles: Array<'_>) -> Result<Vec<(StyleRole, StyleOverride)>, String> {
        styles
            .iter::<Object>()
            .map(|style| {
                let style = style.map_err(|error| js_error(ctx, error))?;
                let name: String = style.get("name").map_err(|error| js_error(ctx, error))?;
                let role = StyleRole::parse(&name)?;
                let foreground: Option<String> = style
                    .get("foreground")
                    .map_err(|error| js_error(ctx, error))?;
                let background: Option<String> = style
                    .get("background")
                    .map_err(|error| js_error(ctx, error))?;
                Ok((
                    role,
                    StyleOverride::new(role, foreground.as_deref(), background.as_deref())?
                        .with_text_attributes(
                            role,
                            style
                                .get::<_, Option<String>>("weight")
                                .map_err(|error| js_error(ctx, error))?
                                .as_deref(),
                            style.get("italic").map_err(|error| js_error(ctx, error))?,
                            style
                                .get("underline")
                                .map_err(|error| js_error(ctx, error))?,
                            style
                                .get("strikethrough")
                                .map_err(|error| js_error(ctx, error))?,
                        )?,
                ))
            })
            .collect()
    }

    pub(super) fn transform(
        &mut self,
        command: ScriptCommand,
        input: &str,
    ) -> Result<String, String> {
        if command.generation != self.generation || command.index >= self.functions.len() {
            return Err("Unknown or obsolete script command".into());
        }
        if self.asynchronous {
            return Err("Asynchronous jobs are not supported; reload configuration".into());
        }
        self.deadline.set(Instant::now() + COMMAND_TIMEOUT);
        let result = self.context.with(|ctx| {
            let function = self.functions[command.index]
                .clone()
                .restore(&ctx)
                .map_err(|error| js_error(&ctx, error))?;
            let output: rquickjs::String = function
                .call((input,))
                .map_err(|error| js_error(&ctx, error))?;
            let bytes = output
                .clone()
                .to_cstring()
                .map_err(|error| js_error(&ctx, error))?;
            if bytes.len() > MAX_TEXT_BYTES {
                return Err("Script output exceeds the 256 KiB limit".into());
            }
            drop(bytes);
            output.to_string().map_err(|error| js_error(&ctx, error))
        });
        self.asynchronous = self.runtime.is_job_pending();
        if Instant::now() >= self.deadline.get() {
            return Err("Script command exceeded its 100 ms execution limit".into());
        }
        if self.asynchronous {
            return Err("Asynchronous jobs are not supported; reload configuration".into());
        }
        result
    }
}
