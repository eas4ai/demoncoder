//! Compaction is an owned operation in the existing session ledger.
use super::{HostInvocation, Identity, Operation, Record, SharedRuntime};
use crate::plugins::admission::digest;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Compaction {
    pub version: u32,
    pub trigger: String,
    #[serde(default)]
    pub external_backend: Option<u64>,
    #[serde(default)]
    pub native_session: Option<u64>,
    pub task: Option<u64>,
    pub child_owner: Option<String>,
    pub source: String,
    pub source_bytes: usize,
    pub pins: String,
    pub model_operation: Option<u64>,
    pub applied: Option<String>,
    pub applied_bytes: Option<usize>,
    pub summary: Option<String>,
    pub hold: Option<String>,
}
fn checkpoint<'a>(record: &'a Record, phase: &str) -> Result<&'a Value> {
    if phase == "worker" {
        return record
            .checkpoint
            .as_ref()
            .context("no durable context to compact");
    }
    let id: u64 = phase
        .strip_prefix("agent:")
        .and_then(|v| v.strip_suffix(":worker"))
        .context("compaction requires worker ownership")?
        .parse()?;
    record
        .agents
        .iter()
        .find(|a| a.id == id)
        .and_then(|a| a.checkpoint.as_ref())
        .context("child context missing")
}
pub(super) fn owner<'a>(
    record: &'a Record,
    phase: &str,
) -> Result<super::plugin_non_tool::owner::Owner<'a>> {
    ensure!(
        !record.recovery_pending,
        "compaction requires reconciliation"
    );
    if phase != "worker" {
        return super::plugin_non_tool::owner::resolve(record, phase);
    }
    ensure!(
        record.task.as_ref().is_none_or(|t| t.accepted.is_none()),
        "accepted task cannot compact"
    );
    // A stopped task retains its original allocation. This authorizes only the typed transaction.
    Ok(super::plugin_non_tool::owner::Owner {
        identity: &record.identity,
        root: &record.workspace,
        child: None,
    })
}
pub(super) fn validate<'a>(record: &'a Record, id: u64, phase: &str) -> Result<&'a Compaction> {
    let operation = record
        .operations
        .iter()
        .find(|o| o.id == id)
        .context("compaction owner missing")?;
    let Some(HostInvocation::Compaction(c)) = &operation.host_invocation else {
        anyhow::bail!("not a compaction owner")
    };
    let owner = owner(record, phase)?;
    ensure!(
        operation.phase == phase
            && operation.identity.as_ref() == Some(owner.identity)
            && !operation.reconciled
            && !operation.complete
            && c.task == record.task.as_ref().map(|t| t.id)
            && c.child_owner == owner.child
            && c.hold.is_none(),
        "compaction owner changed or is held"
    );
    Ok(c)
}
/// Return only the captured live session of an actually taskless transaction.
/// The summary keeps its ordinary Unallocated budget; this chain funds hooks only.
pub(super) fn hook_lifetime(record: &Record, id: u64, phase: &str) -> Result<Option<u64>> {
    let c = validate(record, id, phase)?;
    let Some(lifetime) = c.native_session else {
        return Ok(None);
    };
    ensure!(
        phase == "worker"
            && c.external_backend.is_none()
            && c.task.is_none()
            && c.child_owner.is_none()
            && record.task.is_none()
            && record.allocation.is_none()
            && super::budget_accounting::inherited(record, id)? == super::BudgetRef::Unallocated,
        "session funding requires an actually taskless compaction"
    );
    super::plugin_session::validate_live(record, lifetime)?;
    let (_, owner) = super::plugin_session::lifetime(record, lifetime)?;
    ensure!(owner.end.is_none(), "compaction native session ended");
    Ok(Some(lifetime))
}
impl SharedRuntime {
    pub(crate) fn compaction_remaining(&self, id: u64) -> Result<std::time::Duration> {
        self.validate_operation_deadline(id)?;
        Ok(self.remaining()?.min(std::time::Duration::from_secs(30)))
    }
    pub(crate) fn begin_compaction(
        &self,
        phase: &str,
        identity: Option<&Identity>,
        trigger: &str,
        source: &Value,
        native_session: Option<u64>,
    ) -> Result<u64> {
        let session = self.plugin_session()?;
        self.admission(|record| {
            ensure!(
                matches!(trigger, "manual" | "auto"),
                "unknown compaction trigger"
            );
            let owner = owner(record, phase)?;
            ensure!(
                identity.is_none_or(|i| i == owner.identity)
                    && (owner.child.is_none() || identity.is_some()),
                "compaction identity differs"
            );
            let execution_identity = owner.identity.clone();
            let child_owner = owner.child;
            super::plugin_lifecycle::ensure_continuation(record, phase)?;
            ensure!(
                !record.operations.iter().any(|o| o.phase == phase
                    && matches!(o.host_invocation, Some(HostInvocation::Compaction(_)))
                    && !o.complete),
                "compaction already pending; never replay"
            );
            ensure!(
                checkpoint(record, phase)? == source,
                "active context differs from durable checkpoint"
            );
            let bytes = serde_json::to_vec(source)?.len();
            ensure!(
                bytes <= 2 * 1024 * 1024 && record.operations.len() < 4096,
                "compaction retention bound"
            );
            ensure!(
                record.task.is_none() || record.allocation.is_some(),
                "task compaction requires its original allocation"
            );
            let budget = super::budget_accounting::capture(record, &session);
            let native_session =
                if phase == "worker" && record.task.is_none() && record.allocation.is_none() {
                    if let Some(id) = native_session {
                        super::plugin_session::validate_live(record, id)?;
                    }
                    native_session
                } else {
                    None
                };
            super::budget_accounting::active(record, &session, &budget)?;
            if let Some(a) = &record.allocation {
                ensure!(
                    a.remaining_ms()? > 0
                        && a.model_calls < a.limits.model_calls
                        && a.tool_calls < a.limits.tool_calls,
                    "original compaction allocation exhausted"
                );
            }
            let id = record.operations.len() as u64 + 1;
            record.operations.push(Operation {
                id,
                phase: phase.into(),
                identity: Some(execution_identity),
                budget: Some(budget),
                usage_receipt: None,
                verification: None,
                call: None,
                result: None,
                tool_receipt: None,
                host_invocation: Some(HostInvocation::Compaction(Compaction {
                    version: 1,
                    trigger: trigger.into(),
                    external_backend: None,
                    native_session,
                    task: record.task.as_ref().map(|t| t.id),
                    child_owner,
                    source: digest(source)?,
                    source_bytes: bytes,
                    pins: digest(&(
                        &record.identity,
                        &record.plugin_activations,
                        &record.task.as_ref().map(|t| (&t.id, &t.objective)),
                    ))?,
                    model_operation: None,
                    applied: None,
                    applied_bytes: None,
                    summary: None,
                    hold: None,
                })),
                complete: false,
                reconciled: false,
                usage_reported: true,
            });
            Ok(id)
        })
    }
    pub(crate) fn compaction_model(
        &self,
        id: u64,
        phase: &str,
        identity: Option<&Identity>,
    ) -> Result<u64> {
        {
            let r = self.record()?;
            let c = validate(&r, id, phase)?;
            ensure!(
                c.model_operation.is_none() && c.applied.is_none(),
                "compaction summary already requested"
            );
        }
        let model = self.begin_model_from(phase, identity, None, Some(id), None)?;
        self.update(|r| {
            let Some(HostInvocation::Compaction(c)) = &mut r
                .operations
                .iter_mut()
                .find(|o| o.id == id)
                .context("compaction missing")?
                .host_invocation
            else {
                anyhow::bail!("compaction kind changed")
            };
            c.model_operation = Some(model);
            Ok(())
        })?;
        Ok(model)
    }
    pub(crate) fn apply_compaction(
        &self,
        id: u64,
        phase: &str,
        new: Value,
        summary: String,
    ) -> Result<()> {
        let session = self.plugin_session()?;
        self.update(|r| {
            let c = validate(r, id, phase)?;
            ensure!(
                c.applied.is_none() && c.source == digest(checkpoint(r, phase)?)?,
                "compaction source changed or applied"
            );
            ensure!(
                c.pins
                    == digest(&(
                        &r.identity,
                        &r.plugin_activations,
                        &r.task.as_ref().map(|t| (&t.id, &t.objective))
                    ))?,
                "compaction policy or objective changed"
            );
            let operation = r.operations.iter().find(|o| o.id == id).expect("validated");
            super::budget_accounting::active_operation(r, &session, operation)?;
            let model = r
                .operations
                .iter()
                .find(|o| Some(o.id) == c.model_operation)
                .context("compaction summary admission missing")?;
            ensure!(
                model.complete
                    && !model.reconciled
                    && model.budget == operation.budget
                    && model.phase == phase,
                "compaction summary is unfinished or has another owner"
            );
            let bytes = serde_json::to_vec(&new)?.len();
            ensure!(
                bytes < c.source_bytes && !summary.trim().is_empty() && summary.len() <= 16 * 1024,
                "compaction summary is empty, oversized or does not shorten context"
            );
            let revision = digest(&new)?;
            let cursor = r.operations.len() as u64;
            if phase == "worker" {
                r.checkpoint = Some(new);
                r.checkpoint_cursor = cursor;
            } else {
                let id: u64 = phase
                    .strip_prefix("agent:")
                    .and_then(|s| s.strip_suffix(":worker"))
                    .context("child phase changed")?
                    .parse()?;
                let child = r
                    .agents
                    .iter_mut()
                    .find(|a| a.id == id)
                    .context("child missing")?;
                child.checkpoint = Some(new);
                child.checkpoint_cursor = cursor;
            }
            let operation = r
                .operations
                .iter_mut()
                .find(|o| o.id == id)
                .expect("validated");
            let Some(HostInvocation::Compaction(c)) = &mut operation.host_invocation else {
                unreachable!()
            };
            c.applied = Some(revision);
            c.applied_bytes = Some(bytes);
            c.summary = Some(summary);
            Ok(())
        })
    }
    pub(crate) fn end_compaction(&self, id: u64, hold: Option<String>) -> Result<()> {
        self.update(|r| {
            let operation = r
                .operations
                .iter_mut()
                .find(|o| o.id == id)
                .context("compaction missing")?;
            let Some(HostInvocation::Compaction(c)) = &mut operation.host_invocation else {
                anyhow::bail!("compaction kind changed")
            };
            c.hold = hold;
            operation.complete = true;
            Ok(())
        })
    }
    /// A write error can occur after rename. Read that exact installed checkpoint;
    /// never assume the old in-memory snapshot remains on disk or write it back.
    pub(crate) fn installed_compaction_checkpoint(&self, phase: &str) -> Result<Value> {
        let runtime = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("runtime lock failed"))?;
        let record: Record = serde_json::from_value(runtime.store.read()?)?;
        Ok(checkpoint(&record, phase)?.clone())
    }
}

pub(super) fn validate_occurrence(
    record: &Record,
    phase: &str,
    occurrence: &crate::plugins::receipts::NonToolOccurrence,
) -> Result<()> {
    use crate::plugins::receipts::NonToolOccurrence::*;
    let id = occurrence
        .host_operation()
        .context("compaction operation missing")?;
    let c = validate(record, id, phase)?;
    if c.external_backend.is_some() {
        match occurrence {
            PreCompact { trigger, .. } => ensure!(
                trigger == &c.trigger && c.applied.is_none(),
                "source PreCompact is already applied or differs"
            ),
            PostCompact {
                trigger,
                compact_summary,
                ..
            } => ensure!(
                trigger == &c.trigger && c.applied.is_some() && compact_summary == &c.summary,
                "source PostCompact lacks actual callback"
            ),
            _ => anyhow::bail!("not a compaction event"),
        }
        return Ok(());
    }
    match occurrence {
        PreCompact { trigger, .. } => ensure!(
            trigger == &c.trigger
                && c.applied.is_none()
                && c.source == digest(checkpoint(record, phase)?)?,
            "PreCompact owner is stale or already applied"
        ),
        PostCompact {
            trigger,
            compact_summary,
            ..
        } => ensure!(
            trigger == &c.trigger
                && c.applied.as_ref() == Some(&digest(checkpoint(record, phase)?)?)
                && compact_summary == &c.summary,
            "PostCompact lacks actual applied context"
        ),
        _ => anyhow::bail!("not a compaction event"),
    }
    Ok(())
}

impl SharedRuntime {
    pub(crate) fn begin_external_compaction(
        &self,
        phase: &str,
        backend: u64,
        input: &Value,
    ) -> Result<u64> {
        self.admission(|r| {
            let own = owner(r, phase)?;
            super::plugin_non_tool::owner::validate_backend(r, phase, own.identity, backend)?;
            let identity = own.identity.clone();
            let child = own.child;
            let trigger = input["trigger"]
                .as_str()
                .context("source compaction trigger missing")?;
            ensure!(
                matches!(trigger, "auto" | "manual") && r.operations.len() < 4096,
                "source compaction bound"
            );
            ensure!(
                !r.operations.iter().any(|o| o.phase == phase
                    && matches!(o.host_invocation, Some(HostInvocation::Compaction(_)))
                    && !o.complete),
                "source compaction overlaps an unfinished transaction"
            );
            ensure!(
                r.task.is_none() || r.allocation.is_some(),
                "source task compaction requires its original allocation"
            );
            let budget = super::budget_accounting::inherited(r, backend)?;
            let id = r.operations.len() as u64 + 1;
            r.operations.push(Operation {
                id,
                phase: phase.into(),
                identity: Some(identity),
                budget: Some(budget),
                usage_receipt: None,
                verification: None,
                call: None,
                result: None,
                tool_receipt: None,
                host_invocation: Some(HostInvocation::Compaction(Compaction {
                    version: 1,
                    trigger: trigger.into(),
                    external_backend: Some(backend),
                    native_session: None,
                    task: r.task.as_ref().map(|t| t.id),
                    child_owner: child,
                    source: digest(input)?,
                    source_bytes: serde_json::to_vec(input)?.len(),
                    pins: digest(&r.plugin_activations)?,
                    model_operation: None,
                    applied: None,
                    applied_bytes: None,
                    summary: None,
                    hold: None,
                })),
                complete: false,
                reconciled: false,
                usage_reported: true,
            });
            Ok(id)
        })
    }
    pub(crate) fn observe_external_compaction(
        &self,
        id: u64,
        phase: &str,
        backend: u64,
        input: &Value,
    ) -> Result<()> {
        self.update(|r| {
            let c = validate(r, id, phase)?;
            ensure!(
                c.external_backend == Some(backend)
                    && c.applied.is_none()
                    && input["trigger"] == c.trigger,
                "source PostCompact is orphaned, repeated or changed"
            );
            let Some(HostInvocation::Compaction(c)) = &mut r
                .operations
                .iter_mut()
                .find(|o| o.id == id)
                .expect("validated")
                .host_invocation
            else {
                unreachable!()
            };
            c.applied = Some(digest(input)?);
            c.summary = input["compact_summary"].as_str().map(str::to_owned);
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
impl SharedRuntime {
    pub(crate) fn fail_compaction_sync_when(&self, predicate: fn(&Value) -> bool) {
        self.0
            .lock()
            .unwrap()
            .store
            .fail_directory_sync_when(predicate);
    }
}
