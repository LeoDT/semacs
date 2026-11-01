use std::collections::HashSet;

use gpui_kit::{KeyDownEvent, Keystroke};
use semacs::command::{COMMANDS, Command, Scope};
use semacs::keymap::{
    BindError, Dispatch, Key, Keymap, KeymapState, Lookup, lookup, parse_sequence,
};

fn keys(sequence: &'static str) -> Vec<Key> {
    parse_sequence(sequence).unwrap()
}

fn event(key: &'static str) -> KeyDownEvent {
    KeyDownEvent {
        keystroke: Keystroke::parse(key).unwrap().with_simulated_ime(),
        is_held: false,
        prefer_character_input: false,
    }
}

fn dispatch(state: &mut KeymapState, key: &'static str, map: &Keymap) -> Dispatch {
    state.dispatch(&event(key), &[map])
}

#[test]
fn command_registry_has_unique_names_and_identities() {
    let mut names = HashSet::new();
    let mut commands = HashSet::new();
    for spec in COMMANDS {
        assert!(names.insert(spec.name));
        assert!(commands.insert(spec.command));
        assert_eq!(spec.command.name(), spec.name);
        assert_eq!(Command::named(spec.name), Some(spec.command));
    }
    assert_eq!(Command::named("not-a-command"), None);
}

fn reference_table(source: &'static str) -> std::collections::HashMap<&'static str, &'static str> {
    assert!(source.starts_with("# emacs-version: 31.1\n"));
    source
        .lines()
        .filter(|line| !line.starts_with('#'))
        .map(|line| line.split_once('\t').unwrap())
        .collect()
}

#[test]
fn every_default_binding_matches_the_fixed_emacs_reference() {
    let body_reference = reference_table(include_str!("emacs/m1-bindings.tsv"));
    let session_reference = reference_table(include_str!("emacs/m2-minibuffer-bindings.tsv"));
    let application = Keymap::for_scope(Scope::Application);
    let edit = Keymap::for_scope(Scope::Edit);
    let session = Keymap::for_scope(Scope::ReadSession);
    let body = [&application, &edit];
    let minibuffer = [&session, &edit];
    for spec in COMMANDS {
        let (layers, reference): (&[&Keymap], _) = match spec.scope {
            Scope::Application | Scope::Edit => (&body, &body_reference),
            Scope::ReadSession => (&minibuffer, &session_reference),
        };
        for binding in spec.bindings {
            assert_eq!(reference.get(binding.reference), Some(&spec.name));
            assert_eq!(
                lookup(layers, &keys(binding.keys)),
                Lookup::Command(spec.command),
                "{}",
                binding.keys
            );
        }
    }
    for sequence in ["ctrl-x", "ctrl-h", "escape", "escape escape", "alt-escape"] {
        assert_eq!(lookup(&body, &keys(sequence)), Lookup::Prefix);
    }
    // Application bindings live only in the application layer used by the body.
    assert_eq!(lookup(&[&edit], &keys("ctrl-x ctrl-f")), Lookup::Unbound);
    assert_eq!(lookup(&[&application], &keys("ctrl-f")), Lookup::Unbound);
    // Session bindings only exist in the layer a read session adds.
    assert_eq!(
        lookup(&[&edit], &keys("enter")),
        Lookup::Command(Command::Newline)
    );
    assert_eq!(
        lookup(&minibuffer, &keys("enter")),
        Lookup::Command(Command::MinibufferCompleteAndExit)
    );
    assert_eq!(
        lookup(&minibuffer, &keys("ctrl-g")),
        Lookup::Command(Command::AbortMinibuffers)
    );
}

#[test]
fn command_prefix_and_duplicate_conflicts_are_rejected_without_changing_bindings() {
    for (first, second) in [("ctrl-x", "ctrl-x u"), ("ctrl-x u", "ctrl-x")] {
        let mut map = Keymap::default();
        map.bind(&keys(first), Command::Undo).unwrap();
        assert!(matches!(
            map.bind(&keys(second), Command::ForwardChar),
            Err(BindError::Conflict(_))
        ));
        assert_eq!(
            lookup(&[&map], &keys(first)),
            Lookup::Command(Command::Undo)
        );
    }
    let mut map = Keymap::default();
    map.bind(&keys("ctrl-f"), Command::ForwardChar).unwrap();
    assert!(matches!(
        map.bind(&keys("ctrl-f"), Command::ForwardChar),
        Err(BindError::Conflict(_))
    ));
    assert!(matches!(
        map.define_prefix(&keys("ctrl-f")),
        Err(BindError::Conflict(_))
    ));
    assert_eq!(
        lookup(&[&map], &keys("ctrl-f")),
        Lookup::Command(Command::ForwardChar)
    );
}

#[test]
fn explicit_prefixes_can_be_extended_but_not_replaced_by_commands() {
    let mut map = Keymap::default();
    map.define_prefix(&keys("ctrl-x")).unwrap();
    map.define_prefix(&keys("ctrl-x")).unwrap();
    assert!(matches!(
        map.bind(&keys("ctrl-x"), Command::Undo),
        Err(BindError::Conflict(_))
    ));
    map.bind(&keys("ctrl-x u"), Command::Undo).unwrap();
    assert_eq!(lookup(&[&map], &keys("ctrl-x")), Lookup::Prefix);
    assert_eq!(
        lookup(&[&map], &keys("ctrl-x u")),
        Lookup::Command(Command::Undo)
    );
}

#[test]
fn empty_and_malformed_sequences_are_rejected() {
    assert_eq!(parse_sequence("  "), Err(BindError::EmptySequence));
    assert!(matches!(
        parse_sequence("ctrl-x-y"),
        Err(BindError::InvalidKey(_))
    ));
    let mut map = Keymap::default();
    assert_eq!(map.bind(&[], Command::Undo), Err(BindError::EmptySequence));
    assert_eq!(map.define_prefix(&[]), Err(BindError::EmptySequence));
    assert_eq!(lookup(&[&map], &[]), Lookup::Unbound);
}

#[test]
fn meta_and_escape_share_one_binding_and_conflict_namespace() {
    let mut map = Keymap::default();
    map.bind(&keys("alt-f"), Command::ForwardWord).unwrap();
    assert_eq!(
        lookup(&[&map], &keys("escape f")),
        Lookup::Command(Command::ForwardWord)
    );
    assert_eq!(lookup(&[&map], &keys("escape")), Lookup::Prefix);
    assert!(matches!(
        map.bind(&keys("escape f"), Command::BackwardWord),
        Err(BindError::Conflict(_))
    ));
    map.bind(&keys("ctrl-alt-_"), Command::UndoRedo).unwrap();
    assert_eq!(
        lookup(&[&map], &keys("escape ctrl-_")),
        Lookup::Command(Command::UndoRedo)
    );
}

#[test]
fn layers_override_commands_and_merge_prefix_children() {
    let global = Keymap::for_scope(Scope::Edit);
    let mut local = Keymap::default();
    local.bind(&keys("ctrl-f"), Command::BackwardChar).unwrap();
    local.bind(&keys("ctrl-x z"), Command::Yank).unwrap();
    let layers = [&local, &global];
    assert_eq!(
        lookup(&layers, &keys("ctrl-f")),
        Lookup::Command(Command::BackwardChar)
    );
    assert_eq!(
        lookup(&layers, &keys("ctrl-b")),
        Lookup::Command(Command::BackwardChar)
    );
    assert_eq!(lookup(&layers, &keys("ctrl-x")), Lookup::Prefix);
    assert_eq!(
        lookup(&layers, &keys("ctrl-x z")),
        Lookup::Command(Command::Yank)
    );
    assert_eq!(
        lookup(&layers, &keys("ctrl-x u")),
        Lookup::Command(Command::Undo)
    );

    let mut overriding = Keymap::default();
    overriding.bind(&keys("ctrl-x"), Command::Yank).unwrap();
    assert_eq!(
        lookup(&[&overriding, &global], &keys("ctrl-x u")),
        Lookup::Unbound
    );

    let mut prefix = Keymap::default();
    prefix.define_prefix(&keys("ctrl-f")).unwrap();
    assert_eq!(lookup(&[&prefix, &global], &keys("ctrl-f")), Lookup::Prefix);
}

#[test]
fn prefix_state_is_target_owned_and_waits_until_a_following_key() {
    let map = Keymap::for_scope(Scope::Edit);
    let mut first = KeymapState::default();
    let mut second = KeymapState::default();
    assert_eq!(
        dispatch(&mut first, "ctrl-x", &map),
        Dispatch::Pending("C-x".into())
    );
    assert!(first.is_pending());
    assert_eq!(dispatch(&mut second, "u", &map), Dispatch::SelfInsert);
    assert!(!second.is_pending());
    assert_eq!(
        dispatch(&mut first, "u", &map),
        Dispatch::Command(Command::Undo)
    );
    assert!(!first.is_pending());
}

#[test]
fn quit_is_looked_up_in_the_current_prefix_not_handled_as_a_global_interrupt() {
    let map = Keymap::for_scope(Scope::Edit);
    let mut state = KeymapState::default();
    assert_eq!(
        dispatch(&mut state, "ctrl-g", &map),
        Dispatch::Command(Command::KeyboardQuit)
    );
    dispatch(&mut state, "ctrl-x", &map);
    assert_eq!(
        dispatch(&mut state, "ctrl-g", &map),
        Dispatch::Undefined("C-x C-g is undefined".into())
    );
    assert!(!state.is_pending());
    assert_eq!(dispatch(&mut state, "u", &map), Dispatch::SelfInsert);
    dispatch(&mut state, "escape", &map);
    assert_eq!(
        dispatch(&mut state, "ctrl-g", &map),
        Dispatch::Undefined("ESC C-g is undefined".into())
    );
}

#[test]
fn escape_sequences_and_meta_events_resolve_to_the_same_commands() {
    let map = Keymap::for_scope(Scope::Edit);
    let mut state = KeymapState::default();
    assert_eq!(
        dispatch(&mut state, "alt-f", &map),
        Dispatch::Command(Command::ForwardWord)
    );
    assert_eq!(
        dispatch(&mut state, "escape", &map),
        Dispatch::Pending("ESC".into())
    );
    assert_eq!(
        dispatch(&mut state, "f", &map),
        Dispatch::Command(Command::ForwardWord)
    );
    assert_eq!(
        dispatch(&mut state, "alt-escape", &map),
        Dispatch::Pending("M-ESC".into())
    );
    assert_eq!(
        dispatch(&mut state, "escape", &map),
        Dispatch::Command(Command::KeyboardEscapeQuit)
    );
    dispatch(&mut state, "escape", &map);
    assert_eq!(
        dispatch(&mut state, "escape", &map),
        Dispatch::Pending("ESC ESC".into())
    );
    assert_eq!(
        dispatch(&mut state, "escape", &map),
        Dispatch::Command(Command::KeyboardEscapeQuit)
    );
}

#[test]
fn tab_return_and_backspace_keep_their_distinct_key_identities() {
    let map = Keymap::for_scope(Scope::Edit);
    for (key, command) in [
        ("tab", Command::Indent),
        ("ctrl-i", Command::Indent),
        ("enter", Command::Newline),
        ("return", Command::Newline),
        ("ctrl-m", Command::Newline),
        ("ctrl-j", Command::ElectricNewline),
        ("backspace", Command::DeleteBackwardChar),
    ] {
        assert_eq!(
            dispatch(&mut KeymapState::default(), key, &map),
            Dispatch::Command(command)
        );
    }
    assert_eq!(
        dispatch(&mut KeymapState::default(), "ctrl-h", &map),
        Dispatch::Pending("C-h".into())
    );
    let mut local = Keymap::default();
    local.bind(&keys("tab"), Command::Yank).unwrap();
    assert_eq!(
        lookup(&[&local, &map], &keys("tab")),
        Lookup::Command(Command::Yank)
    );
    assert_eq!(
        lookup(&[&local, &map], &keys("ctrl-i")),
        Lookup::Command(Command::Indent)
    );
}

#[test]
fn only_printable_unmodified_or_shifted_input_can_self_insert() {
    let map = Keymap::for_scope(Scope::Edit);
    for key in ["a", "shift-a", "中", "space", "!", "shift-!"] {
        assert_eq!(
            dispatch(&mut KeymapState::default(), key, &map),
            Dispatch::SelfInsert,
            "{key}"
        );
    }
    for key in ["ctrl-z", "alt-z", "super-z", "fn-z", "f1", "left"] {
        let mut event = event(key);
        event.keystroke.key_char = Some("z".into());
        assert!(
            matches!(
                KeymapState::default().dispatch(&event, &[&map]),
                Dispatch::Undefined(_)
            ),
            "{key}"
        );
    }
    let mut fake = event("f1");
    fake.keystroke.key_char = Some("x".into());
    assert_eq!(
        KeymapState::default().dispatch(&fake, &[&map]),
        Dispatch::Undefined("<f1> is undefined".into())
    );
}

#[test]
fn explicit_bindings_take_precedence_over_self_insert_and_ignore_key_char_metadata() {
    let mut map = Keymap::default();
    map.bind(&keys("a"), Command::Yank).unwrap();
    let mut typed = event("a");
    typed.keystroke.key_char = Some("文".into());
    assert_eq!(
        KeymapState::default().dispatch(&typed, &[&map]),
        Dispatch::Command(Command::Yank)
    );
    assert_eq!(
        dispatch(&mut KeymapState::default(), "shift-a", &map),
        Dispatch::SelfInsert
    );
    let global = Keymap::for_scope(Scope::Edit);
    assert_eq!(
        dispatch(&mut KeymapState::default(), "alt-shift-<", &global),
        Dispatch::Command(Command::BeginningOfBuffer)
    );
}

#[test]
fn platform_character_input_is_not_mistaken_for_an_editor_command() {
    let map = Keymap::for_scope(Scope::Edit);
    let mut altgr = event("ctrl-alt-q");
    altgr.prefer_character_input = true;
    altgr.keystroke.key_char = Some("@".into());
    let mut dead = event("a");
    dead.keystroke.key_char = None;
    for input in [altgr, dead] {
        let mut state = KeymapState::default();
        assert_eq!(state.dispatch(&input, &[&map]), Dispatch::PlatformInput);
        dispatch(&mut state, "ctrl-x", &map);
        assert_eq!(state.dispatch(&input, &[&map]), Dispatch::InputBlocked);
        assert!(state.is_pending());
        assert_eq!(
            dispatch(&mut state, "u", &map),
            Dispatch::Command(Command::Undo)
        );
    }
}

#[test]
fn undefined_printable_suffix_does_not_fall_through_to_text_input() {
    let map = Keymap::for_scope(Scope::Edit);
    let mut state = KeymapState::default();
    dispatch(&mut state, "ctrl-x", &map);
    assert_eq!(
        dispatch(&mut state, "z", &map),
        Dispatch::Undefined("C-x z is undefined".into())
    );
    assert!(!state.is_pending());
    assert_eq!(dispatch(&mut state, "z", &map), Dispatch::SelfInsert);
    assert_eq!(
        dispatch(&mut state, "alt-z", &map),
        Dispatch::Undefined("M-z is undefined".into())
    );
}

#[test]
fn modifier_only_events_preserve_prefix_and_explicit_clear_ends_it() {
    let map = Keymap::for_scope(Scope::Edit);
    let mut state = KeymapState::default();
    dispatch(&mut state, "ctrl-x", &map);
    for key in ["shift", "ctrl", "alt", "super", "fn"] {
        assert_eq!(dispatch(&mut state, key, &map), Dispatch::Ignored);
        assert!(state.is_pending());
    }
    state.clear();
    assert!(!state.is_pending());
    assert_eq!(dispatch(&mut state, "u", &map), Dispatch::SelfInsert);
}
