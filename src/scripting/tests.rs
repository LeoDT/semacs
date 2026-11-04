use std::path::Path;

use super::{Configuration, MAX_TEXT_BYTES, runtime::Engine};
use crate::command::Command;
use crate::command_registry::{CommandId, ScriptCommand};
use crate::keymap::{Lookup, lookup, parse_sequence};

fn load(source: &str) -> Result<(Engine, Configuration), String> {
    Engine::new(source, Path::new("test-init.js"), 42)
}

fn script(configuration: &Configuration, name: &str) -> ScriptCommand {
    match configuration.commands.named(name).unwrap() {
        CommandId::Script(command) => command,
        CommandId::Native(_) => panic!("expected script"),
    }
}

fn binding(configuration: &Configuration, keys: &str) -> Lookup {
    lookup(
        &[&configuration.body_keymap],
        &parse_sequence(keys).unwrap(),
    )
}

#[test]
fn configuration_registers_commands_font_and_multiple_keys() {
    let (mut engine, configuration) = load(
        r#"
        semacs.setFont({family: " monospace ", size: 20});
        semacs.registerCommand("uppercase-region", text => text.toUpperCase());
        semacs.bindKey("ctrl-c u", "uppercase-region");
        semacs.bindKey("ctrl-c U", "uppercase-region");
    "#,
    )
    .unwrap();
    let command = script(&configuration, "uppercase-region");
    assert_eq!(binding(&configuration, "ctrl-c u"), Lookup::Script(command));
    assert_eq!(binding(&configuration, "ctrl-c U"), Lookup::Script(command));
    assert_eq!(configuration.font.family(), "monospace");
    assert_eq!(f32::from(configuration.font.base_size()), 20.);
    assert_eq!(engine.transform(command, "你好 abc").unwrap(), "你好 ABC");
    assert!(
        configuration
            .commands
            .commands()
            .iter()
            .find(|entry| entry.id == CommandId::Script(command))
            .unwrap()
            .annotation
            .contains("C-c")
    );
}

#[test]
fn navigation_keys_bind_to_native_commands() {
    let bindings = [
        ("ctrl-left", Command::BackwardWord),
        ("ctrl-right", Command::ForwardWord),
        ("ctrl-up", Command::BackwardParagraph),
        ("ctrl-down", Command::ForwardParagraph),
        ("pageup", Command::ScrollDown),
        ("pagedown", Command::ScrollUp),
        ("home", Command::MoveBeginningOfLine),
        ("end", Command::MoveEndOfLine),
        ("ctrl-home", Command::BeginningOfBuffer),
        ("ctrl-end", Command::EndOfBuffer),
    ];
    let source: String = bindings
        .iter()
        .map(|(keys, command)| format!("semacs.bindKey({keys:?}, {:?});\n", command.name()))
        .collect();
    let (_, configuration) = load(&source).unwrap();
    for (keys, command) in bindings {
        assert_eq!(binding(&configuration, keys), Lookup::Command(command));
    }
}

#[test]
fn overrides_and_unbinding_do_not_change_other_bindings() {
    let (_, configuration) = load(
        r#"
        semacs.bindKey("ctrl-f", "backward-char");
        semacs.bindKey("ctrl-x ctrl-s", null);
        semacs.bindKey("ctrl-_", null);
    "#,
    )
    .unwrap();
    assert_eq!(
        binding(&configuration, "ctrl-f"),
        Lookup::Command(Command::BackwardChar)
    );
    assert_eq!(binding(&configuration, "ctrl-x ctrl-s"), Lookup::Unbound);
    assert_eq!(
        binding(&configuration, "ctrl-x ctrl-f"),
        Lookup::Command(Command::FindFile)
    );
    assert_eq!(binding(&configuration, "ctrl-_"), Lookup::Unbound);
    assert_eq!(
        binding(&configuration, "ctrl-/"),
        Lookup::Command(Command::Undo)
    );
    assert!(
        !configuration
            .commands
            .commands()
            .iter()
            .find(|entry| entry.id == CommandId::Native(Command::ForwardChar))
            .unwrap()
            .annotation
            .contains("C-f")
    );
}

#[test]
fn theme_configuration_is_copied_validated_and_resolved_after_all_declarations() {
    use crate::theme::{StyleRole, ThemeSettings};
    let (_, config) = load(
        r##"
        semacs.loadTheme("night");
        const styles = { default: {foreground: "#112233"} };
        semacs.defineTheme("night", {appearance: "dark", styles});
        styles.default.foreground = "#aabbcc";
        const cursor = {foreground: "#445566"};
        semacs.setStyle("cursor", cursor);
        cursor.foreground = "#aabbcc";
        semacs.setStyle("syntax.keyword", {foreground: "#123456"});
        semacs.setStyle("cursor", {background: "#778899"});
    "##,
    )
    .unwrap();
    assert_eq!(config.theme.active(), "night");
    assert_eq!(
        config.theme.style(StyleRole::SyntaxKeyword).foreground,
        gpui_kit::rgb(0x123456).into()
    );
    assert_eq!(
        config.theme.style(StyleRole::Default).foreground,
        gpui_kit::rgb(0x112233).into()
    );
    let cursor = config.theme.style(StyleRole::Cursor);
    assert_eq!(cursor.foreground, gpui_kit::rgb(0x445566).into());
    assert_eq!(cursor.background, gpui_kit::rgb(0x778899).into());
    assert_eq!(load("").unwrap().1.theme, ThemeSettings::default());
}

#[test]
fn a_theme_accepts_every_named_interface_and_syntax_style() {
    use crate::theme::StyleRole;
    let entries = StyleRole::ALL
        .into_iter()
        .map(|role| {
            format!(
                "\"{}\":{{{}:\"#123456\"}}",
                role.name(),
                if role == StyleRole::Region {
                    "background"
                } else {
                    "foreground"
                }
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let (_, config) = load(&format!("semacs.defineTheme('all-styles', {{appearance:'dark', styles:{{{entries}}}}}); semacs.loadTheme('all-styles');")).unwrap();
    for role in StyleRole::ALL {
        let style = config.theme.style(role);
        assert_eq!(
            if role == StyleRole::Region {
                style.background
            } else {
                style.foreground
            },
            gpui_kit::rgb(0x123456).into()
        );
    }
}

#[test]
fn invalid_themes_reject_the_entire_configuration() {
    for source in [
        r##"semacs.loadTheme("missing")"##,
        r##"semacs.defineTheme("dark", {appearance:"dark",styles:{}})"##,
        r##"semacs.defineTheme("a", {appearance:"dark",styles:{}}); semacs.defineTheme("a", {appearance:"dark",styles:{}})"##,
        r##"semacs.defineTheme("Bad", {appearance:"dark",styles:{}})"##,
        r##"semacs.defineTheme("a", {appearance:"auto",styles:{}})"##,
        r##"semacs.defineTheme("a", {appearance:"dark",styles:{},extra:true})"##,
        r##"semacs.defineTheme("a", {appearance:"dark",styles:[]})"##,
        r##"semacs.setStyle("default", {foreground:"#fff"})"##,
        r##"semacs.setStyle("default", {foreground:17})"##,
        r##"semacs.setStyle("default", {background:null})"##,
        r##"semacs.setStyle("default", {foreground:undefined})"##,
        r##"semacs.setStyle("default", {weight:"bold"})"##,
        r##"semacs.setStyle("default", [])"##,
        r##"semacs.setStyle("region", {foreground:"#112233"})"##,
        r##"semacs.setStyle("minibuffer-prompt", {background:"#112233"})"##,
        r##"semacs.setStyle("syntax.unknown", {foreground:"#112233"})"##,
        r##"semacs.setStyle("syntax.keyword", {background:"#112233"})"##,
        r##"for(let i=0;i<33;i++)semacs.defineTheme("t-"+i,{appearance:"dark",styles:{}})"##,
        r##"for(let i=0;i<129;i++)semacs.setStyle("default", {})"##,
    ] {
        assert!(load(source).is_err(), "{source}");
    }
}

#[test]
fn theme_apis_cannot_be_called_from_commands() {
    for call in [
        r##"semacs.loadTheme("dark")"##,
        r##"semacs.defineTheme("x", {appearance:"dark",styles:{}})"##,
        r##"semacs.setStyle("default", {foreground:"#112233"})"##,
    ] {
        let (mut engine, config) = load(&format!(
            "semacs.registerCommand('bad', text => {{ {call}; return text; }})"
        ))
        .unwrap();
        assert!(
            engine
                .transform(script(&config, "bad"), "abc")
                .unwrap_err()
                .contains("only available")
        );
    }
}

#[test]
fn invalid_configuration_is_rejected_as_a_whole() {
    for source in [
        "semacs.registerCommand('forward-char', text => text)",
        "semacs.registerCommand('minibuffer-complete', text => text)",
        "semacs.registerCommand('UPPER', text => text)",
        "semacs.registerCommand('ok', text => text); semacs.registerCommand('ok', text => text)",
        "semacs.bindKey('ctrl-f', 'does-not-exist')",
        "semacs.bindKey('ctrl-f', 'minibuffer-complete')",
        "semacs.bindKey('', 'forward-char')",
        "semacs.bindKey('ctrl-x', 'forward-char')",
        "semacs.bindKey('ctrl-x', null)",
        "semacs.bindKey('ctrl-f a', 'forward-char')",
        "semacs.bindKey('alt-x', null); semacs.bindKey('escape x', null)",
        "semacs.setFont({size: 1})",
        "semacs.setFont({family: ''})",
        "semacs.setFont({size: NaN})",
        "semacs.setFont({weight: 'bold'})",
        "semacs.registerCommand('bad', 3)",
        "throw new Error('broken init')",
        "const syntax = ;",
    ] {
        assert!(load(source).is_err(), "{source}");
    }
}

#[test]
fn configurations_are_fresh_and_do_not_accumulate_state() {
    let (_, old) =
        load("semacs.registerCommand('old', text => text); semacs.setFont({size: 24})").unwrap();
    let (_, next) = load("").unwrap();
    assert!(old.commands.named("old").is_some());
    assert!(next.commands.named("old").is_none());
    assert_eq!(f32::from(next.font.base_size()), 16.);
    assert_eq!(
        binding(&next, "ctrl-f"),
        Lookup::Command(Command::ForwardChar)
    );
}

#[test]
fn script_errors_types_and_obsolete_id_are_isolated() {
    let (mut engine, configuration) = load(r#"
        semacs.registerCommand("failure", () => { throw new Error("boom"); });
        semacs.registerCommand("wrong-type", () => 42);
        semacs.registerCommand("registration", text => { semacs.bindKey("ctrl-f", null); return text; });
        semacs.registerCommand("success", text => text + "!");
    "#).unwrap();
    for name in ["failure", "wrong-type", "registration"] {
        assert!(
            engine
                .transform(script(&configuration, name), "abc")
                .is_err()
        );
    }
    let command = script(&configuration, "success");
    assert_eq!(engine.transform(command, "abc").unwrap(), "abc!");
    assert!(
        engine
            .transform(
                ScriptCommand {
                    generation: 41,
                    ..command
                },
                "abc"
            )
            .is_err()
    );
    assert!(
        engine
            .transform(
                ScriptCommand {
                    index: usize::MAX,
                    ..command
                },
                "abc"
            )
            .is_err()
    );
}

#[test]
fn async_configuration_and_command_results_are_not_supported() {
    assert!(load("Promise.resolve().then(() => semacs.setFont({size: 20}))").is_err());
    let (mut engine, configuration) =
        load("semacs.registerCommand('async', async text => text)").unwrap();
    assert!(
        engine
            .transform(script(&configuration, "async"), "abc")
            .is_err()
    );
    let (mut engine, configuration) = load(
        "semacs.registerCommand('job', text => { Promise.resolve().then(() => {}); return text; })",
    )
    .unwrap();
    assert!(
        engine
            .transform(script(&configuration, "job"), "abc")
            .is_err()
    );
    assert!(
        engine
            .transform(script(&configuration, "job"), "abc")
            .is_err()
    );
}

#[test]
fn execution_memory_and_output_limits_are_enforced() {
    assert!(load("while (true) {}").err().unwrap().contains("500 ms"));
    let (mut engine, configuration) = load(r#"
        semacs.registerCommand("loop", () => { while (true) {} });
        semacs.registerCommand("large", () => "x".repeat(262145));
        semacs.registerCommand("memory", () => "x".repeat(100000000));
        semacs.registerCommand("stack", () => { function recurse() { return recurse() + 1; } return recurse(); });
        semacs.registerCommand("good", text => text);
    "#).unwrap();
    assert!(
        engine
            .transform(script(&configuration, "loop"), "")
            .unwrap_err()
            .contains("100 ms")
    );
    for name in ["large", "memory", "stack"] {
        assert!(
            engine.transform(script(&configuration, name), "").is_err(),
            "{name}"
        );
    }
    assert_eq!(
        engine
            .transform(script(&configuration, "good"), "ok")
            .unwrap(),
        "ok"
    );
}

#[test]
fn declaration_counts_are_bounded() {
    assert!(
        load("for (let i = 0; i < 257; i++) semacs.registerCommand('command-' + i, text => text)")
            .is_err()
    );
    assert!(load("for (let i = 0; i < 1025; i++) semacs.bindKey('ctrl-f', null)").is_err());
}

#[test]
fn keymap_validation_uses_final_bindings_not_declaration_order() {
    for source in [
        "semacs.bindKey('ctrl-f', null); semacs.bindKey('ctrl-f a', 'forward-char')",
        "semacs.bindKey('ctrl-f a', 'forward-char'); semacs.bindKey('ctrl-f', null)",
    ] {
        let (_, configuration) = load(source).unwrap();
        assert_eq!(binding(&configuration, "ctrl-f"), Lookup::Prefix);
        assert_eq!(
            binding(&configuration, "ctrl-f a"),
            Lookup::Command(Command::ForwardChar)
        );
    }
}

#[test]
fn nested_unbinding_marks_are_merged_in_either_declaration_order() {
    for source in [
        "semacs.bindKey('escape escape escape', null); semacs.bindKey('escape escape', null)",
        "semacs.bindKey('escape escape', null); semacs.bindKey('escape escape escape', null)",
    ] {
        let (_, configuration) = load(source).unwrap();
        let defaults = crate::keymap::Keymap::for_body();
        for keys in ["escape escape", "escape escape escape"] {
            assert_eq!(
                lookup(
                    &[&configuration.body_keymap, &defaults],
                    &parse_sequence(keys).unwrap()
                ),
                Lookup::Unbound
            );
        }
        assert_eq!(
            binding(&configuration, "alt-x"),
            Lookup::Command(Command::ExecuteExtendedCommand)
        );
    }
    assert!(load("semacs.bindKey('escape escape', null)").is_err());
}

#[test]
fn empty_configuration_preserves_native_body_lookup() {
    let (_, configuration) = load("").unwrap();
    let defaults = crate::keymap::Keymap::for_body();
    for spec in crate::command::COMMANDS
        .iter()
        .filter(|spec| spec.scope != crate::command::Scope::ReadSession)
    {
        for default in spec.bindings {
            assert_eq!(
                binding(&configuration, default.keys),
                lookup(&[&defaults], &parse_sequence(default.keys).unwrap())
            );
        }
    }
    for keys in [
        "ctrl-x",
        "ctrl-h",
        "escape",
        "escape escape",
        "alt-escape",
        "ctrl-x ctrl-g",
    ] {
        assert_eq!(
            binding(&configuration, keys),
            lookup(&[&defaults], &parse_sequence(keys).unwrap())
        );
    }
}

#[test]
fn worker_request_queue_and_input_are_bounded_without_blocking() {
    let (requests, receiver) = std::sync::mpsc::sync_channel(1);
    let host = super::ScriptHost {
        generation: 42,
        requests,
    };
    let command = ScriptCommand {
        generation: 42,
        index: 0,
    };
    assert!(
        host.transform(command, "x".repeat(MAX_TEXT_BYTES + 1))
            .is_err()
    );
    assert!(
        host.transform(
            ScriptCommand {
                generation: 41,
                ..command
            },
            "abc".into()
        )
        .is_err()
    );
    let first = host.transform(command, "abc".into()).unwrap();
    assert!(
        host.transform(command, "def".into())
            .err()
            .unwrap()
            .contains("busy")
    );
    drop(first);
    assert!(receiver.recv().unwrap().reply.is_canceled());
    drop(receiver);
    assert!(
        host.transform(command, "abc".into())
            .err()
            .unwrap()
            .contains("stopped")
    );
}

#[test]
fn configuration_paths_never_fall_back_to_the_working_directory() {
    assert_eq!(
        super::config_path_from(Some("/xdg".into()), Some("/home/user".into())).unwrap(),
        std::path::PathBuf::from("/xdg/semacs/init.js")
    );
    for xdg in [None, Some("".into()), Some("relative".into())] {
        assert_eq!(
            super::config_path_from(xdg, Some("/home/user".into())).unwrap(),
            std::path::PathBuf::from("/home/user/.config/semacs/init.js")
        );
    }
    assert!(super::config_path_from(None, None).is_err());
    assert!(super::config_path_from(Some("project".into()), Some(".".into())).is_err());
}

#[test]
fn no_external_io_apis_are_installed() {
    let (mut engine, configuration) = load(r#"
        semacs.registerCommand("check", () =>
            [typeof process, typeof require, typeof fetch, typeof setTimeout, typeof std, typeof os].join(","));
    "#).unwrap();
    assert_eq!(
        engine
            .transform(script(&configuration, "check"), "")
            .unwrap(),
        "undefined,undefined,undefined,undefined,undefined,undefined"
    );
}
