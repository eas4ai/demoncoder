//! A separately registered provider composed with the unchanged host and UI.
use std::{path::Path, time::Duration};

use anyhow::{Result, ensure};
use async_trait::async_trait;
use clap::Parser;
use demoncoder::{
    adapters,
    config::{Args, Connection},
    events::{Event, EventSink},
    native::{Model, NativeSession},
    session::{self, ADAPTER_INTERFACE_VERSION, Command, Session},
    startup, terminal,
    tools::{ToolCall, ToolResult},
};
use serde_json::json;
use tokio::sync::mpsc;

#[derive(Default)]
struct IndependentModel {
    prompt: String,
    result: Option<ToolResult>,
}

#[async_trait]
impl Model for IndependentModel {
    fn prompt(&mut self, text: String) {
        self.prompt = text;
        self.result = None;
    }

    fn results(&mut self, mut results: Vec<ToolResult>) {
        assert_eq!(results.len(), 1);
        self.result = results.pop();
    }

    async fn response(&mut self, events: &EventSink) -> Result<Vec<ToolCall>> {
        if let Some(result) = &self.result {
            ensure!(result.success && result.call_id == "independent-read");
            events
                .emit(Event::Text {
                    text: format!("REGISTERED-{}-{}", self.prompt, result.output.trim()),
                })
                .await?;
            Ok(Vec::new())
        } else {
            Ok(vec![ToolCall {
                id: "independent-read".into(),
                name: "read".into(),
                arguments: json!({"path":"seed.txt"}),
            }])
        }
    }
}

fn open(config: &Connection, workspace: &Path) -> Result<Box<dyn Session>> {
    ensure!(
        config.model.as_deref() == Some("independent-model"),
        "fixture model assignment was lost"
    );
    Ok(Box::new(NativeSession::new(
        Box::<IndependentModel>::default(),
        workspace,
    )?))
}

#[tokio::test]
#[ignore = "driven through a pseudo-terminal by tests/registry.py"]
async fn registered_provider_uses_normal_config_loop_and_terminal() -> Result<()> {
    let args = Args::try_parse_from([
        "registry-fixture".to_owned(),
        "--trust-workspace".into(),
        "--config".into(),
        std::env::var("REGISTRY_CONFIG")?,
        "--workspace".into(),
        std::env::var("REGISTRY_WORKSPACE")?,
        "--event-log".into(),
        std::env::var("REGISTRY_EVENTS")?,
    ])?;
    startup::prepare(&args).await?;
    let selection = args.selection()?;
    let mut registry = adapters::builtins()?;
    if std::env::var_os("REGISTRY_OMIT_REGISTRATION").is_none() {
        registry.register("independent", ADAPTER_INTERFACE_VERSION, open)?;
    }
    let session = registry.open(&selection.connection, &selection.workspace)?;
    let (command_tx, command_rx) = mpsc::channel(16);
    let (event_tx, event_rx) = mpsc::channel(256);
    let events = EventSink::new(selection.name.clone(), event_tx, args.event_log.as_deref())?;
    let worker = tokio::spawn(session::run(session, command_rx, events));
    let ui = terminal::run(&selection.name, command_tx.clone(), event_rx).await;
    let _ = command_tx.send(Command::Shutdown).await;
    tokio::time::timeout(Duration::from_secs(3), worker).await???;
    ui?;
    Ok(())
}
