//! Layered key lookup and target-owned, timeout-free prefix state.

use std::collections::HashMap;

use gpui_kit::{KeyDownEvent, Keystroke, Modifiers, OperatingSystem};

use crate::command::{COMMANDS, Command, Scope};
use crate::command_registry::{CommandId, ScriptCommand};

mod configuration;

pub(crate) const DEFAULT_PREFIXES: [&str; 3] = ["ctrl-x", "ctrl-h", "escape"];

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Key {
    key: String,
    modifiers: Modifiers,
}

impl Key {
    pub fn from_keystroke(stroke: &Keystroke) -> Self {
        let key = match stroke.key.as_str() {
            "return" | "linefeed" | "kp-enter" => "enter",
            _ => &stroke.key,
        };
        let mut modifiers = stroke.modifiers;
        if key.chars().count() == 1 && key.to_lowercase() == key.to_uppercase() {
            modifiers.shift = false;
        }
        Self {
            key: key.to_owned(),
            modifiers,
        }
    }

    fn canonical(&self) -> impl Iterator<Item = Self> {
        let mut key = self.clone();
        let escape = key.modifiers.alt.then(|| {
            key.modifiers.alt = false;
            Self {
                key: "escape".into(),
                modifiers: Modifiers::default(),
            }
        });
        escape.into_iter().chain(std::iter::once(key))
    }

    fn label(&self) -> String {
        let mut label = String::new();
        for (enabled, prefix) in [
            (self.modifiers.control, "C-"),
            (self.modifiers.alt, "M-"),
            (self.modifiers.shift, "S-"),
            (self.modifiers.platform, "s-"),
            (self.modifiers.function, "Fn-"),
        ] {
            if enabled {
                label.push_str(prefix);
            }
        }
        label.push_str(match self.key.as_str() {
            "escape" => "ESC",
            "enter" => "RET",
            "tab" => "<tab>",
            "backspace" => "DEL",
            "space" => "SPC",
            key if key.chars().count() == 1 => key,
            key => return format!("{label}<{key}>"),
        });
        label
    }
}

pub fn parse_sequence(keys: &str) -> Result<Vec<Key>, BindError> {
    let sequence: Vec<_> = keys
        .split_whitespace()
        .map(|key| {
            if key
                .split('-')
                .any(|part| part.eq_ignore_ascii_case("secondary"))
            {
                return Err(BindError::InvalidKey(key.to_owned()));
            }
            Keystroke::parse_with_os(key, OperatingSystem::Unknown)
                .map(|stroke| Key::from_keystroke(&stroke))
                .map_err(|_| BindError::InvalidKey(key.to_owned()))
        })
        .collect::<Result<_, _>>()?;
    if sequence.is_empty() {
        return Err(BindError::EmptySequence);
    }
    Ok(sequence)
}

fn label(sequence: &[Key]) -> String {
    sequence
        .iter()
        .map(Key::label)
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BindError {
    EmptySequence,
    InvalidKey(String),
    Conflict(String),
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Binding {
    #[default]
    Prefix,
    Command(CommandId),
    Unbound,
}

#[derive(Default)]
struct Node {
    binding: Binding,
    children: HashMap<Key, Node>,
}

#[derive(Default)]
pub struct Keymap {
    root: Node,
    labels: HashMap<Vec<Key>, String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lookup {
    Command(Command),
    Script(ScriptCommand),
    Prefix,
    Unbound,
}

enum LayerLookup {
    Match(Lookup),
    Missing,
    Shadowed,
}

impl Keymap {
    pub fn bind(
        &mut self,
        sequence: &[Key],
        command: impl Into<CommandId>,
    ) -> Result<(), BindError> {
        self.insert(sequence, Binding::Command(command.into()))?;
        self.labels
            .insert(Self::canonical_sequence(sequence), label(sequence));
        Ok(())
    }

    pub fn define_prefix(&mut self, sequence: &[Key]) -> Result<(), BindError> {
        self.insert(sequence, Binding::Prefix)
    }

    pub(crate) fn disable(&mut self, sequence: &[Key]) -> Result<(), BindError> {
        self.insert(sequence, Binding::Unbound)
    }

    fn insert(&mut self, sequence: &[Key], binding: Binding) -> Result<(), BindError> {
        if sequence.is_empty() {
            return Err(BindError::EmptySequence);
        }
        let keys: Vec<_> = sequence.iter().flat_map(Key::canonical).collect();
        let mut node = &mut self.root;
        for key in &keys[..keys.len() - 1] {
            if node.binding != Binding::Prefix {
                return Err(BindError::Conflict(label(sequence)));
            }
            node = node.children.entry(key.clone()).or_default();
        }
        if node.binding != Binding::Prefix {
            return Err(BindError::Conflict(label(sequence)));
        }
        match node.children.entry(keys.last().unwrap().clone()) {
            std::collections::hash_map::Entry::Occupied(entry) => {
                if binding != Binding::Prefix || entry.get().binding != Binding::Prefix {
                    return Err(BindError::Conflict(label(sequence)));
                }
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(Node {
                    binding,
                    children: HashMap::new(),
                });
            }
        }
        Ok(())
    }

    fn lookup(&self, sequence: &[Key]) -> LayerLookup {
        let mut node = &self.root;
        for key in sequence.iter().flat_map(Key::canonical) {
            if node.binding != Binding::Prefix {
                return LayerLookup::Shadowed;
            }
            let Some(child) = node.children.get(&key) else {
                return LayerLookup::Missing;
            };
            node = child;
        }
        match node.binding {
            Binding::Command(CommandId::Native(command)) => {
                LayerLookup::Match(Lookup::Command(command))
            }
            Binding::Command(CommandId::Script(command)) => {
                LayerLookup::Match(Lookup::Script(command))
            }
            Binding::Prefix => LayerLookup::Match(Lookup::Prefix),
            Binding::Unbound => LayerLookup::Shadowed,
        }
    }

    pub fn for_body() -> Self {
        let mut map = Self::for_scope(Scope::Application);
        for spec in COMMANDS
            .iter()
            .filter(|spec| spec.scope == Scope::Edit && spec.modes.is_empty())
        {
            for binding in spec.bindings {
                map.bind(&parse_sequence(binding.keys).unwrap(), spec.command)
                    .unwrap();
                map.labels.insert(
                    Self::canonical_sequence(&parse_sequence(binding.keys).unwrap()),
                    binding.reference.to_owned(),
                );
            }
        }
        map
    }

    pub(crate) fn annotations(layers: &[&Self]) -> HashMap<CommandId, Vec<String>> {
        fn sequences(node: &Node, prefix: &mut Vec<Key>, output: &mut Vec<Vec<Key>>) {
            for (key, child) in &node.children {
                prefix.push(key.clone());
                if matches!(child.binding, Binding::Command(_)) {
                    output.push(prefix.clone());
                }
                sequences(child, prefix, output);
                prefix.pop();
            }
        }
        let mut keys = Vec::new();
        for layer in layers {
            sequences(&layer.root, &mut Vec::new(), &mut keys);
        }
        let mut annotations: HashMap<CommandId, Vec<String>> = HashMap::new();
        for keys in keys {
            let command = match lookup(layers, &keys) {
                Lookup::Command(command) => command.into(),
                Lookup::Script(command) => CommandId::Script(command),
                _ => continue,
            };
            let display = layers
                .iter()
                .find_map(|map| map.labels.get(&keys))
                .cloned()
                .unwrap_or_else(|| label(&keys));
            annotations.entry(command).or_default().push(display);
        }
        for labels in annotations.values_mut() {
            labels.sort();
            labels.dedup();
        }
        annotations
    }

    fn canonical_sequence(sequence: &[Key]) -> Vec<Key> {
        sequence.iter().flat_map(Key::canonical).collect()
    }

    /// Default bindings for one command scope. Layers combine scopes at dispatch
    /// time; a single map never mixes edit, application and session commands.
    pub fn for_scope(scope: Scope) -> Self {
        let mut map = Self::default();
        for prefix in DEFAULT_PREFIXES {
            map.define_prefix(&parse_sequence(prefix).expect("valid default prefix"))
                .expect("nonconflicting default prefix");
        }
        for spec in COMMANDS
            .iter()
            .filter(|spec| spec.scope == scope && spec.modes.is_empty())
        {
            for binding in spec.bindings {
                map.bind(
                    &parse_sequence(binding.keys).expect("valid default binding"),
                    spec.command,
                )
                .expect("nonconflicting default binding");
                map.labels.insert(
                    Self::canonical_sequence(&parse_sequence(binding.keys).unwrap()),
                    binding.reference.to_owned(),
                );
            }
        }
        map
    }
}

/// Named keys such as RET and TAB arrive without `key_char` from the platform.
/// They are commands, not unfinished IME text; only empty / single-scalar keys
/// and dead-key composition events belong to the platform input path.
///
/// This deliberately does not call `Keystroke::is_ime_in_progress`: that helper
/// classifies named keys through `is_printable_key`, which treats RET, TAB and
/// space as preedit when `key_char` is absent and would hide them from keymaps.
/// Real Wayland / IME behavior still has to be verified in M2-F.
fn is_ime_in_progress(stroke: &Keystroke) -> bool {
    let key = stroke.key.as_str();
    stroke.key_char.is_none()
        && (key.is_empty() || key.chars().count() == 1 || key.starts_with("dead_"))
        && !(stroke.modifiers.platform
            || stroke.modifiers.control
            || stroke.modifiers.function
            || stroke.modifiers.alt)
}

/// Layers are ordered from most specific to global. Prefix children may fall
/// through to lower layers, but a higher command shadows the entire lower prefix.
pub fn lookup(layers: &[&Keymap], sequence: &[Key]) -> Lookup {
    if sequence.is_empty() {
        return Lookup::Unbound;
    }
    for map in layers {
        match map.lookup(sequence) {
            LayerLookup::Match(found) => return found,
            LayerLookup::Shadowed => return Lookup::Unbound,
            LayerLookup::Missing => {}
        }
    }
    Lookup::Unbound
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Dispatch {
    Command(Command),
    Script(ScriptCommand),
    Pending(String),
    Undefined(String),
    SelfInsert,
    PlatformInput,
    InputBlocked,
    Ignored,
}

#[derive(Default)]
pub struct KeymapState {
    pending: Vec<Key>,
}

impl KeymapState {
    pub fn is_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    pub fn clear(&mut self) {
        self.pending.clear();
    }

    pub fn dispatch(&mut self, event: &KeyDownEvent, layers: &[&Keymap]) -> Dispatch {
        self.dispatch_keystroke(&event.keystroke, event.prefer_character_input, layers)
    }

    pub fn dispatch_keystroke(
        &mut self,
        stroke: &Keystroke,
        prefer_character_input: bool,
        layers: &[&Keymap],
    ) -> Dispatch {
        if matches!(
            stroke.key.as_str(),
            "shift" | "control" | "alt" | "platform" | "function"
        ) {
            return Dispatch::Ignored;
        }
        if prefer_character_input || is_ime_in_progress(stroke) {
            return if self.is_pending() {
                Dispatch::InputBlocked
            } else {
                Dispatch::PlatformInput
            };
        }

        let was_pending = self.is_pending();
        self.pending.push(Key::from_keystroke(stroke));
        match lookup(layers, &self.pending) {
            Lookup::Command(command) => {
                self.clear();
                Dispatch::Command(command)
            }
            Lookup::Script(command) => {
                self.clear();
                Dispatch::Script(command)
            }
            Lookup::Prefix => Dispatch::Pending(label(&self.pending)),
            Lookup::Unbound => {
                let sequence = label(&self.pending);
                self.clear();
                if !was_pending {
                    let modifiers = stroke.modifiers;
                    let printable_key = stroke.key.chars().count() == 1 || stroke.key == "space";
                    let printable_text = stroke.key_char.as_ref().is_some_and(|text| {
                        !text.is_empty() && text.chars().all(|ch| !ch.is_control())
                    });
                    if printable_key
                        && printable_text
                        && !modifiers.control
                        && !modifiers.alt
                        && !modifiers.platform
                        && !modifiers.function
                    {
                        return Dispatch::SelfInsert;
                    }
                }
                Dispatch::Undefined(format!("{sequence} is undefined"))
            }
        }
    }
}
