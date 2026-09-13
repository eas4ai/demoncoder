use super::*;
use crate::plugins::receipts::{NonToolOccurrence, ObservedLifecycle};
use std::os::unix::fs::MetadataExt;
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

struct KnownFailureGate;

#[async_trait::async_trait]
impl HookRunner for UnknownGate {
    async fn run(&self, _: &HookInvocation) -> Result<RawOutcome> {
        anyhow::bail!("controlled gate outcome is unknown")
    }
}

#[async_trait::async_trait]
impl HookRunner for KnownFailureGate {
    fn side_effect_free(&self) -> bool {
        true
    }

    async fn run(&self, _: &HookInvocation) -> Result<RawOutcome> {
        anyhow::bail!("controlled side-effect-free gate failure")
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
    plan_for(HookEvent::ConfigChange, registrations)
}

fn plan_for(event: HookEvent, registrations: Vec<Registration>) -> Arc<NonToolPlan> {
    Arc::new(NonToolPlan::new(event, registrations).unwrap())
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

fn controlled_claude_without_model(
    root: &std::path::Path,
    catalog: serde_json::Value,
) -> crate::config::Connection {
    let binary = root.join("claude-without-model.py");
    std::fs::write(
        root.join("claude-init-catalog.json"),
        serde_json::to_vec(&catalog).unwrap(),
    )
    .unwrap();
    std::fs::write(
        &binary,
        r#"#!/usr/bin/python3
import json,pathlib,sys
def send(value): print(json.dumps(value),flush=True)
initialize=json.loads(input())
catalog=json.loads(pathlib.Path('claude-init-catalog.json').read_text())
send({'type':'control_response','response':{'subtype':'success','request_id':initialize['request_id'],'response':catalog}})
users=0
for raw in sys.stdin:
    message=json.loads(raw)
    if message.get('type')!='user': continue
    users+=1
    pathlib.Path('claude-user-count').write_text(str(users))
    send({'type':'system','subtype':'init','session_id':'missing-model-session','apiKeySource':'none'})
    send({'type':'result','subtype':'success','is_error':False,'session_id':'missing-model-session','usage':{'input_tokens':1,'output_tokens':1}})
"#,
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut connection: crate::config::Connection = serde_json::from_value(serde_json::json!({
        "adapter":"claude",
        "model":"original-alias",
        "binary":binary,
    }))
    .unwrap();
    connection.access.supervisor = Some(crate::config_change_test_support::supervisor());
    connection
}

#[tokio::test]
async fn ordinary_claude_ignores_model_switch_catalog_without_model_switch_policy() {
    let root = tempfile::tempdir().unwrap();
    let connection = controlled_claude_without_model(
        root.path(),
        serde_json::json!({"models":[{"value":"malformed-without-resolution"}]}),
    );
    let (handle, _) = owner_fixture(root.path(), &connection);
    let mut running = crate::config_change_test_support::start_actual_control_with_session(
        &handle,
        root.path(),
        connection,
        Limits {
            seconds: 60,
            model_calls: 0,
            tool_calls: 0,
        },
        None,
    )
    .await;

    running.submit("ordinary authenticated Claude turn").await;
    running.wait_turn_finished().await;
    assert_eq!(
        std::fs::read_to_string(root.path().join("claude-user-count")).unwrap(),
        "1"
    );
    running.shutdown().await;
}

#[tokio::test]
async fn missing_claude_model_knowledge_cannot_prove_noop_or_release_source_policy() {
    let root = tempfile::tempdir().unwrap();
    let mut connection = controlled_claude_without_model(
        root.path(),
        serde_json::json!({"models":[
            {"value":"original-alias","resolvedModel":"resolved-current"},
            {"value":"same-model-alias","resolvedModel":"resolved-current"}
        ]}),
    );
    let gate = Arc::new(ToggleGate::new(false));
    let mut pre = registration(
        "missing-model-source-policy",
        gate.clone(),
        HandlerKind::Command,
        0,
    );
    pre.declaration.identity.dialect = HookDialect::Claude;
    pre.declaration.concurrent_group = Some("missing-model-source".into());
    connection.access.non_tools = vec![plan_for(HookEvent::PreModelSwitch, vec![pre])];
    let (handle, _) = owner_fixture(root.path(), &connection);
    let mut running = crate::config_change_test_support::start_actual_control_with_session(
        &handle,
        root.path(),
        connection,
        Limits {
            seconds: 60,
            model_calls: 0,
            tool_calls: 0,
        },
        None,
    )
    .await;
    running
        .submit("initialize missing-model Claude owner")
        .await;
    running.wait_turn_finished().await;
    let before = running.runtime.record().unwrap();

    let mut replacement = replacement_draft(&handle, "same-model-alias");
    assert!(matches!(
        handle.save(&mut replacement).await,
        Ok(SaveStatus::Applied)
    ));
    running
        .submit("must remain held without authenticated model knowledge")
        .await;
    let events = running.collect_turn_events("failed").await;

    assert_eq!(gate.calls.load(Ordering::SeqCst), 0);
    assert!(assignment_models(&events).is_empty());
    assert_eq!(
        std::fs::read_to_string(root.path().join("claude-user-count")).unwrap(),
        "1"
    );
    let after = running.runtime.record().unwrap();
    assert_eq!(after.identity, before.identity);
    let receipt = after
        .operations
        .iter()
        .filter_map(|operation| operation.non_tool_receipt())
        .find(|receipt| {
            matches!(
                receipt.facts.subject.occurrence,
                NonToolOccurrence::PreModelSwitch { .. }
            )
        })
        .expect("held PreModelSwitch receipt");
    assert!(receipt.facts.source.is_none());
    assert!(receipt.facts.callback.is_none());
    assert_eq!(
        receipt.hold.as_deref(),
        Some(
            "required PreModelSwitch handler lacks its original session allowance or truthful source callback"
        )
    );
    running.shutdown().await;
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

fn expire_native_observation_window(
    runtime: &crate::workflow::runtime::SharedRuntime,
    lifetime_id: u64,
) {
    runtime
        .update(|record| {
            let lifetime = record
                .operations
                .iter_mut()
                .find(|operation| operation.id == lifetime_id)
                .and_then(|operation| match &mut operation.host_invocation {
                    Some(HostInvocation::NativeSession(lifetime)) => Some(lifetime),
                    _ => None,
                })
                .expect("native lifetime");
            lifetime.deadline = Some(
                std::time::Instant::now()
                    .checked_sub(std::time::Duration::from_secs(1))
                    .unwrap(),
            );
            Ok(())
        })
        .unwrap();
}

fn expire_session_hook_allowance(runtime: &crate::workflow::runtime::SharedRuntime) {
    runtime
        .update(|record| {
            let allocation = &mut record
                .session_hook_allowance
                .as_mut()
                .expect("session hook allowance")
                .allocation;
            allocation.deadline_ms = allocation.observed_ms;
            Ok(())
        })
        .unwrap();
}

fn allowance_identity(record: &Record) -> Option<(u64, u64, u64, u64)> {
    record.session_hook_allowance.as_ref().map(|allowance| {
        let allocation = &allowance.allocation;
        (
            allocation.started_ms,
            allocation.deadline_ms,
            allocation.model_calls,
            allocation.tool_calls,
        )
    })
}

fn assignment_models(events: &[crate::events::Event]) -> Vec<Option<String>> {
    events
        .iter()
        .filter_map(|event| match event {
            crate::events::Event::ModelAssignment { model, .. } => Some(model.clone()),
            _ => None,
        })
        .collect()
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
    assert_eq!(
        replaced.identity, before.identity,
        "creator replacement published its durable identity before the old provider closed"
    );
    gate.release();
    let (old_outcome, old_cleanup) = pending.finish().await;
    let old_error = old_outcome.unwrap_err();
    assert!(
        old_cleanup.is_ok(),
        "held old Settings cleanup: {old_cleanup:?}"
    );
    assert!(
        format!("{old_error:#}").contains(
            "settings control expired, was cancelled, or belongs to another host session"
        ),
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
    assert_ne!(running.runtime.record().unwrap().identity, before.identity);
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
async fn denied_then_allowed_creator_model_switch_uses_exact_provider_and_receipts() {
    let root = tempfile::tempdir().unwrap();
    let owner_peer =
        crate::config_change_test_support::ActualOwnerPeer::start(root.path(), "openai-api", false)
            .await
            .unwrap();
    let (handle, _) = owner_fixture(root.path(), &owner_peer.connection);
    let gate = Arc::new(ToggleGate::new(true));
    let mut connection = owner_peer.connection.clone();
    let pre = registration("model-switch-deny", gate.clone(), HandlerKind::Command, 0);
    let mut post = registration(
        "model-switch-observe",
        gate.clone(),
        HandlerKind::Command,
        0,
    );
    post.declaration.required_gate = false;
    connection.access.non_tools = vec![
        plan_for(HookEvent::PreModelSwitch, vec![pre]),
        plan_for(HookEvent::PostModelSwitch, vec![post]),
    ];
    let mut running = crate::config_change_test_support::start_actual_control_with_session(
        &handle,
        root.path(),
        connection,
        Limits {
            seconds: 60,
            model_calls: 0,
            tool_calls: 0,
        },
        None,
    )
    .await;
    running.submit("old provider request").await;
    running.wait_turn_finished().await;
    let before = running.runtime.record().unwrap();

    let mut replacement = replacement_draft(&handle, "denied-new-model");
    assert!(matches!(
        handle.save(&mut replacement).await,
        Ok(SaveStatus::Applied)
    ));
    running.submit("must remain pending").await;
    let errors = running.wait_turn_failed().await;

    assert_eq!(
        gate.calls.load(Ordering::SeqCst),
        1,
        "turn errors: {errors:?}; operations: {:?}",
        running
            .runtime
            .record()
            .unwrap()
            .operations
            .iter()
            .map(|operation| (&operation.phase, operation.complete))
            .collect::<Vec<_>>()
    );
    let denied = running.runtime.record().unwrap();
    assert_eq!(denied.identity, before.identity);
    assert!(
        errors.iter().any(|message| {
            message.contains("This submission was retained but was not executed")
        })
    );
    assert!(denied.messages.windows(2).any(|messages| {
        messages[0].role == "developer"
            && messages[0].text == "must remain pending"
            && messages[1].role == "assistant"
            && messages[1]
                .text
                .contains("This submission was retained but was not executed")
    }));
    assert_eq!(owner_peer.request_models(), ["owner-model"]);

    gate.deny.store(false, Ordering::SeqCst);
    running.submit("fresh explicit submission").await;
    running.wait_turn_finished().await;
    let applied = running.runtime.record().unwrap();
    assert_ne!(applied.identity, before.identity);
    assert_eq!(gate.calls.load(Ordering::SeqCst), 3);
    assert_eq!(
        owner_peer.request_models(),
        ["owner-model", "denied-new-model"]
    );
    let model_events = applied
        .operations
        .iter()
        .filter_map(|operation| operation.non_tool_receipt())
        .filter(|receipt| {
            matches!(
                receipt.facts.subject.occurrence,
                NonToolOccurrence::PreModelSwitch { .. }
                    | NonToolOccurrence::PostModelSwitch { .. }
            )
        })
        .map(|receipt| receipt.facts.subject.occurrence.event())
        .collect::<Vec<_>>();
    assert_eq!(
        model_events,
        [
            HookEvent::PreModelSwitch,
            HookEvent::PreModelSwitch,
            HookEvent::PostModelSwitch
        ]
    );
    running.shutdown().await;
}

#[tokio::test]
async fn cancel_during_held_model_switch_pre_does_not_apply_or_replay() {
    let root = tempfile::tempdir().unwrap();
    let owner_peer =
        crate::config_change_test_support::ActualOwnerPeer::start(root.path(), "openai-api", false)
            .await
            .unwrap();
    let (handle, _) = owner_fixture(root.path(), &owner_peer.connection);
    let gate = Arc::new(HeldGate::new());
    let pre = registration(
        "held-model-switch-cancel",
        gate.clone(),
        HandlerKind::Command,
        0,
    );
    let mut connection = owner_peer.connection.clone();
    connection.access.non_tools = vec![plan_for(HookEvent::PreModelSwitch, vec![pre])];
    let mut running = crate::config_change_test_support::start_actual_control_with_session(
        &handle,
        root.path(),
        connection,
        Limits {
            seconds: 60,
            model_calls: 0,
            tool_calls: 0,
        },
        None,
    )
    .await;
    running
        .submit("establish provider before cancellation")
        .await;
    running.wait_turn_finished().await;
    let before = running.runtime.record().unwrap();

    let mut replacement = replacement_draft(&handle, "model-after-cancel");
    assert!(matches!(
        handle.save(&mut replacement).await,
        Ok(SaveStatus::Applied)
    ));
    gate.hold();
    running.submit("cancel this selected submission").await;
    gate.wait_entered().await;
    running.cancel().await;
    running.wait_turn_cancelled().await;

    let cancelled = running.runtime.record().unwrap();
    assert_eq!(cancelled.identity, before.identity);
    assert!(cancelled.recovery_pending);
    assert_eq!(gate.calls.load(Ordering::SeqCst), 1);
    assert_eq!(owner_peer.request_models(), ["owner-model"]);

    gate.release();
    running.shutdown().await;
}

async fn native_final_validation_command_case(shutdown: bool) {
    let root = tempfile::tempdir().unwrap();
    let owner_peer =
        crate::config_change_test_support::ActualOwnerPeer::start(root.path(), "openai-api", false)
            .await
            .unwrap();
    let (handle, _) = owner_fixture(root.path(), &owner_peer.connection);
    let gate = Arc::new(ToggleGate::new(false));
    let pre = registration(
        "native-final-validation-command",
        gate.clone(),
        HandlerKind::Command,
        0,
    );
    let mut connection = owner_peer.connection.clone();
    connection.access.non_tools = vec![plan_for(HookEvent::PreModelSwitch, vec![pre])];
    let mut running = crate::config_change_test_support::start_actual_control_with_session(
        &handle,
        root.path(),
        connection,
        Limits {
            seconds: 60,
            model_calls: 0,
            tool_calls: 0,
        },
        None,
    )
    .await;
    running
        .submit("establish native final-validation owner")
        .await;
    running.wait_turn_finished().await;
    let before = running.runtime.record().unwrap();

    let mut replacement = replacement_draft(&handle, "native-after-final-validation");
    assert!(matches!(
        handle.save(&mut replacement).await,
        Ok(SaveStatus::Applied)
    ));
    let (pre_complete, release_validation) =
        running.pause_next_before_model_switch_final_validation();
    running
        .submit("must not pass a queued final-boundary command")
        .await;
    tokio::time::timeout(std::time::Duration::from_secs(10), pre_complete)
        .await
        .expect("native PreModelSwitch did not reach final validation")
        .expect("native final-validation pause was dropped");
    let metadata = std::fs::metadata(root.path()).unwrap();
    let mutation = running
        .runtime
        .mutation_boundary((metadata.dev(), metadata.ino()))
        .unwrap()
        .lock_owned()
        .await;
    release_validation.send(()).unwrap();

    if shutdown {
        let runtime = running.runtime.clone();
        running.request_shutdown().await;
        let prompt_serviced = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while !running.worker_finished() {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .is_ok();
        drop(mutation);
        running.finish_shutdown().await;
        assert!(prompt_serviced, "Shutdown waited behind final validation");
        let after = runtime.record().unwrap();
        assert_eq!(after.identity, before.identity);
        assert!(!after.recovery_pending);
    } else {
        running.cancel().await;
        let timely = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            running.wait_turn_status(),
        )
        .await;
        drop(mutation);
        let status = match timely {
            Ok(status) => status,
            Err(_) => running.wait_turn_status().await,
        };
        let after = running.runtime.record().unwrap();
        running.shutdown().await;
        assert_eq!(status, "cancelled", "Cancel waited behind final validation");
        assert_eq!(after.identity, before.identity);
        assert!(!after.recovery_pending);
    }
    assert_eq!(gate.calls.load(Ordering::SeqCst), 1);
    assert_eq!(owner_peer.request_models(), ["owner-model"]);
}

#[tokio::test]
async fn cancel_while_native_final_validation_waits_prevents_teardown() {
    native_final_validation_command_case(false).await;
}

#[tokio::test]
async fn shutdown_while_native_final_validation_waits_prevents_teardown() {
    native_final_validation_command_case(true).await;
}

#[tokio::test]
async fn recovery_raised_after_native_pre_prevents_teardown_without_replay() {
    let root = tempfile::tempdir().unwrap();
    let owner_peer =
        crate::config_change_test_support::ActualOwnerPeer::start(root.path(), "openai-api", false)
            .await
            .unwrap();
    let (handle, _) = owner_fixture(root.path(), &owner_peer.connection);
    let gate = Arc::new(ToggleGate::new(false));
    let pre = registration(
        "native-final-recovery",
        gate.clone(),
        HandlerKind::Command,
        0,
    );
    let mut connection = owner_peer.connection.clone();
    connection.access.non_tools = vec![plan_for(HookEvent::PreModelSwitch, vec![pre])];
    let mut running = crate::config_change_test_support::start_actual_control_with_session(
        &handle,
        root.path(),
        connection,
        Limits {
            seconds: 60,
            model_calls: 0,
            tool_calls: 0,
        },
        None,
    )
    .await;
    running.submit("establish native recovery owner").await;
    running.wait_turn_finished().await;
    let before = running.runtime.record().unwrap();
    let mut replacement = replacement_draft(&handle, "must-not-teardown-after-recovery");
    assert!(matches!(
        handle.save(&mut replacement).await,
        Ok(SaveStatus::Applied)
    ));
    let (pre_complete, release_validation) =
        running.pause_next_before_model_switch_final_validation();
    running
        .submit("must stay on old provider after recovery")
        .await;
    pre_complete.await.unwrap();
    running.runtime.hold().unwrap();
    release_validation.send(()).unwrap();
    let _ = running.wait_turn_failed().await;

    let after = running.runtime.record().unwrap();
    assert_eq!(after.identity, before.identity);
    assert!(after.recovery_pending);
    assert_eq!(gate.calls.load(Ordering::SeqCst), 1);
    assert_eq!(owner_peer.request_models(), ["owner-model"]);
    running.shutdown().await;
}

#[tokio::test]
async fn native_model_switch_uses_live_original_allowance_after_start_observation_expires() {
    let root = tempfile::tempdir().unwrap();
    let owner_peer =
        crate::config_change_test_support::ActualOwnerPeer::start(root.path(), "openai-api", false)
            .await
            .unwrap();
    let (handle, _) = owner_fixture(root.path(), &owner_peer.connection);
    let gate = Arc::new(ToggleGate::new(false));
    let pre = registration(
        "aged-native-model-switch-pre",
        gate.clone(),
        HandlerKind::Command,
        0,
    );
    let mut post = registration(
        "aged-native-model-switch-post",
        gate.clone(),
        HandlerKind::Command,
        0,
    );
    post.declaration.required_gate = false;
    let mut connection = owner_peer.connection.clone();
    connection.access.non_tools = vec![
        plan_for(HookEvent::PreModelSwitch, vec![pre]),
        plan_for(HookEvent::PostModelSwitch, vec![post]),
    ];
    let mut running = crate::config_change_test_support::start_actual_control_with_session(
        &handle,
        root.path(),
        connection,
        Limits {
            seconds: 60,
            model_calls: 0,
            tool_calls: 0,
        },
        None,
    )
    .await;
    running.submit("establish aged native owner").await;
    running.wait_turn_finished().await;
    let before = running.runtime.record().unwrap();
    expire_native_observation_window(&running.runtime, host_lifetime(&before).0);

    let mut replacement = replacement_draft(&handle, "aged-native-model");
    assert!(matches!(
        handle.save(&mut replacement).await,
        Ok(SaveStatus::Applied)
    ));
    running.submit("apply with live original allowance").await;
    running.wait_turn_finished().await;

    let after = running.runtime.record().unwrap();
    assert_eq!(gate.calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        owner_peer.request_models(),
        ["owner-model", "aged-native-model"]
    );
    assert_eq!(allowance_identity(&after), allowance_identity(&before));
    running.shutdown().await;
}

#[tokio::test]
async fn native_model_switch_refuses_expired_original_allowance_after_pre() {
    let root = tempfile::tempdir().unwrap();
    let owner_peer =
        crate::config_change_test_support::ActualOwnerPeer::start(root.path(), "openai-api", false)
            .await
            .unwrap();
    let (handle, _) = owner_fixture(root.path(), &owner_peer.connection);
    let gate = Arc::new(ToggleGate::new(false));
    let pre = registration(
        "expired-native-model-switch-pre",
        gate.clone(),
        HandlerKind::Command,
        0,
    );
    let mut connection = owner_peer.connection.clone();
    connection.access.non_tools = vec![plan_for(HookEvent::PreModelSwitch, vec![pre])];
    let mut running = crate::config_change_test_support::start_actual_control_with_session(
        &handle,
        root.path(),
        connection,
        Limits {
            seconds: 60,
            model_calls: 0,
            tool_calls: 0,
        },
        None,
    )
    .await;
    running.submit("establish expiring native owner").await;
    running.wait_turn_finished().await;
    let before = running.runtime.record().unwrap();

    let mut replacement = replacement_draft(&handle, "must-not-apply-expired");
    assert!(matches!(
        handle.save(&mut replacement).await,
        Ok(SaveStatus::Applied)
    ));
    let (pre_complete, release_validation) =
        running.pause_next_before_model_switch_final_validation();
    running.submit("expire original allowance after Pre").await;
    pre_complete.await.unwrap();
    expire_session_hook_allowance(&running.runtime);
    let expired = allowance_identity(&running.runtime.record().unwrap());
    release_validation.send(()).unwrap();
    let errors = running.wait_turn_failed().await;

    let after = running.runtime.record().unwrap();
    assert_eq!(after.identity, before.identity);
    assert_eq!(allowance_identity(&after), expired);
    assert_eq!(gate.calls.load(Ordering::SeqCst), 1);
    assert_eq!(owner_peer.request_models(), ["owner-model"]);
    assert!(
        errors
            .iter()
            .any(|error| error.contains("original host allowance ended")),
        "unexpected expired-allowance errors: {errors:?}"
    );
    running.shutdown().await;
}

async fn unfunded_model_switch_after_observation_expiry(with_command: bool) {
    let root = tempfile::tempdir().unwrap();
    let owner_peer =
        crate::config_change_test_support::ActualOwnerPeer::start(root.path(), "openai-api", false)
            .await
            .unwrap();
    let (handle, _) = owner_fixture(root.path(), &owner_peer.connection);
    let gate = Arc::new(ToggleGate::new(false));
    let mut connection = owner_peer.connection.clone();
    if with_command {
        let pre = registration(
            "unfunded-aged-model-switch-pre",
            gate.clone(),
            HandlerKind::Command,
            0,
        );
        connection.access.non_tools = vec![plan_for(HookEvent::PreModelSwitch, vec![pre])];
    }
    let mut running =
        crate::config_change_test_support::start_actual_control_without_grant_for_connection(
            &handle,
            root.path(),
            connection,
        )
        .await;
    running.submit("establish unfunded native owner").await;
    running.wait_turn_finished().await;
    let before = running.runtime.record().unwrap();
    assert!(before.session_hook_allowance.is_none());
    expire_native_observation_window(&running.runtime, host_lifetime(&before).0);

    let mut replacement = replacement_draft(&handle, "unfunded-aged-model");
    assert!(matches!(
        handle.save(&mut replacement).await,
        Ok(SaveStatus::Applied)
    ));
    running
        .submit("apply unfunded after observation expiry")
        .await;
    running.wait_turn_finished().await;

    assert_eq!(gate.calls.load(Ordering::SeqCst), usize::from(with_command));
    assert_eq!(
        owner_peer.request_models(),
        ["owner-model", "unfunded-aged-model"]
    );
    assert!(
        running
            .runtime
            .record()
            .unwrap()
            .session_hook_allowance
            .is_none()
    );
    running.shutdown().await;
}

#[tokio::test]
async fn grant_free_command_model_switch_survives_start_observation_expiry() {
    unfunded_model_switch_after_observation_expiry(true).await;
}

#[tokio::test]
async fn grant_free_no_plan_model_switch_survives_start_observation_expiry() {
    unfunded_model_switch_after_observation_expiry(false).await;
}

#[tokio::test]
async fn model_switch_rejects_stale_inspected_inputs_before_provider_teardown() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("model-policy"), "allow").unwrap();
    let owner_peer =
        crate::config_change_test_support::ActualOwnerPeer::start(root.path(), "openai-api", false)
            .await
            .unwrap();
    let (handle, _) = owner_fixture(root.path(), &owner_peer.connection);
    let gate = Arc::new(HeldGate::new());
    let mut pre = registration(
        "stale-model-switch-input",
        gate.clone(),
        HandlerKind::Command,
        0,
    );
    pre.declaration.reads = crate::plugins::gate_snapshot::GateReadSet::new(
        vec!["model-policy".into()],
        vec![],
        vec![],
    )
    .unwrap();
    let mut connection = owner_peer.connection.clone();
    connection.access.non_tools = vec![plan_for(HookEvent::PreModelSwitch, vec![pre])];
    let mut running = crate::config_change_test_support::start_actual_control_with_session(
        &handle,
        root.path(),
        connection,
        Limits {
            seconds: 60,
            model_calls: 0,
            tool_calls: 0,
        },
        None,
    )
    .await;
    running.submit("initial stale-input owner").await;
    running.wait_turn_finished().await;
    let before = running.runtime.record().unwrap();

    let mut replacement = replacement_draft(&handle, "stale-input-candidate");
    assert!(matches!(
        handle.save(&mut replacement).await,
        Ok(SaveStatus::Applied)
    ));
    gate.hold();
    running.submit("reject changed inspected input").await;
    gate.wait_entered().await;
    std::fs::write(root.path().join("model-policy"), "deny").unwrap();
    gate.release();
    let errors = running.wait_turn_failed().await;

    let after = running.runtime.record().unwrap();
    assert_eq!(after.identity, before.identity);
    assert!(!after.recovery_pending);
    assert_eq!(owner_peer.request_models(), ["owner-model"]);
    assert!(
        errors
            .iter()
            .any(|error| error.contains("lifecycle inspected inputs changed before continuation")),
        "unexpected stale-input errors: {errors:?}"
    );
    running.shutdown().await;
}

#[tokio::test]
async fn old_provider_close_failure_holds_teardown_without_publishing_or_replaying() {
    let root = tempfile::tempdir().unwrap();
    let owner_peer =
        crate::config_change_test_support::ActualOwnerPeer::start(root.path(), "openai-api", false)
            .await
            .unwrap();
    let (handle, _) = owner_fixture(root.path(), &owner_peer.connection);
    let pre = registration(
        "close-failure-model-switch",
        Arc::new(ToggleGate::new(false)),
        HandlerKind::Command,
        0,
    );
    let mut connection = owner_peer.connection.clone();
    connection.access.non_tools = vec![plan_for(HookEvent::PreModelSwitch, vec![pre])];
    let actual = crate::adapters::builtins()
        .unwrap()
        .open(&connection, root.path())
        .unwrap();
    let mut running = crate::config_change_test_support::start_actual_control_with_session(
        &handle,
        root.path(),
        connection,
        Limits {
            seconds: 60,
            model_calls: 0,
            tool_calls: 0,
        },
        Some(crate::config_change_test_support::fail_next_session_close(
            actual,
        )),
    )
    .await;
    running.submit("initial close-failure owner").await;
    running.wait_turn_finished().await;
    let before = running.runtime.record().unwrap();
    let mut replacement = replacement_draft(&handle, "must-not-publish-after-close-error");
    assert!(matches!(
        handle.save(&mut replacement).await,
        Ok(SaveStatus::Applied)
    ));

    running.submit("must not run after uncertain close").await;
    let errors = running.wait_turn_failed().await;
    let after = running.runtime.record().unwrap();
    assert_eq!(after.identity, before.identity);
    assert!(after.recovery_pending);
    assert_eq!(owner_peer.request_models(), ["owner-model"]);
    assert!(
        errors
            .iter()
            .any(|error| error.contains("old provider could not close"))
    );
    assert!(after.operations.iter().any(|operation| {
        matches!(&operation.host_invocation, Some(HostInvocation::ModelSwitch(switch))
            if switch.stage == crate::workflow::runtime::model_switch::Stage::Teardown
                && switch.hold.as_deref() == Some("model switch interrupted with uncertain lifecycle or provider effects"))
            && operation.complete
    }));
    running.shutdown().await;
}

#[tokio::test]
async fn settings_activation_failure_after_apply_keeps_new_identity_and_holds_prompt() {
    let root = tempfile::tempdir().unwrap();
    let owner_peer =
        crate::config_change_test_support::ActualOwnerPeer::start(root.path(), "openai-api", false)
            .await
            .unwrap();
    let (handle, _) = owner_fixture(root.path(), &owner_peer.connection);
    let pre = registration(
        "activation-failure-model-switch",
        Arc::new(ToggleGate::new(false)),
        HandlerKind::Command,
        0,
    );
    let config = registration(
        "activation-failure-settings-policy",
        Arc::new(ToggleGate::new(false)),
        HandlerKind::Command,
        0,
    );
    let mut connection = owner_peer.connection.clone();
    connection.access.non_tools = vec![
        plan_for(HookEvent::PreModelSwitch, vec![pre]),
        plan_for(HookEvent::ConfigChange, vec![config]),
    ];
    let mut running = crate::config_change_test_support::start_actual_control_with_session(
        &handle,
        root.path(),
        connection,
        Limits {
            seconds: 60,
            model_calls: 0,
            tool_calls: 0,
        },
        None,
    )
    .await;
    running.submit("initial activation-failure owner").await;
    running.wait_turn_finished().await;
    let mut replacement = replacement_draft(&handle, "applied-before-activation-error");
    assert!(matches!(
        handle.save(&mut replacement).await,
        Ok(SaveStatus::Applied)
    ));
    handle.fail_next_control_activation();

    running
        .submit("must remain unexecuted after activation error")
        .await;
    let errors = running.wait_turn_failed().await;
    let after = running.runtime.record().unwrap();
    let mut expected = owner_peer.connection.clone();
    expected.model = Some("applied-before-activation-error".into());
    assert_eq!(
        after.identity,
        crate::workflow::runtime::Identity::from(&expected)
    );
    assert!(after.recovery_pending);
    assert_eq!(owner_peer.request_models(), ["owner-model"]);
    assert!(
        errors
            .iter()
            .any(|error| error.contains("injected Settings control activation failure"))
    );
    assert!(after.operations.iter().any(|operation| {
        matches!(&operation.host_invocation, Some(HostInvocation::ModelSwitch(switch))
            if switch.stage == crate::workflow::runtime::model_switch::Stage::Applied
                && switch.hold.as_deref() == Some("model switch interrupted with uncertain lifecycle or provider effects"))
            && operation.complete
    }));
    running.shutdown().await;
}

#[tokio::test]
async fn held_switch_keeps_captured_assignment_allowance_and_refuses_queued_prompt() {
    let config_root = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let peer_root = tempfile::tempdir().unwrap();
    let owner_peer = crate::config_change_test_support::ActualOwnerPeer::start(
        peer_root.path(),
        "openai-api",
        false,
    )
    .await
    .unwrap();
    let (handle, _) = owner_fixture(config_root.path(), &owner_peer.connection);
    let gate = Arc::new(HeldGate::new());
    let pre = registration(
        "interleaved-model-switch",
        gate.clone(),
        HandlerKind::Command,
        0,
    );
    let mut connection = owner_peer.connection.clone();
    connection.access.non_tools = vec![plan_for(HookEvent::PreModelSwitch, vec![pre])];
    let limits = Limits {
        seconds: 60,
        model_calls: 2,
        tool_calls: 2,
    };
    let mut running = crate::config_change_test_support::start_actual_control_with_session(
        &handle,
        workspace.path(),
        connection,
        limits,
        None,
    )
    .await;
    running.submit("initial interleaving owner").await;
    running.wait_turn_finished().await;
    let original = running
        .runtime
        .record()
        .unwrap()
        .session_hook_allowance
        .unwrap()
        .allocation;

    let mut b = replacement_draft(&handle, "captured-model-b");
    assert!(matches!(handle.save(&mut b).await, Ok(SaveStatus::Applied)));
    gate.hold();
    running.submit("submission captured for model b").await;
    gate.wait_entered().await;
    let (reply, accepted) = tokio::sync::oneshot::channel();
    running
        .command_tx
        .send(crate::session::Command::Submit {
            text: "unrelated queued submission".into(),
            reply,
        })
        .await
        .unwrap();
    assert_eq!(
        accepted.await.unwrap().unwrap_err(),
        "Creator model switch in progress; draft retained."
    );
    let mut c = replacement_draft(&handle, "later-model-c");
    assert!(matches!(handle.save(&mut c).await, Ok(SaveStatus::Applied)));
    gate.release();
    running.wait_turn_finished().await;

    let after_b = running.runtime.record().unwrap();
    let mut expected_b = owner_peer.connection.clone();
    expected_b.model = Some("captured-model-b".into());
    assert_eq!(
        after_b.identity,
        crate::workflow::runtime::Identity::from(&expected_b)
    );
    assert_eq!(
        owner_peer.request_models(),
        ["owner-model", "captured-model-b"]
    );
    let retained = after_b.session_hook_allowance.unwrap().allocation;
    assert_eq!(
        (
            retained.started_ms,
            retained.deadline_ms,
            retained.model_calls,
            retained.tool_calls
        ),
        (
            original.started_ms,
            original.deadline_ms,
            original.model_calls,
            original.tool_calls
        )
    );

    running.submit("fresh submission for model c").await;
    running.wait_turn_finished().await;
    assert_eq!(
        owner_peer.request_models(),
        ["owner-model", "captured-model-b", "later-model-c"]
    );
    let retained = running
        .runtime
        .record()
        .unwrap()
        .session_hook_allowance
        .unwrap()
        .allocation;
    assert_eq!(
        (
            retained.started_ms,
            retained.deadline_ms,
            retained.model_calls,
            retained.tool_calls
        ),
        (
            original.started_ms,
            original.deadline_ms,
            original.model_calls,
            original.tool_calls
        )
    );
    running.shutdown().await;
}

#[tokio::test]
async fn model_switch_pre_runs_all_native_runner_types_under_original_grant() {
    let root = tempfile::tempdir().unwrap();
    let owner_peer =
        crate::config_change_test_support::ActualOwnerPeer::start(root.path(), "openai-api", false)
            .await
            .unwrap();
    let (handle, _) = owner_fixture(root.path(), &owner_peer.connection);
    let command = Arc::new(ToggleGate::new(false));
    let command_registration = registration(
        "model-switch-command",
        command.clone(),
        HandlerKind::Command,
        0,
    );
    let (http, http_peer) =
        crate::config_change_test_support::model_switch_http_registration().await;
    let ([mcp, _], mcp_peer, service) =
        crate::config_change_test_support::model_switch_mcp_registrations(root.path(), 2).await;
    let (prompt, prompt_peer) =
        crate::config_change_test_support::model_switch_model_registration(HandlerKind::Prompt)
            .await;
    let (agent, agent_peer) =
        crate::config_change_test_support::model_switch_model_registration(HandlerKind::Agent)
            .await;
    let mut connection = owner_peer.connection.clone();
    connection.access.non_tools = vec![plan_for(
        HookEvent::PreModelSwitch,
        vec![command_registration, http, mcp, prompt, agent],
    )];
    let mut running = crate::config_change_test_support::start_actual_control_with_session(
        &handle,
        root.path(),
        connection,
        Limits {
            seconds: 60,
            model_calls: 4,
            tool_calls: 4,
        },
        None,
    )
    .await;
    running.submit("establish runner matrix owner").await;
    running.wait_turn_finished().await;
    let before = running.runtime.record().unwrap();
    let allowance_before = before
        .session_hook_allowance
        .as_ref()
        .unwrap()
        .allocation
        .clone();

    let mut replacement = replacement_draft(&handle, "runner-matrix-model");
    assert!(matches!(
        handle.save(&mut replacement).await,
        Ok(SaveStatus::Applied)
    ));
    running.submit("run model switch runner matrix").await;
    running.wait_turn_finished().await;

    assert_eq!(command.calls.load(Ordering::SeqCst), 1);
    assert_eq!(http_peer.count(), 1);
    assert_eq!(mcp_peer.method_count("initialize"), 1);
    assert_eq!(mcp_peer.method_count("tools/call"), 1);
    assert_eq!(prompt_peer.count(), 1);
    assert_eq!(agent_peer.count(), 1);
    assert_eq!(
        owner_peer.request_models(),
        ["owner-model", "runner-matrix-model"]
    );
    let after = running.runtime.record().unwrap();
    let allowance = &after.session_hook_allowance.as_ref().unwrap().allocation;
    assert_eq!(allowance.started_ms, allowance_before.started_ms);
    assert_eq!(allowance.deadline_ms, allowance_before.deadline_ms);
    assert_eq!(allowance.model_calls, allowance_before.model_calls + 2);
    assert_eq!(allowance.tool_calls, allowance_before.tool_calls);
    let pre = after
        .operations
        .iter()
        .filter_map(|operation| operation.non_tool_receipt())
        .find(|receipt| {
            matches!(
                receipt.facts.subject.occurrence,
                NonToolOccurrence::PreModelSwitch { .. }
            )
        })
        .expect("model switch runner matrix receipt");
    assert_eq!(pre.hooks.len(), 5);
    for kind in [
        HandlerKind::Command,
        HandlerKind::Http,
        HandlerKind::McpTool,
        HandlerKind::Prompt,
        HandlerKind::Agent,
    ] {
        assert!(pre.hooks.iter().any(|hook| hook.declaration.runner == kind));
    }
    service.stop().await.unwrap();
    running.shutdown().await;
}

#[tokio::test]
async fn consecutive_model_switches_reuse_original_mcp_service_cap() {
    let root = tempfile::tempdir().unwrap();
    let owner_peer =
        crate::config_change_test_support::ActualOwnerPeer::start(root.path(), "openai-api", false)
            .await
            .unwrap();
    let (handle, _) = owner_fixture(root.path(), &owner_peer.connection);
    let ([pre, post], mcp_peer, service) =
        crate::config_change_test_support::model_switch_mcp_registrations(root.path(), 3).await;
    let mut connection = owner_peer.connection.clone();
    connection.access.non_tools = vec![
        plan_for(HookEvent::PreModelSwitch, vec![pre]),
        plan_for(HookEvent::PostModelSwitch, vec![post]),
    ];
    let mut running = crate::config_change_test_support::start_actual_control_with_session(
        &handle,
        root.path(),
        connection,
        Limits {
            seconds: 60,
            model_calls: 0,
            tool_calls: 0,
        },
        None,
    )
    .await;
    running.submit("establish original model owner").await;
    running.wait_turn_finished().await;
    let before = running.runtime.record().unwrap();
    let (lifetime_id, _) = host_lifetime(&before);
    let allowance_before = before
        .session_hook_allowance
        .as_ref()
        .unwrap()
        .allocation
        .clone();

    let mut first = replacement_draft(&handle, "model-b");
    assert!(matches!(
        handle.save(&mut first).await,
        Ok(SaveStatus::Applied)
    ));
    running.submit("use model b").await;
    running.wait_turn_finished().await;
    assert_eq!(mcp_peer.method_count("initialize"), 1);
    assert_eq!(mcp_peer.method_count("tools/call"), 2);
    assert_eq!(
        service.state(),
        crate::plugins::services::ServiceState::Ready
    );

    let mut second = replacement_draft(&handle, "model-c");
    assert!(matches!(
        handle.save(&mut second).await,
        Ok(SaveStatus::Applied)
    ));
    running.submit("use model c").await;
    running.wait_turn_finished().await;

    assert_eq!(
        owner_peer.request_models(),
        ["owner-model", "model-b", "model-c"]
    );
    assert_eq!(
        mcp_peer.method_count("initialize"),
        1,
        "a later switch must not reconnect and renew its MCP allowance"
    );
    assert_eq!(mcp_peer.method_count("tools/call"), 3);
    assert_eq!(
        service.state(),
        crate::plugins::services::ServiceState::Failed,
        "the observation beyond the original service cap remains visible"
    );
    let after = running.runtime.record().unwrap();
    let allowance = &after.session_hook_allowance.as_ref().unwrap().allocation;
    assert_eq!(allowance.started_ms, allowance_before.started_ms);
    assert_eq!(allowance.deadline_ms, allowance_before.deadline_ms);
    assert_eq!(allowance.model_calls, allowance_before.model_calls);
    assert_eq!(allowance.tool_calls, allowance_before.tool_calls);
    assert_eq!(host_lifetime(&after).0, lifetime_id);
    let model_switches = after
        .operations
        .iter()
        .filter_map(|operation| {
            operation
                .non_tool_receipt()
                .map(|receipt| (operation, receipt))
        })
        .filter(|(_, receipt)| {
            matches!(
                receipt.facts.subject.occurrence,
                NonToolOccurrence::PreModelSwitch { .. }
                    | NonToolOccurrence::PostModelSwitch { .. }
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(model_switches.len(), 4);
    assert!(model_switches.iter().all(|(operation, receipt)| {
        receipt.facts.host_session.is_none()
            && receipt.facts.task.is_none()
            && receipt.facts.child_owner.is_none()
            && receipt.facts.role == "model-switch"
            && matches!(
                &operation.budget,
                Some(BudgetRef::SessionHooks { session }) if session == &receipt.facts.session
            )
    }));
    service.stop().await.unwrap();
    running.shutdown().await;
}

#[tokio::test]
async fn model_switch_keeps_original_native_session_end_owner() {
    let root = tempfile::tempdir().unwrap();
    let owner_peer =
        crate::config_change_test_support::ActualOwnerPeer::start(root.path(), "openai-api", false)
            .await
            .unwrap();
    let (handle, _) = owner_fixture(root.path(), &owner_peer.connection);
    let gate = Arc::new(ToggleGate::new(false));
    let pre = registration(
        "model-switch-before-native-end",
        gate.clone(),
        HandlerKind::Command,
        0,
    );
    let mut post = registration(
        "model-switch-after-native-end",
        gate.clone(),
        HandlerKind::Command,
        1,
    );
    post.declaration.required_gate = false;
    let mut end = registration(
        "original-native-session-end",
        gate.clone(),
        HandlerKind::Command,
        2,
    );
    end.declaration.required_gate = false;
    let mut connection = owner_peer.connection.clone();
    connection.access.non_tools = vec![
        plan_for(HookEvent::PreModelSwitch, vec![pre]),
        plan_for(HookEvent::PostModelSwitch, vec![post]),
        plan_for(HookEvent::SessionEnd, vec![end]),
    ];
    let mut running = crate::config_change_test_support::start_actual_control_with_session(
        &handle,
        root.path(),
        connection,
        Limits {
            seconds: 60,
            model_calls: 0,
            tool_calls: 0,
        },
        None,
    )
    .await;
    running.submit("establish native lifetime").await;
    running.wait_turn_finished().await;
    let before = running.runtime.record().unwrap();
    let (lifetime_id, _) = host_lifetime(&before);
    let runtime = running.runtime.clone();

    let mut replacement = replacement_draft(&handle, "native-model-after-switch");
    assert!(matches!(
        handle.save(&mut replacement).await,
        Ok(SaveStatus::Applied)
    ));
    running.submit("apply native model switch").await;
    running.wait_turn_finished().await;
    assert_eq!(gate.calls.load(Ordering::SeqCst), 2);
    running.shutdown().await;

    let ended = runtime.record().unwrap();
    assert_eq!(
        gate.calls.load(Ordering::SeqCst),
        3,
        "ended record: {ended:?}"
    );
    let end_receipts = ended
        .operations
        .iter()
        .filter_map(|operation| operation.non_tool_receipt())
        .filter(|receipt| {
            matches!(
                receipt.facts.subject.occurrence,
                NonToolOccurrence::SessionEnd {
                    reason: crate::session::SessionEnd::Shutdown
                }
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(end_receipts.len(), 1);
    assert_eq!(end_receipts[0].facts.native_session, Some(lifetime_id));
    let (_, lifetime) = host_lifetime(&ended);
    assert_eq!(lifetime, host_lifetime(&before).1);
}

#[tokio::test]
#[ignore = "requires the pinned installed Claude with a controlled local model peer"]
async fn native_to_external_switch_keeps_original_native_session_end_owner() {
    let root = tempfile::tempdir().unwrap();
    let external_root = tempfile::tempdir().unwrap();
    let owner_peer =
        crate::config_change_test_support::ActualOwnerPeer::start(root.path(), "openai-api", false)
            .await
            .unwrap();
    let external_peer = crate::config_change_test_support::ActualOwnerPeer::start(
        external_root.path(),
        "claude",
        false,
    )
    .await
    .unwrap();
    let (handle, _) = owner_fixture(root.path(), &owner_peer.connection);
    let gate = Arc::new(ToggleGate::new(false));
    let pre = registration(
        "native-to-external-model-switch",
        gate.clone(),
        HandlerKind::Command,
        0,
    );
    let mut post = registration(
        "native-to-external-model-observer",
        gate.clone(),
        HandlerKind::Command,
        1,
    );
    post.declaration.required_gate = false;
    let mut end = registration(
        "native-to-external-session-end",
        gate.clone(),
        HandlerKind::Command,
        2,
    );
    end.declaration.required_gate = false;
    let mut connection = owner_peer.connection.clone();
    connection.access.non_tools = vec![
        plan_for(HookEvent::PreModelSwitch, vec![pre]),
        plan_for(HookEvent::PostModelSwitch, vec![post]),
        plan_for(HookEvent::SessionEnd, vec![end]),
    ];
    let mut running = crate::config_change_test_support::start_actual_control_with_session(
        &handle,
        root.path(),
        connection,
        Limits {
            seconds: 60,
            model_calls: 0,
            tool_calls: 0,
        },
        None,
    )
    .await;
    running.submit("establish original native owner").await;
    running.wait_turn_finished().await;
    let before = running.runtime.record().unwrap();
    let (lifetime_id, lifetime_session) = host_lifetime(&before);
    let runtime = running.runtime.clone();

    let mut draft = handle.draft().unwrap();
    draft
        .config
        .connections
        .insert("external".into(), external_peer.connection.clone());
    let mut assignments = Assignments::from_config(&draft.config);
    assignments.providers.push("external".into());
    assignments.creator = Some(Assignment {
        connection: "external".into(),
        model: external_peer.connection.model.clone(),
        effort: external_peer.connection.effort.clone(),
    });
    draft.config.settings = Some(assignments);
    assert!(matches!(
        handle.save(&mut draft).await,
        Ok(SaveStatus::Applied)
    ));
    running.submit("run on external owner").await;
    running.wait_turn_finished().await;
    assert_eq!(owner_peer.request_models(), ["owner-model"]);
    assert_eq!(external_peer.request_count(), 1);
    assert_eq!(gate.calls.load(Ordering::SeqCst), 2);

    let mut alias = handle.draft().unwrap();
    let mut assignments = Assignments::from_config(&alias.config);
    assignments.creator = Some(Assignment {
        connection: "external".into(),
        model: Some("opus".into()),
        effort: None,
    });
    alias.config.settings = Some(assignments);
    assert!(matches!(
        handle.save(&mut alias).await,
        Ok(SaveStatus::Applied)
    ));
    running.submit("switch external owner to alias").await;
    running.wait_turn_finished().await;
    let resolved = running
        .runtime
        .record()
        .unwrap()
        .operations
        .iter()
        .filter_map(|operation| operation.non_tool_receipt())
        .find_map(|receipt| match &receipt.facts.subject.occurrence {
            NonToolOccurrence::PreModelSwitch {
                requested_model: Some(requested),
                resolved_model: Some(resolved),
                ..
            } if requested == "opus" => Some(resolved.clone()),
            _ => None,
        })
        .expect("Claude alias did not resolve");
    let mut canonical = handle.draft().unwrap();
    let mut assignments = Assignments::from_config(&canonical.config);
    assignments.creator = Some(Assignment {
        connection: "external".into(),
        model: Some(resolved),
        effort: None,
    });
    canonical.config.settings = Some(assignments);
    assert!(matches!(
        handle.save(&mut canonical).await,
        Ok(SaveStatus::Applied)
    ));
    running.submit("proven canonical no-op").await;
    running.wait_turn_finished().await;
    assert_eq!(gate.calls.load(Ordering::SeqCst), 4);
    assert_eq!(external_peer.request_count(), 3);
    running.shutdown().await;

    let ended = runtime.record().unwrap();
    assert_eq!(gate.calls.load(Ordering::SeqCst), 5);
    assert_eq!(host_lifetime(&ended), (lifetime_id, lifetime_session));
    assert!(ended.operations.iter().any(|operation| {
        operation.non_tool_receipt().is_some_and(|receipt| {
            matches!(
                receipt.facts.subject.occurrence,
                NonToolOccurrence::SessionEnd {
                    reason: crate::session::SessionEnd::Shutdown
                }
            ) && receipt.facts.native_session == Some(lifetime_id)
        })
    }));
}

#[tokio::test]
#[ignore = "requires the pinned installed Claude with a controlled local model peer"]
async fn installed_claude_model_switch_uses_genuine_source_callbacks() {
    let root = tempfile::tempdir().unwrap();
    let peer_root = tempfile::tempdir().unwrap();
    let owner_peer = crate::config_change_test_support::ActualOwnerPeer::start(
        peer_root.path(),
        "claude",
        false,
    )
    .await
    .unwrap();
    let (handle, _) = owner_fixture(root.path(), &owner_peer.connection);
    let gate = Arc::new(ToggleGate::new(true));
    let pre = registration(
        "installed-claude-switch-gate",
        gate.clone(),
        HandlerKind::Command,
        0,
    );
    let (http_pre, http_peer) =
        crate::config_change_test_support::claude_model_switch_http_registration().await;
    let ([mcp_pre, mcp_post], mcp_peer, mcp_service) =
        crate::config_change_test_support::model_switch_mcp_registrations_for_dialect(
            root.path(),
            4,
            HookDialect::Claude,
        )
        .await;
    let mut post = registration(
        "installed-claude-switch-observer",
        gate.clone(),
        HandlerKind::Command,
        0,
    );
    post.declaration.required_gate = false;
    let mut connection = owner_peer.connection.clone();
    connection.access.non_tools = vec![
        plan_for(HookEvent::PreModelSwitch, vec![pre, http_pre, mcp_pre]),
        plan_for(HookEvent::PostModelSwitch, vec![post, mcp_post]),
    ];
    let mut running = crate::config_change_test_support::start_actual_control_with_session(
        &handle,
        root.path(),
        connection,
        Limits {
            seconds: 60,
            model_calls: 0,
            tool_calls: 0,
        },
        None,
    )
    .await;
    running.submit("initialize installed Claude").await;
    running.wait_turn_finished().await;

    let mut replacement = replacement_draft(&handle, "opus");
    assert!(matches!(
        handle.save(&mut replacement).await,
        Ok(SaveStatus::Applied)
    ));
    running.submit("refused Claude submission").await;
    let denied_events = running.collect_turn_events("failed").await;
    let errors = denied_events
        .iter()
        .filter_map(|event| match event {
            crate::events::Event::Error { message } => Some(message.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(
        errors
            .iter()
            .any(|message| message.contains("not executed")),
        "unexpected Claude switch errors: {errors:?}"
    );
    assert!(
        assignment_models(&denied_events).is_empty(),
        "a denied switch must not publish an applied assignment"
    );
    assert_eq!(owner_peer.request_models(), ["claude-sonnet-4-6"]);

    gate.deny.store(false, Ordering::SeqCst);
    running.submit("fresh allowed Claude submission").await;
    let allowed_events = running.collect_turn_events("complete").await;
    assert_eq!(gate.calls.load(Ordering::SeqCst), 3);
    assert_eq!(http_peer.count(), 2);
    assert_eq!(mcp_peer.method_count("initialize"), 1);
    let record = running.runtime.record().unwrap();
    assert_eq!(mcp_peer.method_count("tools/call"), 2);
    let callbacks = record
        .operations
        .iter()
        .filter_map(|operation| operation.non_tool_receipt())
        .filter(|receipt| {
            matches!(
                receipt.facts.subject.occurrence,
                NonToolOccurrence::PreModelSwitch { .. }
                    | NonToolOccurrence::PostModelSwitch { .. }
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(callbacks.len(), 3);
    assert_eq!(
        callbacks
            .iter()
            .map(|receipt| receipt.hooks.len())
            .sum::<usize>(),
        7
    );
    assert!(callbacks.iter().all(|receipt| {
        matches!(receipt.facts.source, Some(ObservedLifecycle::Claude(_)))
            && receipt.facts.callback.is_some()
    }));
    let resolved =
        match callbacks
            .iter()
            .find_map(|receipt| match &receipt.facts.subject.occurrence {
                NonToolOccurrence::PreModelSwitch {
                    requested_model: Some(requested),
                    resolved_model: Some(resolved),
                    ..
                } if requested == "opus" && resolved.starts_with("claude-opus-") => {
                    Some(resolved.clone())
                }
                _ => None,
            }) {
            Some(resolved) => resolved,
            None => panic!("installed Claude alias resolution was not recorded: {callbacks:?}"),
        };
    assert_eq!(
        owner_peer.request_models(),
        ["claude-sonnet-4-6", resolved.as_str()]
    );
    let post_model = callbacks
        .iter()
        .find_map(|receipt| match &receipt.facts.subject.occurrence {
            NonToolOccurrence::PostModelSwitch {
                model: Some(model), ..
            } => Some(model.as_str()),
            _ => None,
        })
        .expect("genuine Claude PostModelSwitch model");
    assert_eq!(post_model, resolved);
    assert_eq!(assignment_models(&allowed_events), [Some(resolved.clone())]);
    assert!(owner_peer.requests_contain("claude-model-switch-post-context"));

    let mut no_op = replacement_draft(&handle, &resolved);
    assert!(matches!(
        handle.save(&mut no_op).await,
        Ok(SaveStatus::Applied)
    ));
    running.submit("same resolved Claude model").await;
    let no_op_events = running.collect_turn_events("complete").await;
    assert_eq!(
        gate.calls.load(Ordering::SeqCst),
        3,
        "a proven unchanged resolved model must not fabricate model switch callbacks"
    );
    assert_eq!(
        owner_peer.request_models(),
        ["claude-sonnet-4-6", resolved.as_str(), resolved.as_str()]
    );
    assert_eq!(assignment_models(&no_op_events), [Some(resolved.clone())]);
    assert_eq!(
        running
            .runtime
            .record()
            .unwrap()
            .operations
            .iter()
            .filter_map(|operation| operation.non_tool_receipt())
            .filter(|receipt| matches!(
                receipt.facts.subject.occurrence,
                NonToolOccurrence::PreModelSwitch { .. }
                    | NonToolOccurrence::PostModelSwitch { .. }
            ))
            .count(),
        3
    );

    let before_next = running.runtime.record().unwrap();
    let original_allowance = before_next
        .session_hook_allowance
        .as_ref()
        .map(|allowance| allowance.allocation.clone());
    let mut next = replacement_draft(&handle, "sonnet");
    assert!(matches!(
        handle.save(&mut next).await,
        Ok(SaveStatus::Applied)
    ));
    running.submit("genuine switch after canonical no-op").await;
    let next_events = running.collect_turn_events("complete").await;

    let after_next = running.runtime.record().unwrap();
    let resolved_next = after_next
        .operations
        .iter()
        .filter_map(|operation| operation.non_tool_receipt())
        .find_map(|receipt| match &receipt.facts.subject.occurrence {
            NonToolOccurrence::PreModelSwitch {
                requested_model: Some(requested),
                resolved_model: Some(resolved),
                ..
            } if requested == "sonnet" => Some(resolved.clone()),
            _ => None,
        })
        .expect("second Claude alias did not resolve");
    assert_eq!(gate.calls.load(Ordering::SeqCst), 5);
    assert_eq!(http_peer.count(), 3);
    assert_eq!(mcp_peer.method_count("initialize"), 1);
    assert_eq!(mcp_peer.method_count("tools/call"), 4);
    assert_eq!(
        owner_peer.request_models(),
        [
            "claude-sonnet-4-6",
            resolved.as_str(),
            resolved.as_str(),
            resolved_next.as_str(),
        ]
    );
    assert_eq!(
        assignment_models(&next_events),
        [Some(resolved_next.clone())]
    );
    let callbacks = after_next
        .operations
        .iter()
        .filter_map(|operation| operation.non_tool_receipt())
        .filter(|receipt| {
            matches!(
                receipt.facts.subject.occurrence,
                NonToolOccurrence::PreModelSwitch { .. }
                    | NonToolOccurrence::PostModelSwitch { .. }
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(callbacks.len(), 5);
    assert_eq!(
        callbacks
            .iter()
            .map(|receipt| receipt.hooks.len())
            .sum::<usize>(),
        12
    );
    if let (Some(original), Some(current)) = (
        original_allowance.as_ref(),
        after_next
            .session_hook_allowance
            .as_ref()
            .map(|allowance| &allowance.allocation),
    ) {
        assert_eq!(current.started_ms, original.started_ms);
        assert_eq!(current.deadline_ms, original.deadline_ms);
        assert_eq!(current.model_calls, original.model_calls);
        assert_eq!(current.tool_calls, original.tool_calls);
    }
    running.shutdown().await;
    mcp_service.stop().await.unwrap();
}

#[tokio::test]
#[ignore = "requires the pinned installed Claude with a controlled local model peer"]
async fn installed_claude_pre_revalidates_inspected_settings_before_permission() {
    let root = tempfile::tempdir().unwrap();
    let peer_root = tempfile::tempdir().unwrap();
    let owner_peer = crate::config_change_test_support::ActualOwnerPeer::start(
        peer_root.path(),
        "claude",
        false,
    )
    .await
    .unwrap();
    let (handle, _) = owner_fixture(root.path(), &owner_peer.connection);
    let gate = Arc::new(ToggleGate::new(false));
    let mut pre = registration(
        "installed-claude-final-validation-gate",
        gate.clone(),
        HandlerKind::Command,
        0,
    );
    pre.declaration.reads = crate::plugins::gate_snapshot::GateReadSet::new(
        vec!["settings.toml".into()],
        vec![],
        vec![],
    )
    .unwrap();
    let mut connection = owner_peer.connection.clone();
    connection.access.non_tools = vec![plan_for(HookEvent::PreModelSwitch, vec![pre])];
    let mut running = crate::config_change_test_support::start_actual_control_with_session(
        &handle,
        root.path(),
        connection,
        Limits {
            seconds: 60,
            model_calls: 0,
            tool_calls: 0,
        },
        None,
    )
    .await;
    running.submit("initialize inspected Claude owner").await;
    running.wait_turn_finished().await;

    let mut replacement = replacement_draft(&handle, "opus");
    assert!(matches!(
        handle.save(&mut replacement).await,
        Ok(SaveStatus::Applied)
    ));
    let (pre_dispatched, release_permission) = running.pause_next_model_switch_source_dispatch();
    running
        .submit("must be denied after inspected Settings change")
        .await;
    tokio::time::timeout(std::time::Duration::from_secs(20), pre_dispatched)
        .await
        .expect("Claude PreModelSwitch did not finish dispatch")
        .expect("Claude PreModelSwitch dispatch pause was dropped");

    let mut changed = replacement_draft(&handle, "haiku");
    assert!(matches!(
        handle.save(&mut changed).await,
        Ok(SaveStatus::Applied)
    ));
    release_permission.send(()).unwrap();
    let errors = running.wait_turn_failed().await;

    assert_eq!(gate.calls.load(Ordering::SeqCst), 1);
    assert_eq!(owner_peer.request_models(), ["claude-sonnet-4-6"]);
    assert!(
        !running.runtime.record().unwrap().recovery_pending,
        "a confirmed explicit denial before provider effect must not create recovery; errors={errors:?}"
    );
    assert!(
        errors.iter().any(|error| {
            error.contains("inspected inputs changed") || error.contains("not executed")
        }),
        "unexpected stale-input denial errors: {errors:?}"
    );
    running.shutdown().await;
}

#[tokio::test]
#[ignore = "requires the pinned installed Claude with a controlled local model peer"]
async fn installed_claude_recovery_before_permission_is_explicitly_denied() {
    let root = tempfile::tempdir().unwrap();
    let peer_root = tempfile::tempdir().unwrap();
    let owner_peer = crate::config_change_test_support::ActualOwnerPeer::start(
        peer_root.path(),
        "claude",
        false,
    )
    .await
    .unwrap();
    let (handle, _) = owner_fixture(root.path(), &owner_peer.connection);
    let gate = Arc::new(ToggleGate::new(false));
    let pre = registration(
        "installed-claude-final-owner-gate",
        gate.clone(),
        HandlerKind::Command,
        0,
    );
    let mut connection = owner_peer.connection.clone();
    connection.access.non_tools = vec![plan_for(HookEvent::PreModelSwitch, vec![pre])];
    let mut running = crate::config_change_test_support::start_actual_control_with_session(
        &handle,
        root.path(),
        connection,
        Limits {
            seconds: 60,
            model_calls: 0,
            tool_calls: 0,
        },
        None,
    )
    .await;
    running
        .submit("initialize recovery-bound Claude owner")
        .await;
    running.wait_turn_finished().await;

    let mut replacement = replacement_draft(&handle, "opus");
    assert!(matches!(
        handle.save(&mut replacement).await,
        Ok(SaveStatus::Applied)
    ));
    let (pre_dispatched, release_permission) = running.pause_next_model_switch_source_dispatch();
    running
        .submit("must be denied after recovery is raised")
        .await;
    tokio::time::timeout(std::time::Duration::from_secs(20), pre_dispatched)
        .await
        .expect("Claude PreModelSwitch did not finish dispatch")
        .expect("Claude PreModelSwitch dispatch pause was dropped");
    running.runtime.hold().unwrap();
    release_permission.send(()).unwrap();
    let errors = running.wait_turn_failed().await;

    assert_eq!(gate.calls.load(Ordering::SeqCst), 1);
    assert_eq!(owner_peer.request_models(), ["claude-sonnet-4-6"]);
    assert!(running.runtime.record().unwrap().recovery_pending);
    assert!(
        errors.iter().any(|error| error.contains("not executed")),
        "unexpected recovery denial errors: {errors:?}"
    );
    let record = running.runtime.record().unwrap();
    let receipt = record
        .operations
        .iter()
        .filter_map(|operation| operation.non_tool_receipt())
        .find(|receipt| {
            matches!(
                receipt.facts.subject.occurrence,
                NonToolOccurrence::PreModelSwitch { .. }
            )
        })
        .expect("genuine PreModelSwitch receipt missing");
    assert_eq!(
        receipt.source_delivery,
        Some(crate::plugins::receipts::SourceDelivery::Acknowledged)
    );
    running.shutdown().await;
}

#[tokio::test]
#[ignore = "requires the pinned installed Claude with a controlled local model peer"]
async fn installed_claude_model_switch_uses_live_allowance_after_start_observation_expires() {
    let root = tempfile::tempdir().unwrap();
    let peer_root = tempfile::tempdir().unwrap();
    let owner_peer = crate::config_change_test_support::ActualOwnerPeer::start(
        peer_root.path(),
        "claude",
        false,
    )
    .await
    .unwrap();
    let (handle, _) = owner_fixture(root.path(), &owner_peer.connection);
    let gate = Arc::new(ToggleGate::new(false));
    let pre = registration(
        "aged-claude-model-switch-pre",
        gate.clone(),
        HandlerKind::Command,
        0,
    );
    let mut post = registration(
        "aged-claude-model-switch-post",
        gate.clone(),
        HandlerKind::Command,
        0,
    );
    post.declaration.required_gate = false;
    let mut connection = owner_peer.connection.clone();
    connection.access.non_tools = vec![
        plan_for(HookEvent::PreModelSwitch, vec![pre]),
        plan_for(HookEvent::PostModelSwitch, vec![post]),
    ];
    let mut running = crate::config_change_test_support::start_actual_control_with_session(
        &handle,
        root.path(),
        connection,
        Limits {
            seconds: 60,
            model_calls: 0,
            tool_calls: 0,
        },
        None,
    )
    .await;
    running.submit("establish aged Claude owner").await;
    running.wait_turn_finished().await;
    let before = running.runtime.record().unwrap();
    expire_native_observation_window(&running.runtime, host_lifetime(&before).0);

    let mut replacement = replacement_draft(&handle, "opus");
    assert!(matches!(
        handle.save(&mut replacement).await,
        Ok(SaveStatus::Applied)
    ));
    running
        .submit("apply Claude with live original allowance")
        .await;
    running.wait_turn_finished().await;

    let after = running.runtime.record().unwrap();
    let callbacks = after
        .operations
        .iter()
        .filter_map(|operation| operation.non_tool_receipt())
        .filter(|receipt| {
            matches!(
                receipt.facts.subject.occurrence,
                NonToolOccurrence::PreModelSwitch { .. }
                    | NonToolOccurrence::PostModelSwitch { .. }
            )
        })
        .collect::<Vec<_>>();
    let resolved = callbacks
        .iter()
        .find_map(|receipt| match &receipt.facts.subject.occurrence {
            NonToolOccurrence::PreModelSwitch {
                requested_model: Some(requested),
                resolved_model: Some(resolved),
                ..
            } if requested == "opus" => Some(resolved.as_str()),
            _ => None,
        })
        .expect("installed Claude resolution");
    assert_eq!(callbacks.len(), 2);
    assert_eq!(gate.calls.load(Ordering::SeqCst), 2);
    assert_eq!(owner_peer.request_models(), ["claude-sonnet-4-6", resolved]);
    assert_eq!(allowance_identity(&after), allowance_identity(&before));
    running.shutdown().await;
}

#[tokio::test]
#[ignore = "requires the pinned installed Claude with a controlled local model peer"]
async fn installed_claude_expired_original_allowance_after_pre_is_explicitly_denied() {
    let root = tempfile::tempdir().unwrap();
    let peer_root = tempfile::tempdir().unwrap();
    let owner_peer = crate::config_change_test_support::ActualOwnerPeer::start(
        peer_root.path(),
        "claude",
        false,
    )
    .await
    .unwrap();
    let (handle, _) = owner_fixture(root.path(), &owner_peer.connection);
    let gate = Arc::new(ToggleGate::new(false));
    let pre = registration(
        "expired-claude-model-switch-pre",
        gate.clone(),
        HandlerKind::Command,
        0,
    );
    let mut connection = owner_peer.connection.clone();
    connection.access.non_tools = vec![plan_for(HookEvent::PreModelSwitch, vec![pre])];
    let mut running = crate::config_change_test_support::start_actual_control_with_session(
        &handle,
        root.path(),
        connection,
        Limits {
            seconds: 60,
            model_calls: 0,
            tool_calls: 0,
        },
        None,
    )
    .await;
    running.submit("establish expiring Claude owner").await;
    running.wait_turn_finished().await;
    let before = running.runtime.record().unwrap();

    let mut replacement = replacement_draft(&handle, "opus");
    assert!(matches!(
        handle.save(&mut replacement).await,
        Ok(SaveStatus::Applied)
    ));
    let (pre_dispatched, release_permission) = running.pause_next_model_switch_source_dispatch();
    running.submit("expire Claude allowance after Pre").await;
    pre_dispatched.await.unwrap();
    expire_session_hook_allowance(&running.runtime);
    let expired = allowance_identity(&running.runtime.record().unwrap());
    release_permission.send(()).unwrap();
    let errors = running.wait_turn_failed().await;

    let after = running.runtime.record().unwrap();
    assert_eq!(after.identity, before.identity);
    assert_eq!(allowance_identity(&after), expired);
    assert!(!after.recovery_pending);
    assert_eq!(gate.calls.load(Ordering::SeqCst), 1);
    assert_eq!(owner_peer.request_models(), ["claude-sonnet-4-6"]);
    assert!(
        errors.iter().any(|error| error.contains("not executed")),
        "unexpected expired-allowance denial errors: {errors:?}"
    );
    let pre_receipts = after
        .operations
        .iter()
        .filter_map(|operation| operation.non_tool_receipt())
        .filter(|receipt| {
            matches!(
                receipt.facts.subject.occurrence,
                NonToolOccurrence::PreModelSwitch { .. }
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(pre_receipts.len(), 1);
    assert_eq!(
        pre_receipts[0].source_delivery,
        Some(crate::plugins::receipts::SourceDelivery::Acknowledged)
    );
    running.shutdown().await;
}

async fn installed_claude_after_effect_case(recovery: bool) {
    let root = tempfile::tempdir().unwrap();
    let peer_root = tempfile::tempdir().unwrap();
    let owner_peer = crate::config_change_test_support::ActualOwnerPeer::start(
        peer_root.path(),
        "claude",
        false,
    )
    .await
    .unwrap();
    let (handle, _) = owner_fixture(root.path(), &owner_peer.connection);
    let gate = Arc::new(ToggleGate::new(false));
    let pre = registration(
        "installed-claude-after-effect-gate",
        gate.clone(),
        HandlerKind::Command,
        0,
    );
    let mut connection = owner_peer.connection.clone();
    connection.access.non_tools = vec![plan_for(HookEvent::PreModelSwitch, vec![pre])];
    let mut expected_applied = connection.clone();
    expected_applied.model = Some("opus".into());
    let mut running = crate::config_change_test_support::start_actual_control_with_session(
        &handle,
        root.path(),
        connection,
        Limits {
            seconds: 60,
            model_calls: 0,
            tool_calls: 0,
        },
        None,
    )
    .await;
    running.submit("initialize post-effect Claude owner").await;
    running.wait_turn_finished().await;
    let before = running.runtime.record().unwrap();

    let mut replacement = replacement_draft(&handle, "opus");
    assert!(matches!(
        handle.save(&mut replacement).await,
        Ok(SaveStatus::Applied)
    ));
    let (effect_applied, release_effect) = running.pause_next_after_model_switch_source_effect();
    running
        .submit("must not run after an interrupted provider effect")
        .await;
    tokio::time::timeout(std::time::Duration::from_secs(20), effect_applied)
        .await
        .expect("Claude set_model did not reach its applied effect")
        .expect("Claude source-effect pause was dropped");

    if recovery {
        running.runtime.hold().unwrap();
        release_effect.send(()).unwrap();
        let errors = running.wait_turn_failed().await;
        let after = running.runtime.record().unwrap();
        assert_eq!(
            after.identity,
            crate::workflow::runtime::Identity::from(&expected_applied),
            "a known SDK effect must retain the applied durable identity"
        );
        assert!(after.recovery_pending);
        assert!(
            !errors.is_empty(),
            "recovery after provider effect was hidden"
        );
    } else {
        running.cancel().await;
        release_effect.send(()).unwrap();
        running.wait_turn_cancelled().await;
        let after = running.runtime.record().unwrap();
        assert_eq!(
            after.identity, before.identity,
            "cancellation before the applied result reached the durable owner must retain uncertainty"
        );
        assert!(after.recovery_pending);
    }
    assert_eq!(gate.calls.load(Ordering::SeqCst), 1);
    assert_eq!(owner_peer.request_models(), ["claude-sonnet-4-6"]);
    running.shutdown().await;
}

#[tokio::test]
#[ignore = "requires the pinned installed Claude with a controlled local model peer"]
async fn installed_claude_cancel_after_provider_effect_holds_without_dependent_request() {
    installed_claude_after_effect_case(false).await;
}

#[tokio::test]
#[ignore = "requires the pinned installed Claude with a controlled local model peer"]
async fn installed_claude_recovery_after_provider_effect_retains_applied_identity() {
    installed_claude_after_effect_case(true).await;
}

async fn installed_claude_single_model_switch_plan_case(event: HookEvent) {
    let root = tempfile::tempdir().unwrap();
    let peer_root = tempfile::tempdir().unwrap();
    let owner_peer = crate::config_change_test_support::ActualOwnerPeer::start(
        peer_root.path(),
        "claude",
        false,
    )
    .await
    .unwrap();
    let (handle, _) = owner_fixture(root.path(), &owner_peer.connection);
    let gate = Arc::new(ToggleGate::new(false));
    let mut only = registration(
        "installed-claude-single-switch-plan",
        gate.clone(),
        HandlerKind::Command,
        0,
    );
    if event == HookEvent::PostModelSwitch {
        only.declaration.required_gate = false;
    }
    let mut connection = owner_peer.connection.clone();
    connection.access.non_tools = vec![plan_for(event, vec![only])];
    let mut running = crate::config_change_test_support::start_actual_control_with_session(
        &handle,
        root.path(),
        connection,
        Limits {
            seconds: 60,
            model_calls: 0,
            tool_calls: 0,
        },
        None,
    )
    .await;
    running.submit("initialize single-plan Claude owner").await;
    running.wait_turn_finished().await;

    let mut replacement = replacement_draft(&handle, "opus");
    assert!(matches!(
        handle.save(&mut replacement).await,
        Ok(SaveStatus::Applied)
    ));
    running
        .submit("switch with one declared callback side")
        .await;
    running.wait_turn_finished().await;

    assert_eq!(owner_peer.request_count(), 2);
    assert_eq!(gate.calls.load(Ordering::SeqCst), 1);
    let record = running.runtime.record().unwrap();
    let receipts = record
        .operations
        .iter()
        .filter_map(|operation| operation.non_tool_receipt())
        .filter(|receipt| {
            matches!(
                receipt.facts.subject.occurrence,
                NonToolOccurrence::PreModelSwitch { .. }
                    | NonToolOccurrence::PostModelSwitch { .. }
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(receipts.len(), 2);
    assert!(receipts.iter().all(|receipt| {
        matches!(receipt.facts.source, Some(ObservedLifecycle::Claude(_)))
            && receipt.facts.callback.is_some()
            && receipt.source_delivery
                == Some(crate::plugins::receipts::SourceDelivery::Acknowledged)
    }));
    let absent = receipts
        .iter()
        .find(|receipt| receipt.facts.subject.occurrence.event() != event)
        .expect("authenticated empty callback observation missing");
    assert!(absent.declarations.is_empty() && absent.hooks.is_empty());
    assert_eq!(
        absent.plan,
        crate::plugins::admission::digest(&(
            "authenticated_source_observation_v1",
            absent.facts.subject.occurrence.event().as_str(),
        ))
        .unwrap()
    );
    running.shutdown().await;
}

#[tokio::test]
#[ignore = "requires the pinned installed Claude with a controlled local model peer"]
async fn installed_claude_pre_only_plan_records_authentic_empty_post() {
    installed_claude_single_model_switch_plan_case(HookEvent::PreModelSwitch).await;
}

#[tokio::test]
#[ignore = "requires the pinned installed Claude with a controlled local model peer"]
async fn installed_claude_post_only_plan_records_authentic_empty_pre() {
    installed_claude_single_model_switch_plan_case(HookEvent::PostModelSwitch).await;
}

#[tokio::test]
#[ignore = "requires the pinned installed Claude with a controlled local model peer"]
async fn installed_claude_without_model_switch_plans_uses_host_observation_path() {
    let root = tempfile::tempdir().unwrap();
    let peer_root = tempfile::tempdir().unwrap();
    let owner_peer = crate::config_change_test_support::ActualOwnerPeer::start(
        peer_root.path(),
        "claude",
        false,
    )
    .await
    .unwrap();
    let (handle, _) = owner_fixture(root.path(), &owner_peer.connection);
    let mut running = crate::config_change_test_support::start_actual_control_with_session(
        &handle,
        root.path(),
        owner_peer.connection.clone(),
        Limits {
            seconds: 60,
            model_calls: 0,
            tool_calls: 0,
        },
        None,
    )
    .await;
    running.submit("initialize no-plan Claude owner").await;
    running.wait_turn_finished().await;

    let mut replacement = replacement_draft(&handle, "opus");
    assert!(matches!(
        handle.save(&mut replacement).await,
        Ok(SaveStatus::Applied)
    ));
    running
        .submit("host-owned switch without declarations")
        .await;
    running.wait_turn_finished().await;

    assert_eq!(owner_peer.request_count(), 2);
    assert!(!running.runtime.record().unwrap().recovery_pending);
    assert_eq!(
        running
            .runtime
            .record()
            .unwrap()
            .operations
            .iter()
            .filter_map(|operation| operation.non_tool_receipt())
            .filter(|receipt| matches!(
                receipt.facts.subject.occurrence,
                NonToolOccurrence::PreModelSwitch { .. }
                    | NonToolOccurrence::PostModelSwitch { .. }
            ))
            .count(),
        0
    );
    running.shutdown().await;
}

#[tokio::test]
#[ignore = "requires the pinned installed Claude with a controlled disconnected relay"]
async fn installed_claude_model_switch_relay_loss_holds_without_provider_replay() {
    let root = tempfile::tempdir().unwrap();
    let peer_root = tempfile::tempdir().unwrap();
    let owner_peer = crate::config_change_test_support::ActualOwnerPeer::start(
        peer_root.path(),
        "claude",
        false,
    )
    .await
    .unwrap();
    let (handle, _) = owner_fixture(root.path(), &owner_peer.connection);
    let gate = Arc::new(HeldGate::new());
    let pre = registration(
        "installed-claude-relay-loss-gate",
        gate.clone(),
        HandlerKind::Command,
        0,
    );
    let mut connection = owner_peer.connection.clone();
    connection.access.non_tools = vec![plan_for(HookEvent::PreModelSwitch, vec![pre])];
    let mut running = crate::config_change_test_support::start_actual_control_with_session(
        &handle,
        root.path(),
        connection,
        Limits {
            seconds: 60,
            model_calls: 0,
            tool_calls: 0,
        },
        None,
    )
    .await;
    running.submit("initialize installed Claude relay").await;
    running.wait_turn_finished().await;
    let before = running.runtime.record().unwrap();

    let mut replacement = replacement_draft(&handle, "claude-opus-4-6");
    assert!(matches!(
        handle.save(&mut replacement).await,
        Ok(SaveStatus::Applied)
    ));
    gate.hold();
    running.submit("must not replay after relay loss").await;
    gate.wait_entered().await;
    owner_peer.disconnect_relay().await;
    gate.release();
    let errors = running.wait_turn_failed().await;

    let after = running.runtime.record().unwrap();
    assert_eq!(after.identity, before.identity);
    assert!(after.recovery_pending);
    assert_eq!(owner_peer.request_models(), ["claude-sonnet-4-6"]);
    assert!(errors.iter().any(|message| {
        message.contains("recovery hold")
            || message.contains("Claude model switch")
            || message.contains("process stdout ended")
    }));
    assert!(after.operations.iter().any(|operation| {
        matches!(
            &operation.host_invocation,
            Some(HostInvocation::ModelSwitch(switch))
                if switch.stage == crate::workflow::runtime::model_switch::Stage::Teardown
        ) && operation.complete
    }));
    running.shutdown().await;
}

#[tokio::test]
#[ignore = "requires the pinned installed Claude with a controlled local model peer"]
async fn installed_claude_model_switch_runner_error_sends_explicit_denial() {
    let root = tempfile::tempdir().unwrap();
    let peer_root = tempfile::tempdir().unwrap();
    let owner_peer = crate::config_change_test_support::ActualOwnerPeer::start(
        peer_root.path(),
        "claude",
        false,
    )
    .await
    .unwrap();
    let (handle, _) = owner_fixture(root.path(), &owner_peer.connection);
    let pre = registration(
        "installed-claude-failing-gate",
        Arc::new(KnownFailureGate),
        HandlerKind::Command,
        0,
    );
    let mut connection = owner_peer.connection.clone();
    connection.access.non_tools = vec![plan_for(HookEvent::PreModelSwitch, vec![pre])];
    let mut running = crate::config_change_test_support::start_actual_control_with_session(
        &handle,
        root.path(),
        connection,
        Limits {
            seconds: 60,
            model_calls: 0,
            tool_calls: 0,
        },
        None,
    )
    .await;
    running
        .submit("initialize installed Claude failure case")
        .await;
    running.wait_turn_finished().await;
    let before = running.runtime.record().unwrap();

    let mut replacement = replacement_draft(&handle, "opus");
    assert!(matches!(
        handle.save(&mut replacement).await,
        Ok(SaveStatus::Applied)
    ));
    running.submit("must be denied after runner error").await;
    let errors = running.wait_turn_failed().await;

    let after = running.runtime.record().unwrap();
    assert_eq!(after.identity, before.identity);
    assert!(!after.recovery_pending);
    assert_eq!(owner_peer.request_models(), ["claude-sonnet-4-6"]);
    assert!(
        errors
            .iter()
            .any(|message| message.contains("not executed"))
    );
    let pre = after
        .operations
        .iter()
        .filter_map(|operation| operation.non_tool_receipt())
        .find(|receipt| {
            matches!(
                receipt.facts.subject.occurrence,
                NonToolOccurrence::PreModelSwitch { .. }
            )
        })
        .expect("genuine failed PreModelSwitch receipt");
    assert_eq!(
        pre.source_delivery,
        Some(crate::plugins::receipts::SourceDelivery::Acknowledged)
    );
    assert!(matches!(
        pre.facts.source,
        Some(ObservedLifecycle::Claude(_))
    ));
    assert_eq!(
        pre.hold.as_deref(),
        Some("required lifecycle handler failed")
    );
    running.shutdown().await;
}

async fn host_model_switch_owner_case(adapter: &str, target_model: &str) {
    let root = tempfile::tempdir().unwrap();
    let peer_root = tempfile::tempdir().unwrap();
    let owner_peer =
        crate::config_change_test_support::ActualOwnerPeer::start(peer_root.path(), adapter, false)
            .await
            .unwrap();
    let initial_model = owner_peer.connection.model.clone().unwrap();
    let (handle, _) = owner_fixture(root.path(), &owner_peer.connection);
    let gate = Arc::new(ToggleGate::new(false));
    let pre = registration(
        "native-owner-model-switch-pre",
        gate.clone(),
        HandlerKind::Command,
        0,
    );
    let mut post = registration(
        "native-owner-model-switch-post",
        gate.clone(),
        HandlerKind::Command,
        0,
    );
    post.declaration.required_gate = false;
    let mut connection = owner_peer.connection.clone();
    connection.access.non_tools = vec![
        plan_for(HookEvent::PreModelSwitch, vec![pre]),
        plan_for(HookEvent::PostModelSwitch, vec![post]),
    ];
    let mut running = crate::config_change_test_support::start_actual_control_with_session(
        &handle,
        root.path(),
        connection,
        Limits {
            seconds: 60,
            model_calls: 0,
            tool_calls: 0,
        },
        None,
    )
    .await;
    running.submit("initial owner request").await;
    running.wait_turn_finished().await;

    let mut replacement = replacement_draft(&handle, target_model);
    assert!(matches!(
        handle.save(&mut replacement).await,
        Ok(SaveStatus::Applied)
    ));
    running.submit("replacement owner request").await;
    running.wait_turn_finished().await;

    assert_eq!(gate.calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        owner_peer.request_models(),
        [initial_model.as_str(), target_model]
    );
    let record = running.runtime.record().unwrap();
    let receipts = record
        .operations
        .iter()
        .filter_map(|operation| operation.non_tool_receipt())
        .filter(|receipt| {
            matches!(
                receipt.facts.subject.occurrence,
                NonToolOccurrence::PreModelSwitch { .. }
                    | NonToolOccurrence::PostModelSwitch { .. }
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(receipts.len(), 2);
    assert!(receipts.iter().all(|receipt| {
        receipt.facts.source.is_none()
            && receipt.facts.callback.is_none()
            && receipt.source_delivery.is_none()
    }));
    running.shutdown().await;
}

#[tokio::test]
async fn anthropic_api_model_switch_uses_native_host_owner() {
    host_model_switch_owner_case("anthropic-api", "native-anthropic-replacement").await;
}

#[tokio::test]
async fn post_model_switch_observer_error_is_visible_without_rolling_back_application() {
    let root = tempfile::tempdir().unwrap();
    let owner_peer =
        crate::config_change_test_support::ActualOwnerPeer::start(root.path(), "openai-api", false)
            .await
            .unwrap();
    let (handle, _) = owner_fixture(root.path(), &owner_peer.connection);
    let pre = registration(
        "model-switch-pre-allow",
        Arc::new(ToggleGate::new(false)),
        HandlerKind::Command,
        0,
    );
    let mut post = registration(
        "model-switch-post-failure",
        Arc::new(KnownFailureGate),
        HandlerKind::Command,
        0,
    );
    post.declaration.required_gate = false;
    let mut connection = owner_peer.connection.clone();
    connection.access.non_tools = vec![
        plan_for(HookEvent::PreModelSwitch, vec![pre]),
        plan_for(HookEvent::PostModelSwitch, vec![post]),
    ];
    let mut running = crate::config_change_test_support::start_actual_control_with_session(
        &handle,
        root.path(),
        connection,
        Limits {
            seconds: 60,
            model_calls: 0,
            tool_calls: 0,
        },
        None,
    )
    .await;
    running.submit("initial observer owner").await;
    running.wait_turn_finished().await;

    let target = "observer-error-applied-model";
    let mut replacement = replacement_draft(&handle, target);
    assert!(matches!(
        handle.save(&mut replacement).await,
        Ok(SaveStatus::Applied)
    ));
    running.submit("apply despite observer failure").await;
    let errors = running.wait_turn_finished_with_errors().await;

    assert!(errors.iter().any(|error| {
        error.contains("model-switch-post-failure")
            && error.contains("invalid or failed lifecycle response")
    }));
    assert_eq!(owner_peer.request_models(), ["owner-model", target]);
    let record = running.runtime.record().unwrap();
    let mut expected = owner_peer.connection.clone();
    expected.model = Some(target.into());
    assert_eq!(
        record.identity,
        crate::workflow::runtime::Identity::from(&expected)
    );
    assert!(!record.recovery_pending);
    assert!(record.operations.iter().any(|operation| {
        matches!(
            &operation.host_invocation,
            Some(HostInvocation::ModelSwitch(switch))
                if switch.stage == crate::workflow::runtime::model_switch::Stage::Applied
                    && switch.hold.is_none()
        ) && operation.complete
    }));
    running.shutdown().await;
}

#[tokio::test]
#[ignore = "requires the pinned installed Codex with a controlled local model peer"]
async fn installed_codex_model_switch_uses_native_host_owner_without_source_callback() {
    host_model_switch_owner_case("codex", "gpt-5.3-codex").await;
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
