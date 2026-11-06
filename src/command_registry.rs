use crate::command::{COMMANDS, Command, Scope};

#[cfg(test)]
#[path = "command_registry_tests.rs"]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ScriptCommand {
    pub generation: u64,
    pub index: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CommandId {
    Native(Command),
    Script(ScriptCommand),
}

impl From<Command> for CommandId {
    fn from(command: Command) -> Self {
        Self::Native(command)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScriptKind {
    Transform,
    Workflow,
}

#[derive(Clone, Debug)]
pub struct RegisteredCommand {
    pub id: CommandId,
    pub name: String,
    pub annotation: String,
    script: Option<(ScriptKind, Option<Vec<crate::mode::ModeId>>)>,
}

impl RegisteredCommand {
    fn available_in(&self, mode: crate::mode::ModeId) -> bool {
        match self.id {
            CommandId::Native(command) => command.available_in(mode),
            CommandId::Script(_) => self
                .script
                .as_ref()
                .is_some_and(|(_, modes)| modes.as_ref().is_none_or(|modes| modes.contains(&mode))),
        }
    }
}

pub struct CommandRegistry {
    commands: Vec<RegisteredCommand>,
    mode_annotations: std::collections::HashMap<
        crate::mode::ModeId,
        std::collections::HashMap<CommandId, Vec<String>>,
    >,
}

impl Default for CommandRegistry {
    fn default() -> Self {
        Self {
            mode_annotations: Default::default(),
            commands: COMMANDS
                .iter()
                .filter(|spec| spec.scope != Scope::ReadSession)
                .map(|spec| RegisteredCommand {
                    script: None,
                    id: spec.command.into(),
                    name: spec.name.into(),
                    annotation: spec
                        .bindings
                        .iter()
                        .map(|binding| binding.reference)
                        .collect::<Vec<_>>()
                        .join(" / "),
                })
                .collect(),
        }
    }
}

impl CommandRegistry {
    pub fn register_script(&mut self, name: &str, id: ScriptCommand) -> Result<(), String> {
        self.register(name, id, ScriptKind::Transform, None)
    }

    pub fn register_workflow(
        &mut self,
        name: &str,
        id: ScriptCommand,
        modes: Option<Vec<crate::mode::ModeId>>,
    ) -> Result<(), String> {
        if let Some(modes) = &modes {
            let unique: std::collections::HashSet<_> = modes.iter().collect();
            if modes.is_empty() || unique.len() != modes.len() {
                return Err("Workflow modes must be nonempty and unique".into());
            }
        }
        self.register(name, id, ScriptKind::Workflow, modes)
    }

    fn register(
        &mut self,
        name: &str,
        id: ScriptCommand,
        kind: ScriptKind,
        modes: Option<Vec<crate::mode::ModeId>>,
    ) -> Result<(), String> {
        if self
            .commands
            .iter()
            .filter(|entry| entry.script.is_some())
            .count()
            >= 256
        {
            return Err("At most 256 script commands are allowed".into());
        }
        if name.is_empty()
            || name.len() > 128
            || !name
                .bytes()
                .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == b'-')
        {
            return Err(
                "Command names must contain 1–128 lowercase ASCII letters, digits or hyphens"
                    .into(),
            );
        }
        if Command::named(name).is_some() || self.named(name).is_some() {
            return Err(format!("Command already registered: {name}"));
        }
        self.commands.push(RegisteredCommand {
            script: Some((kind, modes)),
            id: CommandId::Script(id),
            name: name.into(),
            annotation: String::new(),
        });
        Ok(())
    }

    pub fn named(&self, name: &str) -> Option<CommandId> {
        self.commands
            .iter()
            .find(|command| command.name == name)
            .map(|command| command.id)
    }

    pub fn script_kind(&self, id: ScriptCommand) -> Option<ScriptKind> {
        self.commands
            .iter()
            .find(|entry| entry.id == CommandId::Script(id))?
            .script
            .as_ref()
            .map(|(kind, _)| *kind)
    }

    pub fn available_in(&self, id: CommandId, mode: crate::mode::ModeId) -> bool {
        match id {
            CommandId::Native(command) => command.available_in(mode),
            CommandId::Script(_) => self
                .commands
                .iter()
                .find(|entry| entry.id == id)
                .is_some_and(|entry| entry.available_in(mode)),
        }
    }

    pub fn commands_for(
        &self,
        mode: crate::mode::ModeId,
    ) -> impl Iterator<Item = RegisteredCommand> + '_ {
        self.commands
            .iter()
            .filter(move |entry| entry.available_in(mode))
            .map(move |entry| {
                let mut entry = entry.clone();
                if let Some(annotations) = self.mode_annotations.get(&mode) {
                    entry.annotation = annotations
                        .get(&entry.id)
                        .map(|keys| keys.join(" / "))
                        .unwrap_or_default();
                }
                entry
            })
    }

    pub(crate) fn annotate_mode(
        &mut self,
        mode: crate::mode::ModeId,
        annotations: std::collections::HashMap<CommandId, Vec<String>>,
    ) {
        self.mode_annotations.insert(mode, annotations);
    }

    pub fn commands(&self) -> &[RegisteredCommand] {
        &self.commands
    }

    pub(crate) fn annotate(
        &mut self,
        annotations: &std::collections::HashMap<CommandId, Vec<String>>,
    ) {
        for command in &mut self.commands {
            command.annotation = annotations
                .get(&command.id)
                .map(|keys| keys.join(" / "))
                .unwrap_or_default();
        }
    }
}
