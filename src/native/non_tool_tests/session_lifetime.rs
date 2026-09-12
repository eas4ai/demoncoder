use super::*;
use crate::session::{SessionEnd, SessionStart};
use std::time::{Duration, Instant};

struct Trap(Arc<AtomicUsize>);
#[async_trait]
impl HookRunner for Trap {
    fn side_effect_free(&self) -> bool {
        true
    }
    async fn prepare(&self, _: &HookInvocation) -> Result<()> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    async fn run(&self, _: &HookInvocation) -> Result<RawOutcome> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(RawOutcome::Callback { value: json!({}) })
    }
}
fn observation_plan(
    event: HookEvent,
    kind: HandlerKind,
    calls: Arc<AtomicUsize>,
) -> Arc<NonToolPlan> {
    let runner = Arc::new(Trap(calls));
    let base = plan(HookEvent::Stop, runner.clone());
    let mut declaration = base.plan.handlers[0].registration.declaration.clone();
    declaration.required_gate = false;
    declaration.class = HandlerClass::Observer;
    declaration.identity.runner = kind;
    Arc::new(
        NonToolPlan::new(
            event,
            vec![Registration {
                declaration,
                runner,
                revalidation: None,
            }],
        )
        .unwrap(),
    )
}
fn fixture() -> (
    tempfile::TempDir,
    tempfile::TempDir,
    SharedRuntime,
    EventSink,
    mpsc::Receiver<crate::events::Envelope>,
) {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let mut record = crate::inspection::tests::record(root.path());
    record.phase = None;
    let runtime = SharedRuntime::for_test(&state.path().join("record"), record).unwrap();
    let (tx, rx) = mpsc::channel(256);
    let events = EventSink::new("lifetime".into(), tx, None)
        .unwrap()
        .with_runtime(runtime.clone());
    (root, state, runtime, events, rx)
}
fn native(tools: ToolExecutor) -> NativeSession {
    NativeSession::with_tools(
        Box::new(Counting {
            requests: Arc::new(AtomicUsize::new(0)),
            prompts: Arc::new(Mutex::new(vec![])),
        }),
        tools,
    )
}

#[tokio::test]
async fn native_workflow_resume_forwards_one_actual_resume_lifetime() {
    let (root, _state, runtime, events, mut rx) = fixture();
    let calls = Arc::new(AtomicUsize::new(0));
    let mut tools = ToolExecutor::new(root.path()).unwrap();
    tools
        .register_non_tool_plan(observation_plan(
            HookEvent::SessionStart,
            HandlerKind::Command,
            calls.clone(),
        ))
        .unwrap();
    let connection = serde_json::from_value(json!({"adapter":"openai-api"})).unwrap();
    runtime
        .update(|record| {
            record.identity = crate::workflow::runtime::Identity::from(&connection);
            Ok(())
        })
        .unwrap();
    let session = crate::workflow::WorkflowSession::new(
        Box::new(native(tools)),
        connection,
        root.path().into(),
        crate::workflow::Settings::default(),
        runtime.clone(),
        true,
    )
    .unwrap();
    let (tx, commands) = mpsc::channel(4);
    let controls = async {
        while !matches!(rx.recv().await.unwrap().event, Event::Ready { .. }) {}
        tx.send(Command::Shutdown).await.unwrap();
    };
    let (result, ()) = tokio::join!(
        crate::session::run(Box::new(session), commands, events),
        controls
    );
    result.unwrap();
    let record = runtime.record().unwrap();
    let lifetimes: Vec<_> = record
        .operations
        .iter()
        .filter_map(|o| match &o.host_invocation {
            Some(crate::workflow::runtime::HostInvocation::NativeSession(v)) => Some(v),
            _ => None,
        })
        .collect();
    assert_eq!(lifetimes.len(), 1);
    assert_eq!(lifetimes[0].source, SessionStart::Resume);
    assert_eq!(lifetimes[0].end, Some(SessionEnd::Shutdown));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn unsupported_lifetime_runners_cannot_prepare_even_with_task_allowance() {
    for kind in [
        HandlerKind::Http,
        HandlerKind::McpTool,
        HandlerKind::Prompt,
        HandlerKind::Agent,
    ] {
        let (root, _state, runtime, events, mut rx) = fixture();
        runtime
            .allocate(crate::workflow::allocation::Limits::default(), None)
            .unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let mut tools = ToolExecutor::new(root.path()).unwrap();
        tools
            .register_non_tool_plan(observation_plan(
                HookEvent::SessionStart,
                kind,
                calls.clone(),
            ))
            .unwrap();
        let (tx, commands) = mpsc::channel(4);
        let controls = async {
            while !matches!(rx.recv().await.unwrap().event, Event::Ready { .. }) {}
            tx.send(Command::Shutdown).await.unwrap();
        };
        let (result, ()) = tokio::join!(
            crate::session::run(Box::new(native(tools)), commands, events),
            controls
        );
        result.unwrap();
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "{kind:?} borrowed task authority"
        );
        let record = runtime.record().unwrap();
        assert_eq!(record.allocation.unwrap().model_calls, 0);
        assert!(
            record
                .operations
                .iter()
                .find_map(|o| o.non_tool_receipt())
                .unwrap()
                .diagnostics[0]
                .contains("supports synchronous native commands only")
        );
    }
}

#[tokio::test]
async fn lifetime_end_deadline_includes_runner_permit_and_mutation_lock_waits() {
    for permit in [false, true] {
        let (root, _state, runtime, events, mut rx) = fixture();
        let calls = Arc::new(AtomicUsize::new(0));
        let plan = observation_plan(HookEvent::SessionEnd, HandlerKind::Command, calls.clone());
        let _permit = if permit {
            Some(
                plan.plan
                    .runners
                    .clone()
                    .acquire_many_owned(32)
                    .await
                    .unwrap(),
            )
        } else {
            None
        };
        use std::os::unix::fs::MetadataExt;
        let metadata = std::fs::metadata(root.path()).unwrap();
        let boundary = runtime
            .mutation_boundary((metadata.dev(), metadata.ino()))
            .unwrap();
        let _guard = if permit {
            None
        } else {
            Some(boundary.lock_owned().await)
        };
        let mut tools = ToolExecutor::new(root.path()).unwrap();
        tools.register_non_tool_plan(plan).unwrap();
        let (tx, commands) = mpsc::channel(4);
        let start = Instant::now();
        let controls = async {
            while !matches!(rx.recv().await.unwrap().event, Event::Ready { .. }) {}
            tx.send(Command::Shutdown).await.unwrap();
        };
        let (result, ()) = tokio::time::timeout(Duration::from_millis(5300), async {
            tokio::join!(
                crate::session::run(Box::new(native(tools)), commands, events),
                controls
            )
        })
        .await
        .unwrap();
        result.unwrap();
        assert!(start.elapsed() < Duration::from_secs(5));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        let record = runtime.record().unwrap();
        let lifetime = record
            .operations
            .iter()
            .find_map(|o| match &o.host_invocation {
                Some(crate::workflow::runtime::HostInvocation::NativeSession(v)) => Some(v),
                _ => None,
            })
            .unwrap();
        assert_eq!(lifetime.source, SessionStart::Startup);
        assert_eq!(lifetime.end, Some(SessionEnd::Shutdown));
        assert!(
            lifetime
                .diagnostics
                .iter()
                .any(|d| d.contains("SessionEnd observation unavailable"))
        );
    }
}
