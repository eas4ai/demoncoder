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
        let session = self.plugin_session()?;
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
                budget: Some(super::super::budget_accounting::capture(record, &session)),
                usage_receipt: None,
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
                    diagnostics: vec![],
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

    /// Diagnostics cannot grant execution, including after allowance exhaustion.
    pub(crate) fn native_turn_diagnostic(
        &self,
        id: u64,
        phase: &str,
        identity: Option<&super::super::Identity>,
        message: &str,
    ) -> Result<()> {
        self.update(|record| {
            let operation = record
                .operations
                .iter()
                .rev()
                .find(|o| {
                    o.phase == phase
                        && matches!(o.host_invocation, Some(HostInvocation::NativeTurn(_)))
                })
                .context("native turn missing")?;
            let Some(HostInvocation::NativeTurn(turn)) = &operation.host_invocation else {
                anyhow::bail!("native turn kind changed");
            };
            ensure!(
                operation.id == id
                    && !operation.reconciled
                    && turn.version == 1
                    && turn.diagnostics.len() < 8
                    && operation.phase == phase
                    && operation.identity.as_ref() == Some(identity.unwrap_or(&record.identity))
                    && turn.task == record.task.as_ref().map(|t| t.id)
                    && turn.end.is_none_or(|end| end == NativeTurnEnd::Failed),
                "native turn diagnostics unavailable"
            );
            if phase == "worker" {
                ensure!(
                    record.phase.as_deref() == turn.owner_phase.as_deref(),
                    "native cleanup phase changed"
                );
            } else {
                let child_id = super::delegation::agent_id(phase)
                    .context("native cleanup child phase invalid")?;
                let child = record
                    .agents
                    .iter()
                    .find(|a| a.id == child_id)
                    .context("native cleanup child missing")?;
                ensure!(
                    turn.child_owner.as_ref() == Some(&owner::child_fingerprint(record, child)?),
                    "native cleanup child assignment changed"
                );
            }
            let message = if message.len() > 4096 {
                let mut end = 4096 - " [truncated]".len();
                while !message.is_char_boundary(end) {
                    end -= 1;
                }
                format!("{} [truncated]", &message[..end])
            } else {
                message.to_owned()
            };
            let operation = record
                .operations
                .iter_mut()
                .find(|o| o.id == id)
                .expect("validated");
            let Some(HostInvocation::NativeTurn(turn)) = &mut operation.host_invocation else {
                unreachable!("validated")
            };
            turn.diagnostics.push(message);
            Ok(())
        })
    }
}
