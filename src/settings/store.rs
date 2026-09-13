use crate::config::{Args, Config};
use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::File,
    os::unix::fs::MetadataExt,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

#[cfg(test)]
type PublicationPause = (Arc<tokio::sync::Notify>, Arc<tokio::sync::Notify>);

#[cfg(test)]
type BlockingPublicationPause = (
    tokio::sync::oneshot::Sender<()>,
    std::sync::mpsc::Receiver<()>,
);

#[derive(Clone)]
pub struct Handle {
    args: Arc<Args>,
    state: Arc<Mutex<Config>>,
    uncertain: Arc<AtomicBool>,
    publication_transaction: Arc<Mutex<()>>,
    control: Arc<Mutex<ControlState>>,
    #[cfg(test)]
    fail_directory_sync: Arc<AtomicBool>,
    #[cfg(test)]
    publication_pause: Arc<Mutex<Option<PublicationPause>>>,
    #[cfg(test)]
    file_publication_pause: Arc<Mutex<Option<BlockingPublicationPause>>>,
    #[cfg(test)]
    final_admission_pause: Arc<Mutex<Option<BlockingPublicationPause>>>,
    #[cfg(test)]
    control_timeout: Arc<Mutex<Duration>>,
}

#[derive(Clone, Default)]
enum ControlState {
    #[default]
    Disabled,
    Required,
    Ready(Arc<SettingsControl>),
}

struct SettingsControl {
    plan: Arc<crate::plugins::non_tool::NonToolPlan>,
    events: crate::events::EventSink,
    workspace: Arc<crate::plugins::gate_snapshot::GateWorkspace>,
    host: crate::plugins::runners::HookHost,
    workspace_identity: (u64, u64),
    runtime: crate::workflow::runtime::SharedRuntime,
}

struct Attempt(Arc<AtomicBool>);
impl Drop for Attempt {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

struct SaveAttempt {
    live: Arc<AtomicBool>,
    owned: Arc<AtomicBool>,
    operation: Arc<AtomicU64>,
    owner: Mutex<
        Option<(
            crate::workflow::runtime::SharedRuntime,
            tokio::time::Instant,
        )>,
    >,
}

impl Drop for SaveAttempt {
    fn drop(&mut self) {
        self.owned.store(false, Ordering::Release);
    }
}

impl SaveAttempt {
    fn new() -> Self {
        Self {
            live: Arc::new(AtomicBool::new(true)),
            owned: Arc::new(AtomicBool::new(true)),
            operation: Arc::new(AtomicU64::new(0)),
            owner: Mutex::new(None),
        }
    }

    fn start(
        &self,
        runtime: crate::workflow::runtime::SharedRuntime,
        deadline: Instant,
    ) -> Result<()> {
        let mut owner = self
            .owner
            .lock()
            .map_err(|_| anyhow::anyhow!("settings attempt owner unavailable"))?;
        ensure!(owner.is_none(), "settings attempt owner already started");
        *owner = Some((
            runtime,
            tokio::time::Instant::from_std(deadline) + crate::session::NATIVE_END_BUDGET,
        ));
        Ok(())
    }

    fn cancel(&self) {
        self.live.store(false, Ordering::Release);
    }

    fn ensure_live(&self) -> Result<()> {
        ensure!(
            self.live.load(Ordering::Acquire),
            "settings save was cancelled before publication"
        );
        Ok(())
    }

    async fn drain(&self) -> Result<()> {
        struct ReleaseCleanupOwner<'a>(&'a AtomicBool);
        impl Drop for ReleaseCleanupOwner<'_> {
            fn drop(&mut self) {
                self.0.store(false, Ordering::Release);
            }
        }
        let _release = ReleaseCleanupOwner(self.owned.as_ref());
        let operation = self.operation.load(Ordering::Acquire);
        let owner = self
            .owner
            .lock()
            .map_err(|_| anyhow::anyhow!("settings attempt owner unavailable"))?
            .clone();
        if operation != 0
            && let Some((runtime, deadline)) = owner
        {
            runtime.drain_settings_control(operation, deadline).await?;
            runtime.settle_cancelled_settings_control(operation)?;
        }
        Ok(())
    }
}

pub(crate) struct SaveJob {
    task: Option<tokio::task::JoinHandle<SaveCompletion>>,
    attempt: Arc<SaveAttempt>,
}

struct SaveCompletion {
    outcome: Result<SaveStatus>,
    cleanup: Result<()>,
}

impl SaveJob {
    pub(crate) fn is_finished(&self) -> bool {
        self.task
            .as_ref()
            .is_none_or(tokio::task::JoinHandle::is_finished)
    }

    pub(crate) fn cancel(&self) {
        self.attempt.cancel();
    }

    pub(crate) async fn finish(mut self) -> (Result<SaveStatus>, Result<()>) {
        match self
            .task
            .take()
            .expect("settings save task")
            .await
            .context("settings save task failed")
        {
            Ok(completion) => (completion.outcome, completion.cleanup),
            Err(error) => (
                Err(error),
                Err(anyhow::anyhow!(
                    "settings save task ended before owned cleanup completed"
                )),
            ),
        }
    }
}

impl Drop for SaveJob {
    fn drop(&mut self) {
        self.attempt.cancel();
    }
}

struct GatedAttempt {
    control: Arc<SettingsControl>,
    outcome: crate::plugins::non_tool::NonToolOutcome,
    deadline: Instant,
}

#[derive(Clone)]
pub(crate) struct Draft {
    pub config: Config,
    base: Option<[u8; 32]>,
}

#[derive(Debug)]
pub(crate) enum SaveStatus {
    Applied,
    AppliedUncertain(String),
    PublicationUncertain(String),
}

#[derive(Clone, Copy)]
#[repr(u8)]
enum PublicationStage {
    BeforePublication = 0,
    Publishing = 1,
    Published = 2,
}

struct PublicationProgress(AtomicU8);

impl PublicationProgress {
    fn new() -> Self {
        Self(AtomicU8::new(PublicationStage::BeforePublication as u8))
    }

    fn mark(&self, stage: PublicationStage) {
        self.0.store(stage as u8, Ordering::Release);
    }

    fn stage(&self) -> PublicationStage {
        match self.0.load(Ordering::Acquire) {
            0 => PublicationStage::BeforePublication,
            1 => PublicationStage::Publishing,
            2 => PublicationStage::Published,
            _ => unreachable!("invalid Settings publication stage"),
        }
    }
}

impl Handle {
    pub fn open(args: &Args) -> Result<Self> {
        Ok(Self {
            args: Arc::new(args.clone()),
            state: Arc::new(Mutex::new(args.load_config()?)),
            uncertain: Arc::new(AtomicBool::new(false)),
            publication_transaction: Arc::new(Mutex::new(())),
            control: Arc::new(Mutex::new(ControlState::Disabled)),
            #[cfg(test)]
            fail_directory_sync: Arc::new(AtomicBool::new(false)),
            #[cfg(test)]
            publication_pause: Arc::new(Mutex::new(None)),
            #[cfg(test)]
            file_publication_pause: Arc::new(Mutex::new(None)),
            #[cfg(test)]
            final_admission_pause: Arc::new(Mutex::new(None)),
            #[cfg(test)]
            control_timeout: Arc::new(Mutex::new(Duration::from_secs(30))),
        })
    }

    pub(crate) fn draft(&self) -> Result<Draft> {
        let (config, base) = self.args.load_config_snapshot()?;
        Ok(Draft { base, config })
    }

    pub(crate) fn current(&self) -> Result<Config> {
        let state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("settings state unavailable"))?;
        ensure!(
            !self.uncertain.load(Ordering::SeqCst),
            "settings publication is uncertain; restart the session, reopen Settings, and save a verified revision before new work"
        );
        Ok(state.clone())
    }

    pub(crate) fn args(&self) -> &Args {
        &self.args
    }

    pub(crate) fn creator(
        &self,
        previous: &crate::config::Connection,
    ) -> Result<crate::config::Selection> {
        let mut selection = self.args.selection_from(&self.current()?)?;
        let oracle = selection.connection.access.oracle.take();
        // Assignment edits cannot change trust, extension tools or launch authority.
        selection.connection.access = previous.access.clone();
        selection.connection.access.oracle = oracle;
        Ok(selection)
    }

    pub(crate) fn role(
        &self,
        role: super::Role,
        explicit: Option<&str>,
    ) -> Result<crate::config::Connection> {
        let config = self.current()?;
        let mut connection = if let Some(name) = explicit {
            Args::role_connection(&config, role, name)?
        } else {
            super::Assignments::from_config(&config).resolve(&config, role)?
        };
        connection.access = crate::tools::AccessPolicy::review_only();
        connection.validate()?;
        Ok(connection)
    }

    pub(crate) fn require_control(&self, required: bool) {
        let mut control = self
            .control
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        *control = if required {
            ControlState::Required
        } else {
            ControlState::Disabled
        };
    }

    pub(crate) fn activate_control(
        &self,
        workspace: &Path,
        connection: &crate::config::Connection,
        events: &crate::events::EventSink,
        runtime: crate::workflow::runtime::SharedRuntime,
    ) -> Result<()> {
        use crate::plugins::hook_types::HookEvent;
        let Some(plan) = connection
            .access
            .non_tools
            .iter()
            .find(|plan| plan.plan.event == HookEvent::ConfigChange)
            .cloned()
        else {
            self.require_control(false);
            return Ok(());
        };
        let canonical = workspace
            .canonicalize()
            .context("resolve settings workspace")?;
        let mut credentials = connection.access.credential_paths.clone();
        if let Some(path) = self.args.config_path() {
            credentials.push(path);
        }
        let gate = Arc::new(
            crate::plugins::gate_snapshot::GateWorkspace::open_with_credentials(
                &canonical,
                &credentials,
            )?,
        );
        let root = Arc::new(File::open(&canonical).context("open settings workspace")?);
        let metadata = root.metadata()?;
        let control = SettingsControl {
            plan,
            events: events.clone(),
            workspace: gate.clone(),
            host: crate::plugins::runners::HookHost::new(
                root,
                canonical,
                gate.frozen_credentials(),
                connection.access.supervisor.clone(),
            ),
            workspace_identity: (metadata.dev(), metadata.ino()),
            runtime,
        };
        let mut state = self
            .control
            .lock()
            .map_err(|_| anyhow::anyhow!("settings policy state unavailable"))?;
        *state = ControlState::Ready(Arc::new(control));
        Ok(())
    }

    #[cfg(test)]
    pub(crate) async fn save(&self, draft: &mut Draft) -> Result<SaveStatus> {
        self.save_with_attempt(draft, None).await
    }

    pub(crate) fn start_save(&self, mut draft: Draft) -> SaveJob {
        let attempt = Arc::new(SaveAttempt::new());
        let owned = attempt.clone();
        let handle = self.clone();
        let saving_attempt = owned.clone();
        let saving = tokio::spawn(async move {
            handle
                .save_with_attempt(&mut draft, Some(saving_attempt))
                .await
        });
        let task = tokio::spawn(async move {
            let outcome = match saving.await {
                Ok(outcome) => outcome,
                Err(error) => Err(anyhow::anyhow!("settings save task failed: {error}")),
            };
            let cleanup = owned.drain().await;
            SaveCompletion { outcome, cleanup }
        });
        SaveJob {
            task: Some(task),
            attempt,
        }
    }

    async fn save_with_attempt(
        &self,
        draft: &mut Draft,
        owned_attempt: Option<Arc<SaveAttempt>>,
    ) -> Result<SaveStatus> {
        let attempt = owned_attempt.unwrap_or_else(|| Arc::new(SaveAttempt::new()));
        let authority = Attempt(attempt.live.clone());
        attempt.ensure_live()?;
        self.ensure_save_available()?;
        if let Some(settings) = &draft.config.settings {
            settings.validate(&draft.config)?;
        }
        for connection in draft.config.connections.values() {
            connection.validate()?;
        }
        let proposed = toml::to_string_pretty(&draft.config)?;
        let proposed_digest = digest_hex(proposed.as_bytes());
        let occurrence = crate::plugins::receipts::NonToolOccurrence::ConfigChange {
            source: "user_settings".into(),
            proposed_digest,
            base_revision: draft.base.map(|digest| digest_hex(&digest)),
            structure: structural_view(&draft.config),
        };
        let subject = crate::plugins::receipts::LifecycleSubject {
            version: 1,
            occurrence: occurrence.clone(),
        };
        let captured_control = match self.control.try_lock() {
            Ok(control) => control.clone(),
            Err(std::sync::TryLockError::Poisoned(_)) => {
                anyhow::bail!("settings policy state unavailable")
            }
            Err(std::sync::TryLockError::WouldBlock) => {
                anyhow::bail!("settings publication is already in progress; wait before saving")
            }
        };
        let control = match captured_control {
            ControlState::Disabled => None,
            ControlState::Required => {
                anyhow::bail!("settings policy is active but its host capability is not ready")
            }
            ControlState::Ready(control) => Some(control),
        };
        let gated = if let Some(control) = control {
            #[cfg(test)]
            let timeout = *self.control_timeout.lock().unwrap();
            #[cfg(not(test))]
            let timeout = Duration::from_secs(30);
            let deadline = Instant::now() + timeout;
            attempt.start(control.runtime.clone(), deadline)?;
            let events = control.events.settings_control_events(
                deadline,
                attempt.live.clone(),
                attempt.owned.clone(),
                attempt.operation.clone(),
            )?;
            let dispatched = control
                .plan
                .dispatch(
                    occurrence,
                    &events,
                    control.workspace.clone(),
                    control.workspace_identity,
                    control.host.clone(),
                )
                .await;
            if Instant::now() >= deadline {
                anyhow::bail!("settings gate deadline expired before publication");
            }
            let outcome = dispatched?;
            if let Some(reason) = &outcome.hold {
                anyhow::bail!(reason.clone());
            }
            Some(GatedAttempt {
                control,
                outcome,
                deadline,
            })
        } else {
            None
        };

        self.publish_candidate(draft, proposed, subject, gated, attempt, authority)
            .await
    }

    async fn publish_candidate(
        &self,
        draft: &mut Draft,
        proposed: String,
        subject: crate::plugins::receipts::LifecycleSubject,
        gated: Option<GatedAttempt>,
        save_attempt: Arc<SaveAttempt>,
        authority: Attempt,
    ) -> Result<SaveStatus> {
        #[cfg(test)]
        let publication_pause = { self.publication_pause.lock().unwrap().take() };
        #[cfg(test)]
        if let Some((entered, release)) = publication_pause {
            entered.notify_one();
            release.notified().await;
        }
        save_attempt.ensure_live()?;

        // Lock order is workspace snapshot boundary, runtime identity/policy,
        // settings file, then in-memory state. No lock crosses gate execution.
        let workspace = if let Some(attempt) = &gated {
            Some(
                tokio::time::timeout_at(
                    tokio::time::Instant::from_std(attempt.deadline),
                    attempt
                        .outcome
                        .validation
                        .as_ref()
                        .context("settings gate validation missing")?
                        .validate(),
                )
                .await
                .context("settings gate deadline expired before publication")??,
            )
        } else {
            None
        };
        let base = draft.base;
        let config = draft.config.clone();
        let proposed_revision: [u8; 32] = Sha256::digest(proposed.as_bytes()).into();
        let handle = self.clone();
        let progress = Arc::new(PublicationProgress::new());
        let result = if let Some(gated_attempt) = gated {
            let GatedAttempt {
                control,
                outcome,
                deadline: _,
            } = gated_attempt;
            let runtime = control.runtime.clone();
            let workspace = workspace.context("settings workspace validation missing")?;
            let worker_progress = progress.clone();
            let worker_attempt = save_attempt.clone();
            let worker = tokio::task::spawn_blocking(move || {
                // The worker owns the validated workspace boundary and uncertainty
                // state. Dropping the awaiting UI task cannot expose a partially
                // finalized publication or permit a competing workspace mutation.
                let _workspace = workspace;
                let _transaction = handle
                    .publication_transaction
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                #[cfg(test)]
                handle.wait_before_final_admission();
                worker_attempt.ensure_live()?;
                let previous_uncertain = handle.begin_publication()?;
                let result = runtime.publish_config_change(outcome.operation, &subject, || {
                    handle.publish(&config, base, Some(&control), &worker_progress)
                });
                handle.finish_publication(&result, previous_uncertain);
                result
            });
            // Revocation authority remains with the awaiting save. Cancellation
            // before runtime final admission invalidates the detached worker.
            let joined = worker.await;
            drop(authority);
            publication_worker_result(joined, progress.stage())?
        } else {
            let worker_progress = progress.clone();
            let worker_attempt = save_attempt.clone();
            let worker = tokio::task::spawn_blocking(move || {
                let _transaction = handle
                    .publication_transaction
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                #[cfg(test)]
                handle.wait_before_final_admission();
                worker_attempt.ensure_live()?;
                let previous_uncertain = handle.begin_publication()?;
                let result = handle.publish(&config, base, None, &worker_progress);
                handle.finish_publication(&result, previous_uncertain);
                result
            });
            let joined = worker.await;
            drop(authority);
            publication_worker_result(joined, progress.stage())?
        };
        match result {
            SaveStatus::Applied => draft.base = Some(proposed_revision),
            SaveStatus::AppliedUncertain(_) | SaveStatus::PublicationUncertain(_) => {}
        }
        Ok(result)
    }

    fn ensure_save_available(&self) -> Result<()> {
        if !self.uncertain.load(Ordering::Acquire) {
            return Ok(());
        }
        let _transaction = match self.publication_transaction.try_lock() {
            Ok(transaction) => transaction,
            Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => {
                anyhow::bail!("settings publication is already in progress; wait before saving")
            }
        };
        anyhow::bail!(
            "settings publication is uncertain; restart the session, reopen Settings, and save a verified revision before new work"
        )
    }

    fn begin_publication(&self) -> Result<bool> {
        let previous_uncertain = self.uncertain.load(Ordering::Acquire);
        ensure!(
            !previous_uncertain,
            "settings publication is uncertain; restart the session, reopen Settings, and save a verified revision before new work"
        );
        self.uncertain.store(true, Ordering::Release);
        Ok(previous_uncertain)
    }

    fn finish_publication(&self, result: &Result<SaveStatus>, previous_uncertain: bool) {
        let uncertain = match result {
            Ok(SaveStatus::Applied) => false,
            Ok(SaveStatus::AppliedUncertain(_) | SaveStatus::PublicationUncertain(_)) => true,
            Err(_) => previous_uncertain,
        };
        self.uncertain.store(uncertain, Ordering::SeqCst);
    }

    fn publish(
        &self,
        config: &Config,
        base: Option<[u8; 32]>,
        expected_control: Option<&Arc<SettingsControl>>,
        progress: &PublicationProgress,
    ) -> Result<SaveStatus> {
        let control = self
            .control
            .lock()
            .map_err(|_| anyhow::anyhow!("settings policy state unavailable"))?;
        ensure!(
            match (&*control, expected_control) {
                (ControlState::Disabled, None) => true,
                (ControlState::Ready(current), Some(expected)) => Arc::ptr_eq(current, expected),
                _ => false,
            },
            "settings policy or host owner changed after inspection; reopen Settings before saving"
        );
        let path = self
            .args
            .config_path()
            .context("set HOME or select --config")?;
        let _lock = crate::startup::settings_lock(&path, self.args.config.is_none())?;
        let (_, current) = self.args.load_config_snapshot()?;
        ensure!(
            current == base,
            "settings changed in another editor; close and reopen Settings before saving"
        );
        // Acquire the state lock before publication, so failure cannot publish a file
        // that this handle then refuses to make available to subsequent work.
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("settings state unavailable"))?;
        progress.mark(PublicationStage::Publishing);
        #[cfg(test)]
        let saved = if self.fail_directory_sync.swap(false, Ordering::SeqCst) {
            crate::startup::save_with_failed_directory_sync(&path, config)
        } else {
            crate::startup::save(&path, config)
        };
        #[cfg(not(test))]
        let saved = crate::startup::save(&path, config);
        if let Err(error) = saved {
            if error
                .downcast_ref::<crate::startup::PublicationUncertain>()
                .is_some()
            {
                progress.mark(PublicationStage::Published);
                return Ok(SaveStatus::AppliedUncertain(error.to_string()));
            }
            return Err(error);
        }
        progress.mark(PublicationStage::Published);
        *state = config.clone();
        #[cfg(test)]
        if let Some((entered, release)) = self.file_publication_pause.lock().unwrap().take() {
            let _ = entered.send(());
            release
                .recv()
                .expect("release blocked Settings publication transaction");
        }
        Ok(SaveStatus::Applied)
    }

    #[cfg(test)]
    fn fail_next_directory_sync(&self) {
        self.fail_directory_sync.store(true, Ordering::SeqCst);
    }

    #[cfg(test)]
    fn pause_next_publication(
        &self,
        entered: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
    ) {
        *self.publication_pause.lock().unwrap() = Some((entered, release));
    }

    #[cfg(test)]
    pub(crate) fn pause_next_after_file_publication(
        &self,
    ) -> (
        tokio::sync::oneshot::Receiver<()>,
        std::sync::mpsc::Sender<()>,
    ) {
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        *self.file_publication_pause.lock().unwrap() = Some((entered_tx, release_rx));
        (entered_rx, release_tx)
    }

    #[cfg(test)]
    pub(crate) fn pause_next_before_final_admission(
        &self,
    ) -> (
        tokio::sync::oneshot::Receiver<()>,
        std::sync::mpsc::Sender<()>,
    ) {
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        *self.final_admission_pause.lock().unwrap() = Some((entered_tx, release_rx));
        (entered_rx, release_tx)
    }

    #[cfg(test)]
    fn wait_before_final_admission(&self) {
        if let Some((entered, release)) = self.final_admission_pause.lock().unwrap().take() {
            let _ = entered.send(());
            release
                .recv()
                .expect("release blocked Settings final admission");
        }
    }

    #[cfg(test)]
    pub(crate) async fn wait_for_publication_transaction(&self) {
        let transaction = self.publication_transaction.clone();
        tokio::task::spawn_blocking(move || {
            drop(
                transaction
                    .lock()
                    .unwrap_or_else(|error| error.into_inner()),
            );
        })
        .await
        .expect("join Settings publication transaction");
    }

    #[cfg(test)]
    pub(crate) fn set_control_timeout(&self, timeout: Duration) {
        *self.control_timeout.lock().unwrap() = timeout;
    }
}

fn publication_worker_result(
    joined: std::result::Result<Result<SaveStatus>, tokio::task::JoinError>,
    stage: PublicationStage,
) -> Result<SaveStatus> {
    match joined {
        Ok(result) => result,
        Err(error) => match stage {
            PublicationStage::BeforePublication => {
                Err(anyhow::Error::new(error).context("settings publication worker failed"))
            }
            PublicationStage::Publishing => Ok(SaveStatus::PublicationUncertain(format!(
                "the Settings publication outcome could not be determined because its worker failed during file replacement; restart the session and inspect the saved revision before continuing: {error}"
            ))),
            PublicationStage::Published => Ok(SaveStatus::AppliedUncertain(format!(
                "the Settings file was published but its worker failed before verification completed; restart the session and inspect the saved revision before continuing: {error}"
            ))),
        },
    }
}

/// An allowlist of configuration shape. Values, names, endpoints, prompts,
/// paths, headers and credentials never enter plugin frames.
fn structural_view(config: &Config) -> serde_json::Value {
    let adapters = config
        .connections
        .values()
        .fold(BTreeMap::new(), |mut counts, connection| {
            let category = match connection.adapter.as_str() {
                "openai-api" | "anthropic-api" | "codex" | "claude" => connection.adapter.as_str(),
                _ => "other",
            };
            *counts.entry(category.to_owned()).or_insert(0_u64) += 1;
            counts
        });
    serde_json::json!({
        "version": 1,
        "connection_count": config.connections.len(),
        "adapter_counts": adapters,
        "onboarding_complete": config.onboarding_complete,
        "trusted_workspace_count": config.trusted_workspaces.len(),
        "has_default_connection": config.default_connection.is_some(),
        "has_oracle_assignment": config.oracle.is_some(),
        "has_role_assignments": config.settings.is_some(),
    })
}

fn digest_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    mod owner_matrix;
    use crate::settings::{Assignment, Assignments, Role};
    use crate::{
        events::{Event, EventSink},
        native::{Model, NativeSession},
        plugins::{
            dispatch::{
                Declaration, DeclarationIdentity, HandlerClass, HookInvocation, HookRunner,
                Matcher, RawOutcome, Registration, Scope,
            },
            gate_snapshot::GateReadSet,
            hook_types::{HandlerKind, HookDialect, HookEvent},
            non_tool::NonToolPlan,
        },
        session::{self, Command},
        tools::ToolExecutor,
        workflow::{
            Settings as WorkflowSettings, WorkflowSession,
            allocation::Limits,
            runtime::{BudgetRef, HostInvocation, Operation, Record, SharedRuntime},
            workspace::CaptureScope,
        },
    };
    use clap::Parser;
    use std::{os::unix::fs::PermissionsExt, sync::Arc};
    use tokio::sync::mpsc;

    struct DenyConfigChange;

    struct PanicConfigChange;

    struct AllowConfigChange(Arc<Mutex<Vec<serde_json::Value>>>);

    struct HeldAllowConfigChange {
        entered: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
        effects: Arc<std::sync::atomic::AtomicUsize>,
    }

    type CapturedWorkspace = Arc<Mutex<Vec<(String, Vec<u8>)>>>;

    struct CaptureWorkspace(CapturedWorkspace);

    #[async_trait::async_trait]
    impl HookRunner for DenyConfigChange {
        async fn run(&self, _: &HookInvocation) -> Result<RawOutcome> {
            Ok(RawOutcome::Command {
                exit_code: Some(0),
                stdout: br#"{"decision":"block","reason":"active policy denied settings"}"#
                    .to_vec(),
                stderr: vec![],
            })
        }
    }

    #[async_trait::async_trait]
    impl HookRunner for PanicConfigChange {
        async fn run(&self, _: &HookInvocation) -> Result<RawOutcome> {
            panic!("synthetic Settings runner panic")
        }
    }

    #[async_trait::async_trait]
    impl HookRunner for AllowConfigChange {
        async fn run(&self, invocation: &HookInvocation) -> Result<RawOutcome> {
            self.0.lock().unwrap().push(serde_json::to_value(
                invocation.lifecycle.as_ref().expect("ConfigChange facts"),
            )?);
            Ok(RawOutcome::Command {
                exit_code: Some(0),
                stdout: br#"{"decision":"approve"}"#.to_vec(),
                stderr: vec![],
            })
        }
    }

    #[async_trait::async_trait]
    impl HookRunner for HeldAllowConfigChange {
        async fn run(&self, _: &HookInvocation) -> Result<RawOutcome> {
            self.effects.fetch_add(1, Ordering::SeqCst);
            self.entered.notify_one();
            self.release.notified().await;
            Ok(RawOutcome::Command {
                exit_code: Some(0),
                stdout: br#"{"decision":"approve"}"#.to_vec(),
                stderr: vec![],
            })
        }
    }

    #[async_trait::async_trait]
    impl HookRunner for CaptureWorkspace {
        async fn run(&self, invocation: &HookInvocation) -> Result<RawOutcome> {
            *self.0.lock().unwrap() = invocation
                .snapshot
                .entries()
                .map(|(name, entry)| (name.to_owned(), entry.bytes().to_vec()))
                .collect();
            Ok(RawOutcome::Command {
                exit_code: Some(0),
                stdout: br#"{"decision":"approve"}"#.to_vec(),
                stderr: vec![],
            })
        }
    }

    struct IdleModel;

    #[async_trait::async_trait]
    impl Model for IdleModel {
        fn prompt(&mut self, _: String) {}
        fn results(&mut self, _: Vec<crate::tools::ToolResult>) {}
        fn checkpoint(&self) -> Option<serde_json::Value> {
            Some(serde_json::json!({"fixture":"idle"}))
        }
        async fn response(&mut self, _: &EventSink) -> Result<Vec<crate::tools::ToolCall>> {
            Ok(Vec::new())
        }
    }

    fn denying_config_change_plan() -> Arc<NonToolPlan> {
        config_change_plan(Arc::new(DenyConfigChange))
    }

    fn config_change_plan(runner: Arc<dyn HookRunner>) -> Arc<NonToolPlan> {
        config_change_plan_with_reads(runner, GateReadSet::default())
    }

    fn config_change_plan_with_reads(
        runner: Arc<dyn HookRunner>,
        reads: GateReadSet,
    ) -> Arc<NonToolPlan> {
        let declaration = Declaration {
            required_gate: true,
            source: None,
            once: None,
            identity: DeclarationIdentity {
                package: "active-settings-policy".into(),
                code: "active-code".into(),
                policy: "active-policy".into(),
                configuration: "active-configuration".into(),
                generation: "1".into(),
                scope: Scope::Project,
                role: "worker".into(),
                declaration: "deny-config-change".into(),
                index: 0,
                dialect: HookDialect::Native,
                runner: HandlerKind::Command,
            },
            class: HandlerClass::Combined,
            priority: 0,
            matcher: Matcher::default(),
            reads,
            concurrent_group: None,
            read_only_endpoint: None,
            external_precondition: None,
        };
        Arc::new(
            NonToolPlan::new(
                HookEvent::ConfigChange,
                vec![Registration {
                    declaration,
                    runner,
                    revalidation: None,
                }],
            )
            .unwrap(),
        )
    }

    fn plan_from_registration(registration: Registration) -> Arc<NonToolPlan> {
        Arc::new(NonToolPlan::new(HookEvent::ConfigChange, vec![registration]).unwrap())
    }

    struct SaveRun {
        result: Result<SaveStatus>,
        record: Record,
        allowance_started_ms: u64,
        allowance_deadline_ms: u64,
    }

    struct RunningControl {
        connection: crate::config::Connection,
        limits: Limits,
        runtime: SharedRuntime,
        command_tx: mpsc::Sender<Command>,
        worker: tokio::task::JoinHandle<Result<()>>,
        _event_rx: mpsc::Receiver<crate::events::Envelope>,
        allowance_started_ms: u64,
        allowance_deadline_ms: u64,
    }

    impl RunningControl {
        async fn shutdown(self) -> Result<()> {
            self.command_tx.send(Command::Shutdown).await?;
            self.worker.await??;
            Ok(())
        }
    }

    #[derive(Clone, Copy)]
    struct ExpectedAccounting {
        model_calls: u64,
        tool_calls: u64,
        reported_input: u64,
        reported_output: u64,
        model_operations: usize,
        service_operations: usize,
    }

    async fn save_with_plan(
        handle: &Handle,
        root: &std::path::Path,
        plan: Arc<NonToolPlan>,
        draft: &mut Draft,
    ) -> SaveRun {
        let running = start_control(handle, root, plan).await;
        let result = handle.save(draft).await;
        let record = running.runtime.record().unwrap();
        let allowance_started_ms = running.allowance_started_ms;
        let allowance_deadline_ms = running.allowance_deadline_ms;
        running.shutdown().await.unwrap();
        SaveRun {
            result,
            record,
            allowance_started_ms,
            allowance_deadline_ms,
        }
    }

    async fn start_control(
        handle: &Handle,
        root: &std::path::Path,
        plan: Arc<NonToolPlan>,
    ) -> RunningControl {
        start_control_with(handle, root, plan, |_| {}).await
    }

    async fn start_control_with(
        handle: &Handle,
        root: &std::path::Path,
        plan: Arc<NonToolPlan>,
        configure_connection: impl FnOnce(&mut crate::config::Connection),
    ) -> RunningControl {
        let mut connection = handle.current().unwrap().connections["a"].clone();
        connection.access.non_tools = vec![plan];
        configure_connection(&mut connection);
        let supervisor = std::env::var_os("CARGO_BIN_EXE_demoncoder")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::env::current_exe()
                    .unwrap()
                    .parent()
                    .unwrap()
                    .parent()
                    .unwrap()
                    .join("demoncoder")
            });
        assert!(
            supervisor.is_file(),
            "build the application test supervisor first"
        );
        connection.access.supervisor = Some(supervisor);
        let limits = Limits {
            seconds: 60,
            model_calls: 16,
            tool_calls: 16,
        };
        let (runtime, _) = SharedRuntime::open_with_session_hooks(
            root,
            &connection,
            None,
            &CaptureScope::default(),
            Some(&limits),
        )
        .unwrap();
        let initial = runtime.record().unwrap();
        let initial_allowance = initial.session_hook_allowance.as_ref().unwrap();
        let allowance_started_ms = initial_allowance.allocation.started_ms;
        let allowance_deadline_ms = initial_allowance.allocation.deadline_ms;
        let tools = ToolExecutor::with_policy(root, &connection.access).unwrap();
        let workflow = WorkflowSession::new(
            Box::new(NativeSession::with_tools(Box::new(IdleModel), tools)),
            connection.clone(),
            root.to_owned(),
            WorkflowSettings::default(),
            runtime.clone(),
            false,
        )
        .unwrap()
        .with_live_settings(handle.clone());
        let (command_tx, command_rx) = mpsc::channel(4);
        let (event_tx, mut event_rx) = mpsc::channel(32);
        let events = EventSink::new("settings-runner-test".into(), event_tx, None)
            .unwrap()
            .with_runtime(runtime.clone());
        let worker = tokio::spawn(session::run(Box::new(workflow), command_rx, events));
        while !matches!(event_rx.recv().await.unwrap().event, Event::Ready { .. }) {}
        RunningControl {
            connection,
            limits,
            runtime,
            command_tx,
            worker,
            _event_rx: event_rx,
            allowance_started_ms,
            allowance_deadline_ms,
        }
    }

    fn fixture(root: &std::path::Path) -> (Handle, std::path::PathBuf) {
        std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = root.join("settings.toml");
        let config: Config = serde_json::from_value(serde_json::json!({
            "default_connection": "a", "onboarding_complete": true,
            "connections": {
                "a": {"adapter":"openai-api", "model":"a-model", "api_key":"synthetic-settings-key"},
                "b": {"adapter":"anthropic-api", "model":"b-model", "api_key":"synthetic-other-key"}
            }
        })).unwrap();
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

    fn configure(draft: &mut Draft, connection: &str, model: &str) {
        let mut assignments = Assignments::from_config(&draft.config);
        assignments.creator = Some(Assignment {
            connection: connection.into(),
            model: Some(model.into()),
            effort: None,
        });
        draft.config.settings = Some(assignments);
    }

    fn canary_draft(handle: &Handle) -> Draft {
        let mut draft = handle.draft().unwrap();
        configure(&mut draft, "a", "private-model-canary");
        draft.config.connections.get_mut("a").unwrap().api_key =
            Some("private-api-key-canary".into());
        draft
    }

    fn assert_save_effect(
        handle: &Handle,
        path: &std::path::Path,
        original: &[u8],
        allow: bool,
        expected_denial: &str,
        run: SaveRun,
        expected: ExpectedAccounting,
    ) {
        assert_original_session_accounting(&run, expected);
        let result = run.result;
        if allow {
            result.unwrap();
            assert_ne!(std::fs::read(path).unwrap(), original);
            assert_eq!(
                handle.role(Role::Creator, None).unwrap().model.as_deref(),
                Some("private-model-canary")
            );
        } else {
            let error = result.unwrap_err();
            assert!(error.to_string().contains(expected_denial), "{error:#}");
            assert_eq!(std::fs::read(path).unwrap(), original);
            assert_eq!(
                handle.role(Role::Creator, None).unwrap().model.as_deref(),
                Some("a-model")
            );
        }
    }

    fn assert_original_session_accounting(run: &SaveRun, expected: ExpectedAccounting) {
        assert!(
            run.record.task.is_none(),
            "ConfigChange acquired a task owner"
        );
        assert!(
            run.record.allocation.is_none(),
            "ConfigChange acquired a task allocation"
        );
        let lifetime = run
            .record
            .operations
            .iter()
            .find_map(|operation| match &operation.host_invocation {
                Some(HostInvocation::NativeSession(lifetime)) => Some((operation.id, lifetime)),
                _ => None,
            })
            .expect("original host session receipt");
        let config_change = run
            .record
            .operations
            .iter()
            .find_map(|operation| match &operation.host_invocation {
                Some(HostInvocation::Lifecycle(receipt))
                    if receipt.facts.subject.occurrence.event() == HookEvent::ConfigChange =>
                {
                    Some((operation, receipt))
                }
                _ => None,
            })
            .expect("ConfigChange receipt");
        assert_eq!(config_change.1.facts.host_session, Some(lifetime.0));
        assert_eq!(config_change.1.facts.task, None);
        assert_eq!(config_change.1.facts.child_owner, None);
        assert!(
            run.record.operations.iter().all(|operation| matches!(
                &operation.budget,
                Some(BudgetRef::SessionHooks { session }) if session == &lifetime.1.session
            )),
            "ConfigChange or a funded suboperation left the original SessionHooks allowance"
        );
        assert!(matches!(
            &config_change.0.budget,
            Some(BudgetRef::SessionHooks { session }) if session == &lifetime.1.session
        ));
        assert_eq!(
            run.record
                .operations
                .iter()
                .filter(|operation| matches!(
                    operation.host_invocation,
                    Some(HostInvocation::HookModel { owner }) if owner == config_change.0.id
                ))
                .count(),
            expected.model_operations
        );
        assert_eq!(
            run.record
                .operations
                .iter()
                .filter(|operation| matches!(
                    operation.host_invocation,
                    Some(HostInvocation::PluginService { .. })
                ))
                .count(),
            expected.service_operations
        );
        let allowance = run.record.session_hook_allowance.as_ref().unwrap();
        assert_eq!(allowance.allocation.started_ms, run.allowance_started_ms);
        assert_eq!(allowance.allocation.deadline_ms, run.allowance_deadline_ms);
        assert_eq!(allowance.allocation.model_calls, expected.model_calls);
        assert_eq!(allowance.allocation.tool_calls, expected.tool_calls);
        assert_eq!(
            allowance.allocation.usage.reported_input,
            expected.reported_input
        );
        assert_eq!(
            allowance.allocation.usage.reported_output,
            expected.reported_output
        );
        assert_eq!(allowance.backend_invocations, 0);
        assert_eq!(run.record.backend_invocations, 0);
    }

    fn config_change_receipt(
        record: &Record,
    ) -> (
        &crate::workflow::runtime::Operation,
        &crate::plugins::receipts::NonToolReceipt,
    ) {
        record
            .operations
            .iter()
            .find_map(|operation| {
                operation.non_tool_receipt().and_then(|receipt| {
                    (receipt.facts.subject.occurrence.event() == HookEvent::ConfigChange)
                        .then_some((operation, receipt))
                })
            })
            .expect("ConfigChange receipt")
    }

    fn assert_one_retained_unpublished_invocation(record: &Record) {
        let (operation, receipt) = config_change_receipt(record);
        assert!(operation.complete && receipt.settled);
        assert_eq!(receipt.publication, None);
        assert_eq!(receipt.hooks.len(), 1);
        assert!(receipt.hooks[0].outcome.is_some());
    }

    fn published_config_change(payload: &serde_json::Value) -> bool {
        payload["operations"].as_array().is_some_and(|operations| {
            operations.iter().any(|operation| {
                operation["host_invocation"]["lifecycle"]["publication"] == "published"
            })
        })
    }

    fn unrelated_settings_persistence_transition(payload: &serde_json::Value) -> bool {
        payload["phase"] == "settings-persistence-fault"
    }

    fn unsettled_config_change(payload: &serde_json::Value) -> bool {
        payload["operations"].as_array().is_some_and(|operations| {
            operations.iter().any(|operation| {
                operation["host_invocation"]["lifecycle"]["settled"] == false
                    && operation["host_invocation"]["lifecycle"]["facts"]["subject"]
                        ["occurrence"]["event"]
                        == "ConfigChange"
            })
        })
    }

    #[tokio::test]
    async fn active_config_change_denial_keeps_actual_settings_unpublished() {
        let root = tempfile::tempdir().unwrap();
        let (handle, path) = fixture(root.path());
        let original = std::fs::read(&path).unwrap();
        let mut connection = handle.current().unwrap().connections["a"].clone();
        connection.access.non_tools = vec![denying_config_change_plan()];
        let (runtime, _) = SharedRuntime::open(root.path(), &connection, None).unwrap();
        let tools = ToolExecutor::with_policy(root.path(), &connection.access).unwrap();
        let native = NativeSession::with_tools(Box::new(IdleModel), tools);
        let workflow = WorkflowSession::new(
            Box::new(native),
            connection,
            root.path().to_owned(),
            WorkflowSettings::default(),
            runtime.clone(),
            false,
        )
        .unwrap()
        .with_live_settings(handle.clone());
        let (command_tx, command_rx) = mpsc::channel(4);
        let (event_tx, mut event_rx) = mpsc::channel(32);
        let events = EventSink::new("settings-test".into(), event_tx, None)
            .unwrap()
            .with_runtime(runtime.clone());
        let worker = tokio::spawn(session::run(Box::new(workflow), command_rx, events));
        while !matches!(event_rx.recv().await.unwrap().event, Event::Ready { .. }) {}

        let mut draft = handle.draft().unwrap();
        configure(&mut draft, "a", "proposed-model");
        let result = handle.save(&mut draft).await;
        command_tx.send(Command::Shutdown).await.unwrap();
        worker.await.unwrap().unwrap();

        let error = result.expect_err("the active ConfigChange policy must gate Handle::save");
        assert!(
            error.to_string().contains("active policy denied settings"),
            "{error:#}"
        );
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert_eq!(
            handle.role(Role::Creator, None).unwrap().model.as_deref(),
            Some("a-model")
        );
    }

    #[tokio::test]
    async fn settings_control_does_not_borrow_a_stable_task_owner_or_allocation() {
        let root = tempfile::tempdir().unwrap();
        let (handle, _path) = fixture(root.path());
        let mut running = start_control(
            &handle,
            root.path(),
            config_change_plan(Arc::new(AllowConfigChange(Arc::new(Mutex::new(vec![]))))),
        )
        .await;
        let (reply, accepted) = tokio::sync::oneshot::channel();
        running
            .command_tx
            .send(Command::Submit {
                text: "/task preserve the admitted coding task".into(),
                reply,
            })
            .await
            .unwrap();
        accepted.await.unwrap().unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            while running
                .runtime
                .record()
                .unwrap()
                .task
                .as_ref()
                .is_none_or(|task| !task.stopped)
            {
                let envelope = running
                    ._event_rx
                    .recv()
                    .await
                    .expect("coding task event stream closed");
                assert!(
                    !matches!(envelope.event, Event::Error { .. }),
                    "coding task failed before the Settings check: {:?}",
                    envelope.event
                );
            }
        })
        .await
        .expect("coding task did not finish its baseline turn");
        let before = running.runtime.record().unwrap();
        let task_before = before.task.as_ref().map(|task| {
            (
                task.id,
                task.objective.clone(),
                task.stopped,
                task.accepted.clone(),
                task.corrections,
                task.verification_generation,
            )
        });
        let allocation_before = before.allocation.as_ref().map(|allocation| {
            (
                allocation.started_ms,
                allocation.deadline_ms,
                allocation.model_calls,
                allocation.tool_calls,
            )
        });

        let mut draft = canary_draft(&handle);
        assert!(matches!(
            handle.save(&mut draft).await,
            Ok(SaveStatus::Applied)
        ));

        let after = running.runtime.record().unwrap();
        assert_eq!(
            after.task.as_ref().map(|task| (
                task.id,
                task.objective.clone(),
                task.stopped,
                task.accepted.clone(),
                task.corrections,
                task.verification_generation,
            )),
            task_before
        );
        assert_eq!(
            after.allocation.as_ref().map(|allocation| (
                allocation.started_ms,
                allocation.deadline_ms,
                allocation.model_calls,
                allocation.tool_calls,
            )),
            allocation_before
        );
        let (operation, receipt) = after
            .operations
            .iter()
            .filter_map(|operation| {
                operation
                    .non_tool_receipt()
                    .map(|receipt| (operation, receipt))
            })
            .find(|(_, receipt)| {
                receipt.facts.subject.occurrence.event() == HookEvent::ConfigChange
            })
            .expect("ConfigChange receipt");
        assert_eq!(
            receipt.facts.task, None,
            "Settings borrowed the coding task"
        );
        assert_eq!(receipt.facts.child_owner, None);
        assert!(matches!(
            &operation.budget,
            Some(BudgetRef::SessionHooks { session }) if session == &receipt.facts.session
        ));
        running.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn application_shutdown_joins_actual_settings_command_descendants() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("effects")).unwrap();
        let (handle, path) = fixture(root.path());
        let original = std::fs::read(&path).unwrap();
        let token = format!("settings-shutdown-command-{}", std::process::id());
        let plan = plan_from_registration(
            crate::config_change_test_support::pending_command_registration(&token),
        );
        let running = start_control(&handle, root.path(), plan).await;
        let mut draft = handle.draft().unwrap();
        configure(&mut draft, "a", "must-not-publish-after-shutdown");
        let save = tokio::spawn({
            let handle = handle.clone();
            async move { handle.save(&mut draft).await }
        });
        tokio::time::timeout(Duration::from_secs(3), async {
            while crate::config_change_test_support::processes(&token).len() < 2 {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("actual Settings command descendants did not start");

        let runtime = running.runtime.clone();
        let started = tokio::time::Instant::now();
        session::shutdown(running.command_tx, running.worker, false)
            .await
            .unwrap()
            .unwrap();
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "external application shutdown exceeded its existing three-second whole-operation bound"
        );
        let joined_before_shutdown_returned =
            crate::config_change_test_support::processes(&token).is_empty();
        let save_result = tokio::time::timeout(Duration::from_secs(1), save)
            .await
            .expect("cancelled Settings save future remained detached")
            .unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            while !crate::config_change_test_support::processes(&token).is_empty() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("failure control left actual Settings command descendants running");
        assert!(
            joined_before_shutdown_returned,
            "application shutdown returned before its Settings command descendants stopped"
        );
        assert!(
            save_result.is_err(),
            "shutdown must not publish the cancelled proposal"
        );
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert_eq!(
            handle.role(Role::Creator, None).unwrap().model.as_deref(),
            Some("a-model")
        );
        let record = runtime.record().unwrap();
        let (_, receipt) = config_change_receipt(&record);
        assert_eq!(receipt.publication, None);
    }

    #[tokio::test]
    async fn save_task_failure_still_runs_attempt_owned_cleanup() {
        let root = tempfile::tempdir().unwrap();
        let (handle, path) = fixture(root.path());
        let original = std::fs::read(&path).unwrap();
        let running = start_control(
            &handle,
            root.path(),
            config_change_plan(Arc::new(PanicConfigChange)),
        )
        .await;
        let mut draft = handle.draft().unwrap();
        configure(&mut draft, "a", "must-not-publish-after-task-failure");
        let job = handle.start_save(draft);

        tokio::time::timeout(Duration::from_secs(1), async {
            while !job.is_finished() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("failed Settings task did not complete its cleanup");
        let (outcome, cleanup) = job.finish().await;

        let error = outcome.expect_err("panicked Settings task must fail the save");
        assert!(
            error.to_string().contains("settings save task failed"),
            "{error:#}"
        );
        cleanup.expect("the attempt owner must drain after its save task fails");
        assert_eq!(std::fs::read(&path).unwrap(), original);
        let record = running.runtime.record().unwrap();
        let operation = record
            .operations
            .iter()
            .find(|operation| {
                operation.non_tool_receipt().is_some_and(|receipt| {
                    receipt.facts.subject.occurrence.event() == HookEvent::ConfigChange
                })
            })
            .expect("failed Settings attempt receipt");
        assert!(!operation.complete);
        assert_eq!(operation.non_tool_receipt().unwrap().publication, None);
        running.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn cancellation_before_operation_creation_admits_no_handler() {
        let root = tempfile::tempdir().unwrap();
        let (handle, path) = fixture(root.path());
        let original = std::fs::read(&path).unwrap();
        let captured = Arc::new(Mutex::new(Vec::new()));
        let running = start_control(
            &handle,
            root.path(),
            config_change_plan(Arc::new(AllowConfigChange(captured.clone()))),
        )
        .await;
        let mut draft = handle.draft().unwrap();
        configure(&mut draft, "a", "must-not-publish-before-operation");
        let job = handle.start_save(draft);
        job.cancel();

        tokio::time::timeout(Duration::from_secs(1), async {
            while !job.is_finished() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("pre-operation cancellation did not settle");
        let (outcome, cleanup) = job.finish().await;

        assert!(outcome.is_err());
        cleanup.expect("pre-operation cancellation has no detached runner to drain");
        assert!(captured.lock().unwrap().is_empty());
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert!(
            !running
                .runtime
                .record()
                .unwrap()
                .operations
                .iter()
                .any(|operation| {
                    operation.non_tool_receipt().is_some_and(|receipt| {
                        receipt.facts.subject.occurrence.event() == HookEvent::ConfigChange
                    })
                })
        );
        running.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn immediate_ungated_save_job_cancellation_preserves_private_settings() {
        let root = tempfile::tempdir().unwrap();
        let (handle, path) = fixture(root.path());
        let original = std::fs::read(&path).unwrap();
        let mut draft = handle.draft().unwrap();
        configure(&mut draft, "a", "cancelled-ungated-model");
        let job = handle.start_save(draft);

        job.cancel();
        let (outcome, cleanup) = job.finish().await;

        let error = outcome.expect_err("cancelled ungated save must not publish");
        assert!(error.to_string().contains("cancel"), "{error:#}");
        cleanup.expect("pre-admission cancellation created no work to clean up");
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert_eq!(
            handle.role(Role::Creator, None).unwrap().model.as_deref(),
            Some("a-model")
        );
    }

    #[tokio::test]
    async fn production_command_runner_gates_native_and_claude_actual_saves() {
        for dialect in [HookDialect::Native, HookDialect::Claude] {
            for allow in [false, true] {
                let root = tempfile::tempdir().unwrap();
                let (handle, path) = fixture(root.path());
                let original = std::fs::read(&path).unwrap();
                let mut draft = canary_draft(&handle);
                let plan = plan_from_registration(
                    crate::config_change_test_support::command_registration(dialect, allow),
                );

                let result = save_with_plan(&handle, root.path(), plan, &mut draft).await;

                assert_save_effect(
                    &handle,
                    &path,
                    &original,
                    allow,
                    "production command verdict",
                    result,
                    ExpectedAccounting {
                        model_calls: 0,
                        tool_calls: 0,
                        reported_input: 0,
                        reported_output: 0,
                        model_operations: 0,
                        service_operations: 0,
                    },
                );
            }
        }
    }

    #[tokio::test]
    async fn production_http_runner_gates_native_and_claude_actual_saves() {
        for dialect in [HookDialect::Native, HookDialect::Claude] {
            for allow in [false, true] {
                let root = tempfile::tempdir().unwrap();
                let (handle, path) = fixture(root.path());
                let original = std::fs::read(&path).unwrap();
                let mut draft = canary_draft(&handle);
                let (registration, peer) =
                    crate::config_change_test_support::http_registration(dialect, allow).await;

                let result = save_with_plan(
                    &handle,
                    root.path(),
                    plan_from_registration(registration),
                    &mut draft,
                )
                .await;

                assert_eq!(peer.count(), 1, "{dialect:?} did not perform one HTTP call");
                assert_save_effect(
                    &handle,
                    &path,
                    &original,
                    allow,
                    "production HTTP verdict",
                    result,
                    ExpectedAccounting {
                        model_calls: 0,
                        tool_calls: 0,
                        reported_input: 0,
                        reported_output: 0,
                        model_operations: 0,
                        service_operations: 0,
                    },
                );
            }
        }
    }

    #[tokio::test]
    async fn production_mcp_runner_gates_native_and_claude_actual_saves() {
        for dialect in [HookDialect::Native, HookDialect::Claude] {
            for allow in [false, true] {
                let root = tempfile::tempdir().unwrap();
                let (handle, path) = fixture(root.path());
                let original = std::fs::read(&path).unwrap();
                let mut draft = canary_draft(&handle);
                let (registration, peer, service) =
                    crate::config_change_test_support::mcp_registration(
                        root.path(),
                        dialect,
                        allow,
                    )
                    .await;

                let result = save_with_plan(
                    &handle,
                    root.path(),
                    plan_from_registration(registration),
                    &mut draft,
                )
                .await;
                service.stop().await.unwrap();

                assert_eq!(
                    peer.method_count("tools/call"),
                    1,
                    "{dialect:?} did not perform one MCP tool call"
                );
                assert_save_effect(
                    &handle,
                    &path,
                    &original,
                    allow,
                    "production MCP verdict",
                    result,
                    ExpectedAccounting {
                        model_calls: 0,
                        tool_calls: 0,
                        reported_input: 0,
                        reported_output: 0,
                        model_operations: 0,
                        service_operations: 1,
                    },
                );
            }
        }
    }

    #[tokio::test]
    async fn production_prompt_and_agent_runners_gate_native_actual_saves() {
        for kind in [HandlerKind::Prompt, HandlerKind::Agent] {
            for allow in [false, true] {
                let root = tempfile::tempdir().unwrap();
                let (handle, path) = fixture(root.path());
                let original = std::fs::read(&path).unwrap();
                let mut draft = canary_draft(&handle);
                let (registration, peer) =
                    crate::config_change_test_support::model_registration(kind, allow).await;

                let result = save_with_plan(
                    &handle,
                    root.path(),
                    plan_from_registration(registration),
                    &mut draft,
                )
                .await;

                assert_eq!(
                    peer.count(),
                    1,
                    "{kind:?} did not perform one model request"
                );
                assert_save_effect(
                    &handle,
                    &path,
                    &original,
                    allow,
                    "production model verdict",
                    result,
                    ExpectedAccounting {
                        model_calls: 1,
                        tool_calls: 0,
                        reported_input: 1,
                        reported_output: 1,
                        model_operations: 1,
                        service_operations: 0,
                    },
                );
            }
        }
    }

    #[tokio::test]
    async fn required_policy_is_closed_until_host_capability_is_ready() {
        let root = tempfile::tempdir().unwrap();
        let (handle, path) = fixture(root.path());
        let original = std::fs::read(&path).unwrap();
        handle.require_control(true);
        let mut draft = handle.draft().unwrap();
        configure(&mut draft, "a", "proposed-model");

        let error = handle.save(&mut draft).await.unwrap_err();

        assert!(error.to_string().contains("host capability is not ready"));
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }

    #[tokio::test]
    async fn allowed_config_change_publishes_without_exposing_private_values() {
        let root = tempfile::tempdir().unwrap();
        let (handle, path) = fixture(root.path());
        let captured = Arc::new(Mutex::new(Vec::new()));
        let mut connection = handle.current().unwrap().connections["a"].clone();
        connection.access.non_tools = vec![config_change_plan(Arc::new(AllowConfigChange(
            captured.clone(),
        )))];
        let (runtime, _) = SharedRuntime::open(root.path(), &connection, None).unwrap();
        let tools = ToolExecutor::with_policy(root.path(), &connection.access).unwrap();
        let workflow = WorkflowSession::new(
            Box::new(NativeSession::with_tools(Box::new(IdleModel), tools)),
            connection,
            root.path().to_owned(),
            WorkflowSettings::default(),
            runtime.clone(),
            false,
        )
        .unwrap()
        .with_live_settings(handle.clone());
        let (command_tx, command_rx) = mpsc::channel(4);
        let (event_tx, mut event_rx) = mpsc::channel(32);
        let events = EventSink::new("settings-allow-test".into(), event_tx, None)
            .unwrap()
            .with_runtime(runtime.clone());
        let worker = tokio::spawn(session::run(Box::new(workflow), command_rx, events));
        while !matches!(event_rx.recv().await.unwrap().event, Event::Ready { .. }) {}

        let mut draft = handle.draft().unwrap();
        configure(&mut draft, "a", "private-model-canary");
        let proposed = draft.config.connections.get_mut("a").unwrap();
        proposed.api_key = Some("private-api-key-canary".into());
        proposed.endpoint = Some("https://private-endpoint-canary.example/secret".into());
        handle.save(&mut draft).await.unwrap();
        assert!(
            runtime
                .record()
                .unwrap()
                .operations
                .iter()
                .any(|operation| {
                    operation.non_tool_receipt().is_some_and(|receipt| {
                        receipt.publication
                            == Some(crate::plugins::receipts::ConfigPublication::Published)
                    })
                })
        );
        command_tx.send(Command::Shutdown).await.unwrap();
        worker.await.unwrap().unwrap();

        let frame = serde_json::to_string(&captured.lock().unwrap()[0]).unwrap();
        for canary in [
            "private-model-canary",
            "private-api-key-canary",
            "private-endpoint-canary",
            "synthetic-settings-key",
        ] {
            assert!(!frame.contains(canary), "private canary escaped: {canary}");
        }
        let value = &captured.lock().unwrap()[0];
        assert_eq!(value["subject"]["occurrence"]["source"], "user_settings");
        assert_eq!(
            value["subject"]["occurrence"]["structure"]["connection_count"],
            2
        );
        assert!(
            String::from_utf8(std::fs::read(&path).unwrap())
                .unwrap()
                .contains("private-model-canary")
        );
    }

    #[tokio::test]
    async fn inherited_roles_follow_creator_while_overrides_and_authority_remain_distinct() {
        let root = tempfile::tempdir().unwrap();
        let (handle, _) = fixture(root.path());
        let mut draft = handle.draft().unwrap();
        configure(&mut draft, "a", "a-model");
        draft.config.settings.as_mut().unwrap().overrides.insert(
            Role::Judge,
            Assignment {
                connection: "a".into(),
                model: Some("judge-model".into()),
                effort: Some("high".into()),
            },
        );
        handle.save(&mut draft).await.unwrap();
        configure(&mut draft, "b", "b-model");
        handle.save(&mut draft).await.unwrap();
        for role in [Role::Worker, Role::Oracle, Role::Reviewer, Role::Advisor] {
            let connection = handle.role(role, None).unwrap();
            assert_eq!(connection.model.as_deref(), Some("b-model"));
            assert!(!connection.access.tools_enabled);
            assert!(!connection.access.unrestricted);
            assert!(connection.access.extension.is_none());
        }
        assert_eq!(
            handle.role(Role::Judge, None).unwrap().model.as_deref(),
            Some("judge-model")
        );
        assert!(handle.args().agent_settings().unwrap().is_none());
        assert!(
            handle
                .args()
                .workflow_settings()
                .unwrap()
                .reviewer
                .is_none()
        );
        assert_eq!(
            handle
                .role(Role::Reviewer, Some("a"))
                .unwrap()
                .model
                .as_deref(),
            Some("a-model")
        );
        draft
            .config
            .settings
            .as_mut()
            .unwrap()
            .overrides
            .remove(&Role::Judge);
        handle.save(&mut draft).await.unwrap();
        assert_eq!(
            handle.role(Role::Judge, None).unwrap().model.as_deref(),
            Some("b-model")
        );
    }

    #[tokio::test]
    async fn private_publication_rejects_competing_edits_and_preserves_active_revision() {
        let root = tempfile::tempdir().unwrap();
        let (handle, path) = fixture(root.path());
        let mut first = handle.draft().unwrap();
        let mut second = handle.draft().unwrap();
        configure(&mut first, "a", "first-model");
        configure(&mut second, "b", "second-model");
        handle.save(&mut first).await.unwrap();
        let saved = std::fs::read(&path).unwrap();
        let error = handle.save(&mut second).await.unwrap_err();
        assert!(error.to_string().contains("another editor"), "{error:#}");
        assert_eq!(std::fs::read(&path).unwrap(), saved);
        assert_eq!(
            handle.role(Role::Creator, None).unwrap().model.as_deref(),
            Some("first-model")
        );
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let mut stale = handle.draft().unwrap();
        let mut changed = saved;
        changed.extend_from_slice(b"\n# independent edit\n");
        std::fs::write(&path, &changed).unwrap();
        assert!(
            handle.save(&mut stale).await.is_err(),
            "even a concurrent comment edit must not be lost"
        );
        assert_eq!(std::fs::read(&path).unwrap(), changed);
    }

    #[tokio::test]
    async fn failed_or_invalid_save_does_not_apply_and_deselection_never_redirects() {
        let root = tempfile::tempdir().unwrap();
        let (handle, path) = fixture(root.path());
        let mut draft = handle.draft().unwrap();
        configure(&mut draft, "a", "a-model");
        handle.save(&mut draft).await.unwrap();
        let saved = std::fs::read(&path).unwrap();
        draft
            .config
            .settings
            .as_mut()
            .unwrap()
            .creator
            .as_mut()
            .unwrap()
            .model = Some("bad\nmodel".into());
        let error = handle.save(&mut draft).await.unwrap_err();
        assert!(!error.to_string().contains("synthetic-settings-key"));
        assert_eq!(std::fs::read(&path).unwrap(), saved);
        configure(&mut draft, "b", "b-model");
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o777)).unwrap();
        assert!(handle.save(&mut draft).await.is_err());
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(
            handle.role(Role::Creator, None).unwrap().model.as_deref(),
            Some("a-model")
        );
        configure(&mut draft, "a", "a-model");
        draft
            .config
            .settings
            .as_mut()
            .unwrap()
            .providers
            .retain(|p| p != "a");
        handle.save(&mut draft).await.unwrap();
        assert!(
            handle
                .role(Role::Creator, None)
                .err()
                .unwrap()
                .to_string()
                .contains("deselected")
        );
        assert_eq!(
            handle
                .current()
                .unwrap()
                .settings
                .unwrap()
                .creator
                .unwrap()
                .connection,
            "a"
        );
        let restarted = Handle::open(handle.args()).unwrap();
        assert!(restarted.role(Role::Creator, None).is_err());
    }

    #[tokio::test]
    async fn changed_inspected_workspace_holds_without_replaying_effectful_gate() {
        let root = tempfile::tempdir().unwrap();
        let (handle, path) = fixture(root.path());
        let original = std::fs::read(&path).unwrap();
        let inspected = root.path().join("inspected.txt");
        std::fs::write(&inspected, "before").unwrap();
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let effects = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let plan = config_change_plan_with_reads(
            Arc::new(HeldAllowConfigChange {
                entered: entered.clone(),
                release: release.clone(),
                effects: effects.clone(),
            }),
            GateReadSet::new(vec!["inspected.txt".into()], vec![], vec![]).unwrap(),
        );
        let running = start_control(&handle, root.path(), plan).await;
        let mut draft = handle.draft().unwrap();
        configure(&mut draft, "a", "changed-workspace-model");
        let saving_handle = handle.clone();
        let save = tokio::spawn(async move { saving_handle.save(&mut draft).await });

        entered.notified().await;
        std::fs::write(&inspected, "after").unwrap();
        release.notify_one();
        let error = save.await.unwrap().unwrap_err();

        assert!(
            error.to_string().contains("inspected inputs changed"),
            "{error:#}"
        );
        assert_eq!(effects.load(Ordering::SeqCst), 1);
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert_eq!(
            handle.role(Role::Creator, None).unwrap().model.as_deref(),
            Some("a-model")
        );
        assert_one_retained_unpublished_invocation(&running.runtime.record().unwrap());
        running.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn changed_disk_revision_after_pass_is_rejected_without_replaying_gate() {
        let root = tempfile::tempdir().unwrap();
        let (handle, path) = fixture(root.path());
        let mut changed = std::fs::read(&path).unwrap();
        changed.extend_from_slice(b"\n# concurrent editor\n");
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let effects = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let running = start_control(
            &handle,
            root.path(),
            config_change_plan(Arc::new(HeldAllowConfigChange {
                entered: entered.clone(),
                release: release.clone(),
                effects: effects.clone(),
            })),
        )
        .await;
        let mut draft = handle.draft().unwrap();
        configure(&mut draft, "a", "stale-pass-model");
        let saving_handle = handle.clone();
        let save = tokio::spawn(async move { saving_handle.save(&mut draft).await });

        entered.notified().await;
        std::fs::write(&path, &changed).unwrap();
        release.notify_one();
        let error = save.await.unwrap().unwrap_err();

        assert!(error.to_string().contains("another editor"), "{error:#}");
        assert_eq!(effects.load(Ordering::SeqCst), 1);
        assert_eq!(std::fs::read(&path).unwrap(), changed);
        assert_eq!(
            handle.role(Role::Creator, None).unwrap().model.as_deref(),
            Some("a-model")
        );
        assert_one_retained_unpublished_invocation(&running.runtime.record().unwrap());
        running.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn held_original_policy_rejects_pending_pass_without_replaying_gate() {
        let root = tempfile::tempdir().unwrap();
        let (handle, path) = fixture(root.path());
        let original = std::fs::read(&path).unwrap();
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let effects = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let running = start_control(
            &handle,
            root.path(),
            config_change_plan(Arc::new(HeldAllowConfigChange {
                entered: entered.clone(),
                release: release.clone(),
                effects: effects.clone(),
            })),
        )
        .await;
        let mut draft = handle.draft().unwrap();
        configure(&mut draft, "a", "held-policy-model");
        let saving_handle = handle.clone();
        let save = tokio::spawn(async move { saving_handle.save(&mut draft).await });

        entered.notified().await;
        handle.require_control(true);
        release.notify_one();
        let error = save.await.unwrap().unwrap_err();

        assert!(
            error.to_string().contains("policy or host owner changed"),
            "{error:#}"
        );
        assert_eq!(effects.load(Ordering::SeqCst), 1);
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert_one_retained_unpublished_invocation(&running.runtime.record().unwrap());
        running.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn abort_before_final_admission_revokes_detached_publication_worker() {
        let root = tempfile::tempdir().unwrap();
        let (handle, path) = fixture(root.path());
        let original = std::fs::read(&path).unwrap();
        let effects = Arc::new(Mutex::new(Vec::new()));
        let running = start_control(
            &handle,
            root.path(),
            config_change_plan(Arc::new(AllowConfigChange(effects.clone()))),
        )
        .await;
        let (admission_waiting, release_admission) = handle.pause_next_before_final_admission();
        let mut draft = handle.draft().unwrap();
        configure(&mut draft, "a", "cancelled-before-admission-model");
        let saving_handle = handle.clone();
        let save = tokio::spawn(async move { saving_handle.save(&mut draft).await });

        admission_waiting
            .await
            .expect("blocking worker reached final admission");
        assert_eq!(std::fs::read(&path).unwrap(), original);
        save.abort();
        assert!(save.await.unwrap_err().is_cancelled());
        release_admission.send(()).unwrap();
        handle.wait_for_publication_transaction().await;

        assert_eq!(effects.lock().unwrap().len(), 1);
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert_eq!(
            handle.role(Role::Creator, None).unwrap().model.as_deref(),
            Some("a-model")
        );
        assert_one_retained_unpublished_invocation(&running.runtime.record().unwrap());
        running.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn failed_runtime_persistence_holds_final_settings_publication() {
        let root = tempfile::tempdir().unwrap();
        let (handle, path) = fixture(root.path());
        let original = std::fs::read(&path).unwrap();
        let effects = Arc::new(Mutex::new(Vec::new()));
        let running = start_control(
            &handle,
            root.path(),
            config_change_plan(Arc::new(AllowConfigChange(effects.clone()))),
        )
        .await;
        let (at_final_admission, release_final_admission) =
            handle.pause_next_before_final_admission();
        let pending = handle.start_save(canary_draft(&handle));
        tokio::time::timeout(Duration::from_secs(10), at_final_admission)
            .await
            .expect("Settings save did not reach final Runtime admission")
            .expect("Settings final admission pause was dropped");

        running
            .runtime
            .fail_config_change_receipt_before_rename_when(
                unrelated_settings_persistence_transition,
            );
        let persistence_error = running
            .runtime
            .begin_phase("settings-persistence-fault", None)
            .unwrap_err();
        assert!(
            format!("{persistence_error:#}")
                .contains("persist session transition; execution is held"),
            "unexpected persistence failure: {persistence_error:#}"
        );
        assert!(
            !running.runtime.record().unwrap().recovery_pending,
            "test used recovery_pending instead of the persistence-failure hold"
        );
        release_final_admission.send(()).unwrap();

        let (outcome, cleanup) = pending.finish().await;
        let error = outcome.unwrap_err();
        assert!(
            format!("{error:#}")
                .contains("session persistence failed; execution is held until recovery"),
            "final admission did not report the persistence hold: {error:#}"
        );
        assert!(
            cleanup
                .unwrap_err()
                .to_string()
                .contains("session persistence failed; execution is held until recovery")
        );
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert_eq!(
            handle.role(Role::Creator, None).unwrap().model.as_deref(),
            Some("a-model")
        );
        assert_eq!(effects.lock().unwrap().len(), 1);
        let record = running.runtime.record().unwrap();
        let (_, receipt) = config_change_receipt(&record);
        assert!(receipt.publication.is_none());
        assert!(running.shutdown().await.is_err());
    }

    #[tokio::test]
    async fn save_captured_without_policy_cannot_publish_after_policy_becomes_required() {
        let root = tempfile::tempdir().unwrap();
        let (handle, path) = fixture(root.path());
        let original = std::fs::read(&path).unwrap();
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        handle.pause_next_publication(entered.clone(), release.clone());
        let mut draft = handle.draft().unwrap();
        configure(&mut draft, "a", "ungated-race-model");
        let saving_handle = handle.clone();
        let save = tokio::spawn(async move { saving_handle.save(&mut draft).await });

        entered.notified().await;
        handle.require_control(true);
        release.notify_one();
        let error = save.await.unwrap().unwrap_err();

        assert!(
            error.to_string().contains("policy or host owner changed"),
            "{error:#}"
        );
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert_eq!(
            handle.role(Role::Creator, None).unwrap().model.as_deref(),
            Some("a-model")
        );
    }

    #[tokio::test]
    async fn competing_publications_do_not_leave_a_clean_revision_uncertain() {
        let root = tempfile::tempdir().unwrap();
        let (handle, path) = fixture(root.path());
        let (file_published, release_publication) = handle.pause_next_after_file_publication();
        let mut first = handle.draft().unwrap();
        configure(&mut first, "a", "first-concurrent-model");
        let mut second = handle.draft().unwrap();
        configure(&mut second, "a", "second-concurrent-model");
        let first_handle = handle.clone();
        let first_save = tokio::spawn(async move { first_handle.save(&mut first).await });
        file_published
            .await
            .expect("first save reached file publication");

        let second_result =
            tokio::time::timeout(Duration::from_millis(100), handle.save(&mut second))
                .await
                .expect("a competing save blocked the async executor");
        let error = second_result.unwrap_err();
        assert!(
            error.to_string().contains("already in progress"),
            "{error:#}"
        );
        release_publication.send(()).unwrap();

        assert!(matches!(
            first_save.await.unwrap().unwrap(),
            SaveStatus::Applied
        ));
        assert!(
            String::from_utf8(std::fs::read(&path).unwrap())
                .unwrap()
                .contains("first-concurrent-model")
        );
        assert_eq!(
            handle.role(Role::Creator, None).unwrap().model.as_deref(),
            Some("first-concurrent-model")
        );
    }

    #[tokio::test]
    async fn final_validation_uses_original_attempt_deadline() {
        let root = tempfile::tempdir().unwrap();
        let (handle, path) = fixture(root.path());
        let original = std::fs::read(&path).unwrap();
        let captured = Arc::new(Mutex::new(Vec::new()));
        let running = start_control(
            &handle,
            root.path(),
            config_change_plan(Arc::new(AllowConfigChange(captured.clone()))),
        )
        .await;
        handle.set_control_timeout(Duration::from_millis(100));
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        handle.pause_next_publication(entered.clone(), release.clone());
        let mut draft = handle.draft().unwrap();
        configure(&mut draft, "a", "expired-final-validation-model");
        let saving_handle = handle.clone();
        let save = tokio::spawn(async move { saving_handle.save(&mut draft).await });

        entered.notified().await;
        tokio::time::sleep(Duration::from_millis(150)).await;
        release.notify_one();
        let error = save.await.unwrap().unwrap_err();

        assert!(
            error
                .to_string()
                .contains("deadline expired before publication"),
            "{error:#}"
        );
        assert_eq!(captured.lock().unwrap().len(), 1);
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert_one_retained_unpublished_invocation(&running.runtime.record().unwrap());
        running.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn active_credential_alias_and_settings_file_stay_out_of_actual_snapshot() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let (handle, path) = fixture(root.path());
        let credential_dir = root.path().join("credentials");
        std::fs::create_dir(&credential_dir).unwrap();
        let credential = credential_dir.join("active.key");
        std::fs::write(&credential, "active-credential-canary").unwrap();
        let alias = root.path().join("active-credential-alias");
        symlink(&credential, &alias).unwrap();
        let proposed_only = root.path().join("proposed-only-public.txt");
        std::fs::write(&proposed_only, "proposed-only-public-canary").unwrap();
        let captured = Arc::new(Mutex::new(Vec::new()));
        let running = start_control_with(
            &handle,
            root.path(),
            config_change_plan(Arc::new(CaptureWorkspace(captured.clone()))),
            |connection| connection.access.credential_paths = vec![alias.clone()],
        )
        .await;
        let previous = running.connection.clone();
        let mut draft = handle.draft().unwrap();
        configure(&mut draft, "a", "credential-snapshot-model");
        draft
            .config
            .connections
            .get_mut("a")
            .unwrap()
            .access
            .credential_paths = vec![proposed_only.clone()];

        handle.save(&mut draft).await.unwrap();

        {
            let captured = captured.lock().unwrap();
            assert!(captured.iter().any(|(name, bytes)| {
                name == "proposed-only-public.txt" && bytes == b"proposed-only-public-canary"
            }));
            for protected in [
                "settings.toml",
                "active-credential-alias",
                "credentials/active.key",
            ] {
                assert!(
                    captured.iter().all(|(name, _)| name != protected),
                    "protected path entered snapshot: {protected}"
                );
            }
            let bytes = captured
                .iter()
                .flat_map(|(_, bytes)| bytes.iter().copied())
                .collect::<Vec<_>>();
            assert!(
                !bytes
                    .windows(b"active-credential-canary".len())
                    .any(|window| { window == b"active-credential-canary" })
            );
            assert!(
                !bytes
                    .windows(b"synthetic-settings-key".len())
                    .any(|window| { window == b"synthetic-settings-key" })
            );
        }
        assert!(
            String::from_utf8(std::fs::read(&path).unwrap())
                .unwrap()
                .contains("credential-snapshot-model")
        );
        let creator = handle.creator(&previous).unwrap().connection;
        assert_eq!(
            creator.access.credential_paths,
            previous.access.credential_paths
        );
        assert_eq!(creator.access.unrestricted, previous.access.unrestricted);
        assert_eq!(creator.access.tools_enabled, previous.access.tools_enabled);
        assert_eq!(creator.access.supervisor, previous.access.supervisor);
        assert_eq!(
            creator.access.non_tools.len(),
            previous.access.non_tools.len()
        );
        assert!(
            creator
                .access
                .non_tools
                .iter()
                .zip(&previous.access.non_tools)
                .all(|(current, original)| Arc::ptr_eq(current, original))
        );
        let role = handle.role(Role::Creator, None).unwrap();
        assert!(!role.access.tools_enabled);
        assert!(!role.access.unrestricted);
        assert!(role.access.credential_paths.is_empty());
        assert!(role.access.supervisor.is_none());
        assert!(role.access.non_tools.is_empty());
        assert!(role.access.pre_tool.is_none());
        assert!(role.access.post_tools.is_empty());
        running.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn real_pre_rename_failure_preserves_old_file_and_active_state() {
        let root = tempfile::tempdir().unwrap();
        let (handle, path) = fixture(root.path());
        let mut draft = handle.draft().unwrap();
        handle.save(&mut draft).await.unwrap();
        let original = std::fs::read(&path).unwrap();
        configure(&mut draft, "a", "must-not-publish-model");
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o500)).unwrap();

        let result = handle.save(&mut draft).await;

        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let error = result.unwrap_err();
        assert!(
            error
                .to_string()
                .contains("prepare private settings transaction"),
            "{error:#}"
        );
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert_eq!(
            handle.role(Role::Creator, None).unwrap().model.as_deref(),
            Some("a-model")
        );
    }

    #[tokio::test]
    async fn real_post_rename_failure_keeps_new_file_and_holds_handle_as_uncertain() {
        let root = tempfile::tempdir().unwrap();
        let (handle, path) = fixture(root.path());
        let original = std::fs::read(&path).unwrap();
        let mut draft = handle.draft().unwrap();
        configure(&mut draft, "a", "post-rename-model");
        handle.fail_next_directory_sync();

        let status = handle.save(&mut draft).await.unwrap();

        let SaveStatus::AppliedUncertain(reason) = status else {
            panic!("post-rename sync failure was reported as cleanly applied")
        };
        assert!(reason.contains("published but directory synchronization failed"));
        assert_ne!(std::fs::read(&path).unwrap(), original);
        assert!(
            String::from_utf8(std::fs::read(&path).unwrap())
                .unwrap()
                .contains("post-rename-model")
        );
        let held = handle.current().err().unwrap();
        assert!(held.to_string().contains("restart the session"), "{held:#}");
        let reopened = Handle::open(handle.args()).unwrap();
        assert_eq!(
            reopened.role(Role::Creator, None).unwrap().model.as_deref(),
            Some("post-rename-model")
        );
    }

    #[tokio::test]
    async fn same_draft_cannot_clear_prior_post_rename_uncertainty_or_replay_gate() {
        let root = tempfile::tempdir().unwrap();
        let (handle, path) = fixture(root.path());
        let mut draft = handle.draft().unwrap();
        handle.save(&mut draft).await.unwrap();
        let normalized = std::fs::read(&path).unwrap();
        let captured = Arc::new(Mutex::new(Vec::new()));
        let running = start_control(
            &handle,
            root.path(),
            config_change_plan(Arc::new(AllowConfigChange(captured.clone()))),
        )
        .await;
        handle.fail_next_directory_sync();

        let first = handle.save(&mut draft).await.unwrap();
        assert!(matches!(first, SaveStatus::AppliedUncertain(_)));
        assert_eq!(std::fs::read(&path).unwrap(), normalized);
        assert_eq!(captured.lock().unwrap().len(), 1);

        let retry = handle.save(&mut draft).await.unwrap_err();
        assert!(
            retry.to_string().contains("restart the session"),
            "{retry:#}"
        );
        assert_eq!(captured.lock().unwrap().len(), 1, "retry reran the gate");
        assert_eq!(std::fs::read(&path).unwrap(), normalized);
        assert!(
            handle.current().is_err(),
            "retry cleared publication uncertainty"
        );
        running.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn already_inspected_save_rechecks_prior_uncertainty_inside_publication_transaction() {
        let root = tempfile::tempdir().unwrap();
        let (handle, _) = fixture(root.path());
        let mut normalized = handle.draft().unwrap();
        handle.save(&mut normalized).await.unwrap();
        let captured = Arc::new(Mutex::new(Vec::new()));
        let running = start_control(
            &handle,
            root.path(),
            config_change_plan(Arc::new(AllowConfigChange(captured.clone()))),
        )
        .await;
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        handle.pause_next_publication(entered.clone(), release.clone());
        let mut queued = handle.draft().unwrap();
        configure(&mut queued, "a", "queued-before-uncertainty-model");
        let queued_handle = handle.clone();
        let queued_save = tokio::spawn(async move { queued_handle.save(&mut queued).await });
        entered.notified().await;

        let mut uncertain = handle.draft().unwrap();
        configure(&mut uncertain, "a", "uncertain-winning-model");
        handle.fail_next_directory_sync();
        assert!(matches!(
            handle.save(&mut uncertain).await.unwrap(),
            SaveStatus::AppliedUncertain(_)
        ));
        release.notify_one();

        let error = queued_save.await.unwrap().unwrap_err();
        assert!(
            error.to_string().contains("restart the session"),
            "{error:#}"
        );
        assert_eq!(
            captured.lock().unwrap().len(),
            2,
            "queued pass was rerun after uncertainty"
        );
        assert!(handle.current().is_err());
        running.shutdown().await.unwrap();
    }

    #[derive(Clone, Copy)]
    enum ReceiptFault {
        BeforeRename,
        DirectorySync,
    }

    async fn assert_receipt_failure_recovery(
        fault: ReceiptFault,
        expected_reopened_publication: Option<crate::plugins::receipts::ConfigPublication>,
        abort_waiter: bool,
    ) {
        let root = tempfile::tempdir().unwrap();
        let (handle, path) = fixture(root.path());
        let original = std::fs::read(&path).unwrap();
        let effects = Arc::new(Mutex::new(Vec::new()));
        let running = start_control(
            &handle,
            root.path(),
            config_change_plan(Arc::new(AllowConfigChange(effects.clone()))),
        )
        .await;
        let directory = running.runtime.directory().unwrap();
        let connection = running.connection.clone();
        let limits = running.limits.clone();
        let metadata = std::fs::metadata(root.path()).unwrap();
        let mutation_boundary = running
            .runtime
            .mutation_boundary((metadata.dev(), metadata.ino()))
            .unwrap();
        match fault {
            ReceiptFault::BeforeRename => running
                .runtime
                .fail_config_change_receipt_before_rename_when(published_config_change),
            ReceiptFault::DirectorySync => running
                .runtime
                .fail_config_change_receipt_sync_when(published_config_change),
        }
        let mut draft = handle.draft().unwrap();
        configure(&mut draft, "a", "receipt-failure-model");

        if abort_waiter {
            let (file_published, release_publication) = handle.pause_next_after_file_publication();
            let saving_handle = handle.clone();
            let save = tokio::spawn(async move { saving_handle.save(&mut draft).await });
            file_published
                .await
                .expect("Settings publication reached the receipt boundary");
            assert!(
                tokio::time::timeout(Duration::from_millis(50), mutation_boundary.lock())
                    .await
                    .is_err(),
                "blocking transaction released the workspace boundary before receipt finalization"
            );
            save.abort();
            assert!(save.await.unwrap_err().is_cancelled());
            assert!(
                tokio::time::timeout(Duration::from_millis(50), mutation_boundary.lock())
                    .await
                    .is_err(),
                "aborting the waiter released the blocking transaction's workspace boundary"
            );
            release_publication.send(()).unwrap();
            drop(
                tokio::time::timeout(Duration::from_secs(5), mutation_boundary.lock())
                    .await
                    .expect("blocking publication did not finish after release"),
            );
        } else {
            let status = handle.save(&mut draft).await.unwrap();
            let SaveStatus::AppliedUncertain(reason) = status else {
                panic!("receipt sync failure was reported as cleanly applied")
            };
            assert!(reason.contains("policy receipt could not be recorded"));
            assert!(reason.contains("restart the session"));
        }
        assert_ne!(std::fs::read(&path).unwrap(), original);
        assert!(
            String::from_utf8(std::fs::read(&path).unwrap())
                .unwrap()
                .contains("receipt-failure-model")
        );
        let in_memory = running.runtime.record().unwrap();
        let (operation, receipt) = config_change_receipt(&in_memory);
        let operation_id = operation.id;
        assert_eq!(
            receipt.publication,
            Some(crate::plugins::receipts::ConfigPublication::Published)
        );
        assert_eq!(effects.lock().unwrap().len(), 1);
        let held = handle.current().err().unwrap();
        assert!(held.to_string().contains("restart the session"), "{held:#}");
        assert!(running.shutdown().await.is_err());
        handle.require_control(true);

        let reopened_settings = Handle::open(handle.args()).unwrap();
        assert_eq!(
            reopened_settings
                .role(Role::Creator, None)
                .unwrap()
                .model
                .as_deref(),
            Some("receipt-failure-model")
        );
        let (reopened_runtime, resumed) = SharedRuntime::open_with_session_hooks(
            root.path(),
            &connection,
            Some(&directory),
            &CaptureScope::default(),
            Some(&limits),
        )
        .unwrap();
        assert!(resumed);
        let reopened = reopened_runtime.record().unwrap();
        let (reopened_operation, reopened_receipt) = config_change_receipt(&reopened);
        assert_eq!(reopened_operation.id, operation_id);
        assert!(reopened_operation.complete && reopened_receipt.settled);
        assert_eq!(reopened_receipt.publication, expected_reopened_publication);
        assert_eq!(
            effects.lock().unwrap().len(),
            1,
            "reopen replayed the handler effect"
        );
    }

    #[tokio::test]
    async fn receipt_pre_rename_failure_keeps_old_durable_gate_outcome_after_settings_publish() {
        assert_receipt_failure_recovery(ReceiptFault::BeforeRename, None, false).await;
    }

    #[tokio::test]
    async fn receipt_directory_sync_failure_retains_applied_operation_without_replay() {
        assert_receipt_failure_recovery(
            ReceiptFault::DirectorySync,
            Some(crate::plugins::receipts::ConfigPublication::Published),
            false,
        )
        .await;
    }

    #[tokio::test]
    async fn aborted_waiter_cannot_release_workspace_or_hide_receipt_failure() {
        assert_receipt_failure_recovery(ReceiptFault::BeforeRename, None, true).await;
    }

    #[test]
    fn migration_keeps_legacy_oracle_override() {
        let root = tempfile::tempdir().unwrap();
        let (handle, _) = fixture(root.path());
        let mut draft = handle.draft().unwrap();
        draft.config.oracle = Some(crate::config::OracleAssignment {
            connection: "b".into(),
            model: Some("legacy-oracle".into()),
            effort: Some("high".into()),
        });
        let mut settings = Assignments::from_config(&draft.config);
        assert_eq!(
            settings
                .overrides
                .get(&Role::Oracle)
                .unwrap()
                .model
                .as_deref(),
            Some("legacy-oracle")
        );
        settings.creator.as_mut().unwrap().model = Some("changed-creator".into());
        assert_eq!(
            settings
                .resolve(&draft.config, Role::Oracle)
                .unwrap()
                .model
                .as_deref(),
            Some("legacy-oracle")
        );
    }
}
