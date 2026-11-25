use super::tests::load;
use crate::command::Command;
use crate::keymap::{Lookup, lookup, parse_sequence};
use crate::mode::ModeId;

#[test]
fn tree_default_keys_and_prefixes_are_local_and_keep_ordinary_line_motion() {
    let (_, config) = load("").unwrap();
    let local = &config.mode_keymaps[&ModeId::TREEMACS];
    let layers = &[local, &config.body_keymap];
    for (key, command) in [
        ("tab", Command::TreemacsToggle),
        ("enter", Command::TreemacsReturn),
        ("l", Command::TreemacsReturn),
        ("h", Command::TreemacsCollapse),
        ("H", Command::TreemacsCollapseParent),
        ("n", Command::TreemacsNext),
        ("p", Command::TreemacsPrevious),
        ("alt-n", Command::TreemacsNextNeighbour),
        ("alt-p", Command::TreemacsPreviousNeighbour),
        ("u", Command::TreemacsParent),
        ("q", Command::TreemacsQuit),
        ("Q", Command::TreemacsKill),
        ("g", Command::TreemacsRefresh),
        ("r", Command::TreemacsRefresh),
        ("t h", Command::TreemacsHidden),
        ("t f", Command::TreemacsFollow),
        ("w", Command::TreemacsWidth),
        ("<", Command::TreemacsNarrow),
        (">", Command::TreemacsWiden),
        ("=", Command::TreemacsFit),
        ("o o", Command::TreemacsVisit),
        ("o h", Command::TreemacsVisitHorizontal),
        ("o v", Command::TreemacsVisitVertical),
        ("o r", Command::TreemacsVisitRecent),
        ("o c", Command::TreemacsVisitClose),
        ("ctrl-u enter", Command::TreemacsReturnStay),
        ("ctrl-u ctrl-u enter", Command::TreemacsReturnClose),
        ("ctrl-u ctrl-m", Command::TreemacsReturnStay),
        ("ctrl-u ctrl-u l", Command::TreemacsReturnClose),
        ("ctrl-n", Command::NextLine),
        ("ctrl-p", Command::PreviousLine),
    ] {
        assert_eq!(
            lookup(layers, &parse_sequence(key).unwrap()),
            Lookup::Command(command),
            "{key}"
        );
        assert!(
            config
                .commands
                .available_in(command.into(), ModeId::TREEMACS)
        );
    }
    for key in ["ctrl-u", "t", "o"] {
        assert_eq!(
            lookup(layers, &parse_sequence(key).unwrap()),
            Lookup::Prefix
        );
    }
    assert_eq!(
        lookup(&[&config.body_keymap], &parse_sequence("ctrl-u").unwrap()),
        Lookup::Unbound
    );
    for name in [
        "treemacs-select-window",
        "treemacs-find-file",
        "treemacs-COLLAPSE-action",
        "treemacs-collapse-parent-node",
    ] {
        assert!(config.commands.named(name).is_some());
    }
}

#[test]
fn tree_commands_use_the_same_mode_keymap_and_command_registry() {
    let (_, config) = load("semacs.bindKey('tab', 'treemacs-TAB-action', {mode:'treemacs'}); semacs.bindKey('g', 'treemacs-refresh', {mode:'treemacs'})").unwrap();
    assert_eq!(
        lookup(
            &[&config.mode_keymaps[&ModeId::TREEMACS]],
            &parse_sequence("tab").unwrap()
        ),
        Lookup::Command(Command::TreemacsToggle)
    );
    assert_eq!(
        config.commands.named("treemacs"),
        Some(Command::Treemacs.into())
    );
    assert!(
        config
            .commands
            .available_in(Command::TreemacsRefresh.into(), ModeId::TREEMACS)
    );
    assert!(
        !config
            .commands
            .available_in(Command::TreemacsRefresh.into(), ModeId::FUNDAMENTAL)
    );
}

#[test]
fn local_defaults_overrides_disables_and_annotations_are_contextual() {
    let (_, config) = load(
        r#"
        semacs.bindKey('ctrl-f', 'backward-char');
        semacs.bindKey('n', 'forward-char', {mode: 'magit-diff'});
        semacs.bindKey('ctrl-f', null, {mode: 'magit-diff'});
    "#,
    )
    .unwrap();
    let diff = &config.mode_keymaps[&ModeId::MAGIT_DIFF];
    let body = &config.body_keymap;
    for (key, expected) in [
        ("n", Lookup::Command(Command::ForwardChar)),
        ("tab", Lookup::Command(Command::SectionToggle)),
        ("ctrl-f", Lookup::Unbound),
    ] {
        assert_eq!(
            lookup(&[diff, body], &parse_sequence(key).unwrap()),
            expected
        );
    }
    assert_eq!(
        lookup(&[body], &parse_sequence("ctrl-f").unwrap()),
        Lookup::Command(Command::BackwardChar)
    );
    assert!(
        !config
            .commands
            .commands_for(ModeId::FUNDAMENTAL)
            .any(|command| command.id == Command::SectionToggle.into())
    );
    let forward = config
        .commands
        .commands_for(ModeId::MAGIT_DIFF)
        .find(|command| command.id == Command::ForwardChar.into())
        .unwrap();
    assert_eq!(forward.annotation, "n");
    let backward = config
        .commands
        .commands_for(ModeId::MAGIT_DIFF)
        .find(|command| command.id == Command::BackwardChar.into())
        .unwrap();
    assert_eq!(backward.annotation, "C-b");
    let (_, default) = load("").unwrap();
    for mode in [ModeId::FUNDAMENTAL, ModeId::MAGIT_DIFF] {
        let command = default
            .commands
            .commands_for(mode)
            .find(|command| command.id == Command::ExecuteExtendedCommand.into())
            .unwrap();
        assert_eq!(command.annotation, "M-x");
    }
    assert_eq!(
        lookup(
            &[&default.mode_keymaps[&ModeId::MAGIT_DIFF]],
            &parse_sequence("n").unwrap()
        ),
        Lookup::Command(Command::SectionForward)
    );
}

#[test]
fn local_prefixes_fall_through_but_commands_and_null_mask_lower_subtrees() {
    let (_, config) = load(
        r#"
        semacs.bindKey('ctrl-x z', 'forward-char', {mode: 'magit-diff'});
        semacs.bindKey('ctrl-h', null, {mode: 'magit-diff'});
        semacs.bindKey('ctrl-f a', 'backward-char', {mode: 'magit-diff'});
        semacs.bindKey('ctrl-o a', 'forward-char');
        semacs.bindKey('ctrl-o', 'backward-char', {mode: 'magit-diff'});
    "#,
    )
    .unwrap();
    let layers = [
        &config.mode_keymaps[&ModeId::MAGIT_DIFF],
        &config.body_keymap,
    ];
    for (key, expected) in [
        ("ctrl-x ctrl-f", Lookup::Command(Command::FindFile)),
        ("ctrl-x z", Lookup::Command(Command::ForwardChar)),
        ("ctrl-h e", Lookup::Unbound),
        ("ctrl-f", Lookup::Prefix),
        ("ctrl-f a", Lookup::Command(Command::BackwardChar)),
        ("ctrl-o a", Lookup::Unbound),
    ] {
        assert_eq!(
            lookup(&layers, &parse_sequence(key).unwrap()),
            expected,
            "{key}"
        );
    }
}

#[test]
fn local_validation_is_atomic_and_duplicates_are_per_mode() {
    for source in [
        "semacs.bindKey('n', 'forward-char', {mode: 'missing'})",
        "semacs.bindKey('n', 'missing', {mode: 'magit-diff'})",
        "semacs.bindKey('n', null, {mode: 'magit-diff'}); semacs.bindKey('n', null, {mode: 'magit-diff'})",
        "semacs.bindKey('n', null, {mode: 'magit-diff', extra: true})",
        "semacs.bindKey('n', null, {})",
        "semacs.bindKey('n', null, null)",
        "semacs.bindKey('n a', 'forward-char', {mode: 'magit-diff'})",
    ] {
        assert!(load(source).is_err(), "{source}");
    }
    assert!(load("semacs.bindKey('n', null); semacs.bindKey('n', null, {mode:'magit-diff'}); semacs.bindKey('n', null, {mode:'fundamental'})").is_ok());
    let (_, config) = load("semacs.bindKey('n', null, {mode:'magit-diff'}); semacs.bindKey('n a', 'forward-char', {mode:'magit-diff'})").unwrap();
    assert_eq!(
        lookup(
            &[&config.mode_keymaps[&ModeId::MAGIT_DIFF]],
            &parse_sequence("n a").unwrap()
        ),
        Lookup::Command(Command::ForwardChar)
    );
}
