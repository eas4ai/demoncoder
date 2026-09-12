//! Plugin admissions share the existing tool operation, store, owner and allowance.
use super::{Record, SharedRuntime, delegation};
use crate::plugins::{hook_types::HookEvent, receipts::*};
use anyhow::{Context, Result, ensure};
use std::sync::Arc;

mod transport;
pub(crate) use transport::{TransportInvocation, TransportView};

/// Ephemeral capability derived from the original task or session allowance.
#[derive(Clone)]
pub(crate) struct ServiceOwner {
    fingerprint: String,
    budget: super::BudgetRef,
    lifetime: Option<u64>,
    pub(crate) role: String,
    deadline: std::time::Instant,
    _capacity: Arc<tokio::sync::OwnedSemaphorePermit>,
    _native_cleanup: Option<Arc<tokio::sync::OwnedSemaphorePermit>>,
    cancelled: Arc<std::sync::atomic::AtomicBool>,
}

fn service_fingerprint(
    record: &Record,
    session: &std::path::Path,
    role: &str,
    budget: &super::BudgetRef,
    lifetime: Option<u64>,
) -> Result<String> {
    let session_id = crate::plugins::admission::digest(&session)?;
    if let Some(id) = lifetime {
        super::plugin_session::validate_live(record, id)?;
        ensure!(
            role == "native-session"
                && matches!(budget, super::BudgetRef::SessionHooks { .. })
                && super::budget_accounting::inherited(record, id)? == *budget,
            "MCP native service requires its original session allowance"
        );
        let allocation = super::budget_accounting::active(record, &session_id, budget)?
            .context("MCP session allowance missing")?;
        let (_, owner) = super::plugin_session::lifetime(record, id)?;
        return crate::plugins::admission::digest(&(
            session,
            &record.workspace,
            &record.identity,
            id,
            owner.workspace,
            budget,
            allocation.started_ms,
            allocation.deadline_ms,
            &allocation.limits,
            role,
        ));
    }
    ensure!(
        matches!(budget, super::BudgetRef::Task { .. }),
        "MCP task service requires original task funding"
    );
    super::budget_accounting::active(record, &session_id, budget)?;
    let allocation = record
        .allocation
        .as_ref()
        .context("MCP service requires an owning allowance")?;
    ensure!(
        !record.recovery_pending
            && record
                .task
                .as_ref()
                .is_none_or(|t| !t.stopped && t.accepted.is_none()),
        "MCP service owner is held or stopped"
    );
    delegation::ensure_agent_active(record, role)?;
    let assignment =
        delegation::agent_id(role).and_then(|id| record.agents.iter().find(|a| a.id == id));
    crate::plugins::admission::digest(&(
        session,
        &record.workspace,
        &record.identity,
        record.task.as_ref().map(|t| t.id),
        record.task_allocation_epoch,
        allocation.started_ms,
        allocation.deadline_ms,
        &allocation.limits,
        role,
        assignment.map(|a| {
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

/// A host-marked hook model request. Package/model data cannot create this value.
#[derive(Clone)]
pub(crate) struct ModelAdmission {
    pub key: AdmissionKey,
    pub budget: super::BudgetRef,
    pub owner: u64,
    pub invocation: u32,
    pub event: HookEvent,
    pub maximum: u32,
    pub snapshot: Arc<crate::plugins::runners::SnapshotInspection>,
    pub cancelled: Arc<std::sync::atomic::AtomicBool>,
}

pub(super) fn validate_model_admission(
    record: &Record,
    phase: &str,
    hook: &ModelAdmission,
) -> Result<()> {
    validate_model_owner(record, &hook.key.session, hook)?;
    ensure!(
        record
            .operations
            .iter()
            .filter(|op| op.phase == phase
                && matches!(
                    op.host_invocation,
                    Some(super::HostInvocation::Model | super::HostInvocation::Backend)
                ))
            .count()
            < hook.maximum as usize,
        "hook model/backend invocation allowance exhausted"
    );
    Ok(())
}

pub(super) fn validate_model_owner(
    record: &Record,
    session: &str,
    hook: &ModelAdmission,
) -> Result<()> {
    ensure!(
        hook.key.session == session,
        "model hook belongs to another session"
    );
    ensure!(
        !hook.cancelled.load(std::sync::atomic::Ordering::Acquire),
        "model hook cancelled before admission"
    );
    let receipt = active_for_event(record, hook.owner, hook.event)?;
    validate_model_budget(record, session, hook.owner, hook.event, &hook.budget)?;
    ensure!(
        super::budget_accounting::inherited(record, hook.owner)? == hook.budget,
        "model hook budget differs from its original owner"
    );
    let invocation = receipt.invocation(hook.invocation)?;
    ensure!(
        invocation.inspected == hook.key,
        "model hook capability belongs to a different admission or session"
    );
    ensure!(
        invocation.outcome.is_none(),
        "model hook invocation already settled"
    );
    Ok(())
}

pub(super) fn validate_model_budget(
    record: &Record,
    session: &str,
    owner: u64,
    event: HookEvent,
    budget: &super::BudgetRef,
) -> Result<()> {
    validate_funded_budget(record, session, owner, event, budget)
}

pub(super) fn validate_funded_budget(
    record: &Record,
    session: &str,
    owner: u64,
    event: HookEvent,
    budget: &super::BudgetRef,
) -> Result<()> {
    if matches!(event, HookEvent::SessionStart | HookEvent::SessionEnd) {
        let receipt = super::plugin_non_tool::active(record, owner, event)?;
        let lifetime = receipt
            .facts
            .native_session
            .context("session model lifetime missing")?;
        ensure!(
            matches!(budget, super::BudgetRef::SessionHooks { .. })
                && super::budget_accounting::inherited(record, lifetime)? == *budget,
            "session model requires its original explicit session allowance"
        );
    } else {
        ensure!(
            !matches!(budget, super::BudgetRef::SessionHooks { .. }),
            "ordinary model hook cannot borrow session funding"
        );
    }
    ensure!(
        super::budget_accounting::active(record, session, budget)?.is_some(),
        "hook requires an owning allowance from its task or explicitly configured session"
    );
    Ok(())
}

impl SharedRuntime {
    pub(crate) fn plugin_model_remaining(
        &self,
        owner: u64,
        event: HookEvent,
    ) -> Result<std::time::Duration> {
        self.plugin_funded_remaining(owner, event)
    }
    pub(crate) fn plugin_funded_remaining(
        &self,
        owner: u64,
        event: HookEvent,
    ) -> Result<std::time::Duration> {
        self.plugin_runner_owner(owner, event)?;
        let session = self.plugin_session()?;
        let record = self.record()?;
        let budget = super::budget_accounting::inherited(&record, owner)?;
        validate_funded_budget(&record, &session, owner, event, &budget)?;
        Ok(self
            .budget_remaining(&budget)?
            .min(self.plugin_remaining(owner, event)?))
    }
    pub(crate) fn validate_hook_model_owner(&self, hook: &ModelAdmission) -> Result<()> {
        let session = self.plugin_session()?;
        let runtime = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("runtime lock failed"))?;
        ensure!(!runtime.failed, "session persistence failed");
        validate_model_owner(&runtime.record, &session, hook)?;
        ensure!(
            super::budget_accounting::active(&runtime.record, &session, &hook.budget)?
                .context("hook allocation missing")?
                .remaining_ms()?
                > 0,
            "model hook owner deadline expired"
        );
        Ok(())
    }
    #[cfg(test)]
    pub(crate) fn begin_plugin_service(&self, owner: u64, service: &str) -> Result<u64> {
        self.begin_plugin_service_for(owner, HookEvent::PreToolUse, service)
    }
    pub(crate) fn begin_plugin_service_for(
        &self,
        owner: u64,
        event: HookEvent,
        service: &str,
    ) -> Result<u64> {
        ensure!(
            service.len() == 64 && service.bytes().all(|b| b.is_ascii_hexdigit()),
            "MCP service operation identity is invalid"
        );
        let session = self.plugin_session()?;
        let begin = |record: &mut Record| {
            active_for_event(record, owner, event)?;
            let budget = super::budget_accounting::inherited(record, owner)?;
            validate_funded_budget(record, &session, owner, event, &budget)?;
            ensure!(
                super::budget_accounting::active(record, &session, &budget)?.is_some(),
                "MCP service requires its original owning allowance"
            );
            ensure!(
                record.operations.len() < 4096,
                "session operation history is full"
            );
            ensure!(!record.operations.iter().any(|o| !o.complete && !o.reconciled && matches!(&o.host_invocation,
                Some(super::HostInvocation::PluginService {service: existing,..}) if existing == service)), "MCP startup is unresolved; reconcile before readmission");
            let phase = record
                .operations
                .iter()
                .find(|o| o.id == owner)
                .context("MCP owner missing")?
                .phase
                .clone();
            let budget = super::budget_accounting::inherited(record, owner)?;
            let id = record.operations.len() as u64 + 1;
            record.operations.push(super::Operation {
                budget: Some(budget),
                usage_receipt: None,
                id,
                phase,
                verification: None,
                call: None,
                result: None,
                tool_receipt: None,
                host_invocation: Some(super::HostInvocation::PluginService {
                    owner,
                    service: service.into(),
                    outcome: super::PluginServiceOutcome::Pending,
                }),
                complete: false,
                reconciled: false,
                usage_reported: true,
                identity: Some(record.identity.clone()),
            });
            Ok(id)
        };
        if matches!(event, HookEvent::SessionStart | HookEvent::SessionEnd) {
            let id = self.update(begin)?;
            #[cfg(test)]
            session_services::invalidate_startup_checkpoint(self);
            self.plugin_funded_remaining(owner, event)?;
            Ok(id)
        } else {
            self.admission(begin)
        }
    }
    pub(crate) fn complete_plugin_service(&self, id: u64) -> Result<()> {
        self.update(|record| {
            let operation = record
                .operations
                .iter_mut()
                .find(|o| o.id == id)
                .context("MCP startup operation missing")?;
            ensure!(
                matches!(
                    operation.host_invocation,
                    Some(super::HostInvocation::PluginService { .. })
                ) && !operation.complete,
                "MCP startup operation is not pending"
            );
            operation.complete = true;
            if let Some(super::HostInvocation::PluginService { outcome, .. }) =
                &mut operation.host_invocation
            {
                *outcome = super::PluginServiceOutcome::Ready;
            }
            Ok(())
        })
    }
    pub(crate) fn fail_plugin_service(&self, id: u64, known_local_teardown: bool) -> Result<()> {
        self.update(|record| {
            let operation = record
                .operations
                .iter_mut()
                .find(|o| o.id == id)
                .context("MCP startup operation missing")?;
            let Some(super::HostInvocation::PluginService { outcome, .. }) =
                &mut operation.host_invocation
            else {
                anyhow::bail!("MCP startup operation has different authority");
            };
            if *outcome == super::PluginServiceOutcome::Ready
                || *outcome == super::PluginServiceOutcome::Failed
            {
                return Ok(());
            }
            *outcome = if known_local_teardown {
                super::PluginServiceOutcome::Failed
            } else {
                super::PluginServiceOutcome::Uncertain
            };
            operation.complete = known_local_teardown;
            if !known_local_teardown {
                record.recovery_pending = true;
            }
            Ok(())
        })
    }
    pub(crate) fn admit_plugin_service_for(
        &self,
        operation: u64,
        event: HookEvent,
    ) -> Result<ServiceOwner> {
        let runtime = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("runtime lock failed"))?;
        ensure!(!runtime.failed, "session persistence failed");
        active_for_event(&runtime.record, operation, event)?;
        let session = crate::plugins::admission::digest(&runtime.store.directory())?;
        let budget = super::budget_accounting::inherited(&runtime.record, operation)?;
        validate_funded_budget(&runtime.record, &session, operation, event, &budget)?;
        let allocation = super::budget_accounting::active(&runtime.record, &session, &budget)?
            .context("MCP service requires its original owning allowance")?;
        let role = runtime
            .record
            .operations
            .iter()
            .find(|o| o.id == operation)
            .context("MCP operation missing")?
            .phase
            .clone();
        let lifetime = if matches!(event, HookEvent::SessionStart | HookEvent::SessionEnd) {
            Some(
                super::plugin_non_tool::active(&runtime.record, operation, event)?
                    .facts
                    .native_session
                    .context("MCP session lifetime missing")?,
            )
        } else {
            None
        };
        let fingerprint = service_fingerprint(
            &runtime.record,
            runtime.store.directory(),
            &role,
            &budget,
            lifetime,
        )?;
        let remaining = allocation.remaining_ms()?;
        ensure!(remaining > 0, "MCP owner expired");
        let capacity = runtime
            .service_slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| {
                anyhow::anyhow!("MCP service capacity exhausted (8); stop an existing service")
            })?;
        let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let native_cleanup = lifetime
            .map(|id| -> Result<_> {
                let (_, owner) = super::plugin_session::lifetime(&runtime.record, id)?;
                owner
                    .authority
                    .as_ref()
                    .context("MCP host lifetime unavailable")?
                    .own_service(&cancelled)
            })
            .transpose()?;
        Ok(ServiceOwner {
            fingerprint,
            budget,
            lifetime,
            role,
            deadline: std::time::Instant::now() + std::time::Duration::from_millis(remaining),
            _capacity: Arc::new(capacity),
            _native_cleanup: native_cleanup,
            cancelled,
        })
    }
    pub(crate) fn validate_plugin_service(
        &self,
        owner: &ServiceOwner,
    ) -> Result<std::time::Duration> {
        ensure!(
            !owner.cancelled.load(std::sync::atomic::Ordering::Acquire),
            "MCP owning observation cancelled"
        );
        let runtime = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("runtime lock failed"))?;
        ensure!(!runtime.failed, "session persistence failed");
        ensure!(
            service_fingerprint(
                &runtime.record,
                runtime.store.directory(),
                &owner.role,
                &owner.budget,
                owner.lifetime
            )? == owner.fingerprint,
            "MCP service owner changed"
        );
        let remaining = std::time::Duration::from_millis(
            super::budget_accounting::active(
                &runtime.record,
                &crate::plugins::admission::digest(&runtime.store.directory())?,
                &owner.budget,
            )?
            .context("MCP allocation missing")?
            .remaining_ms()?,
        )
        .min(
            owner
                .deadline
                .saturating_duration_since(std::time::Instant::now()),
        );
        ensure!(!remaining.is_zero(), "MCP service owner expired");
        Ok(remaining)
    }
    pub(crate) fn validate_plugin_service_invocation(
        &self,
        owner: &ServiceOwner,
        operation: u64,
        event: HookEvent,
    ) -> Result<std::time::Duration> {
        let remaining = self.plugin_funded_remaining(operation, event)?;
        let record = self.record()?;
        ensure!(
            super::budget_accounting::inherited(&record, operation)? == owner.budget,
            "MCP invoking budget differs from its connection"
        );
        let lifetime = if matches!(event, HookEvent::SessionStart | HookEvent::SessionEnd) {
            super::plugin_non_tool::active(&record, operation, event)?
                .facts
                .native_session
        } else {
            None
        };
        ensure!(
            lifetime == owner.lifetime
                && record
                    .operations
                    .iter()
                    .find(|o| o.id == operation)
                    .is_some_and(|o| o.phase == owner.role),
            "MCP invoking owner differs from its connection"
        );
        Ok(remaining.min(self.validate_plugin_service(owner)?))
    }
    pub(crate) fn settle_hook_models(&self, phase: &str) -> Result<()> {
        let session = self.plugin_session()?;
        self.update(|record| {
            super::budget_accounting::mark_missing(record, &session, |o| {
                o.phase == phase && !o.complete
            });
            for operation in &mut record.operations {
                if operation.phase == phase && super::budget_accounting::is_model(operation) {
                    operation.complete = true;
                }
            }
            Ok(())
        })
    }
}

/// A borrowed reservation capability. Tool owners keep their distinct admission
/// proofs; runners only receive the event's own retained hook reservations.
pub(super) struct ReservedHooks<'a> {
    hooks: &'a [HookReceipt],
    operation: u64,
    source_operation: u64,
    event: HookEvent,
    role: &'a str,
    subject: Option<&'a LifecycleSubject>,
    declaration_role: &'a str,
}
impl<'a> ReservedHooks<'a> {
    fn invocation(&self, invocation: u32) -> Result<&'a HookReceipt> {
        let hook = self
            .hooks
            .get(invocation as usize)
            .context("hook reservation missing")?;
        ensure!(
            hook.invocation == invocation
                && hook.inspected.operation == self.operation
                && hook.inspected.source_operation == self.source_operation
                && hook.inspected.event == self.event.as_str()
                && hook.inspected.role == self.role
                && hook.declaration.role == self.declaration_role
                && hook.inspected.lifecycle.as_ref() == self.subject
                && hook.inspected.tool.is_none() == self.subject.is_some()
                && hook.inspected.arguments.is_none() == self.subject.is_some(),
            "hook reservation belongs to a different event or owner"
        );
        Ok(hook)
    }
}

pub(super) fn active_for_event(
    record: &Record,
    id: u64,
    event: HookEvent,
) -> Result<ReservedHooks<'_>> {
    let (hooks, source_operation, role, subject, declaration_role) =
        if event == HookEvent::PreToolUse {
            let receipt = active(record, id)?;
            let operation = record
                .operations
                .iter()
                .find(|o| o.id == id)
                .expect("validated");
            (
                receipt
                    .plugin_admission
                    .as_ref()
                    .map_or(&[][..], |p| p.hooks.as_slice()),
                receipt.invocation,
                operation.phase.as_str(),
                None,
                operation.phase.as_str(),
            )
        } else if matches!(
            event,
            HookEvent::UserPromptSubmit
                | HookEvent::Stop
                | HookEvent::StopFailure
                | HookEvent::SessionStart
                | HookEvent::SessionEnd
        ) {
            let receipt = super::plugin_non_tool::active(record, id, event)?;
            (
                receipt.hooks.as_slice(),
                receipt.facts.causal_operation(),
                receipt.facts.role.as_str(),
                Some(&receipt.facts.subject),
                receipt
                    .facts
                    .declaration_role
                    .as_deref()
                    .unwrap_or("worker"),
            )
        } else {
            let receipt = super::plugin_lifecycle::active(record, id, event)?;
            (
                receipt.hooks.as_slice(),
                receipt.facts.source_operation,
                receipt.facts.role.as_str(),
                None,
                receipt.facts.role.as_str(),
            )
        };
    Ok(ReservedHooks {
        hooks,
        operation: id,
        source_operation,
        event,
        role,
        subject,
        declaration_role,
    })
}

fn active(record: &Record, id: u64) -> Result<&super::ToolReceipt> {
    let operation = record
        .operations
        .iter()
        .find(|op| op.id == id)
        .context("plugin tool operation missing")?;
    ensure!(
        operation.result.is_none() && !record.recovery_pending,
        "plugin owner is held or settled"
    );
    delegation::ensure_agent_active(record, &operation.phase)?;
    let receipt = pre_tool_receipt(operation)?;
    ensure!(
        receipt
            .plugin_admission
            .as_ref()
            .is_none_or(|plan| plan.hold.is_none()),
        "plugin admission is held"
    );
    Ok(receipt)
}

fn pre_tool_receipt(operation: &super::Operation) -> Result<&super::ToolReceipt> {
    let receipt = operation
        .tool_receipt
        .as_ref()
        .context("plugin requires a tool receipt")?;
    ensure!(
        receipt.attempt_admitted && !receipt.effect_started,
        "plugin invocation is outside pre-tool admission"
    );
    Ok(receipt)
}
pub(super) fn validate_final_key(
    operation: &super::Operation,
    call: &crate::tools::ToolCall,
    session: &str,
) -> Result<()> {
    let receipt = operation
        .tool_receipt
        .as_ref()
        .context("tool receipt missing")?;
    if let Some(plan) = &receipt.plugin_admission {
        let key = plan
            .final_key
            .as_ref()
            .context("plugin final candidate is not frozen")?;
        ensure!(plan.hold.is_none(), "plugin final candidate is held");
        ensure!(
            key.session == session,
            "plugin final key belongs to another session"
        );
        validate_binding(operation, call, key, &plan.plan)?;
        ensure!(
            plan.hooks.iter().all(|h| h.transferred()
                || (h.outcome.is_some() && !h.uncertain_effects && h.hold.is_none())),
            "plugin decision is unknown or held"
        );
    }
    Ok(())
}
fn validate_binding(
    operation: &super::Operation,
    call: &crate::tools::ToolCall,
    key: &AdmissionKey,
    plan: &str,
) -> Result<()> {
    let receipt = operation
        .tool_receipt
        .as_ref()
        .context("tool receipt missing")?;
    ensure!(
        key.operation == operation.id
            && key.source_operation == receipt.invocation
            && key.role == operation.phase
            && key.plan == plan
            && key.event == "PreToolUse",
        "plugin final key has a different operation, role, event or generation"
    );
    ensure!(
        call.id == receipt.original_call.id
            && call.name == receipt.original_call.name
            && key.lifecycle.is_none()
            && key.tool.as_deref() == Some(call.name.as_str())
            && key.arguments.as_deref()
                == Some(crate::plugins::admission::candidate_digest(call)?.as_str()),
        "plugin final key has a different candidate"
    );
    Ok(())
}
impl SharedRuntime {
    pub(crate) fn plugin_session(&self) -> Result<String> {
        let runtime = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("runtime lock failed"))?;
        crate::plugins::admission::digest(&runtime.store.directory())
    }

    pub(crate) fn mutation_boundary(
        &self,
        identity: (u64, u64),
    ) -> Result<Arc<tokio::sync::Mutex<()>>> {
        let mut runtime = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("runtime lock failed"))?;
        ensure!(!runtime.failed, "session persistence failed");
        if !runtime.mutation_boundaries.contains_key(&identity) {
            ensure!(
                runtime.mutation_boundaries.len() < 128,
                "workspace mutation boundary limit reached"
            );
        }
        Ok(runtime
            .mutation_boundaries
            .entry(identity)
            .or_default()
            .clone())
    }
    pub(crate) fn plugin_hook_key(
        &self,
        id: u64,
        event: HookEvent,
        invocation: u32,
    ) -> Result<AdmissionKey> {
        let record = self.record()?;
        let receipt = active_for_event(&record, id, event)?;
        Ok(receipt.invocation(invocation)?.inspected.clone())
    }
    pub(crate) fn plugin_owner(&self, id: u64) -> Result<(u64, String)> {
        let runtime = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("runtime lock failed"))?;
        ensure!(!runtime.failed, "session persistence failed");
        let receipt = active(&runtime.record, id)?;
        let session = crate::plugins::admission::digest(&runtime.store.directory())?;
        super::budget_accounting::active(
            &runtime.record,
            &session,
            &super::budget_accounting::inherited(&runtime.record, id)?,
        )?;
        let phase = runtime
            .record
            .operations
            .iter()
            .find(|op| op.id == id)
            .expect("validated")
            .phase
            .clone();
        Ok((receipt.invocation, phase))
    }
    pub(crate) fn begin_plugin_plan(
        &self,
        id: u64,
        plan: String,
        declarations: Vec<serde_json::Value>,
    ) -> Result<()> {
        self.admission(|record| {
            let receipt = active(record, id)?;
            ensure!(
                receipt.plugin_admission.is_none(),
                "plugin plan already captured; do not replay"
            );
            ensure!(declarations.len() <= 32, "plugin declaration limit reached");
            let receipt = record
                .operations
                .iter_mut()
                .find(|op| op.id == id)
                .expect("validated")
                .tool_receipt
                .as_mut()
                .expect("validated");
            receipt.plugin_admission = Some(AdmissionReceipt {
                plan,
                declarations,
                ..Default::default()
            });
            Ok(())
        })
    }
    #[cfg(test)]
    pub(crate) fn begin_plugin_hook(
        &self,
        id: u64,
        hook: HookReceipt,
        call: &crate::tools::ToolCall,
    ) -> Result<u32> {
        match self.reserve_plugin_hook(id, hook, call, None, None)? {
            crate::plugins::once::HookReservation::Run(hook) => Ok(hook.invocation),
            crate::plugins::once::HookReservation::Skipped => {
                anyhow::bail!("unexpected one-shot skip")
            }
        }
    }
    pub(crate) fn reserve_plugin_hook(
        &self,
        id: u64,
        mut hook: HookReceipt,
        call: &crate::tools::ToolCall,
        binding: Option<&crate::plugins::once::OnceBinding>,
        lease: Option<&std::sync::Arc<tokio::sync::OwnedSemaphorePermit>>,
    ) -> Result<crate::plugins::once::HookReservation> {
        let session = self.plugin_session()?;
        let tracker = self.once_live()?;
        self.admission(|record| {
            let receipt = active(record, id)?;
            let plan = receipt
                .plugin_admission
                .as_ref()
                .context("plugin plan missing")?;
            ensure!(
                plan.hooks.len() + plan.once_skips.len() < 128 && plan.final_key.is_none(),
                "plugin invocation limit or frozen admission"
            );
            ensure!(
                hook.inspected.session == session
                    && hook.inspected.operation == id
                    && hook.inspected.source_operation == receipt.invocation
                    && hook.inspected.plan == plan.plan,
                "hook causal identity mismatch"
            );
            let operation = record
                .operations
                .iter()
                .find(|op| op.id == id)
                .expect("validated");
            ensure!(
                hook.inspected.role == operation.phase && hook.declaration.role == operation.phase,
                "hook role mismatch"
            );
            let declaration = serde_json::to_value(&hook.declaration)?;
            let encoded_binding = serde_json::to_value(binding)?;
            let encoded_source = serde_json::to_value(&hook.source)?;
            ensure!(
                plan.declarations
                    .iter()
                    .any(|d| d["identity"] == declaration
                        && d["once"] == encoded_binding
                        && d["source"] == encoded_source
                        && d["required_gate"] == hook.required_gate),
                "hook package or generation is not part of the captured plan"
            );
            validate_binding(operation, call, &hook.inspected, &plan.plan)?;
            let index = plan.hooks.len() as u32;
            hook.invocation = index;
            let skipped = super::plugin_once::reserve(record, &mut hook, binding)?;
            if skipped.is_none() {
                super::plugin_once::track_live(&tracker, &hook, lease)?;
            }
            let operation = record
                .operations
                .iter_mut()
                .find(|op| op.id == id)
                .expect("validated");
            operation.call = Some(call.clone());
            let plan = operation
                .tool_receipt
                .as_mut()
                .expect("validated")
                .plugin_admission
                .as_mut()
                .expect("validated");
            if let Some(skip) = skipped {
                plan.once_skips.push(skip);
                Ok(crate::plugins::once::HookReservation::Skipped)
            } else {
                plan.hooks.push(hook.clone());
                Ok(crate::plugins::once::HookReservation::Run(Box::new(hook)))
            }
        })
    }
    pub(crate) fn finish_plugin_hook(&self, id: u64, mut hook: HookReceipt) -> Result<()> {
        let session = self.plugin_session()?;
        self.update(|record| {
            // Reserving a runner required an active owner. Settling that exact
            // invocation retains facts, even after a sibling or owner is held;
            // it never grants another invocation or clears a recovery hold.
            let operation = record
                .operations
                .iter()
                .find(|op| op.id == id)
                .context("plugin tool operation missing")?;
            let receipt = pre_tool_receipt(operation)?;
            let plan = receipt
                .plugin_admission
                .as_ref()
                .context("plugin plan missing")?;
            let previous = plan
                .hooks
                .get(hook.invocation as usize)
                .context("hook invocation missing")?;
            ensure!(
                plan.final_key.is_none()
                    && hook.outcome.is_some()
                    && previous.outcome.is_none()
                    && previous.invocation == hook.invocation
                    && previous.inspected == hook.inspected
                    && previous.declaration == hook.declaration
                    && previous.endpoint == hook.endpoint
                    && previous.class == hook.class
                    && previous.required_gate == hook.required_gate
                    && previous.once == hook.once
                    && previous.source == hook.source,
                "duplicate or mismatched hook outcome"
            );
            ensure!(
                hook.inspected.session == session
                    && hook.inspected.operation == id
                    && hook.inspected.source_operation == receipt.invocation
                    && hook.inspected.role == operation.phase
                    && hook.inspected.plan == plan.plan,
                "hook settlement belongs to a different owner or session"
            );
            ensure!(
                serde_json::to_vec(&hook)?.len() <= 256 * 1024,
                "hook receipt exceeds bound"
            );
            let retained = plan
                .hooks
                .iter()
                .enumerate()
                .map(|(index, h)| {
                    serde_json::to_vec(if index == hook.invocation as usize {
                        &hook
                    } else {
                        h
                    })
                    .map(|v| v.len())
                })
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .sum::<usize>();
            ensure!(retained <= 4 * 1024 * 1024, "plugin history limit reached");
            super::plugin_once::settle_pre(&mut hook)?;
            let index = hook.invocation as usize;
            if hook.uncertain_effects {
                record.recovery_pending = true;
            }
            record
                .operations
                .iter_mut()
                .find(|op| op.id == id)
                .expect("validated")
                .tool_receipt
                .as_mut()
                .expect("validated")
                .plugin_admission
                .as_mut()
                .expect("validated")
                .hooks[index] = hook;
            Ok(())
        })
    }
    pub(crate) fn freeze_plugin(
        &self,
        id: u64,
        key: AdmissionKey,
        call: &crate::tools::ToolCall,
        hold: Option<String>,
    ) -> Result<()> {
        let session = self.plugin_session()?;
        self.admission(|record| {
            let receipt = active(record, id)?;
            let plan = receipt
                .plugin_admission
                .as_ref()
                .context("plugin plan missing")?;
            ensure!(
                key.session == session
                    && key.operation == id
                    && key.source_operation == receipt.invocation
                    && key.plan == plan.plan
                    && plan.final_key.is_none(),
                "final plugin key mismatch or duplicate freeze"
            );
            ensure!(
                plan.hooks
                    .iter()
                    .all(|h| h.outcome.is_some() || h.transferred()),
                "plugin outcome unknown; no release"
            );
            let operation = record
                .operations
                .iter()
                .find(|op| op.id == id)
                .expect("validated");
            validate_binding(operation, call, &key, &plan.plan)?;
            super::plugin_once::validate_skips(record, &plan.once_skips)?;
            let operation = record
                .operations
                .iter_mut()
                .find(|op| op.id == id)
                .expect("validated");
            operation.call = Some(call.clone());
            let plan = operation
                .tool_receipt
                .as_mut()
                .expect("validated")
                .plugin_admission
                .as_mut()
                .expect("validated");
            plan.final_key = Some(key);
            plan.hold = hold;
            Ok(())
        })
    }
    pub(crate) fn cancel_plugin_invocation(
        &self,
        owner: u64,
        event: HookEvent,
        invocation: u32,
    ) -> Result<()> {
        let record = self.record()?;
        let mut hook = record
            .operations
            .iter()
            .find(|o| o.id == owner)
            .and_then(|o| o.plugin_hooks(event))
            .and_then(|hooks| hooks.get(invocation as usize))
            .context("cancelled hook receipt missing")?
            .clone();
        if hook.outcome.is_some() {
            return Ok(());
        }
        hook.outcome = Some(RawOutcome::Failure {
            reason: "MCP hook cancelled; effects may be unknown".into(),
        });
        hook.uncertain_effects = true;
        if event == HookEvent::PreToolUse {
            self.finish_plugin_hook(owner, hook)
        } else if matches!(
            event,
            HookEvent::UserPromptSubmit
                | HookEvent::Stop
                | HookEvent::StopFailure
                | HookEvent::SessionStart
                | HookEvent::SessionEnd
        ) {
            self.finish_non_tool_hook(owner, event, hook)
        } else {
            self.finish_post_hook(owner, event, hook)
        }
    }
    pub(crate) fn hold_plugin(&self, id: u64, reason: &str) -> Result<()> {
        self.update(|record| {
            let receipt = record
                .operations
                .iter_mut()
                .find(|op| op.id == id)
                .and_then(|o| o.tool_receipt.as_mut())
                .context("plugin tool missing")?;
            if let Some(plan) = &mut receipt.plugin_admission {
                plan.hold = Some(reason.chars().take(4096).collect());
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod session_services;

#[cfg(test)]
pub(super) mod session_models;
