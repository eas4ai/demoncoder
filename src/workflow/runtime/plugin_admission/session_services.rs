use super::*;
use crate::{
    events::EventSink,
    plugins::{
        dispatch::{Declaration, HookInvocation, Matcher},
        gate_snapshot::{GateReadSet, GateWorkspace},
        hook_types::{HandlerKind, HookDialect},
        runners::{HttpConfig, HttpRunner, McpBinding, McpConfig, McpRunner},
        services::{
            AdmittedTool, ManagedService, ManagedServices, ServiceConfig, ServiceIdentity,
            ServiceTransport,
        },
    },
    session::SessionStart,
    workflow::{
        allocation::Limits,
        runtime::{HostInvocation, session_budget::SessionHookAllowance},
    },
};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

struct Fixture {
    root: tempfile::TempDir,
    _state: tempfile::TempDir,
    runtime: SharedRuntime,
    events: EventSink,
    _receiver: tokio::sync::mpsc::Receiver<crate::events::Envelope>,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let mut record = crate::inspection::tests::record(root.path());
        record.session_hook_allowance = Some(
            SessionHookAllowance::new(Limits {
                seconds: 60,
                model_calls: 1,
                tool_calls: 1,
            })
            .unwrap(),
        );
        let runtime = SharedRuntime::for_test(&state.path().join("record"), record).unwrap();
        let (sender, receiver) = tokio::sync::mpsc::channel(64);
        let events = EventSink::new("session service test".into(), sender, None)
            .unwrap()
            .with_runtime(runtime.clone());
        let events = events
            .begin_host_lifetime(
                SessionStart::Startup,
                vec![(HookEvent::SessionStart, "plan".into())],
            )
            .unwrap()
            .unwrap();
        Self {
            root,
            _state: state,
            runtime,
            events,
            _receiver: receiver,
        }
    }
    fn invocation(&self, declaration: &Declaration) -> HookInvocation {
        let (events, facts) = self
            .events
            .for_non_tool(
                NonToolOccurrence::SessionStart {
                    source: SessionStart::Startup,
                },
                "plan".into(),
                vec![serde_json::to_value(declaration).unwrap()],
            )
            .unwrap();
        let snapshot = Arc::new(
            GateWorkspace::open(self.root.path())
                .unwrap()
                .capture(&GateReadSet::default(), &AtomicBool::new(false))
                .unwrap(),
        );
        let key = AdmissionKey {
            session: facts.session.clone(),
            operation: facts.operation,
            source_operation: facts.causal_operation(),
            event: "SessionStart".into(),
            tool: None,
            arguments: None,
            lifecycle: Some(facts.subject.clone()),
            plan: "plan".into(),
            role: "native-session".into(),
            workspace: facts.workspace,
            inputs: vec![],
            external: None,
        };
        let hook = HookReceipt {
            required_gate: false,
            observer: None,
            source: declaration.source.as_ref().map(|s| s.0.clone()),
            once: None,
            invocation: 0,
            declaration: declaration.identity.clone(),
            class: HandlerClass::Observer,
            endpoint: None,
            inspected: key.clone(),
            outcome: None,
            uncertain_effects: true,
            hold: None,
            questions: vec![],
            pending_proposals: vec![],
        };
        let lease = Arc::new(
            Arc::new(tokio::sync::Semaphore::new(1))
                .try_acquire_owned()
                .unwrap(),
        );
        self.runtime
            .reserve_non_tool_hook(
                facts.operation,
                HookEvent::SessionStart,
                hook,
                None,
                Some(&lease),
            )
            .unwrap();
        HookInvocation {
            required_gate: false,
            observer: None,
            invocation: 0,
            key,
            declaration: declaration.identity.clone(),
            endpoint: None,
            candidate: None,
            lifecycle: Some(facts),
            snapshot,
            completed: None,
            events,
            host: crate::tools::ToolExecutor::new(self.root.path())
                .unwrap()
                .hook_host(),
            runner_lease: lease,
            mutation_guard: None,
            class: HandlerClass::Observer,
        }
    }
}
fn package() -> Arc<crate::plugins::Package> {
    let source = tempfile::tempdir().unwrap();
    std::fs::create_dir(source.path().join(".claude-plugin")).unwrap();
    std::fs::write(
        source.path().join(".claude-plugin/plugin.json"),
        r#"{"name":"service-owner","version":"1.0.0"}"#,
    )
    .unwrap();
    Arc::new(crate::plugins::inspect(source.path(), &Default::default()).unwrap())
}
fn declaration(kind: HandlerKind) -> Declaration {
    Declaration {
        required_gate: false,
        source: None,
        once: None,
        identity: DeclarationIdentity {
            package: "fixture".into(),
            code: "code".into(),
            policy: "policy".into(),
            configuration: "config".into(),
            generation: "1".into(),
            scope: Scope::Project,
            role: "worker".into(),
            declaration: "session".into(),
            index: 0,
            dialect: HookDialect::Native,
            runner: kind,
        },
        class: HandlerClass::Observer,
        priority: 0,
        matcher: Matcher::default(),
        reads: GateReadSet::default(),
        concurrent_group: None,
        read_only_endpoint: None,
        external_precondition: None,
    }
}
struct Peer {
    endpoint: String,
    calls: Arc<AtomicUsize>,
    requests: Arc<AtomicUsize>,
    owner: tokio::task::JoinHandle<()>,
}
impl Drop for Peer {
    fn drop(&mut self) {
        self.owner.abort();
    }
}
impl Peer {
    async fn new(fault: impl Fn(&str) + Send + Sync + 'static) -> Self {
        Self::with_ping(fault, false).await
    }
    async fn with_ping(fault: impl Fn(&str) + Send + Sync + 'static, ping: bool) -> Self {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/mcp", listener.local_addr().unwrap());
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let requests = Arc::new(AtomicUsize::new(0));
        let traffic = requests.clone();
        let owner = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                let end = loop {
                    let mut buffer = [0; 4096];
                    let n = socket.read(&mut buffer).await.unwrap();
                    if n == 0 {
                        break None;
                    }
                    bytes.extend_from_slice(&buffer[..n]);
                    if let Some(index) = bytes.windows(4).position(|s| s == b"\r\n\r\n") {
                        break Some(index + 4);
                    }
                };
                let Some(end) = end else { continue };
                let headers = String::from_utf8_lossy(&bytes[..end]);
                let length: usize = headers
                    .lines()
                    .find_map(|l| {
                        l.split_once(':')
                            .filter(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                            .map(|(_, v)| v.trim().parse().unwrap())
                    })
                    .unwrap_or(0);
                while bytes.len() < end + length {
                    let mut buffer = [0; 4096];
                    let n = socket.read(&mut buffer).await.unwrap();
                    if n == 0 {
                        break;
                    }
                    bytes.extend_from_slice(&buffer[..n]);
                }
                let request: Value = serde_json::from_slice(&bytes[end..]).unwrap_or(Value::Null);
                let method = request["method"].as_str().unwrap_or("http");
                traffic.fetch_add(1, Ordering::SeqCst);
                if method == "tools/call" || method == "http" {
                    count.fetch_add(1, Ordering::SeqCst);
                }
                fault(method);
                let result = match method {
                    "initialize" => {
                        json!({"protocolVersion":"2025-11-25","capabilities":{"tools":{}},"serverInfo":{"name":"fixture","version":"1"}})
                    }
                    "tools/list" => json!({"tools":[metadata()]}),
                    "tools/call" => json!({"content":[],"structuredContent":{}}),
                    _ => json!({}),
                };
                let body = if method == "http" {
                    json!({})
                } else {
                    json!({"jsonrpc":"2.0","id":request["id"],"result":result})
                }
                .to_string();
                let response = if ping && method == "tools/list" {
                    let body = format!(
                        "event: message\ndata: {}\n\nevent: message\ndata: {body}\n\n",
                        json!({"jsonrpc":"2.0","id":777,"method":"ping"})
                    );
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                } else if method == "notifications/initialized" {
                    "HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        .to_string()
                } else {
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                };
                let _ = socket.write_all(response.as_bytes()).await;
            }
        });
        Self {
            endpoint,
            calls,
            requests,
            owner,
        }
    }
}
fn metadata() -> Value {
    json!({"name":"gate","inputSchema":{"type":"object"}})
}
fn managed(
    f: &Fixture,
    package: Arc<crate::plugins::Package>,
    endpoint: &str,
) -> Arc<ManagedService> {
    use std::os::unix::fs::MetadataExt;
    let m = f.root.path().metadata().unwrap();
    ManagedServices::default()
        .admit(
            package,
            ServiceConfig {
                identity: ServiceIdentity {
                    workspace: (m.dev(), m.ino()),
                    role: "worker".into(),
                    generation: "1".into(),
                    state: "one".into(),
                    credential_revision: "one".into(),
                },
                transport: ServiceTransport::Http(HttpConfig::new(endpoint.into())),
                tools: vec![AdmittedTool {
                    metadata: metadata(),
                    read_only: true,
                }],
                timeout_ms: 2000,
                max_calls: 4,
            },
        )
        .unwrap()
}
fn settle_only_hook(runtime: &SharedRuntime) {
    runtime
        .update(|record| {
            let receipt = record
                .operations
                .iter_mut()
                .find_map(|o| match &mut o.host_invocation {
                    Some(HostInvocation::Lifecycle(r)) => Some(r),
                    _ => None,
                })
                .unwrap();
            receipt.hooks[0].outcome = Some(RawOutcome::Callback { value: json!({}) });
            receipt.hooks[0].uncertain_effects = false;
            Ok(())
        })
        .unwrap();
    let operation = runtime
        .record()
        .unwrap()
        .operations
        .iter()
        .find(|o| matches!(o.host_invocation, Some(HostInvocation::Lifecycle(_))))
        .unwrap()
        .id;
    assert!(
        runtime
            .plugin_funded_remaining(operation, HookEvent::SessionStart)
            .is_ok(),
        "fault revoked broader occurrence authority"
    );
}

struct StartupCheckpoint {
    root: std::path::PathBuf,
    settle: bool,
    reached: Arc<AtomicBool>,
}
thread_local! {
    static STARTUP_CHECKPOINT: std::cell::RefCell<Option<StartupCheckpoint>> = const { std::cell::RefCell::new(None) };
}
/// Revoke authority after independently observing the saved startup admission.
pub(super) fn invalidate_startup_checkpoint(runtime: &SharedRuntime) {
    STARTUP_CHECKPOINT.with_borrow_mut(|pending| {
        let record = runtime.record().unwrap();
        if pending
            .as_ref()
            .is_none_or(|fault| fault.root != record.workspace)
        {
            return;
        }
        let fault = pending.take().unwrap();
        let saved: Value = serde_json::from_slice(
            &std::fs::read(runtime.directory().unwrap().join("state.json")).unwrap(),
        )
        .unwrap();
        let persisted: Record = serde_json::from_value(saved["payload"].clone()).unwrap();
        let startup = persisted.operations.last().unwrap();
        assert!(
            matches!(
                startup.host_invocation,
                Some(HostInvocation::PluginService {
                    outcome: super::super::PluginServiceOutcome::Pending,
                    ..
                })
            ) && !startup.complete
        );
        assert_eq!(startup.budget, persisted.operations[0].budget);
        fault.reached.store(true, Ordering::Release);
        runtime
            .update(|record| {
                for operation in &mut record.operations {
                    match &mut operation.host_invocation {
                        Some(HostInvocation::NativeSession(lifetime)) if !fault.settle => {
                            lifetime.deadline = Some(std::time::Instant::now());
                        }
                        Some(HostInvocation::Lifecycle(receipt)) if fault.settle => {
                            receipt.hooks[0].outcome =
                                Some(RawOutcome::Callback { value: json!({}) });
                            receipt.hooks[0].uncertain_effects = false;
                        }
                        _ => {}
                    }
                }
                Ok(())
            })
            .unwrap();
    });
}

#[tokio::test]
async fn session_service_checkpoint_revocation_retains_startup_without_transport_effects() {
    for fault in [None, Some(false), Some(true)] {
        let f = Fixture::new();
        let peer = Peer::new(|_| {}).await;
        let package = package();
        let service = managed(&f, package.clone(), &peer.endpoint);
        let registration = McpRunner::registration_for_event(
            package,
            declaration(HandlerKind::McpTool),
            HookEvent::SessionStart,
            McpBinding {
                service: service.clone(),
                tool: "gate".into(),
                input: json!({}),
            },
            None,
            McpConfig::default(),
        )
        .unwrap();
        let invocation = f.invocation(&registration.declaration);
        let reached = Arc::new(AtomicBool::new(false));
        STARTUP_CHECKPOINT.set(fault.map(|settle| StartupCheckpoint {
            root: f.root.path().into(),
            settle,
            reached: reached.clone(),
        }));
        let result = registration.runner.prepare(&invocation).await;
        STARTUP_CHECKPOINT.set(None);
        service.stop().await.unwrap();
        assert_eq!(reached.load(Ordering::Acquire), fault.is_some());
        let record = f.runtime.record().unwrap();
        let startup = record
            .operations
            .iter()
            .find(|o| {
                matches!(
                    o.host_invocation,
                    Some(HostInvocation::PluginService { .. })
                )
            })
            .unwrap();
        assert_eq!(startup.budget, record.operations[0].budget);
        if fault.is_some() {
            assert!(result.is_err());
            assert_eq!(
                peer.requests.load(Ordering::SeqCst),
                0,
                "startup crossed revoked persistence checkpoint"
            );
            assert!(
                !startup.complete && !startup.reconciled,
                "denied startup lost original pending settlement"
            );
        } else {
            result.unwrap();
            assert_eq!(peer.requests.load(Ordering::SeqCst), 3);
            assert!(startup.complete);
        }
        assert_eq!(
            f.runtime
                .0
                .lock()
                .unwrap()
                .service_slots
                .available_permits(),
            8
        );
    }
}

async fn settled_service(point: &'static str) {
    let f = Fixture::new();
    let runtime = f.runtime.clone();
    let peer = Peer::new(move |method| {
        if method == point {
            settle_only_hook(&runtime)
        }
    })
    .await;
    let package = package();
    let service = managed(&f, package.clone(), &peer.endpoint);
    let registration = McpRunner::registration_for_event(
        package,
        declaration(HandlerKind::McpTool),
        HookEvent::SessionStart,
        McpBinding {
            service: service.clone(),
            tool: "gate".into(),
            input: json!({}),
        },
        None,
        McpConfig::default(),
    )
    .unwrap();
    let invocation = f.invocation(&registration.declaration);
    let prepared = registration.runner.prepare(&invocation).await;
    let denied = if prepared.is_ok() {
        matches!(
            registration.runner.run(&invocation).await.unwrap(),
            RawOutcome::Failure { .. }
        )
    } else {
        true
    };
    assert!(
        denied,
        "{point}: a settled hook borrowed its still-live lifetime"
    );
    assert_eq!(
        peer.calls.load(Ordering::SeqCst),
        usize::from(point == "tools/call")
    );
    service.stop().await.unwrap();
}

#[tokio::test]
async fn session_service_settled_discovery_cannot_send_with_live_lifetime_and_grant() {
    settled_service("tools/list").await;
}
#[tokio::test]
async fn session_service_settled_reply_cannot_deliver_with_live_lifetime_and_grant() {
    settled_service("tools/call").await;
}

#[tokio::test]
async fn session_http_settled_hook_withholds_late_success_under_live_grant() {
    let f = Fixture::new();
    let runtime = f.runtime.clone();
    let peer = Peer::new(move |_| settle_only_hook(&runtime)).await;
    let registration = HttpRunner::registration_for_event(
        package(),
        declaration(HandlerKind::Http),
        HookEvent::SessionStart,
        HttpConfig::new(peer.endpoint.clone()),
        None,
    )
    .unwrap();
    let invocation = f.invocation(&registration.declaration);
    let result = registration.runner.run(&invocation).await.unwrap();
    assert!(
        f.runtime
            .plugin_funded_remaining(invocation.key.operation, HookEvent::SessionStart)
            .is_ok()
    );
    assert_eq!(peer.calls.load(Ordering::SeqCst), 1);
    assert!(
        matches!(result, RawOutcome::Failure { .. }),
        "settled hook delivered late HTTP success: {result:?}"
    );
}

#[tokio::test]
async fn session_service_stop_releases_capacity_with_retained_handle() {
    let f = Fixture::new();
    let peer = Peer::new(|_| {}).await;
    let package = package();
    let service = managed(&f, package.clone(), &peer.endpoint);
    let registration = McpRunner::registration_for_event(
        package,
        declaration(HandlerKind::McpTool),
        HookEvent::SessionStart,
        McpBinding {
            service: service.clone(),
            tool: "gate".into(),
            input: json!({}),
        },
        None,
        McpConfig::default(),
    )
    .unwrap();
    let invocation = f.invocation(&registration.declaration);
    registration.runner.prepare(&invocation).await.unwrap();
    assert_eq!(
        f.runtime
            .0
            .lock()
            .unwrap()
            .service_slots
            .available_permits(),
        7
    );
    service.stop().await.unwrap();
    assert_eq!(
        f.runtime
            .0
            .lock()
            .unwrap()
            .service_slots
            .available_permits(),
        8,
        "stop returned while a retained handle or monitor still owned capacity"
    );
    let mut retained = vec![service];
    for _ in 0..12 {
        let service = managed(&f, self::package(), &peer.endpoint);
        service.bootstrap(&invocation).await.unwrap();
        service.stop().await.unwrap();
        retained.push(service);
    }
    assert_eq!(
        f.runtime
            .0
            .lock()
            .unwrap()
            .service_slots
            .available_permits(),
        8
    );
    assert_eq!(
        retained.len(),
        13,
        "stopped handles filled native cancellation tracking"
    );
}

#[tokio::test]
async fn session_service_funding_ignores_expired_unrelated_task_clock() {
    let f = Fixture::new();
    let peer = Peer::new(|_| {}).await;
    let package = package();
    let service = managed(&f, package.clone(), &peer.endpoint);
    let registration = McpRunner::registration_for_event(
        package,
        declaration(HandlerKind::McpTool),
        HookEvent::SessionStart,
        McpBinding {
            service: service.clone(),
            tool: "gate".into(),
            input: json!({}),
        },
        None,
        McpConfig::default(),
    )
    .unwrap();
    let invocation = f.invocation(&registration.declaration);
    f.runtime.allocate(Limits::default(), None).unwrap();
    f.runtime
        .update(|r| {
            r.allocation.as_mut().unwrap().deadline_ms = 1;
            Ok(())
        })
        .unwrap();
    assert!(
        f.runtime
            .plugin_funded_remaining(invocation.key.operation, HookEvent::SessionStart)
            .is_ok()
    );
    let prepared = registration.runner.prepare(&invocation).await;
    assert!(
        prepared.is_ok(),
        "session service borrowed expired task clock: {prepared:?}"
    );
    assert!(matches!(
        registration.runner.run(&invocation).await.unwrap(),
        RawOutcome::Mcp { .. }
    ));
    assert_eq!(peer.calls.load(Ordering::SeqCst), 1);
    service.stop().await.unwrap();
}

#[tokio::test]
async fn session_transports_deny_wrong_expired_held_and_reloaded_authority_before_traffic() {
    for mcp in [false, true] {
        for fault in [
            "missing",
            "expired",
            "task",
            "unallocated",
            "foreign-session",
            "foreign-operation",
            "foreign-invocation",
            "snapshot",
            "held",
            "reload",
            "finalized",
            "cancelled",
            "replaced",
        ] {
            let f = Fixture::new();
            let peer = Peer::new(|_| {}).await;
            let package = package();
            let service = managed(&f, package.clone(), &peer.endpoint);
            let registration = if mcp {
                McpRunner::registration_for_event(
                    package,
                    declaration(HandlerKind::McpTool),
                    HookEvent::SessionStart,
                    McpBinding {
                        service: service.clone(),
                        tool: "gate".into(),
                        input: json!({}),
                    },
                    None,
                    McpConfig::default(),
                )
                .unwrap()
            } else {
                HttpRunner::registration_for_event(
                    package,
                    declaration(HandlerKind::Http),
                    HookEvent::SessionStart,
                    HttpConfig::new(peer.endpoint.clone()),
                    None,
                )
                .unwrap()
            };
            let mut invocation = f.invocation(&registration.declaration);
            f.runtime.allocate(Limits::default(), None).unwrap();
            match fault {
                "foreign-session" => invocation.key.session = "other-session".into(),
                "foreign-operation" => invocation.key.operation += 100,
                "foreign-invocation" => invocation.invocation = 1,
                "snapshot" => invocation
                    .key
                    .inputs
                    .push(("changed".into(), "changed".into())),
                "finalized" => f.events.finalize_host_lifetime().unwrap(),
                "cancelled" => f
                    .runtime
                    .cancel_plugin_invocation(invocation.key.operation, HookEvent::SessionStart, 0)
                    .unwrap(),
                "replaced" => {
                    f.runtime
                        .begin_native_session(SessionStart::Resume, None, vec![])
                        .unwrap();
                }
                _ => f
                    .runtime
                    .update(|r| {
                        match fault {
                            "missing" => r.session_hook_allowance = None,
                            "expired" => {
                                r.session_hook_allowance
                                    .as_mut()
                                    .unwrap()
                                    .allocation
                                    .deadline_ms = 1
                            }
                            "held" => r.recovery_pending = true,
                            "reload" => *r = serde_json::from_slice(&serde_json::to_vec(r)?)?,
                            "task" | "unallocated" => {
                                r.operations
                                    .iter_mut()
                                    .find(|o| o.id == invocation.key.operation)
                                    .unwrap()
                                    .budget = Some(if fault == "task" {
                                    super::super::BudgetRef::Task {
                                        session: invocation.key.session.clone(),
                                        epoch: r.task_allocation_epoch,
                                    }
                                } else {
                                    super::super::BudgetRef::Unallocated
                                })
                            }
                            _ => unreachable!(),
                        }
                        Ok(())
                    })
                    .unwrap(),
            }
            let denied = if registration.runner.prepare(&invocation).await.is_err() {
                true
            } else {
                matches!(
                    registration.runner.run(&invocation).await.unwrap(),
                    RawOutcome::Failure { .. }
                )
            };
            assert!(denied, "mcp={mcp} {fault} accepted invalid authority");
            assert_eq!(
                peer.requests.load(Ordering::SeqCst),
                0,
                "mcp={mcp} {fault} sent forbidden transport traffic"
            );
            service.stop().await.unwrap();
        }
    }
}

#[tokio::test]
async fn session_transports_do_not_spend_exhausted_model_or_inspection_slots() {
    for mcp in [false, true] {
        let f = Fixture::new();
        let peer = Peer::new(|_| {}).await;
        let package = package();
        let service = managed(&f, package.clone(), &peer.endpoint);
        let registration = if mcp {
            McpRunner::registration_for_event(
                package,
                declaration(HandlerKind::McpTool),
                HookEvent::SessionStart,
                McpBinding {
                    service: service.clone(),
                    tool: "gate".into(),
                    input: json!({}),
                },
                None,
                McpConfig::default(),
            )
            .unwrap()
        } else {
            HttpRunner::registration_for_event(
                package,
                declaration(HandlerKind::Http),
                HookEvent::SessionStart,
                HttpConfig::new(peer.endpoint.clone()),
                None,
            )
            .unwrap()
        };
        let invocation = f.invocation(&registration.declaration);
        f.runtime
            .update(|r| {
                let grant = r.session_hook_allowance.as_mut().unwrap();
                grant.allocation.model_calls = 1;
                grant.allocation.tool_calls = 1;
                grant.backend_invocations = 1;
                Ok(())
            })
            .unwrap();
        registration.runner.prepare(&invocation).await.unwrap();
        assert!(!matches!(
            registration.runner.run(&invocation).await.unwrap(),
            RawOutcome::Failure { .. }
        ));
        assert_eq!(peer.calls.load(Ordering::SeqCst), 1);
        let grant = f.runtime.record().unwrap().session_hook_allowance.unwrap();
        assert_eq!(
            (
                grant.allocation.model_calls,
                grant.allocation.tool_calls,
                grant.backend_invocations
            ),
            (1, 1, 1)
        );
        service.stop().await.unwrap();
    }
}

#[tokio::test]
async fn session_service_queued_call_expiry_sends_only_the_original_request() {
    let f = Fixture::new();
    let runtime = f.runtime.clone();
    let peer = Peer::new(move |method| {
        if method == "tools/call" {
            runtime
                .update(|r| {
                    let owner = r
                        .operations
                        .iter_mut()
                        .find_map(|o| match &mut o.host_invocation {
                            Some(HostInvocation::NativeSession(owner)) => Some(owner),
                            _ => None,
                        })
                        .unwrap();
                    owner.deadline = Some(std::time::Instant::now());
                    Ok(())
                })
                .unwrap();
        }
    })
    .await;
    let package = package();
    let service = managed(&f, package.clone(), &peer.endpoint);
    let registration = McpRunner::registration_for_event(
        package,
        declaration(HandlerKind::McpTool),
        HookEvent::SessionStart,
        McpBinding {
            service: service.clone(),
            tool: "gate".into(),
            input: json!({}),
        },
        None,
        McpConfig::default(),
    )
    .unwrap();
    let invocation = f.invocation(&registration.declaration);
    registration.runner.prepare(&invocation).await.unwrap();
    let (first, queued) = tokio::join!(
        registration.runner.run(&invocation),
        registration.runner.run(&invocation)
    );
    assert!(matches!(first.unwrap(), RawOutcome::Failure { .. }));
    assert!(matches!(queued.unwrap(), RawOutcome::Failure { .. }));
    assert_eq!(
        peer.calls.load(Ordering::SeqCst),
        1,
        "expired queued work sent a second request"
    );
    let (a, b) = tokio::join!(service.stop(), service.stop());
    a.unwrap();
    b.unwrap();
    assert_eq!(
        f.runtime
            .0
            .lock()
            .unwrap()
            .service_slots
            .available_permits(),
        8
    );
}

#[tokio::test]
async fn session_service_revocation_at_discovery_or_reply_prevents_effect_or_delivery() {
    for point in ["tools/list", "tools/call"] {
        let f = Fixture::new();
        let slot = Arc::new(std::sync::Mutex::new(None::<Arc<ManagedService>>));
        let target = slot.clone();
        let peer = Peer::new(move |method| {
            if method == point {
                target.lock().unwrap().as_ref().unwrap().revoke();
            }
        })
        .await;
        let package = package();
        let service = managed(&f, package.clone(), &peer.endpoint);
        *slot.lock().unwrap() = Some(service.clone());
        let registration = McpRunner::registration_for_event(
            package,
            declaration(HandlerKind::McpTool),
            HookEvent::SessionStart,
            McpBinding {
                service: service.clone(),
                tool: "gate".into(),
                input: json!({}),
            },
            None,
            McpConfig::default(),
        )
        .unwrap();
        let invocation = f.invocation(&registration.declaration);
        let denied = if registration.runner.prepare(&invocation).await.is_err() {
            true
        } else {
            matches!(
                registration.runner.run(&invocation).await.unwrap(),
                RawOutcome::Failure { .. }
            )
        };
        assert!(
            denied,
            "{point}: revoked credentials or service released a pass"
        );
        assert_eq!(
            peer.calls.load(Ordering::SeqCst),
            usize::from(point == "tools/call")
        );
        service.stop().await.unwrap();
        assert_eq!(
            f.runtime
                .0
                .lock()
                .unwrap()
                .service_slots
                .available_permits(),
            8
        );
    }
}

#[tokio::test]
async fn session_transport_credential_remap_at_response_withholds_delivery_and_next_effect() {
    for mcp in [false, true] {
        let f = Fixture::new();
        let alias = f.root.path().join("credential-alias");
        let original = f.root.path().join("first-private");
        let changed = f.root.path().join("second-private");
        std::fs::write(&original, "fixture-private-one").unwrap();
        std::fs::write(&changed, "fixture-private-two").unwrap();
        std::os::unix::fs::symlink(&original, &alias).unwrap();
        let remap = alias.clone();
        let peer = Peer::new(move |method| {
            if method == "tools/list" || method == "http" {
                std::fs::remove_file(&remap).unwrap();
                std::os::unix::fs::symlink(&changed, &remap).unwrap();
            }
        })
        .await;
        let package = package();
        let service = managed(&f, package.clone(), &peer.endpoint);
        let registration = if mcp {
            McpRunner::registration_for_event(
                package,
                declaration(HandlerKind::McpTool),
                HookEvent::SessionStart,
                McpBinding {
                    service: service.clone(),
                    tool: "gate".into(),
                    input: json!({}),
                },
                None,
                McpConfig::default(),
            )
            .unwrap()
        } else {
            HttpRunner::registration_for_event(
                package,
                declaration(HandlerKind::Http),
                HookEvent::SessionStart,
                HttpConfig::new(peer.endpoint.clone()),
                None,
            )
            .unwrap()
        };
        let mut invocation = f.invocation(&registration.declaration);
        invocation.host.credentials = vec![alias.clone()];
        invocation.snapshot = Arc::new(
            GateWorkspace::open_with_credentials(f.root.path(), &[alias])
                .unwrap()
                .capture(&GateReadSet::default(), &AtomicBool::new(false))
                .unwrap(),
        );
        let denied = if registration.runner.prepare(&invocation).await.is_err() {
            true
        } else {
            matches!(
                registration.runner.run(&invocation).await.unwrap(),
                RawOutcome::Failure { .. }
            )
        };
        assert!(denied, "mcp={mcp}: credential remap released a pass");
        assert_eq!(peer.calls.load(Ordering::SeqCst), usize::from(!mcp));
        service.stop().await.unwrap();
    }
}

#[tokio::test]
async fn session_service_settled_sse_request_cannot_send_a_ping_response() {
    let f = Fixture::new();
    let runtime = f.runtime.clone();
    let peer = Peer::with_ping(
        move |method| {
            if method == "tools/list" {
                settle_only_hook(&runtime)
            }
        },
        true,
    )
    .await;
    let package = package();
    let service = managed(&f, package.clone(), &peer.endpoint);
    let registration = McpRunner::registration_for_event(
        package,
        declaration(HandlerKind::McpTool),
        HookEvent::SessionStart,
        McpBinding {
            service: service.clone(),
            tool: "gate".into(),
            input: json!({}),
        },
        None,
        McpConfig::default(),
    )
    .unwrap();
    let invocation = f.invocation(&registration.declaration);
    assert!(registration.runner.prepare(&invocation).await.is_err());
    assert_eq!(
        peer.calls.load(Ordering::SeqCst),
        0,
        "settled SSE exchange sent an unauthorized response"
    );
    assert_eq!(peer.requests.load(Ordering::SeqCst), 3);
    service.stop().await.unwrap();
}
