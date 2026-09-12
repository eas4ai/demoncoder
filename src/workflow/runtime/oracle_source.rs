//! Oracle requests remain auxiliary work of one admitted, live tool.
use super::{BudgetRef, HostInvocation, Record, SharedRuntime, budget_accounting, delegation};
use anyhow::{Context, Result, ensure};

#[derive(Clone)]
pub(crate) struct OracleSource {
    operation: u64,
    invocation: u64,
    phase: String,
    budget: BudgetRef,
}

impl OracleSource {
    pub(super) fn operation(&self) -> u64 {
        self.operation
    }

    pub(super) fn validate(
        &self,
        record: &Record,
        session: &str,
        phase: &str,
    ) -> Result<BudgetRef> {
        let expected = if self.phase.starts_with("agent:") {
            format!("{}:oracle", self.phase)
        } else {
            "oracle".into()
        };
        ensure!(phase == expected, "Oracle source belongs to another phase");
        ensure!(
            !record.recovery_pending,
            "Oracle source requires reconciliation"
        );
        super::plugin_lifecycle::ensure_continuation(record, &self.phase)?;
        delegation::ensure_agent_active(record, &self.phase)?;
        let tool = record
            .operations
            .iter()
            .find(|o| o.id == self.operation)
            .context("Oracle tool owner missing")?;
        let receipt = tool
            .tool_receipt
            .as_ref()
            .context("Oracle tool receipt missing")?;
        ensure!(
            tool.phase == self.phase
                && !tool.complete
                && !tool.reconciled
                && tool.result.is_none()
                && receipt.invocation == self.invocation
                && receipt.attempt_admitted
                && receipt.admitted
                && !receipt.observers_complete,
            "Oracle tool owner changed or ended"
        );
        // Native responses settle before returned tools execute; command hosts
        // are complete at creation. The pending admitted tool is the live owner.
        let host = record
            .operations
            .iter()
            .find(|o| o.id == self.invocation)
            .context("Oracle host invocation missing")?;
        ensure!(
            host.phase == self.phase
                && !host.reconciled
                && host.call.is_none()
                && matches!(
                    host.host_invocation,
                    Some(
                        HostInvocation::Model | HostInvocation::Backend | HostInvocation::Commands
                    )
                )
                && host.budget.as_ref() == Some(&self.budget)
                && tool.budget.as_ref() == Some(&self.budget),
            "Oracle host invocation changed or ended"
        );
        super::plugin_admission::validate_final_key(
            tool,
            tool.call.as_ref().context("Oracle admitted call missing")?,
            session,
        )?;
        if let Some(allocation) = budget_accounting::active(record, session, &self.budget)? {
            ensure!(
                allocation.remaining_ms()? > 0,
                "Oracle allocation deadline exhausted"
            );
        }
        Ok(self.budget.clone())
    }
}

impl SharedRuntime {
    pub(crate) fn oracle_source(
        &self,
        phase: &str,
        operation: u64,
        invocation: u64,
        child_phase: &str,
    ) -> Result<OracleSource> {
        let session = self.plugin_session()?;
        self.admission(|record| {
            let source = OracleSource {
                operation,
                invocation,
                phase: phase.into(),
                budget: budget_accounting::inherited(record, operation)?,
            };
            source.validate(record, &session, child_phase)?;
            Ok(source)
        })
    }
}
