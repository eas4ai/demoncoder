use super::*;
use crate::plugins::receipts::NonToolOccurrence;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

struct ToggleGate {
    deny: AtomicBool,
    calls: AtomicUsize,
    captured: Mutex<Vec<serde_json::Value>>,
}

struct HeldGate {
    hold: AtomicBool,
    calls: AtomicUsize,
    entered: tokio::sync::Semaphore,
    release: tokio::sync::Semaphore,
}

impl HeldGate {
    fn new() -> Self {
        Self {
            hold: AtomicBool::new(false),
            calls: AtomicUsize::new(0),
            entered: tokio::sync::Semaphore::new(0),
            release: tokio::sync::Semaphore::new(0),
        }
    }

    fn hold(&self) {
        self.hold.store(true, Ordering::Release);
    }

    async fn wait_entered(&self) {
        tokio::time::timeout(std::time::Duration::from_secs(10), self.entered.acquire())
            .await
            .expect("held Settings gate was not invoked")
            .unwrap()
            .forget();
    }

    fn release(&self) {
        self.hold.store(false, Ordering::Release);
        self.release.add_permits(1);
    }
}

#[async_trait::async_trait]
impl HookRunner for HeldGate {
    fn side_effect_free(&self) -> bool {
        true
    }

    async fn run(&self, _: &HookInvocation) -> Result<RawOutcome> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.hold.load(Ordering::Acquire) {
            self.entered.add_permits(1);
            self.release.acquire().await.unwrap().forget();
        }
        Ok(RawOutcome::Command {
            exit_code: Some(0),
            stdout: br#"{"decision":"approve"}"#.to_vec(),
            stderr: vec![],
        })
    }
}

struct UnknownGate;

#[async_trait::async_trait]
impl HookRunner for UnknownGate {
    async fn run(&self, _: &HookInvocation) -> Result<RawOutcome> {
        anyhow::bail!("controlled gate outcome is unknown")
    }
}

impl ToggleGate {
    fn new(deny: bool) -> Self {
        Self {
            deny: AtomicBool::new(deny),
            calls: AtomicUsize::new(0),
            captured: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait::async_trait]
impl HookRunner for ToggleGate {
    fn side_effect_free(&self) -> bool {
        true
    }

    async fn run(&self, invocation: &HookInvocation) -> Result<RawOutcome> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.captured.lock().unwrap().push(serde_json::to_value(
            invocation.lifecycle.as_ref().expect("ConfigChange facts"),
        )?);
        Ok(RawOutcome::Command {
            exit_code: Some(0),
            stdout: if self.deny.load(Ordering::SeqCst) {
                br#"{"decision":"block","reason":"actual owner policy denied settings"}"#.to_vec()
            } else {
                br#"{"decision":"approve"}"#.to_vec()
            },
            stderr: vec![],
        })
    }
}

fn registration(
    name: &str,
    runner: Arc<dyn HookRunner>,
    kind: HandlerKind,
    priority: i32,
) -> Registration {
    let mut declaration =
        crate::config_change_test_support::declaration(name, HookDialect::Native, kind);
    declaration.priority = priority;
    Registration {
        declaration,
        runner,
        revalidation: None,
    }
}

fn plan(registrations: Vec<Registration>) -> Arc<NonToolPlan> {
    Arc::new(NonToolPlan::new(HookEvent::ConfigChange, registrations).unwrap())
}

fn owner_fixture(
    root: &std::path::Path,
    current: &crate::config::Connection,
) -> (Handle, std::path::PathBuf) {
    std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = root.join("settings.toml");
    let mut replacement = current.clone();
    replacement.model = Some("saved-next-model".into());
    let mut connections = std::collections::BTreeMap::new();
    connections.insert("current".to_owned(), current.clone());
    connections.insert("replacement".to_owned(), replacement);
    let config = Config {
        onboarding_complete: true,
        default_connection: Some("current".into()),
        connections,
        settings: Some(Assignments {
            providers: vec!["current".into(), "replacement".into()],
            creator: Some(Assignment {
                connection: "current".into(),
                model: current.model.clone(),
                effort: current.effort.clone(),
            }),
            overrides: Default::default(),
        }),
        ..Config::default()
    };
    crate::startup::save(&path, &config).unwrap();
    let args = Args::parse_from([
        "demoncoder",
        "--config",
        path.to_str().unwrap(),
        "--workspace",
        root.to_str().unwrap(),
        "--trust-workspace",
    ]);
    (Handle::open(&args).unwrap(), path)
}

fn replacement_draft(handle: &Handle, model: &str) -> Draft {
    let mut draft = handle.draft().unwrap();
    let mut assignments = Assignments::from_config(&draft.config);
    assignments.creator = Some(Assignment {
        connection: "replacement".into(),
        model: Some(model.into()),
        effort: None,
    });
    draft.config.settings = Some(assignments);
    draft
}

fn pause_next_config_change_operation(
    handle: &Handle,
) -> (
    tokio::sync::oneshot::Receiver<()>,
    std::sync::mpsc::Sender<()>,
) {
    let control = handle.control.lock().unwrap();
    let ControlState::Ready(control) = &*control else {
        panic!("Settings control is not ready")
    };
    control.events.pause_next_after_non_tool_insert()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn config_change_operation_owner_is_atomic_for_finish_and_shutdown_readers() {
    let root = tempfile::tempdir().unwrap();
    let (handle, _) = fixture(root.path());
    let effects = Arc::new(Mutex::new(Vec::new()));
    let running = start_control(
        &handle,
        root.path(),
        config_change_plan(Arc::new(AllowConfigChange(effects.clone()))),
    )
    .await;
    let (operation_inserted, release_dispatch) = pause_next_config_change_operation(&handle);
    let pending = handle.start_save(canary_draft(&handle));
    tokio::time::timeout(std::time::Duration::from_secs(10), operation_inserted)
        .await
        .expect("ConfigChange operation insertion did not pause")
        .expect("ConfigChange insertion pause was dropped");

    let finish = running.runtime.finish_phase();
    let cancel = running.runtime.cancel_settings_controls();
    let drain = running
        .runtime
        .drain_settings_controls(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
        .await;
    let during = running.runtime.record().unwrap();
    release_dispatch.send(()).unwrap();
    let (outcome, cleanup) = pending.finish().await;

    assert!(
        !during.recovery_pending,
        "an exactly owned ConfigChange caused spurious recovery"
    );
    assert!(during.operations.iter().all(|operation| {
        operation
            .usage_receipt
            .as_ref()
            .is_none_or(|receipt| !receipt.missing_report)
    }));
    assert!(finish.is_ok(), "phase finish failed: {finish:?}");
    assert!(cancel.is_ok(), "shutdown cancellation failed: {cancel:?}");
    assert!(drain.is_ok(), "shutdown drain failed: {drain:?}");
    let (operation, receipt) = config_change_receipt(&during);
    assert_eq!(
        receipt
            .host_control
            .as_ref()
            .unwrap()
            .operation
            .load(Ordering::Acquire),
        operation.id
    );
    assert!(outcome.unwrap_err().to_string().contains("cancel"));
    cleanup.unwrap();
    assert_eq!(effects.lock().unwrap().len(), 0);
    running.shutdown().await.unwrap();
}

#[tokio::test]
async fn config_change_duplicate_owner_rejects_without_inserting_an_operation() {
    let root = tempfile::tempdir().unwrap();
    let (handle, _) = fixture(root.path());
    let effects = Arc::new(Mutex::new(Vec::new()));
    let running = start_control(
        &handle,
        root.path(),
        config_change_plan(Arc::new(AllowConfigChange(effects.clone()))),
    )
    .await;
    let attempt = Arc::new(SaveAttempt::new());
    attempt.operation.store(999, Ordering::Release);
    let mut draft = canary_draft(&handle);

    let error = handle
        .save_with_attempt(&mut draft, Some(attempt.clone()))
        .await
        .unwrap_err();
    let record = running.runtime.record().unwrap();

    assert!(
        error
            .to_string()
            .contains("settings control already owns an operation"),
        "unexpected duplicate-owner error: {error:#}"
    );
    assert_eq!(attempt.operation.load(Ordering::Acquire), 999);
    assert_eq!(
        record
            .operations
            .iter()
            .filter_map(Operation::non_tool_receipt)
            .filter(|receipt| {
                receipt.facts.subject.occurrence.event() == HookEvent::ConfigChange
            })
            .count(),
        0
    );
    assert_eq!(effects.lock().unwrap().len(), 0);
    running.shutdown().await.unwrap();
}

#[tokio::test]
async fn config_change_failed_insertion_keeps_exact_original_owner_without_retry() {
    let root = tempfile::tempdir().unwrap();
    let (handle, _) = fixture(root.path());
    let effects = Arc::new(Mutex::new(Vec::new()));
    let running = start_control(
        &handle,
        root.path(),
        config_change_plan(Arc::new(AllowConfigChange(effects.clone()))),
    )
    .await;
    running
        .runtime
        .fail_config_change_receipt_before_rename_when(unsettled_config_change);
    let attempt = Arc::new(SaveAttempt::new());
    let mut draft = canary_draft(&handle);

    let error = handle
        .save_with_attempt(&mut draft, Some(attempt.clone()))
        .await
        .unwrap_err();
    let failed = running.runtime.record().unwrap();
    let (operation, receipt) = config_change_receipt(&failed);
    let operation_id = operation.id;
    let retained_owner = receipt
        .host_control
        .as_ref()
        .unwrap()
        .operation
        .load(Ordering::Acquire);
    let mut retry = canary_draft(&handle);
    let retry_error = handle.save(&mut retry).await.unwrap_err();
    let after_retry = running.runtime.record().unwrap();

    assert!(
        format!("{error:#}").contains("persist session transition; execution is held"),
        "unexpected insertion persistence error: {error:#}"
    );
    assert_eq!(attempt.operation.load(Ordering::Acquire), operation_id);
    assert_eq!(retained_owner, operation_id);
    assert!(
        retry_error
            .to_string()
            .contains("session persistence failed; execution is held until recovery")
    );
    assert_eq!(
        after_retry
            .operations
            .iter()
            .filter_map(Operation::non_tool_receipt)
            .filter(|receipt| {
                receipt.facts.subject.occurrence.event() == HookEvent::ConfigChange
            })
            .count(),
        1
    );
    assert_eq!(effects.lock().unwrap().len(), 0);
    assert!(running.shutdown().await.is_err());
}

fn host_lifetime(record: &Record) -> (u64, String) {
    record
        .operations
        .iter()
        .find_map(|operation| match &operation.host_invocation {
            Some(HostInvocation::NativeSession(lifetime)) => {
                Some((operation.id, lifetime.session.clone()))
            }
            _ => None,
        })
        .expect("host lifetime")
}

fn assert_settings_owner(record: &Record, original_lifetime: u64) {
    let settings = record
        .operations
        .iter()
        .filter_map(|operation| {
            operation
                .non_tool_receipt()
                .map(|receipt| (operation, receipt))
        })
        .filter(|(_, receipt)| receipt.facts.subject.occurrence.event() == HookEvent::ConfigChange)
        .collect::<Vec<_>>();
    assert!(!settings.is_empty());
    for (operation, receipt) in settings {
        assert_eq!(receipt.facts.host_session, Some(original_lifetime));
        assert_eq!(receipt.facts.native_session, None);
        assert_eq!(receipt.facts.native_turn, None);
        assert!(receipt.facts.callback.is_none());
        assert_eq!(receipt.facts.task, None);
        assert_eq!(receipt.facts.child_owner, None);
        assert_eq!(operation.phase, "settings");
        assert!(matches!(
            &operation.budget,
            Some(BudgetRef::SessionHooks { session }) if session == &receipt.facts.session
        ));
    }
}

async fn held_owner_allow_and_deny(adapter: &str) {
    let root = tempfile::tempdir().unwrap();
    let peer_root = tempfile::tempdir().unwrap();
    let peer =
        crate::config_change_test_support::ActualOwnerPeer::start(peer_root.path(), adapter, true)
            .await
            .unwrap();
    let (handle, path) = owner_fixture(root.path(), &peer.connection);
    let original = std::fs::read(&path).unwrap();
    let gate = Arc::new(ToggleGate::new(true));
    let policy = plan(vec![registration(
        "actual-owner-toggle",
        gate.clone(),
        HandlerKind::Command,
        0,
    )]);
    let mut running = crate::config_change_test_support::start_actual_control(
        &handle,
        root.path(),
        peer.connection.clone(),
        policy,
        Limits {
            seconds: 60,
            model_calls: 4,
            tool_calls: 4,
        },
    )
    .await;
    let before = running.runtime.record().unwrap();
    let (lifetime_id, host_session) = host_lifetime(&before);
    let allowance = before
        .session_hook_allowance
        .as_ref()
        .unwrap()
        .allocation
        .clone();
    let identity = before.identity.clone();

    running.submit("held actual owner turn").await;
    eprintln!("installed owner: adapter={adapter} stage=submit-accepted");
    peer.wait_held().await;
    eprintln!(
        "installed owner: adapter={adapter} stage=provider-held request_count={}",
        peer.request_count()
    );
    if matches!(adapter, "claude" | "codex") {
        peer.assert_backend_workspace(root.path());
    }
    assert_eq!(peer.request_count(), 1);

    let mut denied = replacement_draft(&handle, "denied-next-model");
    let error = handle.save(&mut denied).await.unwrap_err();
    let error_chain = format!("{error:#}");
    let gate_calls = gate.calls.load(Ordering::SeqCst);
    let disk_unchanged = std::fs::read(&path).is_ok_and(|current| current == original);
    let role_unchanged = handle
        .role(Role::Creator, None)
        .is_ok_and(|role| role.model.as_deref() == peer.connection.model.as_deref());
    assert!(
        error
            .to_string()
            .contains("actual owner policy denied settings"),
        "installed owner: adapter={adapter} stage=denied-save gate_calls={gate_calls} request_count={} disk_unchanged={disk_unchanged} role_unchanged={role_unchanged} error={error_chain}",
        peer.request_count(),
    );
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert_eq!(
        handle.role(Role::Creator, None).unwrap().model,
        peer.connection.model
    );
    gate.deny.store(false, Ordering::SeqCst);
    let mut allowed = replacement_draft(&handle, "allowed-next-model");
    assert!(matches!(
        handle.save(&mut allowed).await,
        Ok(SaveStatus::Applied)
    ));
    assert_ne!(std::fs::read(&path).unwrap(), original);
    assert_eq!(
        handle.role(Role::Creator, None).unwrap().model.as_deref(),
        Some("allowed-next-model")
    );
    let during = running.runtime.record().unwrap();
    assert_eq!(
        during.identity, identity,
        "Settings changed the admitted owner"
    );
    assert!(during.task.is_none() && during.allocation.is_none());
    assert_settings_owner(&during, lifetime_id);
    let current_allowance = &during.session_hook_allowance.as_ref().unwrap().allocation;
    assert_eq!(current_allowance.started_ms, allowance.started_ms);
    assert_eq!(current_allowance.deadline_ms, allowance.deadline_ms);
    assert_eq!(current_allowance.model_calls, allowance.model_calls);
    assert_eq!(current_allowance.tool_calls, allowance.tool_calls);
    assert_eq!(
        current_allowance.usage.reported_input,
        allowance.usage.reported_input
    );
    assert_eq!(
        current_allowance.usage.reported_output,
        allowance.usage.reported_output
    );
    assert_eq!(host_session, host_lifetime(&during).1);
    assert_eq!(gate.calls.load(Ordering::SeqCst), 2);

    peer.release();
    running.wait_turn_finished().await;
    assert_eq!(peer.request_count(), 1);
    running.shutdown().await;
}

#[tokio::test]
async fn live_settings_gate_is_not_mischarged_as_an_unfinished_provider_turn() {
    let root = tempfile::tempdir().unwrap();
    let owner_peer =
        crate::config_change_test_support::ActualOwnerPeer::start(root.path(), "openai-api", false)
            .await
            .unwrap();
    let (handle, _) = owner_fixture(root.path(), &owner_peer.connection);
    let gate = Arc::new(HeldGate::new());
    let policy = plan(vec![registration(
        "live-settings-during-turn",
        gate.clone(),
        HandlerKind::Command,
        0,
    )]);
    let mut running = crate::config_change_test_support::start_actual_control(
        &handle,
        root.path(),
        owner_peer.connection.clone(),
        policy,
        Limits {
            seconds: 60,
            model_calls: 2,
            tool_calls: 0,
        },
    )
    .await;
    running.submit("actual owner handshake").await;
    running.wait_turn_finished().await;
    gate.hold();
    let pending = handle.start_save(replacement_draft(&handle, "live-settings-model"));
    gate.wait_entered().await;
    running
        .submit("actual unrelated turn while Settings is live")
        .await;
    running.wait_turn_finished().await;
    let during = running.runtime.record().unwrap();
    assert!(!during.recovery_pending);
    assert!(during.task.is_none());
    assert_eq!(owner_peer.request_count(), 2);
    pending.cancel();
    gate.release();
    let (outcome, cleanup) = pending.finish().await;
    assert!(outcome.is_err());
    assert!(cleanup.is_ok());
    assert!(!running.runtime.record().unwrap().recovery_pending);
    let mut fresh = replacement_draft(&handle, "fresh-after-known-cancellation");
    assert!(matches!(
        handle.save(&mut fresh).await,
        Ok(SaveStatus::Applied)
    ));
    assert!(!running.runtime.record().unwrap().recovery_pending);
    running.shutdown().await;
}

#[tokio::test]
async fn live_settings_owner_does_not_hide_a_separate_abandoned_settings_admission() {
    let root = tempfile::tempdir().unwrap();
    let owner_peer =
        crate::config_change_test_support::ActualOwnerPeer::start(root.path(), "openai-api", false)
            .await
            .unwrap();
    let (handle, _) = owner_fixture(root.path(), &owner_peer.connection);
    let gate = Arc::new(HeldGate::new());
    let policy = plan(vec![registration(
        "live-settings-with-foreign-admission",
        gate.clone(),
        HandlerKind::Command,
        0,
    )]);
    let mut running = crate::config_change_test_support::start_actual_control(
        &handle,
        root.path(),
        owner_peer.connection.clone(),
        policy,
        Limits {
            seconds: 60,
            model_calls: 2,
            tool_calls: 0,
        },
    )
    .await;
    running.submit("actual owner handshake").await;
    running.wait_turn_finished().await;
    gate.hold();
    let pending = handle.start_save(replacement_draft(&handle, "live-settings-model"));
    gate.wait_entered().await;

    let abandoned = running
        .runtime
        .update(|record| {
            let id = record.operations.len() as u64 + 1;
            record.operations.push(Operation {
                budget: None,
                usage_receipt: None,
                id,
                phase: "settings".into(),
                verification: None,
                call: None,
                result: None,
                tool_receipt: None,
                host_invocation: Some(HostInvocation::Backend),
                complete: false,
                reconciled: false,
                usage_reported: false,
                identity: Some(record.identity.clone()),
            });
            Ok(id)
        })
        .unwrap();
    running
        .submit("actual unrelated turn with an abandoned Settings admission")
        .await;
    running.wait_turn_finished().await;

    let record = running.runtime.record().unwrap();
    assert!(record.recovery_pending);
    let abandoned = record
        .operations
        .iter()
        .find(|operation| operation.id == abandoned)
        .unwrap();
    assert!(!abandoned.complete && !abandoned.reconciled);
    assert!(
        abandoned
            .usage_receipt
            .as_ref()
            .is_some_and(|receipt| receipt.missing_report)
    );

    pending.cancel();
    gate.release();
    let _ = pending.finish().await;
    running.shutdown().await;
}

#[tokio::test]
async fn live_settings_model_admission_remains_in_its_exact_owned_causal_tree() {
    let root = tempfile::tempdir().unwrap();
    let peer_root = tempfile::tempdir().unwrap();
    let owner_peer = crate::config_change_test_support::ActualOwnerPeer::start(
        peer_root.path(),
        "openai-api",
        false,
    )
    .await
    .unwrap();
    let (handle, _) = owner_fixture(root.path(), &owner_peer.connection);
    let (model_registration, model_peer) =
        crate::config_change_test_support::model_registration_delayed(
            HandlerKind::Prompt,
            true,
            std::time::Duration::from_secs(1),
        )
        .await;
    let mut running = crate::config_change_test_support::start_actual_control(
        &handle,
        root.path(),
        owner_peer.connection.clone(),
        plan(vec![model_registration]),
        Limits {
            seconds: 60,
            model_calls: 2,
            tool_calls: 0,
        },
    )
    .await;
    running.submit("actual owner handshake").await;
    running.wait_turn_finished().await;
    let pending = handle.start_save(replacement_draft(&handle, "causal-model-settings"));
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while model_peer.count() == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("Settings model request was not admitted");
    running
        .submit("actual unrelated turn while Settings model is in flight")
        .await;
    running.wait_turn_finished().await;
    assert!(!running.runtime.record().unwrap().recovery_pending);

    let (outcome, cleanup) = pending.finish().await;
    assert!(matches!(outcome, Ok(SaveStatus::Applied)), "{outcome:?}");
    assert!(cleanup.is_ok());
    let record = running.runtime.record().unwrap();
    let settings_owner = record
        .operations
        .iter()
        .find(|operation| {
            operation.non_tool_receipt().is_some_and(|receipt| {
                receipt.facts.subject.occurrence.event() == HookEvent::ConfigChange
                    && receipt.publication.is_some()
            })
        })
        .unwrap()
        .id;
    assert!(record.operations.iter().any(|operation| matches!(
        operation.host_invocation,
        Some(HostInvocation::HookModel { owner }) if owner == settings_owner
    )));
    assert!(!record.recovery_pending);
    assert_eq!(model_peer.count(), 1);
    assert_eq!(owner_peer.request_count(), 2);
    running.shutdown().await;
}

#[tokio::test]
async fn unknown_settings_effect_still_requires_recovery_after_owned_cleanup() {
    let root = tempfile::tempdir().unwrap();
    let owner_peer =
        crate::config_change_test_support::ActualOwnerPeer::start(root.path(), "openai-api", false)
            .await
            .unwrap();
    let (handle, path) = owner_fixture(root.path(), &owner_peer.connection);
    let original = std::fs::read(&path).unwrap();
    let policy = plan(vec![registration(
        "unknown-settings-effect",
        Arc::new(UnknownGate),
        HandlerKind::Command,
        0,
    )]);
    let mut running = crate::config_change_test_support::start_actual_control(
        &handle,
        root.path(),
        owner_peer.connection.clone(),
        policy,
        Limits {
            seconds: 60,
            model_calls: 1,
            tool_calls: 0,
        },
    )
    .await;
    running.submit("actual owner handshake").await;
    running.wait_turn_finished().await;
    let pending = handle.start_save(replacement_draft(&handle, "unknown-must-not-publish"));
    let (outcome, cleanup) = pending.finish().await;
    assert!(outcome.is_err());
    assert!(cleanup.is_ok());
    let record = running.runtime.record().unwrap();
    assert!(record.recovery_pending);
    assert!(record.operations.iter().any(|operation| {
        operation.non_tool_receipt().is_some_and(|receipt| {
            receipt.facts.subject.occurrence.event() == HookEvent::ConfigChange
                && receipt.publication.is_none()
                && receipt.hooks.iter().any(|hook| hook.uncertain_effects)
        })
    }));
    assert_eq!(std::fs::read(&path).unwrap(), original);
    running.shutdown().await;
}

#[tokio::test]
async fn cancelled_settings_with_source_delivery_cannot_be_safely_finalized() {
    let root = tempfile::tempdir().unwrap();
    let peer_root = tempfile::tempdir().unwrap();
    let owner_peer = crate::config_change_test_support::ActualOwnerPeer::start(
        peer_root.path(),
        "openai-api",
        false,
    )
    .await
    .unwrap();
    let (handle, path) = owner_fixture(root.path(), &owner_peer.connection);
    let original = std::fs::read(&path).unwrap();
    let gate = Arc::new(HeldGate::new());
    let mut running = crate::config_change_test_support::start_actual_control(
        &handle,
        root.path(),
        owner_peer.connection.clone(),
        plan(vec![registration(
            "source-delivery-must-not-be-invented",
            gate.clone(),
            HandlerKind::Command,
            0,
        )]),
        Limits {
            seconds: 60,
            model_calls: 1,
            tool_calls: 0,
        },
    )
    .await;
    running.submit("actual owner handshake").await;
    running.wait_turn_finished().await;
    gate.hold();
    let pending = handle.start_save(replacement_draft(&handle, "must-not-publish"));
    gate.wait_entered().await;
    let operation = running
        .runtime
        .update(|record| {
            let operation = record
                .operations
                .iter_mut()
                .find(|operation| {
                    operation.non_tool_receipt().is_some_and(|receipt| {
                        receipt.facts.subject.occurrence.event() == HookEvent::ConfigChange
                    })
                })
                .unwrap();
            let receipt = operation.non_tool_receipt_mut().unwrap();
            receipt.source_delivery = Some(crate::plugins::receipts::SourceDelivery::Pending);
            receipt.hold = Some("specific retained hold".into());
            Ok(operation.id)
        })
        .unwrap();
    pending.cancel();
    gate.release();
    let (outcome, cleanup) = pending.finish().await;
    assert!(outcome.is_err());
    assert!(cleanup.is_ok());
    let record = running.runtime.record().unwrap();
    assert!(record.recovery_pending);
    let operation = record
        .operations
        .iter()
        .find(|candidate| candidate.id == operation)
        .unwrap();
    let receipt = operation.non_tool_receipt().unwrap();
    assert!(!operation.complete && !receipt.settled);
    assert_eq!(
        receipt.source_delivery,
        Some(crate::plugins::receipts::SourceDelivery::Pending)
    );
    assert_eq!(receipt.hold.as_deref(), Some("specific retained hold"));
    assert!(receipt.publication.is_none());
    assert_eq!(std::fs::read(&path).unwrap(), original);
    running.shutdown().await;
}

#[tokio::test]
async fn held_openai_and_anthropic_owners_gate_settings_without_borrowing_the_turn() {
    for adapter in ["openai-api", "anthropic-api"] {
        held_owner_allow_and_deny(adapter).await;
    }
}

#[tokio::test]
#[ignore = "requires pinned installed Claude and Codex with controlled local model peers"]
async fn held_installed_claude_and_codex_owners_gate_settings_without_borrowing_the_turn() {
    for adapter in ["claude", "codex"] {
        held_owner_allow_and_deny(adapter).await;
    }
}

#[tokio::test]
async fn original_session_grant_is_required_and_cumulative_for_actual_model_handlers() {
    let root = tempfile::tempdir().unwrap();
    let owner_peer =
        crate::config_change_test_support::ActualOwnerPeer::start(root.path(), "openai-api", false)
            .await
            .unwrap();
    let (handle, path) = owner_fixture(root.path(), &owner_peer.connection);
    let original = std::fs::read(&path).unwrap();
    let (model_registration, model_peer) =
        crate::config_change_test_support::model_registration(HandlerKind::Prompt, true).await;
    let policy = plan(vec![model_registration]);
    let mut running = crate::config_change_test_support::start_actual_control(
        &handle,
        root.path(),
        owner_peer.connection.clone(),
        policy,
        Limits {
            seconds: 60,
            model_calls: 1,
            tool_calls: 0,
        },
    )
    .await;
    running.submit("actual owner handshake").await;
    running.wait_turn_finished().await;
    assert_eq!(owner_peer.request_count(), 1);
    let before = running.runtime.record().unwrap();
    let (lifetime_id, _) = host_lifetime(&before);
    let allowance_before = before
        .session_hook_allowance
        .as_ref()
        .unwrap()
        .allocation
        .clone();

    let mut first = replacement_draft(&handle, "first-funded-model");
    assert!(matches!(
        handle.save(&mut first).await,
        Ok(SaveStatus::Applied)
    ));
    assert_eq!(model_peer.count(), 1);
    let first_bytes = std::fs::read(&path).unwrap();
    assert_ne!(first_bytes, original);
    let after_first = running.runtime.record().unwrap();
    let first_allowance = &after_first
        .session_hook_allowance
        .as_ref()
        .unwrap()
        .allocation;
    assert_eq!(first_allowance.started_ms, allowance_before.started_ms);
    assert_eq!(first_allowance.deadline_ms, allowance_before.deadline_ms);
    assert_eq!(first_allowance.model_calls, 1);
    assert_eq!(first_allowance.tool_calls, 0);
    assert_eq!(first_allowance.usage.reported_input, 1);
    assert_eq!(first_allowance.usage.reported_output, 1);
    assert_settings_owner(&after_first, lifetime_id);

    let mut exhausted = replacement_draft(&handle, "must-not-pass-exhausted-model");
    let error = handle.save(&mut exhausted).await.unwrap_err();
    assert!(
        error
            .to_string()
            .contains("required lifecycle handler failed"),
        "{error:#}"
    );
    assert_eq!(model_peer.count(), 1, "exhausted grant reached the model");
    assert_eq!(std::fs::read(&path).unwrap(), first_bytes);
    assert_eq!(
        handle.role(Role::Creator, None).unwrap().model.as_deref(),
        Some("first-funded-model")
    );
    let exhausted_record = running.runtime.record().unwrap();
    let exhausted_allowance = &exhausted_record
        .session_hook_allowance
        .as_ref()
        .unwrap()
        .allocation;
    assert_eq!(exhausted_allowance.started_ms, allowance_before.started_ms);
    assert_eq!(
        exhausted_allowance.deadline_ms,
        allowance_before.deadline_ms
    );
    assert_eq!(exhausted_allowance.model_calls, 1);
    assert_eq!(exhausted_allowance.usage.reported_input, 1);
    assert_eq!(exhausted_allowance.usage.reported_output, 1);
    let exhausted_evidence = exhausted_record
        .operations
        .iter()
        .filter_map(|operation| operation.non_tool_receipt())
        .filter(|receipt| receipt.facts.subject.occurrence.event() == HookEvent::ConfigChange)
        .flat_map(|receipt| receipt.hooks.iter())
        .map(|hook| match &hook.outcome {
            Some(RawOutcome::Failure { reason }) => reason.clone(),
            Some(_) => "non-failure outcome".into(),
            None => hook.hold.clone().unwrap_or_else(|| "no outcome".into()),
        })
        .collect::<Vec<_>>();
    assert!(
        exhausted_evidence
            .iter()
            .any(|reason| reason == "configured hook model request failed"),
        "{exhausted_evidence:?}"
    );
    running.shutdown().await;
}

#[tokio::test]
async fn missing_and_expired_original_grants_hold_actual_required_model_handlers() {
    for expired in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let owner_peer = crate::config_change_test_support::ActualOwnerPeer::start(
            root.path(),
            "openai-api",
            false,
        )
        .await
        .unwrap();
        let (handle, path) = owner_fixture(root.path(), &owner_peer.connection);
        let original = std::fs::read(&path).unwrap();
        let (model_registration, model_peer) =
            crate::config_change_test_support::model_registration(HandlerKind::Prompt, true).await;
        let policy = plan(vec![model_registration]);
        let mut running = if expired {
            crate::config_change_test_support::start_actual_control(
                &handle,
                root.path(),
                owner_peer.connection.clone(),
                policy,
                Limits {
                    seconds: 1,
                    model_calls: 1,
                    tool_calls: 0,
                },
            )
            .await
        } else {
            crate::config_change_test_support::start_actual_control_without_grant(
                &handle,
                root.path(),
                owner_peer.connection.clone(),
                policy,
            )
            .await
        };
        running.submit("actual owner handshake").await;
        running.wait_turn_finished().await;
        if expired {
            tokio::time::sleep(Duration::from_millis(1_100)).await;
        }

        let mut draft = replacement_draft(
            &handle,
            if expired {
                "expired-model"
            } else {
                "missing-model"
            },
        );
        let error = handle.save(&mut draft).await.unwrap_err();
        assert!(
            error
                .to_string()
                .contains("required ConfigChange handler lacks its original session allowance"),
            "{error:#}"
        );
        assert_eq!(model_peer.count(), 0);
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert_eq!(
            handle.role(Role::Creator, None).unwrap().model,
            owner_peer.connection.model
        );
        assert!(
            running
                .runtime
                .record()
                .unwrap()
                .operations
                .iter()
                .any(|operation| {
                    operation.non_tool_receipt().is_some_and(|receipt| {
                        receipt.facts.subject.occurrence.event() == HookEvent::ConfigChange
                            && receipt.hold.as_deref().is_some_and(|reason| {
                                reason.contains("required ConfigChange handler lacks")
                            })
                            && receipt.publication.is_none()
                    })
                })
        );
        running.shutdown().await;
    }
}

#[tokio::test]
async fn zero_tool_counter_still_runs_required_mcp_under_its_original_service_cap() {
    let root = tempfile::tempdir().unwrap();
    let owner_peer =
        crate::config_change_test_support::ActualOwnerPeer::start(root.path(), "openai-api", false)
            .await
            .unwrap();
    let (handle, _path) = owner_fixture(root.path(), &owner_peer.connection);
    let (mcp_registration, mcp_peer, service) =
        crate::config_change_test_support::mcp_registration(root.path(), HookDialect::Native, true)
            .await;
    let policy = plan(vec![mcp_registration]);
    let mut running = crate::config_change_test_support::start_actual_control(
        &handle,
        root.path(),
        owner_peer.connection.clone(),
        policy,
        Limits {
            seconds: 60,
            model_calls: 1,
            tool_calls: 0,
        },
    )
    .await;
    running.submit("actual owner handshake").await;
    running.wait_turn_finished().await;

    let before = running.runtime.record().unwrap();
    let (lifetime_id, _) = host_lifetime(&before);
    let allowance_before = before
        .session_hook_allowance
        .as_ref()
        .unwrap()
        .allocation
        .clone();
    let mut draft = replacement_draft(&handle, "mcp-approved-model");
    assert!(matches!(
        handle.save(&mut draft).await,
        Ok(SaveStatus::Applied)
    ));
    assert_eq!(mcp_peer.method_count("tools/call"), 1);
    let after = running.runtime.record().unwrap();
    let allowance = &after.session_hook_allowance.as_ref().unwrap().allocation;
    assert_eq!(allowance.started_ms, allowance_before.started_ms);
    assert_eq!(allowance.deadline_ms, allowance_before.deadline_ms);
    assert_eq!(allowance.model_calls, 0);
    assert_eq!(allowance.tool_calls, 0);
    assert_settings_owner(&after, lifetime_id);
    service.stop().await.unwrap();
    running.shutdown().await;
}

#[tokio::test]
async fn retained_mcp_service_accepts_fresh_settings_after_creator_replacement() {
    let root = tempfile::tempdir().unwrap();
    let owner_peer =
        crate::config_change_test_support::ActualOwnerPeer::start(root.path(), "openai-api", false)
            .await
            .unwrap();
    let (handle, path) = owner_fixture(root.path(), &owner_peer.connection);
    let (mcp_registration, mcp_peer, service) =
        crate::config_change_test_support::mcp_registration(root.path(), HookDialect::Native, true)
            .await;
    let policy = plan(vec![mcp_registration]);
    let mut running = crate::config_change_test_support::start_actual_control(
        &handle,
        root.path(),
        owner_peer.connection.clone(),
        policy,
        Limits {
            seconds: 60,
            model_calls: 2,
            tool_calls: 0,
        },
    )
    .await;
    running.submit("actual owner handshake").await;
    running.wait_turn_finished().await;

    let before = running.runtime.record().unwrap();
    let (lifetime_id, _) = host_lifetime(&before);
    let allowance_before = before
        .session_hook_allowance
        .as_ref()
        .unwrap()
        .allocation
        .clone();
    let mut first = replacement_draft(&handle, "creator-after-save");
    assert!(matches!(
        handle.save(&mut first).await,
        Ok(SaveStatus::Applied)
    ));
    assert_eq!(mcp_peer.method_count("tools/call"), 1);
    assert_eq!(mcp_peer.method_count("initialize"), 1);
    assert_eq!(
        service.state(),
        crate::plugins::services::ServiceState::Ready
    );
    assert_eq!(running.runtime.record().unwrap().identity, before.identity);

    running.submit("actual replacement turn").await;
    running.wait_turn_finished().await;
    let replaced = running.runtime.record().unwrap();
    assert_ne!(replaced.identity, before.identity);
    assert_eq!(
        serde_json::to_value(&replaced.identity).unwrap()["model"],
        "creator-after-save"
    );
    assert_eq!(owner_peer.request_count(), 2);
    assert_eq!(
        owner_peer.request_models(),
        ["owner-model", "creator-after-save"]
    );

    let mut fresh = replacement_draft(&handle, "fresh-settings-current-identity");
    assert!(matches!(
        handle.save(&mut fresh).await,
        Ok(SaveStatus::Applied)
    ));
    assert_eq!(mcp_peer.method_count("tools/call"), 2);
    assert_eq!(mcp_peer.method_count("initialize"), 1);
    assert_eq!(
        service.state(),
        crate::plugins::services::ServiceState::Ready
    );
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        toml::to_string_pretty(&fresh.config).unwrap()
    );
    let after = running.runtime.record().unwrap();
    let allowance = &after.session_hook_allowance.as_ref().unwrap().allocation;
    assert_eq!(allowance.started_ms, allowance_before.started_ms);
    assert_eq!(allowance.deadline_ms, allowance_before.deadline_ms);
    assert_eq!(allowance.model_calls, allowance_before.model_calls);
    assert_eq!(allowance.tool_calls, allowance_before.tool_calls);
    assert_settings_owner(&after, lifetime_id);
    assert!(
        after
            .operations
            .iter()
            .filter_map(|operation| operation.non_tool_receipt())
            .filter(|receipt| {
                receipt.facts.subject.occurrence.event() == HookEvent::ConfigChange
            })
            .all(|receipt| receipt.facts.host_session == Some(lifetime_id)
                && receipt.facts.task.is_none()
                && receipt.facts.child_owner.is_none()
                && receipt.facts.role == "settings")
    );
    assert!(after.operations.iter().any(|operation| {
        operation.id == lifetime_id && operation.identity.as_ref() == Some(&before.identity)
    }));
    service.stop().await.unwrap();
    running.shutdown().await;
}

#[tokio::test]
async fn fresh_mcp_service_admits_settings_only_after_actual_creator_replacement() {
    let root = tempfile::tempdir().unwrap();
    let owner_peer =
        crate::config_change_test_support::ActualOwnerPeer::start(root.path(), "openai-api", false)
            .await
            .unwrap();
    let (handle, _path) = owner_fixture(root.path(), &owner_peer.connection);
    let mut selected = replacement_draft(&handle, "creator-before-session-turn");
    assert!(matches!(
        handle.save(&mut selected).await,
        Ok(SaveStatus::Applied)
    ));
    let (mcp_registration, mcp_peer, service) =
        crate::config_change_test_support::mcp_registration(root.path(), HookDialect::Native, true)
            .await;
    let policy = plan(vec![mcp_registration]);
    let mut running = crate::config_change_test_support::start_actual_control(
        &handle,
        root.path(),
        owner_peer.connection.clone(),
        policy,
        Limits {
            seconds: 60,
            model_calls: 1,
            tool_calls: 0,
        },
    )
    .await;
    let before = running.runtime.record().unwrap();
    let (lifetime_id, _) = host_lifetime(&before);
    let allowance_before = before
        .session_hook_allowance
        .as_ref()
        .unwrap()
        .allocation
        .clone();
    assert_eq!(mcp_peer.method_count("initialize"), 0);
    running
        .submit("actual first turn selects saved Creator")
        .await;
    running.wait_turn_finished().await;
    let replaced = running.runtime.record().unwrap();
    assert_ne!(replaced.identity, before.identity);
    assert_eq!(owner_peer.request_models(), ["creator-before-session-turn"]);

    let mut fresh = replacement_draft(&handle, "fresh-mcp-after-replacement");
    assert!(matches!(
        handle.save(&mut fresh).await,
        Ok(SaveStatus::Applied)
    ));
    assert_eq!(mcp_peer.method_count("initialize"), 1);
    assert_eq!(mcp_peer.method_count("tools/call"), 1);
    assert_eq!(
        service.state(),
        crate::plugins::services::ServiceState::Ready
    );
    let after = running.runtime.record().unwrap();
    let allowance = &after.session_hook_allowance.as_ref().unwrap().allocation;
    assert_eq!(allowance.started_ms, allowance_before.started_ms);
    assert_eq!(allowance.deadline_ms, allowance_before.deadline_ms);
    assert_eq!(allowance.model_calls, allowance_before.model_calls);
    assert_eq!(allowance.tool_calls, allowance_before.tool_calls);
    assert_settings_owner(&after, lifetime_id);
    service.stop().await.unwrap();
    running.shutdown().await;
}

#[tokio::test]
async fn creator_replacement_invalidates_held_old_settings_and_keeps_control_required() {
    let root = tempfile::tempdir().unwrap();
    let owner_peer =
        crate::config_change_test_support::ActualOwnerPeer::start(root.path(), "openai-api", false)
            .await
            .unwrap();
    let (handle, path) = owner_fixture(root.path(), &owner_peer.connection);
    let gate = Arc::new(HeldGate::new());
    let (mut mcp_registration, mcp_peer, service) =
        crate::config_change_test_support::mcp_registration_with_max_calls(
            root.path(),
            HookDialect::Native,
            true,
            2,
        )
        .await;
    mcp_registration.declaration.priority = 1;
    let policy = plan(vec![
        registration("replacement-hold", gate.clone(), HandlerKind::Command, 0),
        mcp_registration,
    ]);
    let mut connection = owner_peer.connection.clone();
    connection.access.non_tools = vec![policy];
    let actual = crate::adapters::builtins()
        .unwrap()
        .open(&connection, root.path())
        .unwrap();
    let (paused, close) = crate::config_change_test_support::pause_session_close(actual);
    let mut running = crate::config_change_test_support::start_actual_control_with_session(
        &handle,
        root.path(),
        connection,
        Limits {
            seconds: 60,
            model_calls: 2,
            tool_calls: 0,
        },
        Some(paused),
    )
    .await;
    running.submit("actual owner handshake").await;
    running.wait_turn_finished().await;
    let before = running.runtime.record().unwrap();
    let (lifetime_id, _) = host_lifetime(&before);
    let allowance_before = before
        .session_hook_allowance
        .as_ref()
        .unwrap()
        .allocation
        .clone();

    let mut replacement = replacement_draft(&handle, "replacement-current-model");
    assert!(matches!(
        handle.save(&mut replacement).await,
        Ok(SaveStatus::Applied)
    ));
    assert_eq!(mcp_peer.method_count("tools/call"), 1);
    assert_eq!(mcp_peer.method_count("initialize"), 1);
    let saved = std::fs::read(&path).unwrap();

    gate.hold();
    let held = replacement_draft(&handle, "held-old-identity-model");
    let held_digest = digest_hex(toml::to_string_pretty(&held.config).unwrap().as_bytes());
    let pending = handle.start_save(held);
    gate.wait_entered().await;
    running.submit("actual eligible replacement turn").await;
    close.wait_entered().await;
    let required_error = handle
        .save(&mut replacement_draft(
            &handle,
            "cannot-run-during-replacement",
        ))
        .await
        .unwrap_err();
    assert!(
        required_error
            .to_string()
            .contains("settings policy is active but its host capability is not ready")
    );
    assert_eq!(std::fs::read(&path).unwrap(), saved);
    assert_eq!(mcp_peer.method_count("tools/call"), 1);

    let replaced = running.runtime.record().unwrap();
    assert_ne!(replaced.identity, before.identity);
    gate.release();
    let (old_outcome, old_cleanup) = pending.finish().await;
    let old_error = old_outcome.unwrap_err();
    assert!(
        old_cleanup.is_ok(),
        "held old Settings cleanup: {old_cleanup:?}"
    );
    assert!(
        format!("{old_error:#}")
            .contains("settings control owner, identity, workspace or host session changed"),
        "unexpected held old Settings failure: {old_error:#}"
    );
    assert_eq!(std::fs::read(&path).unwrap(), saved);
    assert_eq!(mcp_peer.method_count("tools/call"), 1);
    let invalidated = running.runtime.record().unwrap();
    assert!(invalidated.operations.iter().any(|operation| {
        operation.non_tool_receipt().is_some_and(|receipt| {
            matches!(
                &receipt.facts.subject.occurrence,
                NonToolOccurrence::ConfigChange { proposed_digest, .. }
                    if proposed_digest == &held_digest
            ) && operation.complete
                && receipt.settled
                && receipt.hold.as_deref()
                    == Some("Settings attempt invalidated before publication")
                && receipt.publication.is_none()
        })
    }));
    let selected = invalidated
        .operations
        .iter()
        .filter_map(|operation| {
            operation.non_tool_receipt().map(|receipt| {
                (
                    operation.id,
                    operation.complete,
                    receipt.settled,
                    receipt
                        .hooks
                        .iter()
                        .map(|hook| hook.uncertain_effects)
                        .collect::<Vec<_>>(),
                )
            })
        })
        .collect::<Vec<_>>();
    assert!(
        !invalidated.recovery_pending,
        "old Settings invalidation created recovery: {selected:?}"
    );
    close.release();
    running.wait_turn_finished().await;
    assert_eq!(
        owner_peer.request_models(),
        ["owner-model", "replacement-current-model"]
    );

    let mut fresh = replacement_draft(&handle, "fresh-after-held-old-model");
    let fresh_status = handle.save(&mut fresh).await;
    assert!(
        matches!(fresh_status, Ok(SaveStatus::Applied)),
        "fresh Settings after replacement failed: {fresh_status:?}"
    );
    assert_eq!(mcp_peer.method_count("tools/call"), 2);
    assert_eq!(mcp_peer.method_count("initialize"), 1);
    assert_eq!(
        service.state(),
        crate::plugins::services::ServiceState::Ready
    );
    let fresh_bytes = std::fs::read(&path).unwrap();

    let mut exhausted = replacement_draft(&handle, "must-not-exceed-retained-cap");
    let exhausted_error = handle.save(&mut exhausted).await.unwrap_err();
    assert!(
        format!("{exhausted_error:#}").contains("required lifecycle handler failed"),
        "unexpected retained MCP cap failure: {exhausted_error:#}"
    );
    assert_eq!(mcp_peer.method_count("tools/call"), 2);
    assert_eq!(mcp_peer.method_count("initialize"), 1);
    assert_eq!(
        service.state(),
        crate::plugins::services::ServiceState::Failed
    );
    assert_eq!(std::fs::read(&path).unwrap(), fresh_bytes);
    let after = running.runtime.record().unwrap();
    let allowance = &after.session_hook_allowance.as_ref().unwrap().allocation;
    assert_eq!(allowance.started_ms, allowance_before.started_ms);
    assert_eq!(allowance.deadline_ms, allowance_before.deadline_ms);
    assert_eq!(allowance.model_calls, allowance_before.model_calls);
    assert_eq!(allowance.tool_calls, allowance_before.tool_calls);
    assert_settings_owner(&after, lifetime_id);
    service.stop().await.unwrap();
    running.shutdown().await;
}

#[tokio::test]
async fn malformed_actual_http_gate_reply_cannot_publish_settings() {
    let root = tempfile::tempdir().unwrap();
    let owner_peer =
        crate::config_change_test_support::ActualOwnerPeer::start(root.path(), "openai-api", false)
            .await
            .unwrap();
    let (handle, path) = owner_fixture(root.path(), &owner_peer.connection);
    let original = std::fs::read(&path).unwrap();
    let (malformed, gate_peer) =
        crate::config_change_test_support::malformed_http_registration().await;
    let policy = plan(vec![malformed]);
    let mut running = crate::config_change_test_support::start_actual_control(
        &handle,
        root.path(),
        owner_peer.connection.clone(),
        policy,
        Limits {
            seconds: 60,
            model_calls: 1,
            tool_calls: 1,
        },
    )
    .await;
    running.submit("actual owner handshake").await;
    running.wait_turn_finished().await;

    let mut draft = replacement_draft(&handle, "must-not-publish-malformed");
    let error = handle.save(&mut draft).await.unwrap_err();
    assert!(
        !error.to_string().is_empty(),
        "malformed gate failure was not visible"
    );
    assert_eq!(gate_peer.count(), 1);
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert_eq!(
        handle.role(Role::Creator, None).unwrap().model,
        owner_peer.connection.model
    );
    let record = running.runtime.record().unwrap();
    assert!(record.operations.iter().any(|operation| {
        operation.non_tool_receipt().is_some_and(|receipt| {
            receipt.facts.subject.occurrence.event() == HookEvent::ConfigChange
                && receipt.hooks.iter().any(|hook| {
                    hook.uncertain_effects
                        && matches!(
                            &hook.outcome,
                            Some(RawOutcome::Failure { reason })
                                if reason == "HTTP hook returned invalid JSON or encoding"
                        )
                })
                && receipt.publication.is_none()
        })
    }));
    running.shutdown().await;
}
