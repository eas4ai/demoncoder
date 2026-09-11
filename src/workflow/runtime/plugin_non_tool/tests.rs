use super::*;
use crate::plugins::hook_types::{HandlerKind, HookDialect};
use serde_json::json;

#[test]
fn native_turn_bare_owner_never_borrows_a_later_phase() {
    for with_task in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let mut record = crate::inspection::tests::record(root.path());
        if with_task {
            record.task = Some(
                crate::workflow::state::Task::new(
                    1,
                    "actual task".into(),
                    vec![],
                    crate::workflow::workspace::capture(root.path()).unwrap(),
                    1,
                )
                .unwrap(),
            );
        }
        let runtime = SharedRuntime::for_test(&root.path().join("record"), record).unwrap();
        let turn = runtime
            .begin_native_turn("worker", None, NativeTurnOrigin::Developer)
            .unwrap();
        let saved = runtime.record().unwrap();
        let Some(HostInvocation::NativeTurn(facts)) = &saved.operations[0].host_invocation else {
            panic!("native turn missing");
        };
        assert_eq!(facts.task, with_task.then_some(1));
        assert!(facts.owner_phase.is_none());
        assert!(saved.allocation.is_none());
        let parallel = runtime
            .begin_native_turn("worker", None, NativeTurnOrigin::Developer)
            .unwrap();
        assert_ne!(
            turn, parallel,
            "independent bare sessions need distinct identities"
        );
        runtime
            .finish_native_turn(parallel, NativeTurnEnd::Complete)
            .unwrap();
        runtime
            .update(|r| {
                r.phase = Some("worker".into());
                Ok(())
            })
            .unwrap();
        assert!(
            runtime
                .begin_non_tool_as(
                    "worker",
                    None,
                    Some(turn),
                    NonToolOccurrence::Stop {
                        stop_hook_active: false,
                        last_assistant_message: None
                    },
                    "plan".into(),
                    vec![]
                )
                .is_err()
        );
        runtime
            .finish_native_turn(turn, NativeTurnEnd::Complete)
            .unwrap();
    }
}

#[test]
fn native_turn_real_reopen_holds_unfinished_bare_turn_without_replay() {
    let root = tempfile::tempdir().unwrap();
    let connection: crate::config::Connection =
        serde_json::from_value(json!({"adapter":"openai-api","model":"native-model"})).unwrap();
    let (runtime, _) = SharedRuntime::open(root.path(), &connection, None).unwrap();
    let id = runtime
        .begin_native_turn("worker", None, NativeTurnOrigin::PluginContext)
        .unwrap();
    let directory = runtime.directory().unwrap();
    drop(runtime);
    let (runtime, resumed) =
        SharedRuntime::open(root.path(), &connection, Some(&directory)).unwrap();
    assert!(resumed && runtime.record().unwrap().recovery_pending);
    assert_eq!(runtime.record().unwrap().operations.len(), 1);
    assert!(
        runtime
            .begin_native_turn("worker", None, NativeTurnOrigin::Developer)
            .is_err()
    );
    runtime
        .reconcile("examined interrupted native turn", None)
        .unwrap();
    let next = runtime
        .begin_native_turn("worker", None, NativeTurnOrigin::Developer)
        .unwrap();
    assert_ne!(next, id);
    assert!(runtime.record().unwrap().operations[0].reconciled);
    runtime
        .finish_native_turn(next, NativeTurnEnd::Complete)
        .unwrap();
    drop(runtime);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn native_turn_shared_identity_does_not_cross_stop_reservations_or_outcomes() {
    let root = tempfile::tempdir().unwrap();
    let (runtime, legacy, mut hook) = fixture(root.path());
    let declarations = runtime.record().unwrap().operations[0]
        .non_tool_receipt()
        .unwrap()
        .declarations
        .clone();
    runtime
        .settle_non_tool(legacy, HookEvent::UserPromptSubmit, Default::default())
        .unwrap();
    let turn = runtime
        .begin_native_turn("worker", None, NativeTurnOrigin::Developer)
        .unwrap();
    let first = runtime
        .begin_non_tool_as(
            "worker",
            None,
            Some(turn),
            NonToolOccurrence::Stop {
                stop_hook_active: false,
                last_assistant_message: Some("first answer".into()),
            },
            "plan".into(),
            declarations.clone(),
        )
        .unwrap();
    hook.inspected.operation = first.operation;
    hook.inspected.source_operation = turn;
    hook.inspected.event = "Stop".into();
    hook.inspected.lifecycle = Some(first.subject);
    runtime
        .reserve_non_tool_hook(first.operation, HookEvent::Stop, hook.clone(), None, None)
        .unwrap();
    hook.outcome = Some(RawOutcome::Callback { value: json!({}) });
    hook.uncertain_effects = false;
    runtime
        .finish_non_tool_hook(first.operation, HookEvent::Stop, hook.clone())
        .unwrap();
    runtime
        .settle_non_tool(first.operation, HookEvent::Stop, Default::default())
        .unwrap();
    let second = runtime
        .begin_non_tool_as(
            "worker",
            None,
            Some(turn),
            NonToolOccurrence::Stop {
                stop_hook_active: true,
                last_assistant_message: Some("corrected answer".into()),
            },
            "plan".into(),
            declarations,
        )
        .unwrap();
    assert!(
        runtime
            .reserve_non_tool_hook(second.operation, HookEvent::Stop, hook.clone(), None, None)
            .is_err()
    );
    let mut current = hook.clone();
    current.inspected.operation = second.operation;
    current.inspected.lifecycle = Some(second.subject);
    current.outcome = None;
    current.uncertain_effects = true;
    runtime
        .reserve_non_tool_hook(
            second.operation,
            HookEvent::Stop,
            current.clone(),
            None,
            None,
        )
        .unwrap();
    let key = runtime
        .plugin_hook_key(second.operation, HookEvent::Stop, 0)
        .unwrap();
    assert_eq!(key.source_operation, turn);
    assert_eq!(key.operation, second.operation);
    assert!(
        runtime
            .finish_non_tool_hook(second.operation, HookEvent::Stop, hook)
            .is_err()
    );
    current.outcome = Some(RawOutcome::Callback { value: json!({}) });
    current.uncertain_effects = false;
    runtime
        .finish_non_tool_hook(second.operation, HookEvent::Stop, current)
        .unwrap();
}

#[test]
fn native_turn_plugin_origin_cannot_authorize_a_fabricated_initial_submission() {
    let root = tempfile::tempdir().unwrap();
    let mut record = crate::inspection::tests::record(root.path());
    record.phase = Some("worker".into());
    let runtime = SharedRuntime::for_test(&root.path().join("record"), record).unwrap();
    let turn = runtime
        .begin_native_turn("worker", None, NativeTurnOrigin::PluginContext)
        .unwrap();
    assert!(
        runtime
            .begin_non_tool_as(
                "worker",
                None,
                Some(turn),
                NonToolOccurrence::UserPromptSubmit {
                    prompt: "observer context is not a developer request".into(),
                    correction: false,
                },
                "plan".into(),
                vec![]
            )
            .is_err(),
        "plugin turn fabricated a developer submission"
    );
}

#[test]
fn native_turn_unfinished_reload_and_ended_owner_cannot_be_reused() {
    let root = tempfile::tempdir().unwrap();
    let mut record = crate::inspection::tests::record(root.path());
    record.phase = Some("worker".into());
    let path = root.path().join("record");
    let runtime = SharedRuntime::for_test(&path, record).unwrap();
    let turn = runtime
        .begin_native_turn("worker", None, NativeTurnOrigin::PluginContext)
        .unwrap();
    assert!(
        runtime
            .begin_native_turn("worker", None, NativeTurnOrigin::Developer)
            .is_err()
    );
    let saved = runtime.record().unwrap();
    assert!(saved.operations[0].needs_reconciliation());
    let decoded: Record =
        serde_json::from_value(runtime.0.lock().unwrap().store.read().unwrap()).unwrap();
    assert!(decoded.operations[0].needs_reconciliation());
    let reopened = SharedRuntime::for_test(&root.path().join("reopened"), decoded).unwrap();
    assert!(
        reopened
            .begin_native_turn("worker", None, NativeTurnOrigin::Developer)
            .is_err()
    );
    reopened
        .finish_native_turn(turn, NativeTurnEnd::Cancelled)
        .unwrap();
    assert!(
        reopened
            .begin_non_tool_as(
                "worker",
                None,
                Some(turn),
                NonToolOccurrence::Stop {
                    stop_hook_active: false,
                    last_assistant_message: None
                },
                "plan".into(),
                vec![]
            )
            .is_err()
    );
}

fn fixture(root: &std::path::Path) -> (SharedRuntime, u64, HookReceipt) {
    let mut record = crate::inspection::tests::record(root);
    record.phase = Some("worker".into());
    let runtime = SharedRuntime::for_test(&root.join("record"), record).unwrap();
    let declaration = DeclarationIdentity {
        package: "fixture".into(),
        code: "code".into(),
        policy: "policy".into(),
        configuration: "config".into(),
        generation: "1".into(),
        scope: Scope::Project,
        role: "worker".into(),
        declaration: "submit".into(),
        index: 0,
        dialect: HookDialect::Native,
        runner: HandlerKind::Command,
    };
    let facts = runtime
        .begin_non_tool(
            "worker",
            NonToolOccurrence::UserPromptSubmit {
                prompt: "actual user request".into(),
                correction: false,
            },
            "plan".into(),
            vec![json!({"identity": declaration, "required_gate": true})],
        )
        .unwrap();
    let hook = HookReceipt {
        required_gate: true,
        observer: None,
        source: None,
        once: None,
        invocation: 0,
        declaration,
        class: HandlerClass::Combined,
        endpoint: None,
        inspected: AdmissionKey {
            session: facts.session,
            operation: facts.operation,
            source_operation: facts.operation,
            event: "UserPromptSubmit".into(),
            tool: None,
            arguments: None,
            lifecycle: Some(facts.subject),
            plan: "plan".into(),
            role: "worker".into(),
            workspace: facts.workspace,
            inputs: vec![],
            external: None,
        },
        outcome: None,
        uncertain_effects: true,
        hold: None,
        questions: vec![],
        pending_proposals: vec![],
    };
    (runtime, facts.operation, hook)
}

#[test]
fn non_tool_reservation_uses_real_occurrence_without_tool_or_model_authority() {
    let root = tempfile::tempdir().unwrap();
    let (runtime, id, hook) = fixture(root.path());
    runtime
        .reserve_non_tool_hook(id, HookEvent::UserPromptSubmit, hook.clone(), None, None)
        .unwrap();
    assert_eq!(
        runtime
            .plugin_hook_key(id, HookEvent::UserPromptSubmit, 0)
            .unwrap(),
        hook.inspected
    );
    assert!(runtime.plugin_hook_key(id, HookEvent::Stop, 0).is_err());
    assert!(
        runtime
            .plugin_hook_key(id, HookEvent::PreToolUse, 0)
            .is_err()
    );
    let record = runtime.record().unwrap();
    assert_eq!(record.operations.len(), 1);
    assert!(record.allocation.is_none());
    let operation = &record.operations[0];
    assert!(
        operation.call.is_none() && operation.result.is_none() && operation.tool_receipt.is_none()
    );
    assert_eq!(operation.all_plugin_hooks().count(), 1);
    assert!(operation.needs_reconciliation());
    assert_eq!(
        operation
            .non_tool_receipt()
            .unwrap()
            .facts
            .subject
            .occurrence,
        NonToolOccurrence::UserPromptSubmit {
            prompt: "actual user request".into(),
            correction: false
        }
    );
}

#[test]
fn non_tool_rejects_wrong_subject_workspace_session_and_changed_owner() {
    for change in [
        "event",
        "subject",
        "tool",
        "workspace",
        "session",
        "phase",
        "identity",
    ] {
        let root = tempfile::tempdir().unwrap();
        let (runtime, id, mut hook) = fixture(root.path());
        match change {
            "event" => hook.inspected.event = "Stop".into(),
            "subject" => {
                hook.inspected.lifecycle.as_mut().unwrap().occurrence = NonToolOccurrence::Stop {
                    stop_hook_active: false,
                    last_assistant_message: None,
                }
            }
            "tool" => hook.inspected.tool = Some("read".into()),
            "workspace" => hook.inspected.workspace = (0, 0),
            "session" => hook.inspected.session = "other".into(),
            "phase" => runtime
                .update(|r| {
                    r.phase = None;
                    Ok(())
                })
                .unwrap(),
            _ => runtime
                .update(|r| {
                    r.operations[0].identity = None;
                    Ok(())
                })
                .unwrap(),
        }
        assert!(
            runtime
                .reserve_non_tool_hook(id, HookEvent::UserPromptSubmit, hook, None, None)
                .is_err(),
            "{change}"
        );
        assert_eq!(
            runtime.record().unwrap().operations[0]
                .all_plugin_hooks()
                .count(),
            0
        );
    }
}

#[test]
fn interrupted_non_tool_reservation_survives_reload_and_never_replays() {
    let root = tempfile::tempdir().unwrap();
    let (runtime, id, hook) = fixture(root.path());
    runtime
        .reserve_non_tool_hook(id, HookEvent::UserPromptSubmit, hook, None, None)
        .unwrap();
    runtime.finish_phase().unwrap();
    let retained = runtime.record().unwrap();
    assert!(retained.recovery_pending);
    let reloaded: Record = serde_json::from_value(serde_json::to_value(retained).unwrap()).unwrap();
    let reopened = SharedRuntime::for_test(&root.path().join("reopened"), reloaded).unwrap();
    assert!(
        reopened
            .plugin_runner_owner(id, HookEvent::UserPromptSubmit)
            .is_err()
    );
    assert!(reopened.begin_phase("worker", None).is_err());
    assert_eq!(
        reopened.record().unwrap().operations[0]
            .all_plugin_hooks()
            .count(),
        1
    );
    reopened
        .reconcile("inspected interrupted lifecycle", None)
        .unwrap();
    assert!(
        reopened
            .plugin_runner_owner(id, HookEvent::UserPromptSubmit)
            .is_err()
    );
}

#[test]
fn non_tool_raw_settlement_is_exact_and_does_not_grant_continuation() {
    let root = tempfile::tempdir().unwrap();
    let (runtime, id, mut hook) = fixture(root.path());
    runtime
        .reserve_non_tool_hook(id, HookEvent::UserPromptSubmit, hook.clone(), None, None)
        .unwrap();
    hook.outcome = Some(RawOutcome::Callback { value: json!({}) });
    hook.uncertain_effects = false;
    let mut forged = hook.clone();
    forged.inspected.session = "other".into();
    assert!(
        runtime
            .finish_non_tool_hook(id, HookEvent::UserPromptSubmit, forged)
            .is_err()
    );
    runtime
        .finish_non_tool_hook(id, HookEvent::UserPromptSubmit, hook.clone())
        .unwrap();
    assert!(
        runtime
            .finish_non_tool_hook(id, HookEvent::UserPromptSubmit, hook)
            .is_err()
    );
    let record = runtime.record().unwrap();
    assert!(record.operations[0].needs_reconciliation());
    assert!(!record.operations[0].complete);
    assert!(!record.operations[0].non_tool_receipt().unwrap().settled);
}
