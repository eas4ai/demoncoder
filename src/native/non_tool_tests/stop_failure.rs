use super::*;

struct ProviderFailure;
#[derive(Debug)]
struct OriginalProviderError;
impl std::fmt::Display for OriginalProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("original provider failure")
    }
}
impl std::error::Error for OriginalProviderError {}
#[async_trait]
impl Model for ProviderFailure {
    fn checkpoint(&self) -> Option<serde_json::Value> {
        Some(json!({"fixture":"failed-native"}))
    }
    fn prompt(&mut self, _: String) {}
    fn results(&mut self, _: Vec<ToolResult>) {
        panic!("failed provider cannot produce tools")
    }
    async fn response(&mut self, events: &EventSink) -> Result<Vec<ToolCall>> {
        events
            .emit(Event::Text {
                text: "partial answer".into(),
            })
            .await?;
        Err(provider_response_failure(anyhow::Error::new(
            OriginalProviderError,
        )))
    }
}

struct OwnedFailureObserver {
    runtime: SharedRuntime,
    calls: Arc<AtomicUsize>,
}
#[async_trait]
impl HookRunner for OwnedFailureObserver {
    fn side_effect_free(&self) -> bool {
        true
    }
    async fn run(&self, invocation: &HookInvocation) -> Result<RawOutcome> {
        let record = self.runtime.record()?;
        assert_eq!(record.phase.as_deref(), Some("worker"));
        assert!(!record.task.as_ref().unwrap().stopped);
        let facts = invocation.lifecycle.as_ref().unwrap();
        assert_eq!(facts.task, Some(record.task.as_ref().unwrap().id));
        let turn = record
            .operations
            .iter()
            .find(|o| Some(o.id) == facts.native_turn)
            .unwrap();
        assert!(!turn.complete);
        assert!(
            record
                .operations
                .iter()
                .filter(|o| matches!(
                    o.host_invocation,
                    Some(crate::workflow::runtime::HostInvocation::Model)
                ))
                .all(|o| o.complete)
        );
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(RawOutcome::Callback { value: json!({}) })
    }
}
#[tokio::test]
async fn native_provider_failure_observation_precedes_workflow_task_stop() {
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
    let calls = Arc::new(AtomicUsize::new(0));
    let mut tools = ToolExecutor::new(root.path()).unwrap();
    tools
        .register_non_tool_plan(failure_plan(Arc::new(OwnedFailureObserver {
            runtime: runtime.clone(),
            calls: calls.clone(),
        })))
        .unwrap();
    let native = NativeSession::with_tools(Box::new(ProviderFailure), tools);
    let mut session = WorkflowSession::new(
        Box::new(native),
        connection,
        root.path().into(),
        Settings::default(),
        runtime.clone(),
        false,
    )
    .unwrap();
    let (_sender, mut commands) = mpsc::channel(4);
    let (tx, _rx) = mpsc::channel(256);
    let events = EventSink::new("workflow-failure".into(), tx, None)
        .unwrap()
        .with_runtime(runtime.clone());
    assert!(
        session
            .turn("original objective".into(), &mut commands, &events)
            .await
            .is_err()
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let record = runtime.record().unwrap();
    assert_eq!(record.phase, None);
    let task = record.task.unwrap();
    assert!(task.stopped && task.accepted.is_none());
    assert_eq!(task.objective, "original objective");
    assert_eq!(task.corrections, 0);
    assert_eq!(record.allocation.unwrap().model_calls, 1);
}

fn failure_plan(runner: Arc<dyn HookRunner>) -> Arc<NonToolPlan> {
    let base = plan(HookEvent::Stop, runner.clone());
    let mut declaration = base.plan.handlers[0].registration.declaration.clone();
    declaration.required_gate = false;
    declaration.class = HandlerClass::Observer;
    Arc::new(
        NonToolPlan::new(
            HookEvent::StopFailure,
            vec![Registration {
                declaration,
                runner,
                revalidation: None,
            }],
        )
        .unwrap(),
    )
}

#[tokio::test]
async fn native_provider_failure_cancellation_keeps_error_and_never_corrects() {
    for shutdown in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let mut record = crate::inspection::tests::record(root.path());
        record.phase = Some("worker".into());
        let runtime = SharedRuntime::for_test(&state.path().join("record"), record).unwrap();
        let ready = Arc::new(tokio::sync::Notify::new());
        let mut tools = ToolExecutor::new(root.path()).unwrap();
        tools
            .register_non_tool_plan(failure_plan(Arc::new(PendingGate(ready.clone()))))
            .unwrap();
        let mut session = NativeSession::with_tools(Box::new(ProviderFailure), tools);
        let (sender, mut commands) = mpsc::channel(4);
        let (tx, _rx) = mpsc::channel(256);
        let events = EventSink::new("failed-cancel".into(), tx, None)
            .unwrap()
            .with_runtime(runtime.clone());
        let cancel = async {
            ready.notified().await;
            sender
                .send(if shutdown {
                    Command::Shutdown
                } else {
                    Command::Cancel
                })
                .await
                .unwrap();
        };
        let (outcome, ()) = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            tokio::join!(session.turn("fail".into(), &mut commands, &events), cancel)
        })
        .await
        .unwrap();
        assert_eq!(
            outcome.err().unwrap().to_string(),
            "original provider failure"
        );
        if shutdown {
            assert!(commands.recv().await.is_none());
        }
        let record = runtime.record().unwrap();
        let receipt = record
            .operations
            .iter()
            .find_map(|o| o.non_tool_receipt())
            .unwrap();
        assert!(receipt.hooks[0].outcome.is_none());
        assert!(!receipt.correction_required && !receipt.correction_admitted);
        assert!(
            serde_json::to_string(&record)
                .unwrap()
                .contains("original provider failure retained")
        );
        let restored: crate::workflow::runtime::Record =
            serde_json::from_slice(&serde_json::to_vec(&record).unwrap()).unwrap();
        let page = crate::inspection::project(
            &restored,
            Some(crate::inspection::Request {
                target: crate::inspection::Target::Overview,
                page: 0,
                generation: 0,
            }),
        )
        .page
        .unwrap();
        assert!(
            page.text.contains("Turn diagnostic:")
                && page.text.contains("original provider failure retained"),
            "{}",
            page.text
        );
    }
}

struct ExhaustedFailure {
    runtime: SharedRuntime,
    held: bool,
}
#[async_trait]
impl Model for ExhaustedFailure {
    fn prompt(&mut self, _: String) {}
    fn results(&mut self, _: Vec<ToolResult>) {
        panic!("no tools")
    }
    async fn response(&mut self, _: &EventSink) -> Result<Vec<ToolCall>> {
        self.runtime.update(|r| {
            if self.held {
                r.recovery_pending = true;
            } else {
                r.allocation.as_mut().unwrap().deadline_ms = 1;
            }
            Ok(())
        })?;
        return Err(provider_response_failure(anyhow::anyhow!(
            "original exhausted provider failure"
        )));
    }
}

#[tokio::test]
async fn native_provider_failure_exhausted_or_held_observation_retains_diagnostic_without_allowance()
 {
    for held in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let mut record = crate::inspection::tests::record(root.path());
        record.phase = Some("worker".into());
        let runtime = SharedRuntime::for_test(&state.path().join("record"), record).unwrap();
        runtime
            .allocate(crate::workflow::allocation::Limits::default(), None)
            .unwrap();
        let before = runtime.record().unwrap().allocation.unwrap();
        let count = Arc::new(AtomicUsize::new(0));
        let mut tools = ToolExecutor::new(root.path()).unwrap();
        tools
            .register_non_tool_plan(failure_plan(Arc::new(AllowCount(count.clone()))))
            .unwrap();
        let mut session = NativeSession::with_tools(
            Box::new(ExhaustedFailure {
                runtime: runtime.clone(),
                held,
            }),
            tools,
        );
        let (_sender, mut commands) = mpsc::channel(4);
        let (tx, _rx) = mpsc::channel(256);
        let events = EventSink::new("exhausted-failure".into(), tx, None)
            .unwrap()
            .with_runtime(runtime.clone());
        assert_eq!(
            session
                .turn("fail".into(), &mut commands, &events)
                .await
                .err()
                .unwrap()
                .to_string(),
            "original exhausted provider failure"
        );
        assert_eq!(count.load(Ordering::SeqCst), 0);
        let record = runtime.record().unwrap();
        let restored: crate::workflow::runtime::Record =
            serde_json::from_slice(&serde_json::to_vec(&record).unwrap()).unwrap();
        let text = serde_json::to_string(&restored).unwrap();
        assert!(text.contains("StopFailure observation or cleanup incomplete"));
        let page = crate::inspection::project(
            &restored,
            Some(crate::inspection::Request {
                target: crate::inspection::Target::Overview,
                page: 0,
                generation: 0,
            }),
        )
        .page
        .unwrap();
        assert!(page.text.contains("Turn diagnostic:"), "{}", page.text);
        assert!(
            record
                .operations
                .iter()
                .flat_map(|o| o.all_plugin_hooks())
                .next()
                .is_none()
        );
        let after = record.allocation.unwrap();
        assert_eq!(after.started_ms, before.started_ms);
        assert_eq!(after.model_calls, 1);
        assert_eq!(after.deadline_ms, if held { before.deadline_ms } else { 1 });
    }
}

#[tokio::test]
async fn native_provider_failure_category_matcher_uses_only_recorded_category() {
    for (pattern, expected) in [
        ("^unknown$", 1),
        ("known", 1),
        ("^original provider failure$", 0),
        ("^rate_limit$", 0),
        ("^(u+)+$", 0),
    ] {
        let root = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let mut record = crate::inspection::tests::record(root.path());
        record.phase = Some("worker".into());
        let runtime = SharedRuntime::for_test(&state.path().join("record"), record).unwrap();
        let count = Arc::new(AtomicUsize::new(0));
        let runner = Arc::new(AllowCount(count.clone()));
        let base = failure_plan(runner.clone());
        let mut d = base.plan.handlers[0].registration.declaration.clone();
        d.matcher.error_category = Some(pattern.into());
        let mut tools = ToolExecutor::new(root.path()).unwrap();
        tools
            .register_non_tool_plan(Arc::new(
                NonToolPlan::new(
                    HookEvent::StopFailure,
                    vec![Registration {
                        declaration: d,
                        runner,
                        revalidation: None,
                    }],
                )
                .unwrap(),
            ))
            .unwrap();
        let mut session = NativeSession::with_tools(Box::new(ProviderFailure), tools);
        let (_sender, mut commands) = mpsc::channel(4);
        let (tx, _rx) = mpsc::channel(256);
        let events = EventSink::new("matcher".into(), tx, None)
            .unwrap()
            .with_runtime(runtime);
        assert!(
            session
                .turn("fail".into(), &mut commands, &events)
                .await
                .is_err()
        );
        assert_eq!(count.load(Ordering::SeqCst), expected, "{pattern}");
    }
}

#[test]
fn native_stop_failure_matcher_rejects_bad_patterns_and_preserves_legacy_bytes() {
    assert_eq!(
        serde_json::to_string(&Matcher::default()).unwrap(),
        r#"{"tool":null,"path":null}"#
    );
    for pattern in ["[".to_owned(), "x".repeat(257)] {
        let runner = Arc::new(AllowCount(Arc::new(AtomicUsize::new(0))));
        let base = failure_plan(runner.clone());
        let mut d = base.plan.handlers[0].registration.declaration.clone();
        d.matcher.error_category = Some(pattern);
        assert!(
            NonToolPlan::new(
                HookEvent::StopFailure,
                vec![Registration {
                    declaration: d,
                    runner,
                    revalidation: None
                }]
            )
            .is_err()
        );
    }
    let legacy = json!({"version":1,"owner_phase":"worker","origin":"developer","task":null,"workspace":[1,2],"child_owner":null,"end":"failed"});
    let turn: crate::plugins::receipts::NativeTurn =
        serde_json::from_value(legacy.clone()).unwrap();
    assert!(turn.diagnostics.is_empty());
    assert_eq!(serde_json::to_value(turn).unwrap(), legacy);
}

type SubmitReply = tokio::sync::oneshot::Sender<std::result::Result<(), &'static str>>;
struct ShutdownFailure {
    requests: Arc<AtomicUsize>,
    queued: Option<(mpsc::Sender<Command>, SubmitReply)>,
}
#[async_trait]
impl Model for ShutdownFailure {
    fn prompt(&mut self, _: String) {}
    fn results(&mut self, _: Vec<ToolResult>) {
        panic!("no tools")
    }
    async fn response(&mut self, _: &EventSink) -> Result<Vec<ToolCall>> {
        self.requests.fetch_add(1, Ordering::SeqCst);
        if let Some((sender, reply)) = self.queued.take() {
            sender.try_send(Command::Shutdown).unwrap();
            sender
                .try_send(Command::Submit {
                    text: "must not run".into(),
                    reply,
                })
                .unwrap();
        }
        return Err(provider_response_failure(anyhow::anyhow!(
            "original shutdown provider failure"
        )));
    }
}
struct ClosingNative {
    native: NativeSession,
    closed: Arc<AtomicUsize>,
}
#[async_trait]
impl Session for ClosingNative {
    fn owner(&self) -> &'static str {
        "demoncoder"
    }
    async fn turn(
        &mut self,
        prompt: String,
        commands: &mut mpsc::Receiver<Command>,
        events: &EventSink,
    ) -> Result<TurnEnd> {
        self.native.turn(prompt, commands, events).await
    }
    async fn close(&mut self) -> Result<()> {
        self.closed.fetch_add(1, Ordering::SeqCst);
        self.native.close().await
    }
}
#[tokio::test]
async fn native_stop_failure_shutdown_reaches_outer_close_and_rejects_queued_work() {
    for queued in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let mut record = crate::inspection::tests::record(root.path());
        record.phase = Some("worker".into());
        let runtime = SharedRuntime::for_test(&state.path().join("record"), record).unwrap();
        let ready = Arc::new(tokio::sync::Notify::new());
        let mut tools = ToolExecutor::new(root.path()).unwrap();
        tools
            .register_non_tool_plan(failure_plan(Arc::new(PendingGate(ready.clone()))))
            .unwrap();
        let (sender, commands) = mpsc::channel(8);
        let (reply, rejected) = tokio::sync::oneshot::channel();
        let mut reply = Some(reply);
        let requests = Arc::new(AtomicUsize::new(0));
        let closed = Arc::new(AtomicUsize::new(0));
        let model = ShutdownFailure {
            requests: requests.clone(),
            queued: queued.then(|| (sender.clone(), reply.take().unwrap())),
        };
        let native = NativeSession::with_tools(Box::new(model), tools);
        let (tx, mut rx) = mpsc::channel(256);
        let events = EventSink::new("shutdown-failure".into(), tx, None)
            .unwrap()
            .with_runtime(runtime.clone());
        sender.try_send(Command::Prompt("fail".into())).unwrap();
        let control = async {
            if !queued {
                ready.notified().await;
                sender.try_send(Command::Shutdown).unwrap();
                sender
                    .try_send(Command::Submit {
                        text: "must not run".into(),
                        reply: reply.take().unwrap(),
                    })
                    .unwrap();
            }
        };
        let (result, ()) = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            tokio::join!(
                crate::session::run(
                    Box::new(ClosingNative {
                        native,
                        closed: closed.clone()
                    }),
                    commands,
                    events
                ),
                control
            )
        })
        .await
        .unwrap();
        result.unwrap();
        assert!(rejected.await.unwrap().is_err());
        assert_eq!(closed.load(Ordering::SeqCst), 1);
        assert_eq!(requests.load(Ordering::SeqCst), 1);
        let mut failure = false;
        while let Ok(envelope) = rx.try_recv() {
            if let Event::Error { message } = envelope.event {
                failure |= message == "original shutdown provider failure";
            }
        }
        assert!(failure, "outer session lost original error");
        let record = runtime.record().unwrap();
        assert!(
            serde_json::to_string(&record)
                .unwrap()
                .contains("stopped for shutdown")
        );
    }
}

struct SuccessThenFailure(bool);
#[async_trait]
impl Model for SuccessThenFailure {
    fn prompt(&mut self, _: String) {}
    fn results(&mut self, _: Vec<ToolResult>) {}
    async fn response(&mut self, events: &EventSink) -> Result<Vec<ToolCall>> {
        if self.0 {
            return Err(provider_response_failure(anyhow::anyhow!(
                "second turn failed without text"
            )));
        }
        self.0 = true;
        events
            .emit(Event::Text {
                text: "previous successful answer".into(),
            })
            .await?;
        Ok(vec![])
    }
}
#[tokio::test]
async fn native_stop_failure_never_borrows_previous_assistant_text() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let mut record = crate::inspection::tests::record(root.path());
    record.phase = Some("worker".into());
    let runtime = SharedRuntime::for_test(&state.path().join("record"), record).unwrap();
    let count = Arc::new(AtomicUsize::new(0));
    let mut tools = ToolExecutor::new(root.path()).unwrap();
    tools
        .register_non_tool_plan(failure_plan(Arc::new(AllowCount(count.clone()))))
        .unwrap();
    let mut session = NativeSession::with_tools(Box::new(SuccessThenFailure(false)), tools);
    let (_sender, mut commands) = mpsc::channel(4);
    let (tx, _rx) = mpsc::channel(256);
    let events = EventSink::new("two-turns".into(), tx, None)
        .unwrap()
        .with_runtime(runtime.clone());
    assert!(matches!(
        session.turn("succeed".into(), &mut commands, &events).await,
        Ok(TurnEnd::Complete)
    ));
    assert_eq!(count.load(Ordering::SeqCst), 0);
    assert!(
        session
            .turn("fail".into(), &mut commands, &events)
            .await
            .is_err()
    );
    assert_eq!(count.load(Ordering::SeqCst), 1);
    let record = runtime.record().unwrap();
    let receipt = record
        .operations
        .iter()
        .find_map(|o| o.non_tool_receipt())
        .unwrap();
    assert!(matches!(
        receipt.facts.subject.occurrence,
        crate::plugins::receipts::NonToolOccurrence::StopFailure {
            last_assistant_message: None,
            ..
        }
    ));
}

#[tokio::test]
async fn native_stop_failure_without_handlers_keeps_bare_failed_turn() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let record = crate::inspection::tests::record(root.path());
    let runtime = SharedRuntime::for_test(&state.path().join("record"), record).unwrap();
    let mut session = NativeSession::new(Box::new(ProviderFailure), root.path()).unwrap();
    let (_sender, mut commands) = mpsc::channel(4);
    let (tx, _rx) = mpsc::channel(256);
    let events = EventSink::new("bare-failure".into(), tx, None)
        .unwrap()
        .with_runtime(runtime.clone());
    assert!(
        session
            .turn("fail".into(), &mut commands, &events)
            .await
            .err()
            .unwrap()
            .downcast_ref::<OriginalProviderError>()
            .is_some()
    );
    let record = runtime.record().unwrap();
    assert_eq!(record.operations.len(), 2);
    assert!(record.allocation.is_none());
    assert!(
        record
            .operations
            .iter()
            .all(|o| o.non_tool_receipt().is_none())
    );
    let rows = serde_json::to_value(&record.operations).unwrap();
    assert_eq!(rows[0]["host_invocation"]["native_turn"]["end"], "failed");
}

#[tokio::test]
async fn native_stop_failure_async_receipt_cannot_deliver_or_rewake_later() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let mut record = crate::inspection::tests::record(root.path());
    record.phase = Some("worker".into());
    let runtime = SharedRuntime::for_test(&state.path().join("record"), record).unwrap();
    runtime
        .allocate(crate::workflow::allocation::Limits::default(), None)
        .unwrap();
    let mut tools = ToolExecutor::new(root.path()).unwrap();
    tools
        .register_non_tool_plan(failure_plan(Arc::new(AsyncGate(Arc::new(
            tokio::sync::Notify::new(),
        )))))
        .unwrap();
    let mut session = NativeSession::with_tools(Box::new(ProviderFailure), tools);
    let (_sender, mut commands) = mpsc::channel(4);
    let (tx, _rx) = mpsc::channel(256);
    let events = EventSink::new("async-failure".into(), tx, None)
        .unwrap()
        .with_runtime(runtime.clone());
    assert!(
        session
            .turn("fail".into(), &mut commands, &events)
            .await
            .err()
            .unwrap()
            .downcast_ref::<OriginalProviderError>()
            .is_some()
    );
    let record = runtime.record().unwrap();
    let receipt = record
        .operations
        .iter()
        .find_map(|o| o.non_tool_receipt())
        .unwrap();
    let observer = receipt.hooks[0].observer.as_ref().unwrap();
    assert_eq!(
        observer.delivery,
        crate::plugins::observer::Delivery::Withheld
    );
    assert!(!observer.rewake);
    assert!(
        runtime
            .reserve_observer_context("worker", None, false)
            .unwrap()
            .is_none()
    );
    assert!(!runtime.has_observer_rewake("worker").unwrap());
}

#[tokio::test]
async fn native_stop_failure_once_success_is_not_replayed_on_a_later_failure() {
    use crate::plugins::once::{ActivationChange, ActivationSource, HookOrigin};
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let mut record = crate::inspection::tests::record(root.path());
    record.phase = Some("worker".into());
    let runtime = SharedRuntime::for_test(&state.path().join("record"), record).unwrap();
    let count = Arc::new(AtomicUsize::new(0));
    let runner = Arc::new(AllowCount(count.clone()));
    let base = failure_plan(runner.clone());
    let mut d = base.plan.handlers[0].registration.declaration.clone();
    let binding = runtime
        .plugin_hook_activation(
            HookOrigin::Native,
            Scope::Project,
            &ActivationSource::host_namespace("native-lifecycle").unwrap(),
            "failure-check",
            "worker",
            ActivationChange::ExplicitInvocation,
        )
        .unwrap()
        .unwrap();
    d.source = Some(binding.source());
    d.once = Some(binding);
    let mut tools = ToolExecutor::new(root.path()).unwrap();
    tools
        .register_non_tool_plan(Arc::new(
            NonToolPlan::new(
                HookEvent::StopFailure,
                vec![Registration {
                    declaration: d,
                    runner,
                    revalidation: None,
                }],
            )
            .unwrap(),
        ))
        .unwrap();
    let mut session = NativeSession::with_tools(Box::new(ProviderFailure), tools);
    let (_sender, mut commands) = mpsc::channel(4);
    let (tx, _rx) = mpsc::channel(256);
    let events = EventSink::new("once-failure".into(), tx, None)
        .unwrap()
        .with_runtime(runtime.clone());
    for _ in 0..2 {
        assert!(
            session
                .turn("fail".into(), &mut commands, &events)
                .await
                .is_err()
        );
    }
    assert_eq!(count.load(Ordering::SeqCst), 1);
    let record = runtime.record().unwrap();
    let receipts: Vec<_> = record
        .operations
        .iter()
        .filter_map(|o| o.non_tool_receipt())
        .collect();
    assert_eq!(receipts.len(), 2);
    assert_eq!(receipts[1].once_skips.len(), 1);
    assert_eq!(
        receipts[1].once_skips[0].consumed.operation,
        receipts[0].facts.operation
    );
    assert_ne!(receipts[0].facts.native_turn, receipts[1].facts.native_turn);
}

struct ToolPresentationFailure;
impl crate::tools::ToolHook for ToolPresentationFailure {
    fn before(&self, _: &mut ToolCall) -> Result<()> {
        Ok(())
    }
    fn present(&self, _: &ToolResult) -> Result<String> {
        anyhow::bail!("actual tool presentation error")
    }
}
struct ToolResponse;
#[async_trait]
impl Model for ToolResponse {
    fn prompt(&mut self, _: String) {}
    fn results(&mut self, _: Vec<ToolResult>) {}
    async fn response(&mut self, _: &EventSink) -> Result<Vec<ToolCall>> {
        Ok(vec![ToolCall {
            id: "write".into(),
            name: "write".into(),
            arguments: json!({"path":"actual","content":"written"}),
        }])
    }
}
#[tokio::test]
async fn native_stop_failure_does_not_misclassify_admission_or_tool_errors() {
    for admission in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let mut record = crate::inspection::tests::record(root.path());
        record.phase = Some("worker".into());
        let runtime = SharedRuntime::for_test(&state.path().join("record"), record).unwrap();
        if admission {
            runtime
                .allocate(
                    crate::workflow::allocation::Limits {
                        model_calls: 1,
                        ..Default::default()
                    },
                    None,
                )
                .unwrap();
            runtime
                .update(|r| {
                    r.allocation.as_mut().unwrap().model_calls = 1;
                    Ok(())
                })
                .unwrap();
        }
        let count = Arc::new(AtomicUsize::new(0));
        let mut tools = ToolExecutor::new(root.path()).unwrap();
        tools
            .register_non_tool_plan(failure_plan(Arc::new(AllowCount(count.clone()))))
            .unwrap();
        tools.add_hook(Box::new(ToolPresentationFailure));
        let mut session = NativeSession::with_tools(Box::new(ToolResponse), tools);
        let (_sender, mut commands) = mpsc::channel(4);
        let (tx, _rx) = mpsc::channel(256);
        let events = EventSink::new("non-provider-error".into(), tx, None)
            .unwrap()
            .with_runtime(runtime.clone());
        let error = session
            .turn("fail".into(), &mut commands, &events)
            .await
            .err()
            .unwrap();
        if !admission {
            assert_eq!(error.to_string(), "actual tool presentation error");
            assert!(root.path().join("actual").exists());
        }
        assert_eq!(count.load(Ordering::SeqCst), 0);
        assert!(
            runtime
                .record()
                .unwrap()
                .operations
                .iter()
                .all(|o| o.non_tool_receipt().is_none())
        );
    }
}

struct FailureOutput(serde_json::Value);
#[async_trait]
impl HookRunner for FailureOutput {
    fn side_effect_free(&self) -> bool {
        true
    }
    async fn run(&self, _: &HookInvocation) -> Result<RawOutcome> {
        if self.0.is_null() {
            anyhow::bail!("observer execution failed")
        }
        Ok(RawOutcome::Callback {
            value: self.0.clone(),
        })
    }
}

#[tokio::test]
async fn native_provider_failure_observer_failure_and_control_output_cannot_replace_error() {
    for output in [
        serde_json::Value::Null,
        json!({"continue":true}),
        json!({"decision":"block","reason":"retry me"}),
        json!({"unknown":"malformed"}),
    ] {
        let root = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let mut record = crate::inspection::tests::record(root.path());
        record.phase = Some("worker".into());
        let runtime = SharedRuntime::for_test(&state.path().join("record"), record).unwrap();
        let mut tools = ToolExecutor::new(root.path()).unwrap();
        tools
            .register_non_tool_plan(failure_plan(Arc::new(FailureOutput(output))))
            .unwrap();
        let mut session = NativeSession::with_tools(Box::new(ProviderFailure), tools);
        let (_sender, mut commands) = mpsc::channel(4);
        let (tx, _rx) = mpsc::channel(256);
        let events = EventSink::new("failed-output".into(), tx, None)
            .unwrap()
            .with_runtime(runtime.clone());
        assert_eq!(
            session
                .turn("fail".into(), &mut commands, &events)
                .await
                .err()
                .unwrap()
                .to_string(),
            "original provider failure"
        );
        let record = runtime.record().unwrap();
        let receipts: Vec<_> = record
            .operations
            .iter()
            .filter_map(|o| o.non_tool_receipt())
            .collect();
        assert_eq!(receipts.len(), 1, "observer must not recurse");
        assert_eq!(receipts[0].hooks.len(), 1);
        assert!(receipts[0].hooks[0].outcome.is_some());
        assert!(!receipts[0].correction_required && !receipts[0].correction_admitted);
        assert!(receipts[0].hold.is_none());
        assert!(
            runtime
                .admit_non_tool_correction(receipts[0].facts.operation)
                .is_err()
        );
    }
}

#[tokio::test]
async fn native_provider_failure_observed_before_original_turn_ends() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let mut record = crate::inspection::tests::record(root.path());
    record.phase = Some("worker".into());
    let runtime = SharedRuntime::for_test(&state.path().join("record"), record).unwrap();
    runtime
        .allocate(crate::workflow::allocation::Limits::default(), None)
        .unwrap();
    let before = runtime.record().unwrap().allocation.unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let mut tools = ToolExecutor::new(root.path()).unwrap();
    let base = plan(HookEvent::Stop, Arc::new(AllowCount(calls.clone())));
    let mut declaration = base.plan.handlers[0].registration.declaration.clone();
    declaration.required_gate = false;
    declaration.class = HandlerClass::Observer;
    tools
        .register_non_tool_plan(Arc::new(
            NonToolPlan::new(
                HookEvent::StopFailure,
                vec![Registration {
                    declaration,
                    runner: Arc::new(AllowCount(calls.clone())),
                    revalidation: None,
                }],
            )
            .unwrap(),
        ))
        .unwrap();
    let mut session = NativeSession::with_tools(Box::new(ProviderFailure), tools);
    let (_commands, mut commands) = mpsc::channel(4);
    let (tx, _rx) = mpsc::channel(256);
    let events = EventSink::new("failed-provider".into(), tx, None)
        .unwrap()
        .with_runtime(runtime.clone());
    let error = session
        .turn("prompt".into(), &mut commands, &events)
        .await
        .err()
        .unwrap();
    assert_eq!(error.to_string(), "original provider failure");
    assert!(
        error.downcast_ref::<OriginalProviderError>().is_some(),
        "original error identity was replaced"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let record = runtime.record().unwrap();
    let receipts: Vec<_> = record
        .operations
        .iter()
        .filter_map(|o| o.non_tool_receipt())
        .collect();
    assert_eq!(receipts.len(), 1);
    assert!(receipts[0].settled && !receipts[0].correction_required);
    let value = serde_json::to_value(&receipts[0].facts).unwrap();
    assert_eq!(value["subject"]["occurrence"]["event"], "StopFailure");
    assert_eq!(
        value["subject"]["occurrence"]["last_assistant_message"],
        "partial answer"
    );
    let turn = record
        .operations
        .iter()
        .find(|o| Some(o.id) == receipts[0].facts.native_turn)
        .unwrap();
    assert_eq!(
        serde_json::to_value(&turn.host_invocation).unwrap()["native_turn"]["end"],
        "failed"
    );
    let after = record.allocation.unwrap();
    assert_eq!(after.started_ms, before.started_ms);
    assert_eq!(after.deadline_ms, before.deadline_ms);
    assert_eq!(after.model_calls, 1);
}

async fn assert_local_failure_before_request(invalid_key: bool, metadata: bool, invalid_uri: bool) {
    for adapter in ["openai-api", "anthropic-api"] {
        if metadata && adapter != "anthropic-api" {
            continue;
        }
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let root = tempfile::tempdir().unwrap();
        let mut connection: crate::config::Connection = serde_json::from_value(json!({
            "adapter":adapter, "model":"fixture", "api_key":if invalid_key {"fixture\nkey"} else {"fixture-key"},
            "endpoint":if invalid_uri {"https://{{hostname}}/".to_owned()} else {format!("http://{}/v1/responses",listener.local_addr().unwrap())},
            "max_output_tokens":if metadata {None} else {Some(128)}
        }))
        .unwrap();
        let count = Arc::new(AtomicUsize::new(0));
        connection
            .access
            .non_tools
            .push(failure_plan(Arc::new(AllowCount(count.clone()))));
        let (runtime, _) = SharedRuntime::open(root.path(), &connection, None).unwrap();
        runtime
            .allocate(crate::workflow::allocation::Limits::default(), None)
            .unwrap();
        runtime.begin_phase("worker", Some("original")).unwrap();
        let mut session = crate::adapters::builtins()
            .unwrap()
            .open(&connection, root.path())
            .unwrap();
        let (tx, rx) = mpsc::channel(256);
        let _receiver = if invalid_key || invalid_uri {
            Some(rx)
        } else {
            drop(rx);
            None
        };
        let events = EventSink::new("closed-terminal".into(), tx, None)
            .unwrap()
            .with_runtime(runtime.clone());
        let (_sender, mut commands) = mpsc::channel(4);
        let error = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            session.turn("original".into(), &mut commands, &events),
        )
        .await
        .expect("local request failure must return before any HTTP")
        .err()
        .unwrap();
        assert!(
            error.to_string().contains(if invalid_key || invalid_uri {
                "request configuration is invalid"
            } else {
                "closed"
            }),
            "{error:#}"
        );
        assert!(!format!("{error:#}").contains("fixture\nkey"));
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock,
            "provider HTTP must not start"
        );
        assert_eq!(
            count.load(Ordering::SeqCst),
            0,
            "{adapter}: local failure must not dispatch StopFailure"
        );
        std::fs::remove_dir_all(runtime.directory().unwrap()).unwrap();
    }
}

#[tokio::test]
async fn native_stop_failure_real_adapter_pre_request_sink_error_is_not_provider_failure() {
    assert_local_failure_before_request(false, false, false).await;
}

#[test]
fn native_stop_failure_marker_guidance_retains_original_cause_and_local_origin() {
    let error = provider_failure_context(
        provider_response_failure(
            anyhow::Error::new(OriginalProviderError).context("protocol context"),
        ),
        "metadata guidance".into(),
    );
    let original = error.downcast::<ProviderResponseFailure>().unwrap().0;
    assert_eq!(
        format!("{original:#}"),
        "metadata guidance: protocol context: original provider failure"
    );
    assert!(original.downcast_ref::<OriginalProviderError>().is_some());
    let local = provider_failure_context(anyhow::anyhow!("local configuration"), "guidance".into());
    assert!(local.downcast_ref::<ProviderResponseFailure>().is_none());
}

#[test]
fn native_stop_failure_actual_adapter_local_configuration_remains_unmarked() {
    for adapter in ["openai-api", "anthropic-api"] {
        let root = tempfile::tempdir().unwrap();
        let connection: crate::config::Connection = serde_json::from_value(json!({"adapter":adapter,"model":"fixture","api_key":"fixture-key","max_output_tokens":0})).unwrap();
        let error = crate::adapters::builtins()
            .unwrap()
            .open(&connection, root.path())
            .err()
            .unwrap();
        assert!(error.downcast_ref::<ProviderResponseFailure>().is_none());
        assert!(error.to_string().contains("max_output_tokens"));
    }
}

#[tokio::test]
async fn native_stop_failure_real_adapter_invalid_request_header_is_local() {
    // Connection prefers environment credentials. Isolate this invalid-config
    // probe in a child test process rather than mutating parallel tests' env.
    if ["OPENAI_API_KEY", "ANTHROPIC_API_KEY"]
        .iter()
        .any(|key| std::env::var_os(key).is_some())
    {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "native::non_tool_tests::stop_failure::native_stop_failure_real_adapter_invalid_request_header_is_local", "--nocapture"])
            .env_remove("OPENAI_API_KEY").env_remove("ANTHROPIC_API_KEY")
            .output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        return;
    }
    assert_local_failure_before_request(true, false, false).await;
    assert_local_failure_before_request(true, true, false).await;
}

#[tokio::test]
async fn native_stop_failure_real_adapter_invalid_request_uri_is_local() {
    assert_local_failure_before_request(false, false, true).await;
    assert_local_failure_before_request(false, true, true).await;
}
