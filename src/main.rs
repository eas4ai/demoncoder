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
    if let Some(script) = &args.supervise_bash {
        std::process::exit(demoncoder::supervisor::run(script).await?);
    }
    startup::prepare(&args).await?;
    let live_settings = demoncoder::settings::Handle::open(&args)?;
    let mut selection = args.selection()?;
    let settings = args.workflow_settings()?;
    let (runtime, resumed) = demoncoder::workflow::runtime::SharedRuntime::open(
        &selection.workspace,
        &selection.connection,
        args.resume.as_deref(),
    )?;
    let agent_settings = args.agent_settings()?;
    anyhow::ensure!(
        runtime.record()?.delegation.is_none() || agent_settings.is_some(),
        "resume requires the original --agent-connection settings"
    );
    let manager = agent_settings
        .map(|settings| {
            demoncoder::subagents::manager::Manager::new_with_settings(
                selection.workspace.clone(),
                settings,
                runtime.clone(),
                Some(live_settings.clone()),
            )
        })
        .transpose()?;
    if let Some(manager) = &manager {
        selection.connection.access.extension = Some(manager.extension());
    }
    let session = adapters::builtins()?.open(&selection.connection, &selection.workspace)?;
    let session = Box::new(
        demoncoder::workflow::WorkflowSession::new(
            session,
            selection.connection.clone(),
            selection.workspace.clone(),
            settings,
            runtime.clone(),
            resumed,
        )?
        .with_live_settings(live_settings.clone()),
    );
    let session: Box<dyn session::Session> = match manager {
        Some(manager) => Box::new(demoncoder::subagents::session::DelegatingSession::new(
            session, manager,
        )),
        None => session,
    };
    let (command_tx, command_rx) = mpsc::channel(16);
    let (event_tx, event_rx) = mpsc::channel(256);
    let sink = EventSink::new(selection.name.clone(), event_tx, args.event_log.as_deref())?
        .with_runtime(runtime.clone());
    let mut worker = tokio::spawn(session::run(session, command_rx, sink));
    let label = format!(
        "{} · {}",
        selection.name,
        if args.yolo {
            "HOST ACCESS · Oracle outside guard"
        } else {
            "Project writes · normal reads/network"
        }
    );
    let ui_result = terminal::run_with_settings(
        &label,
        command_tx.clone(),
        event_rx,
        demoncoder::status::DisplayOptions {
            model: selection.connection.model.clone(),
            workspace: Some(selection.workspace.clone()),
            context_window: args.context_window,
        },
        runtime,
        live_settings,
    )
    .await;
    // Queue submission belongs inside the deadline too: a stopped consumer must
    // not trap quit before the cleanup timeout even starts.
    let shutdown = async {
        let _ = command_tx.send(Command::Shutdown).await;
        (&mut worker).await.context("session runtime failed")?
    };
    let worker_result = match tokio::time::timeout(Duration::from_secs(3), shutdown).await {
        Ok(result) => result,
        Err(error) => {
            worker.abort();
            let _ = worker.await;
            return Err(error).context("session shutdown timed out");
        }
    };
    ui_result.and(worker_result)
}
