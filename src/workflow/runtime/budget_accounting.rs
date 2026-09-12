//! Budget references preserve attribution after execution authority ends.
use super::{Allocation, HostInvocation, Operation, Record, SharedRuntime};
use crate::{events::Event, workflow::allocation::Usage};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

pub const MAX_RETIRED_ALLOCATIONS: usize = 32;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub enum BudgetRef {
    Task { session: String, epoch: u64 },
    SessionHooks { session: String },
    Unallocated,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RetiredTaskAllocation {
    pub epoch: u64,
    pub allocation: Allocation,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub enum UnresolvedAttribution {
    LegacyBudget,
    MissingBudget,
    InvalidRecipient,
}

/// One bounded rollup per operation (or session for reports without a recipient).
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UsageReceipt {
    pub usage: Usage,
    pub reports: u64,
    pub missing_report: bool,
    pub unresolved: Option<UnresolvedAttribution>,
}

pub(super) fn capture(record: &Record, session: &str) -> BudgetRef {
    if record.allocation.is_some() {
        BudgetRef::Task {
            session: session.into(),
            epoch: record.task_allocation_epoch,
        }
    } else {
        BudgetRef::Unallocated
    }
}

pub(super) fn inherited(record: &Record, source: u64) -> Result<BudgetRef> {
    record
        .operations
        .iter()
        .find(|o| o.id == source)
        .context("budget source operation missing")?
        .budget
        .clone()
        .context("legacy operation has no provable budget reference; start a new operation")
}

/// Admission searches only the currently active grant. Retired grants cannot execute.
pub(super) fn active<'a>(
    record: &'a Record,
    session: &str,
    budget: &BudgetRef,
) -> Result<Option<&'a Allocation>> {
    match budget {
        BudgetRef::Unallocated => Ok(None),
        BudgetRef::Task {
            session: owner,
            epoch,
        } => {
            ensure!(
                owner == session && *epoch == record.task_allocation_epoch,
                "operation belongs to another session or retired task allocation"
            );
            Ok(Some(
                record
                    .allocation
                    .as_ref()
                    .context("operation task allocation ended")?,
            ))
        }
        BudgetRef::SessionHooks { session: owner } => {
            ensure!(owner == session, "operation belongs to another session");
            Ok(Some(
                &record
                    .session_hook_allowance
                    .as_ref()
                    .context("session hook allowance missing")?
                    .allocation,
            ))
        }
    }
}

pub(super) fn active_operation<'a>(
    record: &'a Record,
    session: &str,
    operation: &Operation,
) -> Result<Option<&'a Allocation>> {
    active(
        record,
        session,
        operation
            .budget
            .as_ref()
            .context("operation budget attribution is unknown")?,
    )
}

/// Debit a copy; failed admission cannot partially mutate counters or clocks.
pub(super) fn admit(
    record: &mut Record,
    session: &str,
    budget: &BudgetRef,
    model: bool,
) -> Result<()> {
    if let Some(allocation) = active(record, session, budget)? {
        let mut staged = allocation.clone();
        staged.admit(model)?;
        match budget {
            BudgetRef::Task { .. } => record.allocation = Some(staged),
            BudgetRef::SessionHooks { .. } => {
                record
                    .session_hook_allowance
                    .as_mut()
                    .expect("validated")
                    .allocation = staged
            }
            BudgetRef::Unallocated => unreachable!(),
        }
    }
    Ok(())
}

pub(super) fn validate_retirement(record: &Record, session: &str) -> Result<bool> {
    let mut epochs = std::collections::BTreeSet::new();
    ensure!(
        record.retired_task_allocations.len() <= MAX_RETIRED_ALLOCATIONS,
        "retired allocation history exceeds its limit; preserve this session and start another"
    );
    for retired in &record.retired_task_allocations {
        ensure!(
            epochs.insert(retired.epoch)
                && (record.allocation.is_none() || retired.epoch != record.task_allocation_epoch),
            "duplicate retired allocation epoch; preserve this session and start another"
        );
    }
    let referenced = record.allocation.is_some()
        && record.operations.iter().any(|o| {
            matches!(&o.budget, Some(BudgetRef::Task { session: owner, epoch })
            if owner == session && *epoch == record.task_allocation_epoch)
        });
    ensure!(
        !referenced || record.retired_task_allocations.len() < MAX_RETIRED_ALLOCATIONS,
        "retired allocation history is full (32); preserve this session and start another"
    );
    Ok(referenced)
}

pub(super) fn retire(record: &mut Record, session: &str) -> Result<()> {
    let referenced = validate_retirement(record, session)?;
    if let Some(mut allocation) = record.allocation.take() {
        allocation.checkpoint_time();
        if referenced {
            record.retired_task_allocations.push(RetiredTaskAllocation {
                epoch: record.task_allocation_epoch,
                allocation,
            });
        }
    }
    Ok(())
}

pub(super) fn replace(record: &mut Record, session: &str, allocation: Allocation) -> Result<()> {
    let epoch = record
        .task_allocation_epoch
        .checked_add(1)
        .context("task allocation epoch exhausted; preserve this session and start another")?;
    ensure!(
        !record
            .retired_task_allocations
            .iter()
            .any(|r| r.epoch >= epoch),
        "retired allocation epoch conflicts with replacement"
    );
    retire(record, session)?;
    record.task_allocation_epoch = epoch;
    record.allocation = Some(allocation);
    Ok(())
}

pub(super) fn is_model(operation: &Operation) -> bool {
    operation.call.is_none()
        && matches!(
            operation.host_invocation,
            Some(HostInvocation::Model | HostInvocation::Backend)
        )
}

fn settlement<'a>(
    record: &'a mut Record,
    session: &str,
    budget: Option<&BudgetRef>,
) -> std::result::Result<Option<&'a mut Allocation>, UnresolvedAttribution> {
    match budget {
        None => Err(UnresolvedAttribution::LegacyBudget),
        Some(BudgetRef::Unallocated) => Ok(None),
        Some(BudgetRef::SessionHooks { session: owner }) if owner == session => record
            .session_hook_allowance
            .as_mut()
            .map(|a| Some(&mut a.allocation))
            .ok_or(UnresolvedAttribution::MissingBudget),
        Some(BudgetRef::Task {
            session: owner,
            epoch,
        }) if owner == session => {
            let current = record.task_allocation_epoch == *epoch && record.allocation.is_some();
            let count = record
                .retired_task_allocations
                .iter()
                .filter(|r| r.epoch == *epoch)
                .count();
            if usize::from(current) + count != 1 {
                return Err(UnresolvedAttribution::MissingBudget);
            }
            if current {
                Ok(record.allocation.as_mut())
            } else {
                Ok(record
                    .retired_task_allocations
                    .iter_mut()
                    .find(|r| r.epoch == *epoch)
                    .map(|r| &mut r.allocation))
            }
        }
        Some(_) => Err(UnresolvedAttribution::MissingBudget),
    }
}

fn add_report(usage: &mut Usage, event: &Event) -> Result<()> {
    let Event::Usage {
        input,
        output,
        cached,
        cost_usd,
    } = event
    else {
        anyhow::bail!("expected a usage report");
    };
    usage.add(*input, *output, *cached, *cost_usd)
}

fn observe(
    record: &mut Record,
    session: &str,
    phase: &str,
    invocation: Option<u64>,
    event: &Event,
) -> Result<()> {
    let index = invocation.and_then(|id| {
        record
            .operations
            .iter()
            .position(|o| o.id == id && o.phase == phase && is_model(o))
    });
    let Some(index) = index else {
        let mut receipt = record.unattributed_usage.clone().unwrap_or_default();
        add_report(&mut receipt.usage, event)?;
        receipt.reports = receipt
            .reports
            .checked_add(1)
            .context("usage report count overflow")?;
        receipt.unresolved = Some(UnresolvedAttribution::InvalidRecipient);
        record.unattributed_usage = Some(receipt);
        return Ok(());
    };
    let budget = record.operations[index].budget.clone();
    let mut receipt = record.operations[index]
        .usage_receipt
        .clone()
        .unwrap_or_default();
    add_report(&mut receipt.usage, event)?;
    receipt.reports = receipt
        .reports
        .checked_add(1)
        .context("usage report count overflow")?;
    // All fallible receipt work precedes accounting. Usage::add itself is atomic.
    match settlement(record, session, budget.as_ref()) {
        Ok(Some(allocation)) => add_report(&mut allocation.usage, event)?,
        Ok(None) => {}
        Err(reason) => receipt.unresolved = Some(reason),
    }
    record.operations[index].usage_receipt = Some(receipt);
    record.operations[index].usage_reported = true;
    Ok(())
}

pub(crate) fn mark_missing(
    record: &mut Record,
    session: &str,
    select: impl Fn(&Operation) -> bool,
) {
    let indices: Vec<_> = record
        .operations
        .iter()
        .enumerate()
        .filter(|(_, o)| {
            select(o)
                && !o.usage_reported
                && (is_model(o) || (o.host_invocation.is_none() && o.call.is_none()))
        })
        .map(|(i, _)| i)
        .collect();
    for index in indices {
        let budget = record.operations[index].budget.clone();
        let legacy = !is_model(&record.operations[index]);
        let result = if legacy {
            Err(UnresolvedAttribution::LegacyBudget)
        } else {
            settlement(record, session, budget.as_ref())
        };
        let unresolved = match result {
            Ok(Some(allocation)) => {
                allocation.usage.uncertain();
                None
            }
            Ok(None) => None,
            Err(reason) => Some(reason),
        };
        let receipt = record.operations[index]
            .usage_receipt
            .get_or_insert_with(Default::default);
        receipt.missing_report = true;
        receipt.usage.uncertain();
        if unresolved.is_some() {
            receipt.unresolved = unresolved;
        }
    }
}

impl SharedRuntime {
    pub(crate) fn operation_budget(&self, source: u64) -> Result<BudgetRef> {
        let runtime = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("runtime lock failed"))?;
        ensure!(!runtime.failed, "session persistence failed");
        inherited(&runtime.record, source)
    }
    pub(crate) fn budget_remaining(&self, budget: &BudgetRef) -> Result<std::time::Duration> {
        let session = self.plugin_session()?;
        let runtime = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("runtime lock failed"))?;
        ensure!(!runtime.failed, "session persistence failed");
        Ok(std::time::Duration::from_millis(
            active(&runtime.record, &session, budget)?
                .map_or(Ok(86400 * 1000), Allocation::remaining_ms)?,
        ))
    }
    pub(crate) fn observe_invocation(
        &self,
        event: &Event,
        phase: &str,
        invocation: Option<u64>,
    ) -> Result<()> {
        if matches!(event, Event::Usage { .. }) {
            let session = self.plugin_session()?;
            self.update(|record| observe(record, &session, phase, invocation, event))
        } else {
            self.observe_from(event, phase, invocation)
        }
    }
}
