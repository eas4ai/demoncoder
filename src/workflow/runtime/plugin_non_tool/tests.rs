use super::*;
use crate::plugins::hook_types::{HandlerKind, HookDialect};
use serde_json::json;

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
