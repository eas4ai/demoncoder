//! Explicit membership shares the existing operation ledger and tool receipts.
use super::{HostInvocation, Operation, Record, SharedRuntime};
use crate::tools::{ToolCall, ToolResult};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolBatch {
    pub version: u32,
    pub wrapper: u64,
    pub members: Vec<ToolCall>,
    pub settled: bool,
    pub interrupted: bool,
    pub skipped: Vec<String>,
}

pub(super) fn validate_member(record: &Record, id: u64, call: &ToolCall) -> Result<()> {
    let operation = record
        .operations
        .iter()
        .find(|o| o.id == id)
        .context("batch missing")?;
    let Some(HostInvocation::ToolBatch(batch)) = &operation.host_invocation else {
        anyhow::bail!("not an explicit batch");
    };
    ensure!(
        !batch.settled && !batch.interrupted && !operation.reconciled,
        "batch is no longer admitting members"
    );
    ensure!(
        batch.members.iter().any(|member| member == call),
        "tool is not a declared batch member"
    );
    let wrapper = record
        .operations
        .iter()
        .find(|o| o.id == batch.wrapper)
        .context("batch wrapper missing")?;
    ensure!(
        wrapper.phase == operation.phase && !wrapper.reconciled && !wrapper.complete,
        "batch wrapper owner changed or ended"
    );
    Ok(())
}

impl SharedRuntime {
    pub(crate) fn begin_tool_batch(
        &self,
        wrapper: u64,
        mut members: Vec<ToolCall>,
    ) -> Result<(u64, Vec<ToolCall>)> {
        self.admission(|record| {
            ensure!(!record.recovery_pending, "batch owner requires reconciliation");
            let outer = record.operations.iter().find(|o| o.id == wrapper).context("batch wrapper missing")?;
            let receipt = outer.tool_receipt.as_ref().context("batch lacks tool admission")?;
            ensure!(receipt.admitted && receipt.original_call.name == "tool_batch" && !outer.complete && !outer.reconciled,
                "batch wrapper is not an active admitted operation");
            ensure!(!record.operations.iter().any(|o| matches!(&o.host_invocation, Some(HostInvocation::ToolBatch(b)) if b.wrapper == wrapper)),
                "batch already declared; never replay unfinished members");
            ensure!(record.operations.len() < 4096 && (1..=32).contains(&members.len()), "batch membership bound");
            let phase = outer.phase.clone();
            let source = record.operations.iter().find(|o| o.id == receipt.invocation).context("batch source missing")?;
            let identity = source.identity.clone();
            let budget = outer.budget.clone();
            let id = record.operations.len() as u64 + 1;
            for (index, call) in members.iter_mut().enumerate() { call.id = format!("host-batch-{id}-{index}"); }
            record.operations.push(Operation {
                id, phase, identity, budget, usage_receipt: None, verification: None,
                call: None, result: None, tool_receipt: None,
                host_invocation: Some(HostInvocation::ToolBatch(ToolBatch {
                    version: 1, wrapper, members: members.clone(), settled: false, interrupted: false, skipped: vec![],
                })), complete: false, reconciled: false, usage_reported: true,
            });
            Ok((id, members))
        })
    }

    pub(crate) fn settle_tool_batch(&self, id: u64) -> Result<Vec<ToolResult>> {
        self.update(|record| {
            let operation = record
                .operations
                .iter()
                .find(|o| o.id == id)
                .context("batch missing")?;
            let Some(HostInvocation::ToolBatch(batch)) = &operation.host_invocation else {
                anyhow::bail!("batch kind changed")
            };
            ensure!(
                !record.recovery_pending
                    && !batch.interrupted
                    && !batch.settled
                    && !operation.reconciled,
                "batch owner is held or settled"
            );
            let mut results = Vec::new();
            for member in &batch.members {
                let member_operation = record
                    .operations
                    .iter()
                    .find(|o| {
                        o.tool_receipt
                            .as_ref()
                            .is_some_and(|r| r.invocation == id && r.original_call.id == member.id)
                    })
                    .context("declared member has not executed")?;
                let receipt = member_operation.tool_receipt.as_ref().expect("matched");
                ensure!(
                    member_operation.phase == operation.phase
                        && member_operation.complete
                        && !member_operation.needs_reconciliation()
                        && receipt.observers_complete,
                    "declared member is not settled"
                );
                results.push(
                    member_operation
                        .result
                        .clone()
                        .context("settled member lacks original evidence")?,
                );
            }
            let operation = record
                .operations
                .iter_mut()
                .find(|o| o.id == id)
                .expect("validated");
            let Some(HostInvocation::ToolBatch(batch)) = &mut operation.host_invocation else {
                unreachable!()
            };
            batch.settled = true;
            operation.complete = true;
            Ok(results)
        })
    }

    pub(crate) fn interrupt_tool_batch(&self, id: u64) -> Result<()> {
        self.update(|record| {
            let members = match record
                .operations
                .iter()
                .find(|o| o.id == id)
                .map(|o| &o.host_invocation)
            {
                Some(Some(HostInvocation::ToolBatch(batch))) if !batch.settled => {
                    batch.members.clone()
                }
                _ => return Ok(()),
            };
            let skipped = members
                .iter()
                .filter(|m| {
                    !record.operations.iter().any(|o| {
                        o.tool_receipt
                            .as_ref()
                            .is_some_and(|r| r.invocation == id && r.original_call.id == m.id)
                    })
                })
                .map(|m| m.id.clone())
                .collect();
            let operation = record
                .operations
                .iter_mut()
                .find(|o| o.id == id)
                .expect("validated");
            let Some(HostInvocation::ToolBatch(batch)) = &mut operation.host_invocation else {
                unreachable!()
            };
            batch.interrupted = true;
            batch.skipped = skipped;
            // Unknown member effects remain incomplete in their original receipts.
            operation.complete = true;
            Ok(())
        })
    }
}

/// Validate the settled causal batch independently of the observer occurrence.
pub(super) fn validate_observation(
    record: &Record,
    phase: &str,
    id: u64,
    calls: &[serde_json::Value],
) -> Result<()> {
    let operation = record
        .operations
        .iter()
        .find(|o| o.id == id)
        .context("batch owner missing")?;
    let Some(HostInvocation::ToolBatch(batch)) = &operation.host_invocation else {
        anyhow::bail!("not a batch owner")
    };
    ensure!(
        operation.phase == phase
            && operation.complete
            && !operation.reconciled
            && batch.settled
            && !batch.interrupted,
        "batch has not settled"
    );
    let refs = member_operations(record, id)?
        .into_iter()
        .map(|o| serde_json::json!({"operation":o.id}))
        .collect::<Vec<_>>();
    ensure!(calls == refs, "batch member evidence changed");
    Ok(())
}
fn member_operations(record: &Record, id: u64) -> Result<Vec<&Operation>> {
    let operation = record
        .operations
        .iter()
        .find(|o| o.id == id)
        .context("batch missing")?;
    let Some(HostInvocation::ToolBatch(batch)) = &operation.host_invocation else {
        anyhow::bail!("batch kind changed")
    };
    batch
        .members
        .iter()
        .map(|m| {
            let o = record
                .operations
                .iter()
                .find(|o| {
                    o.tool_receipt
                        .as_ref()
                        .is_some_and(|r| r.invocation == id && r.original_call.id == m.id)
                })
                .context("batch member missing")?;
            ensure!(
                o.complete && !o.needs_reconciliation() && o.result.is_some(),
                "batch member is unsettled"
            );
            Ok(o)
        })
        .collect()
}
impl SharedRuntime {
    pub(crate) fn batch_occurrence(
        &self,
        id: u64,
    ) -> Result<crate::plugins::receipts::NonToolOccurrence> {
        let record = self.record()?;
        let tool_calls = member_operations(&record, id)?
            .iter()
            .map(|o| serde_json::json!({"operation":o.id}))
            .collect::<Vec<_>>();
        let phase = &record
            .operations
            .iter()
            .find(|o| o.id == id)
            .context("batch missing")?
            .phase;
        validate_observation(&record, phase, id, &tool_calls)?;
        Ok(crate::plugins::receipts::NonToolOccurrence::PostToolBatch {
            batch: Some(id),
            tool_calls,
        })
    }
    pub(crate) fn batch_input(&self, id: u64) -> Result<Vec<serde_json::Value>> {
        let record = self.record()?;
        member_operations(&record, id)?.iter().map(|o| {
            let call = o.call.as_ref().context("batch member call missing")?;
            // The operation retains the admitted rewrite; its receipt preserves the original request.
            // Denied/failed results remain original; execution flags stay in the receipt.
            Ok(serde_json::json!({"tool_name":call.name,"tool_input":call.arguments,"tool_use_id":call.id,"tool_response":o.result}))
        }).collect()
    }
}

#[cfg(test)]
mod tests;
