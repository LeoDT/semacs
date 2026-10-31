//! Minimal key sequence engine for the spike (S2).
//!
//! Only what S2 needs: parse/normalize a platform keystroke into an Emacs-style
//! key sequence, match prefixes without a timeout, and decide
//! Execute / Prefix / Undefined / SelfInsert.

use std::collections::HashMap;
use std::fmt;

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct KeyStroke {
    pub key: String,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub super_: bool,
}

impl KeyStroke {
    /// Normalize a platform keystroke into the Emacs-facing form.
    ///
    /// `C-i` is `TAB` and `C-m` is `RET` in Emacs, so the modifiers are folded
    /// into the key name; GUI `<tab>` / `<return>` stay separate keys.
    pub fn from_platform(ks: &gpui_kit::Keystroke) -> Self {
        let mut key = ks.key.clone();
        let mut ctrl = ks.modifiers.control;
        let mut alt = ks.modifiers.alt;
        let shift = ks.modifiers.shift;
        let super_ = ks.modifiers.platform;
        if ctrl && !alt && !super_ {
            match key.as_str() {
                "i" => {
                    key = "tab".into();
                    ctrl = false;
                }
                "m" => {
                    key = "enter".into();
                    ctrl = false;
                }
                _ => {}
            }
        }
        Self {
            key,
            ctrl,
            alt,
            shift,
            super_,
        }
    }

    pub fn is_plain(&self) -> bool {
        !self.ctrl && !self.alt && !self.super_
    }

    pub fn single_char(&self) -> Option<char> {
        let mut chars = self.key.chars();
        match (chars.next(), chars.next()) {
            (Some(c), None) => Some(c),
            _ => None,
        }
    }
}

impl fmt::Display for KeyStroke {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.ctrl {
            write!(f, "C-")?;
        }
        if self.alt {
            write!(f, "M-")?;
        }
        if self.super_ {
            write!(f, "s-")?;
        }
        if self.shift {
            write!(f, "S-")?;
        }
        write!(f, "{}", self.key)
    }
}

pub type KeySeq = Vec<KeyStroke>;

pub fn describe(seq: &[KeyStroke]) -> String {
    seq.iter()
        .map(|k| k.to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Binding {
    Command(&'static str),
    Prefix,
}

#[derive(PartialEq, Eq, Debug)]
pub enum Resolution {
    Execute(&'static str),
    Prefix,
    Undefined,
    SelfInsert,
}

#[derive(Default)]
pub struct Keymap {
    bindings: HashMap<KeySeq, Binding>,
}

impl Keymap {
    pub fn bind(&mut self, seq: &str, binding: Binding) {
        self.bindings.insert(parse(seq), binding);
    }

    /// Resolve one keystroke against the pending prefix.
    ///
    /// A printable unbound key is `SelfInsert` even for a read-only target: the
    /// command layer decides whether it inserts or reports "Buffer is read-only".
    pub fn resolve(&self, pending: &[KeyStroke], ks: &KeyStroke) -> (Resolution, KeySeq) {
        let mut candidate = pending.to_vec();
        candidate.push(ks.clone());

        match self.bindings.get(&candidate) {
            Some(Binding::Command(name)) => return (Resolution::Execute(name), Vec::new()),
            Some(Binding::Prefix) => return (Resolution::Prefix, candidate),
            None => {}
        }

        // A longer binding or a declared prefix continues the sequence.
        let has_continuation = self
            .bindings
            .keys()
            .any(|seq| seq.len() > candidate.len() && seq.starts_with(&candidate));
        if has_continuation {
            return (Resolution::Prefix, candidate);
        }

        if pending.is_empty() && ks.is_plain() && ks.single_char().is_some() {
            return (Resolution::SelfInsert, Vec::new());
        }

        (Resolution::Undefined, Vec::new())
    }
}

/// Parse "C-x C-s" / "M-x" / "C-M-_" / "TAB" into a key sequence.
pub fn parse(text: &str) -> KeySeq {
    text.split_whitespace().map(parse_one).collect()
}

fn parse_one(token: &str) -> KeyStroke {
    let mut ctrl = false;
    let mut alt = false;
    let mut shift = false;
    let mut super_ = false;
    let mut key = String::new();

    // Modifiers come as "C-"/"M-"/"S-"/"s-" prefixes; a bare "-" key is written
    // as "minus" to stay unambiguous.
    let mut rest = token;
    loop {
        if let Some(tail) = rest.strip_prefix("C-") {
            ctrl = true;
            rest = tail;
        } else if let Some(tail) = rest.strip_prefix("M-") {
            alt = true;
            rest = tail;
        } else if let Some(tail) = rest.strip_prefix("S-") {
            shift = true;
            rest = tail;
        } else if let Some(tail) = rest.strip_prefix("s-") {
            super_ = true;
            rest = tail;
        } else {
            key = rest.to_string();
            break;
        }
    }
    let key = match key.as_str() {
        "minus" => "-".to_string(),
        other => other.to_string(),
    };
    KeyStroke {
        key,
        ctrl,
        alt,
        shift,
        super_,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_modifiers() {
        let seq = parse("C-x C-s");
        assert_eq!(seq.len(), 2);
        assert!(seq[0].ctrl && seq[0].key == "x");
        assert!(seq[1].ctrl && seq[1].key == "s");

        let seq = parse("C-M-_");
        assert!(seq[0].ctrl && seq[0].alt && seq[0].key == "_");

        let seq = parse("M-<");
        assert!(seq[0].alt && seq[0].key == "<");
    }

    #[test]
    fn resolves_prefix_without_timeout() {
        let mut map = Keymap::default();
        map.bind("C-x", Binding::Prefix);
        map.bind("C-x C-s", Binding::Command("save-buffer"));
        map.bind("C-g", Binding::Command("keyboard-quit"));

        let (res, pending) = map.resolve(&[], &parse("C-x")[0]);
        assert_eq!(res, Resolution::Prefix);
        assert_eq!(describe(&pending), "C-x");

        let (res, pending) = map.resolve(&pending, &parse("C-s")[0]);
        assert_eq!(res, Resolution::Execute("save-buffer"));
        assert!(pending.is_empty());

        // Staying pending forever is Emacs behaviour: no timeout, no fallback.
        let (res, _) = map.resolve(&parse("C-x"), &parse("C-g")[0]);
        assert_eq!(res, Resolution::Undefined);
    }

    #[test]
    fn printable_with_prefix_is_undefined() {
        let mut map = Keymap::default();
        map.bind("C-x", Binding::Prefix);
        let (res, _) = map.resolve(&parse("C-x"), &parse("a")[0]);
        assert_eq!(res, Resolution::Undefined);
    }

    #[test]
    fn printable_without_prefix_self_inserts() {
        let map = Keymap::default();
        let (res, _) = map.resolve(&[], &parse("a")[0]);
        assert_eq!(res, Resolution::SelfInsert);
        // Command keys never self-insert.
        let (res, _) = map.resolve(&[], &parse("C-a")[0]);
        assert_eq!(res, Resolution::Undefined);
    }
}
