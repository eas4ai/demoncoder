use demoncoder::{
    events::{Event, EventSink},
    native::{Model, NativeSession},
    plugins::{dispatch::Registration, hook_types::HookEvent, non_tool::NonToolPlan},
    session::Command,
    tools::{AccessPolicy, ToolCall, ToolExecutor, ToolResult},
    workflow::{allocation::Limits, runtime::SharedRuntime, workspace::CaptureScope},
};
use std::{sync::Arc, time::Duration};
use tokio::sync::mpsc;

pub struct Host {
    pub root: tempfile::TempDir,
    pub runtime: SharedRuntime,
    pub events: EventSink,
    pub credentials: Vec<std::path::PathBuf>,
    receiver: mpsc::Receiver<demoncoder::events::Envelope>,
}
struct NoPrompt;
#[async_trait::async_trait]
impl Model for NoPrompt {
    fn prompt(&mut self, _: String) {
        panic!("no prompt was submitted")
    }
    fn results(&mut self, _: Vec<ToolResult>) {
        panic!("no model tool work")
    }
    async fn response(&mut self, _: &EventSink) -> anyhow::Result<Vec<ToolCall>> {
        panic!("session transport borrowed model execution")
    }
}
impl Host {
    pub fn new(granted: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let connection =
            serde_json::from_value(serde_json::json!({"adapter":"openai-api"})).unwrap();
        let limits = Limits {
            seconds: 60,
            model_calls: 1,
            tool_calls: 1,
        };
        let (runtime, _) = SharedRuntime::open_with_session_hooks(
            root.path(),
            &connection,
            None,
            &CaptureScope::default(),
            granted.then_some(&limits),
        )
        .unwrap();
        let (sender, receiver) = mpsc::channel(256);
        let events = EventSink::new("native transport".into(), sender, None)
            .unwrap()
            .with_runtime(runtime.clone());
        Self {
            root,
            runtime,
            events,
            credentials: vec![],
            receiver,
        }
    }
    pub async fn start(&mut self, plans: Vec<(HookEvent, Vec<Registration>)>) -> Running {
        let running = self.launch(plans);
        self.ready().await;
        running
    }
    pub fn launch(&self, plans: Vec<(HookEvent, Vec<Registration>)>) -> Running {
        let policy = AccessPolicy {
            supervisor: Some(env!("CARGO_BIN_EXE_demoncoder").into()),
            credential_paths: self.credentials.clone(),
            ..Default::default()
        };
        let mut tools = ToolExecutor::with_policy(self.root.path(), &policy).unwrap();
        for (event, registrations) in plans {
            tools
                .register_non_tool_plan(Arc::new(NonToolPlan::new(event, registrations).unwrap()))
                .unwrap();
        }
        let session = NativeSession::with_tools(Box::new(NoPrompt), tools);
        let (commands, receiver) = mpsc::channel(8);
        let owner = tokio::spawn(demoncoder::session::run(
            Box::new(session),
            receiver,
            self.events.clone(),
        ));
        Running { commands, owner }
    }
    pub async fn ready(&mut self) {
        tokio::time::timeout(Duration::from_secs(5), async {
            while !matches!(
                self.receiver
                    .recv()
                    .await
                    .expect("host closed before ready")
                    .event,
                Event::Ready { .. }
            ) {}
        })
        .await
        .expect("idle service pinned startup drain");
    }
    pub fn assert_unspent(&self) {
        let record = self.runtime.record().unwrap();
        assert_eq!(record.backend_invocations, 0);
        if let Some(grant) = record.session_hook_allowance {
            assert_eq!(
                (
                    grant.allocation.model_calls,
                    grant.allocation.tool_calls,
                    grant.backend_invocations
                ),
                (0, 0, 0)
            );
        }
    }
}
impl Drop for Host {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.runtime.directory().unwrap());
    }
}
pub struct Running {
    pub commands: mpsc::Sender<Command>,
    pub owner: tokio::task::JoinHandle<anyhow::Result<()>>,
}
impl Running {
    pub async fn stop(mut self) {
        self.commands.send(Command::Shutdown).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), &mut self.owner)
            .await
            .expect("native end exceeded whole boundary")
            .unwrap()
            .unwrap();
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        self.owner.abort();
    }
}
