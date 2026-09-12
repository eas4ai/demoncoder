//! Native observers retain host authority after their startup occurrence settles.
use super::super::{BudgetRef, budget_accounting, plugin_session};
use super::*;
use crate::plugins::{
    gate_snapshot::GateWorkspace,
    hook_types::{HandlerKind, HookDialect},
};

pub(super) struct NativeOwner {
    pub(super) lifetime: u64,
    authority: Arc<plugin_session::NativeSessionAuthority>,
    view: GateWorkspace,
}
pub(super) struct Identity {
    key: AdmissionKey,
    declaration: DeclarationIdentity,
    endpoint: Option<String>,
    class: HandlerClass,
    required_gate: bool,
    budget: BudgetRef,
    source: Option<crate::plugins::once::CapturedSource>,
    once: Option<crate::plugins::once::Activation>,
    /// Original transfer intent; execution also requires the durable receipt.
    pub(super) transfer: std::sync::OnceLock<ObserverReceipt>,
    pub(super) native: Option<NativeOwner>,
}
impl Identity {
    pub(super) fn capture(record: &Record, invocation: &HookInvocation) -> Result<Self> {
        let budget = budget_accounting::inherited(record, invocation.key.operation)?;
        let native = invocation
            .lifecycle
            .as_ref()
            .and_then(|facts| facts.native_session);
        let native = native
            .map(|lifetime| -> Result<_> {
                ensure!(
                    invocation.declaration.dialect == HookDialect::Native
                        && invocation.declaration.runner == HandlerKind::Command
                        && matches!(budget, BudgetRef::SessionHooks { .. }),
                    "native asynchronous command requires its original explicit session allowance"
                );
                let view = GateWorkspace::open_with_credentials(
                    &invocation.host.workspace,
                    &invocation.host.credentials,
                )?;
                ensure!(
                    view.is_current(&invocation.snapshot, &AtomicBool::new(false))?,
                    "observer snapshot or credential authority changed before transfer"
                );
                ensure!(
                    invocation
                        .key
                        .inputs
                        .iter()
                        .any(|(_, revision)| revision == invocation.snapshot.revision()),
                    "observer snapshot differs from its admitted inputs"
                );
                let authority = plugin_session::lifetime(record, lifetime)?
                    .1
                    .authority
                    .clone()
                    .context("native host capability missing")?;
                Ok(NativeOwner {
                    lifetime,
                    authority,
                    view,
                })
            })
            .transpose()?;
        let original = exact(
            record,
            &(
                invocation.key.operation,
                invocation.key.event.clone(),
                invocation.invocation,
            ),
        )?;
        Ok(Self {
            key: invocation.key.clone(),
            declaration: invocation.declaration.clone(),
            endpoint: invocation.endpoint.clone(),
            class: invocation.class,
            required_gate: invocation.required_gate,
            budget,
            native,
            source: original.source.clone(),
            once: original
                .once
                .as_ref()
                .map(|attempt| attempt.activation.clone()),
            transfer: std::sync::OnceLock::new(),
        })
    }
    pub(super) fn allocation<'a>(
        &self,
        record: &'a Record,
    ) -> Result<&'a super::super::Allocation> {
        budget_accounting::active(record, &self.key.session, &self.budget)?
            .context("observer requires its original allowance")
    }
    pub(super) fn validate_admission(&self, record: &Record, hook: &HookReceipt) -> Result<()> {
        ensure!(
            hook.inspected == self.key
                && hook.declaration == self.declaration
                && hook.endpoint == self.endpoint
                && hook.class == self.class
                && hook.required_gate == self.required_gate
                && hook.source == self.source
                && hook.once.as_ref().map(|attempt| &attempt.activation) == self.once.as_ref()
                && budget_accounting::inherited(record, self.key.operation)? == self.budget,
            "observer exact admission or original budget changed"
        );
        Ok(())
    }
    pub(super) fn validate_transfer(&self, hook: &HookReceipt) -> Result<()> {
        let original = self
            .transfer
            .get()
            .context("observer has no owned durable transfer")?;
        let current = hook
            .observer
            .as_ref()
            .context("observer durable transfer missing")?;
        ensure!(
            current.owner == original.owner
                && current.task == original.task
                && current.allocation_started_ms == original.allocation_started_ms
                && current.deadline_ms == original.deadline_ms
                && current.launch_marker == original.launch_marker
                && (current.status != Status::Running || current.rewake == original.rewake),
            "observer durable transfer identity changed"
        );
        Ok(())
    }
    pub(super) fn validate(&self, record: &Record, hook: &HookReceipt) -> Result<String> {
        self.validate_admission(record, hook)?;
        if self.transfer.get().is_some() {
            self.validate_transfer(hook)?;
        } else if self.native.is_some() {
            // Before the first durable transfer, the original occurrence still
            // owns admission. Only transferred work outlives its settlement.
            super::super::plugin_non_tool::active(
                record,
                self.key.operation,
                HookEvent::try_from(self.key.event.as_str())?,
            )?;
        }
        let allocation = self.allocation(record)?;
        ensure!(
            allocation.remaining_ms()? > 0,
            "observer original allowance expired"
        );
        let Some(native) = &self.native else {
            return fingerprint(record, &self.key.role);
        };
        plugin_session::validate_live(record, native.lifetime)?;
        let (_, lifetime) = plugin_session::lifetime(record, native.lifetime)?;
        ensure!(
            lifetime
                .authority
                .as_ref()
                .is_some_and(|authority| Arc::ptr_eq(authority, &native.authority)),
            "observer native host capability replaced"
        );
        let operation = record
            .operations
            .iter()
            .find(|o| o.id == self.key.operation)
            .context("observer occurrence missing")?;
        let receipt = operation
            .non_tool_receipt()
            .context("observer native occurrence missing")?;
        ensure!(!operation.reconciled && receipt.hold.is_none()
            && operation.phase == "native-session" && self.key.role == "native-session"
            && operation.identity.as_ref() == Some(&record.identity)
            && receipt.facts.native_session == Some(native.lifetime)
            && receipt.facts.native_turn.is_none() && receipt.facts.callback.is_none()
            && receipt.facts.child_owner.is_none()
            && receipt.facts.session == self.key.session
            && receipt.facts.workspace == self.key.workspace
            && Some(&receipt.facts.subject) == self.key.lifecycle.as_ref()
            && self.key.source_operation == native.lifetime
            && budget_accounting::inherited(record, native.lifetime)? == self.budget
            && lifetime.plans.iter().any(|(event, plan)| event.as_str() == self.key.event && plan == &self.key.plan),
            "observer native lifetime or occurrence identity changed");
        ensure!(
            match &receipt.facts.subject.occurrence {
                NonToolOccurrence::SessionStart { source } =>
                    lifetime.end.is_none() && lifetime.source == *source,
                NonToolOccurrence::SessionEnd { reason } => lifetime.end == Some(*reason),
                _ => false,
            },
            "observer occurrence no longer owns execution"
        );
        crate::plugins::admission::digest(&(
            &record.workspace,
            &record.identity,
            native.lifetime,
            lifetime.workspace,
            &self.budget,
            allocation.started_ms,
            allocation.deadline_ms,
            &allocation.limits,
        ))
    }
    pub(super) fn validate_view(&self) -> Result<()> {
        if let Some(native) = &self.native {
            native.view.validate_authority(self.key.workspace)?;
        }
        Ok(())
    }
}

impl LiveObservers {
    pub(in crate::workflow::runtime) fn revoke_native(
        &mut self,
        lifetime: u64,
        startup_only: bool,
    ) -> Vec<Key> {
        self.jobs
            .iter_mut()
            .filter(|(key, job)| {
                job.identity
                    .native
                    .as_ref()
                    .is_some_and(|owner| owner.lifetime == lifetime)
                    && (!startup_only || key.1 == "SessionStart")
            })
            .map(|(key, job)| {
                job.cancelled.store(true, Ordering::Release);
                key.clone()
            })
            .collect()
    }
    pub(in crate::workflow::runtime) fn tasks_stopped(&self) -> bool {
        self.jobs
            .values()
            .filter(|job| job.identity.native.is_none())
            .all(|job| job.capacity.strong_count() == 0)
    }
}

pub(crate) struct NativeContextDelivery {
    references: BTreeMap<Key, Arc<Identity>>,
    target: u64,
    lifetime: u64,
    pub(crate) text: String,
}

fn model_target(record: &Record, target: u64, identity: &super::super::Identity) -> Result<()> {
    let operation = record
        .operations
        .iter()
        .find(|o| o.id == target)
        .context("native context request missing")?;
    ensure!(
        !record.recovery_pending
            && !operation.complete
            && !operation.reconciled
            && operation.phase == "worker"
            && operation.identity.as_ref() == Some(identity)
            && identity == &record.identity
            && matches!(
                operation.host_invocation,
                Some(super::super::HostInvocation::Model)
            ),
        "session context requires an existing native Creator request"
    );
    Ok(())
}

impl SharedRuntime {
    pub(crate) fn reserve_native_observer_context(
        &self,
        lifetime: u64,
        target: u64,
        identity: &super::super::Identity,
    ) -> Result<Option<NativeContextDelivery>> {
        let candidates = {
            let runtime = self
                .0
                .lock()
                .map_err(|_| anyhow::anyhow!("observer lock failed"))?;
            runtime
                .observers
                .jobs
                .iter()
                .filter(|(key, job)| {
                    key.1 == "SessionStart"
                        && job
                            .identity
                            .native
                            .as_ref()
                            .is_some_and(|owner| owner.lifetime == lifetime)
                })
                .map(|(key, job)| (key.clone(), job.identity.clone(), job.cancelled.clone()))
                .collect::<Vec<_>>()
        };
        let result = self.update(|record| {
            model_target(record, target, identity)?;
            let mut references = BTreeMap::new();
            let mut text = String::new();
            let mut withheld = Vec::new();
            for (key, owner, cancelled) in &candidates {
                let hook = exact(record, key)?;
                let Some(observer) = &hook.observer else {
                    continue;
                };
                if observer.status != Status::Completed || observer.delivery != Delivery::Pending {
                    continue;
                }
                if cancelled.load(Ordering::Acquire)
                    || owner.validate(record, hook).is_err()
                    || owner.validate_view().is_err()
                    || super::super::super::allocation::now_ms()? >= observer.deadline_ms
                {
                    withheld.push(key.clone());
                    continue;
                }
                let content = match super::delivery::text(hook) {
                    Ok(content) => format!("[Native session lifetime {lifetime}]{}", content),
                    Err(_) => {
                        withheld.push(key.clone());
                        continue;
                    }
                };
                if content.len() > 65536 {
                    withheld.push(key.clone());
                    continue;
                }
                if text.len().saturating_add(content.len()) > 65536 {
                    break;
                }
                text.push_str(&content);
                references.insert(key.clone(), owner.clone());
            }
            for hook in hooks_mut(record) {
                let key = key(hook);
                if withheld.contains(&key) {
                    hook.observer.as_mut().expect("selected").delivery = Delivery::Withheld;
                }
                if references.contains_key(&key) {
                    let observer = hook.observer.as_mut().expect("selected");
                    observer.delivery = Delivery::Reserved;
                    observer.delivery_operation = Some(target);
                }
            }
            Ok((!references.is_empty()).then_some(NativeContextDelivery {
                references,
                target,
                lifetime,
                text,
            }))
        })?;
        if let Some(delivery) = &result
            && !self.prepare_native_observer_context(delivery)?
        {
            return Ok(None);
        }
        Ok(result)
    }
    /// Before model state changes, optional stale context can be withheld alone.
    pub(crate) fn prepare_native_observer_context(
        &self,
        delivery: &NativeContextDelivery,
    ) -> Result<bool> {
        if self.validate_native_observer_context(delivery).is_ok() {
            return Ok(true);
        }
        self.update(|record| {
            model_target(record, delivery.target, &record.identity)?;
            for key in delivery.references.keys() {
                let observer = exact(record, key)?
                    .observer
                    .as_ref()
                    .context("native context transfer missing")?;
                ensure!(
                    observer.delivery == Delivery::Reserved
                        && observer.delivery_operation == Some(delivery.target),
                    "native context reservation changed before withholding"
                );
            }
            for hook in
                hooks_mut(record).filter(|hook| delivery.references.contains_key(&key(hook)))
            {
                hook.observer.as_mut().expect("validated").delivery = Delivery::Withheld;
            }
            Ok(())
        })?;
        self.prune_observer_capabilities()?;
        Ok(false)
    }
    pub(crate) fn validate_native_observer_context(
        &self,
        delivery: &NativeContextDelivery,
    ) -> Result<()> {
        let runtime = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("observer lock failed"))?;
        ensure!(!runtime.failed, "session persistence failed");
        model_target(&runtime.record, delivery.target, &runtime.record.identity)?;
        for key in delivery.references.keys() {
            let job = runtime
                .observers
                .jobs
                .get(key)
                .context("native context live capability missing")?;
            let hook = exact(&runtime.record, key)?;
            let observer = hook
                .observer
                .as_ref()
                .context("native context transfer missing")?;
            ensure!(
                !job.cancelled.load(Ordering::Acquire)
                    && job
                        .identity
                        .native
                        .as_ref()
                        .is_some_and(|owner| owner.lifetime == delivery.lifetime)
                    && job.identity.validate(&runtime.record, hook)? == observer.owner
                    && observer.status == Status::Completed
                    && observer.delivery == Delivery::Reserved
                    && observer.delivery_operation == Some(delivery.target)
                    && super::super::super::allocation::now_ms()? < observer.deadline_ms,
                "native context original authority expired or changed"
            );
            job.identity.validate_view()?;
        }
        Ok(())
    }
    /// A successful response is known delivery, even if authority expires afterward.
    /// This records the original reservation; it cannot authorize another request.
    pub(crate) fn complete_native_observer_context(
        &self,
        delivery: &NativeContextDelivery,
    ) -> Result<()> {
        let result = self.update(|record| {
            ensure!(
                record.operations.iter().any(|o| o.id == delivery.target
                    && o.complete
                    && matches!(o.host_invocation, Some(super::super::HostInvocation::Model))),
                "native context target has no completed response"
            );
            for (key, original) in &delivery.references {
                let hook = exact(record, key)?;
                original.validate_admission(record, hook)?;
                original.validate_transfer(hook)?;
                let observer = hook.observer.as_ref().expect("validated");
                ensure!(
                    matches!(observer.delivery, Delivery::Reserved | Delivery::Withheld)
                        && observer.delivery_operation == Some(delivery.target),
                    "native context delivery does not match reservation"
                );
            }
            for hook in
                hooks_mut(record).filter(|hook| delivery.references.contains_key(&key(hook)))
            {
                hook.observer.as_mut().expect("validated").delivery = Delivery::Delivered;
            }
            Ok(())
        });
        if let Err(error) = result {
            self.hold()?;
            return Err(error.context("native context delivery owner changed; recovery required"));
        }
        self.prune_observer_capabilities()
    }
}
impl SharedRuntime {
    pub(crate) fn cancel_native_observers(&self, lifetime: u64, startup_only: bool) -> Result<()> {
        let keys = {
            let mut runtime = self
                .0
                .lock()
                .map_err(|_| anyhow::anyhow!("observer lock failed"))?;
            runtime.observers.revoke_native(lifetime, startup_only)
        };
        self.update(|record| {
            for hook in hooks_mut(record).filter(|hook| keys.contains(&key(hook))) {
                if let Some(observer) = &mut hook.observer {
                    observer.delivery = Delivery::Withheld;
                }
            }
            Ok(())
        })?;
        self.prune_observer_capabilities()
    }
    pub(crate) async fn join_native_observers(
        &self,
        lifetime: u64,
        deadline: tokio::time::Instant,
    ) -> Result<()> {
        loop {
            let done = {
                let runtime = self
                    .0
                    .lock()
                    .map_err(|_| anyhow::anyhow!("observer lock failed"))?;
                runtime
                    .observers
                    .jobs
                    .values()
                    .filter(|job| {
                        job.identity
                            .native
                            .as_ref()
                            .is_some_and(|owner| owner.lifetime == lifetime)
                    })
                    .all(|job| job.capacity.strong_count() == 0)
            };
            if done {
                return self.prune_observer_capabilities();
            }
            ensure!(
                tokio::time::Instant::now() < deadline,
                "native observer observation deadline exhausted"
            );
            tokio::time::sleep_until(
                deadline.min(tokio::time::Instant::now() + Duration::from_millis(5)),
            )
            .await;
        }
    }
    pub(crate) fn transferred_native_observers(
        &self,
        lifetime: u64,
    ) -> Result<std::collections::BTreeSet<Key>> {
        let runtime = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("observer lock failed"))?;
        Ok(runtime
            .observers
            .jobs
            .iter()
            .filter_map(|(key, job)| {
                let hook = exact(&runtime.record, key).ok()?;
                (job.identity
                    .native
                    .as_ref()
                    .is_some_and(|owner| owner.lifetime == lifetime)
                    && !job.cancelled.load(Ordering::Acquire)
                    && hook.observer.is_some()
                    && job.identity.validate(&runtime.record, hook).is_ok()
                    && job.identity.validate_view().is_ok())
                .then(|| key.clone())
            })
            .collect())
    }
}
