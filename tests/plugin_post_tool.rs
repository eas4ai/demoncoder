use demoncoder::{
    config::Connection,
    events::{Envelope, EventSink},
    native::{Model, NativeSession},
    plugins::{
        dispatch::*,
        gate_snapshot::GateReadSet,
        hook_types::{HandlerKind, HookDialect, HookEvent},
        lifecycle::PostToolPlan,
    },
    session::{Session, TurnEnd},
    tools::{ToolCall, ToolExecutor, ToolResult},
    workflow::runtime::{Record, SharedRuntime},
};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio::sync::mpsc;

struct Responses {
    calls: VecDeque<Vec<ToolCall>>,
    results: Arc<Mutex<Vec<ToolResult>>>,
    requests: Arc<AtomicUsize>,
}
#[async_trait::async_trait]
impl Model for Responses {
    fn prompt(&mut self, _: String) {}
    fn results(&mut self, results: Vec<ToolResult>) {
        self.results.lock().unwrap().extend(results);
    }
    async fn response(&mut self, _: &EventSink) -> anyhow::Result<Vec<ToolCall>> {
        self.requests.fetch_add(1, Ordering::SeqCst);
        Ok(self.calls.pop_front().unwrap_or_default())
    }
}
type Action = dyn Fn(&HookInvocation) -> RawOutcome + Send + Sync;
struct Runner {
    action: Box<Action>,
    count: Arc<AtomicUsize>,
}
#[async_trait::async_trait]
impl HookRunner for Runner {
    async fn run(&self, input: &HookInvocation) -> anyhow::Result<RawOutcome> {
        self.count.fetch_add(1, Ordering::SeqCst);
        Ok((self.action)(input))
    }
}
fn runner(action: impl Fn(&HookInvocation) -> RawOutcome + Send + Sync + 'static) -> Arc<Runner> {
    Arc::new(Runner {
        action: Box::new(action),
        count: Arc::new(AtomicUsize::new(0)),
    })
}
fn output(value: Value) -> RawOutcome {
    RawOutcome::Callback { value }
}
fn registration(name: &str, class: HandlerClass, run: Arc<dyn HookRunner>) -> Registration {
    Registration {
        declaration: Declaration {
            identity: DeclarationIdentity {
                package: name.into(),
                code: "code".into(),
                policy: "policy".into(),
                configuration: "configuration".into(),
                generation: "generation-1".into(),
                scope: Scope::Project,
                role: "worker".into(),
                declaration: name.into(),
                index: 0,
                dialect: HookDialect::Native,
                runner: HandlerKind::Command,
            },
            class,
            priority: 0,
            matcher: Matcher::default(),
            reads: GateReadSet::default(),
            concurrent_group: None,
            read_only_endpoint: None,
            external_precondition: None,
        },
        runner: run,
        revalidation: None,
    }
}
fn write(id: &str, path: &str) -> ToolCall {
    ToolCall {
        id: id.into(),
        name: "write".into(),
        arguments: json!({"path":path,"content":"written"}),
    }
}
static FIXTURE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
struct Fixture {
    root: tempfile::TempDir,
    runtime: SharedRuntime,
    events: EventSink,
    _receiver: mpsc::Receiver<Envelope>,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let connection: Connection =
            serde_json::from_value(json!({"adapter":"openai-api","model":"fixture-main-model"}))
                .unwrap();
        let (runtime, _) = SharedRuntime::open(root.path(), &connection, None).unwrap();
        let (sender, receiver) = mpsc::channel(256);
        let events = EventSink::new("fixture".into(), sender, None)
            .unwrap()
            .with_runtime(runtime.clone());
        Self {
            root,
            runtime,
            events,
            _receiver: receiver,
        }
    }
    fn executor(&self, event: HookEvent, registrations: Vec<Registration>) -> ToolExecutor {
        let mut executor = ToolExecutor::new(self.root.path()).unwrap();
        executor
            .register_post_tool_plan(Arc::new(PostToolPlan::new(event, registrations).unwrap()))
            .unwrap();
        executor
    }
    async fn run(
        &self,
        executor: ToolExecutor,
        calls: Vec<ToolCall>,
    ) -> (anyhow::Result<TurnEnd>, Vec<ToolResult>, usize) {
        let results = Arc::new(Mutex::new(Vec::new()));
        let requests = Arc::new(AtomicUsize::new(0));
        let model = Responses {
            calls: VecDeque::from([calls]),
            results: results.clone(),
            requests: requests.clone(),
        };
        let mut session = NativeSession::with_tools(Box::new(model), executor);
        let (_sender, mut commands) = mpsc::channel(4);
        let end = tokio::time::timeout(
            std::time::Duration::from_secs(15),
            session.turn("test".into(), &mut commands, &self.events),
        )
        .await
        .expect("fixture turn timeout");
        let retained = results.lock().unwrap().clone();
        (end, retained, requests.load(Ordering::SeqCst))
    }
    fn record(&self) -> Record {
        self.runtime.record().unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(self.runtime.directory().unwrap()).unwrap();
    }
}
#[tokio::test]
async fn completed_write_is_original_evidence_before_post_replacement() {
    let _lock = FIXTURE.lock().await;
    let f = Fixture::new();
    let runtime = f.runtime.clone();
    let hook = runner(move |invocation| {
        assert_eq!(invocation.key.event, "PostToolUse");
        let record = runtime.record().unwrap();
        let operation = record
            .operations
            .iter()
            .find(|o| o.id == invocation.key.operation)
            .unwrap();
        let original = operation
            .result
            .as_ref()
            .expect("original retained before post runner");
        assert!(original.success);
        assert_eq!(original.output, "Wrote 7 bytes to created");
        output(
            json!({"hookSpecificOutput":{"hookEventName":"PostToolUse","updatedToolOutput":"model view"}}),
        )
    });
    let executor = f.executor(
        HookEvent::PostToolUse,
        vec![registration(
            "replacement",
            HandlerClass::Combined,
            hook.clone(),
        )],
    );
    let (end, results, requests) = f.run(executor, vec![write("one", "created")]).await;
    assert!(matches!(end.unwrap(), TurnEnd::Complete));
    assert_eq!(hook.count.load(Ordering::SeqCst), 1);
    assert_eq!(requests, 2);
    assert_eq!(
        std::fs::read_to_string(f.root.path().join("created")).unwrap(),
        "written"
    );
    assert!(results[0].output.starts_with("model view\n"));
    assert!(
        results[0]
            .output
            .contains("[Plugin-origin replacement \"replace_model_output\"]")
    );
    assert!(results[0].output.contains("replacement"));
    let record = f.record();
    let operation = record.operations.iter().find(|o| o.call.is_some()).unwrap();
    assert_eq!(
        operation.result.as_ref().unwrap().output,
        "Wrote 7 bytes to created"
    );
    assert!(
        operation
            .tool_receipt
            .as_ref()
            .unwrap()
            .plugin_lifecycle
            .as_ref()
            .unwrap()
            .settled
    );
}
#[tokio::test]
async fn continuation_hold_preserves_write_and_stops_second_tool_and_model() {
    let _lock = FIXTURE.lock().await;
    let f = Fixture::new();
    let hook =
        runner(|_| output(json!({"continue":false,"stopReason":"inspect completed mutation"})));
    let executor = f.executor(
        HookEvent::PostToolUse,
        vec![registration("hold", HandlerClass::Combined, hook.clone())],
    );
    let (end, results, requests) = f
        .run(
            executor,
            vec![write("one", "created"), write("two", "forbidden")],
        )
        .await;
    assert!(end.is_err(), "required post hold must end unmet");
    assert_eq!(hook.count.load(Ordering::SeqCst), 1);
    assert_eq!(requests, 1);
    assert!(f.root.path().join("created").exists());
    assert!(!f.root.path().join("forbidden").exists());
    assert!(
        results[0].success,
        "post objection cannot replace actual success"
    );
    assert!(
        f.record()
            .operations
            .iter()
            .find(|o| o.call.is_some())
            .unwrap()
            .result
            .as_ref()
            .unwrap()
            .success
    );
}
#[tokio::test]
async fn admitted_missing_read_has_failure_event_without_started_effect() {
    let _lock = FIXTURE.lock().await;
    let f = Fixture::new();
    let hook = runner(|invocation| {
        assert_eq!(invocation.key.event, "PostToolUseFailure");
        output(json!({}))
    });
    let executor = f.executor(
        HookEvent::PostToolUseFailure,
        vec![registration(
            "failure",
            HandlerClass::Observer,
            hook.clone(),
        )],
    );
    let (end, results, requests) = f
        .run(
            executor,
            vec![ToolCall {
                id: "missing".into(),
                name: "read".into(),
                arguments: json!({"path":"absent"}),
            }],
        )
        .await;
    assert!(matches!(end.unwrap(), TurnEnd::Complete));
    assert_eq!(hook.count.load(Ordering::SeqCst), 1);
    assert!(!results[0].success);
    assert_eq!(requests, 2);
    let record = f.record();
    let receipt = record
        .operations
        .iter()
        .find_map(|o| o.tool_receipt.as_ref())
        .unwrap();
    assert!(receipt.admitted);
    assert!(!receipt.effect_started);
}
#[tokio::test]
async fn pretool_denial_does_not_invent_an_executed_failure_event() {
    let _lock = FIXTURE.lock().await;
    let f = Fixture::new();
    let post = runner(|_| output(json!({})));
    let mut executor = f.executor(
        HookEvent::PostToolUseFailure,
        vec![registration("post", HandlerClass::Observer, post.clone())],
    );
    let pre = runner(|_| {
        output(
            json!({"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny"}}),
        )
    });
    executor
        .register_pre_tool_plan(Arc::new(
            PreToolPlan::new(vec![registration("pre", HandlerClass::DecisionGate, pre)]).unwrap(),
        ))
        .unwrap();
    let (end, results, _) = f.run(executor, vec![write("one", "forbidden")]).await;
    assert!(matches!(end.unwrap(), TurnEnd::Complete));
    assert!(!results[0].success);
    assert_eq!(post.count.load(Ordering::SeqCst), 0);
    assert!(!f.root.path().join("forbidden").exists());
}

#[path = "plugin_post_tool/runners.rs"]
mod runners;

#[test]
fn claude_falsy_replacements_are_ignored_but_empty_arrays_are_truthy() {
    use demoncoder::plugins::{
        profile::CompatibilityProfile,
        results::{self, HookResponse, ProposedEffect, ResultContext},
    };
    let profile = CompatibilityProfile::embedded().unwrap();
    for (value, replaces) in [
        (Value::Null, false),
        (json!(false), false),
        (json!(0), false),
        (json!(""), false),
        (json!([]), true),
        (json!("replacement"), true),
    ] {
        let response = json!({"hookSpecificOutput":{"hookEventName":"PostToolUse","updatedMCPToolOutput":value}});
        let decoded = results::decode_response(
            &profile,
            HookDialect::Claude,
            HookEvent::PostToolUse,
            HandlerKind::Command,
            &ResultContext {
                tool_is_mcp: true,
                ..Default::default()
            },
            HookResponse::Callback(&response),
        );
        assert!(!decoded.failed());
        assert_eq!(
            decoded
                .effects
                .iter()
                .any(|effect| matches!(effect, ProposedEffect::ReplaceModelOutput { .. })),
            replaces,
            "{value}"
        );
    }
}

#[path = "plugin_post_tool/claude.rs"]
mod claude;
#[path = "plugin_post_tool/external_correction.rs"]
mod external_correction;

#[tokio::test]
async fn post_model_correction_requires_task_and_allocation_and_never_accepts_work() {
    use demoncoder::workflow::{allocation::Limits, state::Task, workspace};
    let _lock = FIXTURE.lock().await;
    for dialect in [HookDialect::Native, HookDialect::Claude] {
        for authority in ["available", "absent", "exhausted", "allocation", "stale"] {
            let f = Fixture::new();
            f.runtime
                .allocate(
                    Limits {
                        seconds: 30,
                        model_calls: 16,
                        tool_calls: if authority == "allocation" { 1 } else { 16 },
                    },
                    None,
                )
                .unwrap();
            if authority != "absent" {
                let task = Task::new(
                    1,
                    "real task".into(),
                    vec![],
                    workspace::capture(f.root.path()).unwrap(),
                    if authority == "exhausted" { 0 } else { 2 },
                )
                .unwrap();
                f.runtime.save_task(&Some(task), 2, None).unwrap();
            }
            let watched = f.root.path().join("watched");
            std::fs::write(&watched, "original").unwrap();
            let hook = runner(move |_| {
                if authority == "stale" {
                    std::fs::write(&watched, "changed").unwrap();
                }
                RawOutcome::Model {
                    value: json!({"ok":false,"reason":"needs correction"}),
                    continue_on_block: dialect == HookDialect::Claude,
                }
            });
            let mut registration = registration("negative-model", HandlerClass::DecisionGate, hook);
            registration.declaration.identity.runner = HandlerKind::Prompt;
            registration.declaration.reads =
                GateReadSet::new(vec!["watched".into()], vec![], vec![]).unwrap();
            registration.declaration.identity.dialect = dialect;
            if dialect == HookDialect::Claude {
                registration.declaration.concurrent_group = Some("source".into());
                registration.declaration.class = HandlerClass::Combined;
            }
            let (end, results, requests) = f
                .run(
                    f.executor(HookEvent::PostToolUse, vec![registration]),
                    vec![write("one", "created"), write("two", "second")],
                )
                .await;
            assert_eq!(
                end.is_ok(),
                authority == "available",
                "{dialect:?}/{authority}: {:?}",
                end.err()
            );
            assert!(results[0].success);
            assert_eq!(requests, if authority == "available" { 2 } else { 1 });
            if let Some(task) = f.record().task {
                assert!(task.accepted.is_none());
                assert_eq!(
                    task.corrections,
                    if authority == "available" {
                        if dialect == HookDialect::Claude { 2 } else { 1 }
                    } else {
                        0
                    }
                );
            }
            if dialect == HookDialect::Native {
                assert!(!f.root.path().join("second").exists());
            }
        }
    }
}

#[tokio::test]
async fn observer_failure_is_visible_without_forcing_extra_model_work() {
    let _lock = FIXTURE.lock().await;
    let f = Fixture::new();
    let hook = runner(|_| output(json!({"unexpected":"malformed"})));
    let (end, results, requests) = f
        .run(
            f.executor(
                HookEvent::PostToolUse,
                vec![registration("observer", HandlerClass::Observer, hook)],
            ),
            vec![write("one", "created")],
        )
        .await;
    assert!(end.is_ok());
    assert_eq!(requests, 2);
    assert!(results[0].success);
    assert!(
        !f.record()
            .operations
            .iter()
            .find_map(|o| o.tool_receipt.as_ref()?.plugin_lifecycle.as_ref())
            .unwrap()
            .diagnostics
            .is_empty()
    );
}

#[tokio::test]
async fn outer_workflow_save_preserves_post_admitted_correction() {
    use demoncoder::workflow::{
        Settings, WorkflowSession, allocation::Limits, state::Task, workspace,
    };
    let _lock = FIXTURE.lock().await;
    let f = Fixture::new();
    f.runtime.allocate(Limits::default(), None).unwrap();
    let task = Task::new(
        1,
        "persist correction".into(),
        vec![],
        workspace::capture(f.root.path()).unwrap(),
        2,
    )
    .unwrap();
    f.runtime.save_task(&Some(task), 2, None).unwrap();
    let hook = runner(|_| RawOutcome::Model {
        value: json!({"ok":false,"reason":"correct completed work"}),
        continue_on_block: false,
    });
    let mut registration = registration("correct", HandlerClass::DecisionGate, hook);
    registration.declaration.identity.runner = HandlerKind::Prompt;
    let results = Arc::new(Mutex::new(Vec::new()));
    let requests = Arc::new(AtomicUsize::new(0));
    let model = Responses {
        calls: VecDeque::from([vec![write("one", "created")]]),
        results,
        requests,
    };
    let inner = NativeSession::with_tools(
        Box::new(model),
        f.executor(HookEvent::PostToolUse, vec![registration]),
    );
    let connection: Connection =
        serde_json::from_value(json!({"adapter":"openai-api","model":"fixture-main-model"}))
            .unwrap();
    let mut outer = WorkflowSession::new(
        Box::new(inner),
        connection,
        f.root.path().to_path_buf(),
        Settings::default(),
        f.runtime.clone(),
        false,
    )
    .unwrap();
    let (_sender, mut commands) = mpsc::channel(4);
    let end = outer
        .turn("continue task".into(), &mut commands, &f.events)
        .await;
    assert!(end.is_ok(), "{:?}", end.err());
    let record = f.record();
    let task = record.task.unwrap();
    assert_eq!(task.corrections, 1);
    assert!(task.stopped);
    assert!(task.accepted.is_none());
}

#[tokio::test]
async fn verification_receipt_uses_original_evidence_and_releases_local_gate() {
    use demoncoder::workflow::{
        Settings, WorkflowSession, allocation::Limits, state::Task, workspace,
    };
    let _lock = FIXTURE.lock().await;
    let f = Fixture::new();
    f.runtime.allocate(Limits::default(), None).unwrap();
    let mut task = Task::new(
        1,
        "verify original".into(),
        vec!["printf original-check-output".into()],
        workspace::capture(f.root.path()).unwrap(),
        2,
    )
    .unwrap();
    task.stopped = true;
    f.runtime.save_task(&Some(task), 2, None).unwrap();
    let hook = runner(|_| {
        output(
            json!({"hookSpecificOutput":{"hookEventName":"PostToolUse","updatedToolOutput":"forged-check-output"}}),
        )
    });
    let mut registration = registration("verification-presentation", HandlerClass::Combined, hook);
    registration.declaration.identity.role = "verification".into();
    let mut connection: Connection =
        serde_json::from_value(json!({"adapter":"openai-api","model":"fixture-main-model"}))
            .unwrap();
    connection.access.post_tools.push(Arc::new(
        PostToolPlan::new(HookEvent::PostToolUse, vec![registration]).unwrap(),
    ));
    connection.access.supervisor = Some(env!("CARGO_BIN_EXE_demoncoder").into());
    let inner = NativeSession::with_tools(
        Box::new(Responses {
            calls: VecDeque::new(),
            results: Arc::new(Mutex::new(Vec::new())),
            requests: Arc::new(AtomicUsize::new(0)),
        }),
        ToolExecutor::new(f.root.path()).unwrap(),
    );
    let mut outer = WorkflowSession::new(
        Box::new(inner),
        connection,
        f.root.path().to_path_buf(),
        Settings::default(),
        f.runtime.clone(),
        false,
    )
    .unwrap();
    let (_sender, mut commands) = mpsc::channel(4);
    let end = outer.turn("/verify".into(), &mut commands, &f.events).await;
    assert!(end.is_ok(), "{:?}", end.err());
    let record = f.record();
    let task = record.task.unwrap();
    assert_eq!(task.checks.len(), 1);
    assert!(task.checks[0].success);
    assert_eq!(task.checks[0].output, "original-check-output");
    assert!(task.accepted.is_none());
    let op = record.operations.iter().find(|o| o.call.is_some()).unwrap();
    assert_eq!(op.result.as_ref().unwrap().output, "original-check-output");
    assert!(
        op.model_result()
            .unwrap()
            .output
            .starts_with("forged-check-output\n")
    );
    assert!(
        op.model_result()
            .unwrap()
            .output
            .contains("[Plugin-origin verification-presentation")
    );
    assert_eq!(
        op.tool_receipt
            .as_ref()
            .unwrap()
            .plugin_lifecycle
            .as_ref()
            .unwrap()
            .delivery,
        demoncoder::plugins::receipts::PostDelivery::Local
    );
}

#[tokio::test]
async fn observer_explicit_continuation_hold_cannot_reverse_completed_success() {
    let _lock = FIXTURE.lock().await;
    let f = Fixture::new();
    let hook = runner(|_| {
        output(
            json!({"continue":false,"stopReason":"inspect the completed result before continuing"}),
        )
    });
    let (end, results, requests) = f
        .run(
            f.executor(
                HookEvent::PostToolUse,
                vec![registration(
                    "observer-control",
                    HandlerClass::Observer,
                    hook,
                )],
            ),
            vec![write("one", "created"), write("two", "forbidden")],
        )
        .await;
    assert!(end.is_err());
    assert_eq!(requests, 1);
    assert!(results[0].success);
    assert_eq!(
        std::fs::read_to_string(f.root.path().join("created")).unwrap(),
        "written"
    );
    assert!(!f.root.path().join("forbidden").exists());
    assert!(
        f.record()
            .operations
            .iter()
            .find(|o| o.call.is_some())
            .unwrap()
            .result
            .as_ref()
            .unwrap()
            .success
    );
}

#[tokio::test]
async fn oracle_denied_bash_is_not_an_executed_tool_failure() {
    let _lock = FIXTURE.lock().await;
    let f = Fixture::new();
    std::fs::write(f.root.path().join("oracle-mode"), "deny").unwrap();
    let oracle:Connection=serde_json::from_value(json!({"adapter":"claude","model":"fixture-model","binary":std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/oracle_fixture.py")})).unwrap();
    let mut tools = ToolExecutor::with_policy(
        f.root.path(),
        &demoncoder::tools::AccessPolicy {
            unrestricted: true,
            oracle: Some(Box::new(oracle)),
            supervisor: Some(env!("CARGO_BIN_EXE_demoncoder").into()),
            ..Default::default()
        },
    )
    .unwrap();
    let post = runner(|_| output(json!({})));
    tools
        .register_post_tool_plan(Arc::new(
            PostToolPlan::new(
                HookEvent::PostToolUseFailure,
                vec![registration(
                    "executed-failure",
                    HandlerClass::Observer,
                    post.clone(),
                )],
            )
            .unwrap(),
        ))
        .unwrap();
    let (end, results, _) = f
        .run(
            tools,
            vec![ToolCall {
                id: "oracle-denial".into(),
                name: "bash".into(),
                arguments: json!({"command":"printf forbidden > forbidden"}),
            }],
        )
        .await;
    assert!(end.is_ok(), "{:?}", end.err());
    assert!(!results[0].success);
    assert!(!f.root.path().join("forbidden").exists());
    assert_eq!(
        post.count.load(Ordering::SeqCst),
        0,
        "policy refusal fabricated an executed failure"
    );
    let record = f.record();
    let receipt = record
        .operations
        .iter()
        .find(|o| o.call.as_ref().is_some_and(|c| c.id == "oracle-denial"))
        .unwrap()
        .tool_receipt
        .as_ref()
        .unwrap();
    assert!(!receipt.admitted);
    assert!(!receipt.effect_started);
    assert!(receipt.plugin_lifecycle.is_none());
}
