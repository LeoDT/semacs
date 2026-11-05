use std::collections::{HashMap, HashSet};

use super::{DEFAULT_PREFIXES, Key, Keymap, label, parse_sequence};
use crate::command::{COMMANDS, Scope};
use crate::command_registry::{CommandId, CommandRegistry};

struct Override {
    keys: Vec<Key>,
    command: Option<CommandId>,
    source: String,
    replaced_command: bool,
}

impl Keymap {
    pub(crate) fn configured_body(
        registry: &mut CommandRegistry,
        bindings: Vec<(String, Option<String>)>,
    ) -> Result<Self, String> {
        Self::configured(registry, bindings, None)
    }

    pub(crate) fn configured_mode(
        registry: &mut CommandRegistry,
        bindings: Vec<(String, Option<String>)>,
        mode: crate::mode::ModeId,
    ) -> Result<Self, String> {
        Self::configured(registry, bindings, Some(mode))
    }

    fn configured(
        registry: &mut CommandRegistry,
        bindings: Vec<(String, Option<String>)>,
        mode: Option<crate::mode::ModeId>,
    ) -> Result<Self, String> {
        let mut effective = HashMap::new();
        for spec in COMMANDS.iter().filter(|spec| {
            spec.scope != Scope::ReadSession && spec.mode == mode.map(|mode| mode.name())
        }) {
            for binding in spec.bindings {
                effective.insert(
                    Self::canonical_sequence(&parse_sequence(binding.keys).unwrap()),
                    (
                        CommandId::Native(spec.command),
                        binding.reference.to_owned(),
                    ),
                );
            }
        }
        for (keys, command) in mode.into_iter().flat_map(|mode| mode.bindings()) {
            let keys = Self::canonical_sequence(&parse_sequence(keys).unwrap());
            effective.insert(keys.clone(), (command.into(), label(&keys)));
        }
        let mut custom = Vec::new();
        let mut seen = HashSet::new();
        for (source, name) in bindings {
            let keys = parse_sequence(&source)
                .map_err(|error| format!("Invalid key sequence {source:?}: {error:?}"))?;
            let keys = Self::canonical_sequence(&keys);
            if !seen.insert(keys.clone()) {
                return Err(format!("Duplicate custom key sequence: {source}"));
            }
            let command = name
                .map(|name| {
                    registry
                        .named(&name)
                        .ok_or_else(|| format!("Unknown or unavailable command: {name}"))
                })
                .transpose()?;
            let replaced_command = effective.remove(&keys).is_some();
            if let Some(command) = command {
                effective.insert(keys.clone(), (command, label(&keys)));
            }
            custom.push(Override {
                keys,
                command,
                source,
                replaced_command,
            });
        }
        let has_children = |keys: &[Key]| {
            effective
                .keys()
                .any(|other| other.len() > keys.len() && other.starts_with(keys))
        };
        for binding in &custom {
            if binding.command.is_none() && !binding.replaced_command && has_children(&binding.keys)
            {
                return Err(format!(
                    "Cannot unbind a prefix with remaining bindings: {}",
                    binding.source
                ));
            }
        }
        let mut map = Self::default();
        for prefix in DEFAULT_PREFIXES.into_iter().filter(|_| mode.is_none()) {
            let keys = parse_sequence(prefix).unwrap();
            if !custom
                .iter()
                .any(|binding| binding.command.is_none() && binding.keys == keys)
            {
                map.define_prefix(&keys).unwrap();
            }
        }
        for (keys, (command, reference)) in &effective {
            map.bind(keys, *command)
                .map_err(|error| format!("Key prefix conflict: {error:?}"))?;
            map.labels.insert(keys.clone(), reference.clone());
        }
        for binding in &custom {
            let covered = custom.iter().any(|parent| {
                parent.command.is_none()
                    && parent.keys.len() < binding.keys.len()
                    && binding.keys.starts_with(&parent.keys)
                    && !has_children(&parent.keys)
            });
            if binding.command.is_none() && !has_children(&binding.keys) && !covered {
                map.disable(&binding.keys)
                    .map_err(|error| format!("Key prefix conflict: {error:?}"))?;
            }
        }
        let mut annotations: HashMap<CommandId, Vec<String>> = HashMap::new();
        for (_, (command, label)) in effective {
            annotations.entry(command).or_default().push(label);
        }
        for labels in annotations.values_mut() {
            labels.sort();
        }
        if mode.is_none() {
            registry.annotate(&annotations);
        }
        Ok(map)
    }
}
