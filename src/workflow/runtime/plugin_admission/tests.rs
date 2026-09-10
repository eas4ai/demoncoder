use super::*;
use crate::{config::Connection, tools::ToolCall, workflow::runtime::Identity};
use serde_json::json;
fn fixture(root: &std::path::Path) -> (SharedRuntime, u64, ToolCall) {
    fixture_at(root, &root.join("record"))
}
fn fixture_at(root: &std::path::Path, store: &std::path::Path) -> (SharedRuntime, u64, ToolCall) {
    let connection: Connection = serde_json::from_value(json!({"adapter":"openai-api"})).unwrap();
    let record:Record=serde_json::from_value(json!({"workspace":root,"identity":Identity::from(&connection),"archived":[],"next_task":1,"checkpoint_cursor":0,"operations":[],"messages":[],"recovery_pending":false,"decisions":[]})).unwrap();
    let runtime = SharedRuntime::for_test(store, record).unwrap();
    let source = runtime.begin_model("worker").unwrap();
    let call = ToolCall {
        id: "call".into(),
        name: "write".into(),
        arguments: json!({"path":"output","content":"value"}),
    };
    let id = match runtime.begin_tool("worker", source, &call).unwrap() {
        crate::workflow::runtime::ToolAdmission::Fresh(id) => id,
        _ => panic!("fresh tool expected"),
    };
    runtime
        .begin_plugin_plan(id, "plan".into(), vec![])
        .unwrap();
    (runtime, id, call)
}
fn key(runtime: &SharedRuntime, id: u64, call: &ToolCall) -> AdmissionKey {
    AdmissionKey {
        session: runtime.plugin_session().unwrap(),
        operation: id,
        source_operation: runtime.plugin_owner(id).unwrap().0,
        event: "PreToolUse".into(),
        tool: call.name.clone(),
        arguments: crate::plugins::admission::candidate_digest(call).unwrap(),
        plan: "plan".into(),
        role: "worker".into(),
        workspace: (1, 2),
        inputs: vec![],
        external: None,
    }
}
#[test]
fn final_key_cannot_change_host_role_or_tool_identity() {
    for field in ["role", "tool", "event", "session"] {
        let root = tempfile::tempdir().unwrap();
        let (runtime, id, call) = fixture(root.path());
        let mut key = key(&runtime, id, &call);
        match field {
            "role" => key.role = "reviewer".into(),
            "tool" => key.tool = "bash".into(),
            "session" => key.session = "different-session".into(),
            _ => key.event = "PostToolUse".into(),
        }
        assert!(
            runtime.freeze_plugin(id, key, &call, None).is_err(),
            "{field} mismatch was accepted"
        );
        assert!(
            runtime
                .record()
                .unwrap()
                .operations
                .last()
                .unwrap()
                .tool_receipt
                .as_ref()
                .unwrap()
                .plugin_admission
                .as_ref()
                .unwrap()
                .final_key
                .is_none()
        );
    }
}
#[test]
fn changed_arguments_cannot_use_an_earlier_frozen_key() {
    let root = tempfile::tempdir().unwrap();
    let (runtime, id, mut call) = fixture(root.path());
    let key = key(&runtime, id, &call);
    runtime.freeze_plugin(id, key, &call, None).unwrap();
    call.arguments["path"] = json!("different");
    assert!(
        runtime.admit_tool(id, &call).is_err(),
        "changed arguments crossed the durable final key"
    );
}

#[test]
fn same_workspace_sessions_cannot_reuse_keys_and_resume_preserves_identity() {
    use crate::workflow::store::Store;
    let root = tempfile::tempdir().unwrap();
    let store_a = root.path().join("session-a");
    let (a, id_a, call_a) = fixture_at(root.path(), &store_a);
    let (b, id_b, call_b) = fixture_at(root.path(), &root.path().join("session-b"));
    assert_eq!(id_a, id_b);
    let key_a = key(&a, id_a, &call_a);
    assert_ne!(key_a.session, b.plugin_session().unwrap());
    a.freeze_plugin(id_a, key_a.clone(), &call_a, None).unwrap();
    assert!(b.freeze_plugin(id_b, key_a.clone(), &call_b, None).is_err());
    assert!(
        !b.record()
            .unwrap()
            .operations
            .last()
            .unwrap()
            .tool_receipt
            .as_ref()
            .unwrap()
            .admitted
    );
    drop(a);
    let store = Store::open(&store_a).unwrap();
    let record: Record = serde_json::from_value(store.read().unwrap()).unwrap();
    let resumed = SharedRuntime(Arc::new(std::sync::Mutex::new(super::super::Runtime {
        store,
        record,
        failed: false,
        learning_view: None,
        mutation_boundaries: Default::default(),
        service_slots: Arc::new(tokio::sync::Semaphore::new(8)),
    })));
    assert_eq!(resumed.plugin_session().unwrap(), key_a.session);
    resumed.admit_tool(id_a, &call_a).unwrap();
}

fn reserve(runtime: &SharedRuntime, id: u64, call: &ToolCall, index: u32) -> HookReceipt {
    use crate::plugins::hook_types::{HandlerKind, HookDialect};
    let declaration = DeclarationIdentity {
        package: "fixture".into(),
        code: "code".into(),
        policy: "policy".into(),
        configuration: "config".into(),
        generation: "1".into(),
        scope: Scope::Project,
        role: "worker".into(),
        declaration: format!("hook-{index}"),
        index,
        dialect: HookDialect::Native,
        runner: HandlerKind::Command,
    };
    runtime
        .update(|record| {
            record
                .operations
                .iter_mut()
                .find(|op| op.id == id)
                .unwrap()
                .tool_receipt
                .as_mut()
                .unwrap()
                .plugin_admission
                .as_mut()
                .unwrap()
                .declarations
                .push(json!({"identity":declaration}));
            Ok(())
        })
        .unwrap();
    let mut hook = HookReceipt {
        invocation: 0,
        declaration,
        class: HandlerClass::Combined,
        endpoint: None,
        inspected: key(runtime, id, call),
        outcome: None,
        uncertain_effects: true,
        hold: None,
        questions: vec![],
        pending_proposals: vec![],
    };
    hook.invocation = runtime.begin_plugin_hook(id, hook.clone(), call).unwrap();
    hook
}

fn completed_deny(mut hook: HookReceipt) -> HookReceipt {
    hook.outcome = Some(RawOutcome::Callback {
        value: json!({"decision":"deny"}),
    });
    hook.uncertain_effects = false;
    hook.hold = Some("decision denied".into());
    hook.questions = vec![Question {
        choice: PendingDecision::Deny,
        reason: Some("known denial".into()),
    }];
    hook
}

#[test]
fn reserved_results_can_settle_after_owner_hold_but_new_work_cannot_start() {
    for uncertain in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let (runtime, id, call) = fixture(root.path());
        let first = reserve(&runtime, id, &call, 0);
        let second = reserve(&runtime, id, &call, 1);
        let unknown = reserve(&runtime, id, &call, 2);
        let frozen = key(&runtime, id, &call);
        if uncertain {
            let mut failure = first;
            failure.outcome = Some(RawOutcome::Failure {
                reason: "transport lost".into(),
            });
            runtime.finish_plugin_hook(id, failure).unwrap();
        } else {
            runtime.hold_plugin(id, "owner is held").unwrap();
            runtime
                .finish_plugin_hook(id, completed_deny(first))
                .unwrap();
        }
        runtime
            .finish_plugin_hook(id, completed_deny(second))
            .unwrap();
        let before = serde_json::to_value(runtime.record().unwrap()).unwrap();
        assert!(runtime.plugin_owner(id).is_err());
        assert!(
            runtime
                .begin_plugin_hook(id, unknown.clone(), &call)
                .is_err()
        );
        assert!(runtime.freeze_plugin(id, frozen, &call, None).is_err());
        assert!(runtime.admit_tool(id, &call).is_err());
        assert!(runtime.tool_effect(id).is_err());
        assert_eq!(
            serde_json::to_value(runtime.record().unwrap()).unwrap(),
            before,
            "failed admissions changed the record"
        );
        let record = runtime.record().unwrap();
        let plan = record
            .operations
            .last()
            .unwrap()
            .tool_receipt
            .as_ref()
            .unwrap()
            .plugin_admission
            .as_ref()
            .unwrap();
        assert!(plan.hooks[1].outcome.is_some());
        assert_eq!(plan.hooks[1].questions[0].choice, PendingDecision::Deny);
        assert!(plan.hooks[2].outcome.is_none());
        assert!(plan.hooks[2].uncertain_effects);
        assert_eq!(record.recovery_pending, uncertain);
        if !uncertain {
            assert_eq!(plan.hold.as_deref(), Some("owner is held"));
        }
    }
}

#[test]
fn result_settlement_rejects_changed_reserved_identity_without_partial_mutation() {
    let root = tempfile::tempdir().unwrap();
    let (runtime, id, call) = fixture(root.path());
    let first = reserve(&runtime, id, &call, 0);
    let second = reserve(&runtime, id, &call, 1);
    runtime.hold_plugin(id, "owner is held").unwrap();
    for field in [
        "invocation",
        "key",
        "class",
        "endpoint",
        "declaration",
        "unknown",
    ] {
        let mut changed = completed_deny(first.clone());
        match field {
            "invocation" => changed.invocation = second.invocation,
            "key" => changed.inspected.arguments = "different-candidate".into(),
            "class" => changed.class = HandlerClass::DecisionGate,
            "endpoint" => changed.endpoint = Some("different-endpoint".into()),
            "declaration" => changed.declaration.generation = "different-generation".into(),
            _ => changed.outcome = None,
        }
        changed.uncertain_effects = true;
        let before = serde_json::to_value(runtime.record().unwrap()).unwrap();
        assert!(
            runtime.finish_plugin_hook(id, changed).is_err(),
            "accepted changed {field}"
        );
        assert_eq!(
            serde_json::to_value(runtime.record().unwrap()).unwrap(),
            before,
            "changed {field} partially mutated the record"
        );
    }
    let completed = completed_deny(first);
    runtime.finish_plugin_hook(id, completed.clone()).unwrap();
    let before = serde_json::to_value(runtime.record().unwrap()).unwrap();
    assert!(runtime.finish_plugin_hook(id, completed).is_err());
    assert_eq!(
        serde_json::to_value(runtime.record().unwrap()).unwrap(),
        before
    );
}

#[test]
fn service_bootstrap_cannot_authorize_tools_or_be_settled_as_a_model_call() {
    let root = tempfile::tempdir().unwrap();
    let (runtime, owner, call) = fixture(root.path());
    let startup = runtime
        .begin_plugin_service(owner, &"a".repeat(64))
        .unwrap();
    assert!(runtime.begin_tool("worker", startup, &call).is_err());
    runtime.settle_hook_models("worker").unwrap();
    let record = runtime.record().unwrap();
    let op = record.operations.iter().find(|o| o.id == startup).unwrap();
    assert!(!op.complete);
    assert!(matches!(
        op.host_invocation,
        Some(super::super::HostInvocation::PluginService {
            outcome: super::super::PluginServiceOutcome::Pending,
            ..
        })
    ));
    runtime.complete_plugin_service(startup).unwrap();
    assert!(runtime.begin_tool("worker", startup, &call).is_err());
    let serialized = serde_json::to_vec(&runtime.record().unwrap()).unwrap();
    let recovered: Record = serde_json::from_slice(&serialized).unwrap();
    assert!(matches!(
        recovered.operations.last().unwrap().host_invocation,
        Some(super::super::HostInvocation::PluginService {
            outcome: super::super::PluginServiceOutcome::Ready,
            ..
        })
    ));
}
