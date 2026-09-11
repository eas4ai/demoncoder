use super::*;
use crate::{
    events::EventSink,
    plugins::{
        dispatch::{HookInvocation, HookRunner, PreToolPlan, Registration},
        gate_snapshot::GateWorkspace,
        hook_types::HookEvent,
        lifecycle::PostToolPlan,
        runners::{HttpConfig, McpBinding, McpConfig, McpRunner},
        services::{
            AdmittedTool, ManagedService, ManagedServices, ServiceConfig, ServiceIdentity,
            ServiceTransport,
        },
    },
    tools::{ToolExecutor, ToolResult},
    workflow::allocation::{Allocation, Limits},
};
use std::{
    os::unix::fs::MetadataExt,
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

#[path = "settlement_tests.rs"]
mod settlement_tests;

struct Prepared {
    prepares: Arc<AtomicUsize>,
    runs: Arc<AtomicUsize>,
    unknown: bool,
}
#[async_trait::async_trait]
impl HookRunner for Prepared {
    async fn prepare(&self, _: &HookInvocation) -> Result<()> {
        self.prepares.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    async fn run(&self, invocation: &HookInvocation) -> Result<RawOutcome> {
        self.runs.fetch_add(1, Ordering::SeqCst);
        if self.unknown && invocation.events.plugin_event() == HookEvent::PostToolUse {
            return std::future::pending().await;
        }
        Ok(if self.unknown {
            RawOutcome::Failure {
                reason: "runner lost its result after execution".into(),
            }
        } else {
            RawOutcome::Callback {
                value: output(invocation.events.plugin_event()),
            }
        })
    }
}
fn output(event: HookEvent) -> serde_json::Value {
    if event == HookEvent::PreToolUse {
        json!({"hookSpecificOutput":{"hookEventName":event.as_str(),"permissionDecision":"allow"}})
    } else {
        json!({"hookSpecificOutput":{"hookEventName":event.as_str(),"additionalContext":"prepared"}})
    }
}
struct DispatchCase {
    root: tempfile::TempDir,
    _storage: tempfile::TempDir,
    runtime: SharedRuntime,
    package: Arc<crate::plugins::Package>,
    event: HookEvent,
    calls: Vec<(ToolCall, EventSink, ToolResult)>,
    _receiver: tokio::sync::mpsc::Receiver<crate::events::Envelope>,
}
impl DispatchCase {
    fn new(event: HookEvent) -> Self {
        Self::with_count(event, 2)
    }
    fn with_count(event: HookEvent, count: usize) -> Self {
        let root = tempfile::tempdir().unwrap();
        let storage = tempfile::tempdir().unwrap();
        let runtime = fixture(
            root.path(),
            storage.path().join("session").to_str().unwrap(),
        );
        runtime
            .update(|r| {
                r.allocation = Some(Allocation::new(Limits::default())?);
                Ok(())
            })
            .unwrap();
        let package_root = root.path().join("package");
        std::fs::create_dir_all(package_root.join(".claude-plugin")).unwrap();
        std::fs::write(
            package_root.join(".claude-plugin/plugin.json"),
            r#"{"name":"pkg","version":"1"}"#,
        )
        .unwrap();
        let package = Arc::new(
            crate::plugins::inspect(&package_root, &crate::plugins::ImportOptions::default())
                .unwrap(),
        );
        let (sender, receiver) = tokio::sync::mpsc::channel(128);
        let events = EventSink::new("prepare-test".into(), sender, None)
            .unwrap()
            .with_runtime(runtime.clone())
            .for_phase("worker")
            .for_invocation(Some(1));
        // Both operations are genuinely admitted before either lifecycle starts.
        // The second dispatch never creates or repeats an underlying tool effect.
        let calls = (0..count)
            .map(|index| match index {
                0 => "first".to_string(),
                1 => "second".to_string(),
                _ => format!("operation-{index}"),
            })
            .map(|name| {
                let call = ToolCall {
                    id: name.clone(),
                    name: "write".into(),
                    arguments: json!({"path":name,"content":"original"}),
                };
                let (events, replay) = events.begin_tool(&call).unwrap();
                assert!(replay.is_none());
                let result = ToolResult {
                    call_id: call.id.clone(),
                    tool: call.name.clone(),
                    success: true,
                    output: "retained original".into(),
                    exit_code: None,
                };
                if event != HookEvent::PreToolUse {
                    let (_, operation) = events.plugin_context().unwrap();
                    runtime.admit_tool(operation, &call).unwrap();
                    runtime.tool_effect(operation).unwrap();
                    std::fs::write(root.path().join(&name), "original").unwrap();
                    runtime.original_tool_result(operation, &result).unwrap();
                }
                (call, events, result)
            })
            .collect();
        Self {
            root,
            _storage: storage,
            runtime,
            package,
            event,
            calls,
            _receiver: receiver,
        }
    }
    fn declaration(&self, once: bool) -> Declaration {
        let source = ActivationSource::from_package(&self.package).unwrap();
        let binding = once.then(|| {
            self.runtime
                .plugin_hook_activation(
                    HookOrigin::Native,
                    Scope::Project,
                    &source,
                    "skills/check",
                    "worker",
                    ActivationChange::ExplicitInvocation,
                )
                .unwrap()
                .unwrap()
        });
        let mut d = declaration(binding);
        d.source = Some(source);
        d
    }
    async fn dispatch(&self, index: usize, registration: Registration) -> Result<()> {
        self.dispatch_registrations(index, vec![registration]).await
    }
    async fn dispatch_registrations(
        &self,
        index: usize,
        registrations: Vec<Registration>,
    ) -> Result<()> {
        let (call, events, original) = &self.calls[index];
        let workspace = Arc::new(GateWorkspace::open(self.root.path()).unwrap());
        let metadata = std::fs::metadata(self.root.path()).unwrap();
        let identity = (metadata.dev(), metadata.ino());
        let executor = ToolExecutor::new(self.root.path()).unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            if self.event == HookEvent::PreToolUse {
                PreToolPlan::new(registrations)?
                    .admit(&mut call.clone(), events, workspace, identity, &executor)
                    .await
                    .map(|_| ())
            } else {
                Arc::new(PostToolPlan::new(self.event, registrations)?)
                    .dispatch(call, original, events, workspace, identity, &executor)
                    .await
                    .map(|_| ())
            }
        })
        .await
        .unwrap()
    }
    async fn leave_unknown(&self) {
        let runs = Arc::new(AtomicUsize::new(0));
        let registration = Registration {
            declaration: self.declaration(true),
            runner: Arc::new(Prepared {
                prepares: Arc::new(AtomicUsize::new(0)),
                runs: runs.clone(),
                unknown: true,
            }),
            revalidation: None,
        };
        if self.event == HookEvent::PostToolUse {
            // Interrupt after the runner actually starts but before its result.
            // Dropping the owner retains uncertainty and releases its live lease.
            let pending = self.dispatch(0, registration);
            tokio::pin!(pending);
            tokio::time::timeout(Duration::from_secs(2), async {
                tokio::select! {
                    result = &mut pending => panic!("expected a pending result: {result:?}"),
                    _ = async {
                        while runs.load(Ordering::SeqCst) == 0 { tokio::task::yield_now().await; }
                    } => {}
                }
            })
            .await
            .unwrap();
        } else {
            let _ = self.dispatch(0, registration).await;
            self.runtime
                .reconcile("generic recovery does not settle this hook", None)
                .unwrap();
        }
        assert_eq!(runs.load(Ordering::SeqCst), 1);
        assert_eq!(self.runtime.unresolved_plugin_once().unwrap().len(), 1);
    }
    fn verify_originals(&self) {
        if self.event == HookEvent::PreToolUse {
            return;
        }
        for (call, events, original) in &self.calls {
            assert_eq!(
                std::fs::read_to_string(self.root.path().join(&call.id)).unwrap(),
                "original"
            );
            assert_eq!(
                events
                    .original_tool_evidence(&call.id)
                    .unwrap()
                    .unwrap()
                    .output,
                original.output
            );
        }
    }
}

async fn check_prepare(event: HookEvent) {
    for unknown in [false, true] {
        let case = DispatchCase::new(event);
        if unknown {
            case.leave_unknown().await;
        }
        let prepares = Arc::new(AtomicUsize::new(0));
        let runs = Arc::new(AtomicUsize::new(0));
        let registration = Registration {
            declaration: case.declaration(false),
            runner: Arc::new(Prepared {
                prepares: prepares.clone(),
                runs: runs.clone(),
                unknown: false,
            }),
            revalidation: None,
        };
        let result = case.dispatch(1, registration).await;
        assert_eq!(
            prepares.load(Ordering::SeqCst),
            usize::from(!unknown),
            "{event:?}: {result:?}"
        );
        assert_eq!(runs.load(Ordering::SeqCst), usize::from(!unknown));
        assert_eq!(result.is_ok(), !unknown);
        case.verify_originals();
    }
}

#[tokio::test]
async fn omitted_once_checks_unknown_before_pre_prepare() {
    check_prepare(HookEvent::PreToolUse).await;
}
#[tokio::test]
async fn omitted_once_checks_unknown_before_post_prepare() {
    check_prepare(HookEvent::PostToolUse).await;
}

#[tokio::test]
async fn consumed_once_skips_preparation_and_execution_in_both_dispatchers() {
    for event in [HookEvent::PreToolUse, HookEvent::PostToolUse] {
        let case = DispatchCase::new(event);
        let prepares = Arc::new(AtomicUsize::new(0));
        let runs = Arc::new(AtomicUsize::new(0));
        let registration = Registration {
            declaration: case.declaration(true),
            runner: Arc::new(Prepared {
                prepares: prepares.clone(),
                runs: runs.clone(),
                unknown: false,
            }),
            revalidation: None,
        };
        let first = Registration {
            declaration: registration.declaration.clone(),
            runner: registration.runner.clone(),
            revalidation: None,
        };
        case.dispatch(0, first).await.unwrap();
        case.dispatch(1, registration).await.unwrap();
        assert_eq!(prepares.load(Ordering::SeqCst), 1);
        assert_eq!(runs.load(Ordering::SeqCst), 1);
        case.verify_originals();
    }
}

fn metadata() -> serde_json::Value {
    json!({"name":"gate","inputSchema":{"type":"object","additionalProperties":true}})
}
struct Peer {
    endpoint: String,
    initializes: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}
impl Peer {
    async fn new(event: HookEvent) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/mcp", listener.local_addr().unwrap());
        let initializes = Arc::new(AtomicUsize::new(0));
        let count = initializes.clone();
        let task = tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let count = count.clone();
                tokio::time::timeout(Duration::from_secs(2), async {
                    use tokio::io::{AsyncReadExt, AsyncWriteExt};
                    let mut header = Vec::new();
                    while !header.ends_with(b"\r\n\r\n") {
                        header.push(stream.read_u8().await?);
                        anyhow::ensure!(header.len() < 65536, "oversized test request");
                    }
                    let header = String::from_utf8(header)?;
                    let length = header.lines().find_map(|l| { let (k,v)=l.split_once(':')?; k.eq_ignore_ascii_case("content-length").then(|| v.trim().parse::<usize>().unwrap()) }).unwrap_or(0);
                    anyhow::ensure!(length < 65536, "oversized test request");
                    let mut body = vec![0; length];
                    stream.read_exact(&mut body).await?;
                    let request: serde_json::Value = if body.is_empty() { json!({}) } else { serde_json::from_slice(&body)? };
                    let (status, body) = if request.get("id").is_none() {
                        (202, String::new())
                    } else {
                        let result = match request["method"].as_str().unwrap() {
                            "initialize" => { count.fetch_add(1, Ordering::SeqCst); json!({"protocolVersion":"2025-11-25","capabilities":{"tools":{}},"serverInfo":{"name":"fixture","version":"1"}}) },
                            "tools/list" => json!({"tools":[metadata()]}),
                            "tools/call" => json!({"content":[],"structuredContent":output(event),"isError":false}),
                            method => anyhow::bail!("unexpected test method {method}"),
                        };
                        (200, json!({"jsonrpc":"2.0","id":request["id"],"result":result}).to_string())
                    };
                    stream.write_all(format!("HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await?;
                    Ok::<(), anyhow::Error>(())
                }).await.unwrap().unwrap();
            }
        });
        Self {
            endpoint,
            initializes,
            task,
        }
    }
}
impl Drop for Peer {
    fn drop(&mut self) {
        self.task.abort();
    }
}
fn managed(case: &DispatchCase, peer: &Peer) -> Arc<ManagedService> {
    let root = std::fs::metadata(case.root.path()).unwrap();
    ManagedServices::default()
        .admit(
            case.package.clone(),
            ServiceConfig {
                identity: ServiceIdentity {
                    workspace: (root.dev(), root.ino()),
                    role: "worker".into(),
                    generation: "new-generation".into(),
                    state: "new-service".into(),
                    credential_revision: "credentials".into(),
                },
                transport: ServiceTransport::Http(HttpConfig::new(peer.endpoint.clone())),
                tools: vec![AdmittedTool {
                    metadata: metadata(),
                    read_only: false,
                }],
                timeout_ms: 2000,
                max_calls: 8,
            },
        )
        .unwrap()
}
async fn check_mcp_prepare(event: HookEvent) {
    for unknown in [false, true] {
        let case = DispatchCase::new(event);
        if unknown {
            case.leave_unknown().await;
        }
        let peer = Peer::new(event).await;
        let service = managed(&case, &peer);
        let mut d = case.declaration(false);
        d.identity.runner = HandlerKind::McpTool;
        d.identity.generation = "new-generation".into();
        let registration = McpRunner::registration_for_event(
            case.package.clone(),
            d,
            event,
            McpBinding {
                service: service.clone(),
                tool: "gate".into(),
                input: json!({}),
            },
            None,
            McpConfig::default(),
        )
        .unwrap();
        let result = case.dispatch(1, registration).await;
        service.stop().await.unwrap();
        assert_eq!(
            peer.initializes.load(Ordering::SeqCst),
            usize::from(!unknown),
            "{event:?}: {result:?}"
        );
        assert_eq!(result.is_ok(), !unknown, "{event:?}: {result:?}");
        let starts = case
            .runtime
            .record()
            .unwrap()
            .operations
            .iter()
            .filter(|o| {
                matches!(
                    o.host_invocation,
                    Some(crate::workflow::runtime::HostInvocation::PluginService { .. })
                )
            })
            .count();
        assert_eq!(starts, usize::from(!unknown));
        case.verify_originals();
    }
}
#[tokio::test]
async fn omitted_once_never_initializes_managed_mcp_before_pre_reservation() {
    check_mcp_prepare(HookEvent::PreToolUse).await;
}
#[tokio::test]
async fn omitted_once_never_initializes_managed_mcp_before_post_reservation() {
    check_mcp_prepare(HookEvent::PostToolUse).await;
}

#[path = "observer_tests.rs"]
mod observer_tests;
