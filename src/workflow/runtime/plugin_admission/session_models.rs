use super::*;
use crate::{
    events::{Event, EventSink},
    plugins::{
        gate_snapshot::{GateReadSet, GateWorkspace},
        hook_types::{HandlerKind, HookDialect},
        runners::SnapshotInspection,
    },
    session::SessionStart,
    workflow::{
        allocation::Limits,
        runtime::{BudgetRef, HostInvocation, session_budget::SessionHookAllowance},
        state::Task,
        workspace,
    },
};
use std::sync::atomic::{AtomicBool, Ordering};

struct Fixture {
    root: tempfile::TempDir,
    _state: tempfile::TempDir,
    runtime: SharedRuntime,
    hook: ModelAdmission,
    receipt: HookReceipt,
    lifetime: u64,
    events: EventSink,
    _receivers: [tokio::sync::mpsc::Receiver<crate::events::Envelope>; 2],
}
fn fixture(slots: u64) -> Fixture {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("public"), "original snapshot").unwrap();
    let mut record = crate::inspection::tests::record(root.path());
    record.session_hook_allowance = Some(
        SessionHookAllowance::new(Limits {
            seconds: 60,
            model_calls: slots,
            tool_calls: 2,
        })
        .unwrap(),
    );
    let runtime = SharedRuntime::for_test(&state.path().join("record"), record).unwrap();
    let (sender, receiver) = tokio::sync::mpsc::channel(64);
    let events = EventSink::new("session model test".into(), sender, None)
        .unwrap()
        .with_runtime(runtime.clone());
    let events = events
        .begin_host_lifetime(
            SessionStart::Startup,
            vec![(HookEvent::SessionStart, "plan".into())],
        )
        .unwrap()
        .unwrap();
    let lifetime = runtime.record().unwrap().operations.last().unwrap().id;
    let declaration = DeclarationIdentity {
        package: "fixture".into(),
        code: "code".into(),
        policy: "policy".into(),
        configuration: "config".into(),
        generation: "1".into(),
        scope: Scope::Project,
        role: "worker".into(),
        declaration: "session".into(),
        index: 0,
        dialect: HookDialect::Native,
        runner: HandlerKind::Agent,
    };
    let (events, facts) = events
        .for_non_tool(
            NonToolOccurrence::SessionStart {
                source: SessionStart::Startup,
            },
            "plan".into(),
            vec![serde_json::json!({"identity":declaration,"required_gate":false})],
        )
        .unwrap();
    let receipt = HookReceipt {
        required_gate: false,
        observer: None,
        source: None,
        once: None,
        invocation: 0,
        declaration,
        class: HandlerClass::DecisionGate,
        endpoint: None,
        inspected: AdmissionKey {
            session: facts.session.clone(),
            operation: facts.operation,
            source_operation: lifetime,
            event: "SessionStart".into(),
            tool: None,
            arguments: None,
            lifecycle: Some(facts.subject),
            plan: "plan".into(),
            role: "native-session".into(),
            workspace: facts.workspace,
            inputs: vec![],
            external: None,
        },
        outcome: None,
        uncertain_effects: false,
        hold: None,
        questions: vec![],
        pending_proposals: vec![],
    };
    runtime
        .reserve_non_tool_hook(
            facts.operation,
            HookEvent::SessionStart,
            receipt.clone(),
            None,
            None,
        )
        .unwrap();
    let snapshot = Arc::new(
        GateWorkspace::open(root.path())
            .unwrap()
            .capture(&GateReadSet::default(), &AtomicBool::new(false))
            .unwrap(),
    );
    let tools = crate::tools::ToolExecutor::new(root.path()).unwrap();
    let hook = ModelAdmission {
        key: receipt.inspected.clone(),
        budget: runtime.operation_budget(facts.operation).unwrap(),
        owner: facts.operation,
        invocation: 0,
        event: HookEvent::SessionStart,
        maximum: 8,
        snapshot: Arc::new(SnapshotInspection::new(
            snapshot,
            tools.hook_host(),
            65536,
            65536,
            4,
        )),
        cancelled: Arc::new(AtomicBool::new(false)),
    };
    let (sender, model_receiver) = tokio::sync::mpsc::channel(64);
    let events = events
        .for_hook_model(0, 8, hook.snapshot.clone(), hook.cancelled.clone(), sender)
        .unwrap();
    Fixture {
        root,
        _state: state,
        runtime,
        hook,
        receipt,
        lifetime,
        events,
        _receivers: [receiver, model_receiver],
    }
}

#[test]
fn session_model_invalid_live_capabilities_never_admit_model_or_backend() {
    for case in [
        "held",
        "reloaded",
        "stale",
        "cancelled",
        "key",
        "occurrence",
        "unallocated",
        "task",
        "foreign",
        "expired",
        "settled",
        "identity",
    ] {
        let mut f = fixture(2);
        f.runtime.allocate(Limits::default(), None).unwrap();
        match case {
            "held" => f.runtime.hold().unwrap(),
            "reloaded" => f
                .runtime
                .update(|record| {
                    *record = serde_json::from_value(serde_json::to_value(&*record)?)?;
                    Ok(())
                })
                .unwrap(),
            "stale" => {
                f.runtime
                    .begin_native_session(SessionStart::Resume, None, vec![])
                    .unwrap();
            }
            "cancelled" => f.hook.cancelled.store(true, Ordering::Release),
            "key" => f.hook.key.plan = "changed-generation".into(),
            "occurrence" => f.hook.event = HookEvent::SessionEnd,
            "unallocated" => f.hook.budget = BudgetRef::Unallocated,
            "task" => {
                f.hook.budget = BudgetRef::Task {
                    session: f.hook.key.session.clone(),
                    epoch: f.runtime.record().unwrap().task_allocation_epoch,
                }
            }
            "foreign" => {
                f.hook.budget = BudgetRef::SessionHooks {
                    session: "foreign".into(),
                }
            }
            "expired" => f
                .runtime
                .update(|r| {
                    r.session_hook_allowance
                        .as_mut()
                        .unwrap()
                        .allocation
                        .deadline_ms = 0;
                    Ok(())
                })
                .unwrap(),
            "settled" => {
                let mut receipt = f.receipt.clone();
                receipt.outcome = Some(RawOutcome::Model {
                    value: serde_json::json!({"ok":true}),
                    continue_on_block: false,
                });
                f.runtime
                    .finish_non_tool_hook(f.hook.owner, f.hook.event, receipt)
                    .unwrap();
            }
            "identity" => f
                .runtime
                .update(|r| {
                    r.identity.model = Some("changed".into());
                    Ok(())
                })
                .unwrap(),
            _ => unreachable!(),
        }
        let before = serde_json::to_value(f.runtime.record().unwrap()).unwrap();
        assert!(
            f.runtime.validate_hook_model_owner(&f.hook).is_err(),
            "{case}"
        );
        assert!(
            f.runtime
                .begin_model_owned("hook:test", None, Some(&f.hook))
                .is_err(),
            "{case}"
        );
        assert!(
            f.runtime
                .begin_backend_owned("hook:test", None, Some(&f.hook))
                .is_err(),
            "{case}"
        );
        assert_eq!(
            serde_json::to_value(f.runtime.record().unwrap()).unwrap(),
            before,
            "{case} changed state before denial"
        );
    }
}

#[test]
fn session_model_task_stop_accept_replacement_and_archive_keep_original_ownership_and_usage() {
    for state in ["stopped", "accepted", "replaced", "archived"] {
        let f = fixture(3);
        f.runtime.allocate(Limits::default(), None).unwrap();
        let mut task = Task::new(
            1,
            "original task".into(),
            vec![],
            workspace::capture(f.root.path()).unwrap(),
            1,
        )
        .unwrap();
        f.runtime.save_task(&Some(task.clone()), 2, None).unwrap();
        let invocation = f
            .runtime
            .begin_model_owned("hook:test", None, Some(&f.hook))
            .unwrap();
        let original = f
            .runtime
            .record()
            .unwrap()
            .session_hook_allowance
            .unwrap()
            .allocation
            .deadline_ms;
        match state {
            "stopped" => task.stopped = true,
            "accepted" => task.accepted = Some("accepted-evidence".into()),
            "replaced" => {
                task.id = 2;
                f.runtime.allocate(Limits::default(), None).unwrap();
            }
            "archived" => {
                f.runtime.archive().unwrap();
            }
            _ => unreachable!(),
        }
        if state != "archived" {
            f.runtime.save_task(&Some(task), 3, None).unwrap();
        }
        f.runtime.validate_hook_model_owner(&f.hook).unwrap();
        f.runtime
            .observe_invocation(
                &Event::Usage {
                    input: Some(17),
                    output: Some(9),
                    cached: None,
                    cost_usd: None,
                },
                "hook:test",
                Some(invocation),
            )
            .unwrap();
        f.runtime.finish_model(invocation).unwrap();
        let second = f
            .runtime
            .begin_model_owned("hook:test", None, Some(&f.hook))
            .unwrap();
        f.runtime.finish_model(second).unwrap();
        let record = f.runtime.record().unwrap();
        let grant = record.session_hook_allowance.unwrap();
        assert_eq!(grant.allocation.model_calls, 2, "{state}");
        assert_eq!(grant.allocation.usage.reported_input, 17, "{state}");
        assert_eq!(grant.allocation.deadline_ms, original, "{state}");
        assert!(
            record
                .allocation
                .is_none_or(|a| a.model_calls == 0 && a.usage.reported_input == 0)
        );
    }
}

#[test]
fn session_model_backend_overflow_and_exhaustion_preserve_every_counter() {
    for case in ["backend", "model", "exhausted"] {
        let f = fixture(2);
        f.runtime
            .update(|r| {
                let grant = r.session_hook_allowance.as_mut().unwrap();
                if case == "backend" {
                    grant.backend_invocations = u64::MAX;
                }
                if case == "model" {
                    grant.allocation.model_calls = u64::MAX;
                }
                if case == "exhausted" {
                    grant.allocation.model_calls = grant.allocation.limits.model_calls;
                }
                Ok(())
            })
            .unwrap();
        let before = serde_json::to_value(f.runtime.record().unwrap()).unwrap();
        assert!(
            f.runtime
                .begin_backend_owned("hook:test", None, Some(&f.hook))
                .is_err()
        );
        assert_eq!(
            serde_json::to_value(f.runtime.record().unwrap()).unwrap(),
            before,
            "{case}"
        );
    }
}

#[test]
fn session_model_backends_ignore_unrelated_delegation_exhaustion_and_overflow() {
    for count in [1, u64::MAX] {
        let f = fixture(2);
        f.runtime
            .update(|r| {
                r.backend_invocations = count;
                r.delegation = Some(crate::subagents::state::DelegationIdentity {
                    connections: Default::default(),
                    reviewer: None,
                    max_active: 1,
                    backend_limit: 1,
                    orchestration: None,
                    default_roles: vec![],
                });
                Ok(())
            })
            .unwrap();
        let id = f
            .runtime
            .begin_backend_owned("hook:test", None, Some(&f.hook))
            .unwrap();
        f.runtime.finish_model(id).unwrap();
        let record = f.runtime.record().unwrap();
        assert_eq!(record.backend_invocations, count);
        let page = crate::inspection::project(
            &record,
            Some(crate::inspection::Request {
                target: crate::inspection::Target::Overview,
                page: 0,
                generation: 0,
            }),
        )
        .page
        .unwrap();
        assert!(page.text.contains("host backend invocations 1"));
        let grant = record.session_hook_allowance.unwrap();
        assert_eq!(grant.backend_invocations, 1);
        assert_eq!(grant.allocation.model_calls, 1);
        assert!(grant.allocation.usage.unknown_input);
    }
}

#[test]
fn session_model_concurrent_hook_admissions_reserve_last_slot_atomically() {
    let f = fixture(1);
    let reservation = f
        .runtime
        .reserve_non_tool_hook(f.hook.owner, f.hook.event, f.receipt.clone(), None, None)
        .unwrap();
    let crate::plugins::once::HookReservation::Run(receipt) = reservation else {
        panic!("second hook unexpectedly skipped")
    };
    let mut second = f.hook.clone();
    second.invocation = receipt.invocation;
    let barrier = Arc::new(std::sync::Barrier::new(3));
    let mut threads = vec![];
    for (index, hook) in [f.hook.clone(), second].into_iter().enumerate() {
        let runtime = f.runtime.clone();
        let barrier = barrier.clone();
        threads.push(std::thread::spawn(move || {
            barrier.wait();
            runtime.begin_model_owned(&format!("hook:{index}"), None, Some(&hook))
        }));
    }
    barrier.wait();
    let results = threads
        .into_iter()
        .map(|t| t.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        f.runtime
            .record()
            .unwrap()
            .session_hook_allowance
            .unwrap()
            .allocation
            .model_calls,
        1
    );
}

#[test]
fn session_model_legacy_backend_counter_defaults_without_restoring_live_lifetime() {
    let f = fixture(2);
    let mut serialized = serde_json::to_value(f.runtime.record().unwrap()).unwrap();
    serialized["session_hook_allowance"]
        .as_object_mut()
        .unwrap()
        .remove("backend_invocations");
    let record: Record = serde_json::from_value(serialized).unwrap();
    assert_eq!(
        record
            .session_hook_allowance
            .as_ref()
            .unwrap()
            .backend_invocations,
        0
    );
    let Some(HostInvocation::NativeSession(lifetime)) = &record
        .operations
        .iter()
        .find(|o| o.id == f.lifetime)
        .unwrap()
        .host_invocation
    else {
        panic!("original lifetime missing")
    };
    assert!(lifetime.deadline.is_none());
    assert!(validate_model_owner(&record, &f.hook.key.session, &f.hook).is_err());
}

struct CheckpointInterruption {
    root: std::path::PathBuf,
    point: &'static str,
    lifetime: u64,
    cancel: bool,
    cancelled: Arc<AtomicBool>,
    reached: Arc<AtomicBool>,
}
thread_local! {
    static INTERRUPT_CHECKPOINT: std::cell::RefCell<Option<CheckpointInterruption>> = const { std::cell::RefCell::new(None) };
}
/// A one-shot test fault between durable admission mutation and its clock checkpoint.
pub(in crate::workflow::runtime) fn invalidate_at_checkpoint(record: &mut Record) {
    INTERRUPT_CHECKPOINT.with_borrow_mut(|pending| {
        let Some(interruption) = pending.as_ref() else {
            return;
        };
        if record.workspace != interruption.root {
            return;
        }
        let Some(receipt) = record
            .operations
            .last()
            .and_then(|o| o.tool_receipt.as_ref())
        else {
            return;
        };
        let matched = match interruption.point {
            "begin" => receipt.attempt_admitted && !receipt.admitted,
            "admit" => receipt.admitted && !receipt.effect_started,
            "effect" => receipt.effect_started,
            "observer" => receipt.observer_pending.is_some(),
            _ => false,
        };
        if !matched {
            return;
        }
        let interruption = pending.take().unwrap();
        interruption.reached.store(true, Ordering::Release);
        if interruption.cancel {
            interruption.cancelled.store(true, Ordering::Release);
        } else {
            let operation = record
                .operations
                .iter_mut()
                .find(|o| o.id == interruption.lifetime)
                .unwrap();
            let Some(HostInvocation::NativeSession(owner)) = &mut operation.host_invocation else {
                panic!("lifetime changed")
            };
            owner.deadline = Some(std::time::Instant::now());
        }
    });
}
struct CountToolHooks(
    Arc<std::sync::atomic::AtomicUsize>,
    Arc<std::sync::atomic::AtomicUsize>,
);
impl crate::tools::ToolHook for CountToolHooks {
    fn before(&self, _: &mut crate::tools::ToolCall) -> Result<()> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn present(&self, _: &crate::tools::ToolResult) -> Result<String> {
        self.1.fetch_add(1, Ordering::SeqCst);
        Ok("presentation effect".into())
    }
}
#[tokio::test]
async fn session_model_checkpoint_expiry_or_cancel_withholds_snapshot_and_observer_effects() {
    use crate::tools::{AccessPolicy, ToolCall, ToolExecutor};
    for point in ["effect", "begin", "admit", "observer", "valid"] {
        for cancel in [false, true] {
            let f = fixture(2);
            let invocation = f.events.begin_model().unwrap();
            let events = f.events.for_invocation(invocation);
            let before = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let presentations = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let mut tools = ToolExecutor::with_policy(
                f.root.path(),
                &AccessPolicy {
                    tools_enabled: true,
                    snapshot: Some(f.hook.snapshot.clone()),
                    ..AccessPolicy::review_only()
                },
            )
            .unwrap();
            tools.add_hook(Box::new(CountToolHooks(
                before.clone(),
                presentations.clone(),
            )));
            let reached = Arc::new(AtomicBool::new(false));
            if point != "valid" {
                INTERRUPT_CHECKPOINT.set(Some(CheckpointInterruption {
                    root: f.root.path().into(),
                    point,
                    lifetime: f.lifetime,
                    cancel,
                    cancelled: f.hook.cancelled.clone(),
                    reached: reached.clone(),
                }));
            }
            let result = tools
                .execute(
                    ToolCall {
                        id: "inspection".into(),
                        name: "snapshot_read".into(),
                        arguments: serde_json::json!({"path":"public"}),
                    },
                    &events,
                )
                .await;
            INTERRUPT_CHECKPOINT.set(None);
            let record = f.runtime.record().unwrap();
            let grant = &record.session_hook_allowance.as_ref().unwrap().allocation;
            assert_eq!(grant.tool_calls, 1, "{point} cancel={cancel}");
            assert!(
                grant.remaining_ms().unwrap() > 0,
                "the cumulative grant itself must stay valid"
            );
            let operation = record
                .operations
                .iter()
                .find(|o| o.tool_receipt.is_some())
                .unwrap();
            if point == "valid" {
                let result = result.unwrap();
                assert!(result.success && result.output.contains("original snapshot"));
                assert_eq!(presentations.load(Ordering::SeqCst), 1);
                continue;
            }
            assert!(
                reached.load(Ordering::Acquire),
                "test missed {point} checkpoint"
            );
            assert_eq!(
                presentations.load(Ordering::SeqCst),
                0,
                "{point} cancel={cancel} ran observer after checkpoint revocation"
            );
            if point == "observer" {
                assert!(result.is_err());
                assert!(
                    operation.result.as_ref().unwrap().success,
                    "completed snapshot evidence must survive"
                );
                assert_eq!(
                    operation.tool_receipt.as_ref().unwrap().observer_pending,
                    Some(0)
                );
            } else {
                assert!(
                    result.as_ref().map_or(true, |r| !r.success
                        && !r.output.contains("original snapshot")),
                    "{point} cancel={cancel} released revoked snapshot: {result:?}"
                );
                assert!(
                    operation
                        .result
                        .as_ref()
                        .is_none_or(|r| !r.success && !r.output.contains("original snapshot")),
                    "{point} cancel={cancel} executed revoked snapshot: {:?}",
                    operation.result
                );
            }
            if point == "begin" {
                assert_eq!(before.load(Ordering::SeqCst), 0);
            }
            if point == "effect" {
                assert!(
                    operation.tool_receipt.as_ref().unwrap().effect_started,
                    "must reach durable effect marker before refusal"
                );
            }
            // Revoked effect authority must still allow retained evidence and usage settlement.
            if point == "observer" {
                let original = operation.result.as_ref().unwrap();
                f.runtime
                    .original_tool_result(operation.id, original)
                    .unwrap();
                f.runtime.model_tool_result(operation.id, original).unwrap();
                f.runtime
                    .tool_observer(
                        operation.id,
                        0,
                        Some(Err("presentation withheld after checkpoint")),
                    )
                    .unwrap();
            }
            f.runtime
                .observe_invocation(
                    &Event::Usage {
                        input: Some(7),
                        output: Some(3),
                        cached: None,
                        cost_usd: None,
                    },
                    &record
                        .operations
                        .iter()
                        .find(|o| Some(o.id) == invocation)
                        .unwrap()
                        .phase,
                    invocation,
                )
                .unwrap();
            events.finish_model(invocation).unwrap();
            let settled = f.runtime.record().unwrap();
            let grant = settled.session_hook_allowance.unwrap();
            assert_eq!(grant.allocation.usage.reported_input, 7);
            assert_eq!(grant.allocation.usage.reported_output, 3);
            assert!(grant.allocation.usage.unknown_cost);
            assert!(
                settled
                    .operations
                    .iter()
                    .find(|o| Some(o.id) == invocation)
                    .unwrap()
                    .complete
            );
        }
    }
}
