//! Exact observer leases extend execution, never admission or allocation.
use super::{Record, RuntimeReference, SharedRuntime, delegation};
use crate::plugins::{
    dispatch::HookInvocation,
    hook_types::HookEvent,
    observer::{Delivery, ObserverConfig, ObserverReceipt, Status},
    receipts::*,
};
use anyhow::{Context, Result, ensure};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::{Notify, OwnedSemaphorePermit, Semaphore};

mod delivery;
mod session;
#[cfg(test)]
pub(super) mod session_tests;
pub(crate) use delivery::ContextDelivery;
pub(crate) use session::NativeContextDelivery;

type Key = (u64, String, u32);
struct Job {
    cancelled: Arc<AtomicBool>,
    capacity: Weak<OwnedSemaphorePermit>,
    phase: String,
    writer: bool,
    handle: Option<tokio::task::JoinHandle<()>>,
    identity: Arc<session::Identity>,
}
pub(super) struct LiveObservers {
    slots: Arc<Semaphore>,
    admission_closed: bool,
    tasks_closed: bool,
    writers_blocked: std::collections::BTreeSet<String>,
    phases_closed: std::collections::BTreeSet<String>,
    jobs: BTreeMap<Key, Job>,
    pub(super) changed: Arc<Notify>,
}
impl Default for LiveObservers {
    fn default() -> Self {
        Self {
            admission_closed: false,
            tasks_closed: false,
            writers_blocked: Default::default(),
            phases_closed: Default::default(),
            slots: Arc::new(Semaphore::new(8)),
            jobs: BTreeMap::new(),
            changed: Arc::new(Notify::new()),
        }
    }
}
impl Drop for LiveObservers {
    fn drop(&mut self) {
        for job in self.jobs.values_mut() {
            job.cancelled.store(true, Ordering::Release);
            if let Some(handle) = &job.handle {
                handle.abort();
            }
        }
    }
}

/// All clones retain capacity until even blocking process cleanup has finished.
pub(crate) struct ObserverLease {
    runtime: RuntimeReference,
    key: Key,
    owner: String,
    deadline: Instant,
    deadline_ms: u64,
    cancelled: Arc<AtomicBool>,
    _capacity: Arc<OwnedSemaphorePermit>,
    pub(crate) transferred: AtomicBool,
    pub(crate) changed: Notify,
    config: ObserverConfig,
    identity: Arc<session::Identity>,
}
impl Drop for ObserverLease {
    fn drop(&mut self) {
        // The last lease also follows any blocking command cleanup. Completed
        // native context alone may retain its small capability until delivery.
        if let Ok(runtime) = self.runtime.upgrade()
            && let Ok(mut runtime) = runtime.0.lock()
            && Arc::strong_count(&self._capacity) == 1
            && exact(&runtime.record, &self.key).ok().is_none_or(|hook| {
                hook.observer.as_ref().is_none_or(|observer| {
                    matches!(observer.delivery, Delivery::Withheld | Delivery::Delivered)
                })
            })
            && runtime
                .observers
                .jobs
                .get(&self.key)
                .is_some_and(|job| Arc::ptr_eq(&job.cancelled, &self.cancelled))
        {
            runtime.observers.jobs.remove(&self.key);
        }
    }
}

fn fingerprint(record: &Record, role: &str) -> Result<String> {
    ensure!(!record.recovery_pending, "observer owner is held");
    ensure!(
        record.task.as_ref().is_none_or(|t| t.accepted.is_none()),
        "observer owner accepted"
    );
    delegation::ensure_agent_active(record, role)?;
    let allocation = record
        .allocation
        .as_ref()
        .context("observer requires an owning allocation")?;
    ensure!(allocation.remaining_ms()? > 0, "observer allowance expired");
    let child = delegation::agent_id(role).and_then(|id| record.agents.iter().find(|a| a.id == id));
    crate::plugins::admission::digest(&(
        &record.workspace,
        &record.identity,
        record.task.as_ref().map(|t| t.id),
        record.task_allocation_epoch,
        allocation.started_ms,
        allocation.deadline_ms,
        &allocation.limits,
        role,
        child.map(|a| {
            (
                &a.id,
                &a.parent_task,
                &a.identity,
                &a.request,
                &a.worktree,
                &a.planned_root,
            )
        }),
    ))
}
fn hooks(record: &Record) -> impl Iterator<Item = &HookReceipt> {
    record
        .operations
        .iter()
        .flat_map(super::Operation::all_plugin_hooks)
}
fn hooks_mut(record: &mut Record) -> impl Iterator<Item = &mut HookReceipt> {
    record
        .operations
        .iter_mut()
        .flat_map(super::Operation::all_plugin_hooks_mut)
}

fn key(hook: &HookReceipt) -> Key {
    (
        hook.inspected.operation,
        hook.inspected.event.clone(),
        hook.invocation,
    )
}
fn exact<'a>(record: &'a Record, target: &Key) -> Result<&'a HookReceipt> {
    hooks(record)
        .find(|h| key(h) == *target)
        .context("observer reservation missing")
}
fn hold_interrupted_owner(record: &mut Record, phase: &str) {
    if let Some(id) = delegation::agent_id(phase) {
        if let Some(agent) = record.agents.iter_mut().find(|agent| agent.id == id) {
            if agent.status != crate::subagents::state::AgentStatus::Cancelled {
                agent.status = crate::subagents::state::AgentStatus::Uncertain;
                agent.outcome="Observer stopped with unknown effects; inspect the original assignment before continuing.".into();
                if let Some(stage) = &mut agent.orchestration {
                    stage.stage = crate::subagents::state::OrchestrationStage::Held;
                    stage.reason = agent.outcome.clone();
                }
            }
        } else {
            record.recovery_pending = true;
        }
    } else {
        record.recovery_pending = true;
    }
}
pub(crate) fn interrupt_restored(record: &mut Record) {
    let mut interrupted = std::collections::BTreeSet::new();
    for hook in hooks_mut(record) {
        if let Some(observer) = &mut hook.observer {
            if observer.status == Status::Running {
                observer.status = Status::Interrupted;
                interrupted.insert(hook.inspected.role.clone());
            }
            if observer.delivery == Delivery::Reserved {
                observer.delivery = Delivery::Withheld;
            }
            if matches!(hook.inspected.event.as_str(), "SessionStart" | "SessionEnd") {
                observer.delivery = Delivery::Withheld;
            }
        }
    }
    for phase in interrupted {
        hold_interrupted_owner(record, &phase);
    }
}
impl ObserverLease {
    pub(crate) fn abandoned(&self) {
        if self
            .settle(RawOutcome::Failure {
                reason: "observer owner interrupted before durable completion".into(),
            })
            .is_err()
            && let Ok(runtime) = self.runtime.upgrade()
        {
            let _ = runtime.hold();
        }
    }
    pub(crate) fn revoke(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
    pub(crate) fn validate(&self) -> Result<Duration> {
        ensure!(
            !self.cancelled.load(Ordering::Acquire) && Instant::now() < self.deadline,
            "observer cancelled or expired"
        );
        let runtime = self.runtime.upgrade()?;
        let guard = runtime
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("observer runtime lock failed"))?;
        ensure!(!guard.failed, "observer persistence failed");
        let hook = exact(&guard.record, &self.key)?;
        if let Some(observer) = &hook.observer {
            self.identity.validate_transfer(hook)?;
            ensure!(
                observer.status == Status::Running
                    && matches!(observer.delivery, Delivery::Pending | Delivery::Withheld)
                    && observer.delivery_operation.is_none()
                    && super::super::allocation::now_ms()? < observer.deadline_ms,
                "observer transfer settled, changed or expired"
            );
        } else {
            ensure!(
                !self.transferred.load(Ordering::Acquire),
                "observer durable transfer disappeared"
            );
        }
        ensure!(
            self.identity.validate(&guard.record, hook)? == self.owner && hook.outcome.is_none(),
            "observer owner changed or already settled"
        );
        ensure!(
            guard
                .observers
                .jobs
                .get(&self.key)
                .is_some_and(|j| Arc::ptr_eq(&j.cancelled, &self.cancelled)),
            "observer live identity lost"
        );
        self.identity.validate_view()?;
        Ok(self.deadline.saturating_duration_since(Instant::now()))
    }
    pub(crate) fn transfer(&self, marker: Option<serde_json::Value>) -> Result<()> {
        self.validate()?;
        if self.transferred.load(Ordering::Acquire) {
            return Ok(());
        }
        let requested = marker
            .as_ref()
            .and_then(|v| v.get("asyncTimeout"))
            .map(|v| {
                v.as_f64()
                    .filter(|n| n.is_finite() && *n > 0.0)
                    .context("invalid async timeout")
            })
            .transpose()?;
        let runtime = self.runtime.upgrade()?;
        runtime.update(|target| {
            let mut record = target.clone();
            ensure!(
                hooks(&record)
                    .filter(
                        |hook| hook.observer.as_ref().is_some_and(|observer| matches!(
                            observer.delivery,
                            Delivery::Pending | Delivery::Reserved
                        ))
                    )
                    .count()
                    < 64,
                "observer pending delivery capacity exhausted (64)"
            );
            let hook = exact(&record, &self.key)?;
            ensure!(
                !hook.required_gate && hook.observer.is_none() && hook.outcome.is_none(),
                "required gate or duplicate observer transfer"
            );
            ensure!(
                self.identity.validate(&record, hook)? == self.owner
                    && !self.cancelled.load(Ordering::Acquire),
                "observer authority changed before transfer"
            );
            let now = super::super::allocation::now_ms()?;
            let deadline_ms = requested
                .map(|n| now.saturating_add(n.min(self.config.timeout_ms as f64).ceil() as u64))
                .unwrap_or(self.deadline_ms)
                .min(self.deadline_ms);
            let task = self
                .identity
                .native
                .is_none()
                .then(|| record.task.as_ref().map(|t| t.id))
                .flatten();
            let allocation_started_ms = self.identity.allocation(&record)?.started_ms;
            let hook = hooks_mut(&mut record)
                .find(|h| key(h) == self.key)
                .expect("validated");
            let transferred = ObserverReceipt {
                owner: self.owner.clone(),
                task,
                allocation_started_ms,
                deadline_ms,
                status: Status::Running,
                delivery: if matches!(hook.inspected.event.as_str(), "StopFailure" | "SessionEnd") {
                    Delivery::Withheld
                } else {
                    Delivery::Pending
                },
                rewake: self.config.rewake && hook.inspected.event != "StopFailure",
                launch_marker: marker.clone(),
                delivery_operation: None,
            };
            // Capture intent under the same lock that publishes the record.
            // Validation also requires the matching durable receipt and a clear
            // persistence latch; this local value alone grants no execution.
            self.identity
                .transfer
                .set(transferred.clone())
                .map_err(|_| anyhow::anyhow!("observer already owns a transfer identity"))?;
            hook.observer = Some(transferred);
            *target = record;
            Ok(())
        })?;
        #[cfg(test)]
        session_tests::pause_transfer_publication();
        self.transferred.store(true, Ordering::Release);
        self.validate()?;
        self.changed.notify_one();
        Ok(())
    }
    pub(crate) fn settle(&self, mut outcome: RawOutcome) -> Result<()> {
        if !outcome.within_retention_bound() {
            outcome = RawOutcome::Failure {
                reason: "observer output exceeded retention bound".into(),
            };
        }
        let runtime = self.runtime.upgrade()?;
        runtime.update(|target| {
            let mut record = target.clone();
            let previous = exact(&record, &self.key)?;
            // A late original result may settle after authority ends, but it
            // cannot settle a replacement admission or one-shot attempt.
            self.identity.validate_admission(&record, previous)?;
            self.identity.validate_transfer(previous)?;
            ensure!(
                previous.outcome.is_none()
                    && previous
                        .observer
                        .as_ref()
                        .is_some_and(|o| o.status == Status::Running
                            && matches!(o.delivery, Delivery::Pending | Delivery::Withheld)
                            && o.delivery_operation.is_none()),
                "observer completion mismatches exact transfer"
            );
            let eligible = !self.cancelled.load(Ordering::Acquire)
                && self
                    .identity
                    .validate(&record, previous)
                    .is_ok_and(|f| f == self.owner)
                && self.identity.validate_view().is_ok();
            let mut hook = previous.clone();
            hook.uncertain_effects = matches!(
                outcome,
                RawOutcome::Failure { .. } | RawOutcome::CommandFailure { .. }
            );
            hook.outcome = Some(outcome.clone());
            let event = HookEvent::try_from(self.key.1.as_str())?;
            let profile = crate::plugins::profile::CompatibilityProfile::embedded()?;
            let context = crate::plugins::results::ResultContext {
                role: crate::plugins::results::ResultRole::Observer,
                asynchronous: true,
                ..Default::default()
            };
            let decoded = outcome.decode_for(&profile, &hook.declaration, event, &context);
            let observer = hook.observer.as_mut().expect("validated");
            observer.status = if hook.uncertain_effects {
                Status::Interrupted
            } else {
                Status::Completed
            };
            observer.rewake &= matches!(
                &outcome,
                RawOutcome::Command {
                    exit_code: Some(2),
                    ..
                }
            ) && hook.declaration.dialect
                == crate::plugins::hook_types::HookDialect::Claude;
            if !eligible
                || hook.inspected.event == "SessionEnd"
                || (!context_valid(&decoded) && !observer.rewake)
            {
                observer.delivery = Delivery::Withheld;
            }
            hook.pending_proposals = decoded
                .effects
                .iter()
                .enumerate()
                .filter(|(_, e)| {
                    matches!(
                        e,
                        crate::plugins::results::ProposedEffect::AdditionalContext(_)
                            | crate::plugins::results::ProposedEffect::Warning(_)
                            | crate::plugins::results::ProposedEffect::TransientNotice(_)
                    )
                })
                .map(|(index, e)| PendingProposal {
                    index,
                    kind: e.into(),
                })
                .collect();
            if hook.pending_proposals.is_empty() && !hook.observer.as_ref().expect("present").rewake
            {
                hook.observer.as_mut().expect("present").delivery = Delivery::Withheld;
            }
            let succeeded = crate::plugins::once::succeeded(&hook, &decoded);
            if let Some(attempt) = &mut hook.once {
                attempt.state = if hook.uncertain_effects {
                    crate::plugins::once::OnceState::Unknown
                } else if succeeded {
                    crate::plugins::once::OnceState::Succeeded
                } else {
                    crate::plugins::once::OnceState::Failed
                };
            }
            ensure!(
                serde_json::to_vec(&hook)?.len() <= 256 * 1024,
                "observer receipt exceeds retention bound"
            );
            if hook.uncertain_effects {
                hold_interrupted_owner(&mut record, &hook.inspected.role);
            }
            *hooks_mut(&mut record)
                .find(|h| key(h) == self.key)
                .expect("validated") = hook;
            *target = record;
            Ok(())
        })?;
        runtime.observer_notification()?.notify_one();
        Ok(())
    }
}

impl SharedRuntime {
    pub(crate) fn observer_notification(&self) -> Result<Arc<Notify>> {
        Ok(self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("observer lock failed"))?
            .observers
            .changed
            .clone())
    }
    pub(crate) fn admit_observer(
        &self,
        invocation: &HookInvocation,
        config: ObserverConfig,
        writer: bool,
    ) -> Result<Arc<ObserverLease>> {
        self.plugin_runner_owner(invocation.key.operation, invocation.events.plugin_event())?;
        let mut runtime = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("observer lock failed"))?;
        ensure!(
            !runtime.observers.admission_closed
                && !runtime
                    .observers
                    .phases_closed
                    .iter()
                    .any(|p| invocation.key.role == *p
                        || invocation.key.role.starts_with(&format!("{p}:")))
                && (!writer
                    || !runtime
                        .observers
                        .writers_blocked
                        .iter()
                        .any(|p| invocation.key.role == *p
                            || p == "worker"
                                && invocation.key.event == "SessionStart"
                                && invocation
                                    .lifecycle
                                    .as_ref()
                                    .is_some_and(|facts| facts.native_session.is_some())
                            || invocation.key.role.starts_with(&format!("{p}:")))),
            "observer admission is closed or writers are quiescing"
        );
        ensure!(
            !runtime.failed && config.timeout_ms > 0,
            "observer deadline or persistence unavailable"
        );
        let target = (
            invocation.key.operation,
            invocation.key.event.clone(),
            invocation.invocation,
        );
        let hook = exact(&runtime.record, &target)?;
        ensure!(
            !hook.required_gate
                && hook.inspected == invocation.key
                && hook.required_gate == invocation.required_gate
                && hook.declaration == invocation.declaration
                && hook.outcome.is_none(),
            "observer lacks exact non-gate reservation"
        );
        let identity = Arc::new(session::Identity::capture(&runtime.record, invocation)?);
        ensure!(
            identity.native.is_none() || config.declared && !config.rewake,
            "native session requires declared async without source-only rewake"
        );
        ensure!(
            identity.native.is_some() || !runtime.observers.tasks_closed,
            "task observer admission is closed"
        );
        let owner = identity.validate(&runtime.record, hook)?;
        ensure!(
            hooks(&runtime.record)
                .filter(|h| h
                    .observer
                    .as_ref()
                    .is_some_and(|o| matches!(o.delivery, Delivery::Pending | Delivery::Reserved)))
                .count()
                < 64,
            "observer pending delivery capacity exhausted (64)"
        );
        let remaining = identity
            .allocation(&runtime.record)?
            .remaining_ms()?
            .min(config.timeout_ms);
        ensure!(remaining > 0, "observer expired");
        let pending = hooks(&runtime.record)
            .filter(|h| {
                h.observer
                    .as_ref()
                    .is_some_and(|o| matches!(o.delivery, Delivery::Pending | Delivery::Reserved))
            })
            .map(key)
            .collect::<std::collections::BTreeSet<_>>();
        runtime.observers.jobs.retain(|key, j| {
            j.capacity.strong_count() > 0
                || j.handle.as_ref().is_some_and(|h| !h.is_finished())
                || (j.identity.native.is_some() && pending.contains(key))
        });
        ensure!(
            !runtime.observers.jobs.contains_key(&target),
            "observer already owns invocation"
        );
        let capacity = Arc::new(
            runtime
                .observers
                .slots
                .clone()
                .try_acquire_owned()
                .context("observer capacity exhausted (8)")?,
        );
        let cancelled = Arc::new(AtomicBool::new(false));
        runtime.observers.jobs.insert(
            target.clone(),
            Job {
                cancelled: cancelled.clone(),
                capacity: Arc::downgrade(&capacity),
                phase: invocation.key.role.clone(),
                writer,
                handle: None,
                identity: identity.clone(),
            },
        );
        Ok(Arc::new(ObserverLease {
            runtime: self.downgrade(),
            key: target,
            owner,
            deadline: Instant::now() + Duration::from_millis(remaining),
            deadline_ms: super::super::allocation::now_ms()?.saturating_add(remaining),
            cancelled,
            _capacity: capacity,
            transferred: AtomicBool::new(false),
            changed: Notify::new(),
            config,
            identity,
        }))
    }
    pub(crate) fn retain_observer_job(
        &self,
        lease: &ObserverLease,
        handle: tokio::task::JoinHandle<()>,
    ) -> Result<()> {
        match self.0.lock() {
            Ok(mut runtime) => {
                if let Some(job) = runtime.observers.jobs.get_mut(&lease.key) {
                    job.handle = Some(handle);
                    return Ok(());
                }
                handle.abort();
                anyhow::bail!("observer job missing");
            }
            Err(_) => {
                handle.abort();
                anyhow::bail!("observer lock failed");
            }
        }
    }
    pub(crate) fn prune_observer_capabilities(&self) -> Result<()> {
        let mut runtime = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("observer lock failed"))?;
        let pending = hooks(&runtime.record)
            .filter(|hook| {
                hook.observer.as_ref().is_some_and(|observer| {
                    matches!(observer.delivery, Delivery::Pending | Delivery::Reserved)
                })
            })
            .map(key)
            .collect::<std::collections::BTreeSet<_>>();
        runtime.observers.jobs.retain(|key, job| {
            job.capacity.strong_count() > 0
                || job
                    .handle
                    .as_ref()
                    .is_some_and(|handle| !handle.is_finished())
                || job.identity.native.is_some() && pending.contains(key)
        });
        Ok(())
    }
    pub(crate) async fn stop_observers(
        &self,
        phase: Option<&str>,
        writers_only: bool,
    ) -> Result<()> {
        self.stop_selected_observers(phase, writers_only, false)
            .await
    }
    pub(crate) async fn stop_task_observers(&self) -> Result<()> {
        self.stop_selected_observers(None, false, true).await
    }
    async fn stop_selected_observers(
        &self,
        phase: Option<&str>,
        writers_only: bool,
        tasks_only: bool,
    ) -> Result<()> {
        let capacities = {
            let mut runtime = self
                .0
                .lock()
                .map_err(|_| anyhow::anyhow!("observer lock failed"))?;
            if tasks_only {
                runtime.observers.tasks_closed = true;
            } else if let Some(phase) = phase {
                runtime.observers.phases_closed.insert(phase.into());
            } else {
                runtime.observers.admission_closed = true;
            }
            runtime
                .observers
                .jobs
                .values_mut()
                .filter(|j| {
                    (!tasks_only || j.identity.native.is_none())
                        && phase
                            .is_none_or(|p| j.phase == p || j.phase.starts_with(&format!("{p}:")))
                        && (!writers_only || j.writer)
                })
                .map(|job| {
                    job.cancelled.store(true, Ordering::Release);
                    job.capacity.clone()
                })
                .collect::<Vec<_>>()
        };
        self.update(|record| {
            for hook in hooks_mut(record) {
                if (!tasks_only || hook.inspected.role != "native-session")
                    && phase.is_none_or(|p| {
                        hook.inspected.role == p
                            || hook.inspected.role.starts_with(&format!("{p}:"))
                    })
                    && (!writers_only
                        || hook
                            .observer
                            .as_ref()
                            .is_some_and(|o| o.status == Status::Running))
                    && let Some(observer) = &mut hook.observer
                {
                    observer.delivery = Delivery::Withheld;
                }
            }
            Ok(())
        })?;
        tokio::time::timeout(Duration::from_secs(3), async {
            while capacities.iter().any(|p| p.strong_count() > 0) {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .context("observer cleanup still owns resources; work remains held")?;
        Ok(())
    }
}

fn context_valid(decoded: &crate::plugins::results::DecodedResult) -> bool {
    !decoded.diagnostics.iter().any(|d| {
        d.kind == crate::plugins::results::DiagnosticKind::Failure
            && !(decoded.dialect == crate::plugins::hook_types::HookDialect::Claude
                && d.detail.path == "/command/exit")
    })
}
impl SharedRuntime {
    pub(crate) fn reopen_observer_admission(&self, phase: &str) -> Result<()> {
        let mut runtime = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("observer lock failed"))?;
        runtime.observers.admission_closed = false;
        runtime.observers.tasks_closed = false;
        runtime.observers.writers_blocked.remove(phase);
        runtime.observers.phases_closed.remove(phase);
        Ok(())
    }
    pub(crate) fn observer_phase_owner(&self, phase: &str) -> Result<String> {
        fingerprint(&self.record()?, phase)
    }
    /// Foreground completion does not end already-admitted child observer work.
    pub(crate) async fn drain_observers(&self, phase: &str, owner: &str) -> Result<()> {
        tokio::time::timeout(self.remaining()?, async {
            loop {
                let finished = {
                    let runtime = self
                        .0
                        .lock()
                        .map_err(|_| anyhow::anyhow!("observer lock failed"))?;
                    ensure!(
                        !runtime.failed && fingerprint(&runtime.record, phase)? == owner,
                        "observer phase owner changed before completion"
                    );
                    runtime
                        .observers
                        .jobs
                        .values()
                        .filter(|job| job.phase == phase)
                        .all(|job| job.capacity.strong_count() == 0)
                };
                if finished {
                    return Ok::<_, anyhow::Error>(());
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .context("original child observer allowance exhausted")?
    }
    pub(crate) async fn quiesce_observer_writers(&self, phase: &str) -> Result<()> {
        let capacities = {
            let mut runtime = self
                .0
                .lock()
                .map_err(|_| anyhow::anyhow!("observer lock failed"))?;
            runtime.observers.writers_blocked.insert(phase.into());
            runtime
                .observers
                .jobs
                .values()
                .filter(|j| {
                    j.writer
                        && (j.phase == phase
                            || j.phase.starts_with(&format!("{phase}:"))
                            || phase == "worker" && j.identity.native.is_some())
                })
                .map(|j| j.capacity.clone())
                .collect::<Vec<_>>()
        };
        tokio::time::timeout(self.remaining()?.min(Duration::from_secs(30)), async {
            while capacities.iter().any(|p| p.strong_count() > 0) {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .context("observer writer cleanup still pending; no evidence snapshot admitted")?;
        let record = self.record()?;
        ensure!(
            !record.recovery_pending,
            "observer writer left uncertain effects; reconcile before snapshot"
        );
        delegation::ensure_agent_active(&record, phase)?;
        Ok(())
    }
}

impl LiveObservers {
    pub(super) fn stopped(&self) -> bool {
        self.jobs.values().all(|j| j.capacity.strong_count() == 0)
    }
}
