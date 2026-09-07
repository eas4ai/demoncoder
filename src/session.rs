use std::{collections::BTreeMap, path::Path, sync::Arc};

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use tokio::sync::{OwnedSemaphorePermit, Semaphore, TryAcquireError, mpsc, oneshot};

use crate::{
    config::Connection,
    events::{Envelope, Event, EventSink},
};

pub const ADAPTER_INTERFACE_VERSION: u32 = 1;

/// Result sent when the session owner accepts or rejects a submitted draft.
pub type PromptAdmission = std::result::Result<(), &'static str>;
pub(crate) const CORRECTION_CAPACITY: usize = 32;
pub(crate) const CORRECTION_REJECTION: &str =
    "Correction queue is full; draft retained. Wait for a tool boundary and try again.";
pub(crate) const CORRECTION_CLOSED: &str =
    "Session ended before prompt admission; draft retained. Start a new turn and try again.";
const CORRECTION_ACCEPTED: &str = "\n[Correction queued for the next tool boundary]\n";

pub enum Command {
    Prompt(String),
    /// Submit a draft while retaining it in the editor until the session owner
    /// replies. The owner replies before awaiting terminal event publication.
    Submit {
        text: String,
        reply: oneshot::Sender<PromptAdmission>,
    },
    Cancel,
    Shutdown,
}

#[derive(PartialEq, Eq)]
pub enum TurnEnd {
    Complete,
    Cancelled,
    Shutdown,
}

pub(crate) struct Correction {
    text: String,
    _slot: OwnedSemaphorePermit,
}

#[derive(Clone)]
pub(crate) struct CorrectionSender {
    sender: mpsc::Sender<Correction>,
    slots: Arc<Semaphore>,
}

pub(crate) fn correction_channel() -> (CorrectionSender, mpsc::Receiver<Correction>) {
    let (sender, receiver) = mpsc::channel(CORRECTION_CAPACITY);
    (
        CorrectionSender {
            sender,
            slots: Arc::new(Semaphore::new(CORRECTION_CAPACITY)),
        },
        receiver,
    )
}

pub(crate) fn correction_prompt(corrections: Vec<Correction>) -> String {
    corrections
        .into_iter()
        .map(|correction| correction.text)
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub(crate) fn relay_command(
    command: Option<Command>,
    corrections: &CorrectionSender,
    events: &EventSink,
) -> Result<Option<TurnEnd>> {
    let (text, reply) = match command {
        Some(Command::Cancel) => return Ok(Some(TurnEnd::Cancelled)),
        Some(Command::Shutdown) | None => return Ok(Some(TurnEnd::Shutdown)),
        Some(Command::Prompt(text)) => (text, None),
        Some(Command::Submit { text, reply }) => (text, Some(reply)),
    };
    if crate::workflow::is_control(&text) {
        if let Some(reply) = reply {
            let _ = reply.send(Err(crate::workflow::BUSY_CONTROL));
        } else {
            events.emit_advisory(Event::Error {
                message: crate::workflow::BUSY_CONTROL.into(),
            })?;
        }
        return Ok(None);
    }
    let slot = match corrections.slots.clone().try_acquire_owned() {
        Ok(slot) => slot,
        Err(TryAcquireError::NoPermits) => {
            if let Some(reply) = reply {
                let _ = reply.send(Err(CORRECTION_REJECTION));
            }
            events.emit_advisory(Event::Error {
                message: CORRECTION_REJECTION.into(),
            })?;
            return Ok(None);
        }
        Err(TryAcquireError::Closed) => {
            if let Some(reply) = reply {
                let _ = reply.send(Err(CORRECTION_CLOSED));
            }
            return Ok(Some(TurnEnd::Shutdown));
        }
    };
    match corrections.sender.try_reserve() {
        Ok(permit) => {
            let acknowledged = reply.is_none_or(|reply| reply.send(Ok(())).is_ok());
            if acknowledged {
                permit.send(Correction { text, _slot: slot });
                events.emit_advisory(Event::Text {
                    text: CORRECTION_ACCEPTED.into(),
                })?;
            }
        }
        Err(mpsc::error::TrySendError::Full(_)) => {
            if let Some(reply) = reply {
                let _ = reply.send(Err(CORRECTION_REJECTION));
            }
            events.emit_advisory(Event::Error {
                message: CORRECTION_REJECTION.into(),
            })?;
        }
        Err(mpsc::error::TrySendError::Closed(_)) => {
            if let Some(reply) = reply {
                let _ = reply.send(Err(CORRECTION_CLOSED));
            }
            return Ok(Some(TurnEnd::Shutdown));
        }
    }
    Ok(None)
}

#[async_trait]
pub trait Session: Send {
    fn owner(&self) -> &'static str;
    fn supports_workflow(&self) -> bool {
        false
    }
    fn initial_events(&self) -> Result<Vec<Event>> {
        Ok(Vec::new())
    }
    fn checkpoint(&self) -> Option<serde_json::Value> {
        None
    }
    fn settle_interruption(&mut self) -> Result<()> {
        Ok(())
    }
    fn restore(
        &mut self,
        _checkpoint: &serde_json::Value,
        _results: &[crate::tools::ToolResult],
    ) -> Result<()> {
        bail!("this adapter cannot restore its internal conversation")
    }
    async fn turn(
        &mut self,
        prompt: String,
        commands: &mut mpsc::Receiver<Command>,
        events: &EventSink,
    ) -> Result<TurnEnd>;
    async fn close(&mut self) -> Result<()> {
        Ok(())
    }
}

pub type Factory = fn(&Connection, &Path) -> Result<Box<dyn Session>>;

/// Controls a registered adapter promises to provide for a coding session.
#[derive(Clone, Copy, Debug, Default)]
pub struct SessionCapabilities {
    pub read: bool,
    pub write: bool,
    pub edit: bool,
    pub bash: bool,
    pub steering: bool,
    pub cancellation: bool,
}

impl SessionCapabilities {
    pub const CODING_SESSION: Self = Self {
        read: true,
        write: true,
        edit: true,
        bash: true,
        steering: true,
        cancellation: true,
    };

    fn require_coding_session(self) -> Result<()> {
        for (name, supported) in [
            ("read", self.read),
            ("write", self.write),
            ("edit", self.edit),
            ("Bash", self.bash),
            ("steering", self.steering),
            ("cancellation", self.cancellation),
        ] {
            if !supported {
                bail!(
                    "selected adapter does not support {name}, which this coding session requires"
                );
            }
        }
        Ok(())
    }
}

struct RegisteredAdapter {
    factory: Factory,
    capabilities: SessionCapabilities,
}

#[derive(Default)]
pub struct Registry {
    adapters: BTreeMap<String, RegisteredAdapter>,
}

impl Registry {
    pub fn register(&mut self, name: &str, version: u32, factory: Factory) -> Result<()> {
        // The original version-1 registration promises the entire session contract.
        self.register_with_capabilities(name, version, SessionCapabilities::CODING_SESSION, factory)
    }

    pub fn register_with_capabilities(
        &mut self,
        name: &str,
        version: u32,
        capabilities: SessionCapabilities,
        factory: Factory,
    ) -> Result<()> {
        if version != ADAPTER_INTERFACE_VERSION {
            bail!("unsupported adapter interface version");
        }
        if self.adapters.contains_key(name) {
            bail!("adapter is already registered");
        }
        self.adapters.insert(
            name.to_owned(),
            RegisteredAdapter {
                factory,
                capabilities,
            },
        );
        Ok(())
    }

    pub fn open(&self, config: &Connection, workspace: &Path) -> Result<Box<dyn Session>> {
        let adapter = self
            .adapters
            .get(&config.adapter)
            .context("unknown adapter")?;
        adapter.capabilities.require_coding_session()?;
        (adapter.factory)(config, workspace)
    }
}

/// Lifecycle publication must keep draining controls too. A held terminal can
/// delay display, but cannot keep the owner alive after shutdown or admit more
/// prompts into an unbounded holding area.
async fn publish_lifecycle(
    event: Event,
    allow_cancel: bool,
    commands: &mut mpsc::Receiver<Command>,
    events: &EventSink,
) -> Result<Option<TurnEnd>> {
    let publication = events.emit(event);
    tokio::pin!(publication);
    loop {
        tokio::select! {
            biased;
            result = &mut publication => return match result {
                Ok(()) => Ok(None),
                // The UI closes its receiver on quit; that is a cleanup request,
                // while a retained-log error must still be reported.
                Err(error) if error.downcast_ref::<mpsc::error::SendError<Envelope>>().is_some() => Ok(Some(TurnEnd::Shutdown)),
                Err(error) => Err(error),
            },
            command = commands.recv() => match command {
                Some(Command::Shutdown) | None => return Ok(Some(TurnEnd::Shutdown)),
                Some(Command::Cancel) if allow_cancel => return Ok(Some(TurnEnd::Cancelled)),
                Some(Command::Cancel) => {},
                Some(command) => {
                    const REASON: &str = "Session is waiting for terminal output; draft retained. Try again shortly.";
                    let report = match command {
                        Command::Submit { reply, .. } => reply.send(Err(REASON)).is_ok(),
                        Command::Prompt(_) => true,
                        _ => unreachable!("control commands handled above"),
                    };
                    if report {
                        events.emit_advisory(Event::Error { message: REASON.into() })?;
                    }
                },
            },
        }
    }
}

pub async fn run(
    mut session: Box<dyn Session>,
    mut commands: mpsc::Receiver<Command>,
    events: EventSink,
) -> Result<()> {
    let result = async {
        if publish_lifecycle(
            Event::Ready {
                owner: session.owner(),
            },
            false,
            &mut commands,
            &events,
        )
        .await?
        .is_some()
        {
            return Ok(());
        }
        for event in session.initial_events()? {
            if publish_lifecycle(event, false, &mut commands, &events)
                .await?
                .is_some()
            {
                return Ok(());
            }
        }
        while let Some(command) = commands.recv().await {
            let prompt = match command {
                Command::Prompt(prompt) => Some(prompt),
                Command::Submit { text, reply } => reply.send(Ok(())).ok().map(|()| text),
                Command::Shutdown => break,
                Command::Cancel => None,
            };
            let Some(prompt) = prompt else {
                continue;
            };
            let outcome =
                match publish_lifecycle(Event::TurnStarted, true, &mut commands, &events).await? {
                    Some(end) => Ok(end),
                    None => session.turn(prompt, &mut commands, &events).await,
                };
            let status = match outcome {
                Ok(TurnEnd::Shutdown) => break,
                Ok(TurnEnd::Complete) => "complete",
                Ok(TurnEnd::Cancelled) => "cancelled",
                Err(error) => {
                    if publish_lifecycle(
                        Event::Error {
                            message: error.to_string(),
                        },
                        false,
                        &mut commands,
                        &events,
                    )
                    .await?
                    .is_some()
                    {
                        break;
                    }
                    "failed"
                }
            };
            if publish_lifecycle(
                Event::TurnFinished { status },
                false,
                &mut commands,
                &events,
            )
            .await?
            .is_some()
            {
                break;
            }
        }
        Ok(())
    }
    .await;
    let close = session.close().await;
    result.and(close)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn closed_correction_handoff_returns_a_specific_admission_reason() {
        let (corrections, receiver) = correction_channel();
        drop(receiver);
        let (event_tx, _event_rx) = mpsc::channel(1);
        let events = EventSink::new("closed".into(), event_tx, None).unwrap();
        let (reply, admission) = oneshot::channel();

        let end = relay_command(
            Some(Command::Submit {
                text: "retained draft".into(),
                reply,
            }),
            &corrections,
            &events,
        )
        .unwrap();

        assert!(matches!(end, Some(TurnEnd::Shutdown)));
        assert_eq!(admission.await.unwrap(), Err(CORRECTION_CLOSED));
    }
}
