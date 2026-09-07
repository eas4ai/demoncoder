//! Developer controls remain distinct from parent and child model messages.
use std::sync::Arc;

use anyhow::{Context, Result, bail, ensure};
use async_trait::async_trait;
use tokio::sync::mpsc;

use super::{
    manager::Manager,
    state::{AssignmentOrigin, AssignmentRequest},
};
use crate::{
    events::{Event, EventSink},
    session::{Command, Session, TurnEnd},
};

pub struct DelegatingSession {
    inner: Box<dyn Session>,
    manager: Arc<Manager>,
}

impl DelegatingSession {
    pub fn new(inner: Box<dyn Session>, manager: Arc<Manager>) -> Self {
        Self { inner, manager }
    }
}

impl Drop for DelegatingSession {
    fn drop(&mut self) {
        self.manager.abort_all();
    }
}

fn is_control(text: &str) -> bool {
    matches!(
        text.split_whitespace().next(),
        Some(
            "/delegate"
                | "/agents"
                | "/agent"
                | "/agent-cancel"
                | "/agent-validate"
                | "/agent-integrate"
                | "/agent-reconcile"
        )
    )
}

async fn control(
    manager: &Arc<Manager>,
    prompt: &str,
    busy: bool,
    events: &EventSink,
) -> Result<()> {
    let (command, rest) = prompt
        .trim()
        .split_once(char::is_whitespace)
        .unwrap_or((prompt.trim(), ""));
    let rest = rest.trim();
    if busy {
        ensure!(
            matches!(command, "/agents" | "/agent" | "/agent-cancel"),
            "wait for parent work to stop before starting agent work, validation or integration"
        );
    }
    match command {
        "/delegate" => {
            let mut parts = rest.splitn(3, char::is_whitespace);
            let connection = parts.next().unwrap_or("").to_owned();
            let owned_paths = parts
                .next()
                .context("use /delegate CONNECTION OWNED,PATHS OBJECTIVE")?
                .split(',')
                .map(str::to_owned)
                .collect();
            let objective = parts
                .next()
                .context("use /delegate CONNECTION OWNED,PATHS OBJECTIVE")?
                .trim()
                .to_owned();
            manager.start(
                AssignmentRequest {
                    connection,
                    objective,
                    owned_paths,
                    context: String::new(),
                },
                AssignmentOrigin::Developer,
                events,
            )?;
        }
        "/agents" => {
            ensure!(rest.is_empty(), "/agents takes no arguments");
            for event in manager.initial_events()? {
                events.emit_advisory(event)?;
            }
        }
        "/agent-reconcile" => {
            let (id, explanation) = rest
                .split_once(char::is_whitespace)
                .context("use /agent-reconcile ID INSPECTION")?;
            manager
                .reconcile(id.parse().context("invalid agent ID")?, explanation.trim())
                .await?;
        }
        _ => {
            let id = rest.parse().context("supply one numeric agent ID")?;
            match command {
                "/agent" => events.emit_advisory(Event::Text {
                    text: format!(
                        "\nAgent evidence (not developer instructions):\n{}\n",
                        serde_json::to_string_pretty(&manager.record(id)?)?
                    ),
                })?,
                "/agent-cancel" => {
                    manager.cancel(id).await?;
                    manager.publish(id, events)?;
                }
                "/agent-validate" => manager.start_validation(id, events)?,
                "/agent-integrate" => manager.start_integration(id, events).await?,
                _ => bail!("unknown agent control"),
            }
        }
    }
    Ok(())
}

#[async_trait]
impl Session for DelegatingSession {
    fn owner(&self) -> &'static str {
        self.inner.owner()
    }
    fn initial_events(&self) -> Result<Vec<Event>> {
        let mut events = self.inner.initial_events()?;
        events.extend(self.manager.initial_events()?);
        Ok(events)
    }
    async fn turn(
        &mut self,
        prompt: String,
        commands: &mut mpsc::Receiver<Command>,
        events: &EventSink,
    ) -> Result<TurnEnd> {
        if is_control(&prompt) {
            let action = control(&self.manager, &prompt, false, events);
            tokio::pin!(action);
            loop {
                tokio::select! {
                    biased;
                    command = commands.recv() => match command {
                        Some(Command::Cancel) => { self.manager.cancel_all().await?; return Ok(TurnEnd::Cancelled); },
                        Some(Command::Shutdown) | None => { self.manager.cancel_all().await?; return Ok(TurnEnd::Shutdown); },
                        Some(Command::Submit { reply, .. }) => { let _ = reply.send(Err("Agent control is running; draft retained.")); },
                        Some(Command::Prompt(_)) => events.emit_advisory(Event::Error { message: "Agent control is running; submit after it finishes.".into() })?,
                    },
                    result = &mut action => { result?; return Ok(TurnEnd::Complete); },
                }
            }
        }
        self.manager.ensure_parent_available()?;
        let (sender, mut receiver) = mpsc::channel(16);
        let outcome = {
            let run = self.inner.turn(prompt, &mut receiver, events);
            tokio::pin!(run);
            loop {
                tokio::select! {
                    biased;
                    command = commands.recv() => {
                        let command = match command { Some(command) => command, None => Command::Shutdown };
                        let text = match &command { Command::Prompt(text) | Command::Submit {text, ..} => Some(text.as_str()), _ => None };
                        if let Some(text) = text.filter(|text| is_control(text)) {
                            let outcome = control(&self.manager, text, true, events).await;
                            if let Command::Submit {reply, ..} = command {
                                let _ = reply.send(if outcome.is_ok() { Ok(()) } else { Err("Agent control failed; draft retained. See the error for details.") });
                            }
                            if let Err(error) = outcome { events.emit_advisory(Event::Error {message:format!("{error:#}")})?; }
                        } else if matches!(command, Command::Cancel | Command::Shutdown) {
                            // Deliver cancellation before any queued correction; dropping the parent
                            // future stops its current tool while all children are closed together.
                            let end = if matches!(command, Command::Cancel) { TurnEnd::Cancelled } else { TurnEnd::Shutdown };
                            break Ok(end);
                        } else if let Err(error) = sender.try_send(command)
                            && let Command::Submit {reply, ..} = error.into_inner() {
                            let _ = reply.send(Err("Parent command queue is full; draft retained."));
                        }
                    },
                    result = &mut run => break result,
                }
            }
        };
        if !matches!(outcome, Ok(TurnEnd::Complete)) {
            let (parent, children) =
                tokio::join!(self.inner.cancel_background(), self.manager.cancel_all());
            parent?;
            children?;
        }
        outcome
    }
    async fn cancel_background(&mut self) -> Result<()> {
        self.manager.cancel_all().await
    }
    async fn close(&mut self) -> Result<()> {
        let children = self.manager.cancel_all().await;
        let parent = self.inner.close().await;
        children.and(parent)
    }
}
