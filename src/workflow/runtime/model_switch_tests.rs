use super::*;
use crate::{
    config::Connection,
    session::{SessionEnd, SessionStart},
    workflow::allocation::Limits,
};
use serde_json::json;

fn model_occurrence(switch: u64, post: bool) -> crate::plugins::receipts::NonToolOccurrence {
    if post {
        crate::plugins::receipts::NonToolOccurrence::PostModelSwitch {
            model_switch: switch,
            model: Some("new".into()),
            source: "settings".into(),
        }
    } else {
        crate::plugins::receipts::NonToolOccurrence::PreModelSwitch {
            model_switch: switch,
            requested_model: Some("new".into()),
            resolved_model: Some("new".into()),
            source: "settings".into(),
        }
    }
}

fn applied_model_switch(payload: &serde_json::Value) -> bool {
    serde_json::from_value::<Record>(payload.clone()).is_ok_and(|record| {
        record.operations.iter().any(|operation| {
            matches!(
                &operation.host_invocation,
                Some(HostInvocation::ModelSwitch(switch)) if switch.stage == model_switch::Stage::Applied
            )
        })
    })
}

#[test]
fn ended_or_revoked_original_lifetime_cannot_release_model_switch() {
    for revoke in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let old: Connection =
            serde_json::from_value(json!({"adapter":"openai-api","model":"old"})).unwrap();
        let new: Connection =
            serde_json::from_value(json!({"adapter":"openai-api","model":"new"})).unwrap();
        let mut record = crate::inspection::tests::record(root.path());
        record.identity = Identity::from(&old);
        record.phase = None;
        record.session_hook_allowance = Some(
            session_budget::SessionHookAllowance::new(Limits {
                seconds: 60,
                model_calls: 2,
                tool_calls: 2,
            })
            .unwrap(),
        );
        let runtime = SharedRuntime::for_test(&state.path().join("record"), record).unwrap();
        let lifetime = runtime
            .begin_native_session(SessionStart::Startup, None, vec![])
            .unwrap();
        let switch = runtime
            .begin_model_switch(&old, &new, "settings", Some(lifetime), None, None)
            .unwrap();

        if revoke {
            runtime.finalize_native_session(lifetime).unwrap();
        } else {
            runtime
                .end_native_session(lifetime, SessionEnd::Shutdown)
                .unwrap();
        }
        assert!(runtime.begin_model_switch_teardown(switch).is_err());
        assert_eq!(runtime.record().unwrap().identity, Identity::from(&old));
    }
}

#[test]
fn model_switch_uses_original_session_allowance_and_applies_once() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let old: Connection =
        serde_json::from_value(json!({"adapter":"openai-api","model":"old"})).unwrap();
    let new: Connection =
        serde_json::from_value(json!({"adapter":"openai-api","model":"new"})).unwrap();
    let mut record = crate::inspection::tests::record(root.path());
    record.identity = Identity::from(&old);
    record.phase = None;
    record.session_hook_allowance = Some(
        session_budget::SessionHookAllowance::new(Limits {
            seconds: 60,
            model_calls: 2,
            tool_calls: 2,
        })
        .unwrap(),
    );
    let runtime = SharedRuntime::for_test(&state.path().join("record"), record).unwrap();
    let lifetime = runtime
        .begin_native_session(SessionStart::Startup, None, vec![])
        .unwrap();

    let switch = runtime
        .begin_model_switch(
            &old,
            &new,
            "settings",
            Some(lifetime),
            Some("pre-plan".into()),
            Some("post-plan".into()),
        )
        .unwrap();
    let prepared = runtime.record().unwrap();
    let operation = prepared.operations.iter().find(|o| o.id == switch).unwrap();
    assert_eq!(
        operation.budget,
        Some(BudgetRef::SessionHooks {
            session: runtime.plugin_session().unwrap(),
        })
    );
    assert_eq!(prepared.identity, Identity::from(&old));

    runtime.begin_model_switch_teardown(switch).unwrap();
    runtime
        .apply_model_switch(switch, &new, None, new.model.clone())
        .unwrap();
    assert!(
        runtime
            .apply_model_switch(switch, &new, None, new.model.clone())
            .is_err()
    );
    runtime.end_model_switch(switch, None).unwrap();
    assert_eq!(runtime.record().unwrap().identity, Identity::from(&new));
}

#[test]
fn model_switch_pre_and_post_share_one_exact_owner() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let old: Connection =
        serde_json::from_value(json!({"adapter":"openai-api","model":"old"})).unwrap();
    let new: Connection =
        serde_json::from_value(json!({"adapter":"openai-api","model":"new"})).unwrap();
    let mut record = crate::inspection::tests::record(root.path());
    record.identity = Identity::from(&old);
    record.phase = None;
    record.session_hook_allowance = Some(
        session_budget::SessionHookAllowance::new(Limits {
            seconds: 60,
            model_calls: 2,
            tool_calls: 2,
        })
        .unwrap(),
    );
    let runtime = SharedRuntime::for_test(&state.path().join("record"), record).unwrap();
    let lifetime = runtime
        .begin_native_session(SessionStart::Startup, None, vec![])
        .unwrap();
    let switch = runtime
        .begin_model_switch(
            &old,
            &new,
            "settings",
            Some(lifetime),
            Some("pre-plan".into()),
            Some("post-plan".into()),
        )
        .unwrap();

    let pre = runtime
        .begin_non_tool_as(
            "model-switch",
            Some(&Identity::from(&old)),
            None,
            model_occurrence(switch, false),
            "pre-plan".into(),
            vec![],
        )
        .unwrap();
    runtime
        .plugin_funded_remaining(
            pre.operation,
            crate::plugins::hook_types::HookEvent::PreModelSwitch,
        )
        .unwrap();
    runtime
        .settle_non_tool(
            pre.operation,
            crate::plugins::hook_types::HookEvent::PreModelSwitch,
            crate::plugins::non_tool::NonToolEffects::default(),
        )
        .unwrap();
    assert!(
        runtime
            .begin_non_tool_as(
                "model-switch",
                Some(&Identity::from(&old)),
                None,
                model_occurrence(switch, false),
                "pre-plan".into(),
                vec![],
            )
            .is_err()
    );

    runtime.begin_model_switch_teardown(switch).unwrap();
    runtime
        .apply_model_switch(switch, &new, None, new.model.clone())
        .unwrap();
    let post = runtime
        .begin_non_tool_as(
            "model-switch",
            Some(&Identity::from(&new)),
            None,
            model_occurrence(switch, true),
            "post-plan".into(),
            vec![],
        )
        .unwrap();
    runtime
        .plugin_funded_remaining(
            post.operation,
            crate::plugins::hook_types::HookEvent::PostModelSwitch,
        )
        .unwrap();
    runtime
        .settle_non_tool(
            post.operation,
            crate::plugins::hook_types::HookEvent::PostModelSwitch,
            crate::plugins::non_tool::NonToolEffects::default(),
        )
        .unwrap();
    runtime.end_model_switch(switch, None).unwrap();
}

#[test]
fn forged_empty_source_observation_digest_cannot_enter_model_switch_owner() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let old: Connection =
        serde_json::from_value(json!({"adapter":"claude","model":"old"})).unwrap();
    let new: Connection =
        serde_json::from_value(json!({"adapter":"claude","model":"new"})).unwrap();
    let mut record = crate::inspection::tests::record(root.path());
    record.identity = Identity::from(&old);
    record.phase = None;
    let runtime = SharedRuntime::for_test(&state.path().join("record"), record).unwrap();
    let authentic = crate::plugins::non_tool::NonToolPlan::authenticated_source_observation_digest(
        crate::plugins::hook_types::HookEvent::PreModelSwitch,
    )
    .unwrap();
    let switch = runtime
        .begin_model_switch(&old, &new, "settings", None, Some(authentic), None)
        .unwrap();

    let error = runtime
        .begin_non_tool_as(
            "model-switch",
            Some(&Identity::from(&old)),
            None,
            model_occurrence(switch, false),
            "forged-empty-observation".into(),
            vec![],
        )
        .unwrap_err();
    assert!(
        format!("{error:#}").contains("differs from its admitted candidate"),
        "unexpected forged observation error: {error:#}"
    );
}

#[test]
fn repeated_model_switches_keep_the_one_original_session_allowance() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let connection = |model: &str| {
        serde_json::from_value::<Connection>(json!({"adapter":"openai-api","model":model})).unwrap()
    };
    let a = connection("a");
    let b = connection("b");
    let c = connection("c");
    let mut record = crate::inspection::tests::record(root.path());
    record.identity = Identity::from(&a);
    record.phase = None;
    record.session_hook_allowance = Some(
        session_budget::SessionHookAllowance::new(Limits {
            seconds: 60,
            model_calls: 2,
            tool_calls: 2,
        })
        .unwrap(),
    );
    let runtime = SharedRuntime::for_test(&state.path().join("record"), record).unwrap();
    let lifetime = runtime
        .begin_native_session(SessionStart::Startup, None, vec![])
        .unwrap();
    let original = runtime
        .record()
        .unwrap()
        .session_hook_allowance
        .unwrap()
        .allocation;

    for (old, new) in [(&a, &b), (&b, &c)] {
        let switch = runtime
            .begin_model_switch(old, new, "settings", Some(lifetime), None, None)
            .unwrap();
        runtime.begin_model_switch_teardown(switch).unwrap();
        runtime
            .apply_model_switch(switch, new, None, new.model.clone())
            .unwrap();
        runtime.end_model_switch(switch, None).unwrap();
    }

    let record = runtime.record().unwrap();
    assert_eq!(record.identity, Identity::from(&c));
    let retained = &record.session_hook_allowance.unwrap().allocation;
    assert_eq!(retained.started_ms, original.started_ms);
    assert_eq!(retained.deadline_ms, original.deadline_ms);
    assert_eq!(retained.model_calls, original.model_calls);
    assert_eq!(retained.tool_calls, original.tool_calls);
}

#[test]
fn unrelated_context_history_cannot_bridge_original_model_switch_lifetime() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let connection = |model: &str| {
        serde_json::from_value::<Connection>(json!({"adapter":"openai-api","model":model})).unwrap()
    };
    let original = connection("original");
    let applied = connection("applied");
    let forged = connection("forged-alias");
    let next = connection("next");
    let mut record = crate::inspection::tests::record(root.path());
    record.identity = Identity::from(&original);
    record.phase = None;
    record.session_hook_allowance = Some(
        session_budget::SessionHookAllowance::new(Limits {
            seconds: 60,
            model_calls: 2,
            tool_calls: 2,
        })
        .unwrap(),
    );
    let runtime = SharedRuntime::for_test(&state.path().join("record"), record).unwrap();
    let lifetime = runtime
        .begin_native_session(SessionStart::Startup, None, vec![])
        .unwrap();
    let switch = runtime
        .begin_model_switch(&original, &applied, "settings", Some(lifetime), None, None)
        .unwrap();
    runtime.begin_model_switch_teardown(switch).unwrap();
    runtime
        .apply_model_switch(switch, &applied, None, applied.model.clone())
        .unwrap();
    runtime.end_model_switch(switch, None).unwrap();

    runtime
        .update(|record| {
            record.prior_contexts.push(ContextBinding {
                identity: Identity::from(&applied),
                through_operation: record.operations.len() as u64,
                checkpoint: None,
            });
            record.identity = Identity::from(&forged);
            Ok(())
        })
        .unwrap();

    assert!(
        runtime
            .begin_model_switch(&forged, &next, "settings", Some(lifetime), None, None)
            .is_err(),
        "unrelated prior context must not extend original host authority"
    );
}

#[test]
fn model_switch_application_persistence_failure_latches_execution_and_retains_teardown_on_disk() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let path = state.path().join("record");
    let old: Connection =
        serde_json::from_value(json!({"adapter":"openai-api","model":"old"})).unwrap();
    let new: Connection =
        serde_json::from_value(json!({"adapter":"openai-api","model":"new"})).unwrap();
    let mut record = crate::inspection::tests::record(root.path());
    record.identity = Identity::from(&old);
    record.phase = None;
    let runtime = SharedRuntime::for_test(&path, record).unwrap();
    let switch = runtime
        .begin_model_switch(&old, &new, "settings", None, None, None)
        .unwrap();
    runtime.begin_model_switch_teardown(switch).unwrap();
    runtime.fail_config_change_receipt_before_rename_when(applied_model_switch);

    let error = runtime
        .apply_model_switch(switch, &new, None, new.model.clone())
        .unwrap_err();
    assert!(format!("{error:#}").contains("execution is held"));
    assert!(runtime.end_model_switch(switch, None).is_err());

    let persisted: Record =
        serde_json::from_value(crate::workflow::store::Store::read_snapshot(&path).unwrap())
            .unwrap();
    assert_eq!(persisted.identity, Identity::from(&old));
    assert!(persisted.operations.iter().any(|operation| {
        matches!(
            &operation.host_invocation,
            Some(HostInvocation::ModelSwitch(owner))
                if owner.stage == model_switch::Stage::Teardown && !operation.complete
        )
    }));
}

#[test]
fn reopening_teardown_marks_recovery_and_never_replays_model_switch() {
    let root = tempfile::tempdir().unwrap();
    let old: Connection = serde_json::from_value(json!({
        "adapter":"openai-api",
        "model":"old",
        "api_key":"restart-fixture-key"
    }))
    .unwrap();
    let new: Connection = serde_json::from_value(json!({
        "adapter":"openai-api",
        "model":"new",
        "api_key":"restart-fixture-key"
    }))
    .unwrap();
    let limits = Limits {
        seconds: 60,
        model_calls: 2,
        tool_calls: 2,
    };
    let (runtime, resumed) = SharedRuntime::open_with_session_hooks(
        root.path(),
        &old,
        None,
        &crate::workflow::workspace::CaptureScope::default(),
        Some(&limits),
    )
    .unwrap();
    assert!(!resumed);
    let lifetime = runtime
        .begin_native_session(SessionStart::Startup, None, vec![])
        .unwrap();
    let switch = runtime
        .begin_model_switch(&old, &new, "settings", Some(lifetime), None, None)
        .unwrap();
    runtime.begin_model_switch_teardown(switch).unwrap();
    let directory = runtime.directory().unwrap();
    let before = runtime.record().unwrap();
    drop(runtime);

    let (runtime, resumed) = SharedRuntime::open_with_session_hooks(
        root.path(),
        &old,
        Some(&directory),
        &crate::workflow::workspace::CaptureScope::default(),
        Some(&limits),
    )
    .unwrap();
    assert!(resumed);
    let after = runtime.record().unwrap();
    assert!(after.recovery_pending);
    assert_eq!(after.identity, Identity::from(&old));
    assert_eq!(after.operations.len(), before.operations.len());
    assert_eq!(
        after
            .operations
            .iter()
            .filter_map(Operation::non_tool_receipt)
            .count(),
        0
    );
    assert!(after.operations.iter().any(|operation| {
        operation.id == switch
            && matches!(&operation.host_invocation, Some(HostInvocation::ModelSwitch(owner))
                if owner.stage == model_switch::Stage::Teardown && !operation.complete)
    }));
    assert!(
        runtime
            .begin_model_switch(&old, &new, "settings", Some(lifetime), None, None)
            .is_err()
    );
    drop(runtime);
    std::fs::remove_dir_all(directory).unwrap();
}
