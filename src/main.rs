use anyhow::Result;
use clap::Parser;
use demoncoder::{adapters, config::Args, events::EventSink, session, startup, terminal};
use tokio::sync::mpsc;

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    if let Some(spec) = &args.supervise_backend {
        return adapters::backend_supervisor::run(spec).await;
    }
    if let Some(address) = args
        .codex_compaction_relay
        .as_ref()
        .or(args.codex_ordinary_relay.as_ref())
    {
        return demoncoder::plugins::codex_relay::run(address).await;
    }
    if let Some(script) = &args.supervise_bash {
        std::process::exit(demoncoder::supervisor::run(script).await?);
    }
    if let Some(spec) = &args.supervise_hook {
        std::process::exit(demoncoder::supervisor::run_hook(spec).await?);
    }
    let session_hook_limits = args.session_hook_limits()?;
    startup::prepare(&args).await?;
    let live_settings = demoncoder::settings::Handle::open(&args)?;
    let mut selection = args.selection()?;
    let settings = args.workflow_settings()?;
    let (runtime, resumed) = demoncoder::workflow::runtime::SharedRuntime::open_with_session_hooks(
        &selection.workspace,
        &selection.connection,
        args.resume.as_deref(),
        &settings.capture_scope,
        session_hook_limits.as_ref(),
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
    let native_lifetime = session.native_lifetime();
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
    let worker_result = session::shutdown(command_tx, worker, native_lifetime).await?;
    ui_result.and(worker_result)
}
