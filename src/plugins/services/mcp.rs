//! Immutable host admission and finite managed lifetime; calls use the original hook owner.
use super::{
    http::Http,
    protocol,
    stdio::{Pipe, Start},
};
use crate::plugins::{
    Package, SourceValidity,
    dispatch::HookInvocation,
    hook_types::HookDialect,
    receipts::HandlerClass,
    runners::{CommandConfig, HttpConfig},
    wire::SchemaValidator,
};
use crate::workflow::runtime::{RuntimeReference, SharedRuntime, plugin_admission::ServiceOwner};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(Clone, Serialize)]
pub enum ServiceTransport {
    Stdio(StdioConfig),
    Http(HttpConfig),
}
/// Credential environment bindings are distinct from ordinary literal environment data.
#[derive(Clone, Serialize)]
pub struct StdioConfig {
    pub command: CommandConfig,
    pub credentials: BTreeMap<String, String>,
}
impl From<CommandConfig> for StdioConfig {
    fn from(command: CommandConfig) -> Self {
        Self {
            command,
            credentials: BTreeMap::new(),
        }
    }
}
impl StdioConfig {
    fn launch(&self) -> Result<CommandConfig> {
        ensure!(
            self.credentials.len() <= 64,
            "MCP credential count exceeds bound"
        );
        let mut command = self.command.clone();
        for (name, value) in &self.credentials {
            ensure!(
                !value.is_empty() && value.len() <= 8192 && !command.environment.contains_key(name),
                "MCP credential binding invalid or duplicated"
            );
            command.environment.insert(name.clone(), value.clone());
        }
        command.validate(HandlerClass::DecisionGate, HookDialect::Native)?;
        ensure!(
            command.write_paths.is_empty(),
            "MCP initial stdio view is read-only"
        );
        Ok(command)
    }
}
/// Host-selected tool metadata must match discovery exactly. An annotation is not a grant.
#[derive(Clone, Serialize)]
pub struct AdmittedTool {
    pub metadata: Value,
    pub read_only: bool,
}
#[derive(Clone, Serialize)]
pub struct ServiceIdentity {
    pub workspace: (u64, u64),
    pub role: String,
    pub generation: String,
    pub state: String,
    pub credential_revision: String,
}
#[derive(Clone, Serialize)]
pub struct ServiceConfig {
    pub identity: ServiceIdentity,
    pub transport: ServiceTransport,
    pub tools: Vec<AdmittedTool>,
    pub timeout_ms: u64,
    pub max_calls: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ServiceState {
    Admitted,
    Starting,
    Ready,
    Stopped,
    Failed,
}
#[derive(Default)]
pub struct ManagedServices {
    entries: Mutex<BTreeMap<String, Weak<ManagedService>>>,
}
pub struct ManagedService {
    pub(crate) identity: String,
    pub(crate) config: ServiceConfig,
    package: Arc<Package>,
    revoked: Arc<AtomicBool>,
    state: AtomicU8,
    changed: tokio::sync::Notify,
    secrets: protocol::Secrets,
    connection: tokio::sync::Mutex<Option<Connection>>,
}
enum Transport {
    Http(Box<Http>),
    Stdio(Arc<Mutex<Pipe>>),
}
struct Connection {
    transport: Transport,
    runtime: RuntimeReference,
    owner: ServiceOwner,
    next_id: u64,
    calls: u32,
    lease: Option<Arc<tokio::sync::OwnedSemaphorePermit>>,
    snapshot: Option<String>,
    startup: u64,
}
struct CallCancellation {
    service: Arc<ManagedService>,
    complete: bool,
    bootstrap: Option<(RuntimeReference, u64)>,
    hook: Option<(RuntimeReference, u64, u32)>,
}
impl Drop for CallCancellation {
    fn drop(&mut self) {
        if !self.complete {
            self.service
                .state
                .store(ServiceState::Failed as u8, Ordering::Release);
            self.service.revoke();
            if let Some((runtime, owner, invocation)) = &self.hook
                && let Ok(runtime) = runtime.upgrade()
            {
                let _ = runtime.cancel_plugin_hook(*owner, *invocation);
            }
            if let Some((runtime, id)) = &self.bootstrap
                && let Ok(runtime) = runtime.upgrade()
            {
                // A persistence failure marks the existing runtime failed; Drop
                // cannot return it, and the service is already revoked above.
                let _ = runtime.fail_plugin_service(*id, false);
            }
        }
    }
}
impl ManagedServices {
    pub fn admit(
        &self,
        package: Arc<Package>,
        config: ServiceConfig,
    ) -> Result<Arc<ManagedService>> {
        ensure!(
            package.source_validity() == SourceValidity::Valid,
            "MCP package is invalid"
        );
        ensure!(
            (1..=20000).contains(&config.timeout_ms)
                && (1..=128).contains(&config.max_calls)
                && !config.tools.is_empty()
                && config.tools.len() <= 32,
            "MCP service bounds invalid"
        );
        for s in [
            &config.identity.role,
            &config.identity.generation,
            &config.identity.state,
            &config.identity.credential_revision,
        ] {
            ensure!(
                !s.is_empty() && s.len() <= 256 && !s.chars().any(char::is_control),
                "MCP service identity invalid"
            );
        }
        let mut names = std::collections::BTreeSet::new();
        for tool in &config.tools {
            protocol::encode(&tool.metadata)?;
            let name = tool
                .metadata
                .get("name")
                .and_then(Value::as_str)
                .context("MCP tool name missing")?;
            ensure!(
                !name.is_empty() && name.len() <= 128 && names.insert(name),
                "MCP duplicate or invalid admitted tool"
            );
            let schema = tool
                .metadata
                .get("inputSchema")
                .context("MCP input schema missing")?;
            ensure!(
                schema.get("type").and_then(Value::as_str) == Some("object"),
                "MCP tool input schema must be an object schema"
            );
            SchemaValidator::compile(schema)?;
            if let Some(output) = tool.metadata.get("outputSchema") {
                ensure!(
                    output.get("type").and_then(Value::as_str) == Some("object"),
                    "MCP output schema must be an object schema"
                );
                SchemaValidator::compile(output)?;
            }
        }
        match &config.transport {
            ServiceTransport::Http(c) => {
                Http::new(c, Arc::new(Mutex::new(Vec::new())))?;
            }
            ServiceTransport::Stdio(c) => {
                c.launch()?;
            }
        }
        let identity = crate::plugins::admission::digest(&(
            package.name(),
            package.digest(),
            &config,
            protocol::VERSION,
        ))?;
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| anyhow::anyhow!("MCP manager lock failed"))?;
        entries.retain(|_, v| v.strong_count() > 0);
        if let Some(existing) = entries.get(&identity).and_then(Weak::upgrade) {
            return Ok(existing);
        }
        ensure!(entries.len() < 8, "MCP admitted service limit reached");
        let secrets = match &config.transport {
            ServiceTransport::Http(c) => c.credentials.values().map(|c| c.secret.clone()).collect(),
            ServiceTransport::Stdio(c) => c.credentials.values().cloned().collect(),
        };
        let service = Arc::new(ManagedService {
            identity: identity.clone(),
            config,
            package,
            revoked: Arc::new(AtomicBool::new(false)),
            state: AtomicU8::new(ServiceState::Admitted as u8),
            changed: tokio::sync::Notify::new(),
            secrets: Arc::new(Mutex::new(secrets)),
            connection: tokio::sync::Mutex::new(None),
        });
        entries.insert(identity, Arc::downgrade(&service));
        Ok(service)
    }
}
impl ManagedService {
    pub fn state(&self) -> ServiceState {
        match self.state.load(Ordering::Acquire) {
            0 => ServiceState::Admitted,
            1 => ServiceState::Starting,
            2 => ServiceState::Ready,
            3 => ServiceState::Stopped,
            _ => ServiceState::Failed,
        }
    }
    /// Revocation rejects new calls synchronously. stop also joins local ownership.
    pub fn revoke(&self) {
        self.revoked.store(true, Ordering::Release);
        if self.state() != ServiceState::Failed {
            self.state
                .store(ServiceState::Stopped as u8, Ordering::Release);
        }
        self.changed.notify_waiters();
    }
    pub async fn stop(&self) -> Result<()> {
        self.revoke();
        if let Some(connection) = self.connection.lock().await.take() {
            connection.close().await?;
        }
        Ok(())
    }
    pub(crate) fn tool(&self, name: &str) -> Result<&AdmittedTool> {
        self.config
            .tools
            .iter()
            .find(|t| t.metadata.get("name").and_then(Value::as_str) == Some(name))
            .context("MCP tool is not admitted")
    }
    pub(crate) fn package(&self) -> &Package {
        &self.package
    }
    pub(crate) fn protect(&self, value: &Value) -> Result<()> {
        protocol::check_secrets(
            &protocol::encode(value)?,
            &self
                .secrets
                .lock()
                .map_err(|_| anyhow::anyhow!("MCP secret owner failed"))?,
        )
    }
    fn invocation_owner(&self, invocation: &HookInvocation) -> Result<(SharedRuntime, u64)> {
        use std::os::unix::fs::MetadataExt;
        let root = invocation.host.root.metadata()?;
        ensure!(
            !self.revoked.load(Ordering::Acquire),
            "MCP service unavailable"
        );
        ensure!(
            invocation.key.workspace == self.config.identity.workspace
                && (root.dev(), root.ino()) == self.config.identity.workspace
                && invocation.snapshot.root_identity() == self.config.identity.workspace
                && invocation.key.role == self.config.identity.role
                && invocation.declaration.generation == self.config.identity.generation,
            "MCP service authority mismatch"
        );
        let (runtime, operation) = invocation.events.plugin_context()?;
        runtime.plugin_owner(operation)?;
        Ok((runtime, operation))
    }
    /// Only the host dependency phase calls this; it never invokes hooks/models/OAuth.
    /// A competing preparation is an in-progress hold, not a fabricated cycle.
    pub(crate) async fn bootstrap(self: &Arc<Self>, invocation: &HookInvocation) -> Result<()> {
        let (runtime, operation) = self.invocation_owner(invocation)?;
        if self.state() == ServiceState::Ready {
            return Ok(());
        }
        self.state
            .compare_exchange(
                ServiceState::Admitted as u8,
                ServiceState::Starting as u8,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .map_err(|_| {
                anyhow::anyhow!(
                    "MCP dependency startup is in progress or unavailable; no recursive startup"
                )
            })?;
        let mut cancellation = CallCancellation {
            service: self.clone(),
            complete: false,
            bootstrap: None,
            hook: None,
        };
        let owner = runtime.admit_plugin_service(operation)?;
        let startup = runtime.begin_plugin_service(operation, &self.identity)?;
        cancellation.bootstrap = Some((runtime.downgrade(), startup));
        let deadline = Instant::now()
            + runtime
                .validate_plugin_service(&owner)?
                .min(Duration::from_millis(self.config.timeout_ms));
        let prepare = async {
            let mut slot = self.connection.lock().await;
            ensure!(slot.is_none(), "MCP dependency already owns a connection");
            *slot = Some(
                self.create_connection(invocation, runtime.clone(), owner, startup, deadline)
                    .await?,
            );
            self.monitor();
            self.initialize(slot.as_mut().expect("stored connection"), deadline)
                .await?;
            runtime.plugin_owner(operation)?;
            runtime.complete_plugin_service(startup)?;
            if let Transport::Stdio(pipe) = &slot.as_ref().expect("stored connection").transport {
                pipe.lock()
                    .map_err(|_| anyhow::anyhow!("MCP pipe owner failed"))?
                    .release_startup();
            }
            slot.as_mut().expect("stored connection").lease.take();
            self.state
                .store(ServiceState::Ready as u8, Ordering::Release);
            Ok(())
        };
        tokio::pin!(prepare);
        let mut poll = tokio::time::interval(Duration::from_millis(20));
        let result = loop {
            tokio::select! {biased;
                _ = poll.tick() => {
                    ensure!(!self.revoked.load(Ordering::Acquire) && runtime.plugin_owner(operation).is_ok()
                        && Instant::now() < deadline, "MCP dependency startup cancelled; reconciliation required");
                }
                result = &mut prepare => break result,
            }
        };
        cancellation.complete = result.is_ok();
        result
    }
    pub(crate) async fn call(
        self: &Arc<Self>,
        invocation: &HookInvocation,
        name: &str,
        input: Value,
    ) -> Result<Value> {
        let (runtime, operation) = self.invocation_owner(invocation)?;
        ensure!(
            self.state() == ServiceState::Ready,
            "MCP dependency must be bootstrapped before hook dispatch"
        );
        let tool = self.tool(name)?;
        SchemaValidator::compile(&tool.metadata["inputSchema"])?.validate(&input)?;
        protocol::encode(&input)?;
        let deadline = Instant::now()
            + runtime
                .remaining()?
                .min(Duration::from_millis(self.config.timeout_ms));
        let mut cancellation = CallCancellation {
            service: self.clone(),
            complete: false,
            bootstrap: None,
            hook: Some((runtime.downgrade(), operation, invocation.invocation)),
        };
        let mut poll = tokio::time::interval(Duration::from_millis(20));
        let run = async {
            let mut slot = self.connection.lock().await;
            ensure!(
                !self.revoked.load(Ordering::Acquire),
                "MCP queued call was revoked"
            );
            runtime.plugin_owner(operation)?;
            let connection = slot.as_mut().context("MCP connection missing")?;
            connection.lease = Some(invocation.runner_lease.clone());
            connection
                .runtime
                .upgrade()?
                .validate_plugin_service(&connection.owner)?;
            ensure!(
                runtime.plugin_session()? == connection.runtime.upgrade()?.plugin_session()?,
                "MCP service session changed"
            );
            if let Some(snapshot) = &connection.snapshot {
                ensure!(
                    *snapshot == crate::plugins::admission::digest(&invocation.key.inputs)?,
                    "MCP retained view changed; readmission required"
                );
            }
            ensure!(
                connection.calls < self.config.max_calls,
                "MCP call allowance exhausted"
            );
            if connection.calls > 0 {
                self.discover(connection, deadline).await?;
            }
            connection.calls += 1;
            runtime.plugin_owner(operation)?;
            let result = connection
                .request(
                    "tools/call",
                    json!({"name":name,"arguments":input}),
                    deadline,
                )
                .await?;
            if let Some(schema) = tool.metadata.get("outputSchema") {
                let structured = result
                    .get("structuredContent")
                    .filter(|v| v.is_object())
                    .context("MCP declared output schema requires structured content")?;
                SchemaValidator::compile(schema)?.validate(structured)?;
            }
            connection.lease.take();
            Ok(result)
        };
        tokio::pin!(run);
        let result = loop {
            tokio::select! {biased;
                _ = poll.tick() => {
                    if self.revoked.load(Ordering::Acquire) || runtime.plugin_owner(operation).is_err() || Instant::now() >= deadline {
                        break Err(anyhow::anyhow!("MCP call cancelled or owner expired; effects may be unknown"));
                    }
                }
                value = &mut run => break value,
            }
        };
        if result.is_err() {
            self.revoked.store(true, Ordering::Release);
            self.state
                .store(ServiceState::Failed as u8, Ordering::Release);
            self.changed.notify_waiters();
        }
        cancellation.complete = true;
        result
    }
    async fn create_connection(
        &self,
        invocation: &HookInvocation,
        runtime: SharedRuntime,
        owner: ServiceOwner,
        startup: u64,
        deadline: Instant,
    ) -> Result<Connection> {
        let transport = match &self.config.transport {
            ServiceTransport::Http(config) => {
                Transport::Http(Box::new(Http::new(config, self.secrets.clone())?))
            }
            ServiceTransport::Stdio(config) => {
                let start = Start::capture(invocation, runtime.clone(), owner.clone());
                let (package, config, revoked, secrets) = (
                    self.package.clone(),
                    config.launch()?,
                    self.revoked.clone(),
                    self.secrets.clone(),
                );
                let pipe = tokio::task::spawn_blocking(move || {
                    Pipe::start(start, package, config, revoked, secrets, deadline)
                })
                .await
                .map_err(|_| anyhow::anyhow!("MCP startup owner failed"))??;
                Transport::Stdio(Arc::new(Mutex::new(pipe)))
            }
        };
        Ok(Connection {
            transport,
            runtime: runtime.downgrade(),
            owner,
            startup,
            next_id: 1,
            calls: 0,
            lease: Some(invocation.runner_lease.clone()),
            snapshot: matches!(self.config.transport, ServiceTransport::Stdio(_))
                .then(|| crate::plugins::admission::digest(&invocation.key.inputs))
                .transpose()?,
        })
    }
    async fn initialize(&self, connection: &mut Connection, deadline: Instant) -> Result<()> {
        let initialized = connection.request("initialize", json!({"protocolVersion":protocol::VERSION,
            "capabilities":{},"clientInfo":{"name":"demoncoder","version":env!("CARGO_PKG_VERSION")}}), deadline).await?;
        protocol::initialize_result(&initialized)?;
        connection
            .send(
                &json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
                None,
                deadline,
            )
            .await?;
        self.discover(connection, deadline).await
    }
    async fn discover(&self, connection: &mut Connection, deadline: Instant) -> Result<()> {
        let mut cursor: Option<String> = None;
        let mut cursors = std::collections::BTreeSet::new();
        let mut tools = BTreeMap::new();
        let mut total = 0;
        for page in 0..8 {
            let list = connection
                .request(
                    "tools/list",
                    cursor
                        .as_ref()
                        .map(|c| json!({"cursor":c}))
                        .unwrap_or(json!({})),
                    deadline,
                )
                .await?;
            let entries = list
                .get("tools")
                .and_then(Value::as_array)
                .context("MCP tool discovery malformed")?;
            for entry in entries {
                total += protocol::encode(entry)?.len();
                ensure!(
                    total <= protocol::MAX_MESSAGE * 4 && tools.len() < 128,
                    "MCP discovery exceeds bound"
                );
                let name = entry
                    .get("name")
                    .and_then(Value::as_str)
                    .context("MCP discovered tool name missing")?;
                ensure!(
                    !name.is_empty()
                        && name.len() <= 128
                        && tools.insert(name.to_owned(), entry.clone()).is_none(),
                    "MCP duplicate discovered tool"
                );
            }
            cursor = list
                .get("nextCursor")
                .map(|c| {
                    c.as_str()
                        .map(str::to_owned)
                        .context("MCP cursor malformed")
                })
                .transpose()?;
            if let Some(cursor) = &cursor {
                ensure!(
                    !cursor.is_empty()
                        && cursor.len() <= 256
                        && cursors.insert(cursor.clone())
                        && page < 7,
                    "MCP discovery cursor cycle or limit"
                );
            } else {
                break;
            }
        }
        for tool in &self.config.tools {
            ensure!(
                tools.get(tool.metadata["name"].as_str().expect("validated tool"))
                    == Some(&tool.metadata),
                "MCP admitted tool changed or unavailable"
            );
        }
        Ok(())
    }
    fn monitor(self: &Arc<Self>) {
        let service = self.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = service.changed.notified() => {}
                    _ = tokio::time::sleep(Duration::from_millis(20)) => {}
                }
                let invalid = service.revoked.load(Ordering::Acquire)
                    || service
                        .connection
                        .try_lock()
                        .ok()
                        .and_then(|slot| {
                            slot.as_ref().map(|c| {
                                if c.runtime
                                    .upgrade()
                                    .and_then(|runtime| runtime.validate_plugin_service(&c.owner))
                                    .is_err()
                                {
                                    return true;
                                }
                                if service.state() == ServiceState::Ready
                                    && let Transport::Stdio(pipe) = &c.transport
                                {
                                    return pipe
                                        .try_lock()
                                        .map(|mut pipe| pipe.idle().is_err())
                                        .unwrap_or(false);
                                }
                                false
                            })
                        })
                        .unwrap_or(false);
                if invalid {
                    let _ = service.stop().await;
                    break;
                }
            }
        });
    }
}
impl Connection {
    async fn send(
        &mut self,
        message: &Value,
        expected: Option<u64>,
        deadline: Instant,
    ) -> Result<Value> {
        self.runtime
            .upgrade()?
            .validate_plugin_service(&self.owner)?;
        match &mut self.transport {
            Transport::Http(http) => http.send(message, expected).await,
            Transport::Stdio(pipe) => {
                let (pipe, message, lease) = (pipe.clone(), message.clone(), self.lease.clone());
                tokio::task::spawn_blocking(move || {
                    let mut pipe = pipe
                        .lock()
                        .map_err(|_| anyhow::anyhow!("MCP pipe owner failed"))?;
                    let result = pipe.send(&message, expected, deadline);
                    if result.is_err() {
                        pipe.hold_uncertain(lease);
                    }
                    result
                })
                .await
                .map_err(|_| anyhow::anyhow!("MCP pipe worker failed"))?
            }
        }
    }
    async fn request(&mut self, method: &str, params: Value, deadline: Instant) -> Result<Value> {
        let id = self.next_id;
        self.next_id = self
            .next_id
            .checked_add(1)
            .context("MCP request count exhausted")?;
        ensure!(id <= 256, "MCP service request limit reached");
        self.send(
            &json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}),
            Some(id),
            deadline,
        )
        .await
    }
    async fn close(self) -> Result<()> {
        let local = matches!(self.transport, Transport::Stdio(_));
        match self.transport {
            Transport::Http(http) => tokio::time::timeout(Duration::from_millis(500), http.close())
                .await
                .map_err(|_| anyhow::anyhow!("MCP remote close timed out; no rollback implied"))?,
            Transport::Stdio(pipe) => tokio::task::spawn_blocking(move || {
                pipe.lock()
                    .map_err(|_| anyhow::anyhow!("MCP pipe owner failed"))?
                    .close()?;
                drop(pipe);
                Ok(())
            })
            .await
            .map_err(|_| anyhow::anyhow!("MCP shutdown owner failed"))?,
        }?;
        if local && let Ok(runtime) = self.runtime.upgrade() {
            runtime.fail_plugin_service(self.startup, true)?;
        }
        Ok(())
    }
}
