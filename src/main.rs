use anyhow::{Context, Result};
use clap::Parser;
use demoncoder::{
    adapters,
    config::Args,
    events::EventSink,
    session::{self, Command},
    startup, terminal,
};
use std::time::Duration;
use tokio::sync::mpsc;

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    startup::prepare(&args)?;
    let selection = args.selection()?;
    let session = adapters::builtins()?.open(&selection.connection, &selection.workspace)?;
    let (command_tx, command_rx) = mpsc::channel(16);
    let (event_tx, event_rx) = mpsc::channel(256);
    let sink = EventSink::new(selection.name.clone(), event_tx, args.event_log.as_deref())?;
    let worker = tokio::spawn(session::run(session, command_rx, sink));
    let label = format!(
        "{} · {}",
        selection.name,
        if args.yolo {
            "HOST ACCESS · Oracle outside guard"
        } else {
            "Project writes · normal reads/network"
        }
    );
    let ui_result = terminal::run(&label, command_tx.clone(), event_rx).await;
    let _ = command_tx.send(Command::Shutdown).await;
    let worker_result = tokio::time::timeout(Duration::from_secs(3), worker)
        .await
        .context("session shutdown timed out")?
        .context("session runtime failed")?;
    ui_result.and(worker_result)
}
