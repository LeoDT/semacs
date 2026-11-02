use crate::command::Command;
use crate::command_registry::{CommandId, CommandRegistry, ScriptCommand};

#[test]
fn registry_resolves_native_and_script_commands_but_not_session_commands() {
    let mut registry = CommandRegistry::default();
    assert_eq!(
        registry.named("forward-char"),
        Some(CommandId::Native(Command::ForwardChar))
    );
    assert_eq!(
        registry.named("save-buffer"),
        Some(CommandId::Native(Command::SaveBuffer))
    );
    assert_eq!(registry.named("minibuffer-complete"), None);
    assert_eq!(registry.named("nosuch"), None);
    let id = ScriptCommand {
        generation: 5,
        index: 0,
    };
    registry.register_script("uppercase-region", id).unwrap();
    assert_eq!(
        registry.named("uppercase-region"),
        Some(CommandId::Script(id))
    );
    assert!(registry.register_script("forward-char", id).is_err());
    assert!(registry.register_script("minibuffer-complete", id).is_err());
    assert!(registry.register_script("uppercase-region", id).is_err());
}
