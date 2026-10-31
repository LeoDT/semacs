//! Minimal key sequence engine for the spike (S2).
//!
//! Only what S2 needs: parse/normalize a platform keystroke into an Emacs-style
//! key sequence, match prefixes without a timeout, and decide
//! Execute / Prefix / Undefined / SelfInsert.

use std::collections::HashMap;
use std::fmt;
use std::hash::{Hash, Hasher};

#[derive(Clone, Debug)]
pub struct KeyStroke {
    pub key: String,
    /// Printable text the platform says this keystroke produces. Shifted
    /// letters, AltGr and dead-key compositions all arrive here; `key` alone
    /// cannot tell them apart from a command key.
    pub key_char: Option<String>,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub super_: bool,
}

/// A keystroke's identity is its key plus modifiers. `key_char` is payload
/// (what the platform says would be typed) and must not affect keymap lookup:
/// bindings are parsed without it, platform events carry it.
impl PartialEq for KeyStroke {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
            && self.ctrl == other.ctrl
            && self.alt == other.alt
            && self.shift == other.shift
            && self.super_ == other.super_
    }
}
impl Eq for KeyStroke {}
impl Hash for KeyStroke {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.key.hash(state);
        self.ctrl.hash(state);
        self.alt.hash(state);
        self.shift.hash(state);
        self.super_.hash(state);
    }
}

impl KeyStroke {
    /// Normalize a platform keystroke into the Emacs-facing form.
    ///
    /// `C-i` is `TAB` and `C-m` is `RET` in Emacs, so the modifiers are folded
    /// into the key name; GUI `<tab>` / `<return>` stay separate keys.
    pub fn from_platform(ks: &gpui_kit::Keystroke) -> Self {
        let mut key = ks.key.clone();
        let mut ctrl = ks.modifiers.control;
        let alt = ks.modifiers.alt;
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
            key_char: ks.key_char.clone(),
            ctrl,
            alt,
            shift,
            super_,
        }
    }

    pub fn is_plain(&self) -> bool {
        !self.ctrl && !self.alt && !self.super_
    }

    /// Text input path: the platform gave us printable text for this key.
    pub fn prints_text(&self) -> bool {
        self.key_char
            .as_deref()
            .and_then(|s| s.chars().next())
            .is_some_and(|c| !c.is_control())
    }

    /// A dead key (or an in-progress composition) arrives as a plain single
    /// character key with no printable text: GPUI has already delivered the
    /// composition as marked text, so this keystroke must not be reported as
    /// undefined either.
    pub fn is_composition_probe(&self) -> bool {
        let mut chars = self.key.chars();
        self.is_plain()
            && !self.prints_text()
            && matches!((chars.next(), chars.next()), (Some(_), None))
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
    /// Dead key / composition state: consume silently, never echo undefined.
    Composition,
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

        if pending.is_empty() && ks.is_plain() && ks.prints_text() {
            return (Resolution::SelfInsert, Vec::new());
        }
        if pending.is_empty() && ks.is_composition_probe() {
            return (Resolution::Composition, Vec::new());
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

    // Modifiers come as "C-"/"M-"/"S-"/"s-" prefixes; a bare "-" key is written
    // as "minus" to stay unambiguous.
    let mut rest = token;
    let key = loop {
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
            break rest.to_string();
        }
    };
    // Emacs key notation for keys GPUI names literally.
    let key = match key.as_str() {
        "minus" => "-".to_string(),
        "SPC" => "space".to_string(),
        other => other.to_string(),
    };
    KeyStroke {
        key,
        key_char: None,
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

    fn typed(key: &str, key_char: &str) -> KeyStroke {
        let mut ks = parse(key)[0].clone();
        ks.key_char = Some(key_char.to_string());
        ks
    }

    #[test]
    fn printable_without_prefix_self_inserts() {
        let map = Keymap::default();
        let (res, _) = map.resolve(&[], &typed("a", "a"));
        assert_eq!(res, Resolution::SelfInsert);
        // Shifted text and compositions self-insert through key_char, even when
        // the key name is not a single printable character.
        let (res, _) = map.resolve(&[], &typed("a", "A"));
        assert_eq!(res, Resolution::SelfInsert);
        let (res, _) = map.resolve(&[], &typed("aacute", "á"));
        assert_eq!(res, Resolution::SelfInsert);
        // Command keys never self-insert.
        let mut ctrl_a = typed("C-a", "a");
        ctrl_a.key_char = None;
        let (res, _) = map.resolve(&[], &ctrl_a);
        assert_eq!(res, Resolution::Undefined);
    }

    #[test]
    fn dead_key_is_a_composition_probe() {
        let map = Keymap::default();
        // Dead acute arrives as `'` with no printable text (the composition
        // itself is delivered as marked text before this keystroke).
        let mut dead = parse("'")[0].clone();
        dead.key_char = None;
        assert_eq!(map.resolve(&[], &dead).0, Resolution::Composition);
        // Function keys with no binding stay undefined.
        let mut f1 = parse("f1")[0].clone();
        f1.key_char = None;
        assert_eq!(map.resolve(&[], &f1).0, Resolution::Undefined);
    }
}
