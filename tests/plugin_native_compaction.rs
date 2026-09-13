use demoncoder::{
    config::Connection,
    events::{Event, EventSink},
    native::{Model, NativeSession},
    session::{Command, Session, TurnEnd},
    tools::{ToolCall, ToolExecutor, ToolResult},
    workflow::runtime::SharedRuntime,
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
static FIXTURE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
struct Transcript {
    history: Vec<Value>,
    summaries: Arc<Mutex<Vec<String>>>,
    summary: String,
    pause: bool,
    change: Option<std::path::PathBuf>,
}
#[async_trait::async_trait]
impl Model for Transcript {
    fn checkpoint(&self) -> Option<Value> {
        Some(json!(self.history))
    }
    fn restore(&mut self, v: &Value) -> anyhow::Result<()> {
        self.history = v.as_array().unwrap().clone();
        Ok(())
    }
    fn prompt(&mut self, s: String) {
        self.history.push(json!({"role":"user","content":s}));
    }
    fn results(&mut self, _: Vec<ToolResult>) {}
    async fn response(&mut self, e: &EventSink) -> anyhow::Result<Vec<ToolCall>> {
        self.history
            .push(json!({"role":"assistant","content":"Completed seed response."}));
        e.emit(Event::Usage {
            input: Some(2),
            output: Some(1),
            cached: None,
            cost_usd: None,
        })
        .await?;
        Ok(vec![])
    }
    async fn summarize(&mut self, input: String, e: &EventSink) -> anyhow::Result<String> {
        self.summaries.lock().unwrap().push(input);
        if let Some(path) = &self.change {
            std::fs::write(path, "changed after gate")?;
        }
        if self.pause {
            std::future::pending::<()>().await;
        }
        e.emit(Event::Usage {
            input: Some(123),
            output: Some(7),
            cached: Some(1),
            cost_usd: None,
        })
        .await?;
        Ok(self.summary.clone())
    }
}
async fn seed(native: &mut NativeSession, e: &EventSink, commands: &mut mpsc::Receiver<Command>) {
    for i in 0..5 {
        native
            .turn(
                format!("Developer seed {i}: {}", "history ".repeat(600)),
                commands,
                e,
            )
            .await
            .unwrap();
    }
}
#[tokio::test]
async fn manual_compaction_installs_smaller_context_with_original_usage_and_prompt() {
    let _lock = FIXTURE.lock().await;
    let workspace = tempfile::tempdir().unwrap();
    let config: Connection =
        serde_json::from_value(json!({"adapter":"openai-api","model":"same-model"})).unwrap();
    let (runtime, _) = SharedRuntime::open(workspace.path(), &config, None).unwrap();
    let directory = runtime.directory().unwrap();
    let (tx, mut rx) = mpsc::channel(128);
    let drain = tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let events = EventSink::new("compaction".into(), tx, None)
        .unwrap()
        .with_runtime(runtime.clone());
    let summaries = Arc::new(Mutex::new(vec![]));
    let mut native = NativeSession::new(
        Box::new(Transcript {
            history: vec![],
            summaries: summaries.clone(),
            summary: "Retained summary canary: unfinished work and decisions.".into(),
            pause: false,
            change: None,
        }),
        workspace.path(),
    )
    .unwrap();
    let (_tx, mut commands) = mpsc::channel(8);
    seed(&mut native, &events, &mut commands).await;
    let before = runtime.record().unwrap();
    let result = native.compact(&mut commands, &events).await;
    let after = runtime.record().unwrap();
    let checkpoint = native.checkpoint().unwrap();
    drop(native);
    drop(events);
    drop(runtime);
    drain.await.unwrap();
    std::fs::remove_dir_all(directory).unwrap();
    assert!(
        matches!(result, Ok(TurnEnd::Complete)),
        "{}",
        result
            .as_ref()
            .err()
            .map(|e| format!("{e:#}"))
            .unwrap_or_default()
    );
    assert_eq!(summaries.lock().unwrap().len(), 1);
    assert!(
        serde_json::to_vec(&checkpoint).unwrap().len()
            < serde_json::to_vec(before.checkpoint.as_ref().unwrap())
                .unwrap()
                .len()
    );
    assert_eq!(after.checkpoint.as_ref(), Some(&checkpoint));
    let text = serde_json::to_string(&checkpoint).unwrap();
    assert!(text.contains("Retained summary canary"));
    assert!(text.contains("Developer seed 4:"));
    assert_eq!(
        serde_json::to_value(&after.task).unwrap(),
        serde_json::to_value(&before.task).unwrap()
    );
    assert!(after.session_hook_allowance.is_none());
    assert_eq!(
        &serde_json::to_value(&after.operations[..before.operations.len()]).unwrap(),
        &serde_json::to_value(&before.operations).unwrap()
    );
    let models: Vec<_> = after
        .operations
        .iter()
        .filter(|o| o.usage_receipt.is_some())
        .collect();
    assert_eq!(models.len(), 6);
    let usage = serde_json::to_value(models.last().unwrap()).unwrap();
    assert!(serde_json::to_string(&usage).unwrap().contains("123"));
    assert_eq!(usage["budget"], "Unallocated");
}
#[tokio::test]
async fn oversized_summary_preserves_original_and_records_no_applied_compaction() {
    let _lock = FIXTURE.lock().await;
    let workspace = tempfile::tempdir().unwrap();
    let config: Connection = serde_json::from_value(json!({"adapter":"openai-api"})).unwrap();
    let (runtime, _) = SharedRuntime::open(workspace.path(), &config, None).unwrap();
    let directory = runtime.directory().unwrap();
    let (tx, _rx) = mpsc::channel(128);
    let events = EventSink::new("bad-summary".into(), tx, None)
        .unwrap()
        .with_runtime(runtime.clone());
    let mut native = NativeSession::new(
        Box::new(Transcript {
            history: vec![],
            summaries: Default::default(),
            summary: "x".repeat(16385),
            pause: false,
            change: None,
        }),
        workspace.path(),
    )
    .unwrap();
    let (_tx, mut commands) = mpsc::channel(8);
    seed(&mut native, &events, &mut commands).await;
    let before = native.checkpoint();
    let result = native.compact(&mut commands, &events).await;
    assert!(result.is_err());
    assert_eq!(native.checkpoint(), before);
    assert_eq!(runtime.record().unwrap().checkpoint, before);
    assert!(
        !serde_json::to_string(&runtime.record().unwrap().operations)
            .unwrap()
            .contains("\"applied\":\"")
    );
    drop(native);
    drop(events);
    drop(runtime);
    std::fs::remove_dir_all(directory).unwrap();
}
#[tokio::test]
async fn cancellation_during_summary_keeps_context_and_no_ordinary_continuation() {
    let _lock = FIXTURE.lock().await;
    let workspace = tempfile::tempdir().unwrap();
    let config: Connection = serde_json::from_value(json!({"adapter":"openai-api"})).unwrap();
    let (runtime, _) = SharedRuntime::open(workspace.path(), &config, None).unwrap();
    let directory = runtime.directory().unwrap();
    let (tx, _rx) = mpsc::channel(128);
    let events = EventSink::new("cancel-summary".into(), tx, None)
        .unwrap()
        .with_runtime(runtime.clone());
    let summaries = Arc::new(Mutex::new(vec![]));
    let mut native = NativeSession::new(
        Box::new(Transcript {
            history: vec![],
            summaries: summaries.clone(),
            summary: "summary".into(),
            pause: true,
            change: None,
        }),
        workspace.path(),
    )
    .unwrap();
    let (tx, mut commands) = mpsc::channel(8);
    seed(&mut native, &events, &mut commands).await;
    let before = native.checkpoint();
    let notice = summaries.clone();
    let cancel = tokio::spawn(async move {
        loop {
            if !notice.lock().unwrap().is_empty() {
                break;
            }
            tokio::task::yield_now().await;
        }
        tx.send(Command::Cancel).await.unwrap();
    });
    let result = native.compact(&mut commands, &events).await;
    cancel.await.unwrap();
    assert!(matches!(result, Ok(TurnEnd::Cancelled)));
    assert_eq!(native.checkpoint(), before);
    assert_eq!(runtime.record().unwrap().checkpoint, before);
    assert_eq!(summaries.lock().unwrap().len(), 1);
    drop(native);
    drop(events);
    drop(runtime);
    std::fs::remove_dir_all(directory).unwrap();
}

use demoncoder::plugins::{
    dispatch::*,
    gate_snapshot::GateReadSet,
    hook_types::{HandlerKind, HookDialect, HookEvent},
    non_tool::NonToolPlan,
};
struct LifecycleHook {
    deny: bool,
    seen: Arc<Mutex<Vec<String>>>,
}
#[async_trait::async_trait]
impl HookRunner for LifecycleHook {
    fn side_effect_free(&self) -> bool {
        true
    }
    async fn run(&self, i: &HookInvocation) -> anyhow::Result<RawOutcome> {
        self.seen.lock().unwrap().push(i.key.event.clone());
        Ok(RawOutcome::Callback {
            value: if self.deny {
                json!({"decision":"block","reason":"required pre-compaction policy"})
            } else {
                json!({})
            },
        })
    }
}
fn plan(event: HookEvent, runner: Arc<dyn HookRunner>, reads: GateReadSet) -> Arc<NonToolPlan> {
    Arc::new(
        NonToolPlan::new(
            event,
            vec![Registration {
                declaration: Declaration {
                    required_gate: event == HookEvent::PreCompact,
                    source: None,
                    once: None,
                    identity: DeclarationIdentity {
                        package: "compact-policy".into(),
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
                    reads,
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
async fn precompact_denial_and_changed_gate_inputs_preserve_context_and_skip_post() {
    let _lock = FIXTURE.lock().await;
    for mode in ["allow", "deny", "stale"] {
        let workspace = tempfile::tempdir().unwrap();
        std::fs::write(workspace.path().join("watched"), "inspected").unwrap();
        let config: Connection = serde_json::from_value(json!({"adapter":"openai-api"})).unwrap();
        let (runtime, _) = SharedRuntime::open(workspace.path(), &config, None).unwrap();
        let directory = runtime.directory().unwrap();
        let (tx, mut rx) = mpsc::channel(128);
        let drain = tokio::spawn(async move { while rx.recv().await.is_some() {} });
        let events = EventSink::new("precompact".into(), tx, None)
            .unwrap()
            .with_runtime(runtime.clone());
        let summaries = Arc::new(Mutex::new(vec![]));
        let seen = Arc::new(Mutex::new(vec![]));
        let mut tools = ToolExecutor::new(workspace.path()).unwrap();
        tools
            .register_non_tool_plan(plan(
                HookEvent::PreCompact,
                Arc::new(LifecycleHook {
                    deny: mode == "deny",
                    seen: seen.clone(),
                }),
                GateReadSet::new(vec!["watched".into()], vec![], vec![]).unwrap(),
            ))
            .unwrap();
        tools
            .register_non_tool_plan(plan(
                HookEvent::PostCompact,
                Arc::new(LifecycleHook {
                    deny: false,
                    seen: seen.clone(),
                }),
                GateReadSet::default(),
            ))
            .unwrap();
        let mut native = NativeSession::with_tools(
            Box::new(Transcript {
                history: vec![],
                summaries: summaries.clone(),
                summary: "Earlier decisions retained in summary.".into(),
                pause: false,
                change: (mode == "stale").then(|| workspace.path().join("watched")),
            }),
            tools,
        );
        let (_tx, mut commands) = mpsc::channel(8);
        seed(&mut native, &events, &mut commands).await;
        let before = native.checkpoint();
        let result = native.compact(&mut commands, &events).await;
        let after = runtime.record().unwrap();
        drop(native);
        drop(events);
        drop(runtime);
        drain.await.unwrap();
        std::fs::remove_dir_all(directory).unwrap();
        assert_eq!(
            result.is_ok(),
            mode == "allow",
            "mode={mode}: {:?}",
            result.err()
        );
        assert_eq!(summaries.lock().unwrap().len(), usize::from(mode != "deny"));
        assert_eq!(
            seen.lock().unwrap().as_slice(),
            if mode == "allow" {
                vec!["PreCompact", "PostCompact"]
            } else {
                vec!["PreCompact"]
            }
        );
        if mode != "allow" {
            assert_eq!(after.checkpoint, before);
        }
    }
}

#[tokio::test]
async fn nonshrinking_summary_preserves_original_and_records_no_applied_compaction() {
    let _lock = FIXTURE.lock().await;
    let workspace = tempfile::tempdir().unwrap();
    let config: Connection = serde_json::from_value(json!({"adapter":"openai-api"})).unwrap();
    let (runtime, _) = SharedRuntime::open(workspace.path(), &config, None).unwrap();
    let directory = runtime.directory().unwrap();
    let (tx, _rx) = mpsc::channel(128);
    let events = EventSink::new("bad-summary".into(), tx, None)
        .unwrap()
        .with_runtime(runtime.clone());
    let mut native = NativeSession::new(
        Box::new(Transcript {
            history: vec![],
            summaries: Default::default(),
            summary: "x".repeat(16384),
            pause: false,
            change: None,
        }),
        workspace.path(),
    )
    .unwrap();
    let (_tx, mut commands) = mpsc::channel(8);
    seed(&mut native, &events, &mut commands).await;
    let before = native.checkpoint();
    let result = native.compact(&mut commands, &events).await;
    assert!(result.is_err());
    assert_eq!(native.checkpoint(), before);
    assert_eq!(runtime.record().unwrap().checkpoint, before);
    assert!(
        !serde_json::to_string(&runtime.record().unwrap().operations)
            .unwrap()
            .contains("\"applied\":\"")
    );
    drop(native);
    drop(events);
    drop(runtime);
    std::fs::remove_dir_all(directory).unwrap();
}
