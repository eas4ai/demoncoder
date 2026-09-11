use super::*;
use crate::{
    config::Connection,
    plugins::{
        dispatch::{Declaration, HandlerClass, Matcher},
        gate_snapshot::GateReadSet,
        hook_types::{HandlerKind, HookDialect},
        once::HookReservation,
        receipts::{AdmissionKey, DeclarationIdentity, RawOutcome},
    },
    tools::ToolCall,
    workflow::runtime::{Identity, ToolAdmission},
};
use serde_json::json;
use std::sync::{Arc, Barrier};

#[path = "preparation_tests.rs"]
mod preparation_tests;

fn fixture(root: &std::path::Path, name: &str) -> SharedRuntime {
    let connection: Connection = serde_json::from_value(json!({"adapter":"openai-api"})).unwrap();
    let record: Record = serde_json::from_value(json!({"workspace":root,"identity":Identity::from(&connection),"archived":[],"next_task":1,"checkpoint_cursor":0,"operations":[],"messages":[],"recovery_pending":false,"decisions":[]})).unwrap();
    let runtime = SharedRuntime::for_test(&root.join(name), record).unwrap();
    runtime.begin_model("worker").unwrap();
    runtime
}
fn binding(runtime: &SharedRuntime, change: ActivationChange) -> OnceBinding {
    runtime
        .plugin_hook_activation(
            HookOrigin::Native,
            Scope::Project,
            &ActivationSource::host_namespace("pkg").unwrap(),
            "skills/check",
            "worker",
            change,
        )
        .unwrap()
        .unwrap()
}
fn declaration(binding: Option<OnceBinding>) -> Declaration {
    Declaration {
        required_gate: true,
        source: binding.as_ref().map(OnceBinding::source),
        once: binding,
        identity: DeclarationIdentity {
            package: "pkg".into(),
            code: "code".into(),
            policy: "policy".into(),
            configuration: "config".into(),
            generation: "generation".into(),
            scope: Scope::Project,
            role: "worker".into(),
            declaration: "check".into(),
            index: 0,
            dialect: HookDialect::Native,
            runner: HandlerKind::Command,
        },
        class: HandlerClass::Combined,
        priority: 0,
        matcher: Matcher::default(),
        reads: GateReadSet::default(),
        concurrent_group: None,
        read_only_endpoint: None,
        external_precondition: None,
    }
}
fn owner(runtime: &SharedRuntime, name: &str, d: &Declaration) -> (u64, ToolCall, HookReceipt) {
    let call = ToolCall {
        id: name.into(),
        name: "write".into(),
        arguments: json!({"path":name,"content":"value"}),
    };
    let id = match runtime.begin_tool("worker", 1, &call).unwrap() {
        ToolAdmission::Fresh(id) => id,
        _ => panic!("fresh owner expected"),
    };
    runtime
        .begin_plugin_plan(id, "plan".into(), vec![serde_json::to_value(d).unwrap()])
        .unwrap();
    let hook = HookReceipt {
        required_gate: true,
        observer: None,
        source: d.source.as_ref().map(|s| s.0.clone()),
        once: None,
        invocation: 0,
        declaration: d.identity.clone(),
        class: d.class,
        endpoint: None,
        inspected: AdmissionKey {
            session: runtime.plugin_session().unwrap(),
            operation: id,
            source_operation: 1,
            event: "PreToolUse".into(),
            tool: Some(call.name.clone()),
            lifecycle: None,
            arguments: Some(crate::plugins::admission::candidate_digest(&call).unwrap()),
            plan: "plan".into(),
            role: "worker".into(),
            workspace: (1, 2),
            inputs: vec![],
            external: None,
        },
        outcome: None,
        uncertain_effects: true,
        hold: None,
        questions: vec![],
        pending_proposals: vec![],
    };
    (id, call, hook)
}
fn reserve(
    runtime: &SharedRuntime,
    owner: &(u64, ToolCall, HookReceipt),
    binding: &OnceBinding,
) -> HookReceipt {
    match runtime
        .reserve_plugin_hook(owner.0, owner.2.clone(), &owner.1, Some(binding), None)
        .unwrap()
    {
        HookReservation::Run(hook) => *hook,
        HookReservation::Skipped => panic!("execution expected"),
    }
}
fn finish(runtime: &SharedRuntime, mut hook: HookReceipt, success: bool) {
    hook.uncertain_effects = false;
    hook.outcome = Some(RawOutcome::Callback {
        value: json!({"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":if success {"allow"} else {"deny"}}}),
    });
    runtime
        .finish_plugin_hook(hook.inspected.operation, hook)
        .unwrap();
}
fn reopen(runtime: SharedRuntime) -> SharedRuntime {
    use crate::workflow::store::Store;
    let path = runtime.directory().unwrap();
    drop(runtime);
    let store = Store::open(&path).unwrap();
    let mut record = serde_json::from_value(store.read().unwrap()).unwrap();
    super::super::plugin_observer::interrupt_restored(&mut record);
    SharedRuntime(Arc::new(std::sync::Mutex::new(
        crate::workflow::runtime::Runtime {
            store,
            record,
            failed: false,
            learning_view: None,
            mutation_boundaries: Default::default(),
            service_slots: Arc::new(tokio::sync::Semaphore::new(8)),
            once_live: Default::default(),
            observers: Default::default(),
        },
    )))
}
#[test]
fn simultaneous_matching_events_reserve_exactly_one_attempt_without_memory_leak() {
    let root = tempfile::tempdir().unwrap();
    let runtime = fixture(root.path(), "session");
    let b = binding(&runtime, ActivationChange::ExplicitInvocation);
    let d = declaration(Some(b.clone()));
    let a = owner(&runtime, "first", &d);
    let z = owner(&runtime, "second", &d);
    let barrier = Arc::new(Barrier::new(2));
    let outcomes = std::thread::scope(|scope| {
        let handles = [a, z]
            .into_iter()
            .map(|o| {
                let r = runtime.clone();
                let b = b.clone();
                let barrier = barrier.clone();
                scope.spawn(move || {
                    barrier.wait();
                    r.reserve_plugin_hook(o.0, o.2, &o.1, Some(&b), None)
                        .is_ok()
                })
            })
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(outcomes.iter().filter(|ok| **ok).count(), 1);
    runtime
        .update(|r| {
            r.decisions.push("unrelated successful update".into());
            Ok(())
        })
        .unwrap();
    assert_eq!(hooks(&runtime.record().unwrap()).count(), 1);
    let resumed = reopen(runtime);
    assert_eq!(hooks(&resumed.record().unwrap()).count(), 1);
}
#[test]
fn failure_never_retries_same_operation_and_success_skip_references_exact_prior_evidence() {
    for success in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let runtime = fixture(root.path(), "session");
        let b = binding(&runtime, ActivationChange::ExplicitInvocation);
        let d = declaration(Some(b.clone()));
        let first = owner(&runtime, "first", &d);
        let h = reserve(&runtime, &first, &b);
        finish(&runtime, h, success);
        assert!(
            runtime
                .reserve_plugin_hook(first.0, first.2.clone(), &first.1, Some(&b), None)
                .is_err()
        );
        let next = owner(&runtime, "next", &d);
        let result = runtime
            .reserve_plugin_hook(next.0, next.2.clone(), &next.1, Some(&b), None)
            .unwrap();
        assert_eq!(matches!(result, HookReservation::Skipped), success);
        if success {
            let record = runtime.record().unwrap();
            let skip = &record
                .operations
                .last()
                .unwrap()
                .tool_receipt
                .as_ref()
                .unwrap()
                .plugin_admission
                .as_ref()
                .unwrap()
                .once_skips[0];
            assert_eq!(skip.consumed.operation, first.0);
            assert_eq!(skip.consumed.invocation, 0);
            assert_ne!(skip.inspected.arguments, first.2.inspected.arguments);
            assert!(
                runtime
                    .reserve_plugin_hook(next.0, next.2, &next.1, Some(&b), None)
                    .is_err()
            );
        }
    }
}
#[test]
fn restart_unknown_and_generic_reconcile_cannot_bypass_via_generation_epoch_or_omission() {
    let root = tempfile::tempdir().unwrap();
    let runtime = fixture(root.path(), "session");
    let b = binding(&runtime, ActivationChange::ExplicitInvocation);
    let d = declaration(Some(b.clone()));
    let first = owner(&runtime, "first", &d);
    reserve(&runtime, &first, &b);
    let runtime = reopen(runtime);
    assert_eq!(
        serde_json::to_value(binding(&runtime, ActivationChange::Reuse)).unwrap(),
        serde_json::to_value(&b).unwrap()
    );
    runtime
        .reconcile("inspected generic recovery", None)
        .unwrap();
    assert!(
        runtime
            .plugin_hook_activation(
                HookOrigin::Native,
                Scope::Project,
                &ActivationSource::host_namespace("pkg").unwrap(),
                "skills/check",
                "worker",
                ActivationChange::ExplicitInvocation
            )
            .is_err()
    );
    for once in [Some(b.clone()), None] {
        let mut d = declaration(once);
        d.identity.generation = "new-generation".into();
        let next = owner(
            &runtime,
            if d.once.is_some() {
                "new-generation"
            } else {
                "omitted-once"
            },
            &d,
        );
        assert!(
            runtime
                .reserve_plugin_hook(next.0, next.2, &next.1, d.once.as_ref(), None)
                .is_err()
        );
    }
    runtime
        .update(|r| {
            r.decisions
                .push("publish after rejected activations".into());
            Ok(())
        })
        .unwrap();
    assert_eq!(hooks(&runtime.record().unwrap()).count(), 1);
    assert_eq!(runtime.record().unwrap().plugin_activations[0].epoch, 1);
}
#[test]
fn exact_failed_attestation_retains_unknown_evidence_allows_only_later_retry_and_survives_restart()
{
    let root = tempfile::tempdir().unwrap();
    let runtime = fixture(root.path(), "session");
    let b = binding(&runtime, ActivationChange::ExplicitInvocation);
    let d = declaration(Some(b.clone()));
    let first = owner(&runtime, "first", &d);
    let mut hook = reserve(&runtime, &first, &b);
    hook.outcome = Some(RawOutcome::Failure {
        reason: "transport disconnected after effect".into(),
    });
    runtime.finish_plugin_hook(first.0, hook).unwrap();
    let target = runtime.unresolved_plugin_once().unwrap().pop().unwrap();
    let foreign = fixture(root.path(), "foreign");
    assert!(
        foreign
            .reconcile_failed_plugin_once(&target, "developer", "inspected failure")
            .is_err()
    );
    let mut mismatch = target.clone();
    mismatch.declaration.generation = "invented".into();
    assert!(
        runtime
            .reconcile_failed_plugin_once(&mismatch, "developer", "inspected failure")
            .is_err()
    );
    runtime
        .reconcile_failed_plugin_once(&target, "developer", "runner stopped; effect unsuccessful")
        .unwrap();
    assert!(
        runtime
            .reconcile_failed_plugin_once(&target, "developer", "same request")
            .is_err()
    );
    let runtime = reopen(runtime);
    let record = runtime.record().unwrap();
    let previous = hooks(&record).next().unwrap();
    assert!(previous.uncertain_effects);
    assert!(matches!(previous.outcome, Some(RawOutcome::Failure { .. })));
    assert_eq!(previous.once.as_ref().unwrap().state, OnceState::Unknown);
    assert_eq!(
        previous
            .once
            .as_ref()
            .unwrap()
            .reconciliation
            .as_ref()
            .unwrap()
            .actor,
        "developer"
    );
    runtime
        .reconcile("developer inspected failed runner", None)
        .unwrap();
    assert!(
        runtime
            .reserve_plugin_hook(first.0, first.2, &first.1, Some(&b), None)
            .is_err()
    );
    let next = owner(&runtime, "next", &d);
    let hook = reserve(&runtime, &next, &b);
    finish(&runtime, hook, true);
    assert!(
        runtime
            .reconcile_failed_plugin_once(&target, "developer", "cannot change observed success")
            .is_err()
    );
    assert!(runtime.unresolved_plugin_once().unwrap().is_empty());
}
#[test]
fn stale_foreign_and_wrong_role_tokens_reject_before_record_publication() {
    let root = tempfile::tempdir().unwrap();
    let a = fixture(root.path(), "a");
    let b = fixture(root.path(), "b");
    let token = binding(&a, ActivationChange::ExplicitInvocation);
    let d = declaration(Some(token.clone()));
    let foreign = owner(&b, "foreign", &d);
    assert!(
        b.reserve_plugin_hook(foreign.0, foreign.2, &foreign.1, Some(&token), None)
            .is_err()
    );
    binding(&a, ActivationChange::ExplicitInvocation);
    let stale = owner(&a, "stale", &d);
    assert!(
        a.reserve_plugin_hook(stale.0, stale.2, &stale.1, Some(&token), None)
            .is_err()
    );
    a.update(|r| {
        r.decisions.push("after rejection".into());
        Ok(())
    })
    .unwrap();
    b.update(|r| {
        r.decisions.push("after rejection".into());
        Ok(())
    })
    .unwrap();
    assert_eq!(hooks(&a.record().unwrap()).count(), 0);
    assert_eq!(hooks(&b.record().unwrap()).count(), 0);
    let mut d = declaration(Some(binding(&a, ActivationChange::Reuse)));
    d.identity.role = "agent:foreign-child".into();
    assert!(d.once.as_ref().unwrap().validate(&d.identity).is_err());
}
#[test]
fn activation_bound_rejects_without_evicting_existing_retry_identity() {
    let root = tempfile::tempdir().unwrap();
    let runtime = fixture(root.path(), "session");
    for index in 0..MAX_ACTIVATIONS {
        runtime
            .plugin_hook_activation(
                HookOrigin::Native,
                Scope::Project,
                &ActivationSource::host_namespace("pkg").unwrap(),
                &format!("skill-{index}"),
                "worker",
                ActivationChange::ExplicitInvocation,
            )
            .unwrap();
    }
    assert!(
        runtime
            .plugin_hook_activation(
                HookOrigin::Native,
                Scope::Project,
                &ActivationSource::host_namespace("pkg").unwrap(),
                "overflow",
                "worker",
                ActivationChange::ExplicitInvocation
            )
            .is_err()
    );
    runtime
        .update(|r| {
            r.decisions.push("after rejected bound".into());
            Ok(())
        })
        .unwrap();
    assert_eq!(
        runtime.record().unwrap().plugin_activations.len(),
        MAX_ACTIVATIONS
    );
    assert!(
        runtime
            .plugin_hook_activation(
                HookOrigin::Native,
                Scope::Project,
                &ActivationSource::host_namespace("pkg").unwrap(),
                "skill-0",
                "worker",
                ActivationChange::Reuse
            )
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn reservation_lease_blocks_attestation_even_before_runner_dispatch() {
    let root = tempfile::tempdir().unwrap();
    let runtime = fixture(root.path(), "session");
    let b = binding(&runtime, ActivationChange::ExplicitInvocation);
    let d = declaration(Some(b.clone()));
    let first = owner(&runtime, "first", &d);
    let lease = Arc::new(
        Arc::new(tokio::sync::Semaphore::new(1))
            .acquire_owned()
            .await
            .unwrap(),
    );
    assert!(matches!(
        runtime
            .reserve_plugin_hook(first.0, first.2, &first.1, Some(&b), Some(&lease))
            .unwrap(),
        HookReservation::Run(_)
    ));
    let target = runtime.unresolved_plugin_once().unwrap().pop().unwrap();
    assert!(
        runtime
            .reconcile_failed_plugin_once(&target, "developer", "runner not dispatched yet")
            .is_err()
    );
    assert!(
        hooks(&runtime.record().unwrap())
            .next()
            .unwrap()
            .once
            .as_ref()
            .unwrap()
            .reconciliation
            .is_none()
    );
    drop(lease);
    runtime
        .reconcile_failed_plugin_once(
            &target,
            "developer",
            "reservation abandoned before dispatch",
        )
        .unwrap();
}

#[test]
fn source_directory_replacement_and_once_omission_cannot_erase_unknown_attempts() {
    let root = tempfile::tempdir().unwrap();
    let runtime = fixture(root.path(), "session");
    let source = root.path().join("package");
    let capture = |source: &std::path::Path| {
        std::fs::create_dir_all(source.join(".claude-plugin")).unwrap();
        std::fs::write(
            source.join(".claude-plugin/plugin.json"),
            r#"{"name":"pkg","version":"1"}"#,
        )
        .unwrap();
        crate::plugins::inspect(source, &crate::plugins::ImportOptions::default()).unwrap()
    };
    let original = capture(&source);
    let address = ActivationSource::from_package(&original).unwrap();
    let b = runtime
        .plugin_hook_activation(
            HookOrigin::Native,
            Scope::Project,
            &address,
            "skills/check",
            "worker",
            ActivationChange::ExplicitInvocation,
        )
        .unwrap()
        .unwrap();
    let d = declaration(Some(b.clone()));
    let first = owner(&runtime, "first", &d);
    reserve(&runtime, &first, &b);
    std::fs::rename(&source, root.path().join("old-package")).unwrap();
    capture(&source);
    std::fs::write(source.join("changed-code"), "new generation").unwrap();
    let replacement =
        crate::plugins::inspect(&source, &crate::plugins::ImportOptions::default()).unwrap();
    assert_ne!(original.source().inode, replacement.source().inode);
    let new_address = ActivationSource::from_package(&replacement).unwrap();
    assert_eq!(
        serde_json::to_value(&address).unwrap(),
        serde_json::to_value(&new_address).unwrap()
    );
    assert!(
        runtime
            .plugin_hook_activation(
                HookOrigin::Native,
                Scope::Project,
                &new_address,
                "skills/check",
                "worker",
                ActivationChange::ExplicitInvocation
            )
            .is_err()
    );
    let mut omitted = declaration(None);
    omitted.source = Some(new_address);
    omitted.identity.code = replacement.digest().into();
    let next = owner(&runtime, "updated-without-once", &omitted);
    assert!(
        runtime
            .reserve_plugin_hook(next.0, next.2, &next.1, None, None)
            .is_err()
    );
    let unrelated = capture(&root.path().join("unrelated-package"));
    let mut other = declaration(None);
    other.source = Some(ActivationSource::from_package(&unrelated).unwrap());
    let next = owner(&runtime, "unrelated-same-name", &other);
    assert!(matches!(
        runtime
            .reserve_plugin_hook(next.0, next.2, &next.1, None, None)
            .unwrap(),
        HookReservation::Run(_)
    ));
}
