use super::*;
use crate::{
    events::EventSink,
    native::{Model, NativeSession},
    plugins::{
        dispatch::HookRunner,
        gate_snapshot::{GateReadSet, GateWorkspace},
        hook_types::{HandlerKind, HookDialect},
        once::{ActivationChange, ActivationSource, HookOrigin, HookReservation, OnceState},
    },
    session::{Session, SessionStart},
    tools::{ToolCall, ToolExecutor, ToolResult},
    workflow::{
        allocation::Limits,
        runtime::{HostInvocation, session_budget::SessionHookAllowance},
    },
};
use serde_json::json;
use std::sync::atomic::AtomicUsize;

struct Fixture {
    root: tempfile::TempDir,
    _state: tempfile::TempDir,
    runtime: SharedRuntime,
    events: EventSink,
    lifetime: u64,
    invocation: Option<HookInvocation>,
    _receiver: tokio::sync::mpsc::Receiver<crate::events::Envelope>,
}
impl Fixture {
    fn new() -> Self {
        Self::new_with_once(true)
    }
    fn new_with_once(once: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("first-store"), "first credential").unwrap();
        std::fs::write(root.path().join("second-store"), "second credential").unwrap();
        let alias = state.path().join("credential");
        std::os::unix::fs::symlink(root.path().join("first-store"), &alias).unwrap();
        let mut record = crate::inspection::tests::record(root.path());
        record.session_hook_allowance = Some(
            SessionHookAllowance::new(Limits {
                seconds: 60,
                model_calls: 0,
                tool_calls: 0,
            })
            .unwrap(),
        );
        let runtime = SharedRuntime::for_test(&state.path().join("record"), record).unwrap();
        let source = ActivationSource::host_namespace("fixture").unwrap();
        let binding = once.then(|| {
            runtime
                .plugin_hook_activation(
                    HookOrigin::Native,
                    Scope::Project,
                    &source,
                    "session",
                    "worker",
                    ActivationChange::ExplicitInvocation,
                )
                .unwrap()
                .unwrap()
        });
        let (sender, receiver) = tokio::sync::mpsc::channel(128);
        let events = EventSink::new("native observer test".into(), sender, None)
            .unwrap()
            .with_runtime(runtime.clone());
        let lifecycle = events
            .begin_host_lifetime(
                SessionStart::Startup,
                vec![
                    (HookEvent::SessionStart, "plan".into()),
                    (HookEvent::SessionEnd, "end-plan".into()),
                ],
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
            runner: HandlerKind::Command,
        };
        let (occurrence_events, facts) = lifecycle.for_non_tool(
            NonToolOccurrence::SessionStart { source:SessionStart::Startup }, "plan".into(),
            vec![json!({"identity":declaration,"source":source,"once":binding,"required_gate":false})]).unwrap();
        let snapshot = Arc::new(
            GateWorkspace::open_with_credentials(root.path(), std::slice::from_ref(&alias))
                .unwrap()
                .capture(&GateReadSet::default(), &AtomicBool::new(false))
                .unwrap(),
        );
        let receipt = HookReceipt {
            required_gate: false,
            observer: None,
            source: Some(source.0),
            once: None,
            invocation: 0,
            declaration: declaration.clone(),
            class: HandlerClass::Observer,
            endpoint: None,
            inspected: AdmissionKey {
                session: facts.session.clone(),
                operation: facts.operation,
                source_operation: lifetime,
                event: "SessionStart".into(),
                tool: None,
                arguments: None,
                lifecycle: Some(facts.subject.clone()),
                plan: "plan".into(),
                role: "native-session".into(),
                workspace: facts.workspace,
                inputs: vec![("snapshot".into(), snapshot.revision().into())],
                external: None,
            },
            outcome: None,
            uncertain_effects: false,
            hold: None,
            questions: vec![],
            pending_proposals: vec![],
        };
        let HookReservation::Run(receipt) = runtime
            .reserve_non_tool_hook(
                facts.operation,
                HookEvent::SessionStart,
                receipt,
                binding.as_ref(),
                None,
            )
            .unwrap()
        else {
            panic!("unexpected skip")
        };
        let invocation = HookInvocation {
            required_gate: false,
            observer: None,
            invocation: receipt.invocation,
            key: receipt.inspected,
            declaration,
            endpoint: None,
            candidate: None,
            lifecycle: Some(facts),
            snapshot,
            completed: None,
            events: occurrence_events,
            host: ToolExecutor::with_policy(
                root.path(),
                &crate::tools::AccessPolicy {
                    credential_paths: vec![alias],
                    ..Default::default()
                },
            )
            .unwrap()
            .hook_host(),
            runner_lease: Arc::new(Arc::new(Semaphore::new(1)).try_acquire_owned().unwrap()),
            mutation_guard: None,
            class: HandlerClass::Observer,
        };
        Self {
            root,
            _state: state,
            runtime,
            events,
            lifetime,
            invocation: Some(invocation),
            _receiver: receiver,
        }
    }
    fn another(&self) -> HookInvocation {
        self.invocation(true)
    }
    fn invocation(&self, reserve: bool) -> HookInvocation {
        let original = self.invocation.as_ref().unwrap();
        let events = original.events.clone();
        let facts = original.lifecycle.clone().unwrap();
        let mut receipt = hooks(&self.runtime.record().unwrap())
            .next()
            .unwrap()
            .clone();
        receipt.inspected.operation = facts.operation;
        receipt.inspected.lifecycle = Some(facts.subject.clone());
        receipt.once = None;
        receipt.observer = None;
        receipt.outcome = None;
        let receipt = if reserve {
            let HookReservation::Run(receipt) = self
                .runtime
                .reserve_non_tool_hook(
                    facts.operation,
                    HookEvent::SessionStart,
                    receipt,
                    None,
                    None,
                )
                .unwrap()
            else {
                panic!("unexpected skip")
            };
            *receipt
        } else {
            receipt
        };
        HookInvocation {
            required_gate: false,
            observer: None,
            invocation: receipt.invocation,
            key: receipt.inspected,
            declaration: original.declaration.clone(),
            endpoint: None,
            candidate: None,
            lifecycle: Some(facts),
            snapshot: original.snapshot.clone(),
            completed: None,
            events,
            host: original.host.clone(),
            runner_lease: Arc::new(Arc::new(Semaphore::new(1)).try_acquire_owned().unwrap()),
            mutation_guard: None,
            class: HandlerClass::Observer,
        }
    }
    fn terminal(&self) -> HookInvocation {
        let original = self.invocation.as_ref().unwrap();
        let source = ActivationSource::host_namespace("fixture").unwrap();
        let lifecycle = self
            .events
            .end_host_lifetime(crate::session::SessionEnd::Shutdown)
            .unwrap()
            .unwrap();
        let (events,facts)=lifecycle.for_non_tool(NonToolOccurrence::SessionEnd {reason:crate::session::SessionEnd::Shutdown},
            "end-plan".into(),vec![json!({"identity":original.declaration,"source":source,"once":null,"required_gate":false})]).unwrap();
        let snapshot = Arc::new(
            GateWorkspace::open_with_credentials(self.root.path(), &original.host.credentials)
                .unwrap()
                .capture(&GateReadSet::default(), &AtomicBool::new(false))
                .unwrap(),
        );
        let mut receipt = hooks(&self.runtime.record().unwrap())
            .next()
            .unwrap()
            .clone();
        receipt.inspected.operation = facts.operation;
        receipt.inspected.lifecycle = Some(facts.subject.clone());
        receipt.inspected.event = "SessionEnd".into();
        receipt.inspected.plan = "end-plan".into();
        receipt.inspected.inputs = vec![("snapshot".into(), snapshot.revision().into())];
        receipt.once = None;
        receipt.observer = None;
        receipt.outcome = None;
        let HookReservation::Run(receipt) = self
            .runtime
            .reserve_non_tool_hook(facts.operation, HookEvent::SessionEnd, receipt, None, None)
            .unwrap()
        else {
            panic!("unexpected skip")
        };
        HookInvocation {
            required_gate: false,
            observer: None,
            invocation: receipt.invocation,
            key: receipt.inspected,
            declaration: original.declaration.clone(),
            endpoint: None,
            candidate: None,
            lifecycle: Some(facts),
            snapshot,
            completed: None,
            events,
            host: original.host.clone(),
            runner_lease: Arc::new(Arc::new(Semaphore::new(1)).try_acquire_owned().unwrap()),
            mutation_guard: None,
            class: HandlerClass::Observer,
        }
    }
    fn hook(&self) -> HookReceipt {
        hooks(&self.runtime.record().unwrap())
            .find(|h| h.observer.is_some())
            .unwrap()
            .clone()
    }
    async fn complete(&mut self) {
        crate::plugins::observer::dispatch(
            self.invocation.take().unwrap(),
            Arc::new(Immediate(Arc::new(AtomicUsize::new(0)))),
        )
        .await
        .unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            while self.hook().outcome.is_none() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }
}
struct Immediate(Arc<AtomicUsize>);
#[async_trait::async_trait]
impl HookRunner for Immediate {
    fn observer_config(&self) -> Option<ObserverConfig> {
        Some(ObserverConfig {
            declared: true,
            rewake: false,
            timeout_ms: 5000,
        })
    }
    async fn run(&self, invocation: &HookInvocation) -> Result<RawOutcome> {
        invocation.observer.as_ref().unwrap().validate()?;
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(RawOutcome::Callback {
            value: json!({"hookSpecificOutput":{"hookEventName":"SessionStart","additionalContext":"retained session observation"}}),
        })
    }
}

struct Fault {
    root: std::path::PathBuf,
    point: &'static str,
    grant: bool,
    reached: Arc<AtomicBool>,
}
thread_local! { static FAULT: std::cell::RefCell<Option<Fault>> = const { std::cell::RefCell::new(None) }; }
struct TransferPublicationPause {
    entered: tokio::sync::oneshot::Sender<()>,
    release: std::sync::mpsc::Receiver<()>,
}
thread_local! { static PUBLICATION_PAUSE: std::cell::RefCell<Option<TransferPublicationPause>> = const { std::cell::RefCell::new(None) }; }
pub(in crate::workflow::runtime) fn hold_next_transfer_publication(
    entered: tokio::sync::oneshot::Sender<()>,
    release: std::sync::mpsc::Receiver<()>,
) {
    PUBLICATION_PAUSE.set(Some(TransferPublicationPause { entered, release }));
}
pub(super) fn pause_transfer_publication() {
    PUBLICATION_PAUSE.with_borrow_mut(|pending| {
        if let Some(pause) = pending.take() {
            let _ = pause.entered.send(());
            pause
                .release
                .recv_timeout(Duration::from_secs(2))
                .expect("transfer publication control was not released");
        }
    });
}
pub(in crate::workflow::runtime) fn invalidate_at_checkpoint(record: &mut Record) {
    FAULT.with_borrow_mut(|pending| {
        let Some(fault) = pending.as_ref() else {
            return;
        };
        if record.workspace != fault.root {
            return;
        }
        let matched = match fault.point {
            "transfer" => hooks(record).any(|h| {
                h.observer
                    .as_ref()
                    .is_some_and(|o| o.status == Status::Running)
            }),
            "reservation" => hooks(record).any(|h| {
                h.observer
                    .as_ref()
                    .is_some_and(|o| o.delivery == Delivery::Reserved)
            }),
            "checkpoint" => record
                .checkpoint
                .as_ref()
                .is_some_and(|v| v.to_string().contains("retained session observation")),
            _ => false,
        };
        if !matched {
            return;
        }
        let fault = pending.take().unwrap();
        fault.reached.store(true, Ordering::Release);
        if fault.grant {
            record
                .session_hook_allowance
                .as_mut()
                .unwrap()
                .allocation
                .deadline_ms = 0;
        } else {
            for operation in &mut record.operations {
                if let Some(HostInvocation::NativeSession(owner)) = &mut operation.host_invocation {
                    owner.authority = None;
                }
            }
        }
    });
}

#[tokio::test]
async fn session_async_transfer_checkpoint_revocation_keeps_unknown_without_launch_or_capacity_leak()
 {
    for grant in [false, true] {
        let mut f = Fixture::new();
        let reached = Arc::new(AtomicBool::new(false));
        FAULT.set(Some(Fault {
            root: f.root.path().into(),
            point: "transfer",
            grant,
            reached: reached.clone(),
        }));
        let effects = Arc::new(AtomicUsize::new(0));
        let result = crate::plugins::observer::dispatch(
            f.invocation.take().unwrap(),
            Arc::new(Immediate(effects.clone())),
        )
        .await;
        FAULT.set(None);
        assert!(reached.load(Ordering::Acquire));
        assert!(result.is_err());
        assert_eq!(effects.load(Ordering::SeqCst), 0);
        let hook = f.hook();
        assert!(hook.uncertain_effects && hook.outcome.is_some());
        assert_eq!(hook.once.unwrap().state, OnceState::Unknown);
        assert_eq!(hook.observer.unwrap().status, Status::Interrupted);
        let runtime = f.runtime.0.lock().unwrap();
        assert!(runtime.record.recovery_pending);
        assert_eq!(runtime.observers.slots.available_permits(), 8);
        assert!(runtime.observers.stopped());
    }
}

struct Counting {
    requests: Arc<AtomicUsize>,
    prompts: Arc<std::sync::Mutex<Vec<String>>>,
}
#[async_trait::async_trait]
impl Model for Counting {
    fn prompt(&mut self, text: String) {
        self.prompts.lock().unwrap().push(text);
    }
    fn results(&mut self, _: Vec<ToolResult>) {
        panic!("unexpected tools")
    }
    fn checkpoint(&self) -> Option<serde_json::Value> {
        Some(json!({"prompts":*self.prompts.lock().unwrap()}))
    }
    async fn response(&mut self, _: &EventSink) -> Result<Vec<ToolCall>> {
        self.requests.fetch_add(1, Ordering::SeqCst);
        Ok(vec![])
    }
}
#[tokio::test]
async fn session_async_delivery_checkpoint_revocation_never_sends_or_replays_reserved_context() {
    for point in ["reservation", "checkpoint"] {
        for grant in [false, true] {
            let mut f = Fixture::new();
            f.complete().await;
            let reached = Arc::new(AtomicBool::new(false));
            FAULT.set(Some(Fault {
                root: f.root.path().into(),
                point,
                grant,
                reached: reached.clone(),
            }));
            let requests = Arc::new(AtomicUsize::new(0));
            let prompts = Arc::new(std::sync::Mutex::new(vec![]));
            let mut native = NativeSession::with_tools(
                Box::new(Counting {
                    requests: requests.clone(),
                    prompts: prompts.clone(),
                }),
                ToolExecutor::new(f.root.path()).unwrap(),
            );
            let (_commands, mut receiver) = tokio::sync::mpsc::channel(4);
            let result = native
                .turn("ordinary request".into(), &mut receiver, &f.events)
                .await;
            FAULT.set(None);
            assert!(
                reached.load(Ordering::Acquire),
                "missed {point} grant={grant}: {:?}",
                result.as_ref().err()
            );
            assert_eq!(result.is_err(), point == "checkpoint");
            assert_eq!(
                requests.load(Ordering::SeqCst),
                usize::from(point == "reservation")
            );
            let hook = f.hook();
            let observer = hook.observer.unwrap();
            assert_eq!(
                observer.delivery,
                if point == "checkpoint" {
                    Delivery::Reserved
                } else {
                    Delivery::Withheld
                }
            );
            assert!(observer.delivery_operation.is_some());
            assert_eq!(
                prompts
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|s| s.contains("retained session observation")),
                point == "checkpoint"
            );
            let mut restored: Record =
                serde_json::from_value(serde_json::to_value(f.runtime.record().unwrap()).unwrap())
                    .unwrap();
            interrupt_restored(&mut restored);
            let retained = hooks(&restored)
                .find(|h| h.observer.is_some())
                .unwrap()
                .observer
                .as_ref()
                .unwrap();
            assert_eq!(retained.delivery, Delivery::Withheld);
            assert_eq!(retained.delivery_operation, observer.delivery_operation);
            assert_eq!(
                restored
                    .checkpoint
                    .as_ref()
                    .unwrap()
                    .to_string()
                    .contains("retained session observation"),
                point == "checkpoint"
            );
            assert_eq!(restored.recovery_pending, point == "checkpoint");
            assert!(restored.task.is_none());
            let grant = restored.session_hook_allowance.unwrap();
            assert_eq!(
                (
                    grant.allocation.model_calls,
                    grant.allocation.tool_calls,
                    grant.backend_invocations
                ),
                (0, 0, 0)
            );
        }
    }
}

#[tokio::test]
async fn session_async_known_delivery_settles_after_original_grant_expires_during_provider_response()
 {
    let mut f = Fixture::new();
    f.complete().await;
    let target = f.runtime.begin_model_owned("worker", None, None).unwrap();
    let identity = f.runtime.record().unwrap().identity;
    let delivery = f
        .runtime
        .reserve_native_observer_context(f.lifetime, target, &identity)
        .unwrap()
        .unwrap();
    f.runtime
        .validate_native_observer_context(&delivery)
        .unwrap();
    f.runtime
        .update(|r| {
            r.session_hook_allowance
                .as_mut()
                .unwrap()
                .allocation
                .deadline_ms = 0;
            Ok(())
        })
        .unwrap();
    assert!(
        f.runtime
            .validate_native_observer_context(&delivery)
            .is_err()
    );
    f.runtime.finish_model(target).unwrap();
    f.runtime
        .complete_native_observer_context(&delivery)
        .unwrap();
    let observer = f.hook().observer.unwrap();
    assert_eq!(observer.delivery, Delivery::Delivered);
    assert_eq!(observer.delivery_operation, Some(target));
    assert!(
        f.runtime
            .reserve_native_observer_context(
                f.lifetime,
                f.runtime.begin_model_owned("worker", None, None).unwrap(),
                &identity
            )
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn session_async_unfunded_or_expired_grant_never_borrows_funded_task_or_starts_effect() {
    for missing in [false, true] {
        let mut f = Fixture::new();
        f.runtime.allocate(Limits::default(), None).unwrap();
        f.runtime
            .update(|record| {
                if missing {
                    record.session_hook_allowance = None;
                } else {
                    record
                        .session_hook_allowance
                        .as_mut()
                        .unwrap()
                        .allocation
                        .deadline_ms = 0;
                }
                Ok(())
            })
            .unwrap();
        let effects = Arc::new(AtomicUsize::new(0));
        assert!(
            crate::plugins::observer::dispatch(
                f.invocation.take().unwrap(),
                Arc::new(Immediate(effects.clone()))
            )
            .await
            .is_err()
        );
        assert_eq!(effects.load(Ordering::SeqCst), 0);
        assert!(hooks(&f.runtime.record().unwrap()).all(|hook| hook.observer.is_none()));
        assert_eq!(
            f.runtime.record().unwrap().allocation.unwrap().model_calls,
            0
        );
    }
}

struct Held {
    release: Arc<Notify>,
    effects: Arc<AtomicUsize>,
    fail: bool,
}
#[async_trait::async_trait]
impl HookRunner for Held {
    fn observer_config(&self) -> Option<ObserverConfig> {
        Some(ObserverConfig {
            declared: true,
            rewake: false,
            timeout_ms: 5000,
        })
    }
    async fn run(&self, invocation: &HookInvocation) -> Result<RawOutcome> {
        self.release.notified().await;
        invocation.observer.as_ref().unwrap().validate()?;
        self.effects.fetch_add(1, Ordering::SeqCst);
        Ok(RawOutcome::Command {
            exit_code: Some(if self.fail { 1 } else { 0 }),
            stdout: b"{}".to_vec(),
            stderr: vec![],
        })
    }
}
#[tokio::test]
async fn session_async_once_waits_for_actual_success_and_unknown_recovery_cannot_replay() {
    for state in ["success", "failed", "interrupted"] {
        let mut f = Fixture::new();
        let release = Arc::new(Notify::new());
        let effects = Arc::new(AtomicUsize::new(0));
        crate::plugins::observer::dispatch(
            f.invocation.take().unwrap(),
            Arc::new(Held {
                release: release.clone(),
                effects: effects.clone(),
                fail: state == "failed",
            }),
        )
        .await
        .unwrap();
        assert_eq!(f.hook().once.unwrap().state, OnceState::Reserved);
        assert_eq!(effects.load(Ordering::SeqCst), 0);
        if state == "interrupted" {
            f.runtime
                .cancel_native_observers(f.lifetime, false)
                .unwrap();
        } else {
            release.notify_one();
        }
        tokio::time::timeout(Duration::from_secs(2), async {
            while f.hook().outcome.is_none() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let hook = f.hook();
        assert_eq!(
            hook.once.unwrap().state,
            match state {
                "success" => OnceState::Succeeded,
                "failed" => OnceState::Failed,
                _ => OnceState::Unknown,
            }
        );
        assert_eq!(
            effects.load(Ordering::SeqCst),
            usize::from(state != "interrupted")
        );
        let mut restored: Record =
            serde_json::from_value(serde_json::to_value(f.runtime.record().unwrap()).unwrap())
                .unwrap();
        interrupt_restored(&mut restored);
        assert_eq!(restored.recovery_pending, state == "interrupted");
        assert_eq!(
            hooks(&restored)
                .find(|hook| hook.observer.is_some())
                .unwrap()
                .observer
                .as_ref()
                .unwrap()
                .delivery,
            Delivery::Withheld
        );
        if state == "interrupted" {
            assert_eq!(f.runtime.unresolved_plugin_once().unwrap().len(), 1);
        }
        tokio::task::yield_now().await;
        assert!(
            f.runtime.0.lock().unwrap().observers.jobs.is_empty(),
            "settled capabilities retained for {state}"
        );
    }
}

#[tokio::test]
async fn session_async_shared_active_limit_and_completed_pending_pruning() {
    let f = Fixture::new_with_once(false);
    let effects = Arc::new(AtomicUsize::new(0));
    let release = Arc::new(Notify::new());
    for index in 0..9 {
        let result = crate::plugins::observer::dispatch(
            f.another(),
            Arc::new(Held {
                release: release.clone(),
                effects: effects.clone(),
                fail: false,
            }),
        )
        .await;
        if index == 8 {
            assert!(format!("{:#}", result.unwrap_err()).contains("capacity exhausted"));
        } else {
            result.unwrap();
        }
    }
    assert_eq!(effects.load(Ordering::SeqCst), 0);
    f.runtime
        .cancel_native_observers(f.lifetime, false)
        .unwrap();
    f.runtime
        .join_native_observers(
            f.lifetime,
            tokio::time::Instant::now() + Duration::from_secs(2),
        )
        .await
        .unwrap();
    assert_eq!(
        f.runtime
            .0
            .lock()
            .unwrap()
            .observers
            .slots
            .available_permits(),
        8
    );
    let f = Fixture::new_with_once(false);
    let effects = Arc::new(AtomicUsize::new(0));
    // Native startup has a stricter 32-handler occurrence bound; the base fixture
    // reserves one. The common 64-pending limit is covered by ordinary observers.
    for _ in 0..31 {
        let result =
            crate::plugins::observer::dispatch(f.another(), Arc::new(Immediate(effects.clone())))
                .await;
        result.unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            while !f.runtime.0.lock().unwrap().observers.stopped() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }
    assert_eq!(effects.load(Ordering::SeqCst), 31);
    f.runtime
        .cancel_native_observers(f.lifetime, false)
        .unwrap();
    assert!(f.runtime.0.lock().unwrap().observers.jobs.is_empty());
}

#[tokio::test]
async fn session_async_exact_admission_and_original_host_view_revoke_before_effects() {
    for change in [
        "key",
        "declaration",
        "budget",
        "host",
        "credentials",
        "source",
        "workspace",
    ] {
        let mut f = Fixture::new();
        let effects = Arc::new(AtomicUsize::new(0));
        let release = Arc::new(Notify::new());
        crate::plugins::observer::dispatch(
            f.invocation.take().unwrap(),
            Arc::new(Held {
                release: release.clone(),
                effects: effects.clone(),
                fail: false,
            }),
        )
        .await
        .unwrap();
        if change == "credentials" {
            let alias = f._state.path().join("credential");
            std::fs::remove_file(&alias).unwrap();
            std::os::unix::fs::symlink(f.root.path().join("second-store"), &alias).unwrap();
        } else if change == "workspace" {
            std::fs::rename(f.root.path(), f._state.path().join("original-workspace")).unwrap();
            std::fs::create_dir(f.root.path()).unwrap();
        } else {
            let other = Fixture::new();
            let operation_id = f.hook().inspected.operation;
            let authority = super::super::plugin_session::lifetime(
                &other.runtime.record().unwrap(),
                other.lifetime,
            )
            .unwrap()
            .1
            .authority
            .clone();
            f.runtime
                .update(|record| {
                    match change {
                        "key" => hooks_mut(record)
                            .next()
                            .unwrap()
                            .inspected
                            .inputs
                            .push(("changed".into(), "view".into())),
                        "declaration" => {
                            hooks_mut(record).next().unwrap().declaration.policy = "changed".into()
                        }
                        "budget" => {
                            record
                                .operations
                                .iter_mut()
                                .find(|o| o.id == operation_id)
                                .unwrap()
                                .budget = Some(super::super::BudgetRef::Unallocated)
                        }
                        "host" => {
                            let operation = record
                                .operations
                                .iter_mut()
                                .find(|o| o.id == f.lifetime)
                                .unwrap();
                            let Some(HostInvocation::NativeSession(owner)) =
                                &mut operation.host_invocation
                            else {
                                unreachable!()
                            };
                            owner.authority = authority;
                        }
                        "source" => hooks_mut(record).next().unwrap().source = None,
                        _ => unreachable!(),
                    }
                    Ok(())
                })
                .unwrap();
        }
        release.notify_one();
        f.runtime
            .join_native_observers(
                f.lifetime,
                tokio::time::Instant::now() + Duration::from_secs(2),
            )
            .await
            .unwrap();
        assert_eq!(effects.load(Ordering::SeqCst), 0, "{change}");
        if matches!(change, "key" | "declaration" | "budget" | "source") {
            // The replacement receipt cannot inherit even an unknown result
            // from the original job. Its still-pending attempt needs recovery.
            assert!(f.hook().outcome.is_none(), "old result applied to {change}");
            assert_eq!(f.hook().once.unwrap().state, OnceState::Reserved);
            assert_eq!(f.hook().observer.unwrap().status, Status::Running);
            let mut restored = f.runtime.record().unwrap();
            interrupt_restored(&mut restored);
            assert_eq!(
                hooks(&restored)
                    .next()
                    .unwrap()
                    .observer
                    .as_ref()
                    .unwrap()
                    .status,
                Status::Interrupted
            );
        } else {
            assert_eq!(f.hook().observer.unwrap().delivery, Delivery::Withheld);
            assert_eq!(f.hook().once.unwrap().state, OnceState::Unknown);
        }
        assert!(f.runtime.record().unwrap().recovery_pending);
    }
}

struct Writer {
    release: Arc<Notify>,
    path: std::path::PathBuf,
}
#[async_trait::async_trait]
impl HookRunner for Writer {
    fn observer_config(&self) -> Option<ObserverConfig> {
        Some(ObserverConfig {
            declared: true,
            rewake: false,
            timeout_ms: 5000,
        })
    }
    fn mutates_workspace(&self) -> bool {
        true
    }
    async fn run(&self, invocation: &HookInvocation) -> Result<RawOutcome> {
        self.release.notified().await;
        invocation.observer.as_ref().unwrap().validate()?;
        std::fs::write(&self.path, "writer finished before snapshot")?;
        Ok(RawOutcome::Command {
            exit_code: Some(0),
            stdout: b"{}".to_vec(),
            stderr: vec![],
        })
    }
}
#[tokio::test]
async fn session_async_workspace_snapshot_waits_for_session_writer_and_keeps_end_admission_separate()
 {
    let f = Fixture::new_with_once(false);
    let release = Arc::new(Notify::new());
    let path = f.root.path().join("observed");
    crate::plugins::observer::dispatch(
        f.invocation(false),
        Arc::new(Writer {
            release: release.clone(),
            path: path.clone(),
        }),
    )
    .await
    .unwrap();
    let quiesce = f.runtime.quiesce_observer_writers("worker");
    tokio::pin!(quiesce);
    tokio::select! {
        result=&mut quiesce=>panic!("snapshot admitted while session writer was live: {result:?}"),
        _=tokio::time::sleep(Duration::from_millis(30))=>{}
    }
    let blocked = f.runtime.admit_observer(
        &f.invocation(false),
        ObserverConfig {
            declared: true,
            rewake: false,
            timeout_ms: 5000,
        },
        true,
    );
    assert!(
        blocked
            .err()
            .unwrap()
            .to_string()
            .contains("writers are quiescing")
    );
    release.notify_one();
    quiesce.await.unwrap();
    f.runtime
        .settle_non_tool(
            f.invocation.as_ref().unwrap().key.operation,
            HookEvent::SessionStart,
            Default::default(),
        )
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "writer finished before snapshot"
    );
    std::fs::remove_file(&path).unwrap();
    release.notify_one();
    crate::plugins::observer::dispatch(
        f.terminal(),
        Arc::new(Writer {
            release,
            path: path.clone(),
        }),
    )
    .await
    .unwrap();
    f.runtime
        .join_native_observers(
            f.lifetime,
            tokio::time::Instant::now() + Duration::from_secs(1),
        )
        .await
        .unwrap();
    assert!(
        path.exists(),
        "completed worker snapshot permanently blocked separately authorized terminal writer"
    );
}

#[tokio::test]
async fn session_async_legacy_receipt_and_late_host_revocation_preserve_original_delivery_target() {
    let mut f = Fixture::new();
    f.complete().await;
    let mut legacy = serde_json::to_value(f.hook()).unwrap();
    legacy["observer"]
        .as_object_mut()
        .unwrap()
        .remove("delivery_operation");
    let restored: HookReceipt = serde_json::from_value(legacy).unwrap();
    assert_eq!(restored.observer.unwrap().delivery_operation, None);
    let target = f.runtime.begin_model_owned("worker", None, None).unwrap();
    let delivery = f
        .runtime
        .reserve_native_observer_context(f.lifetime, target, &f.runtime.record().unwrap().identity)
        .unwrap()
        .unwrap();
    f.runtime
        .validate_native_observer_context(&delivery)
        .unwrap();
    // The provider has the authorized request. Cancellation now withholds future
    // context but cannot replace the result of that independent request.
    f.runtime
        .cancel_native_observers(f.lifetime, false)
        .unwrap();
    f.runtime.finish_model(target).unwrap();
    f.runtime
        .complete_native_observer_context(&delivery)
        .unwrap();
    assert_eq!(f.hook().observer.unwrap().delivery, Delivery::Delivered);
    assert!(f.runtime.0.lock().unwrap().observers.jobs.is_empty());
}

fn admitted(f: &Fixture) -> Arc<ObserverLease> {
    f.runtime
        .admit_observer(
            f.invocation.as_ref().unwrap(),
            ObserverConfig {
                declared: true,
                rewake: false,
                timeout_ms: 5000,
            },
            false,
        )
        .unwrap()
}

fn known_success() -> RawOutcome {
    RawOutcome::Command {
        exit_code: Some(0),
        stdout: b"{}".to_vec(),
        stderr: vec![],
    }
}

#[test]
fn session_async_intended_transfer_cannot_authorize_effect_after_persistence_failure() {
    let f = Fixture::new();
    let lease = admitted(&f);
    let state = f.runtime.directory().unwrap().join("state.json");
    std::fs::write(&state, b"damaged fixture record").unwrap();
    let result = lease.transfer(None);
    assert!(result.is_err());
    assert!(f.runtime.0.lock().unwrap().failed);
    assert!(
        lease.validate().is_err(),
        "local transfer intent bypassed failed persistence"
    );
    assert!(!lease.transferred.load(Ordering::Acquire));
    assert_eq!(std::fs::read(&state).unwrap(), b"damaged fixture record");
    lease.revoke();
    drop(lease);
    assert_eq!(
        f.runtime
            .0
            .lock()
            .unwrap()
            .observers
            .slots
            .available_permits(),
        8
    );
}

#[tokio::test]
async fn session_async_review_known_delivery_cannot_settle_replacement_at_same_target() {
    let mut f = Fixture::new();
    f.complete().await;
    let target = f.runtime.begin_model_owned("worker", None, None).unwrap();
    let delivery = f
        .runtime
        .reserve_native_observer_context(f.lifetime, target, &f.runtime.record().unwrap().identity)
        .unwrap()
        .unwrap();
    f.runtime
        .validate_native_observer_context(&delivery)
        .unwrap();
    f.runtime
        .update(|record| {
            hooks_mut(record)
                .next()
                .unwrap()
                .once
                .as_mut()
                .unwrap()
                .activation
                .epoch += 1;
            Ok(())
        })
        .unwrap();
    f.runtime.finish_model(target).unwrap();
    let result = f.runtime.complete_native_observer_context(&delivery);
    let hook = f.hook();
    assert!(
        result.is_err(),
        "known delivery settled a replacement with the same target"
    );
    assert!(f.runtime.record().unwrap().recovery_pending);
    assert_eq!(
        hook.observer.as_ref().unwrap().delivery_operation,
        Some(target)
    );
    assert_eq!(hook.observer.as_ref().unwrap().delivery, Delivery::Reserved);
    let mut restored = f.runtime.record().unwrap();
    interrupt_restored(&mut restored);
    assert_eq!(
        hooks(&restored)
            .next()
            .unwrap()
            .observer
            .as_ref()
            .unwrap()
            .delivery,
        Delivery::Withheld
    );
}

#[test]
fn session_async_review_first_transfer_requires_live_unsettled_occurrence() {
    let mut forbidden = vec![];
    for change in ["expired", "closed", "settled"] {
        for transferred in [false, true] {
            let f = Fixture::new();
            let lease = admitted(&f);
            if transferred {
                lease.transfer(None).unwrap();
            }
            f.runtime
                .update(|record| {
                    if change == "settled" {
                        let operation = record
                            .operations
                            .iter_mut()
                            .find(|o| o.id == lease.key.0)
                            .unwrap();
                        operation.complete = true;
                        let Some(HostInvocation::Lifecycle(receipt)) =
                            &mut operation.host_invocation
                        else {
                            unreachable!()
                        };
                        receipt.settled = true;
                    } else {
                        let operation = record
                            .operations
                            .iter_mut()
                            .find(|o| o.id == f.lifetime)
                            .unwrap();
                        let Some(HostInvocation::NativeSession(owner)) =
                            &mut operation.host_invocation
                        else {
                            unreachable!()
                        };
                        owner.deadline = (change == "expired").then(Instant::now);
                    }
                    Ok(())
                })
                .unwrap();
            let result = if transferred {
                lease.validate().map(|_| ())
            } else {
                lease.transfer(None)
            };
            if result.is_ok() != transferred {
                forbidden.push((change, transferred));
            }
            if lease.transferred.load(Ordering::Acquire) {
                lease.abandoned();
            }
            lease.revoke();
            drop(lease);
            assert_eq!(
                f.runtime
                    .0
                    .lock()
                    .unwrap()
                    .observers
                    .slots
                    .available_permits(),
                8
            );
        }
    }
    assert!(
        forbidden.is_empty(),
        "wrong first-transfer/owned-continuation decisions: {forbidden:?}"
    );
}

#[test]
fn session_async_review_effects_require_exact_durable_running_transfer() {
    let mut forbidden = vec![];
    for change in [
        "missing",
        "owner",
        "task",
        "allocation",
        "deadline",
        "status",
        "marker",
        "target",
        "withheld",
    ] {
        let f = Fixture::new();
        let lease = admitted(&f);
        lease.transfer(None).unwrap();
        f.runtime
            .update(|record| {
                let hook = hooks_mut(record).next().unwrap();
                if change == "missing" {
                    hook.observer = None;
                    return Ok(());
                }
                let observer = hook.observer.as_mut().unwrap();
                match change {
                    "owner" => observer.owner.push_str("changed"),
                    "task" => observer.task = Some(77),
                    "allocation" => observer.allocation_started_ms += 1,
                    "deadline" => observer.deadline_ms += 1,
                    "status" => observer.status = Status::Completed,
                    "marker" => observer.launch_marker = Some(json!({"async": true})),
                    "target" => observer.delivery_operation = Some(77),
                    "withheld" => observer.delivery = Delivery::Withheld,
                    _ => unreachable!(),
                }
                Ok(())
            })
            .unwrap();
        let effect_admitted = lease.validate().is_ok();
        if effect_admitted != (change == "withheld") {
            forbidden.push(change);
        }
        // No process was spawned; exercise the production abandoned-owner path
        // before reporting a failure, including a removed/replaced receipt.
        lease.revoke();
        lease.abandoned();
        drop(lease);
        assert_eq!(
            f.runtime
                .0
                .lock()
                .unwrap()
                .observers
                .slots
                .available_permits(),
            8
        );
    }
    assert!(
        forbidden.is_empty(),
        "effects escaped exact transfer checks: {forbidden:?}"
    );
}

#[test]
fn session_async_review_cached_success_cannot_consume_replaced_attempt() {
    let mut forbidden = vec![];
    for change in [
        "activation",
        "declaration",
        "admission",
        "expired",
        "cancelled",
    ] {
        let f = Fixture::new();
        let lease = admitted(&f);
        lease.transfer(None).unwrap();
        lease.validate().unwrap();
        let outcome = known_success(); // Known under the original valid admission.
        f.runtime
            .update(|record| {
                match change {
                    "activation" => {
                        hooks_mut(record)
                            .next()
                            .unwrap()
                            .once
                            .as_mut()
                            .unwrap()
                            .activation
                            .epoch += 1
                    }
                    "declaration" => hooks_mut(record)
                        .next()
                        .unwrap()
                        .declaration
                        .policy
                        .push_str("changed"),
                    "admission" => hooks_mut(record)
                        .next()
                        .unwrap()
                        .inspected
                        .inputs
                        .push(("changed".into(), "view".into())),
                    "expired" => {
                        record
                            .session_hook_allowance
                            .as_mut()
                            .unwrap()
                            .allocation
                            .deadline_ms = 0
                    }
                    "cancelled" => {}
                    _ => unreachable!(),
                }
                Ok(())
            })
            .unwrap();
        if change == "cancelled" {
            f.runtime
                .cancel_native_observers(f.lifetime, false)
                .unwrap();
        }
        let result = lease.settle(outcome);
        if result.is_err() {
            lease.abandoned();
        }
        lease.revoke();
        drop(lease);
        let hook = f.hook();
        let replaced = matches!(change, "activation" | "declaration" | "admission");
        if replaced {
            if result.is_ok()
                || hook.outcome.is_some()
                || hook.once.as_ref().unwrap().state != OnceState::Reserved
                || !f.runtime.record().unwrap().recovery_pending
            {
                forbidden.push(change);
            }
        } else {
            assert!(
                result.is_ok(),
                "known original success was lost after {change}"
            );
            assert_eq!(hook.once.unwrap().state, OnceState::Succeeded);
            assert_eq!(hook.observer.unwrap().delivery, Delivery::Withheld);
        }
    }
    assert!(
        forbidden.is_empty(),
        "old success settled a replaced attempt: {forbidden:?}"
    );
}
