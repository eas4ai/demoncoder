use demoncoder::{
    config::Connection,
    events::EventSink,
    native::{Model, NativeSession},
    session::Session,
    tools::{ToolCall, ToolExecutor, ToolResult},
    workflow::runtime::SharedRuntime,
};
use serde_json::json;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};
use tokio::sync::mpsc;

static FIXTURE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

struct Responses {
    calls: VecDeque<Vec<ToolCall>>,
    results: Arc<Mutex<Vec<ToolResult>>>,
}
#[async_trait::async_trait]
impl Model for Responses {
    fn prompt(&mut self, _: String) {}
    fn results(&mut self, values: Vec<ToolResult>) {
        self.results.lock().unwrap().extend(values);
    }
    async fn response(&mut self, _: &EventSink) -> anyhow::Result<Vec<ToolCall>> {
        Ok(self.calls.pop_front().unwrap_or_default())
    }
}

#[tokio::test]
async fn explicit_batch_executes_distinct_equal_members_and_reuses_settled_wrapper() {
    let _fixture = FIXTURE.lock().await;
    let workspace = tempfile::tempdir().unwrap();
    let connection: Connection = serde_json::from_value(json!({"adapter":"openai-api"})).unwrap();
    let (runtime, _) = SharedRuntime::open(workspace.path(), &connection, None).unwrap();
    let directory = runtime.directory().unwrap();
    let (tx, _rx) = mpsc::channel(256);
    let events = EventSink::new("batch-fixture".into(), tx, None)
        .unwrap()
        .with_runtime(runtime.clone());
    let calls = json!({"calls":[
        {"tool":"write","arguments":{"path":"counter","content":"a"}},
        {"tool":"edit","arguments":{"path":"counter","old_text":"a","new_text":"ab"}},
        {"tool":"edit","arguments":{"path":"counter","old_text":"a","new_text":"ab"}}
    ]});
    let batch = ToolCall {
        id: "declared".into(),
        name: "tool_batch".into(),
        arguments: calls,
    };
    let results = Arc::new(Mutex::new(Vec::new()));
    let mut session = NativeSession::with_tools(
        Box::new(Responses {
            calls: VecDeque::from([vec![batch.clone(), batch]]),
            results: results.clone(),
        }),
        ToolExecutor::new(workspace.path()).unwrap(),
    );
    let (_tx, mut commands) = mpsc::channel(4);
    let outcome = session
        .turn("perform the declared batch".into(), &mut commands, &events)
        .await;
    let contents = std::fs::read_to_string(workspace.path().join("counter")).ok();
    let record = runtime.record().unwrap();
    drop(session);
    drop(events);
    drop(runtime);
    std::fs::remove_dir_all(directory).unwrap();
    outcome.unwrap();
    assert_eq!(
        contents.as_deref(),
        Some("abb"),
        "all declared members must execute once, including equal payloads"
    );
    let tools: Vec<_> = record
        .operations
        .iter()
        .filter(|o| o.call.is_some())
        .collect();
    assert_eq!(
        tools.len(),
        4,
        "one wrapper and three distinct members, with no replay effects"
    );
    assert!(tools.iter().all(|o| o.complete && o.result.is_some()));
    let output = results.lock().unwrap();
    assert_eq!(output.len(), 2);
    assert_eq!(output[0], output[1]);
}

#[tokio::test]
async fn batch_observer_sees_original_settled_members_once() {
    use demoncoder::plugins::{
        dispatch::*,
        gate_snapshot::GateReadSet,
        hook_types::{HandlerKind, HookDialect, HookEvent},
        non_tool::NonToolPlan,
    };
    struct Observe(Arc<Mutex<Vec<serde_json::Value>>>);
    #[async_trait::async_trait]
    impl HookRunner for Observe {
        fn side_effect_free(&self) -> bool {
            true
        }
        async fn run(&self, invocation: &HookInvocation) -> anyhow::Result<RawOutcome> {
            self.0.lock().unwrap().push(serde_json::to_value(
                invocation.lifecycle.as_ref().unwrap(),
            )?);
            Ok(RawOutcome::Callback { value: json!({}) })
        }
    }
    let _fixture = FIXTURE.lock().await;
    let workspace = tempfile::tempdir().unwrap();
    let connection: Connection = serde_json::from_value(json!({"adapter":"openai-api"})).unwrap();
    let (runtime, _) = SharedRuntime::open(workspace.path(), &connection, None).unwrap();
    let directory = runtime.directory().unwrap();
    runtime.begin_phase("worker", None).unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let plan = NonToolPlan::new(
        HookEvent::PostToolBatch,
        vec![Registration {
            declaration: Declaration {
                required_gate: false,
                source: None,
                once: None,
                identity: DeclarationIdentity {
                    package: "batch-observer".into(),
                    code: "code".into(),
                    policy: "policy".into(),
                    configuration: "config".into(),
                    generation: "1".into(),
                    scope: Scope::Project,
                    role: "worker".into(),
                    declaration: "batch".into(),
                    index: 0,
                    dialect: HookDialect::Native,
                    runner: HandlerKind::Command,
                },
                class: HandlerClass::Observer,
                priority: 0,
                matcher: Matcher::default(),
                reads: GateReadSet::default(),
                concurrent_group: None,
                read_only_endpoint: None,
                external_precondition: None,
            },
            runner: Arc::new(Observe(seen.clone())),
            revalidation: None,
        }],
    );
    let plan = match plan {
        Ok(p) => p,
        Err(e) => {
            drop(runtime);
            std::fs::remove_dir_all(directory).unwrap();
            panic!("real batch lifecycle must be supported: {e:#}");
        }
    };
    let mut tools = ToolExecutor::new(workspace.path()).unwrap();
    tools.register_non_tool_plan(Arc::new(plan)).unwrap();
    let batch = ToolCall {
        id: "batch".into(),
        name: "tool_batch".into(),
        arguments: json!({"calls":[
            {"tool":"write","arguments":{"path":"actual","content":"first"}},
            {"tool":"edit","arguments":{"path":"actual","old_text":"first","new_text":"settled"}}
        ]}),
    };
    let mut native = NativeSession::with_tools(
        Box::new(Responses {
            calls: VecDeque::from([vec![batch.clone(), batch]]),
            results: Default::default(),
        }),
        tools,
    );
    let (tx, _rx) = mpsc::channel(256);
    let events = EventSink::new("batch-observer".into(), tx, None)
        .unwrap()
        .with_runtime(runtime.clone());
    let (_tx, mut commands) = mpsc::channel(4);
    let outcome = native
        .turn("execute batch".into(), &mut commands, &events)
        .await;
    let evidence = runtime.record().unwrap();
    drop(native);
    drop(events);
    drop(runtime);
    std::fs::remove_dir_all(directory).unwrap();
    outcome.unwrap();
    assert_eq!(
        std::fs::read_to_string(workspace.path().join("actual")).unwrap(),
        "settled"
    );
    let seen = seen.lock().unwrap();
    assert_eq!(
        seen.len(),
        1,
        "wrapper retry cannot repeat batch observation"
    );
    let occurrence = &seen[0]["subject"]["occurrence"];
    assert_eq!(occurrence["event"], "PostToolBatch");
    assert_eq!(occurrence["tool_calls"].as_array().unwrap().len(), 2);
    let batch_id = occurrence["batch"].as_u64().unwrap();
    let members: Vec<_> = evidence
        .operations
        .iter()
        .filter(|o| {
            o.tool_receipt
                .as_ref()
                .is_some_and(|r| r.invocation == batch_id)
        })
        .collect();
    assert_eq!(members.len(), 2);
    assert!(
        members
            .iter()
            .all(|o| o.complete && o.result.as_ref().unwrap().success)
    );
}

#[tokio::test]
async fn cancellation_after_first_member_keeps_effect_and_never_observes_unsettled_batch() {
    use demoncoder::{
        events::Event,
        plugins::{
            dispatch::*,
            gate_snapshot::GateReadSet,
            hook_types::{HandlerKind, HookDialect, HookEvent},
            non_tool::NonToolPlan,
        },
        session::{Command, TurnEnd},
    };
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct Observer(Arc<AtomicUsize>);
    #[async_trait::async_trait]
    impl HookRunner for Observer {
        fn side_effect_free(&self) -> bool {
            true
        }
        async fn run(&self, _: &HookInvocation) -> anyhow::Result<RawOutcome> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(RawOutcome::Callback { value: json!({}) })
        }
    }
    let _fixture = FIXTURE.lock().await;
    let workspace = tempfile::tempdir().unwrap();
    let config: Connection = serde_json::from_value(json!({"adapter":"openai-api"})).unwrap();
    let (runtime, _) = SharedRuntime::open(workspace.path(), &config, None).unwrap();
    let directory = runtime.directory().unwrap();
    runtime.begin_phase("worker", None).unwrap();
    let seen = Arc::new(AtomicUsize::new(0));
    let mut tools = ToolExecutor::new(workspace.path()).unwrap();
    tools
        .register_non_tool_plan(Arc::new(
            NonToolPlan::new(
                HookEvent::PostToolBatch,
                vec![Registration {
                    declaration: Declaration {
                        required_gate: false,
                        source: None,
                        once: None,
                        identity: DeclarationIdentity {
                            package: "barrier".into(),
                            code: "code".into(),
                            policy: "policy".into(),
                            configuration: "config".into(),
                            generation: "1".into(),
                            scope: Scope::Project,
                            role: "worker".into(),
                            declaration: "barrier".into(),
                            index: 0,
                            dialect: HookDialect::Native,
                            runner: HandlerKind::Command,
                        },
                        class: HandlerClass::Observer,
                        priority: 0,
                        matcher: Matcher::default(),
                        reads: GateReadSet::default(),
                        concurrent_group: None,
                        read_only_endpoint: None,
                        external_precondition: None,
                    },
                    runner: Arc::new(Observer(seen.clone())),
                    revalidation: None,
                }],
            )
            .unwrap(),
        ))
        .unwrap();
    let batch = ToolCall {
        id: "barrier".into(),
        name: "tool_batch".into(),
        arguments: json!({"calls":[{"tool":"write","arguments":{"path":"first","content":"retained"}},{"tool":"bash","arguments":{"command":"sleep 60"}},{"tool":"write","arguments":{"path":"never","content":"must not run"}}]}),
    };
    let mut native = NativeSession::with_tools(
        Box::new(Responses {
            calls: VecDeque::from([vec![batch]]),
            results: Default::default(),
        }),
        tools,
    );
    let (tx, mut output) = mpsc::channel(256);
    let events = EventSink::new("batch-barrier".into(), tx, None)
        .unwrap()
        .with_runtime(runtime.clone());
    let (sender, mut commands) = mpsc::channel(4);
    let observed = seen.clone();
    let path = workspace.path().to_owned();
    let inspect = tokio::spawn(async move {
        while let Some(e) = output.recv().await {
            if matches!(e.event,Event::ToolStarted{ref call} if call.name=="bash") {
                assert_eq!(
                    std::fs::read_to_string(path.join("first")).unwrap(),
                    "retained"
                );
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                assert_eq!(
                    observed.load(Ordering::SeqCst),
                    0,
                    "unfinished member cannot release PostToolBatch"
                );
                sender.send(Command::Cancel).await.unwrap();
                break;
            }
        }
        while output.recv().await.is_some() {}
    });
    let result = native
        .turn("run declared batch".into(), &mut commands, &events)
        .await;
    let record = runtime.record().unwrap();
    drop(native);
    drop(events);
    drop(runtime);
    inspect.await.unwrap();
    std::fs::remove_dir_all(directory).unwrap();
    assert!(matches!(result, Ok(TurnEnd::Cancelled)));
    assert_eq!(seen.load(Ordering::SeqCst), 0);
    assert!(!workspace.path().join("never").exists());
    assert_eq!(
        std::fs::read_to_string(workspace.path().join("first")).unwrap(),
        "retained"
    );
    assert!(record.operations.iter().any(|o| {
        o.result
            .as_ref()
            .is_some_and(|r| r.tool == "write" && r.success)
    }));
}
#[tokio::test]
async fn malformed_declared_members_are_rejected_before_any_member_effect() {
    let _fixture = FIXTURE.lock().await;
    for calls in [
        json!([]),
        json!([{"tool":"tool_batch","arguments":{"calls":[]}}]),
        json!([{"id":"supplied","tool":"write","arguments":{"path":"bad","content":"bad"}}]),
        json!([{"tool":"write","arguments":{"path":"bad","content":"x".repeat(1024*1024)}}]),
        json!(
            (0..33)
                .map(|_| json!({"tool":"write","arguments":{"path":"bad","content":"bad"}}))
                .collect::<Vec<_>>()
        ),
    ] {
        let workspace = tempfile::tempdir().unwrap();
        let config: Connection = serde_json::from_value(json!({"adapter":"openai-api"})).unwrap();
        let (runtime, _) = SharedRuntime::open(workspace.path(), &config, None).unwrap();
        let directory = runtime.directory().unwrap();
        let (tx, _rx) = mpsc::channel(128);
        let events = EventSink::new("invalid-batch".into(), tx, None)
            .unwrap()
            .with_runtime(runtime.clone());
        let results = Arc::new(Mutex::new(vec![]));
        let mut native = NativeSession::new(
            Box::new(Responses {
                calls: VecDeque::from([vec![ToolCall {
                    id: "invalid".into(),
                    name: "tool_batch".into(),
                    arguments: json!({"calls":calls}),
                }]]),
                results: results.clone(),
            }),
            workspace.path(),
        )
        .unwrap();
        let (_tx, mut commands) = mpsc::channel(4);
        let _ = native
            .turn("invalid batch".into(), &mut commands, &events)
            .await;
        assert!(!workspace.path().join("bad").exists());
        assert!(!results.lock().unwrap().iter().any(|r| r.success));
        assert!(
            !runtime
                .record()
                .unwrap()
                .operations
                .iter()
                .any(|o| matches!(
                    o.host_invocation,
                    Some(demoncoder::workflow::runtime::HostInvocation::ToolBatch(_))
                ))
        );
        drop(native);
        drop(events);
        drop(runtime);
        std::fs::remove_dir_all(directory).unwrap();
    }
}

#[tokio::test]
async fn batch_model_false_follows_declaration_dialect_and_retains_member_evidence() {
    use demoncoder::plugins::{
        dispatch::*,
        gate_snapshot::GateReadSet,
        hook_types::{HandlerKind, HookDialect, HookEvent},
        non_tool::NonToolPlan,
    };
    struct Observe(Arc<Mutex<Vec<serde_json::Value>>>);
    #[async_trait::async_trait]
    impl HookRunner for Observe {
        fn side_effect_free(&self) -> bool {
            true
        }
        async fn run(&self, invocation: &HookInvocation) -> anyhow::Result<RawOutcome> {
            self.0.lock().unwrap().push(serde_json::to_value(
                invocation.lifecycle.as_ref().unwrap(),
            )?);
            Ok(RawOutcome::Model {
                value: json!({"ok":false,"reason":"declared batch verdict"}),
                continue_on_block: true,
            })
        }
    }
    let _fixture = FIXTURE.lock().await;
    for dialect in [HookDialect::Native, HookDialect::Claude] {
        let workspace = tempfile::tempdir().unwrap();
        let connection: Connection =
            serde_json::from_value(json!({"adapter":"openai-api"})).unwrap();
        let (runtime, _) = SharedRuntime::open(workspace.path(), &connection, None).unwrap();
        let directory = runtime.directory().unwrap();
        runtime.begin_phase("worker", None).unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let plan = NonToolPlan::new(
            HookEvent::PostToolBatch,
            vec![Registration {
                declaration: Declaration {
                    required_gate: false,
                    source: None,
                    once: None,
                    identity: DeclarationIdentity {
                        package: "batch-observer".into(),
                        code: "code".into(),
                        policy: "policy".into(),
                        configuration: "config".into(),
                        generation: "1".into(),
                        scope: Scope::Project,
                        role: "worker".into(),
                        declaration: "batch".into(),
                        index: 0,
                        dialect,
                        runner: HandlerKind::Prompt,
                    },
                    class: HandlerClass::Combined,
                    priority: 0,
                    matcher: Matcher::default(),
                    reads: GateReadSet::default(),
                    concurrent_group: (dialect == HookDialect::Claude)
                        .then(|| "source-batch".into()),
                    read_only_endpoint: None,
                    external_precondition: None,
                },
                runner: Arc::new(Observe(seen.clone())),
                revalidation: None,
            }],
        );
        let plan = match plan {
            Ok(p) => p,
            Err(e) => {
                drop(runtime);
                std::fs::remove_dir_all(directory).unwrap();
                panic!("real batch lifecycle must be supported: {e:#}");
            }
        };
        let mut tools = ToolExecutor::new(workspace.path()).unwrap();
        tools.register_non_tool_plan(Arc::new(plan)).unwrap();
        let batch = ToolCall {
            id: "batch".into(),
            name: "tool_batch".into(),
            arguments: json!({"calls":[
                {"tool":"write","arguments":{"path":"actual","content":"first"}},
                {"tool":"edit","arguments":{"path":"actual","old_text":"first","new_text":"settled"}}
            ]}),
        };
        let mut native = NativeSession::with_tools(
            Box::new(Responses {
                calls: VecDeque::from([vec![batch.clone(), batch]]),
                results: Default::default(),
            }),
            tools,
        );
        let (tx, _rx) = mpsc::channel(256);
        let events = EventSink::new("batch-observer".into(), tx, None)
            .unwrap()
            .with_runtime(runtime.clone());
        let (_tx, mut commands) = mpsc::channel(4);
        let outcome = native
            .turn("execute batch".into(), &mut commands, &events)
            .await;
        let evidence = runtime.record().unwrap();
        drop(native);
        drop(events);
        drop(runtime);
        std::fs::remove_dir_all(directory).unwrap();
        assert_eq!(
            outcome.is_err(),
            dialect == HookDialect::Claude,
            "{:?}",
            outcome.err()
        );
        assert_eq!(
            std::fs::read_to_string(workspace.path().join("actual")).unwrap(),
            "settled"
        );
        let seen = seen.lock().unwrap();
        assert_eq!(seen[0]["provenance"], "explicit_host_operation_v1");
        assert_eq!(
            seen.len(),
            1,
            "wrapper retry cannot repeat batch observation"
        );
        let occurrence = &seen[0]["subject"]["occurrence"];
        assert_eq!(occurrence["event"], "PostToolBatch");
        assert_eq!(occurrence["tool_calls"].as_array().unwrap().len(), 2);
        let batch_id = occurrence["batch"].as_u64().unwrap();
        let members: Vec<_> = evidence
            .operations
            .iter()
            .filter(|o| {
                o.tool_receipt
                    .as_ref()
                    .is_some_and(|r| r.invocation == batch_id)
            })
            .collect();
        assert_eq!(members.len(), 2);
        assert!(
            members
                .iter()
                .all(|o| o.complete && o.result.as_ref().unwrap().success)
        );
    }
}
