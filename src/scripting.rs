use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError, sync_channel};

use futures_channel::oneshot;

use crate::appearance::FontSettings;
use crate::command_registry::{CommandRegistry, ScriptCommand};
use crate::keymap::Keymap;

mod runtime;
#[cfg(test)]
mod tests;

pub const MAX_TEXT_BYTES: usize = 256 * 1024;
pub const MAX_SOURCE_BYTES: usize = 1024 * 1024;
pub const MEMORY_LIMIT: usize = 32 * 1024 * 1024;
pub const STACK_LIMIT: usize = 256 * 1024;
pub const CONFIG_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(500);
pub const COMMAND_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(100);

static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);

pub struct Configuration {
    pub generation: u64,
    pub commands: CommandRegistry,
    pub body_keymap: Keymap,
    pub font: FontSettings,
    pub theme: crate::theme::ThemeSettings,
}

impl Default for Configuration {
    fn default() -> Self {
        Self {
            generation: 0,
            commands: CommandRegistry::default(),
            body_keymap: Keymap::for_body(),
            font: FontSettings::default(),
            theme: crate::theme::ThemeSettings::default(),
        }
    }
}

pub struct LoadedConfiguration {
    pub configuration: Configuration,
    pub host: ScriptHost,
}

pub struct ScriptHost {
    generation: u64,
    requests: SyncSender<Transform>,
}

struct Transform {
    command: ScriptCommand,
    input: String,
    reply: oneshot::Sender<Result<String, String>>,
}

pub fn config_path() -> Result<PathBuf, String> {
    config_path_from(
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
        std::env::var_os("HOME").map(PathBuf::from),
    )
}

fn config_path_from(xdg: Option<PathBuf>, home: Option<PathBuf>) -> Result<PathBuf, String> {
    if let Some(path) = xdg.filter(|path| path.is_absolute()) {
        return Ok(path.join("semacs/init.js"));
    }
    home.filter(|path| path.is_absolute())
        .map(|home| home.join(".config/semacs/init.js"))
        .ok_or_else(|| {
            "Cannot locate init.js: neither XDG_CONFIG_HOME nor HOME is an absolute path".into()
        })
}

pub async fn load(path: PathBuf) -> Result<LoadedConfiguration, String> {
    let generation = NEXT_GENERATION
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
            value.checked_add(1)
        })
        .expect("configuration generation exhausted");
    let (requests, receiver) = sync_channel::<Transform>(1);
    let (reply, ready) = oneshot::channel();
    std::thread::Builder::new()
        .name("semacs-js".into())
        .spawn(move || {
            let result = runtime::read_source(&path)
                .and_then(|source| runtime::Engine::new(&source, &path, generation));
            let (mut engine, configuration) = match result {
                Ok(loaded) => loaded,
                Err(error) => {
                    let _ = reply.send(Err(error));
                    return;
                }
            };
            if reply.send(Ok(configuration)).is_err() {
                return;
            }
            while let Ok(request) = receiver.recv() {
                if request.reply.is_canceled() {
                    continue;
                }
                let result = engine.transform(request.command, &request.input);
                let _ = request.reply.send(result);
            }
        })
        .map_err(|error| format!("Cannot start JavaScript worker: {error}"))?;
    let configuration = ready
        .await
        .map_err(|_| "JavaScript worker stopped during configuration".to_owned())??;
    Ok(LoadedConfiguration {
        configuration,
        host: ScriptHost {
            generation,
            requests,
        },
    })
}

impl ScriptHost {
    pub fn transform(
        &self,
        command: ScriptCommand,
        input: String,
    ) -> Result<oneshot::Receiver<Result<String, String>>, String> {
        if command.generation != self.generation {
            return Err("Script command belongs to an obsolete configuration".into());
        }
        if input.len() > MAX_TEXT_BYTES {
            return Err("Region exceeds the 256 KiB script input limit".into());
        }
        let (reply, result) = oneshot::channel();
        self.requests
            .try_send(Transform {
                command,
                input,
                reply,
            })
            .map_err(|error| match error {
                TrySendError::Full(_) => "JavaScript worker is busy".to_owned(),
                TrySendError::Disconnected(_) => {
                    "JavaScript worker stopped; reload configuration".to_owned()
                }
            })?;
        Ok(result)
    }
}
