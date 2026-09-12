use super::*;
use crate::events::EventSink;

fn fixture() -> (
    tempfile::TempDir,
    SharedRuntime,
    EventSink,
    tokio::sync::mpsc::Receiver<crate::events::Envelope>,
) {
    let root = tempfile::tempdir().unwrap();
    let runtime = SharedRuntime::for_test(
        &root.path().join("record"),
        crate::inspection::tests::record(root.path()),
    )
    .unwrap();
    let (sender, _receiver) = tokio::sync::mpsc::channel(32);
    let sink = EventSink::new("fixture".into(), sender, None)
        .unwrap()
        .with_runtime(runtime.clone());
    (root, runtime, sink, _receiver)
}
fn usage(input: Option<u64>) -> Event {
    Event::Usage {
        input,
        output: Some(2),
        cached: Some(0),
        cost_usd: Some(0.0),
    }
}
#[test]
fn unallocated_invocation_cannot_charge_a_later_allocation() {
    let (_root, runtime, sink, _receiver) = fixture();
    let id = runtime.begin_model("worker").unwrap();
    runtime.allocate(Limits::default(), None).unwrap();
    sink.for_invocation(Some(id))
        .emit_advisory(usage(Some(7)))
        .unwrap();
    sink.for_invocation(Some(id))
        .emit_advisory(usage(None))
        .unwrap();
    runtime.finish_model(id).unwrap();
    let record = runtime.record().unwrap();
    let a = record.allocation.unwrap();
    assert_eq!(a.usage.reported_input, 0);
    assert!(!a.usage.unknown_input);
}
#[test]
fn usage_sink_selects_exact_model_among_same_phase_operations() {
    let (_root, runtime, sink, _receiver) = fixture();
    runtime.allocate(Limits::default(), None).unwrap();
    let first = runtime.begin_model("worker").unwrap();
    let second = runtime.begin_model("worker").unwrap();
    runtime.begin_commands("worker", None).unwrap();
    sink.for_invocation(Some(first))
        .emit_advisory(usage(Some(7)))
        .unwrap();
    let record = runtime.record().unwrap();
    assert!(
        record
            .operations
            .iter()
            .find(|o| o.id == first)
            .unwrap()
            .usage_reported
    );
    assert!(
        !record
            .operations
            .iter()
            .find(|o| o.id == second)
            .unwrap()
            .usage_reported
    );
}
#[test]
fn equal_limit_replacement_retains_original_usage_target() {
    let (_root, runtime, sink, _receiver) = fixture();
    runtime.allocate(Limits::default(), None).unwrap();
    let first = runtime.begin_model("worker").unwrap();
    runtime.allocate(Limits::default(), None).unwrap();
    sink.for_invocation(Some(first))
        .emit_advisory(usage(Some(7)))
        .unwrap();
    let value = serde_json::to_value(runtime.record().unwrap()).unwrap();
    assert_eq!(value["allocation"]["usage"]["reported_input"], 0);
    assert_eq!(value["task_allocation_epoch"], 2);
    assert_eq!(
        value["retired_task_allocations"][0]["allocation"]["usage"]["reported_input"],
        7
    );
}
#[test]
fn usage_overflow_leaves_every_field_unchanged() {
    let mut usage = super::super::allocation::Usage {
        reported_output: u64::MAX,
        ..Default::default()
    };
    let before = serde_json::to_value(&usage).unwrap();
    assert!(usage.add(Some(7), Some(1), None, None).is_err());
    assert_eq!(serde_json::to_value(usage).unwrap(), before);
}

#[test]
fn native_turn_created_unfunded_cannot_gain_model_funding() {
    let (_root, runtime, sink, _receiver) = fixture();
    let turn = sink.begin_native_turn().unwrap();
    runtime.allocate(Limits::default(), None).unwrap();
    let id = turn.begin_model().unwrap().unwrap();
    turn.for_invocation(Some(id))
        .emit_advisory(usage(Some(9)))
        .unwrap();
    runtime.finish_model(id).unwrap();
    let record = runtime.record().unwrap();
    assert!(matches!(
        record.operations.last().unwrap().budget,
        Some(BudgetRef::Unallocated)
    ));
    assert_eq!(record.allocation.unwrap().model_calls, 0);
}

#[test]
fn retired_native_turn_cannot_admit_another_model() {
    let (_root, runtime, sink, _receiver) = fixture();
    runtime.allocate(Limits::default(), None).unwrap();
    let turn = sink.begin_native_turn().unwrap();
    runtime.allocate(Limits::default(), None).unwrap();
    assert!(turn.begin_model().is_err());
    assert_eq!(runtime.record().unwrap().allocation.unwrap().model_calls, 0);
}

#[test]
fn unmatched_reports_are_visible_without_charging_or_marking_another_operation() {
    let (_root, runtime, sink, _receiver) = fixture();
    runtime.allocate(Limits::default(), None).unwrap();
    let id = runtime.begin_model("worker").unwrap();
    let commands = runtime.begin_commands("worker", None).unwrap();
    for events in [
        sink.for_invocation(None),
        sink.for_invocation(Some(99)),
        sink.for_invocation(Some(commands)),
        sink.for_phase("other").for_invocation(Some(id)),
    ] {
        events.emit_advisory(usage(Some(5))).unwrap();
    }
    let record = runtime.record().unwrap();
    assert_eq!(record.allocation.as_ref().unwrap().usage.reported_input, 0);
    assert!(!record.operations[0].usage_reported);
    let receipt = record.unattributed_usage.as_ref().unwrap();
    assert_eq!(receipt.reports, 4);
    assert_eq!(receipt.usage.reported_input, 20);
    let view = crate::inspection::project(
        &record,
        Some(crate::inspection::Request {
            target: crate::inspection::Target::Overview,
            page: 0,
            generation: 0,
        }),
    );
    assert!(
        view.page
            .unwrap()
            .text
            .contains("Unresolved usage attribution")
    );
}

#[test]
fn complete_retired_model_accepts_late_partial_reports_and_unknown_stays_sticky() {
    let (_root, runtime, sink, _receiver) = fixture();
    runtime.allocate(Limits::default(), None).unwrap();
    let id = runtime.begin_model("worker").unwrap();
    runtime.finish_model(id).unwrap();
    runtime.allocate(Limits::default(), None).unwrap();
    sink.for_invocation(Some(id))
        .emit_advisory(usage(Some(7)))
        .unwrap();
    sink.for_invocation(Some(id))
        .emit_advisory(usage(None))
        .unwrap();
    runtime.finish_model(id).unwrap();
    let record = runtime.record().unwrap();
    assert!(!record.allocation.unwrap().usage.unknown_input);
    let retired = &record.retired_task_allocations[0].allocation;
    assert_eq!(retired.usage.reported_input, 7);
    assert!(retired.usage.unknown_input && retired.usage.unknown_cost);
    assert_eq!(
        record.operations[0].usage_receipt.as_ref().unwrap().reports,
        2
    );
}

#[test]
fn retention_capacity_and_epoch_overflow_refuse_without_mutation() {
    let (_root, runtime, _sink, _receiver) = fixture();
    for _ in 0..33 {
        runtime.allocate(Limits::default(), None).unwrap();
        let id = runtime.begin_model("worker").unwrap();
        runtime.finish_model(id).unwrap();
    }
    let before = serde_json::to_value(runtime.record().unwrap()).unwrap();
    assert!(runtime.allocate(Limits::default(), None).is_err());
    assert_eq!(
        serde_json::to_value(runtime.record().unwrap()).unwrap(),
        before
    );
    assert!(runtime.archive().is_err());
    assert_eq!(
        serde_json::to_value(runtime.record().unwrap()).unwrap(),
        before
    );
    let (_root2, other, _sink2, _receiver2) = fixture();
    other
        .update(|r| {
            r.task_allocation_epoch = u64::MAX;
            Ok(())
        })
        .unwrap();
    let before = serde_json::to_value(other.record().unwrap()).unwrap();
    assert!(other.allocate(Limits::default(), None).is_err());
    assert_eq!(
        serde_json::to_value(other.record().unwrap()).unwrap(),
        before
    );
}

#[test]
fn legacy_usage_is_retained_as_unresolved_without_a_session_fallback() {
    let (_root, runtime, sink, _receiver) = fixture();
    runtime
        .update(|r| {
            r.allocation = Some(Allocation::new(Limits::default())?);
            Ok(())
        })
        .unwrap();
    let id = runtime.begin_model("worker").unwrap();
    assert!(matches!(
        runtime.operation_budget(id).unwrap(),
        BudgetRef::Task { epoch: 0, .. }
    ));
    runtime
        .update(|r| {
            r.operations[0].budget = None;
            r.session_hook_allowance =
                Some(session_budget::SessionHookAllowance::new(Limits::default())?);
            Ok(())
        })
        .unwrap();
    sink.for_invocation(Some(id))
        .emit_advisory(usage(Some(8)))
        .unwrap();
    runtime.finish_model(id).unwrap();
    let record = runtime.record().unwrap();
    assert_eq!(record.allocation.unwrap().usage.reported_input, 0);
    assert_eq!(
        record
            .session_hook_allowance
            .unwrap()
            .allocation
            .usage
            .reported_input,
        0
    );
    let receipt = record.operations[0].usage_receipt.as_ref().unwrap();
    assert_eq!(receipt.usage.reported_input, 8);
    assert_eq!(
        receipt.unresolved,
        Some(budget_accounting::UnresolvedAttribution::LegacyBudget)
    );
}

#[test]
fn usage_receipt_and_allocation_are_atomic_when_target_overflows() {
    let (_root, runtime, sink, _receiver) = fixture();
    runtime.allocate(Limits::default(), None).unwrap();
    let id = runtime.begin_model("worker").unwrap();
    runtime
        .update(|r| {
            r.allocation.as_mut().unwrap().usage.reported_output = u64::MAX;
            Ok(())
        })
        .unwrap();
    let before = serde_json::to_value(runtime.record().unwrap()).unwrap();
    assert!(
        sink.for_invocation(Some(id))
            .emit_advisory(usage(Some(7)))
            .is_err()
    );
    assert_eq!(
        serde_json::to_value(runtime.record().unwrap()).unwrap(),
        before
    );
}

fn delegation_identity() -> crate::subagents::state::DelegationIdentity {
    crate::subagents::state::DelegationIdentity {
        connections: Default::default(),
        reviewer: None,
        max_active: 2,
        backend_limit: 10,
        orchestration: None,
        default_roles: vec![],
    }
}

fn save_test_task(runtime: &SharedRuntime, root: &std::path::Path) {
    let snapshot = crate::workflow::workspace::capture(root).unwrap();
    let task = Task::new(1, "retain original snapshot".into(), vec![], snapshot, 1).unwrap();
    runtime.save_task(&Some(task), 2, None).unwrap();
}

#[test]
fn archive_snapshot_stays_frozen_while_late_usage_settles_after_reload() {
    let (root, runtime, sink, _receiver) = fixture();
    runtime.allocate(Limits::default(), None).unwrap();
    save_test_task(&runtime, root.path());
    let id = runtime.begin_model("worker").unwrap();
    sink.for_invocation(Some(id))
        .emit_advisory(usage(Some(3)))
        .unwrap();
    runtime.finish_model(id).unwrap();
    runtime.archive().unwrap();
    let snapshot = serde_json::to_value(&runtime.record().unwrap().archived).unwrap();
    runtime.allocate(Limits::default(), None).unwrap();
    let directory = runtime.directory().unwrap();
    drop(sink);
    drop(runtime);
    let store = Store::open(&directory).unwrap();
    let record: Record = serde_json::from_value(store.read().unwrap()).unwrap();
    let restored = SharedRuntime(Arc::new(Mutex::new(Runtime {
        store,
        record,
        failed: false,
        learning_view: None,
        mutation_boundaries: Default::default(),
        service_slots: Arc::new(tokio::sync::Semaphore::new(8)),
        once_live: Default::default(),
        observers: Default::default(),
    })));
    let (sender, _receiver) = tokio::sync::mpsc::channel(16);
    let sink = EventSink::new("fixture".into(), sender, None)
        .unwrap()
        .with_runtime(restored.clone());
    sink.for_invocation(Some(id))
        .emit_advisory(usage(Some(5)))
        .unwrap();
    let record = restored.record().unwrap();
    assert_eq!(serde_json::to_value(&record.archived).unwrap(), snapshot);
    assert_eq!(record.archived[0].allocation_epoch, Some(1));
    assert_eq!(
        record.retired_task_allocations[0]
            .allocation
            .usage
            .reported_input,
        8
    );
    assert_eq!(record.allocation.unwrap().usage.reported_input, 0);
}

#[test]
fn delegation_reuses_epoch_and_retires_latest_counters_after_archive() {
    let (root, runtime, sink, _receiver) = fixture();
    let identity = delegation_identity();
    runtime
        .configure_delegation(identity.clone(), Limits::default())
        .unwrap();
    let id = runtime.begin_model("worker").unwrap();
    sink.for_invocation(Some(id))
        .emit_advisory(usage(Some(2)))
        .unwrap();
    save_test_task(&runtime, root.path());
    runtime.archive().unwrap();
    runtime
        .configure_delegation(identity, Limits::default())
        .unwrap();
    sink.for_invocation(Some(id))
        .emit_advisory(usage(Some(4)))
        .unwrap();
    let record = runtime.record().unwrap();
    assert_eq!(record.task_allocation_epoch, 1);
    assert!(record.retired_task_allocations.is_empty());
    assert_eq!(
        record.archived[0]
            .allocation
            .as_ref()
            .unwrap()
            .usage
            .reported_input,
        2
    );
    assert_eq!(record.allocation.unwrap().usage.reported_input, 6);
    runtime.allocate(Limits::default(), None).unwrap();
    assert_eq!(
        runtime.record().unwrap().retired_task_allocations[0]
            .allocation
            .usage
            .reported_input,
        6
    );
}

#[test]
fn every_completion_path_marks_only_original_missing_model_budgets() {
    for completion in ["model", "hook", "phase", "reconcile"] {
        let (_root, runtime, sink, _receiver) = fixture();
        runtime.allocate(Limits::default(), None).unwrap();
        let id = runtime.begin_model("worker").unwrap();
        runtime.begin_commands("worker", None).unwrap();
        runtime.allocate(Limits::default(), None).unwrap();
        match completion {
            "model" => runtime.finish_model(id).unwrap(),
            "hook" => runtime.settle_hook_models("worker").unwrap(),
            "phase" => runtime.finish_phase().unwrap(),
            _ => runtime
                .reconcile("Inspected remote work; no retry", None)
                .unwrap(),
        }
        let record = runtime.record().unwrap();
        assert!(
            record.retired_task_allocations[0]
                .allocation
                .usage
                .unknown_input,
            "{completion}"
        );
        assert!(
            !record.allocation.unwrap().usage.unknown_input,
            "{completion}"
        );
        assert!(!record.operations[0].usage_reported);
        sink.for_invocation(Some(id))
            .emit_advisory(usage(Some(6)))
            .unwrap();
        let record = runtime.record().unwrap();
        assert_eq!(
            record.retired_task_allocations[0]
                .allocation
                .usage
                .reported_input,
            6
        );
        assert!(
            record.retired_task_allocations[0]
                .allocation
                .usage
                .unknown_input
        );
    }
}

#[test]
fn session_and_wrong_session_references_never_fall_back_to_task() {
    for wrong in [false, true] {
        let (_root, runtime, sink, _receiver) = fixture();
        runtime.allocate(Limits::default(), None).unwrap();
        let id = runtime.begin_model("worker").unwrap();
        let session = if wrong {
            "other session".into()
        } else {
            runtime.plugin_session().unwrap()
        };
        runtime
            .update(|r| {
                r.session_hook_allowance =
                    Some(session_budget::SessionHookAllowance::new(Limits::default())?);
                r.operations[0].budget = Some(BudgetRef::SessionHooks { session });
                Ok(())
            })
            .unwrap();
        runtime.finish_model(id).unwrap();
        sink.for_invocation(Some(id))
            .emit_advisory(usage(Some(5)))
            .unwrap();
        let record = runtime.record().unwrap();
        assert!(!record.allocation.unwrap().usage.unknown_input);
        let session_usage = record.session_hook_allowance.unwrap().allocation.usage;
        assert_eq!(session_usage.reported_input, if wrong { 0 } else { 5 });
        assert_eq!(session_usage.unknown_input, !wrong);
        assert_eq!(
            record.operations[0]
                .usage_receipt
                .as_ref()
                .unwrap()
                .unresolved
                .is_some(),
            wrong
        );
    }
}

#[test]
fn tool_replay_returns_original_result_and_reference_after_retirement() {
    let (_root, runtime, _sink, _receiver) = fixture();
    runtime.allocate(Limits::default(), None).unwrap();
    let source = runtime.begin_model("worker").unwrap();
    let call = ToolCall {
        id: "one".into(),
        name: "read".into(),
        arguments: serde_json::json!({"path":"file"}),
    };
    let ToolAdmission::Fresh(id) = runtime.begin_tool("worker", source, &call).unwrap() else {
        panic!("fresh tool");
    };
    runtime.admit_tool(id, &call).unwrap();
    runtime.tool_effect(id).unwrap();
    let result = ToolResult {
        call_id: call.id.clone(),
        tool: call.name.clone(),
        success: true,
        output: "original".into(),
        exit_code: None,
    };
    runtime.original_tool_result(id, &result).unwrap();
    runtime.model_tool_result(id, &result).unwrap();
    runtime.settle_tool(id).unwrap();
    let budget = runtime.operation_budget(id).unwrap();
    runtime.allocate(Limits::default(), None).unwrap();
    let ToolAdmission::Replay(replayed) = runtime.begin_tool("worker", source, &call).unwrap()
    else {
        panic!("replay");
    };
    assert_eq!(replayed, result);
    assert_eq!(runtime.operation_budget(id).unwrap(), budget);
    assert_eq!(runtime.record().unwrap().allocation.unwrap().tool_calls, 0);
    let mut second = call;
    second.id = "two".into();
    assert!(runtime.begin_tool("worker", source, &second).is_err());
}

#[test]
fn legacy_generic_operation_unknown_is_visible_without_poisoning_a_new_grant() {
    let (_root, runtime, _sink, _receiver) = fixture();
    runtime.allocate(Limits::default(), None).unwrap();
    let id = runtime.begin_model("worker").unwrap();
    runtime
        .update(|r| {
            r.operations[0].budget = None;
            r.operations[0].host_invocation = None;
            Ok(())
        })
        .unwrap();
    runtime.finish_model(id).unwrap();
    let record = runtime.record().unwrap();
    assert!(!record.allocation.unwrap().usage.unknown_input);
    assert!(
        record.operations[0]
            .usage_receipt
            .as_ref()
            .unwrap()
            .missing_report
    );
    assert_eq!(
        record.operations[0]
            .usage_receipt
            .as_ref()
            .unwrap()
            .unresolved,
        Some(budget_accounting::UnresolvedAttribution::LegacyBudget)
    );
}

#[test]
fn unallocated_compatibility_tool_notice_cannot_debit_a_later_task() {
    let (_root, runtime, sink, _receiver) = fixture();
    let id = runtime.begin_backend("worker").unwrap();
    runtime.allocate(Limits::default(), None).unwrap();
    sink.for_invocation(Some(id))
        .emit_advisory(Event::ToolStarted {
            call: ToolCall {
                id: "backend-call".into(),
                name: "read".into(),
                arguments: Value::Null,
            },
        })
        .unwrap();
    let record = runtime.record().unwrap();
    assert_eq!(record.allocation.unwrap().tool_calls, 0);
    assert!(matches!(
        record.operations.last().unwrap().budget,
        Some(BudgetRef::Unallocated)
    ));
}

#[test]
fn resume_settles_only_original_budget_and_preserves_legacy_aggregates() {
    const FLAG: &str = "DEMONCODER_BUDGET_RESUME_FIXTURE";
    if std::env::var_os(FLAG).is_none() {
        let home = tempfile::tempdir().unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "workflow::runtime::budget_tests::resume_settles_only_original_budget_and_preserves_legacy_aggregates", "--nocapture"])
            .env("HOME", home.path()).env(FLAG, "1").output().unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let connection: Connection = serde_json::from_value(
        serde_json::json!({"adapter":"openai-api", "api_key":"test-fixture-key"}),
    )
    .unwrap();
    let (runtime, _) = SharedRuntime::open(root.path(), &connection, None).unwrap();
    runtime.allocate(Limits::default(), None).unwrap();
    let id = runtime.begin_model("worker").unwrap();
    runtime.allocate(Limits::default(), None).unwrap();
    let path = runtime.directory().unwrap();
    drop(runtime);
    let (runtime, resumed) = SharedRuntime::open(root.path(), &connection, Some(&path)).unwrap();
    assert!(resumed);
    let record = runtime.record().unwrap();
    assert!(record.recovery_pending);
    assert!(
        record.retired_task_allocations[0]
            .allocation
            .usage
            .unknown_input
    );
    assert!(!record.allocation.unwrap().usage.unknown_input);
    runtime
        .reconcile("Inspected interrupted remote request", None)
        .unwrap();
    let (sender, _receiver) = tokio::sync::mpsc::channel(8);
    let sink = EventSink::new("fixture".into(), sender, None)
        .unwrap()
        .with_runtime(runtime.clone());
    sink.for_invocation(Some(id))
        .emit_advisory(usage(Some(11)))
        .unwrap();
    assert_eq!(
        runtime.record().unwrap().retired_task_allocations[0]
            .allocation
            .usage
            .reported_input,
        11
    );
    drop(sink);
    drop(runtime);
    // Convert only a separate fixture into the pre-reference wire format.
    let (legacy, _) = SharedRuntime::open(root.path(), &connection, None).unwrap();
    legacy.allocate(Limits::default(), None).unwrap();
    legacy.begin_model("worker").unwrap();
    legacy
        .update(|r| {
            r.allocation.as_mut().unwrap().usage.reported_input = 19;
            Ok(())
        })
        .unwrap();
    let path = legacy.directory().unwrap();
    drop(legacy);
    {
        let mut store = Store::open(&path).unwrap();
        let mut old = store.read().unwrap();
        let object = old.as_object_mut().unwrap();
        object.remove("task_allocation_epoch");
        object.remove("retired_task_allocations");
        object.remove("unattributed_usage");
        for operation in object
            .get_mut("operations")
            .unwrap()
            .as_array_mut()
            .unwrap()
        {
            let operation = operation.as_object_mut().unwrap();
            operation.remove("budget");
            operation.remove("usage_receipt");
            operation.remove("host_invocation");
        }
        store.write(&old).unwrap();
    }
    let (legacy, _) = SharedRuntime::open(root.path(), &connection, Some(&path)).unwrap();
    let record = legacy.record().unwrap();
    assert_eq!(record.task_allocation_epoch, 0);
    assert_eq!(record.allocation.as_ref().unwrap().usage.reported_input, 19);
    assert!(!record.allocation.unwrap().usage.unknown_input);
    assert_eq!(
        record.operations[0]
            .usage_receipt
            .as_ref()
            .unwrap()
            .unresolved,
        Some(budget_accounting::UnresolvedAttribution::LegacyBudget)
    );
    legacy
        .reconcile("Inspected old record without inventing usage", None)
        .unwrap();
    let id = legacy.begin_model("worker").unwrap();
    assert!(matches!(
        legacy.operation_budget(id).unwrap(),
        BudgetRef::Task { epoch: 0, .. }
    ));
}

#[test]
fn full_retention_allows_unreferenced_clear_and_duplicate_epoch_refusal_is_atomic() {
    let (_root, runtime, _sink, _receiver) = fixture();
    for _ in 0..32 {
        runtime.allocate(Limits::default(), None).unwrap();
        let id = runtime.begin_model("worker").unwrap();
        runtime.finish_model(id).unwrap();
    }
    runtime.allocate(Limits::default(), None).unwrap();
    runtime.archive().unwrap();
    runtime.allocate(Limits::default(), None).unwrap();
    assert_eq!(runtime.record().unwrap().retired_task_allocations.len(), 32);
    runtime
        .update(|r| {
            r.retired_task_allocations[1].epoch = r.retired_task_allocations[0].epoch;
            Ok(())
        })
        .unwrap();
    let before = serde_json::to_value(runtime.record().unwrap()).unwrap();
    assert!(runtime.allocate(Limits::default(), None).is_err());
    assert_eq!(
        serde_json::to_value(runtime.record().unwrap()).unwrap(),
        before
    );
}

#[test]
fn unmatched_tool_notices_retain_visible_unfunded_compatibility_outcome() {
    let (_root, runtime, sink, _receiver) = fixture();
    runtime.allocate(Limits::default(), None).unwrap();
    let commands = runtime.begin_commands("worker", None).unwrap();
    for (index, source) in [None, Some(999), Some(commands)].into_iter().enumerate() {
        let call = ToolCall {
            id: format!("unmatched-{index}"),
            name: "read".into(),
            arguments: Value::Null,
        };
        sink.for_invocation(source)
            .emit_advisory(Event::ToolStarted { call })
            .unwrap();
    }
    let record = runtime.record().unwrap();
    assert_eq!(record.allocation.unwrap().tool_calls, 0);
    for operation in &record.operations[1..] {
        assert!(matches!(operation.budget, Some(BudgetRef::Unallocated)));
        assert_eq!(
            operation.usage_receipt.as_ref().unwrap().unresolved,
            Some(budget_accounting::UnresolvedAttribution::InvalidRecipient)
        );
        assert!(operation.tool_receipt.is_none());
    }
}
