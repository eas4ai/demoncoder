use demoncoder::{
    config::Connection,
    plugins::{
        self,
        dispatch::{Declaration, DeclarationIdentity, HandlerClass, Matcher, Registration, Scope},
        gate_snapshot::GateReadSet,
        hook_types::{HandlerKind, HookDialect, HookEvent},
        runners::{
            CommandConfig, CommandProgram, CommandRunner, HttpConfig, HttpRunner, McpBinding,
            McpConfig, McpRunner, ModelConfig, ModelRunner,
        },
        services::{
            AdmittedTool, ManagedService, ManagedServices, ServiceConfig, ServiceIdentity,
            ServiceTransport,
        },
    },
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

struct IdleModel;

#[async_trait::async_trait]
impl demoncoder::native::Model for IdleModel {
    fn prompt(&mut self, _: String) {}
    fn results(&mut self, _: Vec<demoncoder::tools::ToolResult>) {}
    fn checkpoint(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!({"fixture":"idle"}))
    }
    async fn response(
        &mut self,
        _: &demoncoder::events::EventSink,
    ) -> anyhow::Result<Vec<demoncoder::tools::ToolCall>> {
        Ok(Vec::new())
    }
}

pub struct RunningControl {
    pub runtime: demoncoder::workflow::runtime::SharedRuntime,
    pub command_tx: tokio::sync::mpsc::Sender<demoncoder::session::Command>,
    pub worker: tokio::task::JoinHandle<anyhow::Result<()>>,
    _event_rx: tokio::sync::mpsc::Receiver<demoncoder::events::Envelope>,
}

pub struct ActualOwnerPeer {
    pub connection: Connection,
    root: std::path::PathBuf,
    _child: OwnedChild,
}

struct OwnedChild(std::process::Child);

impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

impl ActualOwnerPeer {
    pub async fn start(root: &std::path::Path, adapter: &str, hold: bool) -> anyhow::Result<Self> {
        use anyhow::{Context, ensure};
        use sha2::{Digest, Sha256};
        use std::io::Read;
        use tokio::io::{AsyncBufReadExt, AsyncReadExt};

        std::fs::create_dir_all(root.join("home"))?;
        let tests = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
        let mut child = OwnedChild(
            std::process::Command::new("/usr/bin/python3")
                .arg(tests.join("plugin_batch_model.py"))
                .arg(root)
                .arg(adapter)
                .arg(if hold { "hold-owner" } else { "owner" })
                .stdout(std::process::Stdio::piped())
                .spawn()?,
        );
        let stdout = tokio::process::ChildStdout::from_std(
            child.0.stdout.take().context("owner peer stdout")?,
        )?;
        let mut ready = String::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(10),
            tokio::io::BufReader::new(stdout)
                .take(4097)
                .read_line(&mut ready),
        )
        .await
        .context("owner peer readiness timed out")??;
        ensure!(
            ready.ends_with('\n') && ready.len() <= 4096,
            "invalid owner peer readiness"
        );
        let ready: Value = serde_json::from_str(&ready)?;
        let endpoint = format!("http://127.0.0.1:{}", ready["port"]);
        let mut connection: Connection = if matches!(adapter, "claude" | "codex") {
            let (variable, expected) = if adapter == "claude" {
                (
                    "DEMONCODER_TEST_CLAUDE",
                    "0399c793ff571d5946ef923d80b4f330d05ac4b6842a6b0775468f5d389403c0",
                )
            } else {
                (
                    "DEMONCODER_TEST_CODEX",
                    "c4d77a245a7fcda26f606bb4f726b59ede5fcf0eb322fb7d625a759fc150592a",
                )
            };
            let binary = std::path::PathBuf::from(
                std::env::var_os(variable).with_context(|| format!("{variable} is unset"))?,
            )
            .canonicalize()?;
            let mut source = std::fs::File::open(&binary)?;
            let mut digest = Sha256::new();
            let mut buffer = [0_u8; 64 * 1024];
            loop {
                let count = source.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                digest.update(&buffer[..count]);
            }
            ensure!(
                format!("{:x}", digest.finalize()) == expected,
                "{adapter} executable differs from the pinned artifact"
            );
            std::fs::write(
                root.join("backend.json"),
                serde_json::to_vec(&json!({
                    "binary": binary,
                    "endpoint": endpoint,
                    "ca": ready["ca"],
                }))?,
            )?;
            let launcher = tests.join(format!("plugin_{adapter}_launcher.py"));
            let relay = root.join(format!("plugin_{adapter}_artifact_relay.py"));
            std::fs::write(
                &relay,
                format!(
                    "#!/usr/bin/python3\nimport os, sys\nos.environ['DEMONCODER_TEST_BACKEND_ROOT'] = {}\nos.execv('/usr/bin/python3', ['/usr/bin/python3', {}, *sys.argv[1:]])\n",
                    serde_json::to_string(root.to_str().context("owner peer root is not UTF-8")?)?,
                    serde_json::to_string(
                        launcher
                            .to_str()
                            .context("owner peer launcher path is not UTF-8")?
                    )?,
                ),
            )?;
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&relay, std::fs::Permissions::from_mode(0o700))?;
            serde_json::from_value(json!({
                "adapter": adapter,
                "model": if adapter == "claude" { "claude-sonnet-4-6" } else { "gpt-5.4" },
                "binary": relay,
            }))?
        } else {
            serde_json::from_value(json!({
                "adapter": adapter,
                "endpoint": format!("{endpoint}/{}", if adapter == "openai-api" { "responses" } else { "messages" }),
                "api_key": "synthetic-owner-key",
                "model": "owner-model",
                "max_output_tokens": 1024,
            }))?
        };
        connection.access.supervisor = Some(supervisor());
        Ok(Self {
            connection,
            root: root.to_owned(),
            _child: child,
        })
    }

    pub async fn wait_held(&self) {
        let held = tokio::time::timeout(std::time::Duration::from_secs(20), async {
            while !self.root.join("request-held").is_file() {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await;
        if let Err(error) = held {
            let bytes = |name: &str| {
                std::fs::metadata(self.root.join(name))
                    .map(|metadata| metadata.len())
                    .unwrap_or(0)
            };
            panic!(
                "actual provider request was not held: adapter={} request_count={} request_held={} backend_spawned={} relay_started={} backend_wire_bytes={} backend_stderr_bytes={} timeout={error}",
                self.connection.adapter,
                self.request_count(),
                self.root.join("request-held").is_file(),
                self.root.join("backend.pid").is_file(),
                self.root.join("relay.pid").is_file(),
                bytes("backend-wire.jsonl"),
                bytes("backend-stderr.txt"),
            );
        }
    }

    pub fn request_count(&self) -> usize {
        std::fs::read_to_string(self.root.join("request-count"))
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(0)
    }

    pub fn request_models(&self) -> Vec<String> {
        std::fs::read_to_string(self.root.join("model-requests.jsonl"))
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .filter_map(|request| request["body"]["model"].as_str().map(str::to_owned))
            .collect()
    }

    pub fn requests_contain(&self, text: &str) -> bool {
        std::fs::read_to_string(self.root.join("model-requests.jsonl"))
            .is_ok_and(|requests| requests.contains(text))
    }

    pub fn assert_backend_workspace(&self, expected: &std::path::Path) {
        let pid = std::fs::read_to_string(self.root.join("backend.pid"))
            .expect("backend PID marker")
            .parse::<u32>()
            .expect("backend PID");
        let actual = std::fs::read_link(format!("/proc/{pid}/cwd"))
            .expect("live backend working directory")
            .canonicalize()
            .expect("canonical backend working directory");
        assert_eq!(
            actual,
            expected
                .canonicalize()
                .expect("canonical expected workspace"),
            "{} backend task left the admitted workspace",
            self.connection.adapter
        );
    }

    pub fn release(&self) {
        std::fs::write(self.root.join("release-owner"), "release").unwrap();
    }

    pub async fn disconnect_relay(&self) {
        let pid = std::fs::read_to_string(self.root.join("relay.pid"))
            .expect("relay PID marker")
            .trim()
            .to_owned();
        let status = std::process::Command::new("/bin/kill")
            .args(["-USR1", &pid])
            .status()
            .expect("signal installed backend relay");
        assert!(status.success(), "installed backend relay signal failed");
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            while !self.root.join("relay-disconnected").is_file() {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("installed backend relay did not disconnect");
    }
}

pub struct ActualRunningControl {
    pub runtime: demoncoder::workflow::runtime::SharedRuntime,
    pub command_tx: tokio::sync::mpsc::Sender<demoncoder::session::Command>,
    events: demoncoder::events::EventSink,
    worker: tokio::task::JoinHandle<anyhow::Result<()>>,
    event_rx: tokio::sync::mpsc::Receiver<demoncoder::events::Envelope>,
    native_lifetime: bool,
}

impl ActualRunningControl {
    pub fn pause_next_model_switch_source_dispatch(
        &self,
    ) -> (
        tokio::sync::oneshot::Receiver<()>,
        tokio::sync::oneshot::Sender<()>,
    ) {
        self.events.pause_next_after_model_switch_source_dispatch()
    }

    pub fn pause_next_before_model_switch_final_validation(
        &self,
    ) -> (
        tokio::sync::oneshot::Receiver<()>,
        tokio::sync::oneshot::Sender<()>,
    ) {
        self.events
            .pause_next_before_model_switch_final_validation()
    }

    pub fn pause_next_after_model_switch_source_effect(
        &self,
    ) -> (
        tokio::sync::oneshot::Receiver<()>,
        tokio::sync::oneshot::Sender<()>,
    ) {
        self.events.pause_next_after_model_switch_source_effect()
    }

    pub async fn request_shutdown(&self) {
        self.command_tx
            .send(demoncoder::session::Command::Shutdown)
            .await
            .unwrap();
    }

    pub fn worker_finished(&self) -> bool {
        self.worker.is_finished()
    }

    pub async fn finish_shutdown(self) {
        self.worker.await.unwrap().unwrap();
    }

    pub async fn wait_turn_status(&mut self) -> &'static str {
        tokio::time::timeout(std::time::Duration::from_secs(40), async {
            loop {
                let event = self
                    .event_rx
                    .recv()
                    .await
                    .expect("owner event stream closed");
                if let demoncoder::events::Event::TurnFinished { status } = event.event {
                    return status;
                }
            }
        })
        .await
        .expect("actual owner turn did not finish")
    }

    pub async fn submit(&self, prompt: &str) {
        let (reply, accepted) = tokio::sync::oneshot::channel();
        self.command_tx
            .send(demoncoder::session::Command::Submit {
                text: prompt.into(),
                reply,
            })
            .await
            .unwrap();
        accepted.await.unwrap().unwrap();
    }

    pub async fn cancel(&self) {
        self.command_tx
            .send(demoncoder::session::Command::Cancel)
            .await
            .unwrap();
    }

    pub async fn collect_turn_events(
        &mut self,
        expected_status: &'static str,
    ) -> Vec<demoncoder::events::Event> {
        tokio::time::timeout(std::time::Duration::from_secs(40), async {
            let mut events = Vec::new();
            loop {
                let event = self
                    .event_rx
                    .recv()
                    .await
                    .expect("owner event stream closed")
                    .event;
                let finished = match &event {
                    demoncoder::events::Event::TurnFinished { status } => {
                        assert_eq!(
                            *status, expected_status,
                            "actual owner events before completion: {events:?}"
                        );
                        true
                    }
                    _ => false,
                };
                events.push(event);
                if finished {
                    return events;
                }
            }
        })
        .await
        .expect("actual owner turn did not finish")
    }

    pub async fn wait_turn_finished(&mut self) {
        tokio::time::timeout(std::time::Duration::from_secs(40), async {
            loop {
                let event = self
                    .event_rx
                    .recv()
                    .await
                    .expect("owner event stream closed");
                if let demoncoder::events::Event::TurnFinished { status } = event.event {
                    assert_eq!(status, "complete");
                    break;
                }
            }
        })
        .await
        .expect("actual owner turn did not finish");
    }

    pub async fn wait_turn_finished_with_errors(&mut self) -> Vec<String> {
        tokio::time::timeout(std::time::Duration::from_secs(40), async {
            let mut errors = Vec::new();
            loop {
                let event = self
                    .event_rx
                    .recv()
                    .await
                    .expect("owner event stream closed");
                match event.event {
                    demoncoder::events::Event::Error { message } => errors.push(message),
                    demoncoder::events::Event::TurnFinished { status } => {
                        assert_eq!(status, "complete", "actual owner errors: {errors:?}");
                        return errors;
                    }
                    _ => {}
                }
            }
        })
        .await
        .expect("actual owner turn did not finish")
    }

    pub async fn wait_turn_cancelled(&mut self) {
        tokio::time::timeout(std::time::Duration::from_secs(40), async {
            loop {
                let event = self
                    .event_rx
                    .recv()
                    .await
                    .expect("owner event stream closed");
                if let demoncoder::events::Event::TurnFinished { status } = event.event {
                    assert_eq!(status, "cancelled");
                    break;
                }
            }
        })
        .await
        .expect("actual owner turn did not cancel");
    }

    pub async fn wait_turn_failed(&mut self) -> Vec<String> {
        tokio::time::timeout(std::time::Duration::from_secs(40), async {
            let mut errors = Vec::new();
            loop {
                let event = self
                    .event_rx
                    .recv()
                    .await
                    .expect("owner event stream closed");
                match event.event {
                    demoncoder::events::Event::Error { message } => errors.push(message),
                    demoncoder::events::Event::TurnFinished { status } => {
                        assert_eq!(status, "failed");
                        return errors;
                    }
                    _ => {}
                }
            }
        })
        .await
        .expect("actual owner turn did not fail")
    }

    pub async fn shutdown(self) {
        demoncoder::session::shutdown(self.command_tx, self.worker, self.native_lifetime)
            .await
            .unwrap()
            .unwrap();
    }
}

pub struct ClosePause {
    entered: Arc<tokio::sync::Semaphore>,
    release: Arc<tokio::sync::Semaphore>,
}

impl ClosePause {
    pub async fn wait_entered(&self) {
        tokio::time::timeout(std::time::Duration::from_secs(10), self.entered.acquire())
            .await
            .expect("replacement did not start closing the old provider")
            .unwrap()
            .forget();
    }

    pub fn release(&self) {
        self.release.add_permits(1);
    }
}

struct ClosePauseSession {
    inner: Box<dyn demoncoder::session::Session>,
    entered: Arc<tokio::sync::Semaphore>,
    release: Arc<tokio::sync::Semaphore>,
}

struct FailOnceCloseSession {
    inner: Box<dyn demoncoder::session::Session>,
    fail: bool,
}

pub fn fail_next_session_close(
    inner: Box<dyn demoncoder::session::Session>,
) -> Box<dyn demoncoder::session::Session> {
    Box::new(FailOnceCloseSession { inner, fail: true })
}

#[async_trait::async_trait]
impl demoncoder::session::Session for FailOnceCloseSession {
    fn native_lifetime(&self) -> bool {
        self.inner.native_lifetime()
    }
    fn open_lifetime(
        &mut self,
        source: demoncoder::session::SessionStart,
        events: &demoncoder::events::EventSink,
    ) -> anyhow::Result<()> {
        self.inner.open_lifetime(source, events)
    }
    async fn session_start(
        &mut self,
        source: demoncoder::session::SessionStart,
        events: &demoncoder::events::EventSink,
    ) -> anyhow::Result<()> {
        self.inner.session_start(source, events).await
    }
    async fn session_end(
        &mut self,
        reason: demoncoder::session::SessionEnd,
        events: &demoncoder::events::EventSink,
    ) -> anyhow::Result<()> {
        self.inner.session_end(reason, events).await
    }
    fn observer_notification(&self) -> anyhow::Result<Option<Arc<tokio::sync::Notify>>> {
        self.inner.observer_notification()
    }
    fn observer_ready(&self) -> anyhow::Result<bool> {
        self.inner.observer_ready()
    }
    async fn observer_turn(
        &mut self,
        commands: &mut tokio::sync::mpsc::Receiver<demoncoder::session::Command>,
        events: &demoncoder::events::EventSink,
    ) -> anyhow::Result<demoncoder::session::TurnEnd> {
        self.inner.observer_turn(commands, events).await
    }
    fn owner(&self) -> &'static str {
        self.inner.owner()
    }
    fn admit(&mut self, prompt: &str) -> anyhow::Result<()> {
        self.inner.admit(prompt)
    }
    fn supports_workflow(&self) -> bool {
        self.inner.supports_workflow()
    }
    fn initial_events(&self) -> anyhow::Result<Vec<demoncoder::events::Event>> {
        self.inner.initial_events()
    }
    fn checkpoint(&self) -> Option<serde_json::Value> {
        self.inner.checkpoint()
    }
    fn settle_interruption(&mut self) -> anyhow::Result<()> {
        self.inner.settle_interruption()
    }
    fn restore(
        &mut self,
        checkpoint: &serde_json::Value,
        results: &[demoncoder::tools::ToolResult],
    ) -> anyhow::Result<()> {
        self.inner.restore(checkpoint, results)
    }
    async fn compact(
        &mut self,
        commands: &mut tokio::sync::mpsc::Receiver<demoncoder::session::Command>,
        events: &demoncoder::events::EventSink,
    ) -> anyhow::Result<demoncoder::session::TurnEnd> {
        self.inner.compact(commands, events).await
    }
    async fn turn(
        &mut self,
        prompt: String,
        commands: &mut tokio::sync::mpsc::Receiver<demoncoder::session::Command>,
        events: &demoncoder::events::EventSink,
    ) -> anyhow::Result<demoncoder::session::TurnEnd> {
        self.inner.turn(prompt, commands, events).await
    }
    async fn close(&mut self) -> anyhow::Result<()> {
        if std::mem::take(&mut self.fail) {
            anyhow::bail!("injected old-provider close failure")
        }
        self.inner.close().await
    }
    async fn cancel_background(&mut self) -> anyhow::Result<()> {
        self.inner.cancel_background().await
    }
}

pub fn pause_session_close(
    inner: Box<dyn demoncoder::session::Session>,
) -> (Box<dyn demoncoder::session::Session>, ClosePause) {
    let entered = Arc::new(tokio::sync::Semaphore::new(0));
    let release = Arc::new(tokio::sync::Semaphore::new(0));
    (
        Box::new(ClosePauseSession {
            inner,
            entered: entered.clone(),
            release: release.clone(),
        }),
        ClosePause { entered, release },
    )
}

#[async_trait::async_trait]
impl demoncoder::session::Session for ClosePauseSession {
    fn native_lifetime(&self) -> bool {
        self.inner.native_lifetime()
    }

    fn open_lifetime(
        &mut self,
        source: demoncoder::session::SessionStart,
        events: &demoncoder::events::EventSink,
    ) -> anyhow::Result<()> {
        self.inner.open_lifetime(source, events)
    }

    async fn session_start(
        &mut self,
        source: demoncoder::session::SessionStart,
        events: &demoncoder::events::EventSink,
    ) -> anyhow::Result<()> {
        self.inner.session_start(source, events).await
    }

    async fn session_end(
        &mut self,
        reason: demoncoder::session::SessionEnd,
        events: &demoncoder::events::EventSink,
    ) -> anyhow::Result<()> {
        self.inner.session_end(reason, events).await
    }

    fn observer_notification(&self) -> anyhow::Result<Option<Arc<tokio::sync::Notify>>> {
        self.inner.observer_notification()
    }

    fn observer_ready(&self) -> anyhow::Result<bool> {
        self.inner.observer_ready()
    }

    async fn observer_turn(
        &mut self,
        commands: &mut tokio::sync::mpsc::Receiver<demoncoder::session::Command>,
        events: &demoncoder::events::EventSink,
    ) -> anyhow::Result<demoncoder::session::TurnEnd> {
        self.inner.observer_turn(commands, events).await
    }

    fn owner(&self) -> &'static str {
        self.inner.owner()
    }

    fn admit(&mut self, prompt: &str) -> anyhow::Result<()> {
        self.inner.admit(prompt)
    }

    fn supports_workflow(&self) -> bool {
        self.inner.supports_workflow()
    }

    fn initial_events(&self) -> anyhow::Result<Vec<demoncoder::events::Event>> {
        self.inner.initial_events()
    }

    fn checkpoint(&self) -> Option<serde_json::Value> {
        self.inner.checkpoint()
    }

    fn settle_interruption(&mut self) -> anyhow::Result<()> {
        self.inner.settle_interruption()
    }

    fn restore(
        &mut self,
        checkpoint: &serde_json::Value,
        results: &[demoncoder::tools::ToolResult],
    ) -> anyhow::Result<()> {
        self.inner.restore(checkpoint, results)
    }

    async fn compact(
        &mut self,
        commands: &mut tokio::sync::mpsc::Receiver<demoncoder::session::Command>,
        events: &demoncoder::events::EventSink,
    ) -> anyhow::Result<demoncoder::session::TurnEnd> {
        self.inner.compact(commands, events).await
    }

    async fn turn(
        &mut self,
        prompt: String,
        commands: &mut tokio::sync::mpsc::Receiver<demoncoder::session::Command>,
        events: &demoncoder::events::EventSink,
    ) -> anyhow::Result<demoncoder::session::TurnEnd> {
        self.inner.turn(prompt, commands, events).await
    }

    async fn close(&mut self) -> anyhow::Result<()> {
        self.entered.add_permits(1);
        self.release.acquire().await.unwrap().forget();
        self.inner.close().await
    }

    async fn cancel_background(&mut self) -> anyhow::Result<()> {
        self.inner.cancel_background().await
    }
}

pub async fn start_actual_control(
    handle: &demoncoder::settings::Handle,
    root: &std::path::Path,
    mut connection: Connection,
    plan: Arc<demoncoder::plugins::non_tool::NonToolPlan>,
    limits: demoncoder::workflow::allocation::Limits,
) -> ActualRunningControl {
    connection.access.non_tools = vec![plan];
    start_actual_control_inner(handle, root, connection, Some(&limits), None).await
}

pub async fn start_actual_control_without_grant(
    handle: &demoncoder::settings::Handle,
    root: &std::path::Path,
    mut connection: Connection,
    plan: Arc<demoncoder::plugins::non_tool::NonToolPlan>,
) -> ActualRunningControl {
    connection.access.non_tools = vec![plan];
    start_actual_control_inner(handle, root, connection, None, None).await
}

pub async fn start_actual_control_without_grant_for_connection(
    handle: &demoncoder::settings::Handle,
    root: &std::path::Path,
    connection: Connection,
) -> ActualRunningControl {
    start_actual_control_inner(handle, root, connection, None, None).await
}

pub async fn start_actual_control_with_session(
    handle: &demoncoder::settings::Handle,
    root: &std::path::Path,
    connection: Connection,
    limits: demoncoder::workflow::allocation::Limits,
    inner: Option<Box<dyn demoncoder::session::Session>>,
) -> ActualRunningControl {
    start_actual_control_inner(handle, root, connection, Some(&limits), inner).await
}

pub async fn start_workspace_control(
    root: &std::path::Path,
    connection: Connection,
    limits: demoncoder::workflow::allocation::Limits,
) -> ActualRunningControl {
    use demoncoder::{
        events::{Event, EventSink},
        session,
        workflow::{Settings, WorkflowSession, runtime::SharedRuntime, workspace::CaptureScope},
    };
    let (runtime, _) = SharedRuntime::open_with_session_hooks(
        root,
        &connection,
        None,
        &CaptureScope::default(),
        Some(&limits),
    )
    .unwrap();
    let inner = demoncoder::adapters::builtins()
        .unwrap()
        .open(&connection, root)
        .unwrap();
    let native_lifetime = inner.native_lifetime();
    let workflow = WorkflowSession::new(
        inner,
        connection,
        root.to_owned(),
        Settings::default(),
        runtime.clone(),
        false,
    )
    .unwrap();
    let (command_tx, command_rx) = tokio::sync::mpsc::channel(4);
    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel(512);
    let events = EventSink::new("workspace-runner-owner".into(), event_tx, None)
        .unwrap()
        .with_runtime(runtime.clone());
    let worker = tokio::spawn(session::run(Box::new(workflow), command_rx, events.clone()));
    while !matches!(event_rx.recv().await.unwrap().event, Event::Ready { .. }) {}
    ActualRunningControl {
        runtime,
        command_tx,
        events,
        worker,
        event_rx,
        native_lifetime,
    }
}

async fn start_actual_control_inner(
    handle: &demoncoder::settings::Handle,
    root: &std::path::Path,
    connection: Connection,
    limits: Option<&demoncoder::workflow::allocation::Limits>,
    inner: Option<Box<dyn demoncoder::session::Session>>,
) -> ActualRunningControl {
    use demoncoder::{
        events::{Event, EventSink},
        session,
        workflow::{Settings, WorkflowSession, runtime::SharedRuntime, workspace::CaptureScope},
    };
    let (runtime, _) = SharedRuntime::open_with_session_hooks(
        root,
        &connection,
        None,
        &CaptureScope::default(),
        limits,
    )
    .unwrap();
    let inner = inner.unwrap_or_else(|| {
        demoncoder::adapters::builtins()
            .unwrap()
            .open(&connection, root)
            .unwrap()
    });
    let native_lifetime = inner.native_lifetime();
    let workflow = WorkflowSession::new(
        inner,
        connection.clone(),
        root.to_owned(),
        Settings::default(),
        runtime.clone(),
        false,
    )
    .unwrap()
    .with_live_settings(handle.clone());
    let (command_tx, command_rx) = tokio::sync::mpsc::channel(4);
    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel(512);
    let events = EventSink::new("actual-settings-owner".into(), event_tx, None)
        .unwrap()
        .with_runtime(runtime.clone());
    let worker = tokio::spawn(session::run(Box::new(workflow), command_rx, events.clone()));
    while !matches!(event_rx.recv().await.unwrap().event, Event::Ready { .. }) {}
    ActualRunningControl {
        runtime,
        command_tx,
        events,
        worker,
        event_rx,
        native_lifetime,
    }
}

pub fn supervisor() -> std::path::PathBuf {
    std::env::var_os("CARGO_BIN_EXE_demoncoder")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::env::current_exe()
                .unwrap()
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("demoncoder")
        })
}

impl RunningControl {
    pub async fn start_task(&self, objective: &str) {
        let (reply, accepted) = tokio::sync::oneshot::channel();
        self.command_tx
            .send(demoncoder::session::Command::Submit {
                text: format!("/task {objective}"),
                reply,
            })
            .await
            .unwrap();
        accepted.await.unwrap().unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while self
                .runtime
                .record()
                .unwrap()
                .task
                .as_ref()
                .is_none_or(|task| task.objective != objective || !task.stopped)
            {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("unrelated task did not become active");
    }

    pub async fn shutdown(self) {
        self.command_tx
            .send(demoncoder::session::Command::Shutdown)
            .await
            .unwrap();
        self.worker.await.unwrap().unwrap();
    }

    pub async fn shutdown_application(
        self,
        native_lifetime: bool,
    ) -> anyhow::Result<anyhow::Result<()>> {
        demoncoder::session::shutdown(self.command_tx, self.worker, native_lifetime).await
    }
}

pub async fn start_control(
    handle: &demoncoder::settings::Handle,
    root: &std::path::Path,
    registration: Registration,
) -> RunningControl {
    use demoncoder::{
        events::{Event, EventSink},
        native::NativeSession,
        session,
        tools::ToolExecutor,
        workflow::{
            Settings, WorkflowSession, allocation::Limits, runtime::SharedRuntime,
            workspace::CaptureScope,
        },
    };
    let plan = Arc::new(
        demoncoder::plugins::non_tool::NonToolPlan::new(
            HookEvent::ConfigChange,
            vec![registration],
        )
        .unwrap(),
    );
    let mut connection = handle.current().unwrap().connections["a"].clone();
    connection.access.non_tools = vec![plan];
    let supervisor = std::env::var_os("CARGO_BIN_EXE_demoncoder")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::env::current_exe()
                .unwrap()
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("demoncoder")
        });
    assert!(
        supervisor.is_file(),
        "build the application test supervisor first"
    );
    connection.access.supervisor = Some(supervisor);
    let (runtime, _) = SharedRuntime::open_with_session_hooks(
        root,
        &connection,
        None,
        &CaptureScope::default(),
        Some(&Limits {
            seconds: 60,
            model_calls: 16,
            tool_calls: 16,
        }),
    )
    .unwrap();
    let tools = ToolExecutor::with_policy(root, &connection.access).unwrap();
    let workflow = WorkflowSession::new(
        Box::new(NativeSession::with_tools(Box::new(IdleModel), tools)),
        connection,
        root.to_owned(),
        Settings::default(),
        runtime.clone(),
        false,
    )
    .unwrap()
    .with_live_settings(handle.clone());
    let (command_tx, command_rx) = tokio::sync::mpsc::channel(4);
    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel(32);
    let events = EventSink::new("settings-panel-test".into(), event_tx, None)
        .unwrap()
        .with_runtime(runtime.clone());
    let worker = tokio::spawn(session::run(Box::new(workflow), command_rx, events));
    while !matches!(event_rx.recv().await.unwrap().event, Event::Ready { .. }) {}
    RunningControl {
        runtime,
        command_tx,
        worker,
        _event_rx: event_rx,
    }
}

pub fn declaration(name: &str, dialect: HookDialect, runner: HandlerKind) -> Declaration {
    Declaration {
        required_gate: true,
        source: None,
        once: None,
        identity: DeclarationIdentity {
            package: name.into(),
            code: "captured-code".into(),
            policy: "settings-policy".into(),
            configuration: "captured-configuration".into(),
            generation: "1".into(),
            scope: Scope::Project,
            role: "worker".into(),
            declaration: name.into(),
            index: 0,
            dialect,
            runner,
        },
        class: HandlerClass::Combined,
        priority: 0,
        matcher: Matcher::default(),
        reads: GateReadSet::default(),
        concurrent_group: (dialect == HookDialect::Claude).then(|| "config-change-source".into()),
        read_only_endpoint: None,
        external_precondition: None,
    }
}

fn cwd_declaration(name: &str, dialect: HookDialect, runner: HandlerKind) -> Declaration {
    let mut declaration = declaration(name, dialect, runner);
    declaration.required_gate = false;
    declaration.class = if dialect == HookDialect::Native {
        HandlerClass::Observer
    } else {
        HandlerClass::Combined
    };
    declaration.concurrent_group =
        (dialect == HookDialect::Claude).then(|| "cwd-changed-source".into());
    declaration
}

fn assert_cwd_frame(input: &Value, dialect: HookDialect) {
    assert_eq!(input["hook_event_name"], "CwdChanged");
    assert!(
        input["old_cwd"]
            .as_str()
            .is_some_and(|path| path.ends_with("/a"))
    );
    assert!(
        input["new_cwd"]
            .as_str()
            .is_some_and(|path| path.contains("b with spaces;literal"))
    );
    if dialect == HookDialect::Claude {
        assert!(input["session_id"].is_string());
        assert!(input["transcript_path"].is_string());
        assert!(input["permission_mode"].is_string());
    } else {
        assert_eq!(
            input["demoncoder"]["subject"]["occurrence"]["event"],
            "CwdChanged"
        );
    }
}

pub fn cwd_command_registration(dialect: HookDialect) -> Registration {
    let source = tempfile::tempdir().unwrap();
    std::fs::create_dir(source.path().join(".claude-plugin")).unwrap();
    std::fs::write(
        source.path().join(".claude-plugin/plugin.json"),
        r#"{"name":"cwd-command","version":"1.0.0"}"#,
    )
    .unwrap();
    let source_assertion = if dialect == HookDialect::Claude {
        "assert isinstance(x['session_id'],str) and isinstance(x['transcript_path'],str) and isinstance(x['permission_mode'],str)"
    } else {
        "assert x['demoncoder']['subject']['occurrence']['event']=='CwdChanged'"
    };
    std::fs::write(source.path().join("hook.py"), format!(concat!(
        "import json,sys\n",
        "x=json.load(sys.stdin)\n",
        "assert x['hook_event_name']=='CwdChanged'\n",
        "assert x['old_cwd'].endswith('/a')\n",
        "assert 'b with spaces;literal' in x['new_cwd']\n",
        "{source_assertion}\n",
        "print(json.dumps({{'hookSpecificOutput':{{'hookEventName':'CwdChanged','watchPaths':['/unsupported-command-watch']}}}}))\n",
    ), source_assertion = source_assertion)).unwrap();
    let package =
        Arc::new(plugins::inspect(source.path(), &plugins::ImportOptions::default()).unwrap());
    CommandRunner::registration_for_event(
        package,
        cwd_declaration("cwd-command", dialect, HandlerKind::Command),
        HookEvent::CwdChanged,
        CommandConfig::new(CommandProgram::Argv(vec![
            "/usr/bin/python3".into(),
            "${CLAUDE_PLUGIN_ROOT}/hook.py".into(),
        ])),
        None,
    )
    .unwrap()
}

pub async fn cwd_http_registration(dialect: HookDialect) -> (Registration, Peer) {
    let peer = Peer::new(move |_, input| {
        assert_cwd_frame(input, dialect);
        (
            "application/json".into(),
            json!({"hookSpecificOutput":{
                "hookEventName":"CwdChanged", "watchPaths":["/unsupported-http-watch"]
            }})
            .to_string(),
        )
    })
    .await;
    let registration = HttpRunner::registration_for_event(
        package("cwd-http"),
        cwd_declaration("cwd-http", dialect, HandlerKind::Http),
        HookEvent::CwdChanged,
        HttpConfig::new(peer.endpoint.clone()),
        None,
    )
    .unwrap();
    (registration, peer)
}

pub async fn cwd_mcp_registration(
    workspace: &std::path::Path,
    dialect: HookDialect,
) -> (Registration, Peer, Arc<ManagedService>) {
    use std::os::unix::fs::MetadataExt;
    let metadata =
        json!({"name":"cwd_changed","inputSchema":{"type":"object","additionalProperties":true}});
    let listed = metadata.clone();
    let peer = Peer::new(move |_, input| {
        let result = match input["method"].as_str() {
            Some("initialize") => json!({"protocolVersion":"2025-11-25","capabilities":{"tools":{}},"serverInfo":{"name":"cwd-change","version":"1"}}),
            Some("tools/list") => json!({"tools":[listed]}),
            Some("tools/call") => {
                assert_cwd_frame(&input["params"]["arguments"], dialect);
                json!({"content":[],"structuredContent":{"hookSpecificOutput":{
                    "hookEventName":"CwdChanged", "watchPaths":["/unsupported-mcp-watch"]
                }}})
            }
            _ => json!({}),
        };
        ("application/json".into(), json!({"jsonrpc":"2.0","id":input["id"],"result":result}).to_string())
    }).await;
    let package = package("cwd-mcp");
    let root = std::fs::metadata(workspace).unwrap();
    let mut binding_input = json!({
        "session_id":"${session_id}", "cwd":"${cwd}",
        "hook_event_name":"${hook_event_name}", "old_cwd":"${old_cwd}", "new_cwd":"${new_cwd}"
    });
    if dialect == HookDialect::Claude {
        binding_input["transcript_path"] = json!("${transcript_path}");
        binding_input["permission_mode"] = json!("${permission_mode}");
    } else {
        binding_input["demoncoder"] = json!("${demoncoder}");
    }
    let service = ManagedServices::default()
        .admit(
            package.clone(),
            ServiceConfig {
                identity: ServiceIdentity {
                    workspace: (root.dev(), root.ino()),
                    role: "worker".into(),
                    generation: "1".into(),
                    state: "cwd-change-state".into(),
                    credential_revision: "cwd-change-credentials".into(),
                },
                transport: ServiceTransport::Http(HttpConfig::new(peer.endpoint.clone())),
                tools: vec![AdmittedTool {
                    metadata,
                    read_only: true,
                }],
                timeout_ms: 2_000,
                max_calls: 4,
            },
        )
        .unwrap();
    let registration = McpRunner::registration_for_event(
        package,
        cwd_declaration("cwd-mcp", dialect, HandlerKind::McpTool),
        HookEvent::CwdChanged,
        McpBinding {
            service: service.clone(),
            tool: "cwd_changed".into(),
            input: binding_input,
        },
        None,
        McpConfig::default(),
    )
    .unwrap();
    (registration, peer, service)
}

pub async fn cwd_model_registration(kind: HandlerKind) -> (Registration, Peer) {
    assert!(matches!(kind, HandlerKind::Prompt | HandlerKind::Agent));
    let peer = Peer::new(move |_, request| {
        assert_eq!(request["model"], "cwd-hook-model");
        let prompt = request["input"][0]["content"].as_str().unwrap();
        assert!(prompt.contains("CwdChanged") && prompt.contains("b with spaces;literal") && prompt.contains("/a"));
        let verdict = json!({"ok":true,"reason":format!("cwd {kind:?} observed")}).to_string();
        let events = [
            json!({"type":"response.output_text.delta","delta":verdict}),
            json!({"type":"response.completed","response":{"output":[],"usage":{"input_tokens":1,"output_tokens":1,"input_tokens_details":{"cached_tokens":0}}}}),
        ];
        ("text/event-stream".into(), events.iter().map(|event| format!("data: {event}\n\n")).collect())
    }).await;
    let connection: Connection = serde_json::from_value(json!({
        "adapter":"openai-api", "model":"cwd-hook-model",
        "endpoint":format!("{}/v1/responses", peer.endpoint),
        "api_key":"synthetic-hook-key", "max_output_tokens":512
    }))
    .unwrap();
    let mut declaration = cwd_declaration("cwd-model", HookDialect::Native, kind);
    declaration.class = HandlerClass::DecisionGate;
    let registration = ModelRunner::registration_for_event(
        package("cwd-model"),
        declaration,
        HookEvent::CwdChanged,
        ModelConfig::new(
            connection,
            "Observe the literal CwdChanged event: $ARGUMENTS".into(),
        ),
    )
    .unwrap();
    (registration, peer)
}

pub fn command_registration(dialect: HookDialect, allow: bool) -> Registration {
    let source = tempfile::tempdir().unwrap();
    std::fs::create_dir(source.path().join(".claude-plugin")).unwrap();
    std::fs::write(
        source.path().join(".claude-plugin/plugin.json"),
        r#"{"name":"config-change-command","version":"1.0.0"}"#,
    )
    .unwrap();
    let decision = if allow { "approve" } else { "block" };
    let exact_claude_frame = if dialect == HookDialect::Claude {
        "assert set(x) == {'session_id','transcript_path','cwd','permission_mode','hook_event_name','source'}"
    } else {
        ""
    };
    std::fs::write(
        source.path().join("hook.py"),
            format!(
                concat!(
                    "import json,sys\n",
                    "x=json.load(sys.stdin)\n",
                    "assert x['hook_event_name']=='ConfigChange'\n",
                    "assert x['source']=='user_settings'\n",
                    "{exact_claude_frame}\n",
                    "assert 'private-api-key-canary' not in json.dumps(x)\n",
                    "assert 'private-model-canary' not in json.dumps(x)\n",
                    "print(json.dumps({{'decision':'{decision}','reason':'production command verdict'}}))\n"
                ),
                exact_claude_frame = exact_claude_frame,
                decision = decision,
            ),
    )
    .unwrap();
    let package =
        Arc::new(plugins::inspect(source.path(), &plugins::ImportOptions::default()).unwrap());
    let mut config = CommandConfig::new(CommandProgram::Argv(vec![
        "/usr/bin/python3".into(),
        "${CLAUDE_PLUGIN_ROOT}/hook.py".into(),
    ]));
    config.model = Some("fixture-model".into());
    CommandRunner::registration_for_event(
        package,
        declaration("config-change-command", dialect, HandlerKind::Command),
        HookEvent::ConfigChange,
        config,
        None,
    )
    .unwrap()
}

pub fn pending_command_registration(token: &str) -> Registration {
    let source = tempfile::tempdir().unwrap();
    std::fs::create_dir(source.path().join(".claude-plugin")).unwrap();
    std::fs::write(
        source.path().join(".claude-plugin/plugin.json"),
        r#"{"name":"config-change-pending-command","version":"1.0.0"}"#,
    )
    .unwrap();
    std::fs::write(
        source.path().join("hook.py"),
        concat!(
            "import os,pathlib,time\n",
            "if os.fork()==0:\n",
            " os.setsid()\n",
            " if os.fork(): os._exit(0)\n",
            " for fd in (0,1,2): os.close(fd)\n",
            " pathlib.Path('effects/child-ready').touch()\n",
            " while True:\n",
            "  with open('effects/heartbeat','a') as f: f.write('x')\n",
            "  time.sleep(.005)\n",
            "while not pathlib.Path('effects/child-ready').exists(): time.sleep(.005)\n",
            "pathlib.Path('effects/started').touch()\n",
            "time.sleep(120)\n",
        ),
    )
    .unwrap();
    let package =
        Arc::new(plugins::inspect(source.path(), &plugins::ImportOptions::default()).unwrap());
    let mut config = CommandConfig::new(CommandProgram::Argv(vec![
        "/usr/bin/python3".into(),
        "${CLAUDE_PLUGIN_ROOT}/hook.py".into(),
        token.into(),
    ]));
    config.write_paths = vec!["effects".into()];
    CommandRunner::registration_for_event(
        package,
        declaration(
            "config-change-pending-command",
            HookDialect::Native,
            HandlerKind::Command,
        ),
        HookEvent::ConfigChange,
        config,
        None,
    )
    .unwrap()
}

pub fn processes(token: &str) -> Vec<u32> {
    std::fs::read_dir("/proc")
        .unwrap()
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().to_str()?.parse::<u32>().ok())
        .filter(|pid| {
            std::fs::read(format!("/proc/{pid}/cmdline"))
                .is_ok_and(|bytes| String::from_utf8_lossy(&bytes).contains(token))
        })
        .collect()
}

pub struct Peer {
    pub endpoint: String,
    requests: Arc<Mutex<Vec<Value>>>,
    task: tokio::task::JoinHandle<()>,
}

impl Peer {
    pub async fn new(
        handler: impl Fn(usize, &Value) -> (String, String) + Send + Sync + 'static,
    ) -> Self {
        Self::new_delayed(move |index, value| {
            let (content_type, body) = handler(index, value);
            (std::time::Duration::ZERO, content_type, body)
        })
        .await
    }

    pub async fn new_delayed(
        handler: impl Fn(usize, &Value) -> (std::time::Duration, String, String) + Send + Sync + 'static,
    ) -> Self {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let task = tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                let exchange = async {
                    let mut bytes = Vec::new();
                    let mut buffer = [0_u8; 4096];
                    let header = loop {
                        let n = stream.read(&mut buffer).await.unwrap();
                        if n == 0 {
                            return;
                        }
                        bytes.extend_from_slice(&buffer[..n]);
                        assert!(bytes.len() < 1024 * 1024);
                        if let Some(index) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                            break index + 4;
                        }
                    };
                    let headers = String::from_utf8_lossy(&bytes[..header]);
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    while bytes.len() < header + length {
                        let n = stream.read(&mut buffer).await.unwrap();
                        if n == 0 {
                            return;
                        }
                        bytes.extend_from_slice(&buffer[..n]);
                    }
                    let value: Value =
                        serde_json::from_slice(&bytes[header..header + length]).unwrap();
                    let index = {
                        let mut requests = captured.lock().unwrap();
                        let index = requests.len();
                        requests.push(value.clone());
                        index
                    };
                    let initialized = value["method"] == "notifications/initialized";
                    let (delay, content_type, body) = if initialized {
                        (
                            std::time::Duration::ZERO,
                            "text/plain".into(),
                            String::new(),
                        )
                    } else {
                        handler(index, &value)
                    };
                    tokio::time::sleep(delay).await;
                    let status = if initialized { 202 } else { 200 };
                    let response = format!(
                        "HTTP/1.1 {status} Fixture\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = stream.write_all(response.as_bytes()).await;
                };
                let _ = tokio::time::timeout(std::time::Duration::from_secs(5), exchange).await;
            }
        });
        Self {
            endpoint,
            requests,
            task,
        }
    }

    pub fn count(&self) -> usize {
        self.requests.lock().unwrap().len()
    }

    pub fn requests(&self) -> Vec<Value> {
        self.requests.lock().unwrap().clone()
    }

    pub fn method_count(&self, method: &str) -> usize {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|request| request["method"] == method)
            .count()
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn package(name: &str) -> Arc<plugins::Package> {
    let source = tempfile::tempdir().unwrap();
    std::fs::create_dir(source.path().join(".claude-plugin")).unwrap();
    std::fs::write(
        source.path().join(".claude-plugin/plugin.json"),
        format!(r#"{{"name":"{name}","version":"1.0.0"}}"#),
    )
    .unwrap();
    Arc::new(plugins::inspect(source.path(), &plugins::ImportOptions::default()).unwrap())
}

fn assert_config_change_frame(input: &Value, dialect: HookDialect) {
    assert_eq!(input["hook_event_name"], "ConfigChange");
    assert_eq!(input["source"], "user_settings");
    if dialect == HookDialect::Claude {
        let mut fields = input
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>();
        fields.sort_unstable();
        assert_eq!(
            fields,
            [
                "cwd",
                "hook_event_name",
                "permission_mode",
                "session_id",
                "source",
                "transcript_path",
            ]
        );
    }
    let serialized = input.to_string();
    assert!(!serialized.contains("private-api-key-canary"));
    assert!(!serialized.contains("private-model-canary"));
}

pub async fn http_registration(dialect: HookDialect, allow: bool) -> (Registration, Peer) {
    let peer = Peer::new(move |_, input| {
        assert_config_change_frame(input, dialect);
        let decision = if allow { "approve" } else { "block" };
        (
            "application/json".into(),
            json!({"decision":decision,"reason":"production HTTP verdict"}).to_string(),
        )
    })
    .await;
    let registration = HttpRunner::registration_for_event(
        package("config-change-http"),
        declaration("config-change-http", dialect, HandlerKind::Http),
        HookEvent::ConfigChange,
        HttpConfig::new(peer.endpoint.clone()),
        None,
    )
    .unwrap();
    (registration, peer)
}

pub async fn model_switch_http_registration() -> (Registration, Peer) {
    let peer = Peer::new(|_, input| {
        assert_eq!(input["hook_event_name"], "PreModelSwitch");
        assert_eq!(input["source"], "settings");
        assert_eq!(input["requested_model"], input["model"]);
        (
            "application/json".into(),
            json!({"decision":"approve","reason":"production model switch HTTP verdict"})
                .to_string(),
        )
    })
    .await;
    let registration = HttpRunner::registration_for_event(
        package("model-switch-http"),
        declaration("model-switch-http", HookDialect::Native, HandlerKind::Http),
        HookEvent::PreModelSwitch,
        HttpConfig::new(peer.endpoint.clone()),
        None,
    )
    .unwrap();
    (registration, peer)
}

pub async fn claude_model_switch_http_registration() -> (Registration, Peer) {
    let peer = Peer::new(|_, input| {
        assert_eq!(input["hook_event_name"], "PreModelSwitch");
        assert_eq!(input["source"], "sdk");
        assert!(input["context_tokens"].is_number());
        assert!(input["prompt_cache_warm"].is_boolean());
        assert!(input["cache_ttl"].is_string());
        assert!(input["estimated_cache_write_usd"].is_number());
        assert_eq!(input["pricing"], "catalog");
        (
            "application/json".into(),
            json!({"hookSpecificOutput":{
                "hookEventName":"PreModelSwitch",
                "permissionDecision":"allow"
            }})
            .to_string(),
        )
    })
    .await;
    let registration = HttpRunner::registration_for_event(
        package("claude-model-switch-http"),
        declaration(
            "claude-model-switch-http",
            HookDialect::Claude,
            HandlerKind::Http,
        ),
        HookEvent::PreModelSwitch,
        HttpConfig::new(peer.endpoint.clone()),
        None,
    )
    .unwrap();
    (registration, peer)
}

pub async fn malformed_http_registration() -> (Registration, Peer) {
    let peer = Peer::new(|_, input| {
        assert_config_change_frame(input, HookDialect::Native);
        ("application/json".into(), "{".into())
    })
    .await;
    let registration = HttpRunner::registration_for_event(
        package("config-change-malformed-http"),
        declaration(
            "config-change-malformed-http",
            HookDialect::Native,
            HandlerKind::Http,
        ),
        HookEvent::ConfigChange,
        HttpConfig::new(peer.endpoint.clone()),
        None,
    )
    .unwrap();
    (registration, peer)
}

pub async fn mcp_registration(
    workspace: &std::path::Path,
    dialect: HookDialect,
    allow: bool,
) -> (Registration, Peer, Arc<ManagedService>) {
    mcp_registration_with_max_calls(workspace, dialect, allow, 4).await
}

pub async fn mcp_registration_with_max_calls(
    workspace: &std::path::Path,
    dialect: HookDialect,
    allow: bool,
    max_calls: u32,
) -> (Registration, Peer, Arc<ManagedService>) {
    use std::os::unix::fs::MetadataExt;
    let metadata = json!({
        "name":"config_change",
        "inputSchema":{"type":"object","additionalProperties":true}
    });
    let listed = metadata.clone();
    let peer = Peer::new(move |_, input| {
        let result = match input["method"].as_str() {
            Some("initialize") => json!({
                "protocolVersion":"2025-11-25",
                "capabilities":{"tools":{}},
                "serverInfo":{"name":"config-change","version":"1"}
            }),
            Some("tools/list") => json!({"tools":[listed]}),
            Some("tools/call") => {
                assert_config_change_frame(&input["params"]["arguments"], dialect);
                let decision = if allow { "approve" } else { "block" };
                json!({
                    "content":[],
                    "structuredContent":{
                        "decision":decision,
                        "reason":"production MCP verdict"
                    }
                })
            }
            _ => json!({}),
        };
        (
            "application/json".into(),
            json!({"jsonrpc":"2.0","id":input["id"],"result":result}).to_string(),
        )
    })
    .await;
    let package = package("config-change-mcp");
    let root = std::fs::metadata(workspace).unwrap();
    let mut binding_input = json!({
        "session_id":"${session_id}",
        "cwd":"${cwd}",
        "hook_event_name":"${hook_event_name}",
        "source":"${source}"
    });
    if dialect == HookDialect::Claude {
        binding_input["transcript_path"] = json!("${transcript_path}");
        binding_input["permission_mode"] = json!("${permission_mode}");
    }
    let service = ManagedServices::default()
        .admit(
            package.clone(),
            ServiceConfig {
                identity: ServiceIdentity {
                    workspace: (root.dev(), root.ino()),
                    role: "worker".into(),
                    generation: "1".into(),
                    state: "config-change-state".into(),
                    credential_revision: "config-change-credentials".into(),
                },
                transport: ServiceTransport::Http(HttpConfig::new(peer.endpoint.clone())),
                tools: vec![AdmittedTool {
                    metadata,
                    read_only: true,
                }],
                timeout_ms: 2_000,
                max_calls,
            },
        )
        .unwrap();
    let registration = McpRunner::registration_for_event(
        package,
        declaration("config-change-mcp", dialect, HandlerKind::McpTool),
        HookEvent::ConfigChange,
        McpBinding {
            service: service.clone(),
            tool: "config_change".into(),
            input: binding_input,
        },
        None,
        McpConfig::default(),
    )
    .unwrap();
    (registration, peer, service)
}

pub async fn model_switch_mcp_registrations(
    workspace: &std::path::Path,
    max_calls: u32,
) -> ([Registration; 2], Peer, Arc<ManagedService>) {
    model_switch_mcp_registrations_for_dialect(workspace, max_calls, HookDialect::Native).await
}

pub async fn model_switch_mcp_registrations_for_dialect(
    workspace: &std::path::Path,
    max_calls: u32,
    dialect: HookDialect,
) -> ([Registration; 2], Peer, Arc<ManagedService>) {
    use std::os::unix::fs::MetadataExt;
    let metadata = json!({
        "name":"model_switch",
        "inputSchema":{"type":"object","additionalProperties":true}
    });
    let listed = metadata.clone();
    let peer = Peer::new(move |_, input| {
        let result = match input["method"].as_str() {
            Some("initialize") => json!({
                "protocolVersion":"2025-11-25",
                "capabilities":{"tools":{}},
                "serverInfo":{"name":"model-switch","version":"1"}
            }),
            Some("tools/list") => json!({"tools":[listed]}),
            Some("tools/call") => {
                assert!(matches!(
                    input["params"]["arguments"]["hook_event_name"].as_str(),
                    Some("PreModelSwitch" | "PostModelSwitch")
                ));
                if dialect == HookDialect::Claude {
                    assert_eq!(input["params"]["arguments"]["source"], "sdk");
                    assert!(!input["params"]["arguments"]["context_tokens"].is_null());
                    assert!(!input["params"]["arguments"]["prompt_cache_warm"].is_null());
                    assert!(!input["params"]["arguments"]["cache_ttl"].is_null());
                    assert!(!input["params"]["arguments"]["estimated_cache_write_usd"].is_null());
                    assert!(!input["params"]["arguments"]["pricing"].is_null());
                }
                let event = input["params"]["arguments"]["hook_event_name"]
                    .as_str()
                    .unwrap();
                let structured = if dialect == HookDialect::Claude {
                    if event == "PreModelSwitch" {
                        json!({"hookSpecificOutput":{
                            "hookEventName":event,
                            "permissionDecision":"allow"
                        }})
                    } else {
                        json!({"hookSpecificOutput":{
                            "hookEventName":event,
                            "additionalContext":"claude-model-switch-post-context"
                        }})
                    }
                } else {
                    json!({"decision":"approve","reason":"model switch MCP verdict"})
                };
                json!({
                    "content":[],
                    "structuredContent":structured
                })
            }
            _ => json!({}),
        };
        (
            "application/json".into(),
            json!({"jsonrpc":"2.0","id":input["id"],"result":result}).to_string(),
        )
    })
    .await;
    let package = package("model-switch-mcp");
    let root = std::fs::metadata(workspace).unwrap();
    let service = ManagedServices::default()
        .admit(
            package.clone(),
            ServiceConfig {
                identity: ServiceIdentity {
                    workspace: (root.dev(), root.ino()),
                    role: "worker".into(),
                    generation: "1".into(),
                    state: "model-switch-state".into(),
                    credential_revision: "model-switch-credentials".into(),
                },
                transport: ServiceTransport::Http(HttpConfig::new(peer.endpoint.clone())),
                tools: vec![AdmittedTool {
                    metadata,
                    read_only: true,
                }],
                timeout_ms: 2_000,
                max_calls,
            },
        )
        .unwrap();
    let registration = |event, required_gate, index| {
        let mut declaration = declaration(
            if required_gate {
                "model-switch-mcp-pre"
            } else {
                "model-switch-mcp-post"
            },
            dialect,
            HandlerKind::McpTool,
        );
        declaration.required_gate = required_gate;
        declaration.identity.index = index;
        let input = if dialect == HookDialect::Claude {
            json!({
                "session_id":"${session_id}",
                "cwd":"${cwd}",
                "hook_event_name":"${hook_event_name}",
                "source":"${source}",
                "context_tokens":"${context_tokens}",
                "prompt_cache_warm":"${prompt_cache_warm}",
                "cache_ttl":"${cache_ttl}",
                "estimated_cache_write_usd":"${estimated_cache_write_usd}",
                "pricing":"${pricing}"
            })
        } else {
            json!({
                "session_id":"${session_id}",
                "cwd":"${cwd}",
                "hook_event_name":"${hook_event_name}"
            })
        };
        McpRunner::registration_for_event(
            package.clone(),
            declaration,
            event,
            McpBinding {
                service: service.clone(),
                tool: "model_switch".into(),
                input,
            },
            None,
            McpConfig::default(),
        )
        .unwrap()
    };
    (
        [
            registration(HookEvent::PreModelSwitch, true, 0),
            registration(HookEvent::PostModelSwitch, false, 1),
        ],
        peer,
        service,
    )
}

fn mcp_registration_for_service(
    package: Arc<plugins::Package>,
    service: Arc<ManagedService>,
) -> Registration {
    McpRunner::registration_for_event(
        package,
        declaration(
            "config-change-pending-mcp",
            HookDialect::Native,
            HandlerKind::McpTool,
        ),
        HookEvent::ConfigChange,
        McpBinding {
            service,
            tool: "config_change".into(),
            input: json!({
                "session_id":"${session_id}",
                "cwd":"${cwd}",
                "hook_event_name":"${hook_event_name}",
                "source":"${source}"
            }),
        },
        None,
        McpConfig::default(),
    )
    .unwrap()
}

fn pending_mcp_service(
    workspace: &std::path::Path,
    package: Arc<plugins::Package>,
    transport: ServiceTransport,
    timeout_ms: u64,
) -> Arc<ManagedService> {
    use std::os::unix::fs::MetadataExt;
    let root = std::fs::metadata(workspace).unwrap();
    ManagedServices::default()
        .admit(
            package,
            ServiceConfig {
                identity: ServiceIdentity {
                    workspace: (root.dev(), root.ino()),
                    role: "worker".into(),
                    generation: "1".into(),
                    state: "pending-config-change-state".into(),
                    credential_revision: "pending-config-change-credentials".into(),
                },
                transport,
                tools: vec![AdmittedTool {
                    metadata: json!({
                        "name":"config_change",
                        "inputSchema":{"type":"object","additionalProperties":true}
                    }),
                    read_only: true,
                }],
                timeout_ms,
                max_calls: 4,
            },
        )
        .unwrap()
}

pub async fn pending_http_mcp_registration(
    workspace: &std::path::Path,
) -> (Registration, Peer, Arc<ManagedService>) {
    pending_http_mcp_registration_delayed(workspace, std::time::Duration::from_millis(750), 2_000)
        .await
}

pub async fn pending_http_mcp_registration_delayed(
    workspace: &std::path::Path,
    call_delay: std::time::Duration,
    timeout_ms: u64,
) -> (Registration, Peer, Arc<ManagedService>) {
    let peer = Peer::new_delayed(move |_, input| {
        let delay = if input["method"] == "tools/call" {
            call_delay
        } else {
            std::time::Duration::ZERO
        };
        let result = match input["method"].as_str() {
            Some("initialize") => json!({
                "protocolVersion":"2025-11-25",
                "capabilities":{"tools":{}},
                "serverInfo":{"name":"pending-config-change","version":"1"}
            }),
            Some("tools/list") => json!({"tools":[{
                "name":"config_change",
                "inputSchema":{"type":"object","additionalProperties":true}
            }]}),
            Some("tools/call") => json!({
                "content":[],
                "structuredContent":{"decision":"approve","reason":"late approval"}
            }),
            _ => json!({}),
        };
        (
            delay,
            "application/json".into(),
            json!({"jsonrpc":"2.0","id":input["id"],"result":result}).to_string(),
        )
    })
    .await;
    let package = package("config-change-pending-http-mcp");
    let service = pending_mcp_service(
        workspace,
        package.clone(),
        ServiceTransport::Http(HttpConfig::new(peer.endpoint.clone())),
        timeout_ms,
    );
    (
        mcp_registration_for_service(package, service.clone()),
        peer,
        service,
    )
}

pub fn pending_stdio_mcp_registration(
    workspace: &std::path::Path,
    token: &str,
) -> (Registration, Arc<ManagedService>) {
    let source = tempfile::tempdir().unwrap();
    std::fs::create_dir(source.path().join(".claude-plugin")).unwrap();
    std::fs::write(
        source.path().join(".claude-plugin/plugin.json"),
        r#"{"name":"config-change-pending-stdio-mcp","version":"1.0.0"}"#,
    )
    .unwrap();
    std::fs::write(
        source.path().join("peer.py"),
        concat!(
            "import json,subprocess,sys,time\n",
            "for line in sys.stdin:\n",
            " request=json.loads(line)\n",
            " if 'id' not in request: continue\n",
            " method=request['method']\n",
            " if method=='initialize': result={'protocolVersion':'2025-11-25','capabilities':{'tools':{}},'serverInfo':{'name':'pending','version':'1'}}\n",
            " elif method=='tools/list': result={'tools':[{'name':'config_change','inputSchema':{'type':'object','additionalProperties':True}}]}\n",
            " elif method=='tools/call':\n",
            "  subprocess.Popen(['/usr/bin/python3','-c','import time; time.sleep(120)',sys.argv[1]],start_new_session=True)\n",
            "  time.sleep(120)\n",
            " else: result={}\n",
            " print(json.dumps({'jsonrpc':'2.0','id':request['id'],'result':result}),flush=True)\n",
        ),
    )
    .unwrap();
    let package =
        Arc::new(plugins::inspect(source.path(), &plugins::ImportOptions::default()).unwrap());
    let transport = ServiceTransport::Stdio(
        CommandConfig::new(CommandProgram::Argv(vec![
            "/usr/bin/python3".into(),
            "${CODEX_PLUGIN_ROOT}/peer.py".into(),
            token.into(),
        ]))
        .into(),
    );
    let service = pending_mcp_service(workspace, package.clone(), transport, 2_000);
    (
        mcp_registration_for_service(package, service.clone()),
        service,
    )
}

pub async fn model_registration(kind: HandlerKind, allow: bool) -> (Registration, Peer) {
    model_registration_delayed(kind, allow, std::time::Duration::ZERO).await
}

pub async fn model_switch_model_registration(kind: HandlerKind) -> (Registration, Peer) {
    assert!(matches!(kind, HandlerKind::Prompt | HandlerKind::Agent));
    let peer = Peer::new(|_, request| {
        assert_eq!(request["model"], "model-switch-hook-model");
        let prompt = request["input"][0]["content"].as_str().unwrap();
        assert!(prompt.contains("PreModelSwitch"));
        assert!(prompt.contains("settings"));
        let verdict = json!({"ok":true,"reason":"production model switch verdict"}).to_string();
        let events = [
            json!({"type":"response.output_text.delta","delta":verdict}),
            json!({
                "type":"response.completed",
                "response":{
                    "output":[],
                    "usage":{
                        "input_tokens":1,
                        "output_tokens":1,
                        "input_tokens_details":{"cached_tokens":0}
                    }
                }
            }),
        ];
        (
            "text/event-stream".into(),
            events
                .iter()
                .map(|event| format!("data: {event}\n\n"))
                .collect(),
        )
    })
    .await;
    let connection: Connection = serde_json::from_value(json!({
        "adapter":"openai-api",
        "model":"model-switch-hook-model",
        "endpoint":format!("{}/v1/responses", peer.endpoint),
        "api_key":"synthetic-hook-key",
        "max_output_tokens":512
    }))
    .unwrap();
    let mut declared = declaration("model-switch-model", HookDialect::Native, kind);
    declared.class = HandlerClass::DecisionGate;
    let registration = ModelRunner::registration_for_event(
        package("model-switch-model"),
        declared,
        HookEvent::PreModelSwitch,
        ModelConfig::new(
            connection,
            "Review the literal PreModelSwitch event: $ARGUMENTS".into(),
        ),
    )
    .unwrap();
    (registration, peer)
}

pub async fn model_registration_delayed(
    kind: HandlerKind,
    allow: bool,
    delay: std::time::Duration,
) -> (Registration, Peer) {
    assert!(matches!(kind, HandlerKind::Prompt | HandlerKind::Agent));
    let peer = Peer::new_delayed(move |_, request| {
        assert_eq!(request["model"], "config-change-hook-model");
        let prompt = request["input"][0]["content"].as_str().unwrap();
        assert!(prompt.contains("ConfigChange"));
        assert!(prompt.contains("user_settings"));
        assert!(!prompt.contains("private-api-key-canary"));
        assert!(!prompt.contains("private-model-canary"));
        let verdict = json!({
            "ok":allow,
            "reason":"production model verdict"
        })
        .to_string();
        let events = [
            json!({"type":"response.output_text.delta","delta":verdict}),
            json!({
                "type":"response.completed",
                "response":{
                    "output":[],
                    "usage":{
                        "input_tokens":1,
                        "output_tokens":1,
                        "input_tokens_details":{"cached_tokens":0}
                    }
                }
            }),
        ];
        (
            delay,
            "text/event-stream".into(),
            events
                .iter()
                .map(|event| format!("data: {event}\n\n"))
                .collect(),
        )
    })
    .await;
    let connection: Connection = serde_json::from_value(json!({
        "adapter":"openai-api",
        "model":"config-change-hook-model",
        "endpoint":format!("{}/v1/responses", peer.endpoint),
        "api_key":"synthetic-hook-key",
        "max_output_tokens":512
    }))
    .unwrap();
    let mut declared = declaration("config-change-model", HookDialect::Native, kind);
    declared.class = HandlerClass::DecisionGate;
    let registration = ModelRunner::registration_for_event(
        package("config-change-model"),
        declared,
        HookEvent::ConfigChange,
        ModelConfig::new(
            connection,
            "Review the literal ConfigChange event: $ARGUMENTS".into(),
        ),
    )
    .unwrap();
    (registration, peer)
}
