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

#[derive(Clone, Debug)]
pub struct RegisteredCommand {
    pub id: CommandId,
    pub name: String,
    pub annotation: String,
}

pub struct CommandRegistry {
    commands: Vec<RegisteredCommand>,
}

impl Default for CommandRegistry {
    fn default() -> Self {
        Self {
            commands: COMMANDS
                .iter()
                .filter(|spec| spec.scope != Scope::ReadSession)
                .map(|spec| RegisteredCommand {
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
