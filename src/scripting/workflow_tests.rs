use super::tests::load;
use super::*;
use crate::command_registry::{CommandId, ScriptKind};
use crate::diff::DiffKind;
use crate::keymap::{Lookup, lookup, parse_sequence};
use crate::mode::ModeId;
use crate::status::Group;

fn command(config: &Configuration, name: &str) -> crate::command_registry::ScriptCommand {
    let CommandId::Script(id) = config.commands.named(name).unwrap() else {
        panic!()
    };
    id
}

fn context() -> WorkflowContext {
    WorkflowContext {
        mode: ModeId::MAGIT_STATUS,
        selection: Some(StatusSelection {
            group: Group::Unstaged,
            can_stage: true,
            can_unstage: false,
        }),
    }
}

#[test]
fn bundled_handlers_share_registry_modes_limits_and_overridable_keys() {
    let (mut engine, config) = load("").unwrap();
    let stage = command(&config, "magit-stage");
    let unstage = command(&config, "magit-unstage");
    assert_eq!(
        config.commands.script_kind(stage),
        Some(ScriptKind::Workflow)
    );
    assert_eq!(
        engine.workflow(stage, context()).unwrap(),
        Some(WorkflowRequest::Stage)
    );
    assert!(engine.workflow(unstage, context()).is_err());
    assert_eq!(
        lookup(
            &[&config.mode_keymaps[&ModeId::MAGIT_STATUS]],
            &parse_sequence("s").unwrap()
        ),
        Lookup::Script(stage)
    );
    assert!(
        !config
            .commands
            .commands_for(ModeId::MAGIT_DIFF)
            .any(|entry| entry.id == CommandId::Script(stage))
    );
    assert_eq!(
        config
            .commands
            .commands_for(ModeId::MAGIT_STATUS)
            .find(|entry| entry.id == CommandId::Script(stage))
            .unwrap()
            .annotation,
        "s"
    );
    let (_, config) = load("semacs.bindKey('s', null, {mode:'magit-status'}); semacs.bindKey('S', 'magit-stage', {mode:'magit-status'});").unwrap();
    assert_eq!(
        lookup(
            &[&config.mode_keymaps[&ModeId::MAGIT_STATUS]],
            &parse_sequence("s").unwrap()
        ),
        Lookup::Unbound
    );
    assert_eq!(
        lookup(
            &[&config.mode_keymaps[&ModeId::MAGIT_STATUS]],
            &parse_sequence("S").unwrap()
        ),
        Lookup::Script(command(&config, "magit-stage"))
    );
    assert!(load("for(let i=0;i<254;i++) semacs.registerCommand('c-'+i, t=>t)").is_ok());
    assert!(load("for(let i=0;i<255;i++) semacs.registerCommand('c-'+i, t=>t)").is_err());
}

#[test]
fn workflow_declarations_validate_names_modes_and_all_registration_sources() {
    for source in [
        "semacs.registerCommand('magit-stage', x=>x)",
        "semacs.registerWorkflowCommand('magit-status', {run:()=>null})",
        "semacs.registerWorkflowCommand('Bad', {run:()=>null})",
        "semacs.registerWorkflowCommand('x', {run:()=>null, modes:[]})",
        "semacs.registerWorkflowCommand('x', {run:()=>null, modes:['missing']})",
        "semacs.registerWorkflowCommand('x', {run:()=>null, modes:['magit-status','magit-status']})",
        "semacs.registerWorkflowCommand('x', {run:()=>null, modes:'magit-status'})",
        "semacs.registerWorkflowCommand('x', {run:()=>null, extra:true})",
        "semacs.registerWorkflowCommand('x', {run:42})",
        "semacs.registerWorkflowCommand('x', null)",
        "semacs.registerWorkflowCommand('x', {run:()=>null}); semacs.registerCommand('x', x=>x)",
        "semacs.bindKey('s','magit-stage',{mode:'magit-diff'})",
    ] {
        assert!(load(source).is_err(), "{source}");
    }
}

#[test]
fn workflow_context_is_bounded_deeply_frozen_and_captured_before_user_mutations() {
    let (mut engine, config) = load(r#"
        Object.freeze = value => value;
        semacs.registerWorkflowCommand("inspect", {
            run: context => {
                if (!Object.isFrozen(context) || !Object.isFrozen(context.status) ||
                    !Object.isFrozen(context.status.selection)) throw Error("not frozen");
                if (Object.keys(context).sort().join() !== "mode,status" ||
                    Object.keys(context.status).join() !== "selection" ||
                    Object.keys(context.status.selection).sort().join() !== "canStage,canUnstage,group")
                    throw Error("unexpected payload");
                try { context.status.selection.canStage = false; } catch (_) {}
                if (!context.status.selection.canStage || context.mode !== "magit-status") throw Error("mutable");
                return {operation:"magit.stage"};
            }
        });
    "#).unwrap();
    assert_eq!(
        engine
            .workflow(command(&config, "inspect"), context())
            .unwrap(),
        Some(WorkflowRequest::Stage)
    );
    let (mut engine, config) = load(
        r#"
        semacs.registerWorkflowCommand("inspect", {run: c => {
            if (c.mode === "fundamental" && c.status !== null) throw Error("not null");
            if (c.mode === "magit-status" && c.status.selection !== null) throw Error("not null");
            return null;
        }});
    "#,
    )
    .unwrap();
    for mode in [ModeId::FUNDAMENTAL, ModeId::MAGIT_STATUS] {
        assert_eq!(
            engine
                .workflow(
                    command(&config, "inspect"),
                    WorkflowContext {
                        mode,
                        selection: None
                    }
                )
                .unwrap(),
            None
        );
    }
}

#[test]
fn workflow_accepts_only_exact_typed_requests_and_never_transform_output() {
    for (value, expected) in [
        ("null", None),
        ("{operation:'magit.stage'}", Some(WorkflowRequest::Stage)),
        (
            "{operation:'magit.unstage'}",
            Some(WorkflowRequest::Unstage),
        ),
        (
            "{operation:'magit.status.open'}",
            Some(WorkflowRequest::StatusOpen),
        ),
        (
            "{operation:'magit.status.refresh'}",
            Some(WorkflowRequest::StatusRefresh),
        ),
        (
            "{operation:'magit.diff.open', comparison:'staged'}",
            Some(WorkflowRequest::DiffOpen(DiffKind::Staged)),
        ),
        (
            "{operation:'magit.diff.open', comparison:'unstaged'}",
            Some(WorkflowRequest::DiffOpen(DiffKind::Unstaged)),
        ),
    ] {
        let (mut engine, config) = load(&format!(
            "semacs.registerWorkflowCommand('x', {{run:()=>({value})}})"
        ))
        .unwrap();
        let id = command(&config, "x");
        assert_eq!(engine.workflow(id, context()).unwrap(), expected);
        assert!(engine.transform(id, "").is_err());
    }
    let (mut engine, config) = load("semacs.registerCommand('x', x=>x)").unwrap();
    assert!(engine.workflow(command(&config, "x"), context()).is_err());
}

#[test]
fn invalid_requests_getters_promises_and_pending_jobs_accept_nothing() {
    for value in [
        "undefined",
        "false",
        "42",
        "'magit.stage'",
        "[]",
        "{}",
        "[{operation:'magit.stage'}]",
        "{operation:'magit.stage', path:'a'}",
        "{operation:'magit.stage', comparison:'staged'}",
        "{operation:'git.add'}",
        "{operation:'magit.diff.open'}",
        "{operation:'magit.diff.open',comparison:'other'}",
        "{operation:'magit.diff.open',comparison:'staged',args:[]}",
        "Promise.resolve({operation:'magit.stage'})",
        "Object.create({operation:'magit.stage'})",
        "Object.defineProperty({operation:'magit.stage'},'extra',{value:1})",
        "({operation:'magit.stage', [Symbol('secret')]:true})",
        "({[Symbol('operation')]:'magit.stage', get operation(){return 'magit.stage'}})",
        "({get operation(){throw Error('no')}})",
        "({operation:'x'.repeat(1000000)})",
        "({operation:'magit.stage', ['x'.repeat(1000000)]:true})",
        "({get operation(){Promise.resolve().then(()=>{}); return 'magit.stage'}})",
        "({get operation(){while(true){}}})",
    ] {
        let (mut engine, config) = load(&format!(
            "semacs.registerWorkflowCommand('x', {{run:()=>({value})}})"
        ))
        .unwrap();
        assert!(
            engine.workflow(command(&config, "x"), context()).is_err(),
            "{value}"
        );
    }
    let (mut engine, config) = load("semacs.registerWorkflowCommand('x',{run:()=>{Promise.resolve().then(()=>{});return {operation:'magit.stage'}}})").unwrap();
    assert!(engine.workflow(command(&config, "x"), context()).is_err());
    assert!(
        engine
            .workflow(command(&config, "magit-stage"), context())
            .unwrap_err()
            .contains("reload")
    );
}

#[test]
fn workflow_exceptions_timeout_and_declaration_during_execution_fail() {
    for body in [
        "throw Error('controlled')",
        "while(true){}",
        "semacs.registerWorkflowCommand('later',{run:()=>null}); return null",
    ] {
        let (mut engine, config) = load(&format!(
            "semacs.registerWorkflowCommand('x', {{run:()=>{{{body}}}}})"
        ))
        .unwrap();
        assert!(engine.workflow(command(&config, "x"), context()).is_err());
    }
}

#[test]
fn actual_default_worker_has_real_handlers_and_one_shared_request_queue() {
    let loaded = gpui_kit::block_on(load_defaults()).unwrap();
    let id = command(&loaded.configuration, "magit-stage");
    assert_eq!(
        gpui_kit::block_on(loaded.host.workflow(id, context()).unwrap())
            .unwrap()
            .unwrap(),
        Some(WorkflowRequest::Stage)
    );
    let (requests, receiver) = sync_channel(1);
    let host = ScriptHost {
        generation: id.generation,
        requests,
    };
    let pending = host.workflow(id, context()).unwrap();
    assert!(
        host.transform(id, "x".into())
            .err()
            .unwrap()
            .contains("busy")
    );
    drop(pending);
    assert!(receiver.recv().unwrap().is_canceled());
}
