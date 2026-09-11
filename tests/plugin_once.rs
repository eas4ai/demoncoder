use demoncoder::{
    config::Connection,
    plugins::{
        once::{ActivationChange, ActivationSource, HookOrigin},
        receipts::Scope,
    },
    workflow::runtime::SharedRuntime,
};
use serde_json::json;
#[tokio::test]
async fn activation_rebuild_reuses_epoch_and_explicit_invocation_advances() {
    let _guard = SERIAL.lock().await;
    let root = tempfile::tempdir().unwrap();
    let connection: Connection = serde_json::from_value(json!({"adapter":"openai-api"})).unwrap();
    let (runtime, _) = SharedRuntime::open(root.path(), &connection, None).unwrap();
    let bind = |change| {
        runtime
            .plugin_hook_activation(
                HookOrigin::Native,
                Scope::Project,
                &ActivationSource::host_namespace("package").unwrap(),
                "skills/check",
                "worker",
                change,
            )
            .unwrap()
            .unwrap()
    };
    let first = bind(ActivationChange::ExplicitInvocation);
    assert_eq!(
        serde_json::to_value(&first).unwrap(),
        serde_json::to_value(bind(ActivationChange::Reuse)).unwrap()
    );
    assert_ne!(
        serde_json::to_value(first).unwrap(),
        serde_json::to_value(bind(ActivationChange::ExplicitInvocation)).unwrap()
    );
    std::fs::remove_dir_all(runtime.directory().unwrap()).unwrap();
}

use demoncoder::{
    events::{Envelope, EventSink},
    native::{Model, NativeSession},
    plugins::{
        self,
        dispatch::*,
        gate_snapshot::GateReadSet,
        hook_types::{HandlerKind, HookDialect, HookEvent},
        lifecycle::PostToolPlan,
        once::OnceBinding,
        runners::{CommandConfig, CommandProgram, CommandRunner},
    },
    session::Session,
    tools::{AccessPolicy, ToolCall, ToolExecutor, ToolResult},
};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::mpsc;
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
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
struct Fixture {
    source: tempfile::TempDir,
    root: tempfile::TempDir,
    runtime: SharedRuntime,
    events: EventSink,
    _receiver: mpsc::Receiver<Envelope>,
}
impl Fixture {
    fn new() -> Self {
        let source = tempfile::tempdir().unwrap();
        std::fs::create_dir(source.path().join(".claude-plugin")).unwrap();
        std::fs::write(
            source.path().join(".claude-plugin/plugin.json"),
            r#"{"name":"once-fixture","version":"1"}"#,
        )
        .unwrap();
        let root = tempfile::tempdir().unwrap();
        let connection: Connection =
            serde_json::from_value(json!({"adapter":"openai-api"})).unwrap();
        let (runtime, _) = SharedRuntime::open(root.path(), &connection, None).unwrap();
        let (sender, receiver) = mpsc::channel(256);
        let events = EventSink::new("once-test".into(), sender, None)
            .unwrap()
            .with_runtime(runtime.clone());
        Self {
            source,
            root,
            runtime,
            events,
            _receiver: receiver,
        }
    }
    fn binding(&self, component: &str, change: ActivationChange) -> OnceBinding {
        let package =
            plugins::inspect(self.source.path(), &plugins::ImportOptions::default()).unwrap();
        self.runtime
            .plugin_hook_activation(
                HookOrigin::Native,
                Scope::Project,
                &ActivationSource::from_package(&package).unwrap(),
                component,
                "worker",
                change,
            )
            .unwrap()
            .unwrap()
    }
    fn command(
        &self,
        event: HookEvent,
        name: &str,
        index: u32,
        once: Option<OnceBinding>,
        response: &str,
    ) -> Registration {
        command_at(self.source.path(), event, name, index, once, response)
    }
    fn executor(&self, event: HookEvent, registrations: Vec<Registration>) -> ToolExecutor {
        for registration in &registrations {
            let path = self
                .root
                .path()
                .join(&registration.declaration.identity.declaration);
            if !path.exists() {
                std::fs::write(path, "").unwrap();
            }
        }
        let mut executor = ToolExecutor::with_policy(
            self.root.path(),
            &AccessPolicy {
                supervisor: Some(env!("CARGO_BIN_EXE_demoncoder").into()),
                ..Default::default()
            },
        )
        .unwrap();
        if event == HookEvent::PreToolUse {
            executor
                .register_pre_tool_plan(Arc::new(PreToolPlan::new(registrations).unwrap()))
                .unwrap();
        } else {
            executor
                .register_post_tool_plan(Arc::new(PostToolPlan::new(event, registrations).unwrap()))
                .unwrap();
        }
        executor
    }
    async fn run(&self, executor: ToolExecutor, paths: &[&str]) -> Vec<ToolResult> {
        let results = Arc::new(Mutex::new(Vec::new()));
        let calls = paths
            .iter()
            .enumerate()
            .map(|(i, path)| ToolCall {
                id: format!("call-{i}"),
                name: "write".into(),
                arguments: json!({"path":path,"content":"written"}),
            })
            .collect();
        let model = Responses {
            calls: VecDeque::from([calls]),
            results: results.clone(),
        };
        let mut session = NativeSession::with_tools(Box::new(model), executor);
        let (_sender, mut commands) = mpsc::channel(4);
        let end = tokio::time::timeout(
            Duration::from_secs(25),
            session.turn("ordinary prompt".into(), &mut commands, &self.events),
        )
        .await
        .unwrap();
        if let Err(error) = end {
            eprintln!("turn held: {error:#}");
        }
        for op in self.runtime.record().unwrap().operations {
            if let Some(r) = op.tool_receipt {
                for h in r
                    .plugin_admission
                    .iter()
                    .flat_map(|p| &p.hooks)
                    .chain(r.plugin_lifecycle.iter().flat_map(|p| &p.hooks))
                {
                    if let Some(RawOutcome::Command {
                        exit_code, stderr, ..
                    }) = &h.outcome
                        && *exit_code != Some(0)
                    {
                        eprintln!("command {exit_code:?}: {}", String::from_utf8_lossy(stderr));
                    }
                }
            }
        }
        results.lock().unwrap().clone()
    }
    fn count(&self, name: &str) -> usize {
        std::fs::read_to_string(self.root.path().join(name))
            .unwrap_or_default()
            .lines()
            .count()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(self.runtime.directory().unwrap()).unwrap();
    }
}
fn declaration(name: &str, index: u32, once: Option<OnceBinding>) -> Declaration {
    Declaration {
        required_gate: true,
        source: once.as_ref().map(OnceBinding::source),
        once,
        identity: DeclarationIdentity {
            package: "once-fixture".into(),
            code: "code".into(),
            policy: "policy".into(),
            configuration: "config".into(),
            generation: "generation".into(),
            scope: Scope::Project,
            role: "worker".into(),
            declaration: name.into(),
            index,
            dialect: HookDialect::Native,
            runner: HandlerKind::Command,
        },
        class: HandlerClass::Combined,
        priority: 0,
        matcher: Matcher::default(),
        reads: GateReadSet::new(vec![], vec![], vec![]).unwrap(),
        concurrent_group: None,
        read_only_endpoint: None,
        external_precondition: None,
    }
}
fn command_at(
    source: &std::path::Path,
    event: HookEvent,
    name: &str,
    index: u32,
    once: Option<OnceBinding>,
    response: &str,
) -> Registration {
    let code = format!(
        "import json,sys\nx=json.load(sys.stdin)\nwith open({name:?},'a') as f: f.write('called\\n')\n{response}\n"
    );
    std::fs::write(source.join("hook.py"), code).unwrap();
    let package = Arc::new(plugins::inspect(source, &plugins::ImportOptions::default()).unwrap());
    let mut config = CommandConfig::new(CommandProgram::Argv(vec![
        "/usr/bin/python3".into(),
        "${CLAUDE_PLUGIN_ROOT}/hook.py".into(),
    ]));
    config.write_paths = vec![name.into()];
    let mut d = declaration(name, index, once);
    d.class = if event == HookEvent::PreToolUse {
        HandlerClass::Transformer
    } else {
        HandlerClass::Observer
    };
    d.required_gate = event == HookEvent::PreToolUse;
    d.reads = GateReadSet::new(vec![name.into()], vec![], vec![]).unwrap();
    CommandRunner::registration_for_event(package, d, event, config, None).unwrap()
}
const ALLOW: &str = "print(json.dumps({'hookSpecificOutput':{'hookEventName':'PreToolUse','permissionDecision':'allow'}}))";
const POST: &str = "print(json.dumps({'hookSpecificOutput':{'hookEventName':'PostToolUse','additionalContext':'once context'}}))";

#[tokio::test]
async fn actual_pre_command_consumes_once_across_turns_and_explicit_activation_restores_it() {
    let _guard = SERIAL.lock().await;
    let f = Fixture::new();
    for (turn, change) in [
        ActivationChange::ExplicitInvocation,
        ActivationChange::Reuse,
        ActivationChange::ExplicitInvocation,
    ]
    .into_iter()
    .enumerate()
    {
        let binding = f.binding("skills/check", change);
        let executor = f.executor(
            HookEvent::PreToolUse,
            vec![
                f.command(HookEvent::PreToolUse, "once-count", 0, Some(binding), ALLOW),
                f.command(HookEvent::PreToolUse, "control-count", 1, None, ALLOW),
            ],
        );
        let results = f.run(executor, &[&format!("file-{turn}")]).await;
        assert_eq!(results.len(), 1);
        assert!(results[0].success, "{:?}", results);
        assert_eq!(f.count("once-count"), if turn == 0 { 1 } else { turn });
        assert_eq!(f.count("control-count"), turn + 1);
    }
    let record = f.runtime.record().unwrap();
    let plans: Vec<_> = record
        .operations
        .iter()
        .filter_map(|o| o.tool_receipt.as_ref()?.plugin_admission.as_ref())
        .collect();
    assert_eq!(plans.len(), 3);
    assert_eq!(plans[1].hooks.len(), 1);
    assert_eq!(plans[1].once_skips.len(), 1);
    assert_eq!(
        plans[1].once_skips[0].consumed.operation,
        plans[0].hooks[0].inspected.operation
    );
}

#[tokio::test]
async fn actual_post_command_does_not_reapply_context_or_rewrite_original_evidence() {
    let _guard = SERIAL.lock().await;
    let f = Fixture::new();
    for turn in 0..2 {
        let binding = f.binding(
            "skills/check",
            if turn == 0 {
                ActivationChange::ExplicitInvocation
            } else {
                ActivationChange::Reuse
            },
        );
        let executor = f.executor(
            HookEvent::PostToolUse,
            vec![
                f.command(HookEvent::PostToolUse, "once-count", 0, Some(binding), POST),
                f.command(
                    HookEvent::PostToolUse,
                    "control-count",
                    1,
                    None,
                    "print('{}')",
                ),
            ],
        );
        let results = f.run(executor, &[&format!("file-{turn}")]).await;
        assert_eq!(results.len(), 1);
        assert!(results[0].success);
        assert_eq!(results[0].output.contains("once context"), turn == 0);
    }
    assert_eq!(f.count("once-count"), 1);
    assert_eq!(f.count("control-count"), 2);
    let record = f.runtime.record().unwrap();
    let ops: Vec<_> = record
        .operations
        .iter()
        .filter(|o| {
            o.tool_receipt
                .as_ref()
                .is_some_and(|r| r.plugin_lifecycle.is_some())
        })
        .collect();
    assert_eq!(ops.len(), 2);
    assert!(
        !ops[0]
            .result
            .as_ref()
            .unwrap()
            .output
            .contains("once context")
    );
    let post = ops[1]
        .tool_receipt
        .as_ref()
        .unwrap()
        .plugin_lifecycle
        .as_ref()
        .unwrap();
    assert_eq!(post.once_skips.len(), 1);
    assert!(post.messages.is_empty());
}

#[tokio::test]
async fn known_failure_and_logical_block_retry_only_on_later_events() {
    let _guard = SERIAL.lock().await;
    for response in [
        "sys.exit(1)",
        "print('not-json')",
        "print(json.dumps({'hookSpecificOutput':{'hookEventName':'PreToolUse','permissionDecision':'deny','permissionDecisionReason':'blocked'}}))",
    ] {
        let f = Fixture::new();
        for turn in 0..2 {
            let binding = f.binding(
                "skills/check",
                if turn == 0 {
                    ActivationChange::ExplicitInvocation
                } else {
                    ActivationChange::Reuse
                },
            );
            let executor = f.executor(
                HookEvent::PreToolUse,
                vec![f.command(
                    HookEvent::PreToolUse,
                    "once-count",
                    0,
                    Some(binding),
                    response,
                )],
            );
            f.run(executor, &[&format!("blocked-{turn}")]).await;
            assert!(!f.root.path().join(format!("blocked-{turn}")).exists());
            assert_eq!(f.count("once-count"), turn + 1);
        }
    }
}

#[tokio::test]
async fn ignored_source_locations_cannot_enable_once_and_foreign_dialect_is_rejected() {
    let _guard = SERIAL.lock().await;
    let f = Fixture::new();
    for origin in [
        HookOrigin::ClaudeSettings,
        HookOrigin::ClaudeAgent,
        HookOrigin::Codex,
    ] {
        assert!(
            f.runtime
                .plugin_hook_activation(
                    origin,
                    Scope::Project,
                    &ActivationSource::host_namespace("once-fixture").unwrap(),
                    "settings",
                    "worker",
                    ActivationChange::ExplicitInvocation
                )
                .unwrap()
                .is_none()
        );
    }
    let binding = f.binding("skills/check", ActivationChange::ExplicitInvocation);
    let mut registration = f.command(HookEvent::PreToolUse, "count", 0, Some(binding), ALLOW);
    registration.declaration.identity.dialect = HookDialect::Codex;
    assert!(PreToolPlan::new(vec![registration]).is_err());
}

#[tokio::test]
async fn actual_post_known_failure_is_eligible_on_next_event() {
    let _guard = SERIAL.lock().await;
    let f = Fixture::new();
    for turn in 0..2 {
        let b = f.binding(
            "skills/check",
            if turn == 0 {
                ActivationChange::ExplicitInvocation
            } else {
                ActivationChange::Reuse
            },
        );
        let results = f
            .run(
                f.executor(
                    HookEvent::PostToolUse,
                    vec![f.command(
                        HookEvent::PostToolUse,
                        "once-count",
                        0,
                        Some(b),
                        "sys.exit(1)",
                    )],
                ),
                &[&format!("file-{turn}")],
            )
            .await;
        assert_eq!(results.len(), 1);
        assert!(results[0].success);
        assert_eq!(f.count("once-count"), turn + 1);
    }
}
struct Callback {
    action: Box<dyn Fn(&HookInvocation) -> RawOutcome + Send + Sync>,
    count: Arc<std::sync::atomic::AtomicUsize>,
}
#[async_trait::async_trait]
impl HookRunner for Callback {
    async fn run(&self, invocation: &HookInvocation) -> anyhow::Result<RawOutcome> {
        self.count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok((self.action)(invocation))
    }
}
fn callback(
    d: Declaration,
    action: impl Fn(&HookInvocation) -> RawOutcome + Send + Sync + 'static,
    count: Arc<std::sync::atomic::AtomicUsize>,
) -> Registration {
    Registration {
        declaration: d,
        runner: Arc::new(Callback {
            action: Box::new(action),
            count,
        }),
        revalidation: None,
    }
}
#[tokio::test]
async fn consumed_combined_hook_is_exempt_but_a_new_activation_cannot_approve_a_stale_candidate() {
    let _guard = SERIAL.lock().await;
    let f = Fixture::new();
    let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    for turn in 0..3 {
        let b = f.binding(
            "skills/check",
            if turn == 1 {
                ActivationChange::Reuse
            } else {
                ActivationChange::ExplicitInvocation
            },
        );
        let once = callback(
            declaration("combined", 0, Some(b)),
            |_| RawOutcome::Callback {
                value: json!({"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow"}}),
            },
            count.clone(),
        );
        let mut registrations = vec![once];
        if turn > 0 {
            let mut d = declaration("rewrite", 1, None);
            d.class = HandlerClass::Transformer;
            registrations.push(callback(d,|input|{let mut arguments=input.candidate.arguments.clone();arguments["path"]=json!("rewritten");RawOutcome::Callback{value:json!({"hookSpecificOutput":{"hookEventName":"PreToolUse","updatedInput":arguments}})}},Arc::new(std::sync::atomic::AtomicUsize::new(0))));
        }
        let results = f
            .run(
                f.executor(HookEvent::PreToolUse, registrations),
                &[&format!("requested-{turn}")],
            )
            .await;
        assert_eq!(results.len(), 1);
        if turn < 2 {
            assert!(results[0].success, "{:?}", results);
        } else {
            assert!(!results[0].success);
            assert!(results[0].output.contains("needs-revalidation"));
        }
    }
    assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 2);
    assert!(f.root.path().join("rewritten").exists());
    assert!(!f.root.path().join("requested-1").exists());
}

struct ResumeSupervisor(std::os::fd::OwnedFd);
impl Drop for ResumeSupervisor {
    fn drop(&mut self) {
        let _ = rustix::process::pidfd_send_signal(&self.0, rustix::process::Signal::CONT);
    }
}
fn supervisor_for(workspace: &std::path::Path) -> (u32, std::os::fd::OwnedFd) {
    let mut candidates = Vec::new();
    for task in std::fs::read_dir("/proc/self/task").unwrap().flatten() {
        for pid in std::fs::read_to_string(task.path().join("children"))
            .unwrap_or_default()
            .split_whitespace()
        {
            let pid: u32 = pid.parse().unwrap();
            let cmd = std::fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
            let cmd = String::from_utf8_lossy(&cmd);
            if cmd.contains("--supervise-hook") && cmd.contains(workspace.to_str().unwrap()) {
                let fd = rustix::process::pidfd_open(
                    rustix::process::Pid::from_raw(pid as i32).unwrap(),
                    rustix::process::PidfdFlags::empty(),
                )
                .unwrap();
                candidates.push((pid, fd));
            }
        }
    }
    assert_eq!(
        candidates.len(),
        1,
        "expected exactly this fixture's supervisor"
    );
    candidates.pop().unwrap()
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelled_real_command_cannot_reconcile_until_its_stopped_supervisor_is_reaped() {
    let _guard = SERIAL.lock().await;
    let f = Fixture::new();
    let b = f.binding("skills/check", ActivationChange::ExplicitInvocation);
    let executor = f.executor(
        HookEvent::PreToolUse,
        vec![f.command(
            HookEvent::PreToolUse,
            "once-count",
            0,
            Some(b.clone()),
            "import time\ntime.sleep(15)",
        )],
    );
    let paths = ["never-written"];
    let mut turn = Box::pin(f.run(executor, &paths));
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while f.count("once-count") == 0 {
        tokio::select! { _=&mut turn=>panic!("command finished before cancellation"), _=tokio::time::sleep(Duration::from_millis(5))=>{} }
        assert!(
            tokio::time::Instant::now() < deadline,
            "command did not start"
        );
    }
    let target = f.runtime.unresolved_plugin_once().unwrap().pop().unwrap();
    assert!(
        f.runtime
            .reconcile_failed_plugin_once(&target, "developer", "cannot attest a running hook")
            .is_err()
    );
    let (pid, pidfd) = supervisor_for(f.root.path());
    // Establish the resume guard before STOP. pidfd pins the exact process identity.
    let resume = ResumeSupervisor(pidfd);
    rustix::process::pidfd_send_signal(&resume.0, rustix::process::Signal::STOP).unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let status = std::fs::read_to_string(format!("/proc/{pid}/status")).unwrap();
            if status
                .lines()
                .any(|line| line.starts_with("State:") && line.contains('T'))
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();
    drop(turn);
    tokio::time::sleep(Duration::from_millis(20)).await;
    let error = f
        .runtime
        .reconcile_failed_plugin_once(&target, "developer", "supervisor is still stopped")
        .unwrap_err();
    assert!(error.to_string().contains("still live"), "{error:#}");
    assert_eq!(f.count("once-count"), 1);
    drop(resume);
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match f.runtime.reconcile_failed_plugin_once(
                &target,
                "developer",
                "supervisor reaped; abandoned attempt inspected as unsuccessful",
            ) {
                Ok(()) => break,
                Err(error) => {
                    assert!(error.to_string().contains("still live"), "{error:#}");
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            }
        }
    })
    .await
    .unwrap();
    assert!(
        !std::path::Path::new(&format!("/proc/{pid}")).exists(),
        "supervisor must be reaped before reconciliation"
    );
    assert!(!f.root.path().join("never-written").exists());
    f.runtime
        .reconcile("abandoned command cleanup was observed", None)
        .unwrap();
    let results = f
        .run(
            f.executor(
                HookEvent::PreToolUse,
                vec![f.command(HookEvent::PreToolUse, "once-count", 0, Some(b), ALLOW)],
            ),
            &["next-event"],
        )
        .await;
    assert_eq!(results.len(), 1);
    assert!(results[0].success, "{:?}", results);
    assert_eq!(f.count("once-count"), 2);
}

#[tokio::test]
async fn canonical_duplicates_dedup_but_packages_scopes_and_explicit_skills_remain_distinct() {
    let _guard = SERIAL.lock().await;
    let f = Fixture::new();
    let cases = [
        ("once-fixture", Scope::Project, "skills/one"),
        ("once-fixture", Scope::Project, "skills/two"),
        ("once-fixture", Scope::User, "skills/one"),
        ("different-package", Scope::Project, "skills/one"),
    ];
    let counts = (0..cases.len())
        .map(|_| Arc::new(std::sync::atomic::AtomicUsize::new(0)))
        .collect::<Vec<_>>();
    for turn in 0..2 {
        let mut registrations = Vec::new();
        for ((package, scope, component), count) in cases.iter().zip(&counts) {
            let binding = f
                .runtime
                .plugin_hook_activation(
                    HookOrigin::Native,
                    scope.clone(),
                    &ActivationSource::host_namespace(package).unwrap(),
                    component,
                    "worker",
                    if turn == 0 {
                        ActivationChange::ExplicitInvocation
                    } else {
                        ActivationChange::Reuse
                    },
                )
                .unwrap()
                .unwrap();
            let mut d = declaration("same-declaration", 0, Some(binding));
            d.identity.package = (*package).into();
            d.identity.scope = scope.clone();
            for _ in 0..2 {
                registrations.push(callback(d.clone(),|_|RawOutcome::Callback{value:json!({"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow"}})},count.clone()));
            }
        }
        let results = f
            .run(
                f.executor(HookEvent::PreToolUse, registrations),
                &[&format!("file-{turn}")],
            )
            .await;
        assert_eq!(results.len(), 1);
        assert!(results[0].success, "{:?}", results);
    }
    for count in counts {
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn native_resume_preserves_consumed_claude_frontmatter_binding() {
    let _guard = SERIAL.lock().await;
    let root = tempfile::tempdir().unwrap();
    let connection: Connection = serde_json::from_value(json!({"adapter":"openai-api"})).unwrap();
    let (mut runtime, _) = SharedRuntime::open(root.path(), &connection, None).unwrap();
    let source = tempfile::tempdir().unwrap();
    std::fs::create_dir(source.path().join(".claude-plugin")).unwrap();
    std::fs::write(
        source.path().join(".claude-plugin/plugin.json"),
        r#"{"name":"once-fixture","version":"1"}"#,
    )
    .unwrap();
    let package = plugins::inspect(source.path(), &plugins::ImportOptions::default()).unwrap();
    let captured = ActivationSource::from_package(&package).unwrap();
    let directory = runtime.directory().unwrap();
    let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut first_binding = None;
    for turn in 0..2 {
        let binding = runtime
            .plugin_hook_activation(
                HookOrigin::ClaudeSkillFrontmatter,
                Scope::Project,
                &captured,
                "skills/check",
                "worker",
                if turn == 0 {
                    ActivationChange::ExplicitInvocation
                } else {
                    ActivationChange::Reuse
                },
            )
            .unwrap()
            .unwrap();
        let encoded = serde_json::to_value(&binding).unwrap();
        if let Some(first) = &first_binding {
            assert_eq!(first, &encoded);
        } else {
            first_binding = Some(encoded);
        }
        let mut d = declaration("claude-once", 0, Some(binding));
        d.identity.dialect = HookDialect::Claude;
        d.concurrent_group = Some("source-group".into());
        let registration = callback(
            d,
            |_| {
                RawOutcome::Command{exit_code:Some(0),stdout:br#"{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow"}}"#.to_vec(),stderr:vec![]}
            },
            count.clone(),
        );
        {
            let mut executor = ToolExecutor::new(root.path()).unwrap();
            executor
                .register_pre_tool_plan(Arc::new(PreToolPlan::new(vec![registration]).unwrap()))
                .unwrap();
            let (sender, _receiver) = mpsc::channel(128);
            let events = EventSink::new("resume".into(), sender, None)
                .unwrap()
                .with_runtime(runtime.clone());
            let results = Arc::new(Mutex::new(Vec::new()));
            let model = Responses {
                calls: VecDeque::from([vec![ToolCall {
                    id: format!("call-{turn}"),
                    name: "write".into(),
                    arguments: json!({"path":format!("file-{turn}"),"content":"value"}),
                }]]),
                results: results.clone(),
            };
            let mut session = NativeSession::with_tools(Box::new(model), executor);
            let (_sender, mut commands) = mpsc::channel(4);
            tokio::time::timeout(
                Duration::from_secs(5),
                session.turn("ordinary prompt".into(), &mut commands, &events),
            )
            .await
            .unwrap()
            .unwrap();
            assert!(results.lock().unwrap()[0].success);
        }
        if turn == 0 {
            drop(runtime);
            let (resumed, yes) =
                SharedRuntime::open(root.path(), &connection, Some(&directory)).unwrap();
            assert!(yes);
            runtime = resumed;
        }
    }
    assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(runtime.record().unwrap().plugin_activations.len(), 1);
    std::fs::remove_dir_all(directory).unwrap();
}

fn manifest(root: &std::path::Path) {
    std::fs::create_dir_all(root.join(".claude-plugin")).unwrap();
    std::fs::write(
        root.join(".claude-plugin/plugin.json"),
        r#"{"name":"once-fixture","version":"1"}"#,
    )
    .unwrap();
}
#[tokio::test]
async fn same_named_package_roots_are_independent_and_canonical_aliases_dedup() {
    let _guard = SERIAL.lock().await;
    let f = Fixture::new();
    let other = tempfile::tempdir().unwrap();
    manifest(other.path());
    let aliases = tempfile::tempdir().unwrap();
    let alias = aliases.path().join("alias");
    std::os::unix::fs::symlink(f.source.path(), &alias).unwrap();
    for turn in 0..2 {
        let change = if turn == 0 {
            ActivationChange::ExplicitInvocation
        } else {
            ActivationChange::Reuse
        };
        let first = f.binding("skills/check", change);
        let package = plugins::inspect(other.path(), &plugins::ImportOptions::default()).unwrap();
        let second = f
            .runtime
            .plugin_hook_activation(
                HookOrigin::Native,
                Scope::Project,
                &ActivationSource::from_package(&package).unwrap(),
                "skills/check",
                "worker",
                change,
            )
            .unwrap()
            .unwrap();
        let registrations = vec![
            f.command(
                HookEvent::PreToolUse,
                "count",
                0,
                Some(first.clone()),
                ALLOW,
            ),
            command_at(
                &alias,
                HookEvent::PreToolUse,
                "count",
                0,
                Some(first),
                ALLOW,
            ),
            command_at(
                other.path(),
                HookEvent::PreToolUse,
                "count",
                0,
                Some(second),
                ALLOW,
            ),
        ];
        let result = f
            .run(
                f.executor(HookEvent::PreToolUse, registrations),
                &[&format!("file-{turn}")],
            )
            .await;
        assert_eq!(result.len(), 1);
        assert!(result[0].success, "{:?}", result);
        assert_eq!(f.count("count"), 2);
    }
    assert_eq!(f.runtime.record().unwrap().plugin_activations.len(), 2);
}
#[tokio::test]
async fn mismatched_source_and_unpackaged_host_binding_cannot_launch_a_package_runner() {
    let _guard = SERIAL.lock().await;
    let f = Fixture::new();
    let other = tempfile::tempdir().unwrap();
    manifest(other.path());
    let package =
        Arc::new(plugins::inspect(other.path(), &plugins::ImportOptions::default()).unwrap());
    let captured = f.binding("skills/check", ActivationChange::ExplicitInvocation);
    let host = f
        .runtime
        .plugin_hook_activation(
            HookOrigin::Native,
            Scope::Project,
            &ActivationSource::host_namespace("once-fixture").unwrap(),
            "host-hook",
            "worker",
            ActivationChange::ExplicitInvocation,
        )
        .unwrap()
        .unwrap();
    for binding in [captured, host] {
        let result = CommandRunner::registration(
            package.clone(),
            declaration("count", 0, Some(binding)),
            CommandConfig::new(CommandProgram::Argv(vec!["/bin/true".into()])),
            None,
        );
        assert!(result.is_err());
    }
    assert_eq!(f.count("count"), 0);
}
#[test]
fn canonical_source_hash_preserves_distinct_non_utf8_paths() {
    use std::os::unix::ffi::OsStringExt;
    let root = tempfile::tempdir().unwrap();
    let mut identities = Vec::new();
    for byte in [0xfe, 0xff] {
        let path = root
            .path()
            .join(std::ffi::OsString::from_vec(vec![b'p', byte]));
        manifest(&path);
        let package = plugins::inspect(&path, &plugins::ImportOptions::default()).unwrap();
        identities
            .push(serde_json::to_value(ActivationSource::from_package(&package).unwrap()).unwrap());
    }
    assert_ne!(identities[0], identities[1]);
}

#[path = "plugin_once/async_commands.rs"]
mod async_commands;

#[path = "plugin_once/async_external.rs"]
mod async_external;
