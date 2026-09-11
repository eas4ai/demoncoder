//! Tool correlation and immutable outcomes live in the existing operation ledger.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use super::{Operation, SharedRuntime, delegation, verification_attribution};
use crate::tools::{ToolCall, ToolResult};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HostInvocation {
    Model,
    Backend,
    Commands,
    PluginService {
        owner: u64,
        service: String,
        outcome: PluginServiceOutcome,
    },
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PluginServiceOutcome {
    Pending,
    Ready,
    Failed,
    Uncertain,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolReceipt {
    pub invocation: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin_admission: Option<crate::plugins::receipts::AdmissionReceipt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin_lifecycle: Option<crate::plugins::receipts::LifecycleReceipt>,
    pub original_call: ToolCall,
    pub attempt_admitted: bool,
    pub admitted: bool,
    pub effect_started: bool,
    pub observers_complete: bool,
    pub observer_pending: Option<usize>,
    pub observer_error: Option<String>,
    pub presentations: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_result: Option<ToolResult>,
    #[serde(default)]
    pub model_result_settled: bool,
}

pub(crate) enum ToolAdmission {
    Fresh(u64),
    Denied(u64, ToolResult),
    Replay(ToolResult),
    Held(&'static str),
}

impl Operation {
    pub(super) fn needs_reconciliation(&self) -> bool {
        !self.reconciled
            && (!self.complete
                || self.tool_receipt.as_ref().is_some_and(|receipt| {
                    !receipt.observers_complete
                        || receipt.plugin_lifecycle.as_ref().is_some_and(|plan| {
                            !plan.settled
                                || matches!(
                            plan.delivery,
                            crate::plugins::receipts::PostDelivery::LocalPending
                                | crate::plugins::receipts::PostDelivery::Staged
                                | crate::plugins::receipts::PostDelivery::Reserved
                                | crate::plugins::receipts::PostDelivery::Superseding
                                | crate::plugins::receipts::PostDelivery::Superseded
                                | crate::plugins::receipts::PostDelivery::CorrectionReserved { .. }
                        ) || plan.hooks.iter().any(|h| h.uncertain_effects)
                        })
                        || receipt.plugin_admission.as_ref().is_some_and(|plan| {
                            plan.hooks.iter().any(|hook| hook.uncertain_effects)
                        })
                }))
    }
    pub fn model_result(&self) -> Option<&ToolResult> {
        self.tool_receipt
            .as_ref()
            .and_then(|receipt| receipt.model_result.as_ref())
            .or(self.result.as_ref())
    }
}

impl SharedRuntime {
    /// A host-selected check batch has identity, but is not a model/backend call.
    pub(crate) fn begin_commands(
        &self,
        phase: &str,
        identity: Option<&super::Identity>,
    ) -> Result<u64> {
        self.admission(|record| {
            super::plugin_lifecycle::ensure_continuation(record, phase)?;
            delegation::ensure_agent_active(record, phase)?;
            ensure!(
                !record.recovery_pending,
                "uncertain work needs reconciliation before command admission"
            );
            ensure!(
                record.operations.len() < 4096,
                "session operation history is full"
            );
            let verification = verification_attribution(record, phase)?;
            let id = record.operations.len() as u64 + 1;
            record.operations.push(Operation {
                id,
                phase: phase.into(),
                verification,
                identity: identity.cloned().or_else(|| Some(record.identity.clone())),
                call: None,
                result: None,
                tool_receipt: None,
                host_invocation: Some(HostInvocation::Commands),
                complete: true,
                reconciled: false,
                usage_reported: true,
            });
            Ok(id)
        })
    }

    pub(crate) fn begin_tool(
        &self,
        phase: &str,
        invocation: u64,
        call: &ToolCall,
    ) -> Result<ToolAdmission> {
        ensure!(
            !call.id.is_empty() && call.id.len() <= 256,
            "invalid tool call identity"
        );
        ensure!(
            !call.name.is_empty() && call.name.len() <= 64,
            "invalid tool name"
        );
        ensure!(
            serde_json::to_vec(&call.arguments)?.len() <= 1024 * 1024,
            "tool arguments exceed 1 MiB"
        );
        let admission = self.update(|record| {
            let source = record.operations.iter().find(|operation| operation.id == invocation)
                .context("tool requires a durable host invocation")?;
            ensure!(source.phase == phase && source.call.is_none() && matches!(source.host_invocation, Some(HostInvocation::Model | HostInvocation::Backend | HostInvocation::Commands)), "tool invocation belongs to another owner or phase, or lacks host identity");
            if let Some(operation) = record.operations.iter().find(|operation| {
                operation.tool_receipt.as_ref().is_some_and(|receipt| receipt.invocation == invocation && receipt.original_call.id == call.id)
            }) {
                let receipt = operation.tool_receipt.as_ref().expect("matched receipt");
                let reason = if receipt.original_call.name != call.name || receipt.original_call.arguments != call.arguments {
                    "tool correlation changed its original request; execution is held"
                } else if receipt.observers_complete && receipt.observer_error.is_none()
                    && receipt.plugin_lifecycle.as_ref().is_none_or(|p|p.settled && matches!(p.delivery,crate::plugins::receipts::PostDelivery::Local|crate::plugins::receipts::PostDelivery::Acknowledged) && !matches!(p.continuation,crate::plugins::receipts::PostContinuation::Held{..})) {
                    return Ok(ToolAdmission::Replay(operation.model_result().context("settled tool has no original result")?.clone()));
                } else {
                    "tool invocation already admitted; inspect its retained result and unfinished observers before continuing"
                };
                record.recovery_pending = true;
                return Ok(ToolAdmission::Held(reason));
            }
            super::plugin_lifecycle::ensure_continuation(record, phase)?;
            delegation::ensure_agent_active(record, phase)?;
            ensure!(!record.recovery_pending, "uncertain work needs reconciliation before tool admission");
            ensure!(record.operations.len() < 4096, "session operation history is full");
            let verification = verification_attribution(record, phase)?;
            // Clock uncertainty is an error, not a known budget denial.
            let denial = if let Some(allocation) = &record.allocation {
                if allocation.remaining_ms()? == 0 {
                    Some("cumulative task deadline exhausted")
                } else if allocation.tool_calls >= allocation.limits.tool_calls {
                    Some("cumulative task tool-call allowance exhausted")
                } else { None }
            } else { None };
            let denied = denial.map(|reason| ToolResult {
                call_id: call.id.clone(), tool: call.name.clone(), success: false,
                output: reason.into(), exit_code: None,
            });
            // Reserve the attempt before gates can have effects. A failed gate
            // still spent this attempt; settled retries spend nothing.
            let mut allocation = record.allocation.clone();
            if denied.is_none() && let Some(allocation) = &mut allocation { allocation.admit(false)?; }
            let id = record.operations.len() as u64 + 1;
            record.allocation = allocation;
            record.operations.push(Operation {
                id, phase: phase.into(), verification, identity: None,
                call: Some(call.clone()), result: denied.clone(), complete: denied.is_some(),
                reconciled: false, usage_reported: false,
                host_invocation: None,
                tool_receipt: Some(ToolReceipt {
                    invocation, plugin_admission: None, plugin_lifecycle: None, original_call: call.clone(), attempt_admitted: denied.is_none(), admitted: false,
                    effect_started: false, observers_complete: denied.is_some(),
                    observer_pending: None, observer_error: None, presentations: Vec::new(), model_result: None,
                    model_result_settled: denied.is_some(),
                }),
            });
            Ok(match denied { Some(result) => ToolAdmission::Denied(id, result), None => ToolAdmission::Fresh(id) })
        })?;
        if matches!(admission, ToolAdmission::Fresh(_)) {
            // Persisted clock rollback also withholds the captured attempt.
            ensure!(
                !self.remaining()?.is_zero(),
                "cumulative task deadline exhausted"
            );
        }
        Ok(admission)
    }

    pub(crate) fn admit_tool(&self, id: u64, call: &ToolCall) -> Result<()> {
        let session = self.plugin_session()?;
        self.admission(|record| {
            let operation = record
                .operations
                .iter()
                .find(|operation| operation.id == id)
                .context("tool request was not retained")?;
            let receipt = operation
                .tool_receipt
                .as_ref()
                .context("tool request lacks durable correlation")?;
            ensure!(
                !receipt.admitted && operation.result.is_none(),
                "tool is already admitted"
            );
            ensure!(
                receipt.original_call.id == call.id && receipt.original_call.name == call.name,
                "hooks cannot change tool identity"
            );
            ensure!(
                !record.recovery_pending,
                "uncertain work needs reconciliation before tool admission"
            );
            delegation::ensure_agent_active(record, &operation.phase)?;
            super::plugin_admission::validate_final_key(operation, call, &session)?;
            let operation = record
                .operations
                .iter_mut()
                .find(|operation| operation.id == id)
                .expect("validated operation");
            operation.call = Some(call.clone());
            operation
                .tool_receipt
                .as_mut()
                .expect("validated receipt")
                .admitted = true;
            Ok(())
        })
    }

    /// A later host policy/Oracle refusal is not an executed tool failure.
    pub(crate) fn refuse_tool_execution(&self, id: u64) -> Result<()> {
        self.update(|record| {
            let operation = record
                .operations
                .iter_mut()
                .find(|o| o.id == id)
                .context("tool operation missing")?;
            ensure!(
                operation.result.is_none(),
                "completed tool admission cannot be revoked"
            );
            let receipt = operation
                .tool_receipt
                .as_mut()
                .context("tool receipt missing")?;
            ensure!(
                receipt.admitted && !receipt.effect_started && receipt.plugin_lifecycle.is_none(),
                "policy refusal followed executed tool work"
            );
            receipt.admitted = false;
            Ok(())
        })
    }

    pub(crate) fn tool_effect(&self, id: u64) -> Result<()> {
        let session = self.plugin_session()?;
        self.admission(|record| {
            let operation = record
                .operations
                .iter()
                .find(|operation| operation.id == id)
                .context("tool admission missing")?;
            let receipt = operation
                .tool_receipt
                .as_ref()
                .context("tool correlation missing")?;
            ensure!(
                receipt.admitted && operation.result.is_none(),
                "tool effect lacks an active admission"
            );
            ensure!(
                !record.recovery_pending,
                "uncertain work needs reconciliation before tool effect"
            );
            delegation::ensure_agent_active(record, &operation.phase)?;
            super::plugin_admission::validate_final_key(
                operation,
                operation
                    .call
                    .as_ref()
                    .context("admitted tool call missing")?,
                &session,
            )?;
            if let Some(allocation) = &record.allocation {
                ensure!(
                    allocation.remaining_ms()? > 0,
                    "cumulative task deadline exhausted"
                );
            }
            record
                .operations
                .iter_mut()
                .find(|operation| operation.id == id)
                .expect("validated operation")
                .tool_receipt
                .as_mut()
                .expect("validated receipt")
                .effect_started = true;
            Ok(())
        })
    }

    pub(crate) fn original_tool_result(&self, id: u64, result: &ToolResult) -> Result<()> {
        ensure!(
            result.output.len() <= 2 * 1024 * 1024,
            "tool result exceeds retention limit"
        );
        self.update(|record| {
            let operation = record
                .operations
                .iter_mut()
                .find(|operation| operation.id == id)
                .context("tool request missing")?;
            let receipt = operation
                .tool_receipt
                .as_ref()
                .context("tool correlation missing")?;
            ensure!(
                result.call_id == receipt.original_call.id
                    && result.tool == receipt.original_call.name,
                "tool outcome changed identity"
            );
            if let Some(original) = &operation.result {
                ensure!(
                    original == result,
                    "original tool result cannot be replaced"
                );
                return Ok(());
            }
            ensure!(
                !result.success || receipt.effect_started,
                "successful tool lacks an admitted effect"
            );
            operation.result = Some(result.clone());
            operation.complete = true;
            Ok(())
        })
    }

    pub(crate) fn model_tool_result(&self, id: u64, result: &ToolResult) -> Result<()> {
        ensure!(
            result.output.len() <= 2 * 1024 * 1024,
            "model-facing tool result exceeds retention limit"
        );
        self.update(|record| {
            let operation = record
                .operations
                .iter_mut()
                .find(|operation| operation.id == id)
                .context("tool request missing")?;
            let original = operation
                .result
                .as_ref()
                .context("original tool result missing")?;
            ensure!(
                result.call_id == original.call_id
                    && result.tool == original.tool
                    && result.success == original.success
                    && result.exit_code == original.exit_code,
                "diagnostics cannot change original tool outcome"
            );
            let receipt = operation
                .tool_receipt
                .as_mut()
                .context("tool correlation missing")?;
            if receipt.model_result_settled {
                ensure!(
                    receipt.model_result.as_ref().unwrap_or(original) == result,
                    "settled model-facing result cannot be replaced"
                );
            } else if original.output != result.output {
                receipt.model_result = Some(result.clone());
            }
            receipt.model_result_settled = true;
            Ok(())
        })
    }

    pub(crate) fn tool_observer(
        &self,
        id: u64,
        index: usize,
        outcome: Option<Result<&str, &str>>,
    ) -> Result<()> {
        let transition = |record: &mut super::Record| {
            if outcome.is_none() {
                ensure!(
                    !record.recovery_pending,
                    "uncertain work needs reconciliation before observer"
                );
                let operation = record
                    .operations
                    .iter()
                    .find(|operation| operation.id == id)
                    .context("tool request missing")?;
                delegation::ensure_agent_active(record, &operation.phase)?;
                if let Some(allocation) = &record.allocation {
                    ensure!(
                        allocation.remaining_ms()? > 0,
                        "cumulative task deadline exhausted"
                    );
                }
            }
            let operation = record
                .operations
                .iter_mut()
                .find(|operation| operation.id == id)
                .context("tool request missing")?;
            ensure!(
                operation.result.is_some(),
                "observer requires original tool evidence"
            );
            let receipt = operation
                .tool_receipt
                .as_mut()
                .context("tool correlation missing")?;
            ensure!(
                index < 32 && !receipt.observers_complete,
                "invalid tool observer admission"
            );
            match outcome {
                None => {
                    ensure!(
                        receipt.observer_pending.is_none() && receipt.presentations.len() == index,
                        "observer already admitted"
                    );
                    receipt.observer_pending = Some(index);
                    #[cfg(test)]
                    tests::invalidate_observer_checkpoint(record);
                }
                Some(outcome) => {
                    ensure!(
                        receipt.observer_pending == Some(index),
                        "observer outcome lacks admission"
                    );
                    let text = match outcome {
                        Ok(text) | Err(text) => text,
                    };
                    ensure!(
                        text.len() <= 1024 * 1024,
                        "tool observer output exceeds 1 MiB"
                    );
                    match outcome {
                        Ok(text) => {
                            receipt.presentations.push(text.into());
                            receipt.observer_pending = None;
                        }
                        Err(text) => {
                            receipt.observer_error = Some(text.into());
                            record.recovery_pending = true;
                        }
                    }
                }
            }
            Ok(())
        };
        if outcome.is_none() {
            self.admission(transition)
        } else {
            self.update(transition)
        }
    }

    pub(crate) fn settle_tool(&self, id: u64) -> Result<()> {
        self.update(|record| {
            let operation = record
                .operations
                .iter_mut()
                .find(|operation| operation.id == id)
                .context("tool request missing")?;
            ensure!(operation.result.is_some(), "tool original result missing");
            let receipt = operation
                .tool_receipt
                .as_mut()
                .context("tool correlation missing")?;
            ensure!(
                receipt.model_result_settled
                    && receipt.plugin_lifecycle.as_ref().is_none_or(|p| p.settled)
                    && receipt.observer_pending.is_none()
                    && receipt.observer_error.is_none(),
                "tool observers are not settled"
            );
            receipt.observers_complete = true;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests;
