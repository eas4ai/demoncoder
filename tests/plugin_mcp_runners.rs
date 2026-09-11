use demoncoder::{
    config::Connection,
    events::EventSink,
    native::{Model, NativeSession},
    plugins::{
        self,
        dispatch::*,
        gate_snapshot::GateReadSet,
        hook_types::{HandlerKind, HookDialect},
        runners::{HttpConfig, HttpCredential, McpBinding, McpConfig, McpRunner},
        services::{
            AdmittedTool, ManagedService, ManagedServices, ServiceConfig, ServiceIdentity,
            ServiceTransport,
        },
    },
    session::Session,
    tools::{AccessPolicy, ToolCall, ToolExecutor, ToolResult},
    workflow::runtime::{Record, SharedRuntime},
};
use serde_json::json;
use std::{
    collections::VecDeque,
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
        Self::with_allowance(root, true)
    }
    fn with_allowance(root: tempfile::TempDir, allowance: bool) -> Self {
        let connection: Connection =
            serde_json::from_value(json!({"adapter":"openai-api"})).unwrap();
        let (runtime, _) = SharedRuntime::open(root.path(), &connection, None).unwrap();
        if allowance {
            runtime
                .allocate(
                    demoncoder::workflow::allocation::Limits {
                        seconds: 30,
                        model_calls: 8,
                        tool_calls: 64,
                    },
                    None,
                )
                .unwrap();
        }
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
        required_gate: class != HandlerClass::Observer,
        source: None,
        once: None,
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
            runner: HandlerKind::McpTool,
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

fn metadata() -> serde_json::Value {
    json!({"name":"gate","inputSchema":{"type":"object","additionalProperties":true}})
}

#[tokio::test]
async fn package_runner_rejects_foreign_source_even_without_once() {
    let _lock = FIXTURES.lock().await;
    let fixture = Fixture::new();
    let package = package(HookDialect::Native);
    let service = service(
        &fixture,
        package.clone(),
        ServiceTransport::Http(HttpConfig::new("http://127.0.0.1:1/mcp".into())),
    );
    let mut d = declaration("mcp", HookDialect::Native, HandlerClass::DecisionGate);
    d.source = Some(plugins::once::ActivationSource::host_namespace("mcp-fixture").unwrap());
    let error = McpRunner::registration(
        package,
        d,
        McpBinding {
            service,
            tool: "gate".into(),
            input: json!({}),
        },
        None,
        McpConfig::default(),
    )
    .err()
    .expect("foreign captured source must be rejected");
    assert!(error.to_string().contains("captured source"), "{error:#}");
}
#[tokio::test]
async fn package_runner_rejects_service_from_same_named_distinct_source() {
    let _lock = FIXTURES.lock().await;
    let fixture = Fixture::new();
    let actual = package(HookDialect::Native);
    let different = package(HookDialect::Native);
    assert_eq!(actual.name(), different.name());
    assert_eq!(actual.digest(), different.digest());
    let service = service(
        &fixture,
        actual,
        ServiceTransport::Http(HttpConfig::new("http://127.0.0.1:1/mcp".into())),
    );
    let d = declaration("mcp", HookDialect::Native, HandlerClass::DecisionGate);
    let error = McpRunner::registration(
        different,
        d,
        McpBinding {
            service,
            tool: "gate".into(),
            input: json!({}),
        },
        None,
        McpConfig::default(),
    )
    .err()
    .expect("service from another source must be rejected");
    assert!(error.to_string().contains("package differs"), "{error:#}");
}
fn package(dialect: HookDialect) -> Arc<plugins::Package> {
    let source = tempfile::tempdir().unwrap();
    let directory = if dialect == HookDialect::Codex {
        ".codex-plugin"
    } else {
        ".claude-plugin"
    };
    std::fs::create_dir(source.path().join(directory)).unwrap();
    std::fs::write(
        source.path().join(directory).join("plugin.json"),
        r#"{"name":"mcp-fixture","version":"1.0.0"}"#,
    )
    .unwrap();
    Arc::new(plugins::inspect(source.path(), &plugins::ImportOptions::default()).unwrap())
}
fn service(
    fixture: &Fixture,
    package: Arc<plugins::Package>,
    transport: ServiceTransport,
) -> Arc<ManagedService> {
    service_with_tools(
        fixture,
        package,
        transport,
        vec![AdmittedTool {
            metadata: metadata(),
            read_only: false,
        }],
    )
}
fn service_with_tools(
    fixture: &Fixture,
    package: Arc<plugins::Package>,
    transport: ServiceTransport,
    tools: Vec<AdmittedTool>,
) -> Arc<ManagedService> {
    ManagedServices::default()
        .admit(package, service_config(fixture, transport, tools))
        .unwrap()
}
fn service_config(
    fixture: &Fixture,
    transport: ServiceTransport,
    tools: Vec<AdmittedTool>,
) -> ServiceConfig {
    use std::os::unix::fs::MetadataExt;
    let root = std::fs::metadata(fixture.root.path()).unwrap();
    ServiceConfig {
        identity: ServiceIdentity {
            workspace: (root.dev(), root.ino()),
            role: "worker".into(),
            generation: "fixture-generation".into(),
            state: "state-1".into(),
            credential_revision: "credentials-1".into(),
        },
        transport,
        tools,
        timeout_ms: 2000,
        max_calls: 16,
    }
}

fn register(
    package: Arc<plugins::Package>,
    service: Arc<ManagedService>,
    dialect: HookDialect,
) -> Registration {
    McpRunner::registration(
        package,
        declaration(
            "mcp",
            dialect,
            if dialect == HookDialect::Native {
                HandlerClass::DecisionGate
            } else {
                HandlerClass::Combined
            },
        ),
        McpBinding {
            service,
            tool: "gate".into(),
            input: json!({"input":"${tool_input}","literal":"${tool_input.content}"}),
        },
        None,
        McpConfig {
            model: Some("fixture-model".into()),
            ..McpConfig::default()
        },
    )
    .unwrap()
}
struct Peer {
    endpoint: String,
    requests: Arc<Mutex<Vec<(String, serde_json::Value)>>>,
    worker: Option<std::thread::JoinHandle<()>>,
    stopped: Arc<std::sync::atomic::AtomicBool>,
}
impl Peer {
    fn new(
        handler: impl Fn(&str, &serde_json::Value) -> (u16, String, Vec<u8>) + Send + 'static,
    ) -> Self {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!(
            "http://{}/mcp/a%2Fb?q=x%2Fy",
            listener.local_addr().unwrap()
        );
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stopped = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (record, stop) = (requests.clone(), stopped.clone());
        let worker = std::thread::spawn(move || {
            while !stop.load(std::sync::atomic::Ordering::Acquire) {
                let Ok((mut stream, _)) = listener.accept() else {
                    std::thread::sleep(Duration::from_millis(2));
                    continue;
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut bytes = Vec::new();
                let mut buffer = [0; 4096];
                let end = loop {
                    let count = stream.read(&mut buffer).unwrap();
                    assert!(count > 0);
                    bytes.extend_from_slice(&buffer[..count]);
                    if let Some(i) = bytes.windows(4).position(|b| b == b"\r\n\r\n") {
                        break i + 4;
                    }
                    assert!(bytes.len() < 65536);
                };
                let headers = String::from_utf8(bytes[..end].to_vec()).unwrap();
                let length = headers
                    .lines()
                    .find_map(|line| {
                        let (key, value) = line.split_once(':')?;
                        key.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().unwrap())
                    })
                    .unwrap_or(0);
                while bytes.len() < end + length {
                    let count = stream.read(&mut buffer).unwrap();
                    assert!(count > 0);
                    bytes.extend_from_slice(&buffer[..count]);
                }
                let request = if length == 0 {
                    serde_json::Value::Null
                } else {
                    serde_json::from_slice(&bytes[end..end + length]).unwrap()
                };
                record
                    .lock()
                    .unwrap()
                    .push((headers.clone(), request.clone()));
                let (status, extra, body) = handler(&headers, &request);
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status} Fixture\r\nContent-Length: {}\r\nConnection: close\r\n{extra}\r\n",
                    body.len()
                );
                let _ = stream.write_all(&body);
            }
        });
        Self {
            endpoint,
            requests,
            worker: Some(worker),
            stopped,
        }
    }
    fn result(allow: bool, text: bool, sse: bool, dialect: HookDialect) -> Self {
        Self::new(move |headers, request| {
            if headers.starts_with("DELETE") || request.get("id").is_none() {
                return (202, String::new(), vec![]);
            }
            let result = match request["method"].as_str().unwrap() {
                "initialize" => {
                    json!({"protocolVersion":"2025-11-25","capabilities":{"tools":{}},"serverInfo":{"name":"fixture","version":"1"}})
                }
                "tools/list" => json!({"tools":[metadata()]}),
                "tools/call" => {
                    let mut verdict = json!({"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":if allow {"allow"} else {"deny"},"permissionDecisionReason":"controlled"}});
                    if dialect == HookDialect::Codex && allow {
                        verdict["hookSpecificOutput"]["updatedInput"] =
                            request["params"]["arguments"]["input"].clone();
                    }
                    if text {
                        json!({"content":[{"type":"text","text":verdict.to_string()}]})
                    } else {
                        json!({"content":[],"structuredContent":verdict})
                    }
                }
                other => panic!("unexpected request {other}"),
            };
            let response = json!({"jsonrpc":"2.0","id":request["id"],"result":result}).to_string();
            let session = if request["method"] == "initialize" {
                "Mcp-Session-Id: fixture-session-token\r\n"
            } else {
                ""
            };
            if sse {
                (
                    200,
                    format!("Content-Type: text/event-stream\r\n{session}"),
                    format!("event: message\r\ndata: {response}\r\n\r\n").into_bytes(),
                )
            } else {
                (
                    200,
                    format!("Content-Type: application/json\r\n{session}"),
                    response.into_bytes(),
                )
            }
        })
    }
    fn no_requests(&self) -> bool {
        // Schema retrieval is a bodyless HTTP GET, not a JSON-RPC method.
        self.requests.lock().unwrap().is_empty()
    }
    fn methods(&self) -> Vec<String> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter_map(|(_, r)| r["method"].as_str().map(str::to_owned))
            .collect()
    }
}
impl Drop for Peer {
    fn drop(&mut self) {
        self.stopped
            .store(true, std::sync::atomic::Ordering::Release);
        let joined = self.worker.take().unwrap().join();
        if !std::thread::panicking() {
            joined.unwrap();
        }
    }
}
#[tokio::test]
async fn actual_managed_http_json_and_sse_source_verdicts_and_literal_data() {
    let _lock = FIXTURES.lock().await;
    for dialect in [HookDialect::Native, HookDialect::Claude, HookDialect::Codex] {
        for allow in [true, false] {
            for text in [false, true] {
                let peer = Peer::result(allow, text, text, dialect);
                let fixture = Fixture::new();
                let package = package(dialect);
                let service = service(
                    &fixture,
                    package.clone(),
                    ServiceTransport::Http(HttpConfig::new(peer.endpoint.clone())),
                );
                let registration = register(package, service.clone(), dialect);
                let mut request = call("result");
                request.arguments["content"] = json!("$(touch bad) ${tool_name} `literal` $HOME");
                let results = fixture
                    .run(
                        fixture.executor(vec![registration], false),
                        vec![request.clone()],
                    )
                    .await;
                assert!(!results.is_empty(), "no actual tool result");
                assert_eq!(results[0].success, allow, "{dialect:?} {text} {results:?}");
                assert_eq!(fixture.root.path().join("result").exists(), allow);
                assert_eq!(
                    peer.methods(),
                    [
                        "initialize",
                        "notifications/initialized",
                        "tools/list",
                        "tools/call"
                    ]
                );
                {
                    let calls = peer.requests.lock().unwrap();
                    let (headers, call) = calls
                        .iter()
                        .find(|(_, r)| r["method"] == "tools/call")
                        .unwrap();
                    assert!(headers.contains("mcp-session-id: fixture-session-token"));
                    assert!(headers.contains("mcp-protocol-version: 2025-11-25"));
                    assert_eq!(
                        call["params"]["arguments"]["literal"],
                        request.arguments["content"]
                    );
                    if dialect == HookDialect::Claude {
                        assert_eq!(
                            call["params"]["arguments"]["input"],
                            request.arguments.to_string()
                        );
                    } else {
                        assert_eq!(call["params"]["arguments"]["input"], request.arguments);
                    }
                }
                service.stop().await.unwrap();
            }
        }
    }
}

fn rpc(request: &serde_json::Value, result: serde_json::Value) -> (u16, String, Vec<u8>) {
    (
        200,
        "Content-Type: application/json\r\n".into(),
        json!({"jsonrpc":"2.0","id":request["id"],"result":result})
            .to_string()
            .into_bytes(),
    )
}
fn init() -> serde_json::Value {
    json!({"protocolVersion":"2025-11-25","capabilities":{"tools":{}},"serverInfo":{"name":"fixture","version":"1"}})
}
fn allow_result() -> serde_json::Value {
    json!({"content":[],"structuredContent":{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow"}}})
}
#[tokio::test]
async fn dropping_an_active_call_revokes_the_service_before_another_effect() {
    let _lock = FIXTURES.lock().await;
    let active = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let peer_active = active.clone();
    let peer = Peer::new(move |_, request| match request["method"].as_str() {
        Some("initialize") => rpc(request, init()),
        Some("tools/list") => rpc(request, json!({"tools":[metadata()]})),
        Some("tools/call") => {
            peer_active.store(true, std::sync::atomic::Ordering::Release);
            std::thread::sleep(Duration::from_millis(400));
            rpc(request, allow_result())
        }
        _ => (202, String::new(), vec![]),
    });
    let fixture = Fixture::new();
    let package = package(HookDialect::Native);
    let managed = service(
        &fixture,
        package.clone(),
        ServiceTransport::Http(HttpConfig::new(peer.endpoint.clone())),
    );
    let registration = register(package.clone(), managed.clone(), HookDialect::Native);
    {
        let run = fixture.run(
            fixture.executor(vec![registration], false),
            vec![call("cancelled")],
        );
        tokio::pin!(run);
        while !active.load(std::sync::atomic::Ordering::Acquire) {
            tokio::select! {_ = &mut run => panic!("call completed before cancellation"), _ = tokio::time::sleep(Duration::from_millis(5)) => {}}
        }
    }
    // Dropping the caller must cancel owned work even without a cooperative stop().
    tokio::time::sleep(Duration::from_millis(80)).await;
    assert_ne!(
        managed.state(),
        demoncoder::plugins::services::ServiceState::Ready
    );
    assert!(!fixture.root.path().join("cancelled").exists());
    assert!(
        fixture.record().recovery_pending,
        "cancelled active call left runtime open"
    );
    let before = peer.methods();
    let replacement = service(
        &fixture,
        package.clone(),
        ServiceTransport::Http(HttpConfig::new(peer.endpoint.clone())),
    );
    fixture
        .run(
            fixture.executor(
                vec![register(package, replacement.clone(), HookDialect::Native)],
                false,
            ),
            vec![call("next")],
        )
        .await;
    assert!(!fixture.root.path().join("next").exists());
    assert_eq!(peer.methods(), before);
    replacement.stop().await.unwrap();
    managed.stop().await.unwrap();
}
#[tokio::test]
async fn reused_service_rejects_changed_schema_before_second_effect() {
    let _lock = FIXTURES.lock().await;
    let pages = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed = pages.clone();
    let peer = Peer::new(move |_, request| match request["method"].as_str() {
        Some("initialize") => rpc(request, init()),
        Some("tools/list") => {
            let mut tool = metadata();
            if observed.fetch_add(1, std::sync::atomic::Ordering::AcqRel) > 0 {
                tool["description"] = json!("changed meaning");
            }
            rpc(request, json!({"tools":[tool]}))
        }
        Some("tools/call") => rpc(request, allow_result()),
        _ => (202, String::new(), vec![]),
    });
    let fixture = Fixture::new();
    let package = package(HookDialect::Native);
    let service = service(
        &fixture,
        package.clone(),
        ServiceTransport::Http(HttpConfig::new(peer.endpoint.clone())),
    );
    let registration = register(package, service.clone(), HookDialect::Native);
    let mut second = call("second");
    second.id = "second-call".into();
    let results = fixture
        .run(
            fixture.executor(vec![registration], false),
            vec![call("first"), second],
        )
        .await;
    assert!(results[0].success);
    assert!(
        !fixture.root.path().join("second").exists(),
        "changed catalog still authorized a second write"
    );
    assert_eq!(
        peer.methods().iter().filter(|m| *m == "tools/call").count(),
        1
    );
    service.stop().await.unwrap();
}

fn stdio_package(dialect: HookDialect) -> (tempfile::TempDir, Arc<plugins::Package>) {
    let source = tempfile::tempdir().unwrap();
    let directory = if dialect == HookDialect::Codex {
        ".codex-plugin"
    } else {
        ".claude-plugin"
    };
    std::fs::create_dir(source.path().join(directory)).unwrap();
    std::fs::write(
        source.path().join(directory).join("plugin.json"),
        r#"{"name":"mcp-fixture","version":"1.0.0"}"#,
    )
    .unwrap();
    std::fs::write(
        source.path().join("peer.py"),
        include_str!("fixtures/plugins/mcp_peer.py"),
    )
    .unwrap();
    let package =
        Arc::new(plugins::inspect(source.path(), &plugins::ImportOptions::default()).unwrap());
    (source, package)
}
fn stdio(mode: &str, dialect: HookDialect) -> ServiceTransport {
    use demoncoder::plugins::runners::{CommandConfig, CommandProgram};
    ServiceTransport::Stdio(
        CommandConfig::new(CommandProgram::Argv(vec![
            "/usr/bin/python3".into(),
            "${CODEX_PLUGIN_ROOT}/peer.py".into(),
            mode.into(),
            dialect.as_str().into(),
        ]))
        .into(),
    )
}
#[tokio::test]
async fn actual_confined_stdio_source_results_and_retained_code() {
    let _lock = FIXTURES.lock().await;
    for dialect in [HookDialect::Native, HookDialect::Claude, HookDialect::Codex] {
        for mode in ["allow", "text", "deny", "error"] {
            let fixture = Fixture::new();
            let (source, package) = stdio_package(dialect);
            let service = service(&fixture, package.clone(), stdio(mode, dialect));
            let registration = register(package, service.clone(), dialect);
            std::fs::write(
                source.path().join("peer.py"),
                "raise RuntimeError('live source ran')",
            )
            .unwrap();
            let results = fixture
                .run(
                    fixture.executor(vec![registration], true),
                    vec![call("result")],
                )
                .await;
            assert!(
                !results.is_empty(),
                "missing tool result {dialect:?} {mode}"
            );
            assert_eq!(
                results[0].success,
                matches!(mode, "allow" | "text"),
                "{dialect:?} {mode}: {results:?}"
            );
            assert_eq!(
                fixture.root.path().join("result").exists(),
                matches!(mode, "allow" | "text")
            );
            service.stop().await.unwrap();
        }
    }
}
#[tokio::test]
async fn explicit_nonsecret_stdio_environment_is_not_treated_as_credential() {
    let _lock = FIXTURES.lock().await;
    let fixture = Fixture::new();
    let (_, package) = stdio_package(HookDialect::Native);
    let mut transport = stdio("allow", HookDialect::Native);
    if let ServiceTransport::Stdio(config) = &mut transport {
        config
            .command
            .environment
            .insert("ORDINARY_FLAG".into(), "1".into());
    }
    let service = service(&fixture, package.clone(), transport);
    let registration = register(package, service.clone(), HookDialect::Native);
    let results = fixture
        .run(
            fixture.executor(vec![registration], false),
            vec![call("result")],
        )
        .await;
    assert!(
        results[0].success,
        "ordinary literal environment blocked protocol data: {results:?}"
    );
    service.stop().await.unwrap();
}
fn hooks(record: &Record) -> Vec<&plugins::receipts::HookReceipt> {
    record
        .operations
        .iter()
        .filter_map(|o| o.tool_receipt.as_ref())
        .filter_map(|r| r.plugin_admission.as_ref())
        .flat_map(|p| &p.hooks)
        .collect()
}
fn pending_hook(record: &Record) -> &plugins::receipts::HookReceipt {
    let reserved = hooks(record);
    assert_eq!(reserved.len(), 1);
    assert!(reserved[0].outcome.is_none());
    assert!(reserved[0].uncertain_effects);
    reserved[0]
}
#[tokio::test]
async fn actual_http_protocol_failures_hold_effects_and_never_replay_calls() {
    let _lock = FIXTURES.lock().await;
    for mode in [
        "version",
        "capabilities",
        "duplicate-session",
        "wrong-id",
        "batch",
        "both",
        "duplicate-key",
        "redirect",
        "notification-body",
        "sse-legacy",
        "sse-truncated",
        "sse-duplicate",
        "sse-flood",
        "server-request",
        "changed-session",
        "is-error",
        "unsupported-content",
        "credential",
        "escaped-credential",
        "session",
        "disconnect",
    ] {
        let peer = Peer::new(move |headers, request| {
            if headers.starts_with("DELETE") {
                return (202, String::new(), vec![]);
            }
            if request.get("id").is_none() {
                return (
                    202,
                    String::new(),
                    if mode == "notification-body" {
                        b"not-empty".to_vec()
                    } else {
                        vec![]
                    },
                );
            }
            if request.get("method").is_none() {
                return (202, String::new(), vec![]);
            }
            let method = request["method"].as_str().unwrap();
            let mut result = match method {
                "initialize" => init(),
                "tools/list" => json!({"tools":[metadata()]}),
                _ => allow_result(),
            };
            if method == "initialize" {
                if mode == "version" {
                    result["protocolVersion"] = json!("2026-07-28");
                }
                if mode == "capabilities" {
                    result["capabilities"] = json!({});
                }
            }
            if method == "tools/call" {
                match mode {
                    "is-error" => result["isError"] = json!(true),
                    "unsupported-content" => {
                        result["content"] =
                            json!([{"type":"image","data":"a","mimeType":"image/png"}])
                    }
                    "credential" => result["secret-canary"] = json!(true),
                    "escaped-credential" => {
                        result = json!({"content":[{"type":"text","text":r#"{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow","permissionDecisionReason":"\u0073ecret-canary"}}"#}]})
                    }
                    "session" => result["extra"] = json!("session-canary"),
                    _ => {}
                }
            }
            let mut response = json!({"jsonrpc":"2.0","id":request["id"],"result":result});
            let session = if mode == "duplicate-session" {
                "Mcp-Session-Id: session-canary\r\nMcp-Session-Id: different-session\r\n"
            } else if method == "initialize" {
                "Mcp-Session-Id: session-canary\r\n"
            } else if mode == "changed-session" {
                "Mcp-Session-Id: different-session\r\n"
            } else {
                ""
            };
            if method == "tools/call" {
                match mode {
                    "wrong-id" => response["id"] = json!(999),
                    "batch" => response = json!([response]),
                    "both" => response["error"] = json!({"code":-1,"message":"secret-canary"}),
                    "duplicate-key" => {
                        return (
                            200,
                            "Content-Type: application/json\r\n".into(),
                            br#"{"jsonrpc":"2.0","id":3,"result":{},"result":{}}"#.to_vec(),
                        );
                    }
                    "redirect" => {
                        return (307, "Location: http://127.0.0.1:9/leak\r\n".into(), vec![]);
                    }
                    "disconnect" => {
                        return (200, "Content-Type: application/json\r\n".into(), vec![]);
                    }
                    mode if mode.starts_with("sse-") || mode == "server-request" => {
                        let terminal = format!("data: {}\n\n", response);
                        let body = match mode {
                            "sse-legacy" => "event: endpoint\ndata: http://127.0.0.1:9/new\n\n".into(),
                            "sse-truncated" => format!("data: {}",response),
                            "sse-duplicate" => terminal.repeat(2),
                            "sse-flood" => format!("{}{}","data: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/message\",\"params\":{}}\n\n".repeat(65),terminal),
                            _ => format!("data: {{\"jsonrpc\":\"2.0\",\"id\":\"host-request\",\"method\":\"sampling/createMessage\",\"params\":{{}}}}\n\n{terminal}"),
                        };
                        return (
                            200,
                            "Content-Type: text/event-stream\r\n".into(),
                            body.into_bytes(),
                        );
                    }
                    _ => {}
                }
            }
            (
                200,
                format!("Content-Type: application/json\r\n{session}"),
                response.to_string().into_bytes(),
            )
        });
        let fixture = Fixture::new();
        let package = package(HookDialect::Native);
        let mut config = HttpConfig::new(peer.endpoint.clone());
        config.credentials.insert(
            "Authorization".into(),
            HttpCredential::bearer("secret-canary".into()),
        );
        let service = service(&fixture, package.clone(), ServiceTransport::Http(config));
        let registration = register(package, service.clone(), HookDialect::Native);
        let results = fixture
            .run(
                fixture.executor(vec![registration], true),
                vec![call("never")],
            )
            .await;
        assert!(
            !fixture.root.path().join("never").exists(),
            "{mode} released actual effect: {results:?}"
        );
        let record = fixture.record();
        assert!(
            hooks(&record).iter().any(|h| h.uncertain_effects)
                || record.operations.iter().any(|o| matches!(
                    o.host_invocation,
                    Some(
                        demoncoder::workflow::runtime::HostInvocation::PluginService {
                            outcome: demoncoder::workflow::runtime::PluginServiceOutcome::Uncertain,
                            ..
                        }
                    )
                ) && !o.complete),
            "{mode} lost uncertainty"
        );
        let receipts = serde_json::to_string(&hooks(&record)).unwrap();
        assert!(
            !receipts.contains("secret-canary") && !receipts.contains("session-canary"),
            "{mode} retained secret"
        );
        assert!(
            peer.methods().iter().filter(|m| *m == "tools/call").count() <= 1,
            "{mode} repeated effect"
        );
        service.stop().await.unwrap();
    }
}
#[tokio::test]
async fn unchanged_authority_reuses_connection_across_real_operations_and_usage_changes() {
    let _lock = FIXTURES.lock().await;
    let peer = Peer::result(true, false, false, HookDialect::Native);
    let fixture = Fixture::new();
    let package = package(HookDialect::Native);
    let service = service(
        &fixture,
        package.clone(),
        ServiceTransport::Http(HttpConfig::new(peer.endpoint.clone())),
    );
    let registration = register(package, service.clone(), HookDialect::Native);
    let original = fixture.record().allocation.unwrap();
    let mut second = call("second");
    second.id = "second-call".into();
    let results = fixture
        .run(
            fixture.executor(vec![registration], false),
            vec![call("first"), second],
        )
        .await;
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|r| r.success), "{results:?}");
    assert_eq!(
        peer.methods().iter().filter(|m| *m == "initialize").count(),
        1
    );
    assert_eq!(
        peer.methods().iter().filter(|m| *m == "tools/call").count(),
        2
    );
    let final_allocation = fixture.record().allocation.unwrap();
    assert_eq!(original.deadline_ms, final_allocation.deadline_ms);
    assert_eq!(final_allocation.tool_calls, 2);
    service.stop().await.unwrap();
}
#[tokio::test]
async fn changed_stdio_read_evidence_holds_the_next_real_operation() {
    let _lock = FIXTURES.lock().await;
    let fixture = Fixture::new();
    std::fs::write(fixture.root.path().join("watched"), "before").unwrap();
    let (_, package) = stdio_package(HookDialect::Native);
    let service = service(
        &fixture,
        package.clone(),
        stdio("allow", HookDialect::Native),
    );
    let mut registration = register(package, service.clone(), HookDialect::Native);
    registration.declaration.reads =
        GateReadSet::new(vec!["watched".into()], vec![], vec![]).unwrap();
    let mut second = call("second");
    second.id = "second-call".into();
    let results = fixture
        .run(
            fixture.executor(vec![registration], false),
            vec![call("watched"), second],
        )
        .await;
    assert!(results[0].success, "{results:?}");
    assert!(!fixture.root.path().join("second").exists());
    assert!(hooks(&fixture.record()).iter().any(|h| h.uncertain_effects));
    service.stop().await.unwrap();
}

#[tokio::test]
async fn cancelled_bootstrap_is_durable_and_blocks_the_live_runtime_before_hook_dispatch() {
    let _lock = FIXTURES.lock().await;
    let active = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let observed = active.clone();
    let fixture = Fixture::new();
    let runtime = fixture.runtime.clone();
    let reservation = Arc::new(Mutex::new(None));
    let captured = reservation.clone();
    let peer = Peer::new(move |_, request| {
        if request["method"] == "initialize" {
            let record = runtime.record().unwrap();
            // Dependency preparation owns a reservation, not an observed hook result.
            let reserved = pending_hook(&record);
            *captured.lock().unwrap() = Some(serde_json::to_value(reserved).unwrap());
            assert!(record.operations.iter().any(|o| !o.complete
                && matches!(
                    o.host_invocation,
                    Some(
                        demoncoder::workflow::runtime::HostInvocation::PluginService {
                            outcome: demoncoder::workflow::runtime::PluginServiceOutcome::Pending,
                            ..
                        }
                    )
                )));
            observed.store(true, std::sync::atomic::Ordering::Release);
            std::thread::sleep(Duration::from_millis(400));
            rpc(request, init())
        } else {
            (202, String::new(), vec![])
        }
    });
    let package = package(HookDialect::Native);
    let managed = service(
        &fixture,
        package.clone(),
        ServiceTransport::Http(HttpConfig::new(peer.endpoint.clone())),
    );
    {
        let run = fixture.run(
            fixture.executor(
                vec![register(
                    package.clone(),
                    managed.clone(),
                    HookDialect::Native,
                )],
                false,
            ),
            vec![call("cancelled")],
        );
        tokio::pin!(run);
        while !active.load(std::sync::atomic::Ordering::Acquire) {
            tokio::select! {_ = &mut run => panic!("bootstrap ended before cancellation"), _ = tokio::time::sleep(Duration::from_millis(5)) => {}}
        }
    }
    let record = fixture.record();
    assert!(
        record.recovery_pending,
        "cancelled bootstrap left the live runtime open"
    );
    let reserved = pending_hook(&record);
    assert_eq!(
        serde_json::to_value(reserved).unwrap(),
        reservation.lock().unwrap().clone().unwrap(),
        "cancellation must retain the exact pending reservation"
    );
    let replacement = service(
        &fixture,
        package.clone(),
        ServiceTransport::Http(HttpConfig::new(peer.endpoint.clone())),
    );
    fixture
        .run(
            fixture.executor(
                vec![register(package, replacement.clone(), HookDialect::Native)],
                false,
            ),
            vec![call("next")],
        )
        .await;
    assert!(!fixture.root.path().join("next").exists());
    assert_eq!(peer.methods(), ["initialize"]);
    managed.stop().await.unwrap();
    replacement.stop().await.unwrap();
}

#[tokio::test]
async fn service_capacity_is_shared_across_managers_and_released_after_stop() {
    let _lock = FIXTURES.lock().await;
    let fixture = Fixture::new();
    let peer = Peer::result(true, false, false, HookDialect::Native);
    let package = package(HookDialect::Native);
    let mut services = Vec::new();
    let mut registrations = Vec::new();
    for index in 0..8 {
        let managed = service(
            &fixture,
            package.clone(),
            ServiceTransport::Http(HttpConfig::new(peer.endpoint.clone())),
        );
        let mut declared = declaration(
            &format!("gate-{index}"),
            HookDialect::Native,
            HandlerClass::DecisionGate,
        );
        declared.identity.index = index;
        let registration = McpRunner::registration(
            package.clone(),
            declared,
            McpBinding {
                service: managed.clone(),
                tool: "gate".into(),
                input: json!({"input":"${tool_input}"}),
            },
            None,
            McpConfig::default(),
        )
        .unwrap();
        services.push(managed);
        registrations.push(registration);
    }
    let mut second = call("second");
    second.id = "second".into();
    let results = fixture
        .run(
            fixture.executor(registrations, false),
            vec![call("first"), second],
        )
        .await;
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|r| r.success), "{results:?}");
    assert_eq!(
        peer.methods().iter().filter(|m| *m == "initialize").count(),
        8
    );
    let ninth = service(
        &fixture,
        package.clone(),
        ServiceTransport::Http(HttpConfig::new(peer.endpoint.clone())),
    );
    fixture
        .run(
            fixture.executor(
                vec![register(
                    package.clone(),
                    ninth.clone(),
                    HookDialect::Native,
                )],
                false,
            ),
            vec![call("capacity-held")],
        )
        .await;
    assert!(!fixture.root.path().join("capacity-held").exists());
    assert_eq!(
        peer.methods().iter().filter(|m| *m == "initialize").count(),
        8
    );
    services.pop().unwrap().stop().await.unwrap();
    let replacement = service(
        &fixture,
        package.clone(),
        ServiceTransport::Http(HttpConfig::new(peer.endpoint.clone())),
    );
    let results = fixture
        .run(
            fixture.executor(
                vec![register(package, replacement.clone(), HookDialect::Native)],
                false,
            ),
            vec![call("released")],
        )
        .await;
    assert!(results.iter().any(|r| r.success), "{results:?}");
    assert_eq!(
        peer.methods().iter().filter(|m| *m == "initialize").count(),
        9
    );
    for managed in services {
        managed.stop().await.unwrap();
    }
    ninth.stop().await.unwrap();
    replacement.stop().await.unwrap();
}

fn descendants(pid: u32) -> std::collections::BTreeSet<u32> {
    let mut found = std::collections::BTreeSet::new();
    let mut pending = vec![pid];
    while let Some(parent) = pending.pop() {
        let Ok(tasks) = std::fs::read_dir(format!("/proc/{parent}/task")) else {
            continue;
        };
        for task in tasks.flatten() {
            let Ok(children) = std::fs::read_to_string(task.path().join("children")) else {
                continue;
            };
            for child in children
                .split_whitespace()
                .filter_map(|p| p.parse::<u32>().ok())
            {
                if found.insert(child) {
                    pending.push(child);
                }
            }
        }
    }
    found
}
#[tokio::test]
async fn dropping_the_runtime_stops_idle_stdio_and_detached_descendants_with_service_clones() {
    let _lock = FIXTURES.lock().await;
    let before = descendants(std::process::id());
    let fixture = Fixture::new();
    let (_, package) = stdio_package(HookDialect::Native);
    let managed = service(
        &fixture,
        package.clone(),
        stdio("descendant", HookDialect::Native),
    );
    let clone = managed.clone();
    let results = fixture
        .run(
            fixture.executor(
                vec![register(package, managed.clone(), HookDialect::Native)],
                false,
            ),
            vec![call("result")],
        )
        .await;
    assert!(results[0].success, "{results:?}");
    let owned: Vec<_> = descendants(std::process::id())
        .difference(&before)
        .copied()
        .map(|pid| {
            let fd = rustix::process::pidfd_open(
                rustix::process::Pid::from_raw(pid as i32).unwrap(),
                rustix::process::PidfdFlags::empty(),
            )
            .unwrap();
            (pid, fd)
        })
        .collect();
    assert!(
        owned.len() >= 4,
        "detached process control did not create descendants: {}",
        owned.len()
    );
    drop(fixture);
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let exited = owned.iter().all(|(_, fd)| {
                let mut polls = [rustix::event::PollFd::new(fd, rustix::event::PollFlags::IN)];
                rustix::event::poll(
                    &mut polls,
                    Some(&rustix::event::Timespec {
                        tv_sec: 0,
                        tv_nsec: 0,
                    }),
                )
                .unwrap()
                    > 0
            });
            if exited {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("idle service retained its dead runtime or detached descendant");
    assert_ne!(clone.state(), plugins::services::ServiceState::Ready);
    clone.stop().await.unwrap();
}

#[tokio::test]
async fn idle_stdio_exit_or_unsolicited_response_revokes_without_another_call() {
    let _lock = FIXTURES.lock().await;
    for mode in ["exit_idle", "duplicate"] {
        let fixture = Fixture::new();
        let (_, package) = stdio_package(HookDialect::Native);
        let managed = service(&fixture, package.clone(), stdio(mode, HookDialect::Native));
        fixture
            .run(
                fixture.executor(
                    vec![register(package, managed.clone(), HookDialect::Native)],
                    false,
                ),
                vec![call("result")],
            )
            .await;
        tokio::time::sleep(Duration::from_millis(120)).await;
        assert_ne!(
            managed.state(),
            plugins::services::ServiceState::Ready,
            "{mode} left idle service ready"
        );
        managed.stop().await.unwrap();
    }
}

#[tokio::test]
async fn declared_output_schema_controls_actual_effects_on_both_transports() {
    let _lock = FIXTURES.lock().await;
    for pipe in [false, true] {
        for mode in [
            "output_valid",
            "output_missing",
            "output_invalid",
            "output_text",
        ] {
            let fixture = Fixture::new();
            let mut tool = metadata();
            tool["outputSchema"] = json!({"type":"object","required":["hookSpecificOutput"],"properties":{"hookSpecificOutput":{"type":"object","required":["permissionDecisionReason"],"properties":{"permissionDecisionReason":{"const":"verified"}}}}});
            let discovered = tool.clone();
            let peer = Peer::new(move |_, request| match request["method"].as_str() {
                Some("initialize") => rpc(request, init()),
                Some("tools/list") => rpc(request, json!({"tools":[discovered]})),
                Some("tools/call") => {
                    let mut result = allow_result();
                    if mode == "output_valid" {
                        result["structuredContent"]["hookSpecificOutput"]["permissionDecisionReason"] =
                            json!("verified");
                    }
                    if mode == "output_invalid" {
                        result["structuredContent"]["hookSpecificOutput"]["permissionDecisionReason"] =
                            json!("wrong");
                    }
                    if mode == "output_text" {
                        result = json!({"content":[{"type":"text","text":result["structuredContent"].to_string()}]});
                    }
                    rpc(request, result)
                }
                _ => (202, String::new(), vec![]),
            });
            let (_, package) = stdio_package(HookDialect::Native);
            let mut transport = if pipe {
                stdio(mode, HookDialect::Native)
            } else {
                ServiceTransport::Http(HttpConfig::new(peer.endpoint.clone()))
            };
            if let ServiceTransport::Stdio(config) = &mut transport {
                config
                    .command
                    .environment
                    .insert("MCP_METADATA".into(), tool.to_string());
            }
            let managed = service_with_tools(
                &fixture,
                package.clone(),
                transport,
                vec![AdmittedTool {
                    metadata: tool,
                    read_only: false,
                }],
            );
            let results = fixture
                .run(
                    fixture.executor(
                        vec![register(package, managed.clone(), HookDialect::Native)],
                        false,
                    ),
                    vec![call("schema-result")],
                )
                .await;
            assert_eq!(
                fixture.root.path().join("schema-result").exists(),
                mode == "output_valid",
                "pipe={pipe} mode={mode} {results:?} methods={:?}",
                peer.requests.lock().unwrap()
            );
            if !pipe {
                assert_eq!(
                    peer.methods().iter().filter(|m| *m == "tools/call").count(),
                    1
                );
            }
            managed.stop().await.unwrap();
        }
    }
}
#[tokio::test]
async fn coalesced_stdio_ambiguity_holds_the_first_actual_write() {
    let _lock = FIXTURES.lock().await;
    for mode in [
        "coalesced_duplicate",
        "coalesced_stale",
        "coalesced_malformed",
        "coalesced_partial",
        "coalesced_large",
        "coalesced_notification",
        "allow",
    ] {
        let fixture = Fixture::new();
        let (_, package) = stdio_package(HookDialect::Native);
        let managed = service(&fixture, package.clone(), stdio(mode, HookDialect::Native));
        let results = fixture
            .run(
                fixture.executor(
                    vec![register(package, managed.clone(), HookDialect::Native)],
                    false,
                ),
                vec![call("first-write")],
            )
            .await;
        assert_eq!(
            fixture.root.path().join("first-write").exists(),
            matches!(mode, "allow" | "coalesced_notification"),
            "{mode} {results:?}"
        );
        managed.stop().await.unwrap();
    }
}

#[tokio::test]
async fn discovery_pages_are_bounded_before_any_hook_call_or_write() {
    let _lock = FIXTURES.lock().await;
    for (mode, expected_pages, allow) in [
        ("pagination", 2, true),
        ("cycle", 2, false),
        ("long_cursor", 1, false),
        ("page_limit", 8, false),
        ("tool_limit", 1, false),
        ("duplicate_tool", 1, false),
        ("aggregate", 5, false),
        ("missing_tool", 1, false),
        ("changed_input_schema", 1, false),
    ] {
        let page = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let observed = page.clone();
        let peer = Peer::new(move |_, request| match request["method"].as_str() {
            Some("initialize") => rpc(request, init()),
            Some("tools/list") => {
                let number = observed.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
                let mut result = json!({"tools":[metadata()]});
                match mode {
                    "pagination" if number == 0 => result = json!({"tools":[],"nextCursor":"second"}),
                    "pagination" => assert_eq!(request["params"]["cursor"],"second"),
                    "cycle" => result = json!({"tools":[],"nextCursor":"repeat"}),
                    "long_cursor" => result["nextCursor"] = json!("x".repeat(257)),
                    "page_limit" => result = json!({"tools":[],"nextCursor":format!("page-{number}")}),
                    "tool_limit" => result["tools"] = json!((0..129).map(|n|json!({"name":format!("tool-{n}"),"inputSchema":{"type":"object"}})).collect::<Vec<_>>()),
                    "duplicate_tool" => result["tools"] = json!([metadata(),metadata()]),
                    "aggregate" => result = json!({"tools":[{"name":format!("large-{number}"),"description":"x".repeat(24000),"inputSchema":{"type":"object"}}],"nextCursor":format!("page-{number}")}),
                    "missing_tool" => result["tools"] = json!([]),
                    "changed_input_schema" => result["tools"][0]["inputSchema"] = json!({"type":"object","required":["new"]}),
                    _ => {}
                }
                rpc(request, result)
            }
            Some("tools/call") => rpc(request, allow_result()),
            _ => (202, String::new(), vec![]),
        });
        let fixture = Fixture::new();
        let package = package(HookDialect::Native);
        let managed = service(
            &fixture,
            package.clone(),
            ServiceTransport::Http(HttpConfig::new(peer.endpoint.clone())),
        );
        let results = fixture
            .run(
                fixture.executor(
                    vec![register(package, managed.clone(), HookDialect::Native)],
                    false,
                ),
                vec![call("discovered")],
            )
            .await;
        assert_eq!(
            fixture.root.path().join("discovered").exists(),
            allow,
            "{mode} {results:?}"
        );
        assert_eq!(
            page.load(std::sync::atomic::Ordering::Acquire),
            expected_pages,
            "{mode}"
        );
        assert_eq!(
            peer.methods().iter().filter(|m| *m == "tools/call").count(),
            usize::from(allow),
            "{mode}"
        );
        if !allow {
            pending_hook(&fixture.record());
        }
        managed.stop().await.unwrap();
    }
}
#[tokio::test]
async fn invalid_frozen_schemas_and_unadmitted_tools_never_start_a_service() {
    let _lock = FIXTURES.lock().await;
    for pipe in [false, true] {
        for field in ["inputSchema", "outputSchema"] {
            for invalid in [
                json!({"type":"object","$ref":"http://127.0.0.1:9/schema"}),
                json!({"type":"object","properties":{"field":{"type":"imaginary"}}}),
                json!({"type":"object","description":"x".repeat(29000)}),
            ] {
                let fixture = Fixture::new();
                let (_, package) = stdio_package(HookDialect::Native);
                let peer = Peer::result(true, false, false, HookDialect::Native);
                let transport = if pipe {
                    stdio("allow", HookDialect::Native)
                } else {
                    ServiceTransport::Http(HttpConfig::new(peer.endpoint.clone()))
                };
                let mut invalid = invalid;
                if invalid.get("$ref").is_some() {
                    invalid["$ref"] = json!(peer.endpoint.clone());
                }
                let mut tool = metadata();
                tool[field] = invalid;
                let rejected = ManagedServices::default().admit(
                    package,
                    service_config(
                        &fixture,
                        transport,
                        vec![AdmittedTool {
                            metadata: tool,
                            read_only: false,
                        }],
                    ),
                );
                assert!(rejected.is_err(), "pipe={pipe} accepted invalid {field}");
                assert!(peer.no_requests());
                assert!(!fixture.root.path().join("never").exists());
            }
        }
        let fixture = Fixture::new();
        let (_, package) = stdio_package(HookDialect::Native);
        let peer = Peer::result(true, false, false, HookDialect::Native);
        let transport = if pipe {
            stdio("allow", HookDialect::Native)
        } else {
            ServiceTransport::Http(HttpConfig::new(peer.endpoint.clone()))
        };
        let managed = service(&fixture, package.clone(), transport);
        let rejected = McpRunner::registration(
            package,
            declaration("unknown", HookDialect::Native, HandlerClass::DecisionGate),
            McpBinding {
                service: managed.clone(),
                tool: "not-admitted".into(),
                input: json!({}),
            },
            None,
            McpConfig::default(),
        );
        assert!(rejected.is_err());
        assert_eq!(managed.state(), plugins::services::ServiceState::Admitted);
        assert!(peer.no_requests());
        managed.stop().await.unwrap();
    }
}
#[tokio::test]
async fn admitted_input_schema_rejects_invalid_data_before_bootstrap_on_both_transports() {
    let _lock = FIXTURES.lock().await;
    for pipe in [false, true] {
        for valid in [false, true] {
            let mut tool = metadata();
            tool["inputSchema"] = json!({"type":"object","required":["input"],"properties":{"input":{"type":"object","required":["content"],"properties":{"content":{"const":"written"}}}}});
            let discovered = tool.clone();
            let peer = Peer::new(move |_, request| match request["method"].as_str() {
                Some("initialize") => rpc(request, init()),
                Some("tools/list") => rpc(request, json!({"tools":[discovered]})),
                Some("tools/call") => rpc(request, allow_result()),
                _ => (202, String::new(), vec![]),
            });
            let fixture = Fixture::new();
            let (_, package) = stdio_package(HookDialect::Native);
            let mut transport = if pipe {
                stdio("allow", HookDialect::Native)
            } else {
                ServiceTransport::Http(HttpConfig::new(peer.endpoint.clone()))
            };
            if let ServiceTransport::Stdio(config) = &mut transport {
                config
                    .command
                    .environment
                    .insert("MCP_METADATA".into(), tool.to_string());
            }
            let managed = service_with_tools(
                &fixture,
                package.clone(),
                transport,
                vec![AdmittedTool {
                    metadata: tool,
                    read_only: false,
                }],
            );
            let mut request = call("validated");
            if !valid {
                request.arguments["content"] = json!("invalid");
            }
            let results = fixture
                .run(
                    fixture.executor(
                        vec![register(package, managed.clone(), HookDialect::Native)],
                        false,
                    ),
                    vec![request],
                )
                .await;
            assert_eq!(
                fixture.root.path().join("validated").exists(),
                valid,
                "pipe={pipe} valid={valid} {results:?}"
            );
            if !valid {
                assert_eq!(managed.state(), plugins::services::ServiceState::Admitted);
                assert!(peer.no_requests());
                pending_hook(&fixture.record());
            }
            managed.stop().await.unwrap();
        }
    }
}

#[tokio::test]
async fn changed_candidate_requires_separate_readonly_mcp_revalidation_or_holds() {
    let _lock = FIXTURES.lock().await;
    for mode in ["allow", "deny", "missing", "external"] {
        let mut check = metadata();
        check["name"] = json!("check");
        let advertised = check.clone();
        let peer = Peer::new(move |_, request| match request["method"].as_str() {
            Some("initialize") => rpc(request, init()),
            Some("tools/list") => rpc(request, json!({"tools":[metadata(),advertised]})),
            Some("tools/call") => {
                let mut result = allow_result();
                if request["params"]["name"] == "gate" {
                    result["structuredContent"]["hookSpecificOutput"]["updatedInput"] =
                        json!({"path":"rewritten","content":"written"});
                } else {
                    assert_eq!(request["params"]["arguments"]["input"]["path"], "rewritten");
                    if mode == "deny" {
                        result["structuredContent"]["hookSpecificOutput"]["permissionDecision"] =
                            json!("deny");
                    }
                }
                rpc(request, result)
            }
            _ => (202, String::new(), vec![]),
        });
        let fixture = Fixture::new();
        let package = package(HookDialect::Native);
        let managed = service_with_tools(
            &fixture,
            package.clone(),
            ServiceTransport::Http(HttpConfig::new(peer.endpoint.clone())),
            vec![
                AdmittedTool {
                    metadata: metadata(),
                    read_only: false,
                },
                AdmittedTool {
                    metadata: check,
                    read_only: true,
                },
            ],
        );
        let mut declared = declaration("combined", HookDialect::Native, HandlerClass::Combined);
        let revalidation = if mode == "missing" {
            None
        } else {
            declared.read_only_endpoint = Some("check".into());
            Some(McpBinding {
                service: managed.clone(),
                tool: "check".into(),
                input: json!({"input":"${tool_input}"}),
            })
        };
        if mode == "external" {
            declared.external_precondition = Some("atomic-etag-required".into());
        }
        let registration = McpRunner::registration(
            package,
            declared,
            McpBinding {
                service: managed.clone(),
                tool: "gate".into(),
                input: json!({"input":"${tool_input}"}),
            },
            revalidation,
            McpConfig::default(),
        )
        .unwrap();
        let results = fixture
            .run(
                fixture.executor(vec![registration], false),
                vec![call("original")],
            )
            .await;
        assert!(
            !fixture.root.path().join("original").exists(),
            "{mode} released stale original"
        );
        assert_eq!(
            fixture.root.path().join("rewritten").exists(),
            matches!(mode, "allow" | "coalesced_notification"),
            "{mode} {results:?}"
        );
        let called: Vec<_> = peer
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, r)| r["method"] == "tools/call")
            .map(|(_, r)| r["params"]["name"].clone())
            .collect();
        let expected = match mode {
            "external" => vec![],
            "missing" => vec![json!("gate")],
            _ => vec![json!("gate"), json!("check")],
        };
        assert_eq!(
            called, expected,
            "{mode} repeated primary or released without new decision"
        );
        if mode == "external" {
            assert!(peer.no_requests());
            assert!(hooks(&fixture.record()).is_empty());
        }
        managed.stop().await.unwrap();
    }
}
#[tokio::test]
async fn cancellation_of_a_group_queued_on_one_service_never_sends_the_second_call() {
    let _lock = FIXTURES.lock().await;
    let active = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let finished = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (observed, ended) = (active.clone(), finished.clone());
    let peer = Peer::new(move |_, request| match request["method"].as_str() {
        Some("initialize") => rpc(request, init()),
        Some("tools/list") => rpc(request, json!({"tools":[metadata()]})),
        Some("tools/call") => {
            observed.store(true, std::sync::atomic::Ordering::Release);
            std::thread::sleep(Duration::from_millis(300));
            ended.store(true, std::sync::atomic::Ordering::Release);
            rpc(request, allow_result())
        }
        _ => (202, String::new(), vec![]),
    });
    let fixture = Fixture::new();
    let package = package(HookDialect::Claude);
    let managed = service(
        &fixture,
        package.clone(),
        ServiceTransport::Http(HttpConfig::new(peer.endpoint.clone())),
    );
    let registrations = (0..2)
        .map(|index| {
            let mut declared = declaration(
                &format!("queued-{index}"),
                HookDialect::Claude,
                HandlerClass::Combined,
            );
            declared.identity.index = index;
            McpRunner::registration(
                package.clone(),
                declared,
                McpBinding {
                    service: managed.clone(),
                    tool: "gate".into(),
                    input: json!({"caller":index}),
                },
                None,
                McpConfig::default(),
            )
            .unwrap()
        })
        .collect();
    {
        let run = fixture.run(
            fixture.executor(registrations, false),
            vec![call("cancelled-group")],
        );
        tokio::pin!(run);
        while !active.load(std::sync::atomic::Ordering::Acquire) {
            tokio::select! {_ = &mut run => panic!("group ended before cancellation"),_ = tokio::time::sleep(Duration::from_millis(5)) => {}}
        }
        assert_eq!(
            hooks(&fixture.record()).len(),
            2,
            "both calls must be dispatched/reserved before cancellation"
        );
    }
    tokio::time::timeout(Duration::from_secs(2), async {
        while !finished.load(std::sync::atomic::Ordering::Acquire) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    managed.stop().await.unwrap();
    assert!(!fixture.root.path().join("cancelled-group").exists());
    assert!(fixture.record().recovery_pending);
    assert_eq!(
        peer.methods().iter().filter(|m| *m == "tools/call").count(),
        1
    );
    let before = peer.methods();
    fixture
        .run(fixture.executor(vec![], false), vec![call("next")])
        .await;
    assert!(!fixture.root.path().join("next").exists());
    assert_eq!(peer.methods(), before);
}

#[tokio::test]
async fn complete_service_configuration_controls_handle_reuse_and_admitted_tool_bound() {
    let _lock = FIXTURES.lock().await;
    let fixture = Fixture::new();
    let package = package(HookDialect::Native);
    let peer = Peer::result(true, false, false, HookDialect::Native);
    let baseline = service_config(
        &fixture,
        ServiceTransport::Http(HttpConfig::new(peer.endpoint.clone())),
        vec![AdmittedTool {
            metadata: metadata(),
            read_only: false,
        }],
    );
    for field in [
        "workspace",
        "role",
        "generation",
        "state",
        "credential_revision",
        "endpoint",
        "header",
        "credential",
        "metadata",
        "readonly",
        "timeout",
        "max_calls",
    ] {
        let manager = ManagedServices::default();
        let original = manager.admit(package.clone(), baseline.clone()).unwrap();
        assert!(Arc::ptr_eq(
            &original,
            &manager.admit(package.clone(), baseline.clone()).unwrap()
        ));
        let mut changed = baseline.clone();
        match field {
            "workspace" => changed.identity.workspace.1 += 1,
            "role" => changed.identity.role = "reviewer".into(),
            "generation" => changed.identity.generation = "generation-2".into(),
            "state" => changed.identity.state = "state-2".into(),
            "credential_revision" => changed.identity.credential_revision = "revision-2".into(),
            "endpoint" => {
                if let ServiceTransport::Http(c) = &mut changed.transport {
                    c.endpoint.push_str("&changed=1");
                }
            }
            "header" => {
                if let ServiceTransport::Http(c) = &mut changed.transport {
                    c.headers.insert("x-admitted".into(), "changed".into());
                }
            }
            "credential" => {
                if let ServiceTransport::Http(c) = &mut changed.transport {
                    c.credentials.insert(
                        "Authorization".into(),
                        HttpCredential::bearer("new-secret".into()),
                    );
                }
            }
            "metadata" => {
                changed.tools[0].metadata["description"] = json!("different tool meaning")
            }
            "readonly" => changed.tools[0].read_only = true,
            "timeout" => changed.timeout_ms += 1,
            _ => changed.max_calls += 1,
        }
        let changed = manager.admit(package.clone(), changed).unwrap();
        assert!(
            !Arc::ptr_eq(&original, &changed),
            "{field} reused a differently admitted service"
        );
        original.stop().await.unwrap();
        changed.stop().await.unwrap();
    }
    let mut too_many = baseline.clone();
    too_many.tools = (0..33)
        .map(|n| AdmittedTool {
            metadata: json!({"name":format!("tool-{n}"),"inputSchema":{"type":"object"}}),
            read_only: false,
        })
        .collect();
    assert!(ManagedServices::default().admit(package, too_many).is_err());
    assert!(peer.no_requests());
}
#[tokio::test]
async fn service_call_limit_and_replaced_allowance_cannot_extend_existing_authority() {
    let _lock = FIXTURES.lock().await;
    for replaced_allowance in [false, true] {
        let fixture = Fixture::new();
        let package = package(HookDialect::Native);
        let peer = Peer::result(true, false, false, HookDialect::Native);
        let mut config = service_config(
            &fixture,
            ServiceTransport::Http(HttpConfig::new(peer.endpoint.clone())),
            vec![AdmittedTool {
                metadata: metadata(),
                read_only: false,
            }],
        );
        if !replaced_allowance {
            config.max_calls = 1;
        }
        let managed = ManagedServices::default()
            .admit(package.clone(), config)
            .unwrap();
        let first = fixture
            .run(
                fixture.executor(
                    vec![register(
                        package.clone(),
                        managed.clone(),
                        HookDialect::Native,
                    )],
                    false,
                ),
                vec![call("first")],
            )
            .await;
        assert!(first[0].success);
        let original = fixture.record().allocation.unwrap();
        if replaced_allowance {
            fixture
                .runtime
                .allocate(
                    demoncoder::workflow::allocation::Limits {
                        seconds: 60,
                        model_calls: 8,
                        tool_calls: 64,
                    },
                    None,
                )
                .unwrap();
        }
        fixture
            .run(
                fixture.executor(
                    vec![register(package, managed.clone(), HookDialect::Native)],
                    false,
                ),
                vec![call("second")],
            )
            .await;
        assert!(!fixture.root.path().join("second").exists());
        assert_eq!(
            peer.methods().iter().filter(|m| *m == "initialize").count(),
            1
        );
        assert_eq!(
            peer.methods().iter().filter(|m| *m == "tools/call").count(),
            1
        );
        if !replaced_allowance {
            assert_eq!(
                fixture.record().allocation.unwrap().deadline_ms,
                original.deadline_ms
            );
        }
        managed.stop().await.unwrap();
    }
}
#[tokio::test]
async fn failed_startup_releases_its_slot_after_observed_teardown_and_explicit_reconciliation() {
    let _lock = FIXTURES.lock().await;
    let fixture = Fixture::new();
    let (_, package) = stdio_package(HookDialect::Native);
    let failed = service(
        &fixture,
        package.clone(),
        stdio("version", HookDialect::Native),
    );
    fixture
        .run(
            fixture.executor(
                vec![register(
                    package.clone(),
                    failed.clone(),
                    HookDialect::Native,
                )],
                false,
            ),
            vec![call("failed")],
        )
        .await;
    assert!(!fixture.root.path().join("failed").exists());
    pending_hook(&fixture.record());
    failed.stop().await.unwrap();
    fixture.runtime.reconcile("Test peer unsupported version; exact confined owner stopped and no guarded file exists",None).unwrap();
    let peer = Peer::result(true, false, false, HookDialect::Native);
    let mut services = Vec::new();
    let mut registrations = Vec::new();
    for index in 0..8 {
        let managed = service(
            &fixture,
            package.clone(),
            ServiceTransport::Http(HttpConfig::new(peer.endpoint.clone())),
        );
        let mut declared = declaration(
            &format!("after-failure-{index}"),
            HookDialect::Native,
            HandlerClass::DecisionGate,
        );
        declared.identity.index = index;
        registrations.push(
            McpRunner::registration(
                package.clone(),
                declared,
                McpBinding {
                    service: managed.clone(),
                    tool: "gate".into(),
                    input: json!({}),
                },
                None,
                McpConfig::default(),
            )
            .unwrap(),
        );
        services.push(managed);
    }
    let results = fixture
        .run(
            fixture.executor(registrations, false),
            vec![call("after-reconcile")],
        )
        .await;
    assert!(results[0].success, "{results:?}");
    assert_eq!(
        peer.methods().iter().filter(|m| *m == "initialize").count(),
        8
    );
    assert_eq!(
        peer.methods().iter().filter(|m| *m == "tools/call").count(),
        8
    );
    for managed in services {
        managed.stop().await.unwrap();
    }
}

#[tokio::test]
async fn zero_traffic_observer_detects_a_bodyless_schema_get() {
    let _lock = FIXTURES.lock().await;
    let peer = Peer::result(true, false, false, HookDialect::Native);
    assert!(peer.no_requests(), "untouched peer recorded traffic");
    let response = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .get(&peer.endpoint)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 202);
    {
        let requests = peer.requests.lock().unwrap();
        assert_eq!(requests.len(), 1, "GET positive control did not reach peer");
        assert!(requests[0].0.starts_with("GET "));
        assert!(
            requests[0].1.is_null(),
            "GET control unexpectedly carried a JSON body"
        );
    }
    assert!(
        !peer.no_requests(),
        "zero-traffic observer missed a real bodyless GET"
    );
}
