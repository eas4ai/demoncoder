//! Plugin admissions share the existing tool operation, store, owner and allowance.
use super::{Record, SharedRuntime, delegation};
use crate::plugins::receipts::*;
use anyhow::{Context, Result, ensure};
use std::sync::Arc;

/// Ephemeral capability derived from the existing task allocation, never a new allowance.
#[derive(Clone)]
pub(crate) struct ServiceOwner {
    fingerprint: String,
    pub(crate) role: String,
    deadline: std::time::Instant,
    _capacity: Arc<tokio::sync::OwnedSemaphorePermit>,
}

fn service_fingerprint(record: &Record, session: &std::path::Path, role: &str) -> Result<String> {
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
    pub owner: u64,
    pub invocation: u32,
    pub maximum: u32,
    pub snapshot: Arc<crate::plugins::runners::SnapshotInspection>,
    pub cancelled: Arc<std::sync::atomic::AtomicBool>,
}

pub(super) fn validate_model_admission(
    record: &Record,
    phase: &str,
    hook: &ModelAdmission,
) -> Result<()> {
    ensure!(
        !hook.cancelled.load(std::sync::atomic::Ordering::Acquire),
        "model hook cancelled before admission"
    );
    let receipt = active(record, hook.owner)?;
    ensure!(
        record.allocation.is_some(),
        "model hook requires an owning task or explicitly configured session allowance"
    );
    let invocation = receipt
        .plugin_admission
        .as_ref()
        .and_then(|plan| plan.hooks.iter().find(|h| h.invocation == hook.invocation))
        .context("model hook lacks a reserved invocation")?;
    ensure!(
        invocation.outcome.is_none(),
        "model hook invocation already settled"
    );
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

impl SharedRuntime {
    pub(crate) fn begin_plugin_service(&self, owner: u64, service: &str) -> Result<u64> {
        ensure!(
            service.len() == 64 && service.bytes().all(|b| b.is_ascii_hexdigit()),
            "MCP service operation identity is invalid"
        );
        self.admission(|record| {
            active(record,owner)?;
            ensure!(record.operations.len() < 4096, "session operation history is full");
            ensure!(!record.operations.iter().any(|o| !o.complete && !o.reconciled && matches!(&o.host_invocation,
                Some(super::HostInvocation::PluginService {service: existing,..}) if existing == service)), "MCP startup is unresolved; reconcile before readmission");
            let phase = record.operations.iter().find(|o|o.id == owner).context("MCP owner missing")?.phase.clone();
            let id = record.operations.len() as u64 + 1;
            record.operations.push(super::Operation {id,phase,verification:None,call:None,result:None,tool_receipt:None,
                host_invocation:Some(super::HostInvocation::PluginService {owner,service:service.into(),outcome:super::PluginServiceOutcome::Pending}),complete:false,reconciled:false,
                usage_reported:true,identity:Some(record.identity.clone())});
            Ok(id)
        })
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
    pub(crate) fn admit_plugin_service(&self, operation: u64) -> Result<ServiceOwner> {
        let runtime = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("runtime lock failed"))?;
        ensure!(!runtime.failed, "session persistence failed");
        active(&runtime.record, operation)?;
        let role = runtime
            .record
            .operations
            .iter()
            .find(|o| o.id == operation)
            .context("MCP operation missing")?
            .phase
            .clone();
        let fingerprint = service_fingerprint(&runtime.record, runtime.store.directory(), &role)?;
        let remaining = runtime
            .record
            .allocation
            .as_ref()
            .context("MCP allocation missing")?
            .remaining_ms()?;
        ensure!(remaining > 0, "MCP owner expired");
        let capacity = runtime
            .service_slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| {
                anyhow::anyhow!("MCP service capacity exhausted (8); stop an existing service")
            })?;
        Ok(ServiceOwner {
            fingerprint,
            role,
            deadline: std::time::Instant::now() + std::time::Duration::from_millis(remaining),
            _capacity: Arc::new(capacity),
        })
    }
    pub(crate) fn validate_plugin_service(
        &self,
        owner: &ServiceOwner,
    ) -> Result<std::time::Duration> {
        let runtime = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("runtime lock failed"))?;
        ensure!(!runtime.failed, "session persistence failed");
        ensure!(
            service_fingerprint(&runtime.record, runtime.store.directory(), &owner.role)?
                == owner.fingerprint,
            "MCP service owner changed"
        );
        let remaining = std::time::Duration::from_millis(
            runtime
                .record
                .allocation
                .as_ref()
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
    pub(crate) fn settle_hook_models(&self, phase: &str) -> Result<()> {
        self.update(|record| {
            let mut uncertain = false;
            for operation in &mut record.operations {
                if operation.phase == phase
                    && matches!(
                        operation.host_invocation,
                        Some(super::HostInvocation::Model | super::HostInvocation::Backend)
                    )
                    && !operation.complete
                {
                    operation.complete = true;
                    uncertain |= !operation.usage_reported;
                }
            }
            if uncertain && let Some(allocation) = &mut record.allocation {
                allocation.usage.uncertain();
            }
            Ok(())
        })
    }
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
            plan.hooks
                .iter()
                .all(|h| h.outcome.is_some() && !h.uncertain_effects && h.hold.is_none()),
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
            && key.tool == call.name
            && key.arguments == crate::plugins::admission::candidate_digest(call)?,
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
    pub(crate) fn plugin_owner(&self, id: u64) -> Result<(u64, String)> {
        let runtime = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("runtime lock failed"))?;
        ensure!(!runtime.failed, "session persistence failed");
        let receipt = active(&runtime.record, id)?;
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
    pub(crate) fn begin_plugin_hook(
        &self,
        id: u64,
        mut hook: HookReceipt,
        call: &crate::tools::ToolCall,
    ) -> Result<u32> {
        let session = self.plugin_session()?;
        self.admission(|record| {
            let receipt = active(record, id)?;
            let plan = receipt
                .plugin_admission
                .as_ref()
                .context("plugin plan missing")?;
            ensure!(
                plan.hooks.len() < 128 && plan.final_key.is_none(),
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
            ensure!(
                plan.declarations
                    .iter()
                    .any(|d| d["identity"] == declaration),
                "hook package or generation is not part of the captured plan"
            );
            validate_binding(operation, call, &hook.inspected, &plan.plan)?;
            let index = plan.hooks.len() as u32;
            hook.invocation = index;
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
            plan.hooks.push(hook);
            Ok(index)
        })
    }
    pub(crate) fn finish_plugin_hook(&self, id: u64, hook: HookReceipt) -> Result<()> {
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
                    && previous.class == hook.class,
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
                plan.hooks.iter().all(|h| h.outcome.is_some()),
                "plugin outcome unknown; no release"
            );
            let operation = record
                .operations
                .iter()
                .find(|op| op.id == id)
                .expect("validated");
            validate_binding(operation, call, &key, &plan.plan)?;
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
    pub(crate) fn cancel_plugin_hook(&self, owner: u64, invocation: u32) -> Result<()> {
        let record = self.record()?;
        let mut hook = record
            .operations
            .iter()
            .find(|o| o.id == owner)
            .and_then(|o| o.tool_receipt.as_ref())
            .and_then(|r| r.plugin_admission.as_ref())
            .and_then(|p| p.hooks.get(invocation as usize))
            .context("cancelled hook receipt missing")?
            .clone();
        if hook.outcome.is_some() {
            return Ok(());
        }
        hook.outcome = Some(RawOutcome::Failure {
            reason: "MCP hook cancelled; effects may be unknown".into(),
        });
        hook.uncertain_effects = true;
        self.finish_plugin_hook(owner, hook)
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
