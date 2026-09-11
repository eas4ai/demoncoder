use demoncoder::{
    config::Connection,
    events::EventSink,
    native::{Model, NativeSession},
    plugins::{
        dispatch::*,
        gate_snapshot::GateReadSet,
        hook_types::{HandlerKind, HookDialect},
    },
    session::Session,
    tools::{ToolCall, ToolExecutor, ToolHook, ToolResult},
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
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};
use tokio::sync::{Barrier, Semaphore, mpsc};
struct OwnedTask<T>(tokio::task::JoinHandle<T>);
impl<T> Drop for OwnedTask<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}
impl<T> Future for OwnedTask<T> {
    type Output = Result<T, tokio::task::JoinError>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.0).poll(cx)
    }
}
impl<T> OwnedTask<T> {
    fn abort(&self) {
        self.0.abort();
    }
}
fn spawn_owned<F: Future + Send + 'static>(future: F) -> OwnedTask<F::Output>
where
    F::Output: Send + 'static,
{
    OwnedTask(tokio::spawn(future))
}
async fn signal(semaphore: &Semaphore) {
    tokio::time::timeout(Duration::from_secs(5), semaphore.acquire())
        .await
        .expect("fixture signal timeout")
        .unwrap()
        .forget();
}

static FIXTURE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
struct Responses {
    calls: VecDeque<Vec<ToolCall>>,
    results: Arc<Mutex<Vec<ToolResult>>>,
}
#[async_trait::async_trait]
impl Model for Responses {
    fn prompt(&mut self, _: String) {}
    fn results(&mut self, results: Vec<ToolResult>) {
        self.results.lock().unwrap().extend(results);
    }
    async fn response(&mut self, _: &EventSink) -> anyhow::Result<Vec<ToolCall>> {
        Ok(self.calls.pop_front().unwrap_or_default())
    }
}
type Action = dyn Fn(&HookInvocation) -> RawOutcome + Send + Sync;
struct Runner {
    action: Box<Action>,
    count: Arc<AtomicUsize>,
    barrier: Option<Arc<Barrier>>,
    entered: Option<Arc<Semaphore>>,
    release: Option<Arc<Semaphore>>,
}
#[async_trait::async_trait]
impl HookRunner for Runner {
    async fn run(&self, input: &HookInvocation) -> anyhow::Result<RawOutcome> {
        self.count.fetch_add(1, Ordering::SeqCst);
        if let Some(barrier) = &self.barrier {
            tokio::time::timeout(Duration::from_secs(5), barrier.wait()).await?;
        }
        let result = (self.action)(input);
        if let Some(entered) = &self.entered {
            entered.add_permits(1);
        }
        if let Some(release) = &self.release {
            signal(release).await;
        }
        Ok(result)
    }
}
fn runner(action: impl Fn(&HookInvocation) -> RawOutcome + Send + Sync + 'static) -> Arc<Runner> {
    Arc::new(Runner {
        action: Box::new(action),
        count: Arc::new(AtomicUsize::new(0)),
        barrier: None,
        entered: None,
        release: None,
    })
}
fn output(value: Value) -> RawOutcome {
    RawOutcome::Callback { value }
}
fn decision(choice: &str) -> RawOutcome {
    output(json!({"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":choice}}))
}
fn rewrite(input: &HookInvocation, path: &str) -> RawOutcome {
    let mut arguments = input
        .candidate
        .as_ref()
        .expect("tool invocation")
        .arguments
        .clone();
    arguments["path"] = json!(path);
    output(json!({"hookSpecificOutput":{"hookEventName":"PreToolUse","updatedInput":arguments}}))
}
fn registration(name: &str, class: HandlerClass, run: Arc<dyn HookRunner>) -> Registration {
    Registration {
        declaration: Declaration {
            required_gate: class != HandlerClass::Observer,
            source: None,
            once: None,
            identity: DeclarationIdentity {
                package: name.into(),
                code: "code-sha256".into(),
                policy: "policy-sha256".into(),
                configuration: "configuration-sha256".into(),
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
            matcher: Matcher {
                tool: Some("write".into()),
                path: None,
            },
            reads: GateReadSet::default(),
            concurrent_group: None,
            read_only_endpoint: None,
            external_precondition: None,
        },
        runner: run,
        revalidation: None,
    }
}
fn call(path: &str) -> ToolCall {
    ToolCall {
        id: "source-1".into(),
        name: "write".into(),
        arguments: json!({"path":path,"content":"written"}),
    }
}
struct Fixture {
    root: tempfile::TempDir,
    runtime: SharedRuntime,
    events: EventSink,
    _receiver: mpsc::Receiver<demoncoder::events::Envelope>,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let connection: Connection =
            serde_json::from_value(json!({"adapter":"openai-api"})).unwrap();
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
    fn executor(&self, registrations: Vec<Registration>) -> ToolExecutor {
        let mut executor = ToolExecutor::new(self.root.path()).unwrap();
        executor
            .register_pre_tool_plan(Arc::new(PreToolPlan::new(registrations).unwrap()))
            .unwrap();
        executor
    }
    async fn run(&self, executor: ToolExecutor, calls: Vec<ToolCall>) -> Vec<ToolResult> {
        execute(executor, calls, self.events.clone()).await
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
async fn execute(
    executor: ToolExecutor,
    calls: Vec<ToolCall>,
    events: EventSink,
) -> Vec<ToolResult> {
    let results = Arc::new(Mutex::new(Vec::new()));
    let model = Responses {
        calls: VecDeque::from([calls]),
        results: results.clone(),
    };
    let mut session = NativeSession::with_tools(Box::new(model), executor);
    let (_sender, mut commands) = mpsc::channel(4);
    tokio::time::timeout(
        Duration::from_secs(15),
        session.turn("test".into(), &mut commands, &events),
    )
    .await
    .expect("fixture turn timeout")
    .unwrap();
    results.lock().unwrap().clone()
}
struct Rewriter;
impl ToolHook for Rewriter {
    fn before(&self, call: &mut ToolCall) -> anyhow::Result<()> {
        call.arguments["path"] = json!("generated");
        Ok(())
    }
}
#[tokio::test]
async fn final_candidate_generated_policy_blocks_actual_write() {
    let _lock = FIXTURE.lock().await;
    for reverse in [false, true] {
        let f = Fixture::new();
        let gate = registration(
            "policy",
            HandlerClass::DecisionGate,
            runner(|i| {
                decision(
                    if i.candidate.as_ref().expect("tool invocation").arguments["path"]
                        == "generated"
                    {
                        "deny"
                    } else {
                        "allow"
                    },
                )
            }),
        );
        let transform = registration(
            "rewrite",
            HandlerClass::Transformer,
            runner(|i| rewrite(i, "generated")),
        );
        let declarations = if reverse {
            vec![transform, gate]
        } else {
            vec![gate, transform]
        };
        let result = f.run(f.executor(declarations), vec![call("source")]).await;
        assert!(
            !result[0].success,
            "final generated-file candidate must be denied"
        );
        assert!(!f.root.path().join("generated").exists());
        let record = f.record();
        let receipt = record
            .operations
            .iter()
            .find_map(|o| o.tool_receipt.as_ref())
            .unwrap()
            .plugin_admission
            .as_ref()
            .unwrap();
        assert!(receipt.final_key.is_some());
        assert!(receipt.hold.is_some());
    }
}
#[tokio::test]
async fn generic_hooks_finish_before_final_plugin_decisions() {
    let _lock = FIXTURE.lock().await;
    let f = Fixture::new();
    let mut executor = f.executor(vec![registration(
        "policy",
        HandlerClass::DecisionGate,
        runner(|i| {
            decision(
                if i.candidate.as_ref().expect("tool invocation").arguments["path"] == "generated" {
                    "deny"
                } else {
                    "allow"
                },
            )
        }),
    )]);
    executor.add_hook(Box::new(Rewriter));
    let result = f.run(executor, vec![call("source"), call("source")]).await;
    assert_eq!(result.len(), 2);
    assert!(!result[0].success);
    assert!(!f.root.path().join("generated").exists());
    let record = f.record();
    let operation = record
        .operations
        .iter()
        .find(|o| o.tool_receipt.is_some())
        .unwrap();
    assert_eq!(
        operation.call.as_ref().unwrap().arguments["path"],
        "generated"
    );
    assert_eq!(
        operation
            .tool_receipt
            .as_ref()
            .unwrap()
            .original_call
            .arguments["path"],
        "source"
    );
}
#[tokio::test]
async fn valid_rewrite_writes_and_duplicate_request_does_not_replay_handlers() {
    let _lock = FIXTURE.lock().await;
    let f = Fixture::new();
    let gate = runner(|_| decision("allow"));
    let count = gate.count.clone();
    let executor = f.executor(vec![
        registration(
            "rewrite",
            HandlerClass::Transformer,
            runner(|i| rewrite(i, "final")),
        ),
        registration("policy", HandlerClass::DecisionGate, gate),
    ]);
    let result = f.run(executor, vec![call("source"), call("source")]).await;
    assert!(result.iter().all(|r| r.success));
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert_eq!(
        std::fs::read_to_string(f.root.path().join("final")).unwrap(),
        "written"
    );
    assert!(!f.root.path().join("source").exists());
}

#[tokio::test]
async fn final_schema_and_path_policy_prevent_creation() {
    let _lock = FIXTURE.lock().await;
    for arguments in [
        json!({"path":"bad-schema"}),
        json!({"path":"../outside-plugin-test","content":"bad"}),
        json!({"path":".demoncoder/private","content":"bad"}),
        json!({"path":"new-file","content":"ok","extra":true}),
    ] {
        let f = Fixture::new();
        let proposed = arguments.clone();
        let executor=f.executor(vec![registration("transform",HandlerClass::Transformer,runner(move |_|output(json!({"hookSpecificOutput":{"hookEventName":"PreToolUse","updatedInput":proposed}})))),registration("gate",HandlerClass::DecisionGate,runner(|_|decision("allow")))]);
        let result = f.run(executor, vec![call("original")]).await;
        assert!(!result[0].success, "{result:?}");
        assert!(!f.root.path().join("original").exists());
        assert!(!f.root.path().join("new-file").exists());
        assert!(!f.root.path().join("bad-schema").exists());
    }
}
#[tokio::test]
async fn deny_dominates_rewrite_allow_and_question() {
    let _lock = FIXTURE.lock().await;
    let f = Fixture::new();
    let executor = f.executor(vec![
        registration(
            "a-deny",
            HandlerClass::Combined,
            runner(|_| decision("deny")),
        ),
        registration(
            "rewrite",
            HandlerClass::Transformer,
            runner(|i| rewrite(i, "rewritten")),
        ),
        registration(
            "z-allow",
            HandlerClass::DecisionGate,
            runner(|_| decision("allow")),
        ),
        registration(
            "z-question",
            HandlerClass::DecisionGate,
            runner(|_| decision("ask")),
        ),
    ]);
    let result = f.run(executor, vec![call("original")]).await;
    assert!(!result[0].success);
    assert!(!f.root.path().join("rewritten").exists());
    let record = f.record();
    let receipt = record
        .operations
        .iter()
        .find_map(|o| o.tool_receipt.as_ref())
        .unwrap()
        .plugin_admission
        .as_ref()
        .unwrap();
    assert!(
        receipt
            .hooks
            .iter()
            .flat_map(|h| &h.questions)
            .any(|q| q.choice == demoncoder::plugins::receipts::PendingDecision::Deny)
    );
}
#[tokio::test]
async fn stale_combined_handler_never_reexecutes_and_explicit_endpoint_can_revalidate() {
    let _lock = FIXTURE.lock().await;
    for endpoint in [false, true] {
        let f = Fixture::new();
        let effect = f.root.path().join("handler-effect");
        let combined = runner(move |_| {
            std::fs::write(&effect, "once").unwrap();
            decision("allow")
        });
        let count = combined.count.clone();
        let readonly = runner(|i| {
            assert_eq!(
                i.candidate.as_ref().expect("tool invocation").arguments["path"],
                "final"
            );
            decision("allow")
        });
        let readcount = readonly.count.clone();
        let mut registration = registration("a-combined", HandlerClass::Combined, combined);
        if endpoint {
            registration.declaration.read_only_endpoint = Some("validate-only".into());
            registration.revalidation = Some(readonly);
        }
        let executor = f.executor(vec![
            registration,
            crate::registration(
                "rewrite",
                HandlerClass::Transformer,
                runner(|i| rewrite(i, "final")),
            ),
        ]);
        let result = f.run(executor, vec![call("source")]).await;
        assert_eq!(result[0].success, endpoint, "{result:?}");
        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert_eq!(readcount.load(Ordering::SeqCst), usize::from(endpoint));
        assert_eq!(f.root.path().join("final").exists(), endpoint);
    }
}
#[tokio::test]
async fn path_matchers_are_recomputed_after_a_rewrite() {
    let _lock = FIXTURE.lock().await;
    let f = Fixture::new();
    let gate = runner(|_| decision("deny"));
    let count = gate.count.clone();
    let mut policy = registration("policy", HandlerClass::DecisionGate, gate);
    policy.declaration.matcher.path = Some("generated/**".into());
    std::fs::create_dir(f.root.path().join("generated")).unwrap();
    let executor = f.executor(vec![
        policy,
        registration(
            "rewrite",
            HandlerClass::Transformer,
            runner(|i| rewrite(i, "generated/new")),
        ),
    ]);
    let results = f.run(executor, vec![call("source")]).await;
    assert!(!results[0].success);
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert!(!f.root.path().join("generated/new").exists());
}
#[tokio::test]
async fn source_concurrent_group_starts_together_and_conflicting_rewrites_hold() {
    let _lock = FIXTURE.lock().await;
    let f = Fixture::new();
    let barrier = Arc::new(Barrier::new(2));
    let mut registrations = Vec::new();
    for (index, path) in [(0, "left"), (1, "right")] {
        let run = Arc::new(Runner {
            action: Box::new(move |i| {
                assert_eq!(
                    i.candidate.as_ref().expect("tool invocation").arguments["path"],
                    "source"
                );
                rewrite(i, path)
            }),
            count: Arc::new(AtomicUsize::new(0)),
            barrier: Some(barrier.clone()),
            entered: None,
            release: None,
        });
        let mut r = registration("package", HandlerClass::Combined, run);
        r.declaration.identity.index = index;
        r.declaration.identity.declaration = format!("handler-{index}");
        r.declaration.identity.dialect = HookDialect::Claude;
        r.declaration.concurrent_group = Some("source-group".into());
        registrations.push(r);
    }
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        f.run(f.executor(registrations), vec![call("source")]),
    )
    .await
    .expect("source group serialized/deadlocked");
    assert!(!result[0].success);
    for path in ["source", "left", "right"] {
        assert!(!f.root.path().join(path).exists());
    }
}
#[tokio::test]
async fn repeated_candidate_and_four_revision_limit_never_release_last_write() {
    let _lock = FIXTURE.lock().await;
    for cycle in [false, true] {
        let f = Fixture::new();
        let transform = runner(move |i| {
            let old = i.candidate.as_ref().expect("tool invocation").arguments["path"]
                .as_str()
                .unwrap();
            let path = if cycle {
                if old == "source" {
                    "other".into()
                } else {
                    "source".into()
                }
            } else {
                format!("{old}x")
            };
            rewrite(i, &path)
        });
        let count = transform.count.clone();
        let result = f
            .run(
                f.executor(vec![registration(
                    "transform",
                    HandlerClass::Transformer,
                    transform,
                )]),
                vec![call("source")],
            )
            .await;
        assert!(!result[0].success);
        assert!(count.load(Ordering::SeqCst) <= 4);
        assert_eq!(std::fs::read_dir(f.root.path()).unwrap().count(), 0);
    }
}
#[tokio::test]
async fn delayed_gate_observes_immutable_dirty_bytes_and_changed_inputs_hold() {
    let _lock = FIXTURE.lock().await;
    for change in ["bytes", "new", "absent", "glob", "mode", "acl", "unrelated"] {
        let f = Fixture::new();
        std::fs::write(f.root.path().join("input"), "dirty-untracked").unwrap();
        let entered = Arc::new(Semaphore::new(0));
        let release = Arc::new(Semaphore::new(0));
        let run = Arc::new(Runner {
            action: Box::new(|i| {
                assert_eq!(
                    i.snapshot.entry("input").unwrap().bytes(),
                    b"dirty-untracked"
                );
                decision("allow")
            }),
            count: Arc::new(AtomicUsize::new(0)),
            barrier: None,
            entered: Some(entered.clone()),
            release: Some(release.clone()),
        });
        let mut gate = registration("gate", HandlerClass::DecisionGate, run);
        if change == "unrelated" {
            gate.declaration.reads =
                GateReadSet::new(vec!["input".into()], vec![], vec![]).unwrap();
        }
        if change == "absent" {
            gate.declaration.reads =
                GateReadSet::new(vec!["input".into()], vec![], vec!["missing".into()]).unwrap();
        }
        if change == "glob" {
            gate.declaration.reads =
                GateReadSet::new(vec!["input".into()], vec!["*.policy".into()], vec![]).unwrap();
        }
        let executor = f.executor(vec![gate]);
        let events = f.events.clone();
        let pending =
            spawn_owned(async move { execute(executor, vec![call("output")], events).await });
        signal(&entered).await;
        match change {
            "bytes" => std::fs::write(f.root.path().join("input"), "different").unwrap(),
            "new" | "unrelated" => std::fs::write(f.root.path().join("unrelated"), "new").unwrap(),
            "absent" => std::fs::write(f.root.path().join("missing"), "new").unwrap(),
            "glob" => std::fs::write(f.root.path().join("new.policy"), "new").unwrap(),
            "acl" => {
                let file = std::fs::File::open(f.root.path().join("input")).unwrap();
                let mut acl = 2_u32.to_le_bytes().to_vec();
                for (tag, perm, id) in [
                    (1_u16, 6_u16, u32::MAX),
                    (2, 4, 65534),
                    (4, 4, u32::MAX),
                    (16, 4, u32::MAX),
                    (32, 0, u32::MAX),
                ] {
                    acl.extend(tag.to_le_bytes());
                    acl.extend(perm.to_le_bytes());
                    acl.extend(id.to_le_bytes());
                }
                rustix::fs::fsetxattr(
                    &file,
                    "system.posix_acl_access",
                    &acl,
                    rustix::fs::XattrFlags::empty(),
                )
                .unwrap();
            }
            "mode" => {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(
                    f.root.path().join("input"),
                    std::fs::Permissions::from_mode(0o400),
                )
                .unwrap();
            }
            _ => unreachable!(),
        }
        release.add_permits(1);
        let results = pending.await.unwrap();
        assert_eq!(
            results[0].success,
            change == "unrelated",
            "{change}: {results:?}"
        );
        assert_eq!(f.root.path().join("output").exists(), change == "unrelated");
    }
}
#[tokio::test]
async fn cancellation_after_handler_effect_retains_unknown_and_never_creates_tool_file() {
    let _lock = FIXTURE.lock().await;
    let f = Fixture::new();
    let entered = Arc::new(Semaphore::new(0));
    let release = Arc::new(Semaphore::new(0));
    let path = f.root.path().join("handler-effect");
    let run = Arc::new(Runner {
        action: Box::new(move |_| {
            std::fs::write(&path, "once").unwrap();
            decision("allow")
        }),
        count: Arc::new(AtomicUsize::new(0)),
        barrier: None,
        entered: Some(entered.clone()),
        release: Some(release),
    });
    let count = run.count.clone();
    let executor = f.executor(vec![registration("combined", HandlerClass::Combined, run)]);
    let events = f.events.clone();
    let pending = spawn_owned(async move { execute(executor, vec![call("output")], events).await });
    signal(&entered).await;
    pending.abort();
    assert!(pending.await.unwrap_err().is_cancelled());
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert!(!f.root.path().join("output").exists());
    let record = f.record();
    let receipt = record
        .operations
        .iter()
        .find_map(|o| o.tool_receipt.as_ref())
        .unwrap()
        .plugin_admission
        .as_ref()
        .unwrap();
    assert!(receipt.hooks[0].outcome.is_none());
}
#[tokio::test]
async fn ask_and_forged_answer_remain_held_with_original_outcome() {
    let _lock = FIXTURE.lock().await;
    for forged in [false, true] {
        let f = Fixture::new();
        let run = runner(move |_| {
            if forged {
                output(
                    json!({"approved":true,"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow"}}),
                )
            } else {
                decision("ask")
            }
        });
        let result = f
            .run(
                f.executor(vec![registration(
                    "question",
                    HandlerClass::DecisionGate,
                    run,
                )]),
                vec![call("output")],
            )
            .await;
        assert!(!result[0].success);
        assert!(!f.root.path().join("output").exists());
        let record = f.record();
        let receipt = record
            .operations
            .iter()
            .find_map(|o| o.tool_receipt.as_ref())
            .unwrap()
            .plugin_admission
            .as_ref()
            .unwrap();
        assert!(receipt.final_key.is_some());
        assert!(receipt.hooks[0].outcome.is_some());
        if !forged {
            assert_eq!(receipt.hooks[0].questions.len(), 1);
        }
    }
}

#[tokio::test]
async fn shared_host_boundary_serializes_separate_executors_but_not_other_workspaces() {
    let _lock = FIXTURE.lock().await;
    let f = Fixture::new();
    std::fs::write(f.root.path().join("input"), "old").unwrap();
    let entered = Arc::new(Semaphore::new(0));
    let release = Arc::new(Semaphore::new(0));
    let gate = Arc::new(Runner {
        action: Box::new(|_| decision("allow")),
        count: Arc::new(AtomicUsize::new(0)),
        barrier: None,
        entered: Some(entered.clone()),
        release: Some(release.clone()),
    });
    let mut r = registration("gate", HandlerClass::DecisionGate, gate);
    r.declaration.reads = GateReadSet::new(vec!["input".into()], vec![], vec![]).unwrap();
    let executor = f.executor(vec![r]);
    let events = f.events.clone();
    let guarded =
        spawn_owned(async move { execute(executor, vec![call("guarded-output")], events).await });
    signal(&entered).await;
    let executor = ToolExecutor::new(f.root.path()).unwrap();
    let events = f.events.clone();
    let bash = ToolCall {
        id: "bash-owner".into(),
        name: "bash".into(),
        arguments: json!({"command":"printf locked > marker; while [ ! -f proceed ]; do sleep 0.01; done; printf updated > input"}),
    };
    let mut owner = spawn_owned(async move { execute(executor, vec![bash], events).await });
    tokio::time::timeout(std::time::Duration::from_secs(3),async {
        while !f.root.path().join("marker").exists() {
            tokio::select! { result=&mut owner=>panic!("boundary fixture Bash did not start: {result:?}"), _=tokio::time::sleep(std::time::Duration::from_millis(5))=>{} }
        }
    }).await.unwrap();
    release.add_permits(1);
    let other = tempfile::tempdir().unwrap();
    let independent = ToolExecutor::new(other.path()).unwrap();
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        f.run(independent, vec![call("independent")]),
    )
    .await
    .expect("independent workspace blocked behind Bash");
    assert!(result[0].success);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(
        !f.root.path().join("guarded-output").exists(),
        "guarded tool crossed the shared host boundary"
    );
    std::fs::write(f.root.path().join("proceed"), "go").unwrap();
    assert!(owner.await.unwrap()[0].success);
    let results = guarded.await.unwrap();
    assert!(
        !results[0].success,
        "stale pass crossed the final boundary: {results:?}"
    );
    assert!(!f.root.path().join("guarded-output").exists());
}
#[tokio::test]
async fn unsupported_external_atomic_preconditions_and_wrong_role_hold_without_runner_effects() {
    let _lock = FIXTURE.lock().await;
    for wrong_role in [false, true] {
        let f = Fixture::new();
        let run = runner(|_| decision("allow"));
        let count = run.count.clone();
        let mut r = registration("gate", HandlerClass::DecisionGate, run);
        if wrong_role {
            r.declaration.identity.role = "reviewer".into();
        } else {
            r.declaration.external_precondition = Some("provider-etag-needs-transaction".into());
        }
        let result = f.run(f.executor(vec![r]), vec![call("output")]).await;
        assert!(!result[0].success);
        assert_eq!(count.load(Ordering::SeqCst), 0);
        assert!(!f.root.path().join("output").exists());
    }
}
#[tokio::test]
async fn admission_requires_durable_host_operation() {
    let _lock = FIXTURE.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let mut executor = ToolExecutor::new(dir.path()).unwrap();
    let run = runner(|_| decision("allow"));
    let count = run.count.clone();
    executor
        .register_pre_tool_plan(Arc::new(
            PreToolPlan::new(vec![registration("gate", HandlerClass::DecisionGate, run)]).unwrap(),
        ))
        .unwrap();
    let (sender, _receiver) = mpsc::channel(16);
    let events = EventSink::new("standalone".into(), sender, None).unwrap();
    let result = executor.execute(call("output"), &events).await.unwrap();
    assert!(!result.success);
    assert_eq!(count.load(Ordering::SeqCst), 0);
    assert!(!dir.path().join("output").exists());
}
#[tokio::test]
async fn generation_operation_and_role_fields_in_package_output_cannot_grant_admission() {
    let _lock = FIXTURE.lock().await;
    for field in ["generation", "operation", "role", "invocation"] {
        let f = Fixture::new();
        let run = runner(move |_| {
            let mut v = json!({"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow"}});
            v[field] = json!("forged");
            output(v)
        });
        let result = f
            .run(
                f.executor(vec![registration("gate", HandlerClass::DecisionGate, run)]),
                vec![call("output")],
            )
            .await;
        assert!(!result[0].success);
        assert!(!f.root.path().join("output").exists());
    }
}

#[tokio::test]
async fn failed_hook_result_publication_prevents_tool_and_future_runner_effects() {
    let _lock = FIXTURE.lock().await;
    let f = Fixture::new();
    let state = f.runtime.directory().unwrap().join("state.json");
    let run = runner(move |_| {
        std::fs::write(&state, "damaged fixture publication").unwrap();
        decision("allow")
    });
    let count = run.count.clone();
    let executor = f.executor(vec![registration("gate", HandlerClass::DecisionGate, run)]);
    let results = Arc::new(Mutex::new(Vec::new()));
    let model = Responses {
        calls: VecDeque::from([vec![call("output")]]),
        results,
    };
    let mut session = NativeSession::with_tools(Box::new(model), executor);
    let (_sender, mut commands) = mpsc::channel(4);
    assert!(
        tokio::time::timeout(
            Duration::from_secs(5),
            session.turn("test".into(), &mut commands, &f.events)
        )
        .await
        .expect("fixture turn timeout")
        .is_err()
    );
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert!(!f.root.path().join("output").exists());
    assert!(f.runtime.remaining().is_err());
}
#[tokio::test(flavor = "current_thread")]
async fn near_limit_snapshot_rescans_leave_tokio_timers_responsive() {
    let _lock = FIXTURE.lock().await;
    let f = Fixture::new();
    let bytes = vec![b'a'; 1024 * 1024];
    for i in 0..15 {
        std::fs::write(f.root.path().join(format!("input-{i}")), &bytes).unwrap();
    }
    let entered = Arc::new(Semaphore::new(0));
    let release = Arc::new(Semaphore::new(0));
    let gate = Arc::new(Runner {
        action: Box::new(|_| decision("allow")),
        count: Arc::new(AtomicUsize::new(0)),
        barrier: None,
        entered: Some(entered.clone()),
        release: Some(release.clone()),
    });
    let executor = f.executor(vec![registration("gate", HandlerClass::DecisionGate, gate)]);
    let events = f.events.clone();
    let pending = spawn_owned(async move { execute(executor, vec![call("output")], events).await });
    signal(&entered).await;
    let ticks = Arc::new(AtomicUsize::new(0));
    let heartbeat_ticks = ticks.clone();
    let heartbeat = spawn_owned(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            heartbeat_ticks.fetch_add(1, Ordering::SeqCst);
        }
    });
    release.add_permits(1);
    let results = pending.await.unwrap();
    heartbeat.abort();
    let _ = heartbeat.await;
    assert!(results[0].success, "{results:?}");
    assert!(
        ticks.load(Ordering::SeqCst) > 1,
        "final scans blocked the Tokio worker"
    );
}
#[tokio::test]
async fn aggregate_distinct_readsets_hold_before_excessive_snapshot_retention() {
    let _lock = FIXTURE.lock().await;
    let f = Fixture::new();
    let bytes = vec![b'a'; 1024 * 1024];
    let mut registrations = Vec::new();
    let calls = Arc::new(AtomicUsize::new(0));
    for group in 0..2 {
        let mut paths = Vec::new();
        for i in 0..9 {
            let path = format!("input-{group}-{i}");
            std::fs::write(f.root.path().join(&path), &bytes).unwrap();
            paths.push(path);
        }
        let counted = calls.clone();
        let mut r = registration(
            &format!("gate-{group}"),
            HandlerClass::DecisionGate,
            runner(move |_| {
                counted.fetch_add(1, Ordering::SeqCst);
                decision("allow")
            }),
        );
        r.declaration.reads = GateReadSet::new(paths, vec![], vec![]).unwrap();
        registrations.push(r);
    }
    let result = f.run(f.executor(registrations), vec![call("output")]).await;
    assert!(!result[0].success);
    assert!(result[0].output.contains("aggregate"));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(!f.root.path().join("output").exists());
}

#[test]
fn ambiguous_declaration_order_and_mixed_source_groups_are_rejected() {
    let one = registration(
        "package",
        HandlerClass::Transformer,
        runner(|i| rewrite(i, "left")),
    );
    let mut two = registration(
        "package",
        HandlerClass::Transformer,
        runner(|i| rewrite(i, "right")),
    );
    two.declaration.identity.declaration = "other".into();
    assert!(
        PreToolPlan::new(vec![one, two]).is_err(),
        "same declaration index cannot depend on discovery order"
    );
    let mut one = registration(
        "package",
        HandlerClass::Combined,
        runner(|_| decision("allow")),
    );
    one.declaration.identity.dialect = HookDialect::Claude;
    one.declaration.concurrent_group = Some("concurrent".into());
    let mut two = registration(
        "package",
        HandlerClass::DecisionGate,
        runner(|_| decision("allow")),
    );
    two.declaration.identity.dialect = HookDialect::Claude;
    two.declaration.identity.index = 1;
    two.declaration.identity.declaration = "other".into();
    two.declaration.concurrent_group = Some("concurrent".into());
    assert!(
        PreToolPlan::new(vec![one, two]).is_err(),
        "a source group cannot be split across transformer/final phases"
    );
}
#[tokio::test]
async fn native_order_and_canonical_dedup_do_not_merge_different_packages() {
    let _lock = FIXTURE.lock().await;
    let f = Fixture::new();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut registrations = Vec::new();
    for (name, priority, scope) in [
        ("user", 0, Scope::User),
        ("managed", 0, Scope::Managed),
        ("first", -1, Scope::LocalProject),
        ("project", 0, Scope::Project),
    ] {
        let seen = seen.clone();
        let mut r = registration(
            name,
            HandlerClass::DecisionGate,
            runner(move |_| {
                seen.lock().unwrap().push(name);
                decision("allow")
            }),
        );
        r.declaration.priority = priority;
        r.declaration.identity.scope = scope;
        registrations.push(r);
    }
    let repeated = Registration {
        declaration: registrations[0].declaration.clone(),
        runner: registrations[0].runner.clone(),
        revalidation: None,
    };
    registrations.push(repeated);
    let results = f.run(f.executor(registrations), vec![call("output")]).await;
    assert!(results[0].success);
    assert_eq!(
        *seen.lock().unwrap(),
        vec!["first", "managed", "user", "project"]
    );
}

#[tokio::test]
async fn source_read_only_revalidation_preserves_concurrent_startup() {
    let _lock = FIXTURE.lock().await;
    let f = Fixture::new();
    let barrier = Arc::new(Barrier::new(2));
    let mut registrations = Vec::new();
    for index in 0..2 {
        let mut r = registration(
            "source",
            HandlerClass::Combined,
            runner(|i| {
                let mut args = i
                    .candidate
                    .as_ref()
                    .expect("tool invocation")
                    .arguments
                    .clone();
                args["path"] = json!("final");
                output(
                    json!({"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow","updatedInput":args}}),
                )
            }),
        );
        r.declaration.priority = -(index as i32);
        r.declaration.identity.index = index;
        r.declaration.identity.declaration = format!("source-{index}");
        r.declaration.identity.dialect = HookDialect::Claude;
        r.declaration.concurrent_group = Some("group".into());
        r.declaration.read_only_endpoint = Some("inspect-final".into());
        r.revalidation = Some(Arc::new(Runner {
            action: Box::new(|i| {
                assert_eq!(
                    i.candidate.as_ref().expect("tool invocation").arguments["path"],
                    "final"
                );
                decision("allow")
            }),
            count: Arc::new(AtomicUsize::new(0)),
            barrier: Some(barrier.clone()),
            entered: None,
            release: None,
        }));
        registrations.push(r);
    }
    let results = tokio::time::timeout(
        Duration::from_secs(2),
        f.run(f.executor(registrations), vec![call("source")]),
    )
    .await
    .expect("read-only source revalidation was serialized");
    assert!(results[0].success, "{results:?}");
    assert_eq!(
        std::fs::read_to_string(f.root.path().join("final")).unwrap(),
        "written"
    );
}

#[tokio::test]
async fn distinct_later_readset_requires_visible_combined_revalidation() {
    let _lock = FIXTURE.lock().await;
    let f = Fixture::new();
    for name in ["a", "b"] {
        std::fs::write(f.root.path().join(name), name).unwrap();
    }
    let run = runner(|_| decision("allow"));
    let count = run.count.clone();
    let mut early = registration("early", HandlerClass::Combined, run);
    early.declaration.reads = GateReadSet::new(vec!["a".into()], vec![], vec![]).unwrap();
    let mut late = registration(
        "late",
        HandlerClass::DecisionGate,
        runner(|_| decision("allow")),
    );
    late.declaration.reads = GateReadSet::new(vec!["b".into()], vec![], vec![]).unwrap();
    let result = f
        .run(
            f.executor(vec![early, late]),
            vec![call("output"), call("output")],
        )
        .await;
    assert!(
        result
            .iter()
            .all(|r| !r.success && r.output.contains("needs-revalidation"))
    );
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert!(!f.root.path().join("output").exists());
    let record = f.record();
    let receipt = record
        .operations
        .iter()
        .find_map(|o| o.tool_receipt.as_ref())
        .unwrap()
        .plugin_admission
        .as_ref()
        .unwrap();
    assert_eq!(receipt.hooks[0].inspected.inputs.len(), 1);
    assert_eq!(receipt.final_key.as_ref().unwrap().inputs.len(), 2);
}

#[tokio::test]
async fn path_matchers_use_admitted_targets_for_existing_and_new_files() {
    let _lock = FIXTURE.lock().await;
    for host in [false, true] {
        for exists in [false, true] {
            for spelling in [
                "canonical",
                "repeat",
                "internal-dot",
                "leading-dot",
                "absolute",
                "parent-alias",
                "leaf-alias",
            ] {
                if !exists && spelling == "leaf-alias" {
                    continue;
                }
                for allowed in [false, true] {
                    let f = Fixture::new();
                    std::fs::create_dir(f.root.path().join("generated")).unwrap();
                    if exists {
                        std::fs::write(f.root.path().join("generated/file"), "unchanged").unwrap();
                    }
                    std::os::unix::fs::symlink("generated", f.root.path().join("parent-alias"))
                        .unwrap();
                    std::os::unix::fs::symlink("generated/file", f.root.path().join("leaf-alias"))
                        .unwrap();
                    let path = match spelling {
                        "canonical" => "generated/file".to_owned(),
                        "repeat" => "generated//file".to_owned(),
                        "internal-dot" => "generated/./file".to_owned(),
                        "leading-dot" => "./generated/file".to_owned(),
                        "absolute" => f
                            .root
                            .path()
                            .join("generated/file")
                            .to_str()
                            .unwrap()
                            .to_owned(),
                        "parent-alias" => "parent-alias/file".to_owned(),
                        _ => "leaf-alias".to_owned(),
                    };
                    let run = runner(move |input| {
                        assert_eq!(
                            input.candidate.as_ref().expect("tool invocation").arguments["path"],
                            "generated/file"
                        );
                        decision(if allowed { "allow" } else { "deny" })
                    });
                    let count = run.count.clone();
                    let mut gate = registration("target-policy", HandlerClass::DecisionGate, run);
                    gate.declaration.matcher.path = Some("generated/file".into());
                    let mut executor = ToolExecutor::with_policy(
                        f.root.path(),
                        &demoncoder::tools::AccessPolicy {
                            unrestricted: host,
                            ..Default::default()
                        },
                    )
                    .unwrap();
                    executor
                        .register_pre_tool_plan(Arc::new(PreToolPlan::new(vec![gate]).unwrap()))
                        .unwrap();
                    let results = f.run(executor, vec![call(&path)]).await;
                    let accepted =
                        host || matches!(spelling, "canonical" | "repeat" | "internal-dot");
                    assert_eq!(
                        results[0].success,
                        accepted && allowed,
                        "host={host}, exists={exists}, spelling={spelling}, allowed={allowed}: {}",
                        results[0].output
                    );
                    assert_eq!(
                        count.load(Ordering::SeqCst),
                        usize::from(accepted),
                        "host={host}, exists={exists}, spelling={spelling}, allowed={allowed}"
                    );
                    let value = std::fs::read_to_string(f.root.path().join("generated/file")).ok();
                    assert_eq!(
                        value.as_deref(),
                        if accepted && allowed {
                            Some("written")
                        } else if exists {
                            Some("unchanged")
                        } else {
                            None
                        }
                    );
                    let record = f.record();
                    let receipt = record
                        .operations
                        .iter()
                        .find_map(|o| o.tool_receipt.as_ref())
                        .unwrap();
                    assert_eq!(receipt.original_call.arguments["path"], path);
                }
            }
        }
    }
}

struct AliasRewrite(String);
impl ToolHook for AliasRewrite {
    fn before(&self, call: &mut ToolCall) -> anyhow::Result<()> {
        call.arguments["path"] = json!(self.0);
        Ok(())
    }
}

#[tokio::test]
async fn rewritten_aliases_are_normalized_before_later_matchers() {
    let _lock = FIXTURE.lock().await;
    for host in [false, true] {
        for generic in [false, true] {
            let f = Fixture::new();
            std::fs::create_dir(f.root.path().join("generated")).unwrap();
            std::os::unix::fs::symlink("generated", f.root.path().join("alias")).unwrap();
            let path = if host {
                "alias/./file"
            } else {
                "generated//file"
            };
            let deny = runner(|input| {
                assert_eq!(
                    input.candidate.as_ref().expect("tool invocation").arguments["path"],
                    "generated/file"
                );
                decision("deny")
            });
            let count = deny.count.clone();
            let mut gate = registration("policy", HandlerClass::DecisionGate, deny);
            gate.declaration.matcher.path = Some("generated/file".into());
            let mut declarations = vec![gate];
            if !generic {
                let mut transformer = registration(
                    "transformer",
                    HandlerClass::Transformer,
                    runner(move |input| rewrite(input, path)),
                );
                transformer.declaration.matcher.path = Some("source".into());
                declarations.push(transformer);
            }
            let mut executor = ToolExecutor::with_policy(
                f.root.path(),
                &demoncoder::tools::AccessPolicy {
                    unrestricted: host,
                    ..Default::default()
                },
            )
            .unwrap();
            executor
                .register_pre_tool_plan(Arc::new(PreToolPlan::new(declarations).unwrap()))
                .unwrap();
            if generic {
                executor.add_hook(Box::new(AliasRewrite(path.into())));
            }
            let result = f.run(executor, vec![call("source"), call("source")]).await;
            assert!(result.iter().all(|r| !r.success));
            assert_eq!(count.load(Ordering::SeqCst), 1);
            assert!(!f.root.path().join("generated/file").exists());
            let record = f.record();
            let operation = record
                .operations
                .iter()
                .find(|op| op.tool_receipt.is_some())
                .unwrap();
            assert_eq!(
                operation.call.as_ref().unwrap().arguments["path"],
                "generated/file"
            );
            assert_eq!(
                operation
                    .tool_receipt
                    .as_ref()
                    .unwrap()
                    .original_call
                    .arguments["path"],
                "source"
            );
        }
    }
}

#[tokio::test]
async fn renamed_targets_and_parents_cannot_reuse_an_awaited_gate_pass() {
    let _lock = FIXTURE.lock().await;
    for host in [false, true] {
        for existing in [false, true] {
            for symlink in [false, true] {
                let f = Fixture::new();
                std::fs::create_dir(f.root.path().join("generated")).unwrap();
                std::fs::create_dir(f.root.path().join("protected")).unwrap();
                std::fs::write(f.root.path().join("protected/file"), "protected").unwrap();
                if existing {
                    std::fs::write(f.root.path().join("generated/file"), "original").unwrap();
                }
                let entered = Arc::new(Semaphore::new(0));
                let release = Arc::new(Semaphore::new(0));
                let run = Arc::new(Runner {
                    action: Box::new(|_| decision("allow")),
                    count: Arc::new(AtomicUsize::new(0)),
                    barrier: None,
                    entered: Some(entered.clone()),
                    release: Some(release.clone()),
                });
                let mut gate = registration("policy", HandlerClass::DecisionGate, run);
                gate.declaration.matcher.path = Some("generated/file".into());
                gate.declaration.reads = GateReadSet::new(vec![], vec![], vec![]).unwrap();
                let mut executor = ToolExecutor::with_policy(
                    f.root.path(),
                    &demoncoder::tools::AccessPolicy {
                        unrestricted: host,
                        ..Default::default()
                    },
                )
                .unwrap();
                executor
                    .register_pre_tool_plan(Arc::new(PreToolPlan::new(vec![gate]).unwrap()))
                    .unwrap();
                let events = f.events.clone();
                let task = spawn_owned(async move {
                    execute(executor, vec![call("generated/file")], events).await
                });
                signal(&entered).await;
                if existing {
                    std::fs::rename(
                        f.root.path().join("generated/file"),
                        f.root.path().join("original-file"),
                    )
                    .unwrap();
                    if symlink {
                        std::os::unix::fs::symlink(
                            "../protected/file",
                            f.root.path().join("generated/file"),
                        )
                        .unwrap();
                    } else {
                        std::fs::write(f.root.path().join("generated/file"), "replacement")
                            .unwrap();
                    }
                } else {
                    std::fs::rename(
                        f.root.path().join("generated"),
                        f.root.path().join("original-parent"),
                    )
                    .unwrap();
                    if symlink {
                        std::os::unix::fs::symlink("protected", f.root.path().join("generated"))
                            .unwrap();
                    } else {
                        std::fs::create_dir(f.root.path().join("generated")).unwrap();
                    }
                }
                release.add_permits(1);
                let result = task.await.unwrap();
                assert!(
                    !result[0].success,
                    "host={host}, existing={existing}, symlink={symlink}: {result:?}"
                );
                assert_eq!(
                    std::fs::read_to_string(f.root.path().join("protected/file")).unwrap(),
                    "protected"
                );
                assert!(!f.root.path().join("original-parent/file").exists());
                if existing {
                    assert_eq!(
                        std::fs::read_to_string(f.root.path().join("original-file")).unwrap(),
                        "original"
                    );
                }
            }
        }
    }
}

#[tokio::test]
async fn missing_parent_is_held_without_creating_directories_or_running_the_gate() {
    let _lock = FIXTURE.lock().await;
    for host in [false, true] {
        let f = Fixture::new();
        let run = runner(|_| decision("allow"));
        let count = run.count.clone();
        let mut executor = ToolExecutor::with_policy(
            f.root.path(),
            &demoncoder::tools::AccessPolicy {
                unrestricted: host,
                ..Default::default()
            },
        )
        .unwrap();
        executor
            .register_pre_tool_plan(Arc::new(
                PreToolPlan::new(vec![registration(
                    "policy",
                    HandlerClass::DecisionGate,
                    run,
                )])
                .unwrap(),
            ))
            .unwrap();
        let result = f.run(executor, vec![call("missing/file")]).await;
        assert!(!result[0].success);
        assert_eq!(count.load(Ordering::SeqCst), 0);
        assert!(!f.root.path().join("missing").exists());
    }
}

#[tokio::test]
async fn concurrent_uncertainty_preserves_all_completed_results_in_either_order() {
    let _lock = FIXTURE.lock().await;
    for failure_first in [false, true] {
        let f = Fixture::new();
        let mut registrations = Vec::new();
        for index in 0..4 {
            let failure = (index == 0) == failure_first && index < 2;
            let mut registration = registration(
                "source",
                HandlerClass::Combined,
                runner(move |_| {
                    if failure {
                        RawOutcome::Failure {
                            reason: "transport lost after possible effect".into(),
                        }
                    } else if index == 2 {
                        decision("allow")
                    } else {
                        decision("deny")
                    }
                }),
            );
            registration.declaration.identity.index = index;
            registration.declaration.identity.declaration = format!("source-{index}");
            registration.declaration.identity.dialect = HookDialect::Claude;
            registration.declaration.concurrent_group = Some("group".into());
            registrations.push(registration);
        }
        let executor = f.executor(registrations);
        let results = Arc::new(Mutex::new(Vec::new()));
        let model = Responses {
            calls: VecDeque::from([vec![call("output")]]),
            results,
        };
        let mut session = NativeSession::with_tools(Box::new(model), executor);
        let (_sender, mut commands) = mpsc::channel(4);
        let _outcome = tokio::time::timeout(
            Duration::from_secs(5),
            session.turn("test".into(), &mut commands, &f.events),
        )
        .await
        .expect("fixture timeout");
        assert!(!f.root.path().join("output").exists());
        let record = f.record();
        assert!(record.recovery_pending);
        let receipt = record
            .operations
            .iter()
            .find_map(|o| o.tool_receipt.as_ref())
            .unwrap()
            .plugin_admission
            .as_ref()
            .unwrap();
        assert_eq!(receipt.hooks.len(), 4);
        assert!(
            receipt.hooks.iter().all(|hook| hook.outcome.is_some()),
            "failure_first={failure_first}: {:?}",
            receipt.hooks
        );
        assert_eq!(
            receipt
                .hooks
                .iter()
                .filter(|hook| hook.uncertain_effects)
                .count(),
            1
        );
        assert_eq!(
            receipt
                .hooks
                .iter()
                .flat_map(|hook| &hook.questions)
                .filter(|q| q.choice == demoncoder::plugins::receipts::PendingDecision::Deny)
                .count(),
            2
        );
        assert!(receipt.hold.is_some());
    }
}
