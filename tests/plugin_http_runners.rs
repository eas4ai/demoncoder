use demoncoder::{
    config::Connection,
    events::EventSink,
    native::{Model, NativeSession},
    plugins::{
        self,
        dispatch::*,
        gate_snapshot::GateReadSet,
        hook_types::{HandlerKind, HookDialect, HookEvent},
        runners::{HttpConfig, HttpCredential, HttpRunner},
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
            runner: HandlerKind::Http,
        },
        class,
        priority: 0,
        matcher: Matcher {
            error_category: None,
            tool: Some("write".into()),
            path: None,
        },
        reads: GateReadSet::default(),
        concurrent_group: (dialect == HookDialect::Claude).then(|| "source-group".into()),
        read_only_endpoint: None,
        external_precondition: None,
    }
}

fn register(
    d: Declaration,
    config: HttpConfig,
    revalidation: Option<HttpConfig>,
) -> anyhow::Result<Registration> {
    let source = tempfile::tempdir().unwrap();
    std::fs::create_dir(source.path().join(".claude-plugin")).unwrap();
    std::fs::write(
        source.path().join(".claude-plugin/plugin.json"),
        r#"{"name":"http-fixture","version":"1.0.0"}"#,
    )
    .unwrap();
    let package =
        Arc::new(plugins::inspect(source.path(), &plugins::ImportOptions::default()).unwrap());
    HttpRunner::registration(package, d, config, revalidation)
}
struct Peer {
    endpoint: String,
    requests: Arc<Mutex<Vec<(String, serde_json::Value)>>>,
    worker: Option<std::thread::JoinHandle<()>>,
    stop: Arc<std::sync::atomic::AtomicBool>,
}
impl Peer {
    fn new(handler: impl Fn(&mut std::net::TcpStream) + Send + 'static) -> Self {
        use std::io::Read;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!(
            "http://{}/hook/a%2Fb?q=x%2Fy&literal=$HOME",
            listener.local_addr().unwrap()
        );
        listener.set_nonblocking(true).unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = requests.clone();
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stopped = stop.clone();
        let worker = std::thread::spawn(move || {
            while !stopped.load(std::sync::atomic::Ordering::SeqCst) {
                let Ok((mut stream, _)) = listener.accept() else {
                    std::thread::sleep(Duration::from_millis(2));
                    continue;
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut bytes = Vec::new();
                let mut buffer = [0; 4096];
                let header = loop {
                    let n = stream.read(&mut buffer).unwrap();
                    assert!(n > 0);
                    bytes.extend_from_slice(&buffer[..n]);
                    if let Some(i) = bytes.windows(4).position(|s| s == b"\r\n\r\n") {
                        break i + 4;
                    }
                    assert!(bytes.len() < 65536);
                };
                let headers = String::from_utf8(bytes[..header].to_vec()).unwrap();
                let length: usize = headers
                    .lines()
                    .find_map(|line| {
                        let (key, value) = line.split_once(':')?;
                        key.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse().unwrap())
                    })
                    .unwrap_or(0);
                while bytes.len() < header + length {
                    let n = stream.read(&mut buffer).unwrap();
                    assert!(n > 0);
                    bytes.extend_from_slice(&buffer[..n]);
                }
                recorded.lock().unwrap().push((
                    headers,
                    if length == 0 {
                        serde_json::Value::Null
                    } else {
                        serde_json::from_slice(&bytes[header..header + length]).unwrap()
                    },
                ));
                handler(&mut stream);
            }
        });
        Self {
            endpoint,
            requests,
            worker: Some(worker),
            stop,
        }
    }
    fn response(status: u16, body: Vec<u8>) -> Self {
        Self::new(move |stream| {
            use std::io::Write;
            let _ = write!(
                stream,
                "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(&body);
        })
    }
    fn count(&self) -> usize {
        self.requests.lock().unwrap().len()
    }
}
impl Drop for Peer {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        self.worker.take().unwrap().join().unwrap();
    }
}
fn verdict(allow: bool) -> Vec<u8> {
    json!({"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":if allow {"allow"} else {"deny"},"permissionDecisionReason":"controlled verdict"}}).to_string().into_bytes()
}
#[tokio::test]
async fn actual_http_source_verdicts_and_literal_event() {
    let _lock = FIXTURES.lock().await;
    for dialect in [HookDialect::Native, HookDialect::Claude] {
        for allow in [false, true] {
            let peer = Peer::response(200, verdict(allow));
            let fixture = Fixture::new();
            let class = if dialect == HookDialect::Claude {
                HandlerClass::Combined
            } else {
                HandlerClass::DecisionGate
            };
            let registration = register(
                declaration("http", dialect, class),
                HttpConfig::new(peer.endpoint.clone()),
                None,
            )
            .unwrap();
            let mut request = call("result");
            request.arguments["content"] =
                json!("literal $HOME $(touch forbidden) `text` $ARGUMENTS");
            let results = fixture
                .run(
                    fixture.executor(vec![registration], false),
                    vec![request.clone()],
                )
                .await;
            assert_eq!(results[0].success, allow, "{results:?}");
            assert_eq!(fixture.root.path().join("result").exists(), allow);
            assert_eq!(peer.count(), 1);
            let requests = peer.requests.lock().unwrap();
            let (headers, event) = &requests[0];
            assert!(headers.starts_with("POST /hook/a%2Fb?q=x%2Fy&literal=$HOME HTTP/1.1"));
            assert_eq!(event["tool_input"], request.arguments);
            assert_eq!(event["tool_name"], "write");
            assert_eq!(event["tool_use_id"], request.id);
            assert_eq!(event["hook_event_name"], "PreToolUse");
            assert_eq!(event["cwd"], fixture.root.path().to_str().unwrap());
            assert!(event["session_id"].is_string());
            if dialect == HookDialect::Claude {
                assert_eq!(event["permission_mode"], "default");
                assert_eq!(event["transcript_path"], "");
            }
        }
    }
}

fn native() -> Declaration {
    declaration("http", HookDialect::Native, HandlerClass::DecisionGate)
}

#[test]
fn package_runner_rejects_foreign_source_even_without_once() {
    let mut d = native();
    d.source = Some(plugins::once::ActivationSource::host_namespace("http-fixture").unwrap());
    let error = register(d, HttpConfig::new("http://127.0.0.1:1/hook".into()), None)
        .err()
        .expect("foreign captured source must be rejected");
    assert!(error.to_string().contains("captured source"), "{error:#}");
}
fn authenticated(peer: &Peer) -> HttpConfig {
    let mut c = HttpConfig::new(peer.endpoint.clone());
    c.credentials.insert(
        "Authorization".into(),
        HttpCredential::bearer("synthetic-secret-token".into()),
    );
    c.headers.insert("X-Literal".into(), "allow".into());
    c
}
fn hooks(record: &Record) -> Vec<&demoncoder::plugins::receipts::HookReceipt> {
    record
        .operations
        .iter()
        .filter_map(|o| o.tool_receipt.as_ref())
        .filter_map(|r| r.plugin_admission.as_ref())
        .flat_map(|a| a.hooks.iter())
        .collect()
}
async fn check(peer: &Peer, config: HttpConfig, allowed: bool) -> Record {
    let fixture = Fixture::new();
    let reg = register(native(), config, None).unwrap();
    let results = fixture
        .run(fixture.executor(vec![reg], false), vec![call("result")])
        .await;
    assert_eq!(results[0].success, allowed, "{results:?}");
    assert_eq!(fixture.root.path().join("result").exists(), allowed);
    assert_eq!(peer.count(), 1);
    fixture.record()
}
#[tokio::test]
async fn explicit_credentials_and_literal_headers_work_without_ambient_lookup() {
    let _lock = FIXTURES.lock().await;
    let peer = Peer::response(200, verdict(true));
    check(&peer, authenticated(&peer), true).await;
    let requests = peer.requests.lock().unwrap();
    let headers = requests[0].0.to_ascii_lowercase();
    assert!(headers.contains("authorization: bearer synthetic-secret-token\r\n"));
    assert!(headers.contains("x-literal: allow\r\n"));
}
#[test]
fn invalid_host_bindings_are_secret_safe_and_never_contact_receivers() {
    let peer = Peer::response(200, verdict(true));
    let mut variants = Vec::new();
    for endpoint in [
        "/relative",
        "file:///tmp/private",
        "http://secret@localhost/",
        "http://@localhost/",
        "http:localhost/",
        "http://localhost/\0",
        "http://localhost/#secret",
        "http://localhost/\n",
    ] {
        let mut c = authenticated(&peer);
        c.endpoint = endpoint.into();
        variants.push(c);
    }
    for name in [
        "Host",
        "Content-Length",
        "Transfer-Encoding",
        "Connection",
        "Content-Type",
        "Proxy-Authorization",
        "bad name",
    ] {
        let mut c = authenticated(&peer);
        c.headers
            .insert(name.into(), "synthetic-secret-token".into());
        variants.push(c);
    }
    for value in ["injected\r\nX-Other: secret", "bad\0value"] {
        let mut c = authenticated(&peer);
        c.headers.insert("X-Bad".into(), value.into());
        variants.push(c);
    }
    let mut c = authenticated(&peer);
    c.timeout_ms = 0;
    variants.push(c);
    let mut c = authenticated(&peer);
    c.max_output_bytes = usize::MAX;
    variants.push(c);
    let mut c = authenticated(&peer);
    c.credentials.get_mut("Authorization").unwrap().secret = String::new();
    variants.push(c);
    for c in variants {
        let error = register(native(), c, None)
            .err()
            .expect("invalid config accepted")
            .to_string();
        assert!(!error.contains("synthetic-secret-token"));
    }
    let mut d = native();
    d.identity.dialect = HookDialect::Codex;
    assert!(register(d, authenticated(&peer), None).is_err());
    assert_eq!(peer.count(), 0);
}
#[tokio::test]
async fn mutated_invocation_identity_and_oversized_input_hold_before_traffic() {
    let _lock = FIXTURES.lock().await;
    for mode in ["identity", "class", "input"] {
        let peer = Peer::response(200, verdict(true));
        let fixture = Fixture::new();
        let mut c = HttpConfig::new(peer.endpoint.clone());
        if mode == "input" {
            c.max_input_bytes = 8;
        }
        let mut reg = register(native(), c, None).unwrap();
        if mode == "identity" {
            reg.declaration.identity.configuration = "tampered".into();
        }
        if mode == "class" {
            reg.declaration.class = HandlerClass::Transformer;
        }
        let results = fixture
            .run(fixture.executor(vec![reg], false), vec![call("result")])
            .await;
        assert!(!results[0].success, "{mode}: {results:?}");
        assert!(!fixture.root.path().join("result").exists());
        assert_eq!(peer.count(), 0);
    }
}
#[tokio::test]
async fn failed_and_secret_reflecting_responses_retain_only_safe_uncertainty() {
    let _lock = FIXTURES.lock().await;
    let escaped = "synthetic-secret-token"
        .chars()
        .map(|c| format!("\\u{:04x}", c as u32))
        .collect::<String>();
    let cases = vec![
        (
            503,
            b"synthetic-secret-token reflected server failure".to_vec(),
        ),
        (200, b"{broken".to_vec()),
        (200, b"{\"continue\":true,\"continue\":false}".to_vec()),
        (200, vec![0xff]),
        (200, b"{\"ok\":true}".to_vec()),
        (
            200,
            b"{\"systemMessage\":\"synthetic-secret-token\"}".to_vec(),
        ),
        (
            200,
            format!("{{\"systemMessage\":\"{escaped}\"}}").into_bytes(),
        ),
        (200, format!("{{\"{escaped}\":true}}").into_bytes()),
    ];
    for (status, body) in cases {
        let peer = Peer::response(status, body);
        let record = check(&peer, authenticated(&peer), false).await;
        assert!(hooks(&record)[0].uncertain_effects);
        let stored = serde_json::to_string(&record).unwrap();
        assert!(!stored.contains("synthetic-secret-token"));
        assert!(!stored.contains(&escaped));
    }
}
#[tokio::test]
async fn redirects_never_reach_location_and_disconnects_never_retry() {
    let _lock = FIXTURES.lock().await;
    for status in [301, 302, 303, 307, 308] {
        let receiver = Peer::response(200, verdict(true));
        let endpoint = receiver.endpoint.clone();
        let peer = Peer::new(move |stream| {
            use std::io::Write;
            let _ = write!(
                stream,
                "HTTP/1.1 {status} Redirect\r\nLocation: {endpoint}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
        });
        let record = check(&peer, authenticated(&peer), false).await;
        assert!(hooks(&record)[0].uncertain_effects);
        assert_eq!(receiver.count(), 0);
    }
    let peer = Peer::new(|_| {});
    let record = check(&peer, authenticated(&peer), false).await;
    assert!(hooks(&record)[0].uncertain_effects);
}
#[tokio::test]
async fn streamed_response_bounds_do_not_trust_content_length() {
    let _lock = FIXTURES.lock().await;
    for mode in ["length", "missing", "chunked", "dishonest"] {
        let peer = Peer::new(move |stream| {
            use std::io::Write;
            let body = vec![b' '; 1024];
            match mode {
                "length" => {
                    let _ = write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Length: 1024\r\nConnection: close\r\n\r\n"
                    );
                }
                "missing" => {
                    let _ = write!(stream, "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n");
                }
                "dishonest" => {
                    let _ = write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Length: 1\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n"
                    );
                }
                _ => {
                    let _ = write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n"
                    );
                }
            }
            if ["chunked", "dishonest"].contains(&mode) {
                let _ = write!(stream, "400\r\n");
                let _ = stream.write_all(&body);
                let _ = write!(stream, "\r\n0\r\n\r\n");
            } else {
                let _ = stream.write_all(&body);
            }
        });
        let mut c = HttpConfig::new(peer.endpoint.clone());
        c.max_output_bytes = 256;
        let record = check(&peer, c, false).await;
        assert!(hooks(&record)[0].uncertain_effects, "{mode}");
    }
}
#[tokio::test]
async fn one_deadline_bounds_slow_headers_and_continuous_body() {
    let _lock = FIXTURES.lock().await;
    for body in [false, true] {
        let peer = Peer::new(move |stream| {
            use std::io::Write;
            if body {
                let _ = write!(stream, "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n");
                for _ in 0..30 {
                    if stream.write_all(b" ").is_err() {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
            } else {
                std::thread::sleep(Duration::from_millis(400));
            }
        });
        let mut c = HttpConfig::new(peer.endpoint.clone());
        c.timeout_ms = 120;
        let started = std::time::Instant::now();
        let record = check(&peer, c, false).await;
        assert!(started.elapsed() < Duration::from_millis(350));
        assert!(hooks(&record)[0].uncertain_effects);
    }
}

#[tokio::test]
async fn cancelled_and_held_owners_close_exchange_with_uncertain_reservation() {
    let _lock = FIXTURES.lock().await;
    for abort in [true, false] {
        let closed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let disconnected = closed.clone();
        let peer = Peer::new(move |stream| {
            use std::io::Read;
            let mut byte = [0];
            let n = stream.read(&mut byte);
            disconnected.store(matches!(n, Ok(0)), std::sync::atomic::Ordering::SeqCst);
        });
        let fixture = Fixture::new();
        let reg = register(native(), HttpConfig::new(peer.endpoint.clone()), None).unwrap();
        let executor = fixture.executor(vec![reg], false);
        let task = tokio::spawn(execute(
            executor,
            vec![call("result")],
            fixture.events.clone(),
        ));
        tokio::time::timeout(Duration::from_secs(2), async {
            while peer.count() == 0 {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        if abort {
            task.abort();
            assert!(task.await.unwrap_err().is_cancelled());
        } else {
            fixture.runtime.hold().unwrap();
            let results = task.await.unwrap();
            assert!(!results[0].success);
        }
        tokio::time::timeout(Duration::from_secs(1), async {
            while !closed.load(std::sync::atomic::Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("HTTP socket did not close on cancellation");
        assert_eq!(peer.count(), 1);
        assert!(!fixture.root.path().join("result").exists());
        let record = fixture.record();
        assert!(hooks(&record)[0].uncertain_effects);
        if abort {
            assert!(hooks(&record)[0].outcome.is_none());
        }
    }
}
#[tokio::test]
async fn delayed_approval_holds_when_retained_workspace_changes() {
    let _lock = FIXTURES.lock().await;
    for change in [false, true] {
        let fixture = Fixture::new();
        let public = fixture.root.path().join("public");
        std::fs::write(&public, "original").unwrap();
        let peer = Peer::new(move |stream| {
            use std::io::Write;
            if change {
                std::fs::write(&public, "external change").unwrap();
            }
            let body = verdict(true);
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(&body);
        });
        let reg = register(native(), HttpConfig::new(peer.endpoint.clone()), None).unwrap();
        let results = fixture
            .run(fixture.executor(vec![reg], false), vec![call("result")])
            .await;
        assert_eq!(results[0].success, !change, "{results:?}");
        assert_eq!(fixture.root.path().join("result").exists(), !change);
        assert_eq!(peer.count(), 1);
    }
}
#[tokio::test]
async fn ambient_canaries_and_proxy_are_not_inherited() {
    const CHILD: &str = "DEMONCODER_HTTP_FIXTURE_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "ambient_canaries_and_proxy_are_not_inherited",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .env("ANTHROPIC_API_KEY", "ambient-canary-do-not-send")
            .env("OPENAI_API_KEY", "ambient-canary-do-not-send")
            .env("HTTP_PROXY", "http://127.0.0.1:1")
            .env("ALL_PROXY", "http://127.0.0.1:1")
            .env("http_proxy", "http://127.0.0.1:1")
            .env("NO_PROXY", "")
            .env("no_proxy", "")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        return;
    }
    let peer = Peer::response(200, verdict(true));
    check(&peer, HttpConfig::new(peer.endpoint.clone()), true).await;
    let request = &peer.requests.lock().unwrap()[0];
    assert!(!request.0.to_ascii_lowercase().contains("authorization:"));
    assert!(!request.0.contains("ambient-canary-do-not-send"));
    assert!(!request.1.to_string().contains("ambient-canary-do-not-send"));
}

#[tokio::test]
async fn combined_rewrite_uses_separate_revalidation_without_replaying_primary() {
    let _lock = FIXTURES.lock().await;
    for allow in [false, true] {
        let primary=Peer::response(200,json!({"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow","updatedInput":{"path":"rewritten","content":"written"}}}).to_string().into_bytes());
        let mut read_only = Peer::response(200, verdict(allow));
        read_only
            .endpoint
            .push_str("&private=endpoint-query-canary");
        let fixture = Fixture::new();
        let mut d = declaration("combined", HookDialect::Claude, HandlerClass::Combined);
        d.read_only_endpoint = Some("read-only-policy".into());
        let reg = register(
            d,
            HttpConfig::new(primary.endpoint.clone()),
            Some(HttpConfig::new(read_only.endpoint.clone())),
        )
        .unwrap();
        let results = fixture
            .run(fixture.executor(vec![reg], false), vec![call("original")])
            .await;
        assert_eq!(results[0].success, allow, "{results:?}");
        assert!(!fixture.root.path().join("original").exists());
        assert_eq!(fixture.root.path().join("rewritten").exists(), allow);
        assert_eq!(primary.count(), 1);
        assert_eq!(read_only.count(), 1);
        assert_eq!(
            read_only.requests.lock().unwrap()[0].1["tool_input"]["path"],
            "rewritten"
        );
        let record = fixture.record();
        let receipts = hooks(&record);
        assert_eq!(receipts.len(), 2);
        assert!(
            !serde_json::to_string(&record)
                .unwrap()
                .contains("endpoint-query-canary")
        );
        assert_eq!(receipts[1].endpoint.as_deref(), Some("read-only-policy"));
    }
}
#[tokio::test]
async fn revalidation_identity_cannot_switch_destination() {
    let _lock = FIXTURES.lock().await;
    let primary=Peer::response(200,json!({"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow","updatedInput":{"path":"rewritten","content":"written"}}}).to_string().into_bytes());
    let read_only = Peer::response(200, verdict(true));
    let fixture = Fixture::new();
    let mut d = declaration("combined", HookDialect::Claude, HandlerClass::Combined);
    d.read_only_endpoint = Some("read-only-policy".into());
    assert!(
        register(
            d.clone(),
            HttpConfig::new(primary.endpoint.clone()),
            Some(HttpConfig::new(primary.endpoint.clone()))
        )
        .is_err()
    );
    d.read_only_endpoint = Some("read-only-policy".into());
    let mut reg = register(
        d,
        HttpConfig::new(primary.endpoint.clone()),
        Some(HttpConfig::new(read_only.endpoint.clone())),
    )
    .unwrap();
    reg.declaration.read_only_endpoint = Some("tampered-policy".into());
    let results = fixture
        .run(fixture.executor(vec![reg], false), vec![call("original")])
        .await;
    assert!(!results[0].success);
    assert_eq!(primary.count(), 1);
    assert_eq!(read_only.count(), 0);
    assert!(!fixture.root.path().join("rewritten").exists());
}

#[tokio::test]
async fn credential_reflections_include_nonstring_json_tokens() {
    let _lock = FIXTURES.lock().await;
    for secret in ["9182736450192837465", "true", "null"] {
        let body=format!("{{\"hookSpecificOutput\":{{\"hookEventName\":\"PreToolUse\",\"permissionDecision\":\"allow\",\"updatedInput\":{{\"path\":\"result\",\"content\":{secret}}}}}}}").into_bytes();
        let peer = Peer::response(200, body);
        let mut config = authenticated(&peer);
        config.credentials.get_mut("Authorization").unwrap().secret = secret.into();
        let record = check(&peer, config, false).await;
        assert!(
            hooks(&record)[0].uncertain_effects,
            "nonstring secret {secret} reached raw receipt"
        );
        assert!(matches!(
            hooks(&record)[0].outcome,
            Some(RawOutcome::Failure { .. })
        ));
    }
}
#[test]
fn whitespace_only_credentials_are_invalid() {
    let peer = Peer::response(200, verdict(true));
    let mut c = authenticated(&peer);
    c.credentials.get_mut("Authorization").unwrap().secret = "  ".into();
    assert!(register(native(), c, None).is_err());
}

#[tokio::test]
async fn owning_allowance_and_external_atomic_preconditions_precede_network() {
    let _lock = FIXTURES.lock().await;
    for missing_allowance in [false, true] {
        let peer = Peer::response(200, verdict(true));
        let fixture = Fixture::with_allowance(tempfile::tempdir().unwrap(), !missing_allowance);
        let mut d = native();
        if !missing_allowance {
            d.external_precondition = Some("requires-atomic-transaction".into());
        }
        let reg = register(d, HttpConfig::new(peer.endpoint.clone()), None).unwrap();
        let results = fixture
            .run(fixture.executor(vec![reg], false), vec![call("result")])
            .await;
        assert!(!results[0].success, "{results:?}");
        assert_eq!(peer.count(), 0);
        assert!(!fixture.root.path().join("result").exists());
    }
}
#[tokio::test]
async fn developer_question_remains_held_and_deny_wins_over_allow() {
    let _lock = FIXTURES.lock().await;
    for ask in [false, true] {
        let body = if ask {
            json!({"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"ask","permissionDecisionReason":"developer must decide"}}).to_string().into_bytes()
        } else {
            verdict(false)
        };
        let first = Peer::response(200, body);
        let second = Peer::response(200, verdict(true));
        let fixture = Fixture::new();
        let a = register(native(), HttpConfig::new(first.endpoint.clone()), None).unwrap();
        let mut d = native();
        d.identity.declaration = "second".into();
        d.identity.index = 1;
        let b = register(d, HttpConfig::new(second.endpoint.clone()), None).unwrap();
        let results = fixture
            .run(fixture.executor(vec![a, b], false), vec![call("result")])
            .await;
        assert!(!results[0].success);
        assert!(!fixture.root.path().join("result").exists());
        assert_eq!(first.count(), 1);
        assert_eq!(second.count(), 1);
        let record = fixture.record();
        assert!(!hooks(&record)[0].questions.is_empty());
        assert!(!hooks(&record)[0].uncertain_effects);
        assert_eq!(
            record.allocation.unwrap().model_calls,
            2,
            "HTTP must not allocate model calls"
        );
    }
}

#[tokio::test]
async fn owning_deadline_is_stricter_than_endpoint_timeout() {
    let _lock = FIXTURES.lock().await;
    let peer = Peer::new(|_| std::thread::sleep(Duration::from_secs(2)));
    let fixture = Fixture::with_allowance(tempfile::tempdir().unwrap(), false);
    fixture
        .runtime
        .allocate(
            demoncoder::workflow::allocation::Limits {
                seconds: 4,
                model_calls: 8,
                tool_calls: 64,
            },
            None,
        )
        .unwrap();
    let reg = register(native(), HttpConfig::new(peer.endpoint.clone()), None).unwrap();
    let started = std::time::Instant::now();
    let results = fixture
        .run(fixture.executor(vec![reg], false), vec![call("result")])
        .await;
    assert!(!results[0].success);
    assert!(started.elapsed() < Duration::from_millis(1500));
    assert_eq!(peer.count(), 1);
    assert!(hooks(&fixture.record())[0].uncertain_effects);
}
#[tokio::test]
async fn uncertain_post_is_not_replayed_by_followup_turn() {
    let _lock = FIXTURES.lock().await;
    let peer = Peer::new(|_| {});
    let fixture = Fixture::new();
    for _ in 0..2 {
        let reg = register(native(), HttpConfig::new(peer.endpoint.clone()), None).unwrap();
        let results = fixture
            .run(fixture.executor(vec![reg], false), vec![call("result")])
            .await;
        assert!(results.iter().all(|r| !r.success));
        assert_eq!(peer.count(), 1);
        assert!(!fixture.root.path().join("result").exists());
    }
}

#[tokio::test]
async fn both_endpoint_responses_protect_all_credentials_in_the_binding() {
    let _lock = FIXTURES.lock().await;
    for reflect_primary in [true, false] {
        let mut primary_body = json!({"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow","updatedInput":{"path":"rewritten","content":"written"}}});
        if !reflect_primary {
            primary_body["systemMessage"] = json!("other-endpoint-secret-token");
        }
        let primary = Peer::response(200, primary_body.to_string().into_bytes());
        let read_only = Peer::response(
            200,
            json!({"systemMessage":"other-endpoint-secret-token"})
                .to_string()
                .into_bytes(),
        );
        let fixture = Fixture::new();
        let mut d = declaration("combined", HookDialect::Claude, HandlerClass::Combined);
        d.read_only_endpoint = Some("read-only".into());
        let mut primary_config = HttpConfig::new(primary.endpoint.clone());
        let mut read_config = HttpConfig::new(read_only.endpoint.clone());
        let sensitive = if reflect_primary {
            &mut primary_config
        } else {
            &mut read_config
        };
        sensitive.credentials.insert(
            "Authorization".into(),
            HttpCredential::bearer("other-endpoint-secret-token".into()),
        );
        let reg = register(d, primary_config, Some(read_config)).unwrap();
        let results = fixture
            .run(fixture.executor(vec![reg], false), vec![call("original")])
            .await;
        assert!(!results[0].success);
        assert_eq!(primary.count(), 1);
        assert_eq!(read_only.count(), usize::from(reflect_primary));
        let record = fixture.record();
        let receipts = hooks(&record);
        assert!(
            receipts.last().unwrap().uncertain_effects,
            "other endpoint secret reached ordinary receipt"
        );
        assert!(matches!(
            receipts.last().unwrap().outcome,
            Some(RawOutcome::Failure { .. })
        ));
        let uncredentialed = if reflect_primary {
            &read_only
        } else {
            &primary
        };
        assert!(
            !uncredentialed.requests.lock().unwrap()[0]
                .0
                .to_ascii_lowercase()
                .contains("authorization:")
        );
    }
}

#[test]
fn invalid_imports_and_different_runner_kinds_cannot_bind_http_authority() {
    let peer = Peer::response(200, verdict(true));
    let source = tempfile::tempdir().unwrap();
    std::fs::create_dir(source.path().join(".claude-plugin")).unwrap();
    std::fs::write(
        source.path().join(".claude-plugin/plugin.json"),
        r#"{"name":"invalid-http","hooks":42}"#,
    )
    .unwrap();
    let package =
        Arc::new(plugins::inspect(source.path(), &plugins::ImportOptions::default()).unwrap());
    assert_eq!(package.source_validity(), plugins::SourceValidity::Invalid);
    assert!(
        HttpRunner::registration(
            package,
            native(),
            HttpConfig::new(peer.endpoint.clone()),
            None
        )
        .is_err()
    );
    let mut d = native();
    d.identity.runner = HandlerKind::Command;
    assert!(register(d, HttpConfig::new(peer.endpoint.clone()), None).is_err());
    assert_eq!(peer.count(), 0);
}

#[tokio::test]
async fn unicode_whitespace_preserves_source_empty_response_semantics() {
    let _lock = FIXTURES.lock().await;
    for whitespace in [" \r\n", "\u{2003}"] {
        let peer = Peer::response(200, whitespace.as_bytes().to_vec());
        let fixture = Fixture::new();
        let reg = register(native(), HttpConfig::new(peer.endpoint.clone()), None).unwrap();
        fixture
            .run(fixture.executor(vec![reg], false), vec![call("result")])
            .await;
        let record = fixture.record();
        assert!(
            !hooks(&record)[0].uncertain_effects,
            "valid source-empty response became an invalid transport"
        );
        assert!(matches!(
            hooks(&record)[0].outcome,
            Some(RawOutcome::Http { .. })
        ));
    }
}

#[test]
fn empty_raw_authority_rejected_for_primary_endpoint() {
    let peer = Peer::response(200, verdict(true));
    assert!(register(native(), HttpConfig::new(peer.endpoint.clone()), None).is_ok());
    let destination = peer.endpoint.strip_prefix("http://").unwrap();
    for scheme in ["http", "https"] {
        for slashes in ["///", "////", "/////"] {
            let mut config = authenticated(&peer);
            config.endpoint =
                format!("{scheme}:{slashes}{destination}&private=authority-query-canary");
            let error = register(native(), config, None)
                .err()
                .expect("empty raw primary authority was accepted");
            assert!(!error.to_string().contains("authority-query-canary"));
            assert!(!error.to_string().contains("synthetic-secret-token"));
        }
    }
    assert_eq!(peer.count(), 0);
}

#[test]
fn empty_raw_authority_rejected_for_revalidation_endpoint() {
    let primary = Peer::response(200, verdict(true));
    let read_only = Peer::response(200, verdict(true));
    let mut d = declaration("combined", HookDialect::Claude, HandlerClass::Combined);
    d.read_only_endpoint = Some("read-only".into());
    let primary_config = HttpConfig::new(primary.endpoint.clone());
    assert!(
        register(
            d.clone(),
            primary_config.clone(),
            Some(HttpConfig::new(read_only.endpoint.clone()))
        )
        .is_ok()
    );
    let destination = read_only.endpoint.strip_prefix("http://").unwrap();
    for scheme in ["http", "https"] {
        for slashes in ["///", "////", "/////"] {
            let mut config = authenticated(&read_only);
            config.endpoint =
                format!("{scheme}:{slashes}{destination}&private=authority-query-canary");
            let error = register(d.clone(), primary_config.clone(), Some(config))
                .err()
                .expect("empty raw revalidation authority was accepted");
            assert!(!error.to_string().contains("authority-query-canary"));
            assert!(!error.to_string().contains("synthetic-secret-token"));
        }
    }
    assert_eq!(primary.count(), 0);
    assert_eq!(read_only.count(), 0);
}

#[tokio::test]
async fn native_non_tool_http_frames_actual_submit_and_rejects_missing_owner_before_io() {
    let _lock = FIXTURES.lock().await;
    for allowance in [false, true] {
        let peer = Peer::response(
            200,
            json!({"decision":"block","reason":"non-tool HTTP denied"})
                .to_string()
                .into_bytes(),
        );
        let fixture = Fixture::with_allowance(tempfile::tempdir().unwrap(), allowance);
        fixture.runtime.begin_phase("worker", Some("test")).unwrap();
        let source = tempfile::tempdir().unwrap();
        std::fs::create_dir(source.path().join(".claude-plugin")).unwrap();
        std::fs::write(
            source.path().join(".claude-plugin/plugin.json"),
            r#"{"name":"http-fixture","version":"1.0.0"}"#,
        )
        .unwrap();
        let package =
            Arc::new(plugins::inspect(source.path(), &plugins::ImportOptions::default()).unwrap());
        let mut d = declaration("http", HookDialect::Native, HandlerClass::DecisionGate);
        d.matcher = Matcher::default();
        let registration = HttpRunner::registration_for_event(
            package,
            d,
            HookEvent::UserPromptSubmit,
            HttpConfig::new(peer.endpoint.clone()),
            None,
        )
        .unwrap();
        let mut tools = fixture.executor(vec![], false);
        tools
            .register_non_tool_plan(Arc::new(
                plugins::non_tool::NonToolPlan::new(
                    HookEvent::UserPromptSubmit,
                    vec![registration],
                )
                .unwrap(),
            ))
            .unwrap();
        assert!(fixture.run(tools, vec![call("result")]).await.is_empty());
        assert!(!fixture.root.path().join("result").exists());
        assert_eq!(peer.count(), usize::from(allowance));
        if allowance {
            let requests = peer.requests.lock().unwrap();
            assert_eq!(requests[0].1["hook_event_name"], "UserPromptSubmit");
            assert_eq!(requests[0].1["prompt"], "test");
            assert!(requests[0].1.get("tool_name").is_none());
            assert!(requests[0].1.get("tool_input").is_none());
        }
        let record = fixture.record();
        let receipt = record
            .operations
            .iter()
            .find_map(|op| match &op.host_invocation {
                Some(demoncoder::workflow::runtime::HostInvocation::Lifecycle(receipt)) => {
                    Some(receipt)
                }
                _ => None,
            })
            .unwrap();
        if allowance {
            assert!(receipt.settled && receipt.hold.is_some());
        } else {
            assert!(record.recovery_pending && !receipt.settled);
            assert!(
                serde_json::to_string(&receipt.hooks[0].outcome)
                    .unwrap()
                    .contains("owning allowance")
            );
        }
    }
}

struct FailedNative;
#[async_trait::async_trait]
impl Model for FailedNative {
    fn prompt(&mut self, _: String) {}
    fn results(&mut self, _: Vec<ToolResult>) {
        panic!("no tools")
    }
    async fn response(&mut self, _: &EventSink) -> anyhow::Result<Vec<ToolCall>> {
        Err(demoncoder::native::provider_response_failure(
            anyhow::anyhow!("actual provider error"),
        ))
    }
}
#[tokio::test]
async fn native_stop_failure_http_observes_without_replacing_failure() {
    let _lock = FIXTURES.lock().await;
    let peer = Peer::response(
        200,
        json!({"decision":"block","reason":"cannot retry"})
            .to_string()
            .into_bytes(),
    );
    let fixture = Fixture::with_allowance(tempfile::tempdir().unwrap(), true);
    fixture
        .runtime
        .begin_phase("worker", Some("original"))
        .unwrap();
    let source = tempfile::tempdir().unwrap();
    std::fs::create_dir(source.path().join(".claude-plugin")).unwrap();
    std::fs::write(
        source.path().join(".claude-plugin/plugin.json"),
        r#"{"name":"http-failure","version":"1.0.0"}"#,
    )
    .unwrap();
    let package =
        Arc::new(plugins::inspect(source.path(), &plugins::ImportOptions::default()).unwrap());
    let mut d = declaration("http", HookDialect::Native, HandlerClass::Observer);
    d.matcher = Matcher::default();
    let registration = HttpRunner::registration_for_event(
        package,
        d,
        HookEvent::StopFailure,
        HttpConfig::new(peer.endpoint.clone()),
        None,
    )
    .unwrap();
    let mut tools = fixture.executor(vec![], false);
    tools
        .register_non_tool_plan(Arc::new(
            plugins::non_tool::NonToolPlan::new(HookEvent::StopFailure, vec![registration])
                .unwrap(),
        ))
        .unwrap();
    let mut session = NativeSession::with_tools(Box::new(FailedNative), tools);
    let (_sender, mut commands) = mpsc::channel(4);
    assert_eq!(
        session
            .turn("original".into(), &mut commands, &fixture.events)
            .await
            .err()
            .unwrap()
            .to_string(),
        "actual provider error"
    );
    assert_eq!(peer.count(), 1);
    let requests = peer.requests.lock().unwrap();
    let input = &requests[0].1;
    assert_eq!(input["hook_event_name"], "StopFailure");
    assert_eq!(input["error"], "unknown");
    assert!(input.get("last_assistant_message").is_none());
    let record = fixture.record();
    assert_eq!(record.allocation.unwrap().model_calls, 1);
}

struct ProviderOriginObserver(Arc<std::sync::atomic::AtomicUsize>);
#[async_trait::async_trait]
impl HookRunner for ProviderOriginObserver {
    fn side_effect_free(&self) -> bool {
        true
    }
    async fn run(&self, _: &HookInvocation) -> anyhow::Result<RawOutcome> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(RawOutcome::Callback { value: json!({}) })
    }
}

async fn actual_provider_origin_case(
    adapter: &str,
    peer: &Peer,
    metadata: bool,
    close_after_request: Arc<Mutex<Option<mpsc::Receiver<demoncoder::events::Envelope>>>>,
    close_sink: bool,
    persistence_fault: Option<Arc<Mutex<Option<std::path::PathBuf>>>>,
) -> (String, usize) {
    let root = tempfile::tempdir().unwrap();
    let endpoint = peer.endpoint.split("/hook/").next().unwrap().to_owned() + "/v1/messages";
    let mut connection: Connection = serde_json::from_value(json!({
        "adapter":adapter,"model":"fixture","api_key":"fixture-key","endpoint":endpoint,
        "max_output_tokens":if metadata {None} else {Some(128)}
    }))
    .unwrap();
    let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut d = declaration("origin", HookDialect::Native, HandlerClass::Observer);
    d.matcher = Matcher::default();
    connection.access.non_tools.push(Arc::new(
        plugins::non_tool::NonToolPlan::new(
            HookEvent::StopFailure,
            vec![Registration {
                declaration: d,
                runner: Arc::new(ProviderOriginObserver(count.clone())),
                revalidation: None,
            }],
        )
        .unwrap(),
    ));
    let (runtime, _) = SharedRuntime::open(root.path(), &connection, None).unwrap();
    runtime
        .allocate(demoncoder::workflow::allocation::Limits::default(), None)
        .unwrap();
    runtime.begin_phase("worker", Some("original")).unwrap();
    let mut session = demoncoder::adapters::builtins()
        .unwrap()
        .open(&connection, root.path())
        .unwrap();
    let (tx, rx) = mpsc::channel(256);
    *close_after_request.lock().unwrap() = Some(rx);
    let events = EventSink::new("origin".into(), tx, None)
        .unwrap()
        .with_runtime(runtime.clone());
    if let Some(path) = &persistence_fault {
        *path.lock().unwrap() = Some(runtime.directory().unwrap().join("state.json"));
    }
    let (_sender, mut commands) = mpsc::channel(4);
    let error = tokio::time::timeout(
        Duration::from_secs(10),
        session.turn("original".into(), &mut commands, &events),
    )
    .await
    .unwrap()
    .err()
    .unwrap();
    if close_sink {
        assert!(error.to_string().contains("closed"), "{error:#}");
    }
    if persistence_fault.is_none() {
        let record = runtime.record().unwrap();
        assert_eq!(record.allocation.unwrap().model_calls, 1);
    }
    let result = (
        format!("{error:#}"),
        count.load(std::sync::atomic::Ordering::SeqCst),
    );
    std::fs::remove_dir_all(runtime.directory().unwrap()).unwrap();
    result
}

#[tokio::test]
async fn native_stop_failure_actual_adapter_transport_protocol_and_metadata_origins() {
    let _lock = FIXTURES.lock().await;
    for adapter in ["openai-api", "anthropic-api"] {
        let transport = Peer::new(|_| {});
        let (error, count) = actual_provider_origin_case(
            adapter,
            &transport,
            false,
            Arc::new(Mutex::new(None)),
            false,
            None,
        )
        .await;
        assert!(error.contains("provider request failed"), "{error}");
        assert_eq!(transport.count(), 1);
        assert_eq!(count, 1);
        for (status, body, expected) in [
            (503, b"provider refused".to_vec(), "HTTP 503"),
            (
                200,
                b"data: not-json\n\n".to_vec(),
                "invalid provider stream event",
            ),
            (
                200,
                format!("data: {}\n\n", json!({"type":"error"})).into_bytes(),
                if adapter == "openai-api" {
                    "did not complete"
                } else {
                    "stream error"
                },
            ),
        ] {
            let peer = Peer::response(status, body);
            let (error, count) = actual_provider_origin_case(
                adapter,
                &peer,
                false,
                Arc::new(Mutex::new(None)),
                false,
                None,
            )
            .await;
            assert!(error.contains(expected), "{error}");
            assert_eq!(peer.count(), 1);
            assert_eq!(count, 1, "{adapter}: {error}");
        }
    }
    for (status, body, expected) in [
        (503, b"no metadata".to_vec(), "HTTP 503"),
        (200, b"invalid".to_vec(), "not valid JSON"),
        (200, b"{}".to_vec(), "positive max_tokens"),
    ] {
        let peer = Peer::response(status, body);
        let (error, count) = actual_provider_origin_case(
            "anthropic-api",
            &peer,
            true,
            Arc::new(Mutex::new(None)),
            false,
            None,
        )
        .await;
        assert!(
            error.contains("Set max_output_tokens") && error.contains(expected),
            "{error}"
        );
        assert_eq!(peer.count(), 1);
        assert!(
            peer.requests.lock().unwrap()[0]
                .0
                .starts_with("GET /v1/models/fixture ")
        );
        assert_eq!(count, 1);
    }
}

#[tokio::test]
async fn native_stop_failure_actual_adapter_post_response_sink_failure_is_local() {
    let _lock = FIXTURES.lock().await;
    for adapter in ["openai-api", "anthropic-api"] {
        let receiver = Arc::new(Mutex::new(None));
        let close = receiver.clone();
        let frame = if adapter == "openai-api" {
            json!({"type":"response.completed","response":{"output":[],"usage":{}}})
        } else {
            json!({"type":"message_stop"})
        };
        let body = format!("data: {frame}\n\n");
        let peer = Peer::new(move |stream| {
            use std::io::Write;
            drop(close.lock().unwrap().take());
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });
        let (_, count) =
            actual_provider_origin_case(adapter, &peer, false, receiver, true, None).await;
        assert_eq!(peer.count(), 1);
        assert_eq!(
            count, 0,
            "{adapter}: successful provider response followed by local delivery failure"
        );
    }
}

#[tokio::test]
async fn native_stop_failure_actual_adapter_post_response_persistence_failure_is_local() {
    let _lock = FIXTURES.lock().await;
    for adapter in ["openai-api", "anthropic-api"] {
        let path: Arc<Mutex<Option<std::path::PathBuf>>> = Arc::new(Mutex::new(None));
        let fault = path.clone();
        let frame = if adapter == "openai-api" {
            json!({"type":"response.completed","response":{"output":[],"usage":{}}})
        } else {
            json!({"type":"message_stop"})
        };
        let body = format!("data: {frame}\n\n");
        let peer = Peer::new(move |stream| {
            use std::io::Write;
            let path = fault.lock().unwrap().take().unwrap();
            std::fs::remove_file(&path).unwrap();
            std::fs::create_dir(&path).unwrap();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });
        let (error, count) = actual_provider_origin_case(
            adapter,
            &peer,
            false,
            Arc::new(Mutex::new(None)),
            false,
            Some(path),
        )
        .await;
        assert!(error.contains("session persistence failed"), "{error}");
        assert_eq!(peer.count(), 1);
        assert_eq!(count, 0, "{adapter}: {error}");
    }
}
