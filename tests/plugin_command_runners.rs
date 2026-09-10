use demoncoder::{
    config::Connection,
    events::EventSink,
    native::{Model, NativeSession},
    plugins::{
        self,
        dispatch::*,
        gate_snapshot::GateReadSet,
        hook_types::{HandlerKind, HookDialect},
        runners::{CommandConfig, CommandProgram, CommandRunner, NetworkGrant},
    },
    session::Session,
    tools::{AccessPolicy, ToolCall, ToolExecutor, ToolResult},
    workflow::runtime::{Record, SharedRuntime},
};
use serde_json::json;
use std::{
    collections::VecDeque,
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::mpsc;

static FIXTURES: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
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
struct Fixture {
    root: tempfile::TempDir,
    runtime: SharedRuntime,
    events: EventSink,
    _receiver: mpsc::Receiver<demoncoder::events::Envelope>,
}
impl Fixture {
    fn new() -> Self {
        Self::at(tempfile::tempdir().unwrap())
    }
    fn at(root: tempfile::TempDir) -> Self {
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
    fn executor(&self, registrations: Vec<Registration>, host: bool) -> ToolExecutor {
        self.executor_policy(
            registrations,
            AccessPolicy {
                unrestricted: host,
                supervisor: Some(env!("CARGO_BIN_EXE_demoncoder").into()),
                ..AccessPolicy::default()
            },
        )
    }
    fn executor_policy(
        &self,
        registrations: Vec<Registration>,
        policy: AccessPolicy,
    ) -> ToolExecutor {
        let mut executor = ToolExecutor::with_policy(self.root.path(), &policy).unwrap();
        if !registrations.is_empty() {
            executor
                .register_pre_tool_plan(Arc::new(PreToolPlan::new(registrations).unwrap()))
                .unwrap();
        }
        executor
    }
    fn record(&self) -> Record {
        self.runtime.record().unwrap()
    }
    async fn run(&self, executor: ToolExecutor, calls: Vec<ToolCall>) -> Vec<ToolResult> {
        execute(executor, calls, self.events.clone()).await
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
    let ended = tokio::time::timeout(
        Duration::from_secs(25),
        session.turn("test".into(), &mut commands, &events),
    )
    .await
    .expect("fixture turn timeout");
    if let Err(error) = ended {
        eprintln!("fixture turn held: {error:#}");
    }
    results.lock().unwrap().clone()
}
fn call(path: &str) -> ToolCall {
    ToolCall {
        id: "command-source-1".into(),
        name: "write".into(),
        arguments: json!({"path":path,"content":"written"}),
    }
}
fn declaration(name: &str, dialect: HookDialect, class: HandlerClass) -> Declaration {
    Declaration {
        identity: DeclarationIdentity {
            package: name.into(),
            code: "replaced-by-captured-code".into(),
            policy: "fixture-policy".into(),
            configuration: "replaced-by-config".into(),
            generation: "fixture-generation".into(),
            scope: Scope::Project,
            role: "worker".into(),
            declaration: name.into(),
            index: 0,
            dialect,
            runner: HandlerKind::Command,
        },
        class,
        priority: 0,
        matcher: Matcher {
            tool: Some("write".into()),
            path: None,
        },
        reads: GateReadSet::default(),
        concurrent_group: (dialect == HookDialect::Claude).then(|| "source-group".into()),
        read_only_endpoint: None,
        external_precondition: None,
    }
}
fn package(dialect: HookDialect, code: &str) -> (tempfile::TempDir, Arc<plugins::Package>) {
    let source = tempfile::tempdir().unwrap();
    let metadata = if dialect == HookDialect::Codex {
        ".codex-plugin"
    } else {
        ".claude-plugin"
    };
    std::fs::create_dir(source.path().join(metadata)).unwrap();
    std::fs::write(
        source.path().join(metadata).join("plugin.json"),
        r#"{"name":"command-fixture","version":"1.0.0"}"#,
    )
    .unwrap();
    std::fs::write(source.path().join("hook.py"), code).unwrap();
    let package =
        Arc::new(plugins::inspect(source.path(), &plugins::ImportOptions::default()).unwrap());
    (source, package)
}
fn python_config() -> CommandConfig {
    let mut config = CommandConfig::new(CommandProgram::Argv(vec![
        "/usr/bin/python3".into(),
        "${CLAUDE_PLUGIN_ROOT}/hook.py".into(),
    ]));
    config.model = Some("fixture-model".into());
    config
}
fn register(code: &str, dialect: HookDialect, class: HandlerClass) -> Registration {
    let (_source, package) = package(dialect, code);
    CommandRunner::registration(
        package,
        declaration("main", dialect, class),
        python_config(),
        None,
    )
    .unwrap()
}
const ALLOW: &str = r#"import json,sys
x=json.load(sys.stdin)
assert x['hook_event_name']=='PreToolUse'
assert x['tool_name']=='write'
assert x['tool_input']['content']=='written'
assert x['tool_use_id']=='command-source-1'
print(json.dumps({'hookSpecificOutput':{'hookEventName':'PreToolUse','permissionDecision':'allow'}}))
"#;
fn hook(record: &Record) -> &demoncoder::plugins::receipts::HookReceipt {
    record
        .operations
        .iter()
        .filter_map(|o| o.tool_receipt.as_ref())
        .find_map(|r| r.plugin_admission.as_ref())
        .unwrap()
        .hooks
        .first()
        .unwrap()
}

#[tokio::test]
async fn actual_native_and_source_commands_decode_allow_deny_and_failed_exits() {
    let _fixture_lock = FIXTURES.lock().await;
    for dialect in [HookDialect::Native, HookDialect::Claude, HookDialect::Codex] {
        let allow = if dialect == HookDialect::Codex {
            ALLOW.replace(
                "'permissionDecision':'allow'",
                "'permissionDecision':'allow','updatedInput':x['tool_input']",
            )
        } else {
            ALLOW.into()
        };
        let deny = ALLOW.replace(
            "'permissionDecision':'allow'",
            "'permissionDecision':'deny','permissionDecisionReason':'controlled denial'",
        );
        for (code, allowed) in [
            (allow.clone(), true),
            (deny, false),
            (format!("{allow}\nsys.exit(2)\n"), false),
            ("import sys\nsys.stdout.write('{malformed')\n".into(), false),
            (
                "import sys\nsys.stderr.write('controlled failure')\nsys.exit(1)\n".into(),
                false,
            ),
        ] {
            let fixture = Fixture::new();
            let class = if dialect == HookDialect::Native {
                HandlerClass::DecisionGate
            } else {
                HandlerClass::Combined
            };
            let executor = fixture.executor(vec![register(&code, dialect, class)], false);
            let results = fixture.run(executor, vec![call("result")]).await;
            eprintln!(
                "dialect={dialect:?} expected_allow={allowed} success={}",
                results[0].success
            );
            assert_eq!(results[0].success, allowed);
            assert_eq!(fixture.root.path().join("result").exists(), allowed);
            assert!(matches!(
                hook(&fixture.record()).outcome,
                Some(RawOutcome::Command { .. })
            ));
        }
    }
}

#[tokio::test]
async fn immutable_package_code_and_literal_event_values_are_executed() {
    let _fixture_lock = FIXTURES.lock().await;
    let fixture = Fixture::new();
    let (source, captured) = package(HookDialect::Native, ALLOW);
    let registration = CommandRunner::registration(
        captured.clone(),
        declaration("main", HookDialect::Native, HandlerClass::DecisionGate),
        python_config(),
        None,
    )
    .unwrap();
    std::fs::write(
        source.path().join("hook.py"),
        "raise RuntimeError('live package used')",
    )
    .unwrap();
    assert_eq!(registration.declaration.identity.code, captured.digest());
    let executor = fixture.executor(vec![registration], true);
    let mut request = call("literal");
    request.arguments["content"] = json!("written");
    let results = fixture.run(executor, vec![request]).await;
    assert!(results[0].success, "{results:?}");
}

#[tokio::test]
async fn missing_supervisor_and_malformed_grants_hold_without_fallback() {
    let _fixture_lock = FIXTURES.lock().await;
    let fixture = Fixture::new();
    let executor = fixture.executor_policy(
        vec![register(
            ALLOW,
            HookDialect::Native,
            HandlerClass::DecisionGate,
        )],
        AccessPolicy::default(),
    );
    let results = fixture.run(executor, vec![call("never")]).await;
    assert!(!results[0].success);
    assert!(!fixture.root.path().join("never").exists());
    assert!(matches!(
        hook(&fixture.record()).outcome,
        Some(RawOutcome::CommandFailure { .. })
    ));
    for grant in [
        "../escape",
        "/tmp/escape",
        ".git",
        "a//b",
        "a/./b",
        "a/../b",
    ] {
        let (_source, captured) = package(HookDialect::Native, ALLOW);
        let mut config = python_config();
        config.write_paths = vec![grant.into()];
        assert!(
            CommandRunner::registration(
                captured,
                declaration("main", HookDialect::Native, HandlerClass::Transformer),
                config,
                None
            )
            .is_err()
        );
    }
    for network in [
        NetworkGrant::Host,
        NetworkGrant::Allowlist(vec!["example.org".into()]),
    ] {
        let (_source, captured) = package(HookDialect::Native, ALLOW);
        let mut config = python_config();
        config.network = network;
        let error = CommandRunner::registration(
            captured,
            declaration("main", HookDialect::Native, HandlerClass::DecisionGate),
            config,
            None,
        )
        .err()
        .unwrap();
        assert!(error.to_string().contains("network grants"));
    }
}

#[tokio::test]
async fn readonly_command_confines_files_credentials_fds_sockets_and_outer_environment_in_both_modes()
 {
    let _fixture_lock = FIXTURES.lock().await;
    use std::{io::Write, os::fd::AsRawFd};
    for host in [false, true] {
        let fixture = Fixture::new();
        let outside = tempfile::tempdir().unwrap();
        let home_canary = outside.path().join("home-private");
        std::fs::write(&home_canary, "private-home").unwrap();
        let mut inherited = tempfile::NamedTempFile::new().unwrap();
        inherited.write_all(b"inherited-private").unwrap();
        rustix::io::fcntl_setfd(inherited.as_file(), rustix::io::FdFlags::empty()).unwrap();
        let credential = fixture.root.path().join("ordinary-store");
        std::fs::write(&credential, "custom-credential-secret").unwrap();
        std::fs::create_dir(fixture.root.path().join(".git")).unwrap();
        std::fs::write(fixture.root.path().join(".git/config"), "git-private").unwrap();
        let injected = outside.path().join("outer-executed");
        let bash_env = outside.path().join("bash.env");
        std::fs::write(
            &bash_env,
            format!("printf bad > '{}'\n", injected.display()),
        )
        .unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let code = format!(
            r#"import json,sys,os,socket,errno
x=json.load(sys.stdin)
for path in [{home:?}, 'ordinary-store', '.git/config']:
    try: open(path).read()
    except OSError: pass
    else: raise AssertionError('private file exposed')
try: open('snapshot-write', 'w').write('bad')
except OSError as e: assert e.errno in (errno.EROFS,errno.EACCES,errno.EPERM)
else: raise AssertionError('snapshot was writable')
for name in os.listdir('/proc/self/fd'):
    if int(name)<3: continue
    try: os.fstat(int(name))
    except OSError: pass
    else: raise AssertionError('inherited mount/lifetime/private descriptor exposed: '+name)
try: socket.socket(socket.AF_UNIX,socket.SOCK_STREAM)
except PermissionError: pass
else: raise AssertionError('Unix socket creation was allowed')
s=socket.socket();s.settimeout(.1)
try: s.connect(('127.0.0.1',{port}))
except OSError: pass
else: raise AssertionError('host network accessible')
assert os.environ['BASH_ENV']=={bash_env:?}
assert 'SSH_AUTH_SOCK' not in os.environ
assert 'LD_PRELOAD' not in os.environ
print(json.dumps({{'hookSpecificOutput':{{'hookEventName':'PreToolUse','permissionDecision':'allow'}}}}))
"#,
            home = home_canary.to_str().unwrap(),
            port = listener.local_addr().unwrap().port(),
            bash_env = bash_env.to_str().unwrap()
        );
        let (_source, captured) = package(HookDialect::Native, &code);
        let mut config = python_config();
        config.program =
            CommandProgram::Shell("exec /usr/bin/python3 \"$CLAUDE_PLUGIN_ROOT/hook.py\"".into());
        config
            .environment
            .insert("BASH_ENV".into(), bash_env.to_str().unwrap().into());
        let registration = CommandRunner::registration(
            captured,
            declaration("main", HookDialect::Native, HandlerClass::DecisionGate),
            config,
            None,
        )
        .unwrap();
        let executor = fixture.executor_policy(
            vec![registration],
            AccessPolicy {
                unrestricted: host,
                credential_paths: vec![credential],
                supervisor: Some(env!("CARGO_BIN_EXE_demoncoder").into()),
                ..AccessPolicy::default()
            },
        );
        let results = fixture.run(executor, vec![call("allowed")]).await;
        assert!(
            results[0].success,
            "host={host} {results:?}; {:?}",
            hook(&fixture.record()).outcome
        );
        assert!(!injected.exists());
        assert!(!fixture.root.path().join("snapshot-write").exists());
        assert!(matches!(listener.accept(), Err(e) if e.kind()==std::io::ErrorKind::WouldBlock));
        assert!(inherited.as_raw_fd() > 2);
    }
}

#[tokio::test]
async fn explicit_custom_credential_paths_globs_and_absence_hold_before_gate_execution() {
    let _fixture_lock = FIXTURES.lock().await;
    for (reads, exists) in [
        (
            GateReadSet::new(vec!["ordinary-store".into()], vec![], vec![]).unwrap(),
            true,
        ),
        (
            GateReadSet::new(vec![], vec!["ordinary-*".into()], vec![]).unwrap(),
            true,
        ),
        (
            GateReadSet::new(vec![], vec![], vec!["ordinary-store".into()]).unwrap(),
            false,
        ),
    ] {
        let fixture = Fixture::new();
        let credential = fixture.root.path().join("ordinary-store");
        if exists {
            std::fs::write(&credential, "private").unwrap();
        }
        let mut registration = register(ALLOW, HookDialect::Native, HandlerClass::DecisionGate);
        registration.declaration.reads = reads;
        let executor = fixture.executor_policy(
            vec![registration],
            AccessPolicy {
                credential_paths: vec![credential],
                supervisor: Some(env!("CARGO_BIN_EXE_demoncoder").into()),
                ..AccessPolicy::default()
            },
        );
        let results = fixture.run(executor, vec![call("held")]).await;
        assert!(!results[0].success);
        assert!(!fixture.root.path().join("held").exists());
        let record = fixture.record();
        let plan = record
            .operations
            .iter()
            .filter_map(|o| o.tool_receipt.as_ref())
            .find_map(|r| r.plugin_admission.as_ref())
            .unwrap();
        assert!(
            plan.hooks.is_empty(),
            "gate ran against protected required evidence"
        );
    }
}

#[tokio::test]
async fn changed_credential_alias_and_workspace_inside_store_are_rejected() {
    let _fixture_lock = FIXTURES.lock().await;
    let fixture = Fixture::new();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(fixture.root.path().join("one"), "private").unwrap();
    std::fs::write(fixture.root.path().join("two"), "private").unwrap();
    let alias = outside.path().join("credential-alias");
    std::os::unix::fs::symlink(fixture.root.path().join("one"), &alias).unwrap();
    let executor = fixture.executor_policy(
        vec![register(
            ALLOW,
            HookDialect::Native,
            HandlerClass::DecisionGate,
        )],
        AccessPolicy {
            credential_paths: vec![alias.clone()],
            supervisor: Some(env!("CARGO_BIN_EXE_demoncoder").into()),
            ..AccessPolicy::default()
        },
    );
    std::fs::remove_file(&alias).unwrap();
    std::os::unix::fs::symlink(fixture.root.path().join("two"), &alias).unwrap();
    let results = fixture.run(executor, vec![call("never")]).await;
    assert!(!results[0].success);
    assert!(
        ToolExecutor::with_policy(
            fixture.root.path(),
            &AccessPolicy {
                credential_paths: vec![fixture.root.path().into()],
                ..AccessPolicy::default()
            }
        )
        .is_err()
    );
    assert!(
        ToolExecutor::with_policy(
            fixture.root.path(),
            &AccessPolicy {
                credential_paths: vec![fixture.root.path().join("a/../one")],
                ..AccessPolicy::default()
            }
        )
        .is_err()
    );
}

#[tokio::test]
async fn literal_shell_input_and_explicit_mutation_grant_stay_inside_declared_scope() {
    let _fixture_lock = FIXTURES.lock().await;
    let fixture = Fixture::new();
    std::fs::create_dir(fixture.root.path().join("effects")).unwrap();
    let code = r#"import sys,json,os,errno
x=json.load(sys.stdin)
assert x['tool_input']['content']=='$(touch effects/injected); `touch effects/second`'
open('effects/allowed','w').write('one effect')
try: open('outside-scope','w').write('bad')
except OSError as e: assert e.errno in (errno.EROFS,errno.EPERM,errno.EACCES)
else: raise AssertionError('write escaped declared scope')
print('{}')
"#;
    let (_source, captured) = package(HookDialect::Native, code);
    let mut config = python_config();
    config.program = CommandProgram::Shell("exec python3 \"$CLAUDE_PLUGIN_ROOT/hook.py\"".into());
    config.write_paths = vec!["effects".into()];
    let registration = CommandRunner::registration(
        captured,
        declaration("main", HookDialect::Native, HandlerClass::Transformer),
        config,
        None,
    )
    .unwrap();
    let mut request = call("result");
    request.arguments["content"] = json!("$(touch effects/injected); `touch effects/second`");
    let executor = fixture.executor(vec![registration], false);
    let results = fixture.run(executor, vec![request]).await;
    assert!(
        results[0].success,
        "{results:?}; {:?}",
        hook(&fixture.record()).outcome
    );
    assert_eq!(
        std::fs::read_to_string(fixture.root.path().join("effects/allowed")).unwrap(),
        "one effect"
    );
    for path in ["outside-scope", "effects/injected", "effects/second"] {
        assert!(!fixture.root.path().join(path).exists());
    }
}

#[tokio::test]
async fn bounded_input_timeout_and_output_flood_retain_failure_diagnostics() {
    let _fixture_lock = FIXTURES.lock().await;
    for kind in ["input", "timeout", "flood", "exec"] {
        let fixture = Fixture::new();
        let code = if kind == "flood" {
            "import os\nos.write(1,b'out-prefix'+b'x'*4000)\nos.write(2,b'err-prefix'+b'y'*4000)\nwhile True:\n os.write(1,b'x'*4096)\n os.write(2,b'y'*4096)\n"
        } else {
            "import sys,time\nprint('partial-out',flush=True)\nprint('partial-err',file=sys.stderr,flush=True)\ntime.sleep(10)\n"
        };
        let (_source, captured) = package(HookDialect::Native, code);
        let mut config = python_config();
        config.max_output_bytes = 8192;
        if kind == "timeout" {
            config.timeout_ms = 200;
        }
        if kind == "input" {
            config.max_input_bytes = 20;
        }
        if kind == "exec" {
            config.program = CommandProgram::Argv(vec!["/missing-runtime".into()]);
        }
        let registration = CommandRunner::registration(
            captured,
            declaration("main", HookDialect::Native, HandlerClass::DecisionGate),
            config,
            None,
        )
        .unwrap();
        let executor = fixture.executor(vec![registration], false);
        let results = fixture.run(executor, vec![call("never")]).await;
        assert!(!results[0].success, "{kind}: {results:?}");
        let record = fixture.record();
        let receipt = hook(&record);
        match receipt.outcome.as_ref().unwrap() {
            RawOutcome::CommandFailure {
                reason,
                stdout,
                stderr,
            } => {
                assert_ne!(kind, "exec");
                assert!(receipt.uncertain_effects);
                assert!(stdout.len() + stderr.len() <= 8192);
                if kind != "input" {
                    assert!(!stdout.is_empty() && !stderr.is_empty(), "{kind}: {reason}");
                }
            }
            RawOutcome::Command { exit_code, .. } if kind == "exec" => {
                assert_ne!(*exit_code, Some(0));
            }
            other => panic!("{kind}: unexpected result {other:?}"),
        }
        assert!(!fixture.root.path().join("never").exists());
    }
}

struct OwnedTask<T>(tokio::task::JoinHandle<T>);
impl<T> Drop for OwnedTask<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}
async fn wait_file(path: &Path) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !path.exists() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("fixture file was not produced before deadline");
}
fn all_hooks(record: &Record) -> Vec<&demoncoder::plugins::receipts::HookReceipt> {
    record
        .operations
        .iter()
        .filter_map(|o| o.tool_receipt.as_ref())
        .filter_map(|r| r.plugin_admission.as_ref())
        .flat_map(|p| &p.hooks)
        .collect()
}

#[tokio::test]
async fn immutable_dirty_and_absent_inputs_hold_actual_tool_when_live_workspace_changes() {
    let _fixture_lock = FIXTURES.lock().await;
    let fixture = Fixture::new();
    std::fs::write(fixture.root.path().join("dirty"), "captured-A").unwrap();
    let code = r#"import json,sys,pathlib,time
json.load(sys.stdin)
assert pathlib.Path('dirty').read_text()=='captured-A'
assert not pathlib.Path('later').exists()
time.sleep(.15)
print(json.dumps({'hookSpecificOutput':{'hookEventName':'PreToolUse','permissionDecision':'allow'}}))
"#;
    let mut registration = register(code, HookDialect::Native, HandlerClass::DecisionGate);
    registration.declaration.reads =
        GateReadSet::new(vec!["dirty".into()], vec![], vec!["later".into()]).unwrap();
    let executor = fixture.executor(vec![registration], false);
    let mut running = OwnedTask(tokio::spawn(execute(
        executor,
        vec![call("guarded")],
        fixture.events.clone(),
    )));
    tokio::time::timeout(Duration::from_secs(5), async {
        while all_hooks(&fixture.record()).is_empty() {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();
    // Durable reservation follows capture, so even a not-yet-launched process
    // must receive A from retained evidence after these actual live mutations.
    std::fs::write(fixture.root.path().join("dirty"), "live-B").unwrap();
    std::fs::write(fixture.root.path().join("later"), "new-untracked").unwrap();
    let results = (&mut running.0).await.unwrap();
    assert!(
        !results[0].success,
        "stale snapshot released effect: {results:?}"
    );
    assert!(!fixture.root.path().join("guarded").exists());
    assert!(
        matches!(
            hook(&fixture.record()).outcome,
            Some(RawOutcome::Command {
                exit_code: Some(0),
                ..
            })
        ),
        "runner did not inspect retained A: {:?}",
        hook(&fixture.record()).outcome
    );
}

#[tokio::test]
async fn actual_combined_rewrite_uses_readonly_revalidation_without_replaying_effects() {
    let _fixture_lock = FIXTURES.lock().await;
    for deny in [true, false] {
        let fixture = Fixture::new();
        std::fs::create_dir(fixture.root.path().join("effects")).unwrap();
        std::fs::create_dir(fixture.root.path().join("generated")).unwrap();
        let code = format!(
            r#"import json,sys,errno
x=json.load(sys.stdin)
if sys.argv[-1]=='revalidate':
    assert x['tool_input']['path']=='generated/result'
    try: open('effects/replayed','w').write('bad')
    except OSError as e: assert e.errno in (errno.EROFS,errno.EACCES,errno.EPERM)
    else: raise AssertionError('revalidation was writable')
    result={{'hookEventName':'PreToolUse','permissionDecision':{choice:?}}}
else:
    with open('effects/count','a') as f: f.write('1')
    x['tool_input']['path']='generated/result'
    result={{'hookEventName':'PreToolUse','permissionDecision':'allow','updatedInput':x['tool_input']}}
print(json.dumps({{'hookSpecificOutput':result}}))
"#,
            choice = if deny { "deny" } else { "allow" }
        );
        let (_source, captured) = package(HookDialect::Claude, &code);
        let mut normal = python_config();
        normal.write_paths = vec!["effects".into()];
        let mut revalidation = python_config();
        if let CommandProgram::Argv(args) = &mut revalidation.program {
            args.push("revalidate".into());
        }
        let mut declared = declaration("main", HookDialect::Claude, HandlerClass::Combined);
        declared.read_only_endpoint = Some("decision-only".into());
        let registration =
            CommandRunner::registration(captured, declared, normal, Some(revalidation)).unwrap();
        let executor = fixture.executor(vec![registration], false);
        let results = fixture.run(executor, vec![call("original")]).await;
        assert_eq!(
            results[0].success,
            !deny,
            "{results:?}; {:?}",
            all_hooks(&fixture.record())
        );
        assert_eq!(
            std::fs::read_to_string(fixture.root.path().join("effects/count")).unwrap(),
            "1"
        );
        assert!(!fixture.root.path().join("effects/replayed").exists());
        assert!(!fixture.root.path().join("original").exists());
        assert_eq!(fixture.root.path().join("generated/result").exists(), !deny);
        let record = fixture.record();
        let receipts = all_hooks(&record);
        assert_eq!(receipts.len(), 2);
        assert_eq!(receipts[1].endpoint.as_deref(), Some("decision-only"));
        assert_eq!(receipts[1].class, HandlerClass::DecisionGate);
    }
}

#[tokio::test]
async fn effectful_claude_source_group_rendezvous_starts_both_commands_under_one_guard() {
    let _fixture_lock = FIXTURES.lock().await;
    let fixture = Fixture::new();
    std::fs::create_dir(fixture.root.path().join("effects")).unwrap();
    let code = r#"import json,sys,pathlib,time
x=json.load(sys.stdin)
me,other=sys.argv[1:3]
pathlib.Path('effects/'+me).write_text('started')
limit=time.monotonic()+2
while not pathlib.Path('effects/'+other).exists():
    assert time.monotonic()<limit,'source peer did not start'
    time.sleep(.005)
print(json.dumps({'hookSpecificOutput':{'hookEventName':'PreToolUse','permissionDecision':'allow'}}))
"#;
    let (_source, captured) = package(HookDialect::Claude, code);
    let mut registrations = Vec::new();
    for (index, (me, other)) in [("first", "second"), ("second", "first")]
        .into_iter()
        .enumerate()
    {
        let mut declared = declaration(me, HookDialect::Claude, HandlerClass::Combined);
        declared.identity.index = index as u32;
        let mut config = python_config();
        config.write_paths = vec!["effects".into()];
        if let CommandProgram::Argv(args) = &mut config.program {
            args.extend([me.into(), other.into()]);
        }
        registrations
            .push(CommandRunner::registration(captured.clone(), declared, config, None).unwrap());
    }
    let executor = fixture.executor(registrations, false);
    let results = fixture.run(executor, vec![call("guarded")]).await;
    assert!(
        !results[0].success,
        "combined decision must need revalidation after its writes"
    );
    for name in ["first", "second"] {
        assert_eq!(
            std::fs::read_to_string(fixture.root.path().join("effects").join(name)).unwrap(),
            "started"
        );
    }
    let record = fixture.record();
    let receipts = all_hooks(&record);
    assert_eq!(receipts.len(), 2);
    for receipt in receipts {
        assert!(
            matches!(
                receipt.outcome,
                Some(RawOutcome::Command {
                    exit_code: Some(0),
                    ..
                })
            ),
            "source rendezvous failed: {:?}",
            receipt.outcome
        );
    }
}

#[tokio::test]
async fn actual_mutating_hook_serializes_no_plan_executor_while_other_workspace_progresses() {
    let _fixture_lock = FIXTURES.lock().await;
    let fixture = Fixture::new();
    std::fs::create_dir(fixture.root.path().join("effects")).unwrap();
    let code = r#"import json,sys,pathlib,time
json.load(sys.stdin)
pathlib.Path('effects/started').write_text('started')
limit=time.monotonic()+3
while not pathlib.Path('effects/release').exists():
    assert time.monotonic()<limit
    time.sleep(.005)
print('{}')
"#;
    let (_source, captured) = package(HookDialect::Native, code);
    let mut config = python_config();
    config.write_paths = vec!["effects".into()];
    let registration = CommandRunner::registration(
        captured,
        declaration("main", HookDialect::Native, HandlerClass::Transformer),
        config,
        None,
    )
    .unwrap();
    let executor = fixture.executor(vec![registration], false);
    let mut running = OwnedTask(tokio::spawn(execute(
        executor,
        vec![call("hook-result")],
        fixture.events.clone(),
    )));
    wait_file(&fixture.root.path().join("effects/started")).await;
    let plain = fixture.executor(Vec::new(), false);
    let mut request = call("plain-result");
    request.id = "plain-owner".into();
    let mut waiting = OwnedTask(tokio::spawn(execute(
        plain,
        vec![request],
        fixture.events.clone(),
    )));
    let other = tempfile::tempdir().unwrap();
    let independent = ToolExecutor::new(other.path()).unwrap();
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        fixture.run(independent, vec![call("independent")]),
    )
    .await
    .expect("other workspace was serialized");
    assert!(result[0].success);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        !fixture.root.path().join("plain-result").exists(),
        "no-plan executor bypassed hook mutation guard"
    );
    std::fs::write(fixture.root.path().join("effects/release"), "go").unwrap();
    let _ = (&mut running.0).await.unwrap();
    assert!((&mut waiting.0).await.unwrap()[0].success);
}

#[tokio::test]
async fn identity_changes_and_readonly_class_changes_fail_before_code_execution() {
    let _fixture_lock = FIXTURES.lock().await;
    for field in ["code", "configuration", "class"] {
        let fixture = Fixture::new();
        std::fs::create_dir(fixture.root.path().join("effects")).unwrap();
        let (_source, captured) = package(
            HookDialect::Native,
            "open('effects/ran','w').write('bad')\nprint('{}')\n",
        );
        let mut config = python_config();
        config.write_paths = vec!["effects".into()];
        let mut registration = CommandRunner::registration(
            captured,
            declaration("main", HookDialect::Native, HandlerClass::Transformer),
            config,
            None,
        )
        .unwrap();
        match field {
            "code" => registration.declaration.identity.code = "changed".into(),
            "configuration" => registration.declaration.identity.configuration = "changed".into(),
            _ => registration.declaration.class = HandlerClass::DecisionGate,
        }
        let executor = fixture.executor(vec![registration], false);
        let results = fixture.run(executor, vec![call("never")]).await;
        assert!(!results[0].success);
        assert!(!fixture.root.path().join("effects/ran").exists());
        assert!(matches!(
            hook(&fixture.record()).outcome,
            Some(RawOutcome::CommandFailure { .. })
        ));
    }
}

struct PinnedProcess(std::os::fd::OwnedFd);
impl PinnedProcess {
    fn open(pid: u32) -> Self {
        Self(
            rustix::process::pidfd_open(
                rustix::process::Pid::from_raw(pid as i32).unwrap(),
                rustix::process::PidfdFlags::NONBLOCK,
            )
            .unwrap(),
        )
    }
    fn stop(&self) {
        let _ = rustix::process::pidfd_send_signal(&self.0, rustix::process::Signal::KILL);
    }
    fn stopped(&self) -> bool {
        let mut fds = [rustix::event::PollFd::new(
            &self.0,
            rustix::event::PollFlags::IN,
        )];
        rustix::event::poll(
            &mut fds,
            Some(&rustix::event::Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            }),
        )
        .unwrap();
        fds[0]
            .revents()
            .intersects(rustix::event::PollFlags::IN | rustix::event::PollFlags::HUP)
    }
}
impl Drop for PinnedProcess {
    fn drop(&mut self) {
        self.stop();
    }
}
fn parent_pid(pid: u32) -> u32 {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).unwrap();
    status
        .lines()
        .find_map(|line| line.strip_prefix("PPid:"))
        .unwrap()
        .trim()
        .parse()
        .unwrap()
}
fn sandbox_owners(token: &str) -> (PinnedProcess, PinnedProcess) {
    let payload = std::fs::read_dir("/proc")
        .unwrap()
        .filter_map(Result::ok)
        .find_map(|entry| {
            let pid = entry.file_name().to_str()?.parse::<u32>().ok()?;
            let command = std::fs::read(entry.path().join("cmdline")).ok()?;
            let fields: Vec<_> = command.split(|b| *b == 0).collect();
            (fields.first().is_some_and(|s| s.ends_with(b"python3"))
                && fields.contains(&token.as_bytes()))
            .then_some(pid)
        })
        .expect("running package Python process");
    let namespace = parent_pid(payload);
    let monitor = parent_pid(namespace);
    let supervisor = parent_pid(monitor);
    let command = std::fs::read(format!("/proc/{supervisor}/cmdline")).unwrap();
    assert!(
        command
            .split(|b| *b == 0)
            .any(|arg| arg == b"--supervise-hook")
    );
    (
        PinnedProcess::open(supervisor),
        PinnedProcess::open(namespace),
    )
}

#[tokio::test]
async fn command_descendants_stop_and_guard_releases_after_exit_timeout_cancel_hold_and_supervisor_death()
 {
    let _fixture_lock = FIXTURES.lock().await;
    for mode in ["exit", "timeout", "cancel", "hold", "supervisor-kill"] {
        let fixture = Fixture::new();
        std::fs::create_dir(fixture.root.path().join("effects")).unwrap();
        let token = format!(
            "demoncoder-lifetime-{}-{mode}",
            fixture.root.path().file_name().unwrap().to_string_lossy()
        );
        let code = r#"import os,time,pathlib,sys,json
json.load(sys.stdin)
if os.fork()==0:
    os.setsid()
    if os.fork(): os._exit(0)
    for fd in (0,1,2): os.close(fd)
    pathlib.Path('effects/child-ready').touch()
    while True:
        with open('effects/heartbeat','a') as f: f.write('x')
        time.sleep(.005)
while not pathlib.Path('effects/child-ready').exists(): time.sleep(.005)
pathlib.Path('effects/started').touch()
while not pathlib.Path('effects/release').exists(): time.sleep(.005)
print('{}',flush=True)
"#;
        let (_source, captured) = package(HookDialect::Native, code);
        let mut config = python_config();
        config.write_paths = vec!["effects".into()];
        if mode == "timeout" {
            config.timeout_ms = 800;
        }
        if let CommandProgram::Argv(args) = &mut config.program {
            args.push(token.clone());
        }
        let registration = CommandRunner::registration(
            captured,
            declaration("main", HookDialect::Native, HandlerClass::Transformer),
            config,
            None,
        )
        .unwrap();
        let executor = fixture.executor(vec![registration], false);
        let mut running = OwnedTask(tokio::spawn(execute(
            executor,
            vec![call("original-result")],
            fixture.events.clone(),
        )));
        wait_file(&fixture.root.path().join("effects/started")).await;
        let (supervisor, namespace) = sandbox_owners(&token);
        match mode {
            "exit" => std::fs::write(fixture.root.path().join("effects/release"), "go").unwrap(),
            "cancel" => running.0.abort(),
            "hold" => fixture.runtime.hold().unwrap(),
            "supervisor-kill" => supervisor.stop(),
            _ => {}
        }
        if mode != "cancel" {
            let results = tokio::time::timeout(Duration::from_secs(4), &mut running.0)
                .await
                .expect("command owner failed to settle")
                .unwrap();
            assert_eq!(results[0].success, mode == "exit", "{mode}: {results:?}");
        } else {
            assert!((&mut running.0).await.unwrap_err().is_cancelled());
        }
        fixture.runtime.reconcile("Fixture inspected its own stopped or stopping process; exercise the existing workspace mutation boundary", None).unwrap();
        let plain = fixture.executor(Vec::new(), false);
        let mut follow_up = call("follow-up");
        follow_up.id = "follow-up-owner".into();
        let results =
            tokio::time::timeout(Duration::from_secs(4), fixture.run(plain, vec![follow_up]))
                .await
                .expect("sandbox teardown stranded its mutation guard");
        assert!(results[0].success, "{mode}: {results:?}");
        assert!(
            namespace.stopped(),
            "{mode}: guard released while namespace init was alive"
        );
        let count = std::fs::metadata(fixture.root.path().join("effects/heartbeat"))
            .unwrap()
            .len();
        tokio::time::sleep(Duration::from_millis(80)).await;
        assert_eq!(
            std::fs::metadata(fixture.root.path().join("effects/heartbeat"))
                .unwrap()
                .len(),
            count,
            "{mode}: detached descendant wrote after ownership ended"
        );
    }
}

#[tokio::test]
async fn cancelled_or_held_owner_waiting_for_mutation_guard_cannot_start_its_command() {
    let _fixture_lock = FIXTURES.lock().await;
    for mode in ["cancel", "hold"] {
        let fixture = Fixture::new();
        std::fs::create_dir(fixture.root.path().join("effects")).unwrap();
        let blocker = "import pathlib,time\npathlib.Path('effects/blocker-started').touch()\nwhile not pathlib.Path('effects/release').exists(): time.sleep(.005)\nprint('{}')\n";
        let build = |source: &str| {
            let (_source, captured) = package(HookDialect::Native, source);
            let mut config = python_config();
            config.write_paths = vec!["effects".into()];
            fixture.executor(
                vec![
                    CommandRunner::registration(
                        captured,
                        declaration("main", HookDialect::Native, HandlerClass::Transformer),
                        config,
                        None,
                    )
                    .unwrap(),
                ],
                false,
            )
        };
        let mut blocking = OwnedTask(tokio::spawn(execute(
            build(blocker),
            vec![call("blocking-result")],
            fixture.events.clone(),
        )));
        wait_file(&fixture.root.path().join("effects/blocker-started")).await;
        let mut waiting_call = call("waiting-result");
        waiting_call.id = "waiting-source".into();
        let mut waiting = OwnedTask(tokio::spawn(execute(
            build("open('effects/waiting-ran','w').write('bad')\nprint('{}')\n"),
            vec![waiting_call],
            fixture.events.clone(),
        )));
        tokio::time::sleep(Duration::from_millis(40)).await;
        assert!(!fixture.root.path().join("effects/waiting-ran").exists());
        if mode == "cancel" {
            waiting.0.abort();
            assert!((&mut waiting.0).await.unwrap_err().is_cancelled());
        } else {
            fixture.runtime.hold().unwrap();
        }
        std::fs::write(fixture.root.path().join("effects/release"), "go").unwrap();
        let _ = (&mut blocking.0).await.unwrap();
        if mode == "hold" {
            let _ = (&mut waiting.0).await.unwrap();
        }
        assert!(
            !fixture.root.path().join("effects/waiting-ran").exists(),
            "{mode}: waiting owner launched after revocation"
        );
        assert!(!fixture.root.path().join("waiting-result").exists());
        assert_eq!(
            all_hooks(&fixture.record()).len(),
            1,
            "waiting command must not reserve an executable receipt"
        );
    }
}

#[tokio::test]
async fn unknown_command_effects_are_not_automatically_replayed_on_retry() {
    let _fixture_lock = FIXTURES.lock().await;
    let fixture = Fixture::new();
    std::fs::create_dir(fixture.root.path().join("effects")).unwrap();
    let (_source, captured) = package(
        HookDialect::Native,
        "import time\nwith open('effects/count','a') as f: f.write('1')\ntime.sleep(10)\nprint('{}')\n",
    );
    let mut config = python_config();
    config.write_paths = vec!["effects".into()];
    config.timeout_ms = 250;
    for attempt in 0..2 {
        let registration = CommandRunner::registration(
            captured.clone(),
            declaration("main", HookDialect::Native, HandlerClass::Transformer),
            config.clone(),
            None,
        )
        .unwrap();
        let results = fixture
            .run(
                fixture.executor(vec![registration], false),
                vec![call("never")],
            )
            .await;
        assert!(
            results.iter().all(|result| !result.success),
            "attempt {attempt} released uncertain effect"
        );
        assert_eq!(
            std::fs::read_to_string(fixture.root.path().join("effects/count")).unwrap(),
            "1"
        );
        assert_eq!(all_hooks(&fixture.record()).len(), 1);
        assert!(!fixture.root.path().join("never").exists());
    }
}

#[test]
fn command_owner_death_fixture() {
    let Some(base) = std::env::var_os("DEMONCODER_COMMAND_OWNER_FIXTURE") else {
        return;
    };
    let base = std::path::PathBuf::from(base);
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
        let fixture = Fixture::at(tempfile::tempdir_in(&base).unwrap());
        std::fs::write(base.join("workspace"), fixture.root.path().as_os_str().as_encoded_bytes()).unwrap();
        std::fs::write(base.join("runtime"), fixture.runtime.directory().unwrap().as_os_str().as_encoded_bytes()).unwrap();
        std::fs::create_dir(fixture.root.path().join("effects")).unwrap();
        let token = format!("owner-death-{}", base.file_name().unwrap().to_string_lossy());
        let code = "import os,time,pathlib\nif os.fork()==0:\n os.setsid()\n if os.fork(): os._exit(0)\n for fd in (0,1,2): os.close(fd)\n pathlib.Path('effects/child-ready').touch()\n while True:\n  with open('effects/heartbeat','a') as f: f.write('x')\n  time.sleep(.005)\nwhile not pathlib.Path('effects/child-ready').exists(): time.sleep(.005)\npathlib.Path('effects/started').touch()\ntime.sleep(10)\n";
        let (_source, captured) = package(HookDialect::Native, code);
        let mut config = python_config(); config.write_paths = vec!["effects".into()];
        if let CommandProgram::Argv(args) = &mut config.program { args.push(token); }
        let registration = CommandRunner::registration(captured, declaration("main", HookDialect::Native, HandlerClass::Transformer), config, None).unwrap();
        fixture.run(fixture.executor(vec![registration], false), vec![call("never")]).await;
    });
}

#[tokio::test]
async fn killing_actual_app_owner_stops_its_detached_hook_descendants() {
    let _fixture_lock = FIXTURES.lock().await;
    let base = tempfile::tempdir().unwrap();
    let mut owner = tokio::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "command_owner_death_fixture", "--nocapture"])
        .env("DEMONCODER_COMMAND_OWNER_FIXTURE", base.path())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    wait_file(&base.path().join("workspace")).await;
    let workspace =
        std::path::PathBuf::from(std::fs::read_to_string(base.path().join("workspace")).unwrap());
    wait_file(&workspace.join("effects/started")).await;
    let token = format!(
        "owner-death-{}",
        base.path().file_name().unwrap().to_string_lossy()
    );
    let (supervisor, namespace) = sandbox_owners(&token);
    owner.kill().await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while !supervisor.stopped() || !namespace.stopped() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("hook process tree survived app owner death");
    let count = std::fs::metadata(workspace.join("effects/heartbeat"))
        .unwrap()
        .len();
    tokio::time::sleep(Duration::from_millis(80)).await;
    assert_eq!(
        std::fs::metadata(workspace.join("effects/heartbeat"))
            .unwrap()
            .len(),
        count
    );
    assert!(!workspace.join("never").exists());
    let directory = std::fs::read_to_string(base.path().join("runtime")).unwrap();
    std::fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
async fn hook_parent_grants_keep_default_private_files_hidden_and_public_templates_available() {
    let _fixture_lock = FIXTURES.lock().await;
    let private = [
        ".env",
        ".env.production",
        ".pypirc",
        ".npmrc",
        ".config/git/credentials",
    ];
    for host in [false, true] {
        for grant in [".", "effects"] {
            let fixture = Fixture::new();
            std::fs::create_dir_all(fixture.root.path().join("effects/.config/git")).unwrap();
            for name in private {
                std::fs::write(
                    fixture.root.path().join("effects").join(name),
                    "synthetic-private-canary",
                )
                .unwrap();
            }
            std::fs::write(
                fixture.root.path().join("effects/.env.example"),
                "public-template",
            )
            .unwrap();
            let code = format!(
                r#"import pathlib,json,sys
json.load(sys.stdin)
assert pathlib.Path('effects/.env.example').read_text()=='public-template'
for name in {private:?}:
    try: value=pathlib.Path('effects',name).read_text()
    except OSError: pass
    else:
        pathlib.Path('effects/exposed').write_text(value)
        raise AssertionError('private export path was visible through live grant')
pathlib.Path('effects/public-effect').write_text('allowed')
print('{{}}')
"#
            );
            let (_source, captured) = package(HookDialect::Native, &code);
            let mut config = python_config();
            config.write_paths = vec![grant.into()];
            let registration = CommandRunner::registration(
                captured,
                declaration(
                    "private-grant",
                    HookDialect::Native,
                    HandlerClass::Transformer,
                ),
                config,
                None,
            )
            .unwrap();
            let results = fixture
                .run(
                    fixture.executor(vec![registration], host),
                    vec![call("result")],
                )
                .await;
            assert!(
                results[0].success,
                "host={host} grant={grant}: {results:?}; {:?}",
                hook(&fixture.record()).outcome
            );
            assert!(!fixture.root.path().join("effects/exposed").exists());
            assert_eq!(
                std::fs::read_to_string(fixture.root.path().join("effects/public-effect")).unwrap(),
                "allowed"
            );
        }
        for name in private {
            let (_source, captured) = package(
                HookDialect::Native,
                "raise AssertionError('direct private grant ran')",
            );
            let mut config = python_config();
            config.write_paths = vec![format!("effects/{name}")];
            assert!(
                CommandRunner::registration(
                    captured,
                    declaration(
                        "private-direct",
                        HookDialect::Native,
                        HandlerClass::Transformer
                    ),
                    config,
                    None
                )
                .is_err()
            );
        }
    }
}

#[tokio::test]
async fn frozen_absolute_workspace_relative_and_cwd_relative_credentials_survive_parent_grants() {
    let _fixture_lock = FIXTURES.lock().await;
    for host in [false, true] {
        for spelling in ["absolute", "workspace-relative", "cwd-relative", "alias"] {
            for grant in [
                None,
                Some("."),
                Some("effects"),
                Some("effects/ordinary-store"),
            ] {
                let root = if spelling == "cwd-relative" {
                    tempfile::tempdir_in(std::env::current_dir().unwrap()).unwrap()
                } else {
                    tempfile::tempdir().unwrap()
                };
                let fixture = Fixture::at(root);
                std::fs::create_dir(fixture.root.path().join("effects")).unwrap();
                let secret = fixture.root.path().join("effects/ordinary-store");
                std::fs::write(&secret, "synthetic-declared-canary").unwrap();
                let alias_owner = tempfile::tempdir().unwrap();
                let credential = match spelling {
                    "absolute" => secret.clone(),
                    "workspace-relative" => "effects/ordinary-store".into(),
                    "cwd-relative" => secret
                        .strip_prefix(std::env::current_dir().unwrap())
                        .unwrap()
                        .to_path_buf(),
                    _ => {
                        let alias = alias_owner.path().join("credential-alias");
                        std::os::unix::fs::symlink(&secret, &alias).unwrap();
                        alias
                    }
                };
                let code = "import pathlib,json,sys\njson.load(sys.stdin)\ntry: value=pathlib.Path('effects/ordinary-store').read_text()\nexcept OSError: pass\nelse:\n print(value,file=sys.stderr)\n raise AssertionError('declared credential was visible')\nprint(json.dumps({'hookSpecificOutput':{'hookEventName':'PreToolUse','permissionDecision':'allow'}}))\n";
                let code = if grant.is_some() {
                    code.replace("print(json.dumps({'hookSpecificOutput':{'hookEventName':'PreToolUse','permissionDecision':'allow'}}))", "print('{}')")
                } else {
                    code.into()
                };
                let (_source, captured) = package(HookDialect::Native, &code);
                let mut config = python_config();
                config.write_paths = grant.map(|value| vec![value.into()]).unwrap_or_default();
                let class = if grant.is_some() {
                    HandlerClass::Transformer
                } else {
                    HandlerClass::DecisionGate
                };
                let registration = CommandRunner::registration(
                    captured,
                    declaration("declared-private", HookDialect::Native, class),
                    config,
                    None,
                )
                .unwrap();
                let executor = fixture.executor_policy(
                    vec![registration],
                    AccessPolicy {
                        unrestricted: host,
                        credential_paths: vec![credential],
                        supervisor: Some(env!("CARGO_BIN_EXE_demoncoder").into()),
                        ..AccessPolicy::default()
                    },
                );
                let results = fixture.run(executor, vec![call("result")]).await;
                if grant == Some("effects/ordinary-store") {
                    assert!(!results[0].success, "direct credential grant executed");
                    assert!(
                        matches!(&hook(&fixture.record()).outcome, Some(RawOutcome::CommandFailure { stdout, stderr, .. }) if stdout.is_empty() && stderr.is_empty())
                    );
                    continue;
                }
                assert!(
                    results[0].success,
                    "host={host} spelling={spelling} grant={grant:?}: {results:?}; {:?}",
                    hook(&fixture.record()).outcome
                );
                if let Some(RawOutcome::Command { stdout, stderr, .. }) =
                    &hook(&fixture.record()).outcome
                {
                    for bytes in [stdout, stderr] {
                        assert!(
                            !String::from_utf8_lossy(bytes).contains("synthetic-declared-canary")
                        );
                    }
                } else {
                    panic!("credential control did not execute the confined command");
                }
            }
        }
    }
}

#[tokio::test]
async fn relative_credential_reads_globs_and_absence_hold_before_handler_receipts() {
    let _fixture_lock = FIXTURES.lock().await;
    for host in [false, true] {
        for spelling in ["absolute", "workspace-relative", "cwd-relative", "alias"] {
            for selection in ["read", "glob", "absence"] {
                let root = if spelling == "cwd-relative" {
                    tempfile::tempdir_in(std::env::current_dir().unwrap()).unwrap()
                } else {
                    tempfile::tempdir().unwrap()
                };
                let fixture = Fixture::at(root);
                std::fs::create_dir(fixture.root.path().join("effects")).unwrap();
                let secret = fixture.root.path().join("effects/ordinary-store");
                if selection != "absence" {
                    std::fs::write(&secret, "synthetic-required-canary").unwrap();
                }
                let aliases = tempfile::tempdir().unwrap();
                let credential = match spelling {
                    "absolute" => secret.clone(),
                    "workspace-relative" => "effects/ordinary-store".into(),
                    "cwd-relative" => secret
                        .strip_prefix(std::env::current_dir().unwrap())
                        .unwrap()
                        .to_path_buf(),
                    _ => {
                        let alias = aliases.path().join("store-directory");
                        std::os::unix::fs::symlink(fixture.root.path().join("effects"), &alias)
                            .unwrap();
                        alias.join("ordinary-store")
                    }
                };
                let mut registration =
                    register(ALLOW, HookDialect::Native, HandlerClass::DecisionGate);
                registration.declaration.reads = match selection {
                    "read" => {
                        GateReadSet::new(vec!["effects/ordinary-store".into()], vec![], vec![])
                    }
                    "glob" => GateReadSet::new(vec![], vec!["effects/ordinary-*".into()], vec![]),
                    _ => GateReadSet::new(vec![], vec![], vec!["effects/ordinary-store".into()]),
                }
                .unwrap();
                let executor = fixture.executor_policy(
                    vec![registration],
                    AccessPolicy {
                        unrestricted: host,
                        credential_paths: vec![credential],
                        supervisor: Some(env!("CARGO_BIN_EXE_demoncoder").into()),
                        ..AccessPolicy::default()
                    },
                );
                let results = fixture.run(executor, vec![call("never")]).await;
                assert!(
                    !results[0].success,
                    "host={host} spelling={spelling} selection={selection}"
                );
                assert!(
                    all_hooks(&fixture.record()).is_empty(),
                    "required credential evidence reached a handler"
                );
                assert!(!fixture.root.path().join("never").exists());
            }
        }
    }
}

struct RemapCredential {
    runner: Arc<dyn HookRunner>,
    alias: std::path::PathBuf,
    target: std::path::PathBuf,
}
#[async_trait::async_trait]
impl HookRunner for RemapCredential {
    fn mutates_workspace(&self) -> bool {
        self.runner.mutates_workspace()
    }
    async fn run(&self, invocation: &HookInvocation) -> anyhow::Result<RawOutcome> {
        std::fs::remove_file(&self.alias)?;
        std::os::unix::fs::symlink(&self.target, &self.alias)?;
        self.runner.run(invocation).await
    }
}

#[tokio::test]
async fn credential_alias_remap_after_capture_cannot_expose_old_or_new_targets() {
    let _fixture_lock = FIXTURES.lock().await;
    for host in [false, true] {
        for grant in [None, Some("effects")] {
            let fixture = Fixture::new();
            std::fs::create_dir(fixture.root.path().join("effects")).unwrap();
            for name in ["first", "second"] {
                std::fs::write(
                    fixture.root.path().join("effects").join(name),
                    "synthetic-alias-canary",
                )
                .unwrap();
            }
            let aliases = tempfile::tempdir().unwrap();
            let alias = aliases.path().join("credential");
            std::os::unix::fs::symlink(fixture.root.path().join("effects/first"), &alias).unwrap();
            let code = "import pathlib,json,sys\njson.load(sys.stdin)\nfor name in ['first','second']:\n try: value=pathlib.Path('effects',name).read_text()\n except OSError: pass\n else:\n  print(value,file=sys.stderr)\n  raise AssertionError('old or new credential target exposed after alias remap')\nprint('{}')\n";
            let code = if grant.is_none() {
                code.replace("print('{}')", "print(json.dumps({'hookSpecificOutput':{'hookEventName':'PreToolUse','permissionDecision':'allow'}}))")
            } else {
                code.into()
            };
            let (_source, captured) = package(HookDialect::Native, &code);
            let mut config = python_config();
            config.write_paths = grant.map(|path| vec![path.into()]).unwrap_or_default();
            let class = if grant.is_some() {
                HandlerClass::Transformer
            } else {
                HandlerClass::DecisionGate
            };
            let mut registration = CommandRunner::registration(
                captured,
                declaration("alias-remap", HookDialect::Native, class),
                config,
                None,
            )
            .unwrap();
            registration.runner = Arc::new(RemapCredential {
                runner: registration.runner.clone(),
                alias: alias.clone(),
                target: fixture.root.path().join("effects/second"),
            });
            let executor = fixture.executor_policy(
                vec![registration],
                AccessPolicy {
                    unrestricted: host,
                    credential_paths: vec![alias],
                    supervisor: Some(env!("CARGO_BIN_EXE_demoncoder").into()),
                    ..AccessPolicy::default()
                },
            );
            let results = fixture.run(executor, vec![call("never")]).await;
            assert!(
                !results[0].success,
                "changed credential meaning released the guarded write"
            );
            assert!(
                matches!(&hook(&fixture.record()).outcome, Some(RawOutcome::Command { exit_code: Some(0), stderr, .. }) if stderr.is_empty()),
                "host={host} grant={grant:?}: {:?}",
                hook(&fixture.record()).outcome
            );
            assert!(!fixture.root.path().join("never").exists());
        }
    }
}

#[test]
fn frozen_credential_cwd_fixture() {
    let Some(base) = std::env::var_os("DEMONCODER_CREDENTIAL_CWD_FIXTURE") else {
        return;
    };
    let base = std::path::PathBuf::from(base);
    std::env::set_current_dir(&base).unwrap();
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
        let fixture = Fixture::at(tempfile::tempdir_in(&base).unwrap());
        std::fs::create_dir(fixture.root.path().join("effects")).unwrap();
        let secret = fixture.root.path().join("effects/ordinary-store");
        std::fs::write(&secret, "synthetic-frozen-cwd-canary").unwrap();
        let code = "import pathlib,json,sys\njson.load(sys.stdin)\ntry: value=pathlib.Path('effects/ordinary-store').read_text()\nexcept OSError: pass\nelse: raise AssertionError('credential was reinterpreted after CWD change')\nprint('{}')\n";
        let (_source, captured) = package(HookDialect::Native, code);
        let mut config = python_config(); config.write_paths = vec!["effects".into()];
        let registration = CommandRunner::registration(captured, declaration("frozen-cwd", HookDialect::Native, HandlerClass::Transformer), config, None).unwrap();
        let host = std::env::var("DEMONCODER_CREDENTIAL_CWD_HOST").unwrap() == "true";
        let executor = fixture.executor_policy(vec![registration], AccessPolicy { unrestricted: host, credential_paths: vec![secret.strip_prefix(&base).unwrap().into()], supervisor: Some(env!("CARGO_BIN_EXE_demoncoder").into()), ..AccessPolicy::default() });
        let changed = base.join("changed-cwd"); std::fs::create_dir(&changed).unwrap();
        std::env::set_current_dir(changed).unwrap();
        let results = fixture.run(executor, vec![call("result")]).await;
        assert!(results[0].success, "{results:?}; {:?}", hook(&fixture.record()).outcome);
    });
}

#[tokio::test]
async fn executor_credentials_remain_frozen_when_process_cwd_changes() {
    let _fixture_lock = FIXTURES.lock().await;
    for host in [false, true] {
        let base = tempfile::tempdir().unwrap();
        let child = tokio::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "frozen_credential_cwd_fixture", "--nocapture"])
            .env("DEMONCODER_CREDENTIAL_CWD_FIXTURE", base.path())
            .env("DEMONCODER_CREDENTIAL_CWD_HOST", host.to_string())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let result = tokio::time::timeout(Duration::from_secs(10), child.wait_with_output())
            .await
            .unwrap()
            .unwrap();
        assert!(
            result.status.success(),
            "host={host}: {} {}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
    }
}

#[tokio::test]
async fn dangling_credential_alias_cannot_authorize_required_absence_evidence() {
    let _fixture_lock = FIXTURES.lock().await;
    for host in [false, true] {
        let fixture = Fixture::new();
        let aliases = tempfile::tempdir().unwrap();
        let alias = aliases.path().join("missing-credential");
        std::os::unix::fs::symlink(fixture.root.path().join("ordinary-store"), &alias).unwrap();
        let mut registration = register(ALLOW, HookDialect::Native, HandlerClass::DecisionGate);
        registration.declaration.reads =
            GateReadSet::new(vec![], vec![], vec!["ordinary-store".into()]).unwrap();
        let Ok(mut executor) = ToolExecutor::with_policy(
            fixture.root.path(),
            &AccessPolicy {
                unrestricted: host,
                credential_paths: vec![alias],
                supervisor: Some(env!("CARGO_BIN_EXE_demoncoder").into()),
                ..AccessPolicy::default()
            },
        ) else {
            continue;
        };
        executor
            .register_pre_tool_plan(Arc::new(PreToolPlan::new(vec![registration]).unwrap()))
            .unwrap();
        let results = fixture.run(executor, vec![call("never")]).await;
        assert!(
            !results[0].success,
            "dangling credential alias was treated as unrelated to its missing target"
        );
        assert!(all_hooks(&fixture.record()).is_empty());
        assert!(!fixture.root.path().join("never").exists());
    }
}

#[tokio::test]
async fn no_plan_executor_still_opens_an_owned_directory_inside_the_session_store() {
    let _fixture_lock = FIXTURES.lock().await;
    let fixture = Fixture::new();
    let owned = fixture.runtime.directory().unwrap();
    for host in [false, true] {
        assert!(
            ToolExecutor::with_policy(
                &owned,
                &AccessPolicy {
                    unrestricted: host,
                    ..AccessPolicy::default()
                }
            )
            .is_ok()
        );
    }
}

struct RemoveCredentialBeforeRemap {
    remap: RemapCredential,
    leaf: std::path::PathBuf,
}
#[async_trait::async_trait]
impl HookRunner for RemoveCredentialBeforeRemap {
    fn mutates_workspace(&self) -> bool {
        self.remap.mutates_workspace()
    }
    async fn run(&self, invocation: &HookInvocation) -> anyhow::Result<RawOutcome> {
        std::fs::remove_file(&self.leaf)?;
        self.remap.run(invocation).await
    }
}

#[tokio::test]
async fn launch_credential_resolution_protects_retained_targets_after_alias_changes() {
    let _fixture_lock = FIXTURES.lock().await;
    let mut failures = Vec::new();
    for host in [false, true] {
        for directory_alias in [false, true] {
            for grant in [None, Some("effects")] {
                let fixture = Fixture::new();
                let suffix = if directory_alias {
                    "/ordinary-store"
                } else {
                    ""
                };
                let second = format!("effects/second{suffix}");
                for name in ["first", "second"] {
                    let leaf = fixture.root.path().join(format!("effects/{name}{suffix}"));
                    std::fs::create_dir_all(leaf.parent().unwrap()).unwrap();
                    std::fs::write(leaf, "synthetic-local-credential").unwrap();
                }
                let aliases = tempfile::tempdir().unwrap();
                let alias = aliases.path().join("credential");
                std::os::unix::fs::symlink(fixture.root.path().join("effects/first"), &alias)
                    .unwrap();
                let code = format!(
                    "import pathlib,json,sys\njson.load(sys.stdin)\ntry: pathlib.Path({second:?}).read_text()\nexcept OSError: pass\nelse: raise AssertionError('retained credential target was accessible')\nprint({:?})\n",
                    if grant.is_none() {
                        r#"{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow"}}"#
                    } else {
                        "{}"
                    }
                );
                let (_source, captured) = package(HookDialect::Native, &code);
                let mut config = python_config();
                config.write_paths = grant.map(|path| vec![path.into()]).unwrap_or_default();
                let class = if grant.is_none() {
                    HandlerClass::DecisionGate
                } else {
                    HandlerClass::Transformer
                };
                let mut registration = CommandRunner::registration(
                    captured,
                    declaration("launch-alias", HookDialect::Native, class),
                    config,
                    None,
                )
                .unwrap();
                registration.runner = Arc::new(RemoveCredentialBeforeRemap {
                    remap: RemapCredential {
                        runner: registration.runner.clone(),
                        alias: alias.clone(),
                        target: fixture.root.path().join("effects/second"),
                    },
                    leaf: fixture.root.path().join(&second),
                });
                let credential = if directory_alias {
                    alias.join("ordinary-store")
                } else {
                    alias
                };
                let executor = fixture.executor_policy(
                    vec![registration],
                    AccessPolicy {
                        unrestricted: host,
                        credential_paths: vec![credential],
                        supervisor: Some(env!("CARGO_BIN_EXE_demoncoder").into()),
                        ..AccessPolicy::default()
                    },
                );
                let results = fixture.run(executor, vec![call("never")]).await;
                let record = fixture.record();
                let outcome = hook(&record).outcome.as_ref().unwrap();
                let protected = if directory_alias {
                    matches!(outcome, RawOutcome::Command { exit_code: Some(0), stderr, .. } if stderr.is_empty())
                } else {
                    matches!(outcome, RawOutcome::CommandFailure { stdout, stderr, .. } if stdout.is_empty() && stderr.is_empty())
                };
                if !protected || results[0].success || fixture.root.path().join("never").exists() {
                    failures.push(format!(
                        "host={host} directory_alias={directory_alias} grant={grant:?}: {outcome:?}"
                    ));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[tokio::test]
async fn stable_directory_credential_alias_with_absent_leaf_keeps_public_work_usable() {
    let _fixture_lock = FIXTURES.lock().await;
    for host in [false, true] {
        for grant in [None, Some("effects")] {
            let fixture = Fixture::new();
            std::fs::create_dir(fixture.root.path().join("effects")).unwrap();
            let aliases = tempfile::tempdir().unwrap();
            let alias = aliases.path().join("credential-directory");
            std::os::unix::fs::symlink(fixture.root.path().join("effects"), &alias).unwrap();
            let code = if grant.is_none() {
                "import json,sys\njson.load(sys.stdin)\nprint(json.dumps({'hookSpecificOutput':{'hookEventName':'PreToolUse','permissionDecision':'allow'}}))\n"
            } else {
                "import json,sys,pathlib\njson.load(sys.stdin)\npathlib.Path('effects/public-effect').write_text('allowed')\nprint('{}')\n"
            };
            let (_source, captured) = package(HookDialect::Native, code);
            let mut config = python_config();
            config.write_paths = grant.map(|path| vec![path.into()]).unwrap_or_default();
            let class = if grant.is_none() {
                HandlerClass::DecisionGate
            } else {
                HandlerClass::Transformer
            };
            let registration = CommandRunner::registration(
                captured,
                declaration("stable-absent", HookDialect::Native, class),
                config,
                None,
            )
            .unwrap();
            let executor = fixture.executor_policy(
                vec![registration],
                AccessPolicy {
                    unrestricted: host,
                    credential_paths: vec![alias.join("ordinary-store")],
                    supervisor: Some(env!("CARGO_BIN_EXE_demoncoder").into()),
                    ..AccessPolicy::default()
                },
            );
            let results = fixture.run(executor, vec![call("result")]).await;
            assert!(
                results[0].success,
                "host={host} grant={grant:?}: {results:?}"
            );
            assert!(!fixture.root.path().join("effects/ordinary-store").exists());
            if grant.is_some() {
                assert_eq!(
                    std::fs::read_to_string(fixture.root.path().join("effects/public-effect"))
                        .unwrap(),
                    "allowed"
                );
            }
        }
    }
}

#[test]
fn staging_descriptor_limit_fixture() {
    if std::env::var_os("DEMONCODER_STAGING_DESCRIPTOR_FIXTURE").is_none() {
        return;
    }
    use rustix::process::{Resource, Rlimit, getrlimit, setrlimit};
    let limit = getrlimit(Resource::Nofile);
    setrlimit(
        Resource::Nofile,
        Rlimit {
            current: Some(1024),
            maximum: limit.maximum,
        },
    )
    .unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            for case in ["files", "directories", "package"] {
                for count in [100, 1200] {
                    let fixture = Fixture::new();
                    let registration = if case == "package" {
                        let (source, _) = package(HookDialect::Native, ALLOW);
                        for index in 0..count {
                            std::fs::create_dir(source.path().join(format!("dir-{index:04}")))
                                .unwrap();
                        }
                        let captured = Arc::new(
                            plugins::inspect(source.path(), &plugins::ImportOptions::default())
                                .unwrap(),
                        );
                        CommandRunner::registration(
                            captured,
                            declaration(
                                "many-package-directories",
                                HookDialect::Native,
                                HandlerClass::DecisionGate,
                            ),
                            python_config(),
                            None,
                        )
                        .unwrap()
                    } else {
                        for index in 0..count {
                            let path = fixture.root.path().join(format!("entry-{index:04}"));
                            if case == "files" {
                                std::fs::write(path, b"retained public bytes").unwrap();
                            } else {
                                std::fs::create_dir(path).unwrap();
                            }
                        }
                        plugins::gate_snapshot::GateWorkspace::open(fixture.root.path())
                            .unwrap()
                            .capture(
                                &GateReadSet::default(),
                                &std::sync::atomic::AtomicBool::new(false),
                            )
                            .unwrap();
                        register(ALLOW, HookDialect::Native, HandlerClass::DecisionGate)
                    };
                    let result = fixture
                        .run(
                            fixture.executor(vec![registration], false),
                            vec![call("result")],
                        )
                        .await;
                    assert!(
                        result[0].success,
                        "{case} count={count}: {:?}",
                        hook(&fixture.record()).outcome
                    );
                }
            }
        });
}

#[tokio::test]
async fn admitted_snapshots_and_packages_run_under_an_isolated_descriptor_limit() {
    let _fixture_lock = FIXTURES.lock().await;
    let temporary = tempfile::tempdir().unwrap();
    let child = tokio::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "staging_descriptor_limit_fixture", "--nocapture"])
        .env("DEMONCODER_STAGING_DESCRIPTOR_FIXTURE", "1")
        .env("TMPDIR", temporary.path())
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true)
        .output();
    let result = tokio::time::timeout(Duration::from_secs(60), child)
        .await
        .expect("isolated descriptor fixture timed out")
        .unwrap();
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    // The existing DeveloperAccess overlay probe has a separately recorded
    // cleanup issue. Assert our own staging cleanup, then rmdir only this
    // isolated fixture's known empty overlay-work directories.
    for root in std::fs::read_dir(temporary.path()).unwrap() {
        let root = root.unwrap();
        let name = root.file_name();
        let name = name.to_string_lossy();
        assert!(
            !name.starts_with("demoncoder-hook-staging-"),
            "hook staging survived cleanup"
        );
        assert!(
            name.starts_with("demoncoder-confined-"),
            "unexpected fixture residue"
        );
        for entry in std::fs::read_dir(root.path()).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let work = name
                .strip_prefix("work-")
                .and_then(|n| n.parse::<usize>().ok())
                .is_some();
            let cache = name
                .strip_prefix("cache-")
                .and_then(|n| n.parse::<usize>().ok())
                .is_some();
            assert!(
                work || cache || name == "cache" || name == "share",
                "unknown fixture residue"
            );
            if work {
                let nested = entry.path().join("work");
                match nested.symlink_metadata() {
                    Ok(metadata) => {
                        assert!(metadata.is_dir());
                        std::fs::set_permissions(
                            &nested,
                            std::os::unix::fs::PermissionsExt::from_mode(0o700),
                        )
                        .unwrap();
                        std::fs::remove_dir(nested).unwrap();
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => panic!("inspect isolated overlay work directory: {error}"),
                }
            }
            std::fs::remove_dir(entry.path()).unwrap();
        }
        std::fs::remove_dir(root.path()).unwrap();
    }
    temporary.close().unwrap();
}
