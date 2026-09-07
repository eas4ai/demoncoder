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

#[tokio::test]
#[ignore = "driven through a pseudo-terminal by tests/reliability_queues.py"]
async fn terminal_with_delayed_admission() -> Result<()> {
    let (commands, mut received_commands) = mpsc::channel(1);
    let (events, received_events) = mpsc::channel(4);
    let rejected = std::env::var("QUEUE_ADMISSION")? == "rejected";
    let runtime = tokio::spawn(async move {
        let mut count = 0;
        while let Some(command) = received_commands.recv().await {
            if let Command::Submit { text, reply } = command {
                count += 1;
                if count == 1 {
                    std::fs::write("admission-request", &text).unwrap();
                    while !std::path::Path::new("release-admission").exists() {
                        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                    }
                }
                if count == 1 && rejected {
                    let _ = reply.send(Err("Correction queue is full; draft retained."));
                } else {
                    let _ = reply.send(Ok(()));
                    std::fs::write(format!("accepted-{count}"), &text).unwrap();
                    for event in [
                        Event::TurnStarted,
                        Event::Text {
                            text: "ADMITTED-REPLY".into(),
                        },
                        Event::TurnFinished { status: "complete" },
                    ] {
                        if events
                            .send(Envelope {
                                connection: "admission-fixture".into(),
                                event,
                            })
                            .await
                            .is_err()
                        {
                            return;
                        }
                    }
                }
            }
        }
    });
    let result = terminal::run("admission-fixture", commands, received_events).await;
    runtime.abort();
    let _ = runtime.await;
    result
}
