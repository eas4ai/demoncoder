use std::{collections::BTreeMap, path::Path, sync::Arc};

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use tokio::sync::{OwnedSemaphorePermit, Semaphore, TryAcquireError, mpsc, oneshot};

use crate::{
    config::Connection,
    events::{Envelope, Event, EventSink},
};

pub const ADAPTER_INTERFACE_VERSION: u32 = 1;

/// Native end observation and owned command cleanup share this bound.
pub(crate) const NATIVE_END_BUDGET: std::time::Duration = std::time::Duration::from_secs(5);

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
    CommandsClosed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStart {
    Startup,
    Resume,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionEnd {
    Shutdown,
    CommandsClosed,
    UiClosed,
    HostError,
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
        Some(Command::Shutdown) => return Ok(Some(TurnEnd::Shutdown)),
        None => return Ok(Some(TurnEnd::CommandsClosed)),
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
    fn native_lifetime(&self) -> bool {
        false
    }
    fn open_lifetime(&mut self, _source: SessionStart, _events: &EventSink) -> Result<()> {
        Ok(())
    }
    /// Called only by the outer host lifetime, never by resource close or a turn.
    async fn session_start(&mut self, _source: SessionStart, _events: &EventSink) -> Result<()> {
        Ok(())
    }
    async fn session_end(&mut self, _reason: SessionEnd, _events: &EventSink) -> Result<()> {
        bail!("current adapter cannot execute the native session observation")
    }
    fn observer_notification(&self) -> Result<Option<Arc<tokio::sync::Notify>>> {
        Ok(None)
    }
    fn observer_ready(&self) -> Result<bool> {
        Ok(false)
    }
    async fn observer_turn(
        &mut self,
        _commands: &mut mpsc::Receiver<Command>,
        _events: &EventSink,
    ) -> Result<TurnEnd> {
        Ok(TurnEnd::Complete)
    }

    fn owner(&self) -> &'static str;
    /// Capture defaults before acknowledging the prompt. Implementations must not
    /// start effects here; queued publication may still be cancelled.
    fn admit(&mut self, _prompt: &str) -> Result<()> {
        Ok(())
    }
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
    async fn cancel_background(&mut self) -> Result<()> {
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
    session: &mut dyn Session,
    end_reason: &mut SessionEnd,
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
                Err(error) if error.downcast_ref::<mpsc::error::SendError<Envelope>>().is_some() => { *end_reason=SessionEnd::UiClosed; Ok(Some(TurnEnd::Shutdown)) },
                Err(error) => Err(error),
            },
            command = commands.recv() => match command {
                Some(Command::Shutdown) => { *end_reason=SessionEnd::Shutdown; return Ok(Some(TurnEnd::Shutdown)); },
                None => { *end_reason=SessionEnd::CommandsClosed; return Ok(Some(TurnEnd::Shutdown)); },
                Some(Command::Cancel) => {
                    session.cancel_background().await?;
                    if allow_cancel { return Ok(Some(TurnEnd::Cancelled)); }
                },
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

/// Startup is a host observation boundary, never a prompt admission window.
async fn start_lifetime(
    session: &mut dyn Session,
    commands: &mut mpsc::Receiver<Command>,
    events: &EventSink,
) -> Result<Option<SessionEnd>> {
    if !session.native_lifetime() {
        return Ok(None);
    }
    let started = tokio::time::Instant::now();
    if let Err(error) = session.open_lifetime(SessionStart::Startup, events) {
        let _ = events.emit_advisory(Event::Error {
            message: format!("SessionStart fact could not be retained: {error:#}"),
        });
    }
    let mut cancelled = false;
    let startup = async {
        let observation = session.session_start(SessionStart::Startup, events);
        tokio::pin!(observation);
        loop {
            tokio::select! {
                biased;
                command = commands.recv() => match command {
                    Some(Command::Shutdown) => return Ok(Some(SessionEnd::Shutdown)),
                    None => return Ok(Some(SessionEnd::CommandsClosed)),
                    Some(Command::Cancel) => { cancelled = true; return Ok(None); },
                    Some(Command::Submit { reply, .. }) => {
                        let _ = reply.send(Err("Session startup in progress; draft retained."));
                    },
                    Some(Command::Prompt(_)) => {},
                },
                result = &mut observation => return result.map(|()| None),
            }
        }
    };
    let outcome =
        tokio::time::timeout_at(started + std::time::Duration::from_secs(27), startup).await;
    let interrupted = cancelled || !matches!(&outcome, Ok(Ok(None)));
    if interrupted && let Err(error) = events.cancel_lifetime_services() {
        let _ = events.lifetime_diagnostic(format!(
            "SessionStart service cancellation incomplete: {error:#}"
        ));
    }
    if let Err(error) = events
        .drain_lifetime_commands(started + std::time::Duration::from_secs(30), interrupted)
        .await
    {
        let _ = events.lifetime_diagnostic(format!(
            "SessionStart command cleanup incomplete: {error:#}"
        ));
    }
    if interrupted
        && let Err(error) = events
            .drain_lifetime_services(started + std::time::Duration::from_secs(30))
            .await
    {
        let _ = events.lifetime_diagnostic(format!(
            "SessionStart service cleanup incomplete: {error:#}"
        ));
    }
    match outcome {
        Ok(Ok(Some(reason))) => {
            let _ = events.lifetime_diagnostic(
                "SessionStart observation interrupted by session termination".into(),
            );
            return Ok(Some(reason));
        }
        Ok(Ok(None)) => {}
        outcome => {
            let _ = events
                .lifetime_diagnostic(format!("SessionStart observation unavailable: {outcome:?}"));
        }
    }
    if cancelled {
        session.cancel_background().await?;
        let _ = events.lifetime_diagnostic(
            "SessionStart observation cancelled; effects may be unknown".into(),
        );
        while let Ok(command) = commands.try_recv() {
            match command {
                Command::Submit { reply, .. } => {
                    let _ = reply.send(Err("Session startup cancelled; draft retained."));
                }
                Command::Shutdown => return Ok(Some(SessionEnd::Shutdown)),
                _ => {}
            }
        }
    }
    Ok(None)
}

/// Finish the application worker, including queue backpressure in the deadline.
/// The outer result reports timeout; the inner result preserves worker errors.
pub async fn shutdown(
    commands: mpsc::Sender<Command>,
    mut worker: tokio::task::JoinHandle<Result<()>>,
    native_lifetime: bool,
) -> Result<Result<()>> {
    let shutdown = async {
        let _ = commands.send(Command::Shutdown).await;
        (&mut worker).await.context("session runtime failed")?
    };
    let budget = std::time::Duration::from_secs(3)
        + if native_lifetime {
            NATIVE_END_BUDGET
        } else {
            std::time::Duration::ZERO
        };
    match tokio::time::timeout(budget, shutdown).await {
        Ok(result) => Ok(result),
        Err(error) => {
            worker.abort();
            let _ = worker.await;
            Err(error).context("session shutdown timed out")
        }
    }
}

pub async fn run(
    mut session: Box<dyn Session>,
    mut commands: mpsc::Receiver<Command>,
    events: EventSink,
) -> Result<()> {
    let _lifetime_owner = events.own_host_lifetime();
    let mut end_reason = SessionEnd::Shutdown;
    let result = async {
        if let Some(reason) = start_lifetime(session.as_mut(), &mut commands, &events).await? {
            end_reason = reason;
            return Ok(());
        }
        if publish_lifecycle(
            Event::Ready {
                owner: session.owner(),
            },
            false,
            &mut commands,
            &events,
            session.as_mut(),
            &mut end_reason,
        )
        .await?
        .is_some()
        {
            return Ok(());
        }
        for event in session.initial_events()? {
            if publish_lifecycle(event, false, &mut commands, &events, session.as_mut(), &mut end_reason)
                .await?
                .is_some()
            {
                return Ok(());
            }
        }
        loop {
            let notification=session.observer_notification()?;
            let observer_ready=session.observer_ready()?;
            let command=tokio::select! {
                biased;
                command=commands.recv()=>match command {Some(command)=>Some(command),None=>{end_reason=SessionEnd::CommandsClosed;break}},
                ready=async {
                    if observer_ready { return Ok::<_,anyhow::Error>(()); }
                    if let Some(notification)=notification { notification.notified().await; } else { std::future::pending::<()>().await; }
                    Ok(())
                }=>{ready?;None},
            };
            let observer_turn=command.is_none();
            if observer_turn && !session.observer_ready()? { continue; }
            let prompt = match command {
                None=>Some(String::new()),
                Some(command)=>match command {
                Command::Prompt(prompt) => match session.admit(&prompt) {
                    Ok(()) => Some(prompt),
                    Err(error) => { events.emit_advisory(Event::Error { message: error.to_string() })?; None }
                },
                Command::Submit { text, reply } => match session.admit(&text) {
                    Ok(()) => reply.send(Ok(())).ok().map(|()| text),
                    Err(error) => {
                        let _ = reply.send(Err("Assignment unavailable; draft retained. Repair Settings or the explicit override."));
                        events.emit_advisory(Event::Error { message: error.to_string() })?;
                        None
                    }
                },
                Command::Shutdown => break,
                Command::Cancel => {
                    session.cancel_background().await?;
                    None
                }
                }
            };
            let Some(prompt) = prompt else {
                continue;
            };
            let outcome = match publish_lifecycle(
                Event::TurnStarted,
                true,
                &mut commands,
                &events,
                session.as_mut(),
            &mut end_reason,
            )
            .await?
            {
                Some(end) => Ok(end),
                None if observer_turn => session.observer_turn(&mut commands, &events).await,
                None => session.turn(prompt, &mut commands, &events).await,
            };
            let status = match outcome {
                Ok(TurnEnd::Shutdown) => break,
                Ok(TurnEnd::CommandsClosed) => { end_reason=SessionEnd::CommandsClosed; break; },
                Ok(TurnEnd::Complete) => "complete",
                Ok(TurnEnd::Cancelled) => {
                    session.cancel_background().await?;
                    "cancelled"
                }
                Err(error) => {
                    if publish_lifecycle(
                        Event::Error {
                            message: error.to_string(),
                        },
                        false,
                        &mut commands,
                        &events,
                        session.as_mut(),
            &mut end_reason,
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
                session.as_mut(),
            &mut end_reason,
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
    let ended = tokio::time::Instant::now();
    commands.close();
    while let Ok(command) = commands.try_recv() {
        if let Command::Submit { reply, .. } = command {
            let _ = reply.send(Err(CORRECTION_CLOSED));
        }
    }
    drop(commands);
    if result.is_err() {
        end_reason = SessionEnd::HostError;
    }
    if let Ok(Some(selected)) = events.take_host_end() {
        end_reason = selected;
    }
    let observation = match events.end_host_lifetime(end_reason) {
        Ok(Some(lifetime_events)) => Some(
            tokio::time::timeout_at(
                ended + std::time::Duration::from_secs(2),
                session.session_end(end_reason, &lifetime_events),
            )
            .await,
        ),
        Ok(None) => None,
        Err(error) => {
            let _ = events.emit_advisory(Event::Error {
                message: format!("SessionEnd fact could not be retained: {error:#}"),
            });
            None
        }
    };
    if matches!(observation, Some(Ok(Ok(()))))
        && let Err(error) = events
            .join_lifetime_observers(ended + std::time::Duration::from_secs(2))
            .await
    {
        let _ = events.lifetime_diagnostic(format!(
            "SessionEnd asynchronous observation incomplete: {error:#}"
        ));
    }
    let finalized = events.finalize_host_lifetime();
    if let Some(observation) = observation {
        if let Err(error) = events
            .drain_lifetime_commands(ended + NATIVE_END_BUDGET, true)
            .await
        {
            let _ = events
                .lifetime_diagnostic(format!("SessionEnd command cleanup incomplete: {error:#}"));
        }
        if !matches!(observation, Ok(Ok(()))) {
            let _ = events.lifetime_diagnostic(format!(
                "SessionEnd observation unavailable: {observation:?}"
            ));
        }
    }
    if let Err(error) = events
        .drain_lifetime_services(ended + NATIVE_END_BUDGET)
        .await
    {
        let _ =
            events.lifetime_diagnostic(format!("SessionEnd service cleanup incomplete: {error:#}"));
    }
    let close = session.close().await;
    result.and(finalized).and(close)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn lifecycle_backpressure_does_not_discard_background_cancellation() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        struct Background {
            stopped: Arc<AtomicBool>,
            returned: Arc<AtomicBool>,
        }
        #[async_trait]
        impl Session for Background {
            fn owner(&self) -> &'static str {
                "background-test"
            }
            async fn turn(
                &mut self,
                _: String,
                _: &mut mpsc::Receiver<Command>,
                events: &EventSink,
            ) -> Result<TurnEnd> {
                events
                    .emit(Event::Text {
                        text: "Fill the held terminal queue".into(),
                    })
                    .await?;
                self.returned.store(true, Ordering::SeqCst);
                Ok(TurnEnd::Complete)
            }
            async fn cancel_background(&mut self) -> Result<()> {
                self.stopped.store(true, Ordering::SeqCst);
                Ok(())
            }
            async fn close(&mut self) -> Result<()> {
                self.cancel_background().await
            }
        }
        let stopped = Arc::new(AtomicBool::new(false));
        let returned = Arc::new(AtomicBool::new(false));
        let effects = Arc::new(AtomicUsize::new(0));
        let child = tokio::spawn({
            let stopped = stopped.clone();
            let effects = effects.clone();
            async move {
                while !stopped.load(Ordering::SeqCst) {
                    effects.fetch_add(1, Ordering::SeqCst);
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                }
            }
        });
        let (commands, receiver) = mpsc::channel(4);
        let (sender, mut events) = mpsc::channel(1);
        let owner = tokio::spawn(run(
            Box::new(Background {
                stopped: stopped.clone(),
                returned: returned.clone(),
            }),
            receiver,
            EventSink::new("held".into(), sender, None).unwrap(),
        ));
        assert!(matches!(
            events.recv().await.unwrap().event,
            Event::Ready { .. }
        ));
        commands
            .send(Command::Prompt("start".into()))
            .await
            .unwrap();
        assert!(matches!(
            events.recv().await.unwrap().event,
            Event::TurnStarted
        ));
        while !returned.load(Ordering::SeqCst) {
            tokio::task::yield_now().await;
        }
        commands.send(Command::Cancel).await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while !stopped.load(Ordering::SeqCst) {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("held lifecycle publication discarded cancellation");
        child.await.unwrap();
        let before = effects.load(Ordering::SeqCst);
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        assert_eq!(effects.load(Ordering::SeqCst), before);
        commands.send(Command::Shutdown).await.unwrap();
        owner.await.unwrap().unwrap();
    }

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
