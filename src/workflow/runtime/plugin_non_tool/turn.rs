//! Actual native turns share the operation ledger without granting invocation allowance.
use super::{HostInvocation, Operation, Record, SharedRuntime, owner};
use crate::plugins::receipts::{NativeTurn, NativeTurnEnd, NativeTurnOrigin};
use anyhow::{Context, Result, ensure};
use std::os::unix::fs::MetadataExt;

pub(super) fn validate<'a>(record: &'a Record, id: u64, phase: &str) -> Result<&'a NativeTurn> {
    let operation = record
        .operations
        .iter()
        .find(|o| o.id == id)
        .context("native turn missing")?;
    let Some(HostInvocation::NativeTurn(turn)) = &operation.host_invocation else {
        anyhow::bail!("native turn identity points to another operation kind");
    };
    let owner = owner::resolve(record, phase)?;
    let metadata = std::fs::metadata(owner.root)?;
    ensure!(
        turn.version == 1
            && turn.end.is_none()
            && !operation.complete
            && !operation.reconciled
            && turn.owner_phase.as_deref() == Some(phase)
            && operation.phase == phase
            && operation.identity.as_ref() == Some(owner.identity)
            && turn.task == record.task.as_ref().map(|t| t.id)
            && turn.child_owner == owner.child
            && turn.workspace == (metadata.dev(), metadata.ino())
            && operation.call.is_none()
            && operation.result.is_none()
            && operation.tool_receipt.is_none(),
        "native turn owner changed or ended"
    );
    Ok(turn)
}

impl SharedRuntime {
    pub(crate) fn begin_native_turn(
        &self,
        phase: &str,
        identity: Option<&super::super::Identity>,
        origin: NativeTurnOrigin,
    ) -> Result<u64> {
        self.admission(|record| {
            let bare = phase == "worker" && record.phase.is_none();
            let owner = if bare {
                ensure!(
                    !record.recovery_pending,
                    "native turn requires reconciliation"
                );
                owner::Owner {
                    identity: &record.identity,
                    root: &record.workspace,
                    child: None,
                }
            } else {
                owner::resolve(record, phase)?
            };
            ensure!(
                identity.is_none_or(|i| i == owner.identity)
                    && (owner.child.is_none() || identity.is_some()),
                "native turn assignment differs"
            );
            let identity = owner.identity.clone();
            let metadata = std::fs::metadata(owner.root)?;
            let child_owner = owner.child;
            ensure!(
                record.operations.len() < 4096,
                "session operation history is full"
            );
            ensure!(
                bare || !record.operations.iter().any(|o| o.phase == phase
                    && matches!(o.host_invocation, Some(HostInvocation::NativeTurn(_)))
                    && o.needs_reconciliation()),
                "unfinished native turn requires reconciliation"
            );
            let id = record.operations.len() as u64 + 1;
            record.operations.push(Operation {
                id,
                phase: phase.into(),
                verification: None,
                call: None,
                result: None,
                tool_receipt: None,
                host_invocation: Some(HostInvocation::NativeTurn(NativeTurn {
                    version: 1,
                    owner_phase: (!bare).then(|| phase.to_owned()),
                    origin,
                    task: record.task.as_ref().map(|t| t.id),
                    workspace: (metadata.dev(), metadata.ino()),
                    child_owner,
                    end: None,
                })),
                complete: false,
                reconciled: false,
                usage_reported: true,
                identity: Some(identity),
            });
            Ok(id)
        })
    }
    pub(crate) fn finish_native_turn(&self, id: u64, end: NativeTurnEnd) -> Result<()> {
        self.update(|record| {
            let operation = record
                .operations
                .iter_mut()
                .find(|o| o.id == id)
                .context("native turn missing")?;
            let Some(HostInvocation::NativeTurn(turn)) = &mut operation.host_invocation else {
                anyhow::bail!("native turn kind changed");
            };
            ensure!(
                turn.end.is_none() && !operation.complete && !operation.reconciled,
                "native turn already ended or reconciled"
            );
            turn.end = Some(end);
            operation.complete = true;
            Ok(())
        })
    }
}
