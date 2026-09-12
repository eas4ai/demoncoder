use std::{
    future::pending,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use anyhow::Result;
use async_trait::async_trait;
use demoncoder::{
    adapters,
    config::Connection,
    events::{Envelope, Event, EventSink},
    native::{Model, NativeSession},
    session::{Command, PromptAdmission, Session, TurnEnd},
    tools::{AccessPolicy, ToolCall, ToolResult},
};
use tokio::sync::mpsc;

struct HeldModel;

struct LifecycleSession {
    started: Arc<AtomicUsize>,
    closed: Arc<AtomicBool>,
}

#[async_trait]
impl Session for LifecycleSession {
    fn owner(&self) -> &'static str {
        "lifecycle-fixture"
    }
    async fn turn(
        &mut self,
        _: String,
        _: &mut mpsc::Receiver<Command>,
        _: &EventSink,
    ) -> Result<TurnEnd> {
        self.started.fetch_add(1, Ordering::SeqCst);
        Ok(TurnEnd::Complete)
    }
    async fn close(&mut self) -> Result<()> {
        self.closed.store(true, Ordering::SeqCst);
        Ok(())
    }
}

#[tokio::test]
async fn shutdown_closes_the_session_while_lifecycle_events_are_backpressured() {
    for stage in ["ready", "started", "finished"] {
        let workspace = tempfile::tempdir().unwrap();
        let log = workspace.path().join("events.jsonl");
        let started = Arc::new(AtomicUsize::new(0));
        let closed = Arc::new(AtomicBool::new(false));
        let session = Box::new(LifecycleSession {
            started: started.clone(),
            closed: closed.clone(),
        });
        let (event_tx, mut held_events) = mpsc::channel(1);
        if stage == "ready" {
            event_tx
                .try_send(Envelope {
                    connection: "fixture".into(),
                    event: Event::Text {
                        text: "occupied".into(),
                    },
                })
                .unwrap();
        }
        let events = EventSink::new("lifecycle".into(), event_tx, Some(&log)).unwrap();
        let (commands, command_rx) = mpsc::channel(4);
        let mut running = tokio::spawn(demoncoder::session::run(session, command_rx, events));
        wait_for_log(&log, "\"type\":\"ready\"").await;
        if stage != "ready" {
            let (reply, admitted) = tokio::sync::oneshot::channel();
            commands
                .send(Command::Submit {
                    text: "initial".into(),
                    reply,
                })
                .await
                .unwrap();
            assert_eq!(admitted.await.unwrap(), Ok(()));
            if stage == "finished" {
                held_events.recv().await.unwrap();
                wait_for_log(&log, "\"type\":\"turn_finished\"").await;
            } else {
                wait_for_log(&log, "\"type\":\"turn_started\"").await;
            }
        }
        let (reply, rejected) = tokio::sync::oneshot::channel();
        commands
            .send(Command::Submit {
                text: "keep this draft".into(),
                reply,
            })
            .await
            .unwrap();
        assert_eq!(
            tokio::time::timeout(Duration::from_millis(500), rejected)
                .await
                .unwrap()
                .unwrap(),
            Err("Session is waiting for terminal output; draft retained. Try again shortly."),
        );
        if stage == "started" {
            commands.send(Command::Cancel).await.unwrap();
            wait_for_log(&log, "\"type\":\"turn_finished\"").await;
            assert!(
                std::fs::read_to_string(&log)
                    .unwrap()
                    .contains("\"status\":\"cancelled\"")
            );
        }
        commands.send(Command::Shutdown).await.unwrap();
        match tokio::time::timeout(Duration::from_millis(500), &mut running).await {
            Ok(result) => result.unwrap().unwrap(),
            Err(_) => {
                running.abort();
                let _ = running.await;
                panic!("shutdown blocked behind the {stage} lifecycle event");
            }
        }
        assert!(
            closed.load(Ordering::SeqCst),
            "{stage}: session owner was not closed"
        );
        assert_eq!(
            started.load(Ordering::SeqCst),
            usize::from(stage == "finished")
        );
    }
}

#[async_trait]
impl Model for HeldModel {
    fn prompt(&mut self, _: String) {}

    fn results(&mut self, _: Vec<ToolResult>) {}

    async fn response(&mut self, events: &EventSink) -> Result<Vec<ToolCall>> {
        events
            .emit(Event::Text {
                text: "NATIVE-HELD".into(),
            })
            .await?;
        pending().await
    }
}

async fn wait_for_log(path: &Path, needle: &str) {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if std::fs::read_to_string(path).is_ok_and(|text| text.contains(needle)) {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("event log never contained {needle}"));
}

#[tokio::test]
async fn native_cancel_is_not_blocked_by_a_full_advisory_event_queue() {
    let workspace = tempfile::tempdir().unwrap();
    let log = workspace.path().join("events.jsonl");
    let (event_tx, _held_events) = mpsc::channel(1);
    let events = EventSink::new("native".into(), event_tx, Some(&log)).unwrap();
    let session: Box<dyn Session> =
        Box::new(NativeSession::new(Box::new(HeldModel), workspace.path()).unwrap());

    let (commands, mut command_rx) = mpsc::channel(4);
    let mut running = tokio::spawn(async move {
        let mut session = session;
        session
            .turn("initial prompt".into(), &mut command_rx, &events)
            .await
    });
    wait_for_log(&log, "NATIVE-HELD").await;
    commands
        .send(Command::Prompt("correction".into()))
        .await
        .unwrap();
    commands.send(Command::Cancel).await.unwrap();
    let result = match tokio::time::timeout(Duration::from_millis(500), &mut running).await {
        Ok(result) => result.unwrap().unwrap(),
        Err(_) => {
            running.abort();
            let _ = running.await;
            panic!("native cancellation was blocked by correction event publication");
        }
    };
    assert!(matches!(result, TurnEnd::Cancelled));
    assert!(
        std::fs::read_to_string(log)
            .unwrap()
            .contains("Correction queued for the next tool boundary")
    );
}

#[tokio::test]
async fn subscription_adapters_cancel_when_the_advisory_event_queue_is_full() {
    for adapter in ["codex", "claude"] {
        let workspace = tempfile::tempdir().unwrap();
        std::fs::write(workspace.path().join("cancellation"), "provider").unwrap();
        let log = workspace.path().join("events.jsonl");
        let session = subscription_session(adapter, workspace.path());
        let (event_tx, mut held_events) = mpsc::channel(1);
        let events = EventSink::new(adapter.into(), event_tx, Some(&log)).unwrap();
        let (commands, mut command_rx) = mpsc::channel(4);
        let mut running = tokio::spawn(async move {
            let mut session = session;
            session
                .turn("initial prompt".into(), &mut command_rx, &events)
                .await
        });

        wait_for_log(&log, "\"type\":\"context\"").await;
        held_events.recv().await.unwrap();
        wait_for_log(&log, "CANCELWAIT-initial prompt").await;
        commands
            .send(Command::Prompt("correction".into()))
            .await
            .unwrap();
        commands.send(Command::Cancel).await.unwrap();
        let result = match tokio::time::timeout(Duration::from_secs(2), &mut running).await {
            Ok(result) => result.unwrap().unwrap(),
            Err(_) => {
                running.abort();
                let _ = running.await;
                panic!("{adapter} cancellation was blocked by event publication");
            }
        };
        assert!(matches!(result, TurnEnd::Cancelled), "{adapter}");
        assert!(
            std::fs::read_to_string(&log)
                .unwrap()
                .contains("Correction queued for the next tool boundary")
        );
    }
}

fn subscription_session(adapter: &str, workspace: &Path) -> Box<dyn Session> {
    let binary = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/backend_fixture.py");
    let config = Connection {
        adapter: adapter.into(),
        model: Some("fixture-model".into()),
        endpoint: None,
        binary: Some(binary),
        effort: None,
        max_output_tokens: None,
        api_key: None,
        access: AccessPolicy::default(),
    };
    adapters::builtins()
        .unwrap()
        .open(&config, workspace)
        .unwrap()
}

#[tokio::test]
async fn bounded_correction_admission_rejects_the_thirty_third_draft_without_blocking_cancel() {
    for adapter in ["native", "codex", "claude"] {
        let workspace = tempfile::tempdir().unwrap();
        let log = workspace.path().join("events.jsonl");
        let session: Box<dyn Session> = if adapter == "native" {
            Box::new(NativeSession::new(Box::new(HeldModel), workspace.path()).unwrap())
        } else {
            subscription_session(adapter, workspace.path())
        };
        let (event_tx, _held_events) = mpsc::channel(1);
        event_tx
            .try_send(Envelope {
                connection: adapter.into(),
                event: Event::Ready { owner: "fixture" },
            })
            .unwrap();
        let events = EventSink::new(adapter.into(), event_tx, Some(&log)).unwrap();
        let (commands, mut command_rx) = mpsc::channel(64);
        let (abandoned_reply, abandoned_admission) = tokio::sync::oneshot::channel();
        drop(abandoned_admission);
        commands
            .try_send(Command::Submit {
                text: "abandoned correction".into(),
                reply: abandoned_reply,
            })
            .unwrap();
        let mut admissions = Vec::new();
        for index in 0..33 {
            let (reply, admitted) = tokio::sync::oneshot::channel::<PromptAdmission>();
            commands
                .try_send(Command::Submit {
                    text: format!("correction-{index}"),
                    reply,
                })
                .unwrap();
            admissions.push(admitted);
        }
        commands.try_send(Command::Cancel).unwrap();
        let mut running = tokio::spawn(async move {
            let mut session = session;
            session
                .turn("initial prompt".into(), &mut command_rx, &events)
                .await
        });
        let result = match tokio::time::timeout(Duration::from_millis(500), &mut running).await {
            Ok(result) => result.unwrap().unwrap(),
            Err(_) => {
                running.abort();
                let _ = running.await;
                panic!("{adapter} stopped draining controls while the event queue was full");
            }
        };
        assert!(matches!(result, TurnEnd::Cancelled), "{adapter}");
        for (index, admission) in admissions.into_iter().enumerate() {
            let actual = admission.await.unwrap();
            if index < 32 {
                assert_eq!(actual, Ok(()), "{adapter} correction {index}");
            } else {
                assert_eq!(
                    actual,
                    Err(
                        "Correction queue is full; draft retained. Wait for a tool boundary and try again."
                    ),
                    "{adapter} correction {index}"
                );
            }
        }
        let retained = std::fs::read_to_string(&log).unwrap();
        assert!(retained.contains("Correction queued for the next tool boundary"));
        assert!(retained.contains("Correction queue is full"));
    }
}

#[tokio::test]
async fn drained_subscription_corrections_still_count_toward_the_turn_limit() {
    for adapter in ["codex", "claude"] {
        let workspace = tempfile::tempdir().unwrap();
        std::fs::write(workspace.path().join("cancellation"), "provider").unwrap();
        let log = workspace.path().join("events.jsonl");
        let session = subscription_session(adapter, workspace.path());
        let (event_tx, _held_events) = mpsc::channel(128);
        let events = EventSink::new(adapter.into(), event_tx, Some(&log)).unwrap();
        let (commands, mut command_rx) = mpsc::channel(4);
        let mut running = tokio::spawn(async move {
            let mut session = session;
            session
                .turn("initial prompt".into(), &mut command_rx, &events)
                .await
        });
        wait_for_log(&log, "CANCELWAIT-initial prompt").await;

        let mut admissions = Vec::new();
        for index in 0..33 {
            let (reply, admission) = tokio::sync::oneshot::channel();
            commands
                .send(Command::Submit {
                    text: format!("drained-correction-{index}"),
                    reply,
                })
                .await
                .unwrap();
            admissions.push(admission.await.unwrap());
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        commands.send(Command::Cancel).await.unwrap();
        let result = tokio::time::timeout(Duration::from_secs(2), &mut running)
            .await
            .unwrap_or_else(|_| panic!("{adapter} did not cancel after correction saturation"))
            .unwrap()
            .unwrap();
        assert!(matches!(result, TurnEnd::Cancelled), "{adapter}");
        assert!(admissions[..32].iter().all(|admission| admission.is_ok()));
        assert_eq!(
            admissions[32],
            Err(
                "Correction queue is full; draft retained. Wait for a tool boundary and try again."
            ),
            "{adapter} released bounded capacity while corrections were still held"
        );
    }
}

#[tokio::test]
async fn application_shutdown_bounds_queue_and_worker_and_joins_abort() {
    // Cover a blocked send and a delivered quit with a stuck worker, under both
    // initial lifetime envelopes. Dropping the guard proves abort was joined.
    let mut cases = Vec::new();
    for native in [false, true] {
        for full in [false, true] {
            cases.push(tokio::spawn(async move {
                struct Dropped(Arc<AtomicBool>);
                impl Drop for Dropped {
                    fn drop(&mut self) {
                        self.0.store(true, Ordering::SeqCst);
                    }
                }
                let dropped = Arc::new(AtomicBool::new(false));
                let (sender, mut receiver) = mpsc::channel(1);
                if full {
                    sender.try_send(Command::Cancel).unwrap();
                }
                let (ready, running) = tokio::sync::oneshot::channel();
                let guard = Dropped(dropped.clone());
                let worker = tokio::spawn(async move {
                    let _guard = guard;
                    ready.send(()).unwrap();
                    if !full {
                        assert!(matches!(receiver.recv().await, Some(Command::Shutdown)));
                    }
                    // Keep the receiver alive so a full-queue send cannot resolve.
                    pending::<()>().await;
                    drop(receiver);
                    Ok(())
                });
                running.await.unwrap();
                let started = tokio::time::Instant::now();
                let result = demoncoder::session::shutdown(sender, worker, native).await;
                assert_eq!(
                    result.unwrap_err().to_string(),
                    "session shutdown timed out"
                );
                let seconds = if native { 8 } else { 3 };
                assert!(started.elapsed() >= Duration::from_secs(seconds));
                assert!(started.elapsed() < Duration::from_secs(seconds + 1));
                assert!(dropped.load(Ordering::SeqCst));
            }));
        }
    }
    for case in cases {
        case.await.unwrap();
    }
}

#[tokio::test]
async fn application_shutdown_preserves_worker_and_join_errors() {
    let (sender, receiver) = mpsc::channel(1);
    drop(receiver);
    let worker = tokio::spawn(async { anyhow::bail!("original close error") });
    assert_eq!(
        demoncoder::session::shutdown(sender, worker, false)
            .await
            .unwrap()
            .unwrap_err()
            .to_string(),
        "original close error"
    );
    let (sender, receiver) = mpsc::channel(1);
    drop(receiver);
    let worker = tokio::spawn(async {
        panic!("worker panic");
        #[allow(unreachable_code)]
        Ok(())
    });
    assert_eq!(
        demoncoder::session::shutdown(sender, worker, false)
            .await
            .unwrap()
            .unwrap_err()
            .to_string(),
        "session runtime failed"
    );
}
