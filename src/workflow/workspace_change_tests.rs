use super::is_control;
use crate::{
    config::Connection,
    events::EventSink,
    plugins::receipts::NonToolOccurrence,
    session::{Command, Session, SessionStart, TurnEnd},
    workflow::runtime::{HostInvocation, SharedRuntime},
};
use serde_json::json;
use tokio::sync::mpsc;

#[test]
fn explicit_workspace_selection_is_a_busy_session_control() {
    assert!(is_control("/workspace /tmp/new root"));
    assert!(is_control("/workspace /tmp/root;still-literal"));
    assert!(!is_control("workspace /tmp/model-text-is-not-a-control"));
}

#[test]
fn workspace_change_has_one_typed_durable_owner() {
    let value = json!({
        "workspace_change": {
            "version": 1,
            "from": {"path":"/tmp/a","device":1,"inode":2,"generation":7},
            "to": {"path":"/tmp/b","device":3,"inode":4,"generation":8},
            "predecessor": 7,
            "native_session": 7,
            "allowance": null,
            "identity": {
                "adapter":"openai-api","endpoint":null,"binary":null,"model":"fixture","effort":null,
                "max_output_tokens":null,"unrestricted":false,"credential_revision":null,
                "strict_worktree":false,"tools_enabled":true,"credential_paths":[],"oracle":null
            },
            "source":"developer",
            "handoff": null,
            "pins":"fixture-pins",
            "stage":"prepared",
            "hold":null
        }
    });
    assert!(
        serde_json::from_value::<HostInvocation>(value).is_ok(),
        "workspace replacement needs one typed owner in the existing operation ledger"
    );
}

#[test]
fn cwd_changed_occurrence_retains_exact_host_transition_facts() {
    let value = json!({
        "event":"CwdChanged",
        "workspace_change":8,
        "old_cwd":"/tmp/a",
        "new_cwd":"/tmp/b"
    });
    assert!(
        serde_json::from_value::<NonToolOccurrence>(value).is_ok(),
        "CwdChanged must be a typed post-application observation"
    );
}

enum CloseBehavior {
    Fail,
    Never(std::sync::Arc<std::sync::Mutex<CloseTiming>>),
    Held {
        started: std::sync::Arc<tokio::sync::Notify>,
        release: std::sync::Arc<tokio::sync::Notify>,
    },
    SucceedAfter(std::time::Duration),
}

#[derive(Default)]
struct CloseTiming {
    started: Option<std::time::Instant>,
    dropped_after: Option<std::time::Duration>,
}

struct PendingCloseGuard(std::sync::Arc<std::sync::Mutex<CloseTiming>>);

impl Drop for PendingCloseGuard {
    fn drop(&mut self) {
        let mut timing = self.0.lock().unwrap();
        timing.dropped_after = timing.started.map(|started| started.elapsed());
    }
}

struct ClosingSession(CloseBehavior);

#[async_trait::async_trait]
impl Session for ClosingSession {
    fn owner(&self) -> &'static str {
        "workspace-close-fixture"
    }

    async fn turn(
        &mut self,
        _: String,
        _: &mut mpsc::Receiver<Command>,
        _: &EventSink,
    ) -> anyhow::Result<TurnEnd> {
        Ok(TurnEnd::Complete)
    }

    async fn close(&mut self) -> anyhow::Result<()> {
        match &self.0 {
            CloseBehavior::Fail => anyhow::bail!("injected old-provider close failure"),
            CloseBehavior::Never(timing) => {
                timing.lock().unwrap().started = Some(std::time::Instant::now());
                let _guard = PendingCloseGuard(timing.clone());
                std::future::pending().await
            }
            CloseBehavior::Held { started, release } => {
                started.notify_one();
                release.notified().await;
                Ok(())
            }
            CloseBehavior::SucceedAfter(delay) => {
                tokio::time::sleep(*delay).await;
                Ok(())
            }
        }
    }
}

fn closing_fixture(
    behavior: CloseBehavior,
) -> (
    tempfile::TempDir,
    std::path::PathBuf,
    std::path::PathBuf,
    SharedRuntime,
    super::WorkflowSession,
    EventSink,
    mpsc::Receiver<crate::events::Envelope>,
) {
    let roots = tempfile::tempdir().unwrap();
    let a = roots.path().join("a");
    let b = roots.path().join("b");
    std::fs::create_dir(&a).unwrap();
    std::fs::create_dir(&b).unwrap();
    let connection: Connection = serde_json::from_value(json!({
        "adapter":"openai-api", "endpoint":"http://127.0.0.1:9",
        "model":"workspace-close", "api_key":"synthetic-close-key"
    }))
    .unwrap();
    let mut record = crate::inspection::tests::record(&a);
    record.identity = super::runtime::Identity::from(&connection);
    record.session_hook_allowance = Some(
        super::runtime::session_budget::SessionHookAllowance::new(super::allocation::Limits {
            seconds: 60,
            model_calls: 8,
            tool_calls: 8,
        })
        .unwrap(),
    );
    let runtime = SharedRuntime::for_test(&roots.path().join("record"), record).unwrap();
    let mut session = super::WorkflowSession::new(
        Box::new(ClosingSession(behavior)),
        connection,
        a.clone(),
        Default::default(),
        runtime.clone(),
        false,
    )
    .unwrap();
    let (event_tx, event_rx) = mpsc::channel(64);
    let events = EventSink::new("workspace-close".into(), event_tx, None)
        .unwrap()
        .with_runtime(runtime.clone());
    session
        .open_lifetime(SessionStart::Startup, &events)
        .unwrap();
    (roots, a, b, runtime, session, events, event_rx)
}

#[tokio::test]
async fn close_failure_keeps_old_root_and_holds_teardown_without_replay() {
    let (_roots, a, b, runtime, mut session, events, _event_rx) =
        closing_fixture(CloseBehavior::Fail);
    let (_command_tx, mut commands) = mpsc::channel(2);

    let error = match session
        .turn(
            format!("/workspace {}", b.display()),
            &mut commands,
            &events,
        )
        .await
    {
        Err(error) => error,
        Ok(_) => panic!("close failure unexpectedly completed"),
    };

    assert!(format!("{error:#}").contains("old provider close is uncertain"));
    let record = runtime.record().unwrap();
    assert_eq!(record.workspace, a);
    assert!(record.recovery_pending);
    assert!(matches!(&record.operations.last().unwrap().host_invocation,
        Some(HostInvocation::WorkspaceChange(change))
            if change.stage == super::runtime::workspace_change::Stage::Teardown
                && change.hold.as_deref().is_some_and(|hold| hold.contains("interrupted after old-root teardown"))));
}

#[tokio::test]
async fn cancel_and_shutdown_bound_a_never_closing_old_provider() {
    for (command, expected) in [
        (Command::Cancel, TurnEnd::Cancelled),
        (Command::Shutdown, TurnEnd::Shutdown),
    ] {
        let close_timing = std::sync::Arc::new(std::sync::Mutex::new(CloseTiming::default()));
        let (_roots, a, b, runtime, mut session, events, _event_rx) =
            closing_fixture(CloseBehavior::Never(close_timing.clone()));
        let (command_tx, mut commands) = mpsc::channel(2);
        command_tx.send(command).await.unwrap();

        let outcome = tokio::time::timeout(
            crate::session::NATIVE_END_BUDGET
                + super::WORKSPACE_CLOSE_GRACE
                + std::time::Duration::from_millis(250),
            session.turn(
                format!("/workspace {}", b.display()),
                &mut commands,
                &events,
            ),
        )
        .await
        .expect("workspace cancellation exceeded its close grace and setup allowance")
        .unwrap();

        assert!(outcome == expected);
        let close_elapsed = close_timing
            .lock()
            .unwrap()
            .dropped_after
            .expect("close was not attempted and stopped");
        assert!(
            close_elapsed < super::WORKSPACE_CLOSE_GRACE + std::time::Duration::from_millis(250),
            "never-closing provider future exceeded the declared close grace: {close_elapsed:?}"
        );
        let record = runtime.record().unwrap();
        assert_eq!(record.workspace, a);
        assert!(record.recovery_pending);
    }
}

#[tokio::test]
async fn ordinary_workspace_change_bounds_a_never_closing_old_provider() {
    let close_timing = std::sync::Arc::new(std::sync::Mutex::new(CloseTiming::default()));
    let (_roots, a, b, runtime, mut session, events, _event_rx) =
        closing_fixture(CloseBehavior::Never(close_timing.clone()));
    let (_command_tx, mut commands) = mpsc::channel(2);

    let result = tokio::time::timeout(
        crate::session::NATIVE_END_BUDGET
            + super::WORKSPACE_CLOSE_GRACE
            + std::time::Duration::from_millis(250),
        session.turn(
            format!("/workspace {}", b.display()),
            &mut commands,
            &events,
        ),
    )
    .await;
    if result.is_err() {
        let close_started = close_timing
            .lock()
            .unwrap()
            .started
            .expect("close was not attempted before the setup allowance ended");
        assert!(
            close_started.elapsed()
                >= super::WORKSPACE_CLOSE_GRACE + std::time::Duration::from_millis(250),
            "setup, rather than old-provider close, exhausted the outer test timeout"
        );
        panic!("ordinary workspace close remained pending past its declared grace");
    }
    let result = result.unwrap();
    let close_elapsed = close_timing
        .lock()
        .unwrap()
        .dropped_after
        .expect("close was not attempted and stopped");
    assert!(
        close_elapsed < super::WORKSPACE_CLOSE_GRACE + std::time::Duration::from_millis(250),
        "ordinary close future exceeded the declared close grace: {close_elapsed:?}"
    );
    let error = match result {
        Err(error) => error,
        Ok(_) => panic!("never-closing provider unexpectedly changed workspace"),
    };
    assert!(format!("{error:#}").contains("old provider close"));
    let record = runtime.record().unwrap();
    assert_eq!(record.workspace, a);
    assert!(record.recovery_pending);
    assert!(matches!(
        &record.operations.last().unwrap().host_invocation,
        Some(HostInvocation::WorkspaceChange(change))
            if change.stage == super::runtime::workspace_change::Stage::Teardown
                && change.hold.is_some()
    ));
}

#[tokio::test]
async fn workspace_change_accepts_old_provider_close_within_bound() {
    let (_roots, _a, b, runtime, mut session, events, _event_rx) = closing_fixture(
        CloseBehavior::SucceedAfter(super::WORKSPACE_CLOSE_GRACE / 2),
    );
    let (_command_tx, mut commands) = mpsc::channel(2);

    let result = tokio::time::timeout(
        crate::session::NATIVE_END_BUDGET
            + super::WORKSPACE_CLOSE_GRACE
            + std::time::Duration::from_millis(250),
        session.turn(
            format!("/workspace {}", b.display()),
            &mut commands,
            &events,
        ),
    )
    .await
    .expect("in-bound close did not settle")
    .unwrap();
    assert!(result == TurnEnd::Complete);
    assert_eq!(
        runtime.record().unwrap().workspace,
        b.canonicalize().unwrap()
    );
}

#[tokio::test]
async fn delayed_close_cannot_publish_after_original_allowance_expires() {
    let started = std::sync::Arc::new(tokio::sync::Notify::new());
    let release = std::sync::Arc::new(tokio::sync::Notify::new());
    let (_roots, a, b, runtime, mut session, events, _event_rx) =
        closing_fixture(CloseBehavior::Held {
            started: started.clone(),
            release: release.clone(),
        });
    runtime
        .update(|record| {
            record
                .session_hook_allowance
                .as_mut()
                .unwrap()
                .allocation
                .deadline_ms = crate::workflow::allocation::now_ms()? + 1_000;
            Ok(())
        })
        .unwrap();
    let (_command_tx, mut commands) = mpsc::channel(2);
    let turn = session.turn(
        format!("/workspace {}", b.display()),
        &mut commands,
        &events,
    );
    let expire = async {
        started.notified().await;
        let deadline = runtime
            .record()
            .unwrap()
            .session_hook_allowance
            .unwrap()
            .allocation
            .deadline_ms;
        let remaining = deadline.saturating_sub(crate::workflow::allocation::now_ms().unwrap());
        tokio::time::sleep(std::time::Duration::from_millis(remaining + 10)).await;
        release.notify_one();
    };
    let (result, ()) = tokio::join!(turn, expire);

    let error = match result {
        Err(error) => error,
        Ok(_) => panic!("expired allowance unexpectedly published workspace B"),
    };
    assert!(
        format!("{error:#}").contains("allowance deadline expired"),
        "delayed close must cross the unchanged original deadline: {error:#}"
    );
    let record = runtime.record().unwrap();
    assert_eq!(record.workspace, a);
    assert!(record.recovery_pending);
    assert!(matches!(
        &record.operations.iter().find_map(|operation| match &operation.host_invocation {
            Some(HostInvocation::WorkspaceChange(change)) => Some(change),
            _ => None,
        }),
        Some(change) if change.stage == super::runtime::workspace_change::Stage::Teardown
            && change.hold.is_some()
    ));
}

#[tokio::test]
async fn cwd_changed_runs_all_native_runners_and_supported_claude_host_translations() {
    use crate::plugins::{
        hook_types::{HandlerKind, HookDialect, HookEvent},
        non_tool::NonToolPlan,
    };

    for dialect in [HookDialect::Native, HookDialect::Claude] {
        let roots = tempfile::tempdir().unwrap();
        let a = roots.path().join("a");
        let b = roots.path().join("b with spaces;literal");
        std::fs::create_dir(&a).unwrap();
        std::fs::create_dir(&b).unwrap();
        let mut registrations = vec![crate::config_change_test_support::cwd_command_registration(
            dialect,
        )];
        let (http, http_peer) =
            crate::config_change_test_support::cwd_http_registration(dialect).await;
        registrations.push(http);
        let (mcp, mcp_peer, _service) =
            crate::config_change_test_support::cwd_mcp_registration(&b, dialect).await;
        registrations.push(mcp);
        let mut model_peers = Vec::new();
        if dialect == HookDialect::Native {
            for kind in [HandlerKind::Prompt, HandlerKind::Agent] {
                let (registration, peer) =
                    crate::config_change_test_support::cwd_model_registration(kind).await;
                registrations.push(registration);
                model_peers.push(peer);
            }
        }
        let expected_hooks = registrations.len();
        let plan =
            std::sync::Arc::new(NonToolPlan::new(HookEvent::CwdChanged, registrations).unwrap());
        let mut connection: Connection = serde_json::from_value(json!({
            "adapter":"openai-api", "endpoint":"http://127.0.0.1:9",
            "model":"cwd-workspace-owner", "api_key":"synthetic-cwd-key"
        }))
        .unwrap();
        connection.access.supervisor = Some(crate::config_change_test_support::supervisor());
        connection.access.non_tools = vec![plan];
        let mut running = crate::config_change_test_support::start_workspace_control(
            &a,
            connection,
            crate::workflow::allocation::Limits {
                seconds: 60,
                model_calls: 8,
                tool_calls: 8,
            },
        )
        .await;
        running.submit(&format!("/workspace {}", b.display())).await;
        let events = running.collect_turn_events("complete").await;
        assert!(events.iter().any(|event| matches!(event, crate::events::Event::WorkspaceChanged { workspace, generation: 1, .. } if workspace == &b.display().to_string())));
        let record = running.runtime.record().unwrap();
        assert_eq!(
            running.runtime.workspace_root().unwrap().path,
            b.canonicalize().unwrap()
        );
        let receipt = record
            .operations
            .iter()
            .find_map(|operation| match &operation.host_invocation {
                Some(HostInvocation::Lifecycle(receipt))
                    if matches!(
                        receipt.facts.subject.occurrence,
                        NonToolOccurrence::CwdChanged { .. }
                    ) =>
                {
                    Some(receipt)
                }
                _ => None,
            })
            .expect("CwdChanged receipt missing");
        assert!(
            receipt.settled && receipt.hold.is_none(),
            "{dialect:?} CwdChanged did not settle: {receipt:?}"
        );
        assert_eq!(receipt.hooks.len(), expected_hooks);
        assert!(
            receipt
                .hooks
                .iter()
                .all(|hook| hook.outcome.is_some() && !hook.uncertain_effects),
            "{dialect:?} runner did not produce a known outcome: {:?}",
            receipt.hooks
        );
        assert!(
            receipt
                .proposals
                .iter()
                .filter(|proposal| matches!(
                    proposal.proposal.kind,
                    crate::plugins::receipts::ProposalKind::ReplaceDynamicWatches
                ))
                .count()
                == 3
        );
        assert!(
            receipt
                .proposals
                .iter()
                .filter(|proposal| matches!(
                    proposal.proposal.kind,
                    crate::plugins::receipts::ProposalKind::ReplaceDynamicWatches
                ))
                .all(|proposal| matches!(
                    proposal.disposition,
                    crate::plugins::receipts::ProposalDisposition::Pending
                )),
            "dynamic watch proposals must remain recorded and unapplied: {:?}",
            receipt.proposals
        );
        assert_eq!(http_peer.count(), 1);
        assert_eq!(mcp_peer.method_count("tools/call"), 1);
        let model_counts = model_peers
            .iter()
            .map(|peer| peer.count())
            .collect::<Vec<_>>();
        assert!(
            model_counts.iter().all(|count| *count == 1),
            "{dialect:?} model runner request counts: {model_counts:?}; hooks: {:?}",
            receipt.hooks
        );
        running.shutdown().await;
    }
}

#[test]
fn codex_source_cwd_changed_requires_explicit_native_conversion_and_never_executes() {
    use crate::plugins::{
        dispatch::{
            Declaration, DeclarationIdentity, HandlerClass, HookInvocation, HookRunner, Matcher,
            Registration, Scope,
        },
        gate_snapshot::GateReadSet,
        hook_types::{HandlerKind, HookDialect, HookEvent},
        non_tool::NonToolPlan,
        receipts::RawOutcome,
    };
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    struct Trap(Arc<AtomicUsize>);
    #[async_trait::async_trait]
    impl HookRunner for Trap {
        fn bound_event(&self) -> Option<HookEvent> {
            Some(HookEvent::CwdChanged)
        }
        async fn run(&self, _: &HookInvocation) -> anyhow::Result<RawOutcome> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(RawOutcome::Command {
                exit_code: Some(0),
                stdout: br#"{"hookSpecificOutput":{"hookEventName":"CwdChanged","watchPaths":["/must-not-apply"]}}"#.to_vec(),
                stderr: vec![],
            })
        }
    }

    let calls = Arc::new(AtomicUsize::new(0));
    let registration = Registration {
        declaration: Declaration {
            required_gate: false,
            source: None,
            once: None,
            identity: DeclarationIdentity {
                package: "codex-cwd-unavailable".into(),
                code: "captured-code".into(),
                policy: "captured-policy".into(),
                configuration: "captured-configuration".into(),
                generation: "1".into(),
                scope: Scope::Project,
                role: "worker".into(),
                declaration: "codex-cwd-unavailable".into(),
                index: 0,
                dialect: HookDialect::Codex,
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
        runner: Arc::new(Trap(calls.clone())),
        revalidation: None,
    };
    let error = match NonToolPlan::new(HookEvent::CwdChanged, vec![registration]) {
        Ok(_) => panic!("Codex source CwdChanged unexpectedly became executable"),
        Err(error) => error,
    };
    assert!(
        format!("{error:#}")
            .contains("source pair cannot execute; explicit native conversion required")
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}
