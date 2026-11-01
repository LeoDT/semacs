//! Layered key lookup and target-owned, timeout-free prefix state.

use std::collections::HashMap;

use gpui_kit::{KeyDownEvent, Keystroke, Modifiers};

use crate::command::{COMMANDS, Command, Scope};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Key {
    key: String,
    modifiers: Modifiers,
}

impl Key {
    pub fn from_keystroke(stroke: &Keystroke) -> Self {
        let key = match stroke.key.as_str() {
            "return" => "enter",
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

pub fn parse_sequence(keys: &'static str) -> Result<Vec<Key>, BindError> {
    let sequence: Vec<_> = keys
        .split_whitespace()
        .map(|key| {
            Keystroke::parse(key)
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

#[derive(Default)]
struct Node {
    command: Option<Command>,
    children: HashMap<Key, Node>,
}

#[derive(Default)]
pub struct Keymap {
    root: Node,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lookup {
    Command(Command),
    Prefix,
    Unbound,
}

enum LayerLookup {
    Match(Lookup),
    Missing,
    Shadowed,
}

impl Keymap {
    pub fn bind(&mut self, sequence: &[Key], command: Command) -> Result<(), BindError> {
        self.insert(sequence, Some(command))
    }

    pub fn define_prefix(&mut self, sequence: &[Key]) -> Result<(), BindError> {
        self.insert(sequence, None)
    }

    fn insert(&mut self, sequence: &[Key], command: Option<Command>) -> Result<(), BindError> {
        if sequence.is_empty() {
            return Err(BindError::EmptySequence);
        }
        let keys: Vec<_> = sequence.iter().flat_map(Key::canonical).collect();
        let mut node = &mut self.root;
        for key in &keys[..keys.len() - 1] {
            if node.command.is_some() {
                return Err(BindError::Conflict(label(sequence)));
            }
            node = node.children.entry(key.clone()).or_default();
        }
        if node.command.is_some() {
            return Err(BindError::Conflict(label(sequence)));
        }
        match node.children.entry(keys.last().unwrap().clone()) {
            std::collections::hash_map::Entry::Occupied(entry) => {
                if command.is_some() || entry.get().command.is_some() {
                    return Err(BindError::Conflict(label(sequence)));
                }
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(Node {
                    command,
                    children: HashMap::new(),
                });
            }
        }
        Ok(())
    }

    fn lookup(&self, sequence: &[Key]) -> LayerLookup {
        let mut node = &self.root;
        for key in sequence.iter().flat_map(Key::canonical) {
            if node.command.is_some() {
                return LayerLookup::Shadowed;
            }
            let Some(child) = node.children.get(&key) else {
                return LayerLookup::Missing;
            };
            node = child;
        }
        LayerLookup::Match(match node.command {
            Some(command) => Lookup::Command(command),
            None => Lookup::Prefix,
        })
    }

    /// Default bindings for one command scope. Layers combine scopes at dispatch
    /// time; a single map never mixes edit, application and session commands.
    pub fn for_scope(scope: Scope) -> Self {
        let mut map = Self::default();
        for prefix in ["ctrl-x", "ctrl-h", "escape"] {
            map.define_prefix(&parse_sequence(prefix).expect("valid default prefix"))
                .expect("nonconflicting default prefix");
        }
        for spec in COMMANDS.iter().filter(|spec| spec.scope == scope) {
            for binding in spec.bindings {
                map.bind(
                    &parse_sequence(binding.keys).expect("valid default binding"),
                    spec.command,
                )
                .expect("nonconflicting default binding");
            }
        }
        map
    }
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
        if prefer_character_input || stroke.is_ime_in_progress() {
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
