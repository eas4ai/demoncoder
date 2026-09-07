//! Real terminal input with a deliberately held command consumer.
use anyhow::Result;
use demoncoder::{
    events::{Envelope, Event},
    session::Command,
    terminal,
};
use tokio::sync::mpsc;

#[tokio::test]
#[ignore = "driven through a pseudo-terminal by tests/reliability_queues.py"]
async fn terminal_with_full_command_queue() -> Result<()> {
    let (commands, held_consumer) = mpsc::channel(1);
    commands.try_send(Command::Prompt("already queued".into()))?;
    let (events, received) = mpsc::channel(4);
    events
        .send(Envelope {
            connection: "held-consumer".into(),
            event: Event::TurnStarted,
        })
        .await?;
    let result = terminal::run("held-consumer", commands, received).await;
    drop(held_consumer);
    drop(events);
    result
}
