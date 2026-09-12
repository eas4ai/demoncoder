//! Actual pinned backends against parent-owned local model peers.
use anyhow::{Context, Result};
use async_trait::async_trait;
use demoncoder::{
    adapters,
    config::Connection,
    events::EventSink,
    plugins::{
        dispatch::*,
        gate_snapshot::GateReadSet,
        hook_types::{HandlerKind, HookDialect, HookEvent},
        lifecycle::PostToolPlan,
        non_tool::NonToolPlan,
    },
    session::{Command, TurnEnd},
    workflow::{allocation::Limits, runtime::SharedRuntime, state::Task, workspace},
};
use serde_json::json;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::mpsc;

struct Gate {
    case: String,
    seen: Arc<Mutex<Vec<serde_json::Value>>>,
    entered: Arc<tokio::sync::Notify>,
}
#[async_trait]
impl HookRunner for Gate {
    fn side_effect_free(&self) -> bool {
        true
    }
    async fn run(&self, invocation: &HookInvocation) -> Result<RawOutcome> {
        let facts = invocation
            .lifecycle
            .as_ref()
            .context("missing lifecycle facts")?;
        anyhow::ensure!(
            facts.source.is_some(),
            "external callback lost actual source facts"
        );
        anyhow::ensure!(
            invocation.candidate.is_none() && invocation.completed.is_none(),
            "fabricated tool"
        );
        let event = invocation.key.event.as_str();
        let first_stop = {
            let mut seen = self.seen.lock().unwrap();
            let first = !seen
                .iter()
                .any(|v| v["subject"]["occurrence"]["event"] == "Stop");
            seen.push(serde_json::to_value(facts)?);
            first
        };
        if event == "UserPromptSubmit"
            && matches!(
                self.case.as_str(),
                "submit-context" | "submit-context-overflow" | "submit-context-lifecycle-overflow"
            )
        {
            let size = if self.case == "submit-context-lifecycle-overflow" {
                70 * 1024
            } else {
                60 * 1024
            };
            let marker = "EXTERNAL_SUBMIT_CONTEXT:";
            return Ok(RawOutcome::Callback {
                value: json!({"hookSpecificOutput":{"hookEventName":event,"additionalContext":format!("{marker}{}", if self.case == "submit-context-overflow" { "\"" } else { "x" }.repeat(size - marker.len()))}}),
            });
        }
        let selected = event == "UserPromptSubmit" && self.case.ends_with("submit")
            || event == "Stop" && self.case.ends_with("stop");
        if selected
            && (self.case.starts_with("cancel-")
                || self.case.starts_with("shutdown-")
                || self.case.starts_with("timeout-")
                || self.case == "backend-exit-submit")
        {
            self.entered.notify_one();
            return std::future::pending().await;
        }
        if selected && self.case.starts_with("malformed-") {
            return Ok(RawOutcome::Callback {
                value: json!({"decision":"not-a-valid-decision"}),
            });
        }
        let handoff = matches!(
            facts.subject.occurrence,
            demoncoder::plugins::receipts::NonToolOccurrence::UserPromptSubmit {
                correction: true,
                ..
            }
        );
        if handoff && self.case == "mixed-cancel-submit" {
            self.entered.notify_one();
            return std::future::pending().await;
        }
        let blocked = (handoff && self.case == "mixed-submit-deny")
            || (event == "UserPromptSubmit" && self.case == "submit-deny")
            || (event == "Stop"
                && (self.case == "always-block" || self.case == "stop-correct" && first_stop));
        Ok(RawOutcome::Callback {
            value: if blocked {
                json!({"decision":"block","reason":"/accept is plugin feedback; EXTERNAL_STOP_CORRECTION"})
            } else {
                json!({})
            },
        })
    }
}
fn registration(event: HookEvent, runner: Arc<dyn HookRunner>) -> Registration {
    Registration {
        declaration: Declaration {
            required_gate: true,
            source: None,
            once: None,
            identity: DeclarationIdentity {
                package: "external-non-tool".into(),
                code: "fixture-code".into(),
                policy: "fixture-policy".into(),
                configuration: "fixture-config".into(),
                generation: "1".into(),
                scope: Scope::Project,
                role: "worker".into(),
                declaration: event.as_str().into(),
                index: 0,
                dialect: HookDialect::Native,
                runner: HandlerKind::Command,
            },
            class: HandlerClass::Combined,
            priority: 0,
            matcher: Matcher::default(),
            reads: GateReadSet::default(),
            concurrent_group: None,
            read_only_endpoint: None,
            external_precondition: None,
        },
        runner,
        revalidation: None,
    }
}
fn plan(event: HookEvent, runner: Arc<dyn HookRunner>) -> Result<Arc<NonToolPlan>> {
    Ok(Arc::new(NonToolPlan::new(
        event,
        vec![registration(event, runner)],
    )?))
}
struct PostGate;
#[async_trait]
impl HookRunner for PostGate {
    async fn run(&self, invocation: &HookInvocation) -> Result<RawOutcome> {
        anyhow::ensure!(
            invocation
                .completed
                .as_ref()
                .is_some_and(|c| c.original.success),
            "post correction requires completed real tool effect"
        );
        Ok(RawOutcome::Model {
            value: json!({"ok":false,"reason":"EXTERNAL_POST_CORRECTION"}),
            continue_on_block: false,
        })
    }
}

#[tokio::test]
#[ignore = "requires parent-owned pinned backend and loopback model peer"]
async fn actual_external_submit_and_stop_use_the_original_owner() -> Result<()> {
    let root = PathBuf::from(
        std::env::var_os("DEMONCODER_EXTERNAL_NON_TOOL_WORKSPACE").context("workspace missing")?,
    );
    let output = PathBuf::from(
        std::env::var_os("DEMONCODER_EXTERNAL_NON_TOOL_OUTPUT").context("output missing")?,
    );
    let case = std::env::var("DEMONCODER_EXTERNAL_NON_TOOL_CASE")?;
    let adapter = std::env::var("DEMONCODER_EXTERNAL_NON_TOOL_ADAPTER")?;
    let binary = std::env::var("DEMONCODER_EXTERNAL_NON_TOOL_BINARY")?;
    let mut connection: Connection =
        serde_json::from_value(json!({"adapter":adapter,"binary":binary}))?;
    connection.access.supervisor = Some(env!("CARGO_BIN_EXE_demoncoder").into());
    let seen = Arc::new(Mutex::new(vec![]));
    let entered = Arc::new(tokio::sync::Notify::new());
    let runner = Arc::new(Gate {
        case: case.clone(),
        seen: seen.clone(),
        entered: entered.clone(),
    });
    connection.access.non_tools = vec![];
    if case != "mixed-stop-only" {
        connection
            .access
            .non_tools
            .push(plan(HookEvent::UserPromptSubmit, runner.clone())?);
    }
    if case != "mixed-submit-only" {
        connection
            .access
            .non_tools
            .push(plan(HookEvent::Stop, runner)?);
    }
    let mixed = case.starts_with("mixed-");
    if mixed {
        let mut post = registration(HookEvent::PostToolUse, Arc::new(PostGate));
        post.declaration.class = HandlerClass::DecisionGate;
        post.declaration.identity.runner = HandlerKind::Prompt;
        post.declaration.matcher.tool = Some("write".into());
        connection
            .access
            .post_tools
            .push(Arc::new(PostToolPlan::new(
                HookEvent::PostToolUse,
                vec![post],
            )?));
    }
    let (runtime, _) = SharedRuntime::open(&root, &connection, None)?;
    runtime.allocate(Limits::default(), None)?;
    runtime.save_task(
        &Some(Task::new(
            1,
            "Perform the fixture operation once.".into(),
            vec![],
            workspace::capture(&root)?,
            1,
        )?),
        2,
        None,
    )?;
    runtime.begin_phase("worker", None)?;
    let before = runtime.record()?.allocation.context("allocation missing")?;
    let (sender, mut receiver) = mpsc::channel(256);
    let events = EventSink::new(
        "external-non-tool".into(),
        sender,
        Some(&output.join("host-events.jsonl")),
    )?
    .with_runtime(runtime.clone());
    let drain = tokio::spawn(async move { while receiver.recv().await.is_some() {} });
    let (command_tx, mut commands) = mpsc::channel(4);
    let control = if case.starts_with("cancel-")
        || case.starts_with("shutdown-")
        || case == "mixed-cancel-submit"
        || case == "backend-exit-submit"
    {
        let tx = command_tx.clone();
        let shutdown = case.starts_with("shutdown-");
        let source_exit = case == "backend-exit-submit";
        let pid_file = output.join("backend.pid");
        Some(tokio::spawn(async move {
            entered.notified().await;
            if source_exit {
                kill_exact_source(&pid_file).expect("terminate exact pending fixture source");
                return;
            }
            tx.send(if shutdown {
                Command::Shutdown
            } else {
                Command::Cancel
            })
            .await
            .unwrap();
        }))
    } else {
        None
    };
    let mut session = adapters::builtins()?.open(&connection, &root)?;
    let mut result = tokio::time::timeout(
        Duration::from_secs(120),
        session.turn(
            "Perform the fixture operation once.".into(),
            &mut commands,
            &events,
        ),
    )
    .await
    .unwrap_or_else(|_| Err(anyhow::anyhow!("external fixture turn exceeded deadline")));
    if case == "pass-twice" && matches!(result, Ok(TurnEnd::Complete)) {
        result = session
            .turn(
                "Perform the fixture operation once.".into(),
                &mut commands,
                &events,
            )
            .await;
    }
    session.close().await?;
    if let Some(control) = control {
        if control.is_finished() {
            control.await?;
        } else {
            control.abort();
        }
    }
    let record = runtime.record()?;
    std::fs::write(
        output.join("receipt.json"),
        serde_json::to_vec_pretty(&record)?,
    )?;
    std::fs::write(
        output.join("host-result.json"),
        serde_json::to_vec_pretty(
            &json!({"case":case,"result":result.as_ref().map(|end| match end { TurnEnd::Complete => "complete", TurnEnd::Cancelled => "cancelled", TurnEnd::Shutdown => "shutdown", TurnEnd::CommandsClosed => "commands_closed" }).map_err(|e| format!("{e:#}")),"seen":*seen.lock().unwrap()}),
        )?,
    )?;
    let receipts: Vec<_> = record
        .operations
        .iter()
        .filter_map(|o| match &o.host_invocation {
            Some(demoncoder::workflow::runtime::HostInvocation::Lifecycle(r)) => Some(r),
            _ => None,
        })
        .collect();
    let expected = match case.as_str() {
        "pass" | "empty-stop" | "submit-context" => 2,
        "submit-context-overflow" | "submit-context-lifecycle-overflow" => 1,
        "pass-twice" => 4,
        "submit-deny"
        | "cancel-submit"
        | "shutdown-submit"
        | "timeout-submit"
        | "malformed-submit"
        | "backend-exit-submit" => 1,
        "cancel-stop" | "malformed-stop" => 2,
        "stop-correct" | "always-block" | "mixed-post-correct" | "mixed-stop-only"
        | "mixed-submit-only" => 3,
        "mixed-submit-deny" | "mixed-cancel-submit" => 2,
        _ => anyhow::bail!("unknown case"),
    };
    assert_eq!(
        receipts.len(),
        expected,
        "production adapter did not deliver actual lifecycle callbacks"
    );
    let expected_handlers = match case.as_str() {
        "mixed-stop-only" => 1,
        "mixed-submit-only" => 2,
        _ => expected,
    };
    assert_eq!(seen.lock().unwrap().len(), expected_handlers);
    if case.starts_with("cancel-") || case == "mixed-cancel-submit" {
        assert!(matches!(result, Ok(TurnEnd::Cancelled)));
    } else if case.starts_with("shutdown-") {
        assert!(matches!(result, Ok(TurnEnd::Shutdown)));
    } else if matches!(
        case.as_str(),
        "submit-deny"
            | "always-block"
            | "mixed-submit-deny"
            | "submit-context-overflow"
            | "submit-context-lifecycle-overflow"
    ) || case.starts_with("malformed-")
        || case.starts_with("timeout-")
        || case == "backend-exit-submit"
    {
        assert!(
            result.is_err(),
            "held source completion cannot accept the host gate"
        );
    } else {
        assert!(
            matches!(result, Ok(TurnEnd::Complete)),
            "{}",
            result
                .as_ref()
                .err()
                .map(|e| format!("{e:#}"))
                .unwrap_or_default()
        );
    }
    let after = record.allocation.as_ref().unwrap();
    assert_eq!(
        (before.started_ms, before.deadline_ms),
        (after.started_ms, after.deadline_ms)
    );
    assert!(record.task.as_ref().unwrap().accepted.is_none());
    assert_eq!(
        record.task.as_ref().unwrap().corrections,
        u32::from(mixed || matches!(case.as_str(), "stop-correct" | "always-block"))
    );
    if mixed {
        let tools: Vec<_> = record
            .operations
            .iter()
            .filter(|o| o.call.is_some())
            .collect();
        assert_eq!(tools.len(), 1);
        assert!(tools[0].result.as_ref().unwrap().success);
        assert_eq!(
            std::fs::read_to_string(root.join("proof.txt"))?,
            "external mixed effect\n"
        );
    } else {
        assert!(record.operations.iter().all(|o| o.call.is_none()));
    }
    let backend: Vec<_> = record
        .operations
        .iter()
        .filter(|o| {
            matches!(
                o.host_invocation,
                Some(demoncoder::workflow::runtime::HostInvocation::Backend)
            )
        })
        .collect();
    assert_eq!(
        backend.len(),
        if mixed || case == "pass-twice" { 2 } else { 1 },
        "source correction must retain its original backend invocation"
    );
    for (index, receipt) in receipts.iter().enumerate() {
        let empty_observation =
            (case == "mixed-stop-only" && index < 2) || (case == "mixed-submit-only" && index == 2);
        assert_eq!(receipt.hooks.is_empty(), empty_observation);
        assert_eq!(receipt.declarations.is_empty(), empty_observation);
        let callback = receipt.facts.callback.as_ref().unwrap();
        let owner = if mixed && index > 0 || case == "pass-twice" && index >= 2 {
            backend[1].id
        } else {
            backend[0].id
        };
        assert_eq!(callback.backend_operation, owner);
        if mixed && index > 0 {
            let demoncoder::plugins::receipts::SourceOrigin::PluginPostCorrection {
                post_operation,
                content_digest,
            } = callback.origin.as_ref().unwrap()
            else {
                panic!("machine Submit lost explicit plugin origin");
            };
            let completed = record.operations.iter().find(|o| o.call.is_some()).unwrap();
            assert_eq!(*post_operation, completed.id);
            assert_eq!(content_digest.len(), 64);
            if index == 1 {
                assert!(matches!(
                    receipt.facts.subject.occurrence,
                    demoncoder::plugins::receipts::NonToolOccurrence::UserPromptSubmit {
                        correction: true,
                        ..
                    }
                ));
            }
            let post = completed
                .tool_receipt
                .as_ref()
                .unwrap()
                .plugin_lifecycle
                .as_ref()
                .unwrap();
            use demoncoder::plugins::receipts::{CorrectionAcknowledgment, PostDelivery};
            match &post.delivery {
                PostDelivery::CorrectionAcknowledged {
                    invocation,
                    acknowledgment:
                        CorrectionAcknowledgment::ClaudeUser {
                            uuid,
                            content_digest: acknowledged,
                            ..
                        },
                } => {
                    assert_eq!(*invocation, owner);
                    assert_eq!(content_digest, acknowledged);
                    assert_eq!(callback.command_uuid.as_ref(), Some(uuid));
                }
                PostDelivery::CorrectionAcknowledged {
                    invocation,
                    acknowledgment: CorrectionAcknowledgment::CodexTurn { request_id, .. },
                } => {
                    assert_eq!(*invocation, owner);
                    assert_eq!(callback.command_request_id, Some(*request_id));
                }
                PostDelivery::CorrectionReserved { invocation } => assert_eq!(*invocation, owner),
                other => panic!("unexpected correction delivery {other:?}"),
            }
        } else {
            assert!(matches!(
                callback.origin,
                Some(demoncoder::plugins::receipts::SourceOrigin::HostSubmission)
            ));
        }
        assert_eq!(callback.sequence, index as u64 + 1);
        assert!(receipt.facts.native_turn.is_none());
        if adapter == "claude" {
            assert!(
                callback
                    .command_uuid
                    .as_deref()
                    .is_some_and(|id| !id.is_empty())
            );
            if !mixed && (case != "pass-twice" || index < 2) || mixed && index == 0 {
                assert_eq!(
                    callback.command_uuid,
                    receipts[0].facts.callback.as_ref().unwrap().command_uuid
                );
            } else {
                assert_ne!(
                    callback.command_uuid,
                    receipts[0].facts.callback.as_ref().unwrap().command_uuid
                );
            }
            assert!(
                callback
                    .envelope_id
                    .as_deref()
                    .is_some_and(|id| !id.is_empty())
            );
        }
        if adapter == "codex" {
            assert!(callback.command_uuid.is_none());
            assert!(callback.command_request_id.is_some());
            assert!(
                callback
                    .envelope_id
                    .as_ref()
                    .is_some_and(|id| id.len() == 36)
            );
            if index > 0 {
                assert_ne!(
                    callback.envelope_id,
                    receipts[index - 1]
                        .facts
                        .callback
                        .as_ref()
                        .unwrap()
                        .envelope_id
                );
            }
        }
        if index > 0 && adapter == "claude" {
            assert_ne!(
                callback.request_id,
                receipts[index - 1]
                    .facts
                    .callback
                    .as_ref()
                    .unwrap()
                    .request_id
            );
        }
        let source = match receipt.facts.source.as_ref().unwrap() {
            demoncoder::plugins::receipts::ObservedLifecycle::Claude(v)
            | demoncoder::plugins::receipts::ObservedLifecycle::Codex(v) => v,
        };
        assert_eq!(source["cwd"], root.to_str().unwrap());
        assert!(source["session_id"].as_str().is_some_and(|s| !s.is_empty()));
        assert_ne!(
            source["transcript_path"],
            receipt.facts.host_transcript_path
        );
        assert!(
            receipt
                .hooks
                .iter()
                .all(|h| h.inspected.source_operation == owner)
        );
    }
    if adapter == "codex" && matches!(case.as_str(), "stop-correct" | "always-block") {
        let first = receipts[1].facts.callback.as_ref().unwrap();
        let second = receipts[2].facts.callback.as_ref().unwrap();
        assert_eq!(
            first.request_id, second.request_id,
            "actual native Stop ID repeats across ordered occurrences"
        );
        assert_ne!(first.envelope_id, second.envelope_id);
        assert_eq!(first.command_request_id, second.command_request_id);
    }
    if case.starts_with("cancel-")
        || case.starts_with("shutdown-")
        || case.starts_with("timeout-")
        || case == "mixed-cancel-submit"
        || case == "backend-exit-submit"
    {
        let last = receipts.last().unwrap();
        assert!(!last.settled && last.hooks.iter().any(|h| h.outcome.is_none()));
        assert_eq!(
            last.source_delivery,
            Some(demoncoder::plugins::receipts::SourceDelivery::Pending)
        );
        assert!(
            session
                .turn(
                    "Do not replay uncertain callbacks".into(),
                    &mut commands,
                    &events
                )
                .await
                .is_err()
        );
        session.close().await?;
        assert_eq!(seen.lock().unwrap().len(), expected_handlers);
    } else {
        for receipt in &receipts {
            assert_eq!(receipt.settled, case != "submit-context-lifecycle-overflow");
        }
        for receipt in receipts.iter().filter(|r| {
            r.hold.is_none()
                && !matches!(
                    case.as_str(),
                    "submit-context-overflow" | "submit-context-lifecycle-overflow"
                )
        }) {
            assert_eq!(
                receipt.source_delivery,
                Some(demoncoder::plugins::receipts::SourceDelivery::Acknowledged)
            );
        }
    }
    if matches!(
        case.as_str(),
        "submit-context-overflow" | "submit-context-lifecycle-overflow"
    ) {
        assert_eq!(
            receipts[0].source_delivery,
            Some(demoncoder::plugins::receipts::SourceDelivery::Pending)
        );
        assert!(
            session
                .turn("Never replay overflow".into(), &mut commands, &events)
                .await
                .is_err()
        );
        assert_eq!(seen.lock().unwrap().len(), 1);
    }
    drain.abort();
    Ok(())
}

fn kill_exact_source(pid_file: &std::path::Path) -> Result<()> {
    let raw: i32 = std::fs::read_to_string(pid_file)?.parse()?;
    let pid = rustix::process::Pid::from_raw(raw).context("invalid source PID")?;
    let handle = rustix::process::pidfd_open(pid, rustix::process::PidfdFlags::NONBLOCK)?;
    fn parent(pid: i32) -> Result<i32> {
        let status = std::fs::read_to_string(format!("/proc/{pid}/status"))?;
        status
            .lines()
            .find_map(|line| line.strip_prefix("PPid:"))
            .context("source parent missing")?
            .trim()
            .parse()
            .map_err(Into::into)
    }
    let supervisor = parent(raw)?;
    anyhow::ensure!(
        supervisor > 1 && parent(supervisor)? == i32::try_from(std::process::id())?,
        "PID does not belong to this fixture's supervised source"
    );
    rustix::process::pidfd_send_signal(&handle, rustix::process::Signal::KILL)?;
    Ok(())
}
