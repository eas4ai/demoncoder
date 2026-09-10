use demoncoder::{
    config::Connection,
    events::{Event, EventSink},
    native::{Model, NativeSession},
    plugins::{
        self,
        dispatch::*,
        gate_snapshot::GateReadSet,
        hook_types::{HandlerKind, HookDialect},
        runners::{ModelConfig, ModelRunner},
    },
    session::Session,
    tools::{AccessPolicy, ToolCall, ToolExecutor, ToolResult},
    workflow::{
        allocation::Limits,
        runtime::{Record, SharedRuntime},
    },
};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    io::{Read, Write},
    net::TcpListener,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};
use tokio::sync::mpsc;

static FIXTURES: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

struct Server {
    endpoint: String,
    requests: Arc<Mutex<Vec<Value>>>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Server {
    fn new(
        adapter: &'static str,
        handler: impl Fn(usize, &Value) -> Value + Send + 'static,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/v1/responses", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = requests.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let worker = thread::spawn(move || {
            'serve: while !stopped.load(Ordering::SeqCst) {
                let (mut stream, _) = match listener.accept() {
                    Ok(v) => v,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    Err(e) => panic!("{e}"),
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut bytes = Vec::new();
                let mut buffer = [0u8; 4096];
                let header = loop {
                    let n = match stream.read(&mut buffer) {
                        Ok(0) | Err(_) => continue 'serve,
                        Ok(n) => n,
                    };
                    bytes.extend_from_slice(&buffer[..n]);
                    if let Some(n) = bytes.windows(4).position(|p| p == b"\r\n\r\n") {
                        break n + 4;
                    }
                };
                let headers = String::from_utf8(bytes[..header].to_vec()).unwrap();
                let length = headers
                    .lines()
                    .find_map(|line| {
                        let (key, value) = line.split_once(':')?;
                        key.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().unwrap())
                    })
                    .unwrap();
                while bytes.len() < header + length {
                    let n = match stream.read(&mut buffer) {
                        Ok(0) | Err(_) => continue 'serve,
                        Ok(n) => n,
                    };
                    bytes.extend_from_slice(&buffer[..n]);
                }
                let request: Value =
                    serde_json::from_slice(&bytes[header..header + length]).unwrap();
                let index = {
                    let mut requests = recorded.lock().unwrap();
                    let n = requests.len();
                    requests.push(request.clone());
                    n
                };
                let value = handler(index, &request);
                let body = response(adapter, value)
                    .into_iter()
                    .map(|v| format!("data: {v}\n\n"))
                    .collect::<String>();
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        Self {
            endpoint,
            requests,
            stop,
            worker: Some(worker),
        }
    }
    fn connection(&self, adapter: &str) -> Connection {
        serde_json::from_value(json!({"adapter":adapter,"model":"explicit-hook-model","endpoint":self.endpoint,"api_key":"synthetic-hook-key","max_output_tokens":512})).unwrap()
    }
    fn count(&self) -> usize {
        self.requests.lock().unwrap().len()
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.worker.take().unwrap().join().unwrap();
    }
}
fn response(adapter: &str, value: Value) -> Vec<Value> {
    if value.get("transport_error").is_some() {
        return vec![
            json!({"type":"error","error":{"type":"not_found_error","message":"selected model unavailable"}}),
        ];
    }
    let tool = value.get("tool");
    let text = value
        .get("raw")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| value.to_string());
    if adapter == "openai-api" {
        let output = tool.map(|tool| vec![json!({"type":"function_call","call_id":"inspect","name":tool["name"],"arguments":tool["arguments"].to_string()})]).unwrap_or_default();
        let mut events = Vec::new();
        if tool.is_none() {
            events.push(json!({"type":"response.output_text.delta","delta":text}));
        }
        events.push(json!({"type":"response.completed","response":{"output":output,"usage":{"input_tokens":11,"output_tokens":7,"input_tokens_details":{"cached_tokens":2}}}}));
        events
    } else {
        let block = tool.map(|tool| json!({"type":"tool_use","id":"inspect","name":tool["name"],"input":tool["arguments"]})).unwrap_or_else(|| json!({"type":"text","text":""}));
        let mut events = vec![
            json!({"type":"message_start","message":{"usage":{"input_tokens":11,"cache_read_input_tokens":2}}}),
            json!({"type":"content_block_start","index":0,"content_block":block}),
        ];
        if tool.is_none() {
            events.push(json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":text}}));
        }
        events.extend([json!({"type":"content_block_stop","index":0}),json!({"type":"message_delta","delta":{"stop_reason":if tool.is_some() {"tool_use"} else {"end_turn"}},"usage":{"output_tokens":7}}),json!({"type":"message_stop"})]);
        events
    }
}
struct Source {
    calls: VecDeque<Vec<ToolCall>>,
    results: Arc<Mutex<Vec<ToolResult>>>,
}
#[async_trait::async_trait]
impl Model for Source {
    fn prompt(&mut self, _: String) {}
    fn results(&mut self, results: Vec<ToolResult>) {
        self.results.lock().unwrap().extend(results);
    }
    async fn response(&mut self, events: &EventSink) -> anyhow::Result<Vec<ToolCall>> {
        events
            .emit(Event::Usage {
                input: Some(0),
                output: Some(0),
                cached: Some(0),
                cost_usd: Some(0.0),
            })
            .await?;
        Ok(self.calls.pop_front().unwrap_or_default())
    }
}
struct Fixture {
    root: tempfile::TempDir,
    runtime: SharedRuntime,
    events: EventSink,
    receiver: mpsc::Receiver<demoncoder::events::Envelope>,
}
impl Fixture {
    fn new(limit: Option<u64>) -> Self {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join("public"),
            "retained source\nliteral evidence",
        )
        .unwrap();
        let connection: Connection =
            serde_json::from_value(json!({"adapter":"openai-api"})).unwrap();
        let (runtime, _) = SharedRuntime::open(root.path(), &connection, None).unwrap();
        if let Some(limit) = limit {
            runtime
                .allocate(
                    Limits {
                        seconds: 30,
                        model_calls: limit,
                        tool_calls: 64,
                    },
                    None,
                )
                .unwrap();
        }
        let (sender, receiver) = mpsc::channel(256);
        let events = EventSink::new("model fixture".into(), sender, None)
            .unwrap()
            .with_runtime(runtime.clone());
        Self {
            root,
            runtime,
            events,
            receiver,
        }
    }
    fn executor(&self, registrations: Vec<Registration>, mut access: AccessPolicy) -> ToolExecutor {
        access.supervisor = Some(env!("CARGO_BIN_EXE_demoncoder").into());
        let mut tools = ToolExecutor::with_policy(self.root.path(), &access).unwrap();
        tools
            .register_pre_tool_plan(Arc::new(PreToolPlan::new(registrations).unwrap()))
            .unwrap();
        tools
    }
    async fn run(&self, tools: ToolExecutor, request: ToolCall) -> Vec<ToolResult> {
        let results = Arc::new(Mutex::new(Vec::new()));
        let model = Source {
            calls: VecDeque::from([vec![request]]),
            results: results.clone(),
        };
        let mut session = NativeSession::with_tools(Box::new(model), tools);
        let (_sender, mut commands) = mpsc::channel(1);
        let result = tokio::time::timeout(
            Duration::from_secs(8),
            session.turn("source".into(), &mut commands, &self.events),
        )
        .await
        .unwrap();
        if let Err(e) = result {
            eprintln!("expected or unexpected fixture hold: {e:#}");
        }
        session.close().await.unwrap();
        results.lock().unwrap().clone()
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
fn call() -> ToolCall {
    ToolCall {
        id: "source".into(),
        name: "write".into(),
        arguments: json!({"path":"result","content":"literal $(touch /tmp/not-executed) `text` $ARGUMENTS"}),
    }
}
fn declaration(kind: HandlerKind, dialect: HookDialect, index: u32) -> Declaration {
    Declaration {
        identity: DeclarationIdentity {
            package: "fixture".into(),
            code: "captured".into(),
            policy: "policy".into(),
            configuration: "host".into(),
            generation: "generation".into(),
            scope: Scope::Project,
            role: "worker".into(),
            declaration: format!("model-{index}"),
            index,
            dialect,
            runner: kind,
        },
        class: if dialect == HookDialect::Claude {
            HandlerClass::Combined
        } else {
            HandlerClass::DecisionGate
        },
        priority: 0,
        matcher: Matcher::default(),
        reads: GateReadSet::default(),
        concurrent_group: (dialect == HookDialect::Claude).then(|| "source-group".into()),
        read_only_endpoint: None,
        external_precondition: None,
    }
}
fn register(d: Declaration, config: ModelConfig) -> Registration {
    try_register(d, config).unwrap()
}
fn try_register(mut d: Declaration, config: ModelConfig) -> anyhow::Result<Registration> {
    let package = tempfile::tempdir().unwrap();
    std::fs::create_dir(package.path().join(".claude-plugin")).unwrap();
    std::fs::write(
        package.path().join(".claude-plugin/plugin.json"),
        r#"{"name":"model-fixture","version":"1.0.0"}"#,
    )
    .unwrap();
    let package =
        Arc::new(plugins::inspect(package.path(), &plugins::ImportOptions::default()).unwrap());
    d.identity.package = package.name().into();
    ModelRunner::registration(package, d, config)
}
fn config(server: &Server, adapter: &str) -> ModelConfig {
    ModelConfig::new(
        server.connection(adapter),
        "Inspect the proposed operation; approve only if the evidence permits it.".into(),
    )
}
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
fn prompt(request: &Value) -> &str {
    request["input"][0]["content"]
        .as_str()
        .or_else(|| request["messages"][0]["content"].as_str())
        .unwrap()
}

#[tokio::test]
async fn prompt_profiles_and_native_transports_validate_verdicts_and_charge_once() {
    let _lock = FIXTURES.lock().await;
    for adapter in ["openai-api", "anthropic-api"] {
        for dialect in [HookDialect::Native, HookDialect::Claude] {
            for (verdict, allowed) in [
                (json!({"ok":true}), true),
                (json!({"ok":false,"reason":"denied by evidence"}), false),
                (json!({"ok":false}), false),
                (json!({"raw":"{broken"}), false),
                (json!({"ok":true,"continueOnBlock":true}), false),
            ] {
                let server = Server::new(adapter, move |_, _| verdict.clone());
                let mut fixture = Fixture::new(Some(8));
                let registration = register(
                    declaration(HandlerKind::Prompt, dialect, 0),
                    config(&server, adapter),
                );
                let results = fixture
                    .run(
                        fixture.executor(vec![registration], AccessPolicy::default()),
                        call(),
                    )
                    .await;
                assert_eq!(
                    results[0].success, allowed,
                    "{adapter} {dialect:?}: {results:?}"
                );
                assert_eq!(fixture.root.path().join("result").exists(), allowed);
                assert_eq!(server.count(), 1);
                let request = &server.requests.lock().unwrap()[0];
                assert_eq!(request["model"], "explicit-hook-model");
                assert_eq!(request["tools"], json!([]));
                assert!(prompt(request).contains("retained source"));
                let record = fixture.record();
                let allocation = record.allocation.unwrap();
                assert_eq!(allocation.usage.reported_input, 11);
                assert_eq!(allocation.usage.reported_output, 7);
                assert!((2..=3).contains(&allocation.model_calls));
                let mut review_usage = 0;
                while let Ok(event) = fixture.receiver.try_recv() {
                    if matches!(event.event, Event::ReviewUsage { .. }) {
                        review_usage += 1;
                    }
                }
                assert_eq!(review_usage, 1);
            }
        }
    }
}

#[tokio::test]
async fn prompt_attempted_tools_and_agent_live_tools_cannot_mutate() {
    let _lock = FIXTURES.lock().await;
    for adapter in ["openai-api", "anthropic-api"] {
        for kind in [HandlerKind::Prompt, HandlerKind::Agent] {
            for tool in ["write", "bash", "read", "snapshot_read"] {
                let name = tool.to_owned();
                let server = Server::new(
                    adapter,
                    move |_, _| json!({"tool":{"name":name,"arguments":if name=="bash" {json!({"command":"touch forbidden"})} else if name=="write" {json!({"path":"forbidden","content":"no"})} else {json!({"path":"/etc/passwd"})}}}),
                );
                let fixture = Fixture::new(Some(8));
                let registration = register(
                    declaration(kind, HookDialect::Native, 0),
                    config(&server, adapter),
                );
                let results = fixture
                    .run(
                        fixture.executor(
                            vec![registration],
                            AccessPolicy {
                                unrestricted: true,
                                ..AccessPolicy::default()
                            },
                        ),
                        call(),
                    )
                    .await;
                assert!(!results[0].success, "{kind:?} {tool}: {results:?}");
                assert_eq!(server.count(), 1);
                assert!(!fixture.root.path().join("forbidden").exists());
                assert!(!fixture.root.path().join("result").exists());
            }
        }
    }
}

#[tokio::test]
async fn agent_inspections_retain_original_snapshot_and_literal_input() {
    let _lock = FIXTURES.lock().await;
    for adapter in ["openai-api", "anthropic-api"] {
        for changed in [false, true] {
            let fixture = Fixture::new(Some(8));
            let root = fixture.root.path().to_path_buf();
            std::os::unix::fs::symlink(root.join("public"), root.join("alias")).unwrap();
            let server = Server::new(adapter, move |index, request| {
                if index == 0 {
                    let text = prompt(request);
                    assert!(text.contains("literal $(touch /tmp/not-executed) `text` $ARGUMENTS"));
                    if changed {
                        std::fs::write(root.join("public"), "changed live source").unwrap();
                    }
                    json!({"tool":{"name":"snapshot_read","arguments":{"path":root.join("alias")}}})
                } else {
                    assert!(request.to_string().contains("retained source"));
                    assert!(!request.to_string().contains("changed live source"));
                    json!({"ok":true})
                }
            });
            let registration = register(
                declaration(HandlerKind::Agent, HookDialect::Native, 0),
                config(&server, adapter),
            );
            let results = fixture
                .run(
                    fixture.executor(vec![registration], AccessPolicy::default()),
                    call(),
                )
                .await;
            assert_eq!(results[0].success, !changed, "{adapter}: {results:?}");
            assert_eq!(server.count(), 2);
            let record = fixture.record();
            let inspect = record
                .operations
                .iter()
                .find(|o| o.call.as_ref().is_some_and(|c| c.name == "snapshot_read"))
                .unwrap();
            assert!(inspect.phase.starts_with("hook:"));
            let result = inspect.result.as_ref().unwrap();
            assert!(result.success);
            assert!(result.output.contains("retained source"));
            assert!(result.output.contains("revision"));
            assert_eq!(record.allocation.as_ref().unwrap().usage.reported_input, 22);
            let definitions = server.requests.lock().unwrap()[0]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v["name"].as_str().unwrap().to_owned())
                .collect::<Vec<_>>();
            assert_eq!(
                definitions,
                ["snapshot_read", "snapshot_list", "snapshot_search"]
            );
        }
    }
}

#[tokio::test]
async fn missing_and_exhausted_shared_allowances_send_no_hook_request() {
    let _lock = FIXTURES.lock().await;
    for allowance in [None, Some(1)] {
        let server = Server::new("openai-api", |_, _| json!({"ok":true}));
        let fixture = Fixture::new(allowance);
        let registration = register(
            declaration(HandlerKind::Prompt, HookDialect::Native, 0),
            config(&server, "openai-api"),
        );
        let result = fixture
            .run(
                fixture.executor(vec![registration], AccessPolicy::default()),
                call(),
            )
            .await;
        assert!(!result[0].success);
        assert_eq!(server.count(), 0);
        assert!(!fixture.root.path().join("result").exists());
    }
}

#[tokio::test]
async fn agent_invocation_limit_and_timeout_hold_and_settle_owned_requests() {
    let _lock = FIXTURES.lock().await;
    for timeout in [false, true] {
        let server = Server::new("openai-api", move |_, _| {
            if timeout {
                thread::sleep(Duration::from_millis(250));
            }
            json!({"tool":{"name":"snapshot_read","arguments":{"path":"public"}}})
        });
        let fixture = Fixture::new(Some(16));
        let mut configuration = config(&server, "openai-api");
        configuration.max_invocations = 2;
        configuration.timeout_ms = if timeout { 50 } else { 2000 };
        let registration = register(
            declaration(HandlerKind::Agent, HookDialect::Native, 0),
            configuration,
        );
        let result = fixture
            .run(
                fixture.executor(vec![registration], AccessPolicy::default()),
                call(),
            )
            .await;
        assert!(!result[0].success);
        assert_eq!(server.count(), if timeout { 1 } else { 2 });
        let record = fixture.record();
        assert!(
            record
                .operations
                .iter()
                .filter(|o| o.phase.starts_with("hook:") && o.host_invocation.is_some())
                .all(|o| o.complete)
        );
        if timeout {
            assert!(record.allocation.as_ref().unwrap().usage.unknown_input);
        }
        assert!(matches!(
            hook(&record).outcome,
            Some(RawOutcome::Failure { .. })
        ));
    }
}

#[tokio::test]
async fn external_transports_keep_snapshot_authority_and_charge_backend_invocations() {
    use std::os::unix::fs::PermissionsExt;
    let _lock = FIXTURES.lock().await;
    for adapter in ["claude", "codex"] {
        for kind in [HandlerKind::Prompt, HandlerKind::Agent] {
            for behavior in ["allow", "forbidden", "timeout", "missing", "exhausted"] {
                let expected_calls = usize::from(!matches!(behavior, "missing" | "exhausted"));
                let fixture = Fixture::new(match behavior {
                    "missing" => None,
                    "exhausted" => Some(1),
                    _ => Some(8),
                });
                let backend = tempfile::tempdir().unwrap();
                let binary = backend.path().join("backend");
                let records = backend.path().join("requests.jsonl");
                std::fs::write(&binary, include_str!("plugin_model_backend.py")).unwrap();
                std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
                std::fs::write(binary.with_extension("json"),json!({"agent":kind==HandlerKind::Agent,"workspace":fixture.root.path(),"records":records,"behavior":behavior}).to_string()).unwrap();
                let connection: Connection = serde_json::from_value(
                    json!({"adapter":adapter,"model":"explicit-hook-model","binary":binary}),
                )
                .unwrap();
                let mut configuration =
                    ModelConfig::new(connection, "Inspect the candidate.".into());
                configuration.timeout_ms = if behavior == "timeout" { 300 } else { 3000 };
                let registration =
                    register(declaration(kind, HookDialect::Native, 0), configuration);
                let results = fixture
                    .run(
                        fixture.executor(
                            vec![registration],
                            AccessPolicy {
                                unrestricted: true,
                                ..AccessPolicy::default()
                            },
                        ),
                        call(),
                    )
                    .await;
                assert_eq!(
                    results[0].success,
                    behavior == "allow",
                    "{adapter} {kind:?} {behavior}: {results:?}; {:?}",
                    hook(&fixture.record()).outcome
                );
                assert!(!fixture.root.path().join("forbidden").exists());
                let messages = std::fs::read_to_string(records)
                    .unwrap_or_default()
                    .lines()
                    .map(|l| serde_json::from_str::<Value>(l).unwrap())
                    .collect::<Vec<_>>();
                let calls = messages
                    .iter()
                    .filter(|v| {
                        v["message"]["type"] == "user" || v["message"]["method"] == "turn/start"
                    })
                    .count();
                assert_eq!(calls, expected_calls, "{adapter} {behavior}: {messages:?}");
                let record = fixture.record();
                assert_eq!(record.backend_invocations, expected_calls as u64);
                let admissions = record
                    .operations
                    .iter()
                    .filter(|o| o.phase.starts_with("hook:") && o.host_invocation.is_some())
                    .collect::<Vec<_>>();
                assert_eq!(admissions.len(), expected_calls);
                if expected_calls > 0 {
                    assert!(admissions[0].complete);
                    assert!(matches!(
                        admissions[0].host_invocation,
                        Some(demoncoder::workflow::runtime::HostInvocation::Backend)
                    ));
                }
                if behavior == "allow" {
                    assert_eq!(record.allocation.as_ref().unwrap().usage.reported_input, 13);
                }
                if behavior == "timeout" {
                    assert!(record.allocation.as_ref().unwrap().usage.unknown_input);
                }
                for launch in messages.iter().filter(|v| v.get("argv").is_some()) {
                    if adapter == "claude" {
                        assert!(
                            launch["argv"]
                                .as_array()
                                .unwrap()
                                .iter()
                                .any(|v| v == "--safe-mode")
                        );
                    }
                    if adapter == "codex" && launch["argv"][0] == "app-server" {
                        assert_eq!(launch["mode"], "v1");
                    }
                }
                assert!(
                    messages
                        .iter()
                        .filter_map(|v| v["cwd"].as_str())
                        .all(|cwd| !std::path::Path::new(cwd).exists()),
                    "transport cwd was not removed after close"
                );
            }
        }
    }
}

#[tokio::test]
async fn credential_canaries_are_excluded_in_host_mode_and_agent_live_reads_hold() {
    let _lock = FIXTURES.lock().await;
    for host in [false, true] {
        for agent in [false, true] {
            let fixture = Fixture::new(Some(8));
            let credential = fixture.root.path().join("ordinary-store");
            std::fs::write(&credential, "PRIVATE-MODEL-CANARY").unwrap();
            let server = Server::new("openai-api", move |_, request| {
                assert!(!request.to_string().contains("PRIVATE-MODEL-CANARY"));
                if agent {
                    json!({"tool":{"name":"snapshot_read","arguments":{"path":"ordinary-store"}}})
                } else {
                    json!({"ok":true})
                }
            });
            let registration = register(
                declaration(
                    if agent {
                        HandlerKind::Agent
                    } else {
                        HandlerKind::Prompt
                    },
                    HookDialect::Native,
                    0,
                ),
                config(&server, "openai-api"),
            );
            let result = fixture
                .run(
                    fixture.executor(
                        vec![registration],
                        AccessPolicy {
                            unrestricted: host,
                            credential_paths: vec![credential],
                            ..AccessPolicy::default()
                        },
                    ),
                    call(),
                )
                .await;
            assert_eq!(result[0].success, !agent, "{result:?}");
            assert_eq!(server.count(), 1);
            assert!(
                !serde_json::to_string(&fixture.record())
                    .unwrap()
                    .contains("PRIVATE-MODEL-CANARY")
            );
        }
    }
}

struct RemapCredential {
    alias: std::path::PathBuf,
    target: std::path::PathBuf,
}
#[async_trait::async_trait]
impl HookRunner for RemapCredential {
    async fn run(&self, _: &HookInvocation) -> anyhow::Result<RawOutcome> {
        std::fs::remove_file(&self.alias)?;
        std::os::unix::fs::symlink(&self.target, &self.alias)?;
        Ok(RawOutcome::Callback {
            value: json!({"decision":"approve"}),
        })
    }
}
#[tokio::test]
async fn alias_changed_after_capture_blocks_prompt_before_any_model_delivery() {
    let _lock = FIXTURES.lock().await;
    let fixture = Fixture::new(Some(8));
    let outside = tempfile::tempdir().unwrap();
    let old = fixture.root.path().join("old-credential");
    let new = fixture.root.path().join("new-credential");
    std::fs::write(&old, "OLD-CREDENTIAL-CANARY").unwrap();
    std::fs::write(&new, "RECLASSIFIED-CREDENTIAL-CANARY").unwrap();
    let alias = outside.path().join("credential");
    std::os::unix::fs::symlink(&old, &alias).unwrap();
    let mut first = declaration(HandlerKind::Command, HookDialect::Native, 0);
    first.priority = -1;
    let remap = Registration {
        declaration: first,
        runner: Arc::new(RemapCredential {
            alias: alias.clone(),
            target: new,
        }),
        revalidation: None,
    };
    let server = Server::new("openai-api", |_, _| {
        panic!("reclassified private bytes reached the model")
    });
    let registration = register(
        declaration(HandlerKind::Prompt, HookDialect::Native, 1),
        config(&server, "openai-api"),
    );
    let result = fixture
        .run(
            fixture.executor(
                vec![remap, registration],
                AccessPolicy {
                    credential_paths: vec![alias],
                    ..AccessPolicy::default()
                },
            ),
            call(),
        )
        .await;
    assert!(!result[0].success);
    assert_eq!(server.count(), 0);
}

#[tokio::test]
async fn concurrent_source_hooks_atomically_share_allowance_and_keep_distinct_usage_phases() {
    let _lock = FIXTURES.lock().await;
    for limit in [2, 4] {
        let fixture = Fixture::new(Some(limit));
        let server = Server::new("openai-api", |_, _| {
            thread::sleep(Duration::from_millis(20));
            json!({"ok":true})
        });
        let registrations = (0..2)
            .map(|index| {
                register(
                    declaration(HandlerKind::Prompt, HookDialect::Claude, index),
                    config(&server, "openai-api"),
                )
            })
            .collect();
        let result = fixture
            .run(
                fixture.executor(registrations, AccessPolicy::default()),
                call(),
            )
            .await;
        assert_eq!(result[0].success, limit == 4, "{result:?}");
        assert_eq!(server.count(), if limit == 2 { 1 } else { 2 });
        let record = fixture.record();
        let admissions = record
            .operations
            .iter()
            .filter(|o| o.phase.starts_with("hook:") && o.host_invocation.is_some())
            .collect::<Vec<_>>();
        assert_eq!(admissions.len(), server.count());
        let unique = admissions
            .iter()
            .map(|o| &o.phase)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(unique.len(), admissions.len());
        assert!(admissions.iter().all(|o| o.usage_reported && o.complete));
        assert_eq!(
            record.allocation.as_ref().unwrap().usage.reported_input,
            server.count() as u64 * 11
        );
    }
}

#[tokio::test]
async fn cancellation_closes_inflight_model_and_settles_usage_without_replay() {
    let _lock = FIXTURES.lock().await;
    let server = Server::new("openai-api", |_, _| {
        thread::sleep(Duration::from_millis(250));
        json!({"ok":true})
    });
    let fixture = Fixture::new(Some(8));
    let registration = register(
        declaration(HandlerKind::Prompt, HookDialect::Native, 0),
        config(&server, "openai-api"),
    );
    let tools = fixture.executor(vec![registration], AccessPolicy::default());
    let events = fixture.events.clone();
    let run = tokio::spawn(async move {
        let mut session = NativeSession::with_tools(
            Box::new(Source {
                calls: VecDeque::from([vec![call()]]),
                results: Arc::new(Mutex::new(Vec::new())),
            }),
            tools,
        );
        let (_sender, mut commands) = mpsc::channel(1);
        session.turn("source".into(), &mut commands, &events).await
    });
    tokio::time::timeout(Duration::from_secs(2), async {
        while server.count() == 0 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    run.abort();
    assert!(matches!(run.await, Err(error) if error.is_cancelled()));
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let record = fixture.record();
            if record
                .operations
                .iter()
                .filter(|o| o.phase.starts_with("hook:") && o.host_invocation.is_some())
                .all(|o| o.complete)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(server.count(), 1);
    assert!(!fixture.root.path().join("result").exists());
    assert!(
        fixture
            .record()
            .allocation
            .as_ref()
            .unwrap()
            .usage
            .unknown_input
    );
    assert!(
        hook(&fixture.record()).outcome.is_none(),
        "interrupted hook outcome was falsely completed"
    );
}

#[tokio::test]
async fn input_output_and_inspection_bounds_fail_closed_without_truncating_evidence() {
    let _lock = FIXTURES.lock().await;
    for case in ["input", "expansion", "output", "inspections"] {
        let fixture = Fixture::new(Some(8));
        let server = Server::new("openai-api", move |_, _| {
            if case == "inspections" {
                json!({"tool":{"name":"snapshot_read","arguments":{"path":"public"}}})
            } else {
                json!({"ok":true})
            }
        });
        let mut configuration = config(&server, "openai-api");
        match case {
            "input" => configuration.max_input_bytes = 64,
            "expansion" => configuration.prompt = "$ARGUMENTS ".repeat(1000),
            "output" => configuration.max_output_bytes = 1,
            "inspections" => configuration.max_inspections = 1,
            _ => unreachable!(),
        }
        let kind = if case == "inspections" {
            HandlerKind::Agent
        } else {
            HandlerKind::Prompt
        };
        let registration = register(declaration(kind, HookDialect::Native, 0), configuration);
        let result = fixture
            .run(
                fixture.executor(vec![registration], AccessPolicy::default()),
                call(),
            )
            .await;
        assert!(!result[0].success, "{case}: {result:?}");
        assert_eq!(
            server.count(),
            match case {
                "input" | "expansion" => 0,
                "output" => 1,
                "inspections" => 2,
                _ => unreachable!(),
            }
        );
        assert!(!fixture.root.path().join("result").exists());
    }
}

#[tokio::test]
async fn selected_connection_credential_policy_is_not_discarded_with_live_tool_authority() {
    let _lock = FIXTURES.lock().await;
    for captured_by_owner in [false, true] {
        let fixture = Fixture::new(Some(8));
        let secret = fixture.root.path().join("selected-model-store");
        std::fs::write(&secret, "SELECTED-MODEL-PRIVATE-CANARY").unwrap();
        let aliases = tempfile::tempdir().unwrap();
        let alias = aliases.path().join("credential");
        std::os::unix::fs::symlink(&secret, &alias).unwrap();
        let server = Server::new("openai-api", |_, request| {
            assert!(
                !request
                    .to_string()
                    .contains("SELECTED-MODEL-PRIVATE-CANARY")
            );
            json!({"ok":true})
        });
        let mut configuration = config(&server, "openai-api");
        configuration
            .connection
            .access
            .credential_paths
            .push(alias.clone());
        let registration = register(
            declaration(HandlerKind::Prompt, HookDialect::Native, 0),
            configuration,
        );
        let result = fixture
            .run(
                fixture.executor(
                    vec![registration],
                    AccessPolicy {
                        credential_paths: if captured_by_owner {
                            vec![alias]
                        } else {
                            vec![]
                        },
                        ..AccessPolicy::default()
                    },
                ),
                call(),
            )
            .await;
        assert_eq!(result[0].success, captured_by_owner);
        assert_eq!(server.count(), usize::from(captured_by_owner));
    }
}

#[tokio::test]
async fn unavailable_selected_models_and_agent_schema_errors_never_fallback() {
    let _lock = FIXTURES.lock().await;
    for adapter in ["openai-api", "anthropic-api"] {
        for verdict in [
            json!({"transport_error":true}),
            json!({"ok":true,"impossible":false}),
            json!({"ok":true,"model":"replacement"}),
        ] {
            let server = Server::new(adapter, move |_, _| verdict.clone());
            let fixture = Fixture::new(Some(8));
            let registration = register(
                declaration(HandlerKind::Agent, HookDialect::Native, 0),
                config(&server, adapter),
            );
            let result = fixture
                .run(
                    fixture.executor(vec![registration], AccessPolicy::default()),
                    call(),
                )
                .await;
            assert!(!result[0].success);
            assert_eq!(server.count(), 1);
            assert_eq!(
                server.requests.lock().unwrap()[0]["model"],
                "explicit-hook-model"
            );
            assert!(!fixture.root.path().join("result").exists());
        }
    }
}

#[tokio::test]
async fn claude_prompt_continuation_setting_is_retained_as_host_configuration() {
    let _lock = FIXTURES.lock().await;
    for continue_on_block in [false, true] {
        let server = Server::new(
            "openai-api",
            |_, _| json!({"ok":false,"reason":"blocked by inspected evidence"}),
        );
        let fixture = Fixture::new(Some(8));
        let mut configuration = config(&server, "openai-api");
        configuration.continue_on_block = continue_on_block;
        let registration = register(
            declaration(HandlerKind::Prompt, HookDialect::Claude, 0),
            configuration,
        );
        let result = fixture
            .run(
                fixture.executor(vec![registration], AccessPolicy::default()),
                call(),
            )
            .await;
        assert!(!result[0].success);
        let record = fixture.record();
        let receipt = hook(&record);
        assert!(
            matches!(receipt.outcome,Some(RawOutcome::Model {continue_on_block:value,..}) if value==continue_on_block)
        );
        assert_eq!(
            receipt
                .pending_proposals
                .iter()
                .filter(|p| matches!(p.kind, demoncoder::plugins::receipts::ProposalKind::Control))
                .count(),
            usize::from(!continue_on_block)
        );
    }
}

#[tokio::test]
async fn credential_revocation_between_agent_requests_prevents_another_delivery() {
    let _lock = FIXTURES.lock().await;
    let fixture = Fixture::new(Some(8));
    let aliases = tempfile::tempdir().unwrap();
    let old = fixture.root.path().join("credential-old");
    std::fs::write(&old, "old private").unwrap();
    let alias = aliases.path().join("credential");
    std::os::unix::fs::symlink(&old, &alias).unwrap();
    let changed = alias.clone();
    let target = fixture.root.path().join("public");
    let server = Server::new("openai-api", move |index, _| {
        assert_eq!(
            index, 0,
            "new credential evidence was delivered after revocation"
        );
        std::fs::remove_file(&changed).unwrap();
        std::os::unix::fs::symlink(&target, &changed).unwrap();
        json!({"tool":{"name":"snapshot_read","arguments":{"path":"public"}}})
    });
    let registration = register(
        declaration(HandlerKind::Agent, HookDialect::Native, 0),
        config(&server, "openai-api"),
    );
    let result = fixture
        .run(
            fixture.executor(
                vec![registration],
                AccessPolicy {
                    credential_paths: vec![alias],
                    ..AccessPolicy::default()
                },
            ),
            call(),
        )
        .await;
    assert!(!result[0].success);
    assert_eq!(server.count(), 1);
}

#[test]
fn registration_requires_a_host_model_and_never_executes_codex_source_tags() {
    let connection: Connection = serde_json::from_value(
        json!({"adapter":"openai-api","model":"selected-model","api_key":"synthetic"}),
    )
    .unwrap();
    for kind in [HandlerKind::Prompt, HandlerKind::Agent] {
        let config = ModelConfig::new(connection.clone(), "Inspect.".into());
        assert!(try_register(declaration(kind, HookDialect::Native, 0), config.clone()).is_ok());
        assert!(try_register(declaration(kind, HookDialect::Codex, 0), config.clone()).is_err());
        let mut missing = config.clone();
        missing.connection.model = None;
        assert!(try_register(declaration(kind, HookDialect::Native, 0), missing).is_err());
        let mut wrong = config.clone();
        wrong.connection.adapter = "missing-provider".into();
        assert!(try_register(declaration(kind, HookDialect::Native, 0), wrong).is_err());
        let mut forbidden = config;
        forbidden.continue_on_block = true;
        assert!(try_register(declaration(kind, HookDialect::Native, 0), forbidden).is_err());
    }
}
