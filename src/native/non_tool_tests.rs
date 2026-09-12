use super::*;
use crate::{
    plugins::{
        dispatch::*,
        gate_snapshot::GateReadSet,
        hook_types::{HandlerKind, HookDialect, HookEvent},
        non_tool::NonToolPlan,
    },
    workflow::runtime::SharedRuntime,
};
use serde_json::json;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

mod session_lifetime;
mod stop_failure;

struct Counting {
    requests: Arc<AtomicUsize>,
    prompts: Arc<Mutex<Vec<String>>>,
}
#[async_trait]
impl Model for Counting {
    fn checkpoint(&self) -> Option<serde_json::Value> {
        Some(json!({"fixture":"counting"}))
    }
    fn prompt(&mut self, text: String) {
        self.prompts.lock().unwrap().push(text);
    }
    fn results(&mut self, _: Vec<ToolResult>) {}
    async fn response(&mut self, _: &EventSink) -> Result<Vec<ToolCall>> {
        self.requests.fetch_add(1, Ordering::SeqCst);
        Ok(vec![])
    }
}
struct Deny;
#[async_trait]
impl HookRunner for Deny {
    fn side_effect_free(&self) -> bool {
        true
    }
    async fn run(&self, invocation: &HookInvocation) -> Result<RawOutcome> {
        assert!(invocation.candidate.is_none() && invocation.completed.is_none());
        assert!(invocation.lifecycle.is_some());
        Ok(RawOutcome::Callback {
            value: json!({"decision":"block", "reason":"visible rejection"}),
        })
    }
}
fn plan(event: HookEvent, runner: Arc<dyn HookRunner>) -> Arc<NonToolPlan> {
    Arc::new(
        NonToolPlan::new(
            event,
            vec![Registration {
                declaration: Declaration {
                    required_gate: true,
                    source: None,
                    once: None,
                    identity: DeclarationIdentity {
                        package: "native-lifecycle".into(),
                        code: "code".into(),
                        policy: "policy".into(),
                        configuration: "config".into(),
                        generation: "1".into(),
                        scope: Scope::Project,
                        role: "worker".into(),
                        declaration: event.as_str().into(),
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
                },
                runner,
                revalidation: None,
            }],
        )
        .unwrap(),
    )
}
#[tokio::test]
async fn blocked_native_prompt_never_reaches_model_and_remains_inspectable() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let mut record = crate::inspection::tests::record(root.path());
    record.phase = Some("worker".into());
    let runtime = SharedRuntime::for_test(&state.path().join("record"), record).unwrap();
    let mut tools = ToolExecutor::new(root.path()).unwrap();
    tools
        .register_non_tool_plan(plan(HookEvent::UserPromptSubmit, Arc::new(Deny)))
        .unwrap();
    let requests = Arc::new(AtomicUsize::new(0));
    let prompts = Arc::new(Mutex::new(vec![]));
    let mut session = NativeSession::with_tools(
        Box::new(Counting {
            requests: requests.clone(),
            prompts: prompts.clone(),
        }),
        tools,
    );
    let (_commands, mut command_rx) = mpsc::channel(4);
    let (tx, _rx) = mpsc::channel(256);
    let events = EventSink::new("native-lifecycle".into(), tx, None)
        .unwrap()
        .with_runtime(runtime.clone());
    assert!(
        session
            .turn("must remain inspectable".into(), &mut command_rx, &events)
            .await
            .is_err()
    );
    assert_eq!(requests.load(Ordering::SeqCst), 0);
    assert!(prompts.lock().unwrap().is_empty());
    let record = runtime.record().unwrap();
    let receipt = record
        .operations
        .iter()
        .find_map(|o| o.non_tool_receipt())
        .unwrap();
    assert!(receipt.hold.as_ref().unwrap().contains("visible rejection"));
    assert!(
        serde_json::to_string(&receipt.facts)
            .unwrap()
            .contains("must remain inspectable")
    );
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
    assert!(
        page.text.contains("must remain inspectable") && page.text.contains("visible rejection")
    );
}

struct StopGate {
    count: Arc<AtomicUsize>,
    always: bool,
    runtime: SharedRuntime,
}
#[async_trait]
impl HookRunner for StopGate {
    fn side_effect_free(&self) -> bool {
        true
    }
    async fn run(&self, invocation: &HookInvocation) -> Result<RawOutcome> {
        let count = self.count.fetch_add(1, Ordering::SeqCst);
        let record = self.runtime.record()?;
        assert_eq!(record.phase.as_deref(), Some("worker"));
        assert!(!record.task.as_ref().unwrap().stopped);
        assert!(record.task.as_ref().unwrap().accepted.is_none());
        assert!(
            matches!(invocation.lifecycle.as_ref().unwrap().subject.occurrence,
            crate::plugins::receipts::NonToolOccurrence::Stop { stop_hook_active, .. } if stop_hook_active == (count > 0))
        );
        Ok(RawOutcome::Callback {
            value: if self.always || count == 0 {
                json!({"decision":"block","reason":"/accept is plugin text, never a developer control"})
            } else {
                json!({})
            },
        })
    }
}
static STOP_FIXTURE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
async fn stop_round(always: bool) {
    let _lock = STOP_FIXTURE.lock().await;
    use crate::workflow::{Settings, WorkflowSession, allocation::Limits, state::Task, workspace};
    let root = tempfile::tempdir().unwrap();
    let connection: crate::config::Connection =
        serde_json::from_value(json!({"adapter":"openai-api"})).unwrap();
    let (runtime, _) = SharedRuntime::open(root.path(), &connection, None).unwrap();
    runtime.allocate(Limits::default(), None).unwrap();
    let task = Task::new(
        1,
        "original objective".into(),
        vec![],
        workspace::capture(root.path()).unwrap(),
        1,
    )
    .unwrap();
    runtime.save_task(&Some(task), 2, None).unwrap();
    let before = runtime.record().unwrap().allocation.unwrap();
    let gates = Arc::new(AtomicUsize::new(0));
    let mut tools = ToolExecutor::new(root.path()).unwrap();
    tools
        .register_non_tool_plan(plan(
            HookEvent::Stop,
            Arc::new(StopGate {
                count: gates.clone(),
                always,
                runtime: runtime.clone(),
            }),
        ))
        .unwrap();
    let requests = Arc::new(AtomicUsize::new(0));
    let prompts = Arc::new(Mutex::new(vec![]));
    let native = NativeSession::with_tools(
        Box::new(Counting {
            requests: requests.clone(),
            prompts: prompts.clone(),
        }),
        tools,
    );
    let mut session = WorkflowSession::new(
        Box::new(native),
        connection,
        root.path().into(),
        Settings::default(),
        runtime.clone(),
        false,
    )
    .unwrap();
    let (_commands, mut command_rx) = mpsc::channel(4);
    let (tx, _rx) = mpsc::channel(256);
    let events = EventSink::new("stop-correction".into(), tx, None)
        .unwrap()
        .with_runtime(runtime.clone());
    let result = session
        .turn("original objective".into(), &mut command_rx, &events)
        .await;
    if always {
        assert!(result.is_err(), "always-block must end unmet");
    } else {
        assert!(
            matches!(result, Ok(TurnEnd::Complete)),
            "{:?}",
            result.err()
        );
    }
    assert_eq!(requests.load(Ordering::SeqCst), 2);
    assert_eq!(gates.load(Ordering::SeqCst), 2);
    let record = runtime.record().unwrap();
    assert_eq!(record.task.as_ref().unwrap().corrections, 1);
    let receipts: Vec<_> = record
        .operations
        .iter()
        .filter_map(|o| o.non_tool_receipt())
        .collect();
    assert_eq!(
        receipts.len(),
        2,
        "Stop corrections must not fabricate Submit"
    );
    assert!(receipts[0].facts.native_turn.is_some());
    assert_eq!(receipts[0].facts.native_turn, receipts[1].facts.native_turn);
    assert!(record.task.as_ref().unwrap().stopped);
    assert!(record.task.as_ref().unwrap().accepted.is_none());
    assert_eq!(
        record.task.as_ref().unwrap().objective,
        "original objective"
    );
    let after = record.allocation.as_ref().unwrap();
    assert_eq!(after.started_ms, before.started_ms);
    assert_eq!(after.deadline_ms, before.deadline_ms);
    assert_eq!(after.model_calls, 2);
    assert!(
        prompts
            .lock()
            .unwrap()
            .iter()
            .any(|p| p.starts_with("[Plugin-origin Stop correction]") && p.contains("/accept"))
    );
    assert_eq!(record.phase, None);
}
#[tokio::test]
async fn native_stop_corrected_pass_uses_original_allocation_before_task_stops() {
    stop_round(false).await;
}
#[tokio::test]
async fn native_always_blocking_stop_terminates_with_unmet_gate() {
    stop_round(true).await;
}

struct PendingGate(Arc<tokio::sync::Notify>);
#[async_trait]
impl HookRunner for PendingGate {
    async fn run(&self, _: &HookInvocation) -> Result<RawOutcome> {
        self.0.notify_one();
        std::future::pending().await
    }
}
#[tokio::test]
async fn native_cancel_and_shutdown_win_during_a_blocking_lifecycle_gate() {
    for (event, expected_requests) in [(HookEvent::UserPromptSubmit, 0), (HookEvent::Stop, 1)] {
        for shutdown in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let state = tempfile::tempdir().unwrap();
            let mut record = crate::inspection::tests::record(root.path());
            record.phase = Some("worker".into());
            let runtime = SharedRuntime::for_test(&state.path().join("record"), record).unwrap();
            let ready = Arc::new(tokio::sync::Notify::new());
            let mut tools = ToolExecutor::new(root.path()).unwrap();
            tools
                .register_non_tool_plan(plan(event, Arc::new(PendingGate(ready.clone()))))
                .unwrap();
            let requests = Arc::new(AtomicUsize::new(0));
            let mut session = NativeSession::with_tools(
                Box::new(Counting {
                    requests: requests.clone(),
                    prompts: Arc::new(Mutex::new(vec![])),
                }),
                tools,
            );
            let (commands, mut command_rx) = mpsc::channel(4);
            let (tx, _rx) = mpsc::channel(256);
            let events = EventSink::new("cancel-lifecycle".into(), tx, None)
                .unwrap()
                .with_runtime(runtime.clone());
            let control = async {
                ready.notified().await;
                commands
                    .send(if shutdown {
                        Command::Shutdown
                    } else {
                        Command::Cancel
                    })
                    .await
                    .unwrap();
            };
            let (outcome, ()) = tokio::time::timeout(std::time::Duration::from_secs(2), async {
                tokio::join!(
                    session.turn("pending".into(), &mut command_rx, &events),
                    control
                )
            })
            .await
            .unwrap();
            assert!(matches!(outcome.unwrap(), TurnEnd::Shutdown) == shutdown);
            assert_eq!(requests.load(Ordering::SeqCst), expected_requests);
            let record = runtime.record().unwrap();
            let receipt = record
                .operations
                .iter()
                .find_map(|o| o.non_tool_receipt())
                .unwrap();
            assert!(receipt.hooks[0].outcome.is_none());
            runtime.finish_phase().unwrap();
            assert!(runtime.record().unwrap().recovery_pending);
        }
    }
}

struct AllowCount(Arc<AtomicUsize>);
#[async_trait]
impl HookRunner for AllowCount {
    fn side_effect_free(&self) -> bool {
        true
    }
    async fn run(&self, _: &HookInvocation) -> Result<RawOutcome> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(RawOutcome::Callback { value: json!({}) })
    }
}
#[tokio::test]
async fn native_once_prompt_consumption_is_shared_durable_evidence() {
    use crate::plugins::once::{ActivationChange, ActivationSource, HookOrigin};
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let mut record = crate::inspection::tests::record(root.path());
    record.phase = Some("worker".into());
    let runtime = SharedRuntime::for_test(&state.path().join("record"), record).unwrap();
    let count = Arc::new(AtomicUsize::new(0));
    let runner = Arc::new(AllowCount(count.clone()));
    let base = plan(HookEvent::UserPromptSubmit, runner.clone());
    let mut declaration = base.plan.handlers[0].registration.declaration.clone();
    let binding = runtime
        .plugin_hook_activation(
            HookOrigin::Native,
            Scope::Project,
            &ActivationSource::host_namespace("native-lifecycle").unwrap(),
            "prompt-check",
            "worker",
            ActivationChange::ExplicitInvocation,
        )
        .unwrap()
        .unwrap();
    declaration.source = Some(binding.source());
    declaration.once = Some(binding);
    let plan = Arc::new(
        NonToolPlan::new(
            HookEvent::UserPromptSubmit,
            vec![Registration {
                declaration,
                runner,
                revalidation: None,
            }],
        )
        .unwrap(),
    );
    let mut tools = ToolExecutor::new(root.path()).unwrap();
    tools.register_non_tool_plan(plan).unwrap();
    let requests = Arc::new(AtomicUsize::new(0));
    let mut session = NativeSession::with_tools(
        Box::new(Counting {
            requests: requests.clone(),
            prompts: Arc::new(Mutex::new(vec![])),
        }),
        tools,
    );
    let (_commands, mut command_rx) = mpsc::channel(4);
    let (tx, _rx) = mpsc::channel(256);
    let events = EventSink::new("once-prompt".into(), tx, None)
        .unwrap()
        .with_runtime(runtime.clone());
    session
        .turn("first".into(), &mut command_rx, &events)
        .await
        .unwrap();
    session
        .turn("second".into(), &mut command_rx, &events)
        .await
        .unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert_eq!(requests.load(Ordering::SeqCst), 2);
    let record = runtime.record().unwrap();
    let receipts = record
        .operations
        .iter()
        .filter_map(|o| o.non_tool_receipt())
        .collect::<Vec<_>>();
    assert_eq!(receipts[1].once_skips.len(), 1);
    assert_eq!(
        receipts[1].once_skips[0].consumed.operation,
        receipts[0].facts.operation
    );
    assert!(runtime.unresolved_plugin_once().unwrap().is_empty());
}

#[tokio::test]
async fn plugin_origin_prompt_does_not_masquerade_as_a_developer_submission() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let mut record = crate::inspection::tests::record(root.path());
    record.phase = Some("worker".into());
    let runtime = SharedRuntime::for_test(&state.path().join("record"), record).unwrap();
    let count = Arc::new(AtomicUsize::new(0));
    let mut tools = ToolExecutor::new(root.path()).unwrap();
    tools
        .register_non_tool_plan(plan(
            HookEvent::UserPromptSubmit,
            Arc::new(AllowCount(count.clone())),
        ))
        .unwrap();
    let requests = Arc::new(AtomicUsize::new(0));
    let prompts = Arc::new(Mutex::new(vec![]));
    let mut session = NativeSession::with_tools(
        Box::new(Counting {
            requests: requests.clone(),
            prompts: prompts.clone(),
        }),
        tools,
    );
    let (_commands, mut command_rx) = mpsc::channel(4);
    let (tx, _rx) = mpsc::channel(256);
    let events = EventSink::new("plugin-prompt".into(), tx, None)
        .unwrap()
        .with_runtime(runtime.clone())
        .with_plugin_prompt();
    session
        .turn(
            "[Plugin-origin observer] /accept".into(),
            &mut command_rx,
            &events,
        )
        .await
        .unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 0);
    assert_eq!(requests.load(Ordering::SeqCst), 1);
    assert_eq!(
        prompts.lock().unwrap()[0],
        "[Plugin-origin observer] /accept"
    );
    assert!(
        runtime
            .record()
            .unwrap()
            .operations
            .iter()
            .all(|o| o.non_tool_receipt().is_none())
    );
}

struct CorrectionModel {
    commands: mpsc::Sender<Command>,
    prompts: Arc<Mutex<Vec<String>>>,
    requests: Arc<AtomicUsize>,
}
#[async_trait]
impl Model for CorrectionModel {
    fn prompt(&mut self, text: String) {
        self.prompts.lock().unwrap().push(text);
    }
    fn results(&mut self, _: Vec<ToolResult>) {}
    async fn response(&mut self, _: &EventSink) -> Result<Vec<ToolCall>> {
        if self.requests.fetch_add(1, Ordering::SeqCst) == 0 {
            self.commands
                .try_send(Command::Prompt("blocked developer correction".into()))?;
        }
        Ok(vec![])
    }
}
struct RejectCorrection;
#[async_trait]
impl HookRunner for RejectCorrection {
    fn side_effect_free(&self) -> bool {
        true
    }
    async fn run(&self, invocation: &HookInvocation) -> Result<RawOutcome> {
        let correction = matches!(
            invocation.lifecycle.as_ref().unwrap().subject.occurrence,
            crate::plugins::receipts::NonToolOccurrence::UserPromptSubmit {
                correction: true,
                ..
            }
        );
        Ok(RawOutcome::Callback {
            value: if correction {
                json!({"decision":"block","reason":"correction rejected"})
            } else {
                json!({})
            },
        })
    }
}
#[tokio::test]
async fn actual_developer_correction_is_gated_before_its_model_injection() {
    for plugin_origin in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let mut record = crate::inspection::tests::record(root.path());
        record.phase = Some("worker".into());
        let runtime = SharedRuntime::for_test(&state.path().join("record"), record).unwrap();
        let mut tools = ToolExecutor::new(root.path()).unwrap();
        tools
            .register_non_tool_plan(plan(
                HookEvent::UserPromptSubmit,
                Arc::new(RejectCorrection),
            ))
            .unwrap();
        let requests = Arc::new(AtomicUsize::new(0));
        let prompts = Arc::new(Mutex::new(vec![]));
        let (commands, mut command_rx) = mpsc::channel(4);
        let (tx, _rx) = mpsc::channel(256);
        let mut session = NativeSession::with_tools(
            Box::new(CorrectionModel {
                commands,
                prompts: prompts.clone(),
                requests: requests.clone(),
            }),
            tools,
        );
        let events = EventSink::new("developer-correction".into(), tx, None)
            .unwrap()
            .with_runtime(runtime.clone());
        let events = if plugin_origin {
            events.with_plugin_prompt()
        } else {
            events
        };
        assert!(
            session
                .turn("original prompt".into(), &mut command_rx, &events)
                .await
                .is_err()
        );
        assert_eq!(requests.load(Ordering::SeqCst), 1);
        assert_eq!(&*prompts.lock().unwrap(), &["original prompt"]);
        let record = runtime.record().unwrap();
        let receipts = record
            .operations
            .iter()
            .filter_map(|o| o.non_tool_receipt())
            .collect::<Vec<_>>();
        assert_eq!(receipts.len(), if plugin_origin { 1 } else { 2 });
        assert_eq!(
            receipts.last().unwrap().hold.as_deref(),
            Some("correction rejected")
        );
    }
}

struct AsyncGate(Arc<tokio::sync::Notify>);
#[async_trait]
impl HookRunner for AsyncGate {
    fn observer_config(&self) -> Option<crate::plugins::observer::ObserverConfig> {
        Some(crate::plugins::observer::ObserverConfig {
            declared: true,
            rewake: false,
            timeout_ms: 5000,
        })
    }
    async fn run(&self, _: &HookInvocation) -> Result<RawOutcome> {
        self.0.notify_one();
        std::future::pending().await
    }
}
#[tokio::test]
async fn asynchronous_non_tool_observer_transfers_and_is_cancelled_with_native_owner() {
    for admission_failure in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let mut record = crate::inspection::tests::record(root.path());
        record.phase = Some("worker".into());
        let runtime = SharedRuntime::for_test(&state.path().join("record"), record).unwrap();
        runtime
            .allocate(crate::workflow::allocation::Limits::default(), None)
            .unwrap();
        let ready = Arc::new(tokio::sync::Notify::new());
        let runner = Arc::new(AsyncGate(ready.clone()));
        let base = plan(
            HookEvent::UserPromptSubmit,
            Arc::new(AllowCount(Arc::new(AtomicUsize::new(0)))),
        );
        let mut declaration = base.plan.handlers[0].registration.declaration.clone();
        declaration.required_gate = false;
        declaration.class = HandlerClass::Observer;
        let observer = Arc::new(
            NonToolPlan::new(
                HookEvent::UserPromptSubmit,
                vec![Registration {
                    declaration,
                    runner,
                    revalidation: None,
                }],
            )
            .unwrap(),
        );
        let mut tools = ToolExecutor::new(root.path()).unwrap();
        tools.register_non_tool_plan(observer).unwrap();
        let requests = Arc::new(AtomicUsize::new(0));
        let mut session = NativeSession::with_tools(
            Box::new(Counting {
                requests: requests.clone(),
                prompts: Arc::new(Mutex::new(vec![])),
            }),
            tools,
        );
        let (_commands, mut command_rx) = mpsc::channel(4);
        let (tx, _rx) = mpsc::channel(256);
        let events = EventSink::new("async-non-tool".into(), tx, None)
            .unwrap()
            .with_runtime(runtime.clone());
        session
            .turn("observe prompt".into(), &mut command_rx, &events)
            .await
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), ready.notified())
            .await
            .unwrap();
        assert_eq!(requests.load(Ordering::SeqCst), 1);
        let record = runtime.record().unwrap();
        let receipt = record
            .operations
            .iter()
            .find_map(|o| o.non_tool_receipt())
            .unwrap();
        assert!(receipt.settled && receipt.hooks[0].transferred());
        if admission_failure {
            runtime
                .update(|r| {
                    r.recovery_pending = true;
                    Ok(())
                })
                .unwrap();
            assert!(
                session
                    .turn("held next turn".into(), &mut command_rx, &events)
                    .await
                    .is_err()
            );
        } else {
            tokio::time::timeout(std::time::Duration::from_secs(2), session.close())
                .await
                .unwrap()
                .unwrap();
        }
        let record = runtime.record().unwrap();
        let hook = &record
            .operations
            .iter()
            .find_map(|o| o.non_tool_receipt())
            .unwrap()
            .hooks[0];
        assert_eq!(
            hook.observer.as_ref().unwrap().status,
            crate::plugins::observer::Status::Interrupted
        );
        assert!(hook.outcome.is_some());
    }
}

#[test]
fn snapshot_policy_rejects_non_tool_lifecycle_and_duplicate_native_plans() {
    use crate::{
        plugins::{gate_snapshot::GateWorkspace, runners::SnapshotInspection},
        tools::AccessPolicy,
    };
    let root = tempfile::tempdir().unwrap();
    let tools = ToolExecutor::new(root.path()).unwrap();
    let gate = GateWorkspace::open_with_credentials(root.path(), &[]).unwrap();
    let snapshot = Arc::new(
        gate.capture(
            &GateReadSet::default(),
            &std::sync::atomic::AtomicBool::new(false),
        )
        .unwrap(),
    );
    let mut access = AccessPolicy::review_only();
    access.snapshot = Some(Arc::new(SnapshotInspection::new(
        snapshot,
        tools.hook_host(),
        32768,
        16384,
        8,
    )));
    access.non_tools.push(plan(HookEvent::Stop, Arc::new(Deny)));
    assert!(
        ToolExecutor::with_policy(root.path(), &access).is_err(),
        "snapshot inherited ordinary Stop dispatch"
    );
    access.snapshot = None;
    access.non_tools.push(plan(HookEvent::Stop, Arc::new(Deny)));
    assert!(
        ToolExecutor::with_policy(root.path(), &access).is_err(),
        "duplicate Stop plans selected ambiguously"
    );
}

async fn durable_turn_case(submit: bool, plugin: bool) {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let mut record = crate::inspection::tests::record(root.path());
    record.phase = Some("worker".into());
    let runtime = SharedRuntime::for_test(&state.path().join("record"), record).unwrap();
    let mut tools = ToolExecutor::new(root.path()).unwrap();
    let count = Arc::new(AtomicUsize::new(0));
    if submit {
        tools
            .register_non_tool_plan(plan(
                HookEvent::UserPromptSubmit,
                Arc::new(AllowCount(count.clone())),
            ))
            .unwrap();
    }
    tools
        .register_non_tool_plan(plan(HookEvent::Stop, Arc::new(AllowCount(count.clone()))))
        .unwrap();
    let mut session = NativeSession::with_tools(
        Box::new(Counting {
            requests: Arc::new(AtomicUsize::new(0)),
            prompts: Arc::new(Mutex::new(vec![])),
        }),
        tools,
    );
    let (_commands, mut command_rx) = mpsc::channel(4);
    let (tx, _rx) = mpsc::channel(256);
    let mut events = EventSink::new("turn-identity".into(), tx, None)
        .unwrap()
        .with_runtime(runtime.clone());
    if plugin {
        events = events.with_plugin_prompt();
    }
    for prompt in ["first actual turn", "second actual turn"] {
        session
            .turn(prompt.into(), &mut command_rx, &events)
            .await
            .unwrap();
    }
    let record = runtime.record().unwrap();
    let rows = serde_json::to_value(&record.operations).unwrap();
    let turns: Vec<_> = rows
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["host_invocation"].get("native_turn").is_some())
        .collect();
    assert_eq!(
        turns.len(),
        2,
        "each actual turn needs a durable origin before optional hooks"
    );
    for turn in &turns {
        assert_eq!(
            turn["host_invocation"]["native_turn"]["origin"],
            if plugin {
                "plugin_context"
            } else {
                "developer"
            }
        );
        assert_eq!(turn["complete"], true);
        let receipts: Vec<_> = rows
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|row| row["host_invocation"].get("lifecycle"))
            .filter(|receipt| receipt["facts"]["native_turn"] == turn["id"])
            .collect();
        assert_eq!(receipts.len(), if submit && !plugin { 2 } else { 1 });
        assert!(
            receipts
                .iter()
                .all(|receipt| receipt["facts"]["operation"].as_u64().unwrap()
                    > turn["id"].as_u64().unwrap())
        );
    }
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
    assert!(
        page.text.contains("Native turn")
            && page.text.contains(if plugin {
                "plugin context"
            } else {
                "developer"
            }),
        "turn origin must be inspectable: {}",
        page.text
    );
}

#[tokio::test]
async fn native_turn_submit_and_stop_share_durable_identity() {
    durable_turn_case(true, false).await;
}
#[tokio::test]
async fn native_turn_stop_only_has_real_identity() {
    durable_turn_case(false, false).await;
}
#[tokio::test]
async fn native_turn_plugin_origin_has_identity_without_developer_submit() {
    durable_turn_case(true, true).await;
}

struct TextResponses(VecDeque<String>);
#[async_trait]
impl Model for TextResponses {
    fn prompt(&mut self, _: String) {}
    fn results(&mut self, _: Vec<ToolResult>) {}
    async fn response(&mut self, events: &EventSink) -> Result<Vec<ToolCall>> {
        if let Some(text) = self.0.pop_front() {
            events.emit(Event::Text { text }).await?;
        }
        Ok(vec![])
    }
}
struct NoisyGate;
#[async_trait]
impl HookRunner for NoisyGate {
    fn side_effect_free(&self) -> bool {
        true
    }
    async fn run(&self, invocation: &HookInvocation) -> Result<RawOutcome> {
        invocation
            .events
            .emit(Event::Text {
                text: "Plugin advisory must not become assistant output".into(),
            })
            .await?;
        Ok(RawOutcome::Callback { value: json!({}) })
    }
}
#[tokio::test]
async fn native_turn_text_resets_and_excludes_plugin_output() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let mut record = crate::inspection::tests::record(root.path());
    record.phase = Some("worker".into());
    let runtime = SharedRuntime::for_test(&state.path().join("record"), record).unwrap();
    let mut tools = ToolExecutor::new(root.path()).unwrap();
    tools
        .register_non_tool_plan(plan(HookEvent::UserPromptSubmit, Arc::new(NoisyGate)))
        .unwrap();
    tools
        .register_non_tool_plan(plan(HookEvent::Stop, Arc::new(NoisyGate)))
        .unwrap();
    let mut session = NativeSession::with_tools(
        Box::new(TextResponses(
            ["first answer".into(), "second answer".into()].into(),
        )),
        tools,
    );
    let (_tx, mut commands) = mpsc::channel(4);
    let (tx, _rx) = mpsc::channel(256);
    let events = EventSink::new("text".into(), tx, None)
        .unwrap()
        .with_runtime(runtime.clone());
    for _ in 0..3 {
        session
            .turn("prompt".into(), &mut commands, &events)
            .await
            .unwrap();
    }
    let record = runtime.record().unwrap();
    let messages: Vec<_> = record
        .operations
        .iter()
        .filter_map(|o| o.non_tool_receipt())
        .filter_map(|r| match &r.facts.subject.occurrence {
            crate::plugins::receipts::NonToolOccurrence::Stop {
                last_assistant_message,
                ..
            } => Some(last_assistant_message.as_deref()),
            _ => None,
        })
        .collect();
    assert_eq!(
        messages,
        [Some("first answer"), Some("second answer"), None]
    );
}

#[tokio::test]
async fn native_turn_large_text_without_stop_hooks_preserves_bare_session_behavior() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let runtime = SharedRuntime::for_test(
        &state.path().join("record"),
        crate::inspection::tests::record(root.path()),
    )
    .unwrap();
    let mut session = NativeSession::new(
        Box::new(TextResponses(["x".repeat(70 * 1024)].into())),
        root.path(),
    )
    .unwrap();
    let (_tx, mut commands) = mpsc::channel(4);
    let (tx, _rx) = mpsc::channel(256);
    let events = EventSink::new("bare".into(), tx, None)
        .unwrap()
        .with_runtime(runtime.clone());
    let outcome = session.turn("prompt".into(), &mut commands, &events).await;
    assert!(outcome.is_ok(), "bare response failed: {:?}", outcome.err());
    assert_eq!(runtime.record().unwrap().operations.len(), 2);
}

#[tokio::test]
async fn native_turn_oversize_stop_text_is_visible_and_never_substituted() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let mut record = crate::inspection::tests::record(root.path());
    record.phase = Some("worker".into());
    let runtime = SharedRuntime::for_test(&state.path().join("record"), record).unwrap();
    let count = Arc::new(AtomicUsize::new(0));
    let mut tools = ToolExecutor::new(root.path()).unwrap();
    tools
        .register_non_tool_plan(plan(HookEvent::Stop, Arc::new(AllowCount(count.clone()))))
        .unwrap();
    let mut session = NativeSession::with_tools(
        Box::new(TextResponses(["x".repeat(70 * 1024)].into())),
        tools,
    );
    let (_tx, mut commands) = mpsc::channel(4);
    let (tx, _rx) = mpsc::channel(256);
    let events = EventSink::new("oversize".into(), tx, None)
        .unwrap()
        .with_runtime(runtime.clone());
    let error = session
        .turn("prompt".into(), &mut commands, &events)
        .await
        .err()
        .unwrap();
    assert!(
        error
            .to_string()
            .contains("assistant text exceeds lifecycle input bound")
    );
    assert_eq!(count.load(Ordering::SeqCst), 0);
    assert!(
        runtime
            .record()
            .unwrap()
            .operations
            .iter()
            .all(|o| o.non_tool_receipt().is_none())
    );
    assert!(
        serde_json::to_string(&runtime.record().unwrap().messages)
            .unwrap()
            .contains(&"x".repeat(70 * 1024)),
        "original model output must survive capture overflow"
    );
}
