//! Typed host lifecycle boundaries in the existing durable operation ledger.
use super::{HostInvocation, Operation, Record, SharedRuntime, delegation};
use crate::plugins::{hook_types::HookEvent, receipts::*};
use anyhow::{Context, Result, ensure};
mod owner;
mod turn;

pub(super) fn active(record: &Record, id: u64, event: HookEvent) -> Result<&NonToolReceipt> {
    let operation = record
        .operations
        .iter()
        .find(|o| o.id == id)
        .context("lifecycle operation missing")?;
    let receipt = operation
        .non_tool_receipt()
        .context("operation is not a host lifecycle occurrence")?;
    ensure!(
        !record.recovery_pending
            && !operation.reconciled
            && !operation.complete
            && !receipt.settled,
        "lifecycle owner is held, settled or reconciled"
    );
    ensure!(
        operation.call.is_none() && operation.result.is_none() && operation.tool_receipt.is_none(),
        "lifecycle operation contains conflicting tool authority"
    );
    ensure!(
        receipt.version == 1
            && receipt.facts.subject.version == 1
            && receipt.facts.subject.occurrence.event() == event
            && receipt.facts.operation == id,
        "lifecycle event or version changed"
    );
    owner::validate(record, operation, receipt)?;
    Ok(receipt)
}

impl SharedRuntime {
    fn non_tool_admission<T>(
        &self,
        event: HookEvent,
        f: impl FnOnce(&mut Record) -> Result<T>,
    ) -> Result<T> {
        if matches!(event, HookEvent::SessionStart | HookEvent::SessionEnd) {
            self.update(f)
        } else {
            self.admission(f)
        }
    }
    pub(crate) fn ensure_source_continuation(&self, id: u64, backend: u64) -> Result<()> {
        self.validate_source_continuation(id, backend, SourceDelivery::Sent)
    }
    pub(crate) fn prepare_source_continuation(&self, id: u64, backend: u64) -> Result<()> {
        self.validate_source_continuation(id, backend, SourceDelivery::Pending)
    }
    fn validate_source_continuation(
        &self,
        id: u64,
        backend: u64,
        expected: SourceDelivery,
    ) -> Result<()> {
        let record = self.record()?;
        let operation = record
            .operations
            .iter()
            .find(|o| o.id == id)
            .context("source lifecycle receipt missing")?;
        let receipt = operation
            .non_tool_receipt()
            .context("source lifecycle receipt missing")?;
        owner::validate(&record, operation, receipt)?;
        ensure!(
            receipt.settled
                && receipt.source_delivery == Some(expected)
                && receipt
                    .facts
                    .callback
                    .as_ref()
                    .is_some_and(|c| c.backend_operation == backend),
            "source lifecycle release lacks exact settled owner"
        );
        let post = owner::validate_source(
            &record,
            &operation.phase,
            receipt
                .facts
                .callback
                .as_ref()
                .context("source callback missing")?,
            &receipt.facts.subject.occurrence,
            receipt
                .facts
                .source
                .as_ref()
                .context("source callback input missing")?,
        )?;
        super::plugin_lifecycle::ensure_continuation_except(
            &record,
            &operation.phase,
            post,
            Some(id),
        )
    }

    pub(crate) fn source_lifecycle_delivery(
        &self,
        id: u64,
        backend: u64,
        sent: bool,
    ) -> Result<()> {
        self.update(|record| {
            let operation = record
                .operations
                .iter()
                .find(|o| o.id == id)
                .context("source lifecycle receipt missing")?;
            let receipt = operation
                .non_tool_receipt()
                .context("source lifecycle receipt missing")?;
            owner::validate(record, operation, receipt)?;
            ensure!(
                receipt.settled
                    && receipt
                        .facts
                        .callback
                        .as_ref()
                        .is_some_and(|c| c.backend_operation == backend),
                "source lifecycle owner differs or is unfinished"
            );
            let expected = if sent {
                SourceDelivery::Pending
            } else {
                SourceDelivery::Sent
            };
            ensure!(
                receipt.source_delivery.as_ref() == Some(&expected),
                "source lifecycle delivery repeated or out of order"
            );
            let receipt = record
                .operations
                .iter_mut()
                .find(|o| o.id == id)
                .expect("validated")
                .non_tool_receipt_mut()
                .expect("validated");
            receipt.source_delivery = Some(if sent {
                SourceDelivery::Sent
            } else {
                SourceDelivery::Acknowledged
            });
            Ok(())
        })
    }
    pub(crate) fn non_tool_model_context(
        &self,
        id: u64,
        event: HookEvent,
    ) -> Result<crate::plugins::hook_types::ModelCallContext> {
        let record = self.record()?;
        active(&record, id, event)?;
        if matches!(event, HookEvent::SessionStart | HookEvent::SessionEnd) {
            return Ok(crate::plugins::hook_types::ModelCallContext {
                allocation_available: false,
                correction_available: false,
                ..Default::default()
            });
        }
        Ok(crate::plugins::hook_types::ModelCallContext {
            allocation_available: record.allocation.as_ref().is_some_and(|a| {
                a.remaining_ms().is_ok_and(|ms| ms > 0)
                    && a.model_calls < a.limits.model_calls
                    && a.tool_calls < a.limits.tool_calls
            }),
            correction_available: event != HookEvent::StopFailure
                && super::plugin_lifecycle::owner::non_tool_available(
                    &record,
                    &active(&record, id, event)?.facts,
                ),
            ..Default::default()
        })
    }
    pub(crate) fn settle_non_tool(
        &self,
        id: u64,
        event: HookEvent,
        effects: crate::plugins::non_tool::NonToolEffects,
    ) -> Result<()> {
        self.update(|target| {
            let mut record = target.clone();
            let receipt = active(&record, id, event)?;
            super::plugin_once::validate_skips(&record, &receipt.once_skips)?;
            ensure!(
                !receipt.hooks.iter().any(|h| h.unresolved_effects()),
                "lifecycle effects uncertain; reconcile before continuing"
            );
            let operation = record
                .operations
                .iter_mut()
                .find(|o| o.id == id)
                .expect("validated");
            let receipt = operation.non_tool_receipt_mut().expect("validated");
            receipt.proposals = effects.proposals;
            receipt.messages = effects.messages;
            receipt.diagnostics = effects.diagnostics;
            receipt.hold = effects.hold;
            receipt.correction_required = effects.correction;
            receipt.settled = true;
            super::plugin_once::settle_hooks(
                &mut receipt.hooks,
                &receipt.proposals,
                &effects.successful,
            );
            ensure!(
                serde_json::to_vec(receipt)?.len() <= 6 * 1024 * 1024,
                "lifecycle retention limit"
            );
            operation.complete = true;
            *target = record;
            Ok(())
        })
    }
    pub(crate) fn admit_non_tool_correction(&self, id: u64) -> Result<()> {
        self.admission(|target| {
            let mut record = target.clone();
            let operation = record
                .operations
                .iter()
                .find(|o| o.id == id)
                .context("Stop owner missing")?;
            let receipt = operation
                .non_tool_receipt()
                .context("Stop receipt missing")?;
            ensure!(
                !record.recovery_pending
                    && !operation.reconciled
                    && receipt.facts.subject.occurrence.event() == HookEvent::Stop
                    && receipt.settled
                    && receipt.hold.is_none()
                    && receipt.correction_required
                    && !receipt.correction_admitted
                    && receipt.facts.task == record.task.as_ref().map(|t| t.id),
                "Stop correction owner is held, changed or spent"
            );
            ensure!(
                record
                    .allocation
                    .as_ref()
                    .is_some_and(|a| a.remaining_ms().is_ok_and(|ms| ms > 0)
                        && a.model_calls < a.limits.model_calls
                        && a.tool_calls < a.limits.tool_calls),
                "Stop correction lacks remaining task allocation"
            );
            owner::validate(&record, operation, receipt)?;
            let facts = receipt.facts.clone();
            super::plugin_lifecycle::owner::charge_non_tool(&mut record, &facts)?;
            record
                .operations
                .iter_mut()
                .find(|o| o.id == id)
                .expect("validated")
                .non_tool_receipt_mut()
                .expect("validated")
                .correction_admitted = true;
            *target = record;
            Ok(())
        })
    }
    #[cfg(test)]
    pub(crate) fn begin_non_tool(
        &self,
        phase: &str,
        occurrence: NonToolOccurrence,
        plan: String,
        declarations: Vec<serde_json::Value>,
    ) -> Result<NonToolFacts> {
        self.begin_non_tool_as(phase, None, None, occurrence, plan, declarations)
    }
    #[cfg(test)]
    pub(crate) fn begin_non_tool_as(
        &self,
        phase: &str,
        identity: Option<&super::Identity>,
        native_turn: Option<u64>,
        occurrence: NonToolOccurrence,
        plan: String,
        declarations: Vec<serde_json::Value>,
    ) -> Result<NonToolFacts> {
        self.begin_non_tool_owned(
            phase,
            identity,
            LifecycleOrigin {
                native_session: None,
                native_turn,
                source: None,
            },
            occurrence,
            plan,
            declarations,
        )
    }
    pub(crate) fn begin_non_tool_owned(
        &self,
        phase: &str,
        identity: Option<&super::Identity>,
        origin: LifecycleOrigin,
        occurrence: NonToolOccurrence,
        plan: String,
        declarations: Vec<serde_json::Value>,
    ) -> Result<NonToolFacts> {
        use std::os::unix::fs::MetadataExt;
        let native_turn = origin.native_turn;
        let native_session = origin.native_session;
        ensure!(
            native_session.is_some()
                == matches!(
                    occurrence,
                    NonToolOccurrence::SessionStart { .. } | NonToolOccurrence::SessionEnd { .. }
                ),
            "native lifetime event requires exact session authority"
        );
        ensure!(
            native_session.is_none() || (native_turn.is_none() && origin.source.is_none()),
            "native session cannot borrow turn or source authority"
        );
        ensure!(
            occurrence.event() != HookEvent::StopFailure
                || (native_turn.is_some() && origin.source.is_none()),
            "StopFailure requires its original native turn"
        );
        ensure!(
            native_turn.is_none() || origin.source.is_none(),
            "lifecycle has conflicting native and source owners"
        );
        let session = self.plugin_session()?;
        let host_transcript_path = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("runtime lock failed"))?
            .store
            .state_path()
            .to_str()
            .context("host transcript path is not UTF-8")?
            .to_owned();
        ensure!(
            serde_json::to_vec(&occurrence)?.len() <= 64 * 1024,
            "lifecycle occurrence exceeds input bound"
        );
        ensure!(
            declarations.len() <= 32 && serde_json::to_vec(&declarations)?.len() <= 1024 * 1024,
            "lifecycle declarations exceed bound"
        );
        self.non_tool_admission(occurrence.event(), |record| {
            let owner = if let Some(id) = native_session {
                ensure!(
                    phase == "native-session",
                    "native lifetime cannot authorize child or worker phase"
                );
                ensure!(
                    !record
                        .operations
                        .iter()
                        .filter_map(Operation::non_tool_receipt)
                        .any(|r| r.facts.native_session == Some(id)
                            && r.facts.subject.occurrence.event() == occurrence.event()),
                    "native session occurrence already recorded; never replay"
                );
                let (identity, _) = super::plugin_session::validate(record, id, &occurrence)?;
                let (_, lifetime) = super::plugin_session::lifetime(record, id)?;
                ensure!(
                    lifetime
                        .plans
                        .iter()
                        .any(|(event, digest)| *event == occurrence.event() && digest == &plan),
                    "native session hook policy differs from startup generation"
                );
                owner::Owner {
                    identity,
                    root: &record.workspace,
                    child: None,
                }
            } else {
                owner::resolve(record, phase)?
            };
            ensure!(
                identity.is_none_or(|i| i == owner.identity)
                    && (owner.child.is_none() || identity.is_some()),
                "lifecycle execution identity differs from assignment"
            );
            let execution_identity = owner.identity.clone();
            let workspace = owner.root.to_owned();
            let child_owner = owner.child;
            if let Some(source) = &origin.source {
                owner::validate_backend(
                    record,
                    phase,
                    &execution_identity,
                    source.correlation.backend_operation,
                )?;
                owner::validate_source(
                    record,
                    phase,
                    &source.correlation,
                    &occurrence,
                    &source.input,
                )?;
            }
            if let Some(turn) = native_turn {
                let turn = turn::validate(record, turn, phase)?;
                ensure!(
                    turn.origin == NativeTurnOrigin::Developer
                        || !matches!(
                            occurrence,
                            NonToolOccurrence::UserPromptSubmit {
                                correction: false,
                                ..
                            }
                        ),
                    "plugin-origin turn cannot fabricate an initial developer submission"
                );
            }
            ensure!(
                record.operations.len() < 4096,
                "session operation history is full"
            );
            ensure!(
                !record.operations.iter().any(|o| o.phase == phase
                    && o.non_tool_receipt().is_some()
                    && o.needs_reconciliation()),
                "unfinished lifecycle occurrence must not replay"
            );
            let metadata =
                std::fs::metadata(&workspace).context("lifecycle workspace unavailable")?;
            let id = record.operations.len() as u64 + 1;
            let facts = NonToolFacts {
                native_session,
                callback: origin.source.as_ref().map(|s| s.correlation.clone()),
                native_turn,
                provenance: if origin.source.is_some() {
                    Some("authenticated_source_callback_v1".into())
                } else {
                    native_turn
                        .or(native_session)
                        .map(|_| "native_host_translation_v1".into())
                },
                declaration_role: Some("worker".into()),
                child_owner,
                host_transcript_path: host_transcript_path.clone(),
                host_model: execution_identity.model.clone(),
                host_permission_mode: if execution_identity.unrestricted {
                    "bypassPermissions"
                } else {
                    "default"
                }
                .into(),
                source: origin.source.as_ref().map(|s| s.input.clone()),
                session: session.clone(),
                operation: id,
                task: record.task.as_ref().map(|t| t.id),
                role: phase.into(),
                subject: LifecycleSubject {
                    version: 1,
                    occurrence: occurrence.clone(),
                },
                workspace: (metadata.dev(), metadata.ino()),
            };
            let receipt = NonToolReceipt {
                source_delivery: origin.source.as_ref().map(|_| SourceDelivery::Pending),
                correction_required: false,
                version: 1,
                facts: facts.clone(),
                plan: plan.clone(),
                declarations: declarations.clone(),
                hooks: vec![],
                once_skips: vec![],
                proposals: vec![],
                messages: vec![],
                diagnostics: vec![],
                hold: None,
                settled: false,
                correction_admitted: false,
            };
            let source = native_session.or(native_turn).or_else(|| {
                origin
                    .source
                    .as_ref()
                    .map(|s| s.correlation.backend_operation)
            });
            let budget = match source {
                Some(source) => super::budget_accounting::inherited(record, source)?,
                None => super::budget_accounting::capture(record, &session),
            };
            record.operations.push(Operation {
                id,
                budget: Some(budget),
                usage_receipt: None,
                phase: phase.into(),
                verification: None,
                call: None,
                result: None,
                tool_receipt: None,
                host_invocation: Some(HostInvocation::Lifecycle(Box::new(receipt))),
                complete: false,
                reconciled: false,
                usage_reported: true,
                identity: Some(execution_identity),
            });
            Ok(facts)
        })
    }

    pub(crate) fn reserve_non_tool_hook(
        &self,
        id: u64,
        event: HookEvent,
        mut hook: HookReceipt,
        binding: Option<&crate::plugins::once::OnceBinding>,
        lease: Option<&std::sync::Arc<tokio::sync::OwnedSemaphorePermit>>,
    ) -> Result<crate::plugins::once::HookReservation> {
        let tracker = self.once_live()?;
        self.non_tool_admission(event, |record| {
            let receipt = active(record, id, event)?;
            ensure!(
                !matches!(event, HookEvent::SessionStart | HookEvent::SessionEnd)
                    || (hook.declaration.dialect
                        == crate::plugins::hook_types::HookDialect::Native
                        && match hook.declaration.runner {
                            crate::plugins::hook_types::HandlerKind::Command => true,
                            crate::plugins::hook_types::HandlerKind::Prompt
                            | crate::plugins::hook_types::HandlerKind::Agent
                            | crate::plugins::hook_types::HandlerKind::Http
                            | crate::plugins::hook_types::HandlerKind::McpTool => {
                                let budget = super::budget_accounting::inherited(record, id)?;
                                super::plugin_admission::validate_funded_budget(
                                    record,
                                    &receipt.facts.session,
                                    id,
                                    event,
                                    &budget,
                                )?;
                                true
                            }
                        }
                        && !hook.required_gate),
                "native lifetime requires native declarations without required gates"
            );
            ensure!(
                receipt.hooks.len() + receipt.once_skips.len() < 32,
                "lifecycle hook limit reached"
            );
            let key = &hook.inspected;
            ensure!(
                key.operation == id
                    && key.source_operation == receipt.facts.causal_operation()
                    && key.session == receipt.facts.session
                    && key.event == event.as_str()
                    && key.role == receipt.facts.role
                    && hook.declaration.role
                        == receipt
                            .facts
                            .declaration_role
                            .as_deref()
                            .unwrap_or("worker")
                    && key.plan == receipt.plan
                    && key.workspace == receipt.facts.workspace
                    && key.tool.is_none()
                    && key.arguments.is_none()
                    && key.lifecycle.as_ref() == Some(&receipt.facts.subject),
                "lifecycle hook binding mismatch"
            );
            let identity = serde_json::to_value(&hook.declaration)?;
            let source = serde_json::to_value(&hook.source)?;
            let once = serde_json::to_value(binding)?;
            ensure!(
                receipt
                    .declarations
                    .iter()
                    .any(|d| d["identity"] == identity
                        && d["source"] == source
                        && d["once"] == once
                        && d["required_gate"] == hook.required_gate),
                "lifecycle declaration is not frozen"
            );
            hook.invocation = receipt.hooks.len() as u32;
            let skipped = super::plugin_once::reserve(record, &mut hook, binding)?;
            if skipped.is_none() {
                super::plugin_once::track_live(&tracker, &hook, lease)?;
            }
            let receipt = record
                .operations
                .iter_mut()
                .find(|o| o.id == id)
                .expect("validated")
                .non_tool_receipt_mut()
                .expect("validated");
            if let Some(skip) = skipped {
                receipt.once_skips.push(skip);
                Ok(crate::plugins::once::HookReservation::Skipped)
            } else {
                receipt.hooks.push(hook.clone());
                Ok(crate::plugins::once::HookReservation::Run(Box::new(hook)))
            }
        })
    }

    /// Retain the exact reserved outcome even after owner cancellation. No next
    /// action or successful one-shot consumption follows from raw settlement.
    pub(crate) fn finish_non_tool_hook(
        &self,
        id: u64,
        event: HookEvent,
        mut hook: HookReceipt,
    ) -> Result<()> {
        self.update(|record| {
            let receipt = record
                .operations
                .iter()
                .find(|o| o.id == id)
                .and_then(Operation::non_tool_receipt)
                .context("lifecycle receipt missing")?;
            ensure!(
                !receipt.settled && receipt.facts.subject.occurrence.event() == event,
                "lifecycle settlement event mismatch"
            );
            let previous = receipt
                .hooks
                .get(hook.invocation as usize)
                .context("lifecycle reservation missing")?;
            ensure!(
                previous.outcome.is_none()
                    && hook.outcome.is_some()
                    && previous.declaration == hook.declaration
                    && previous.inspected == hook.inspected
                    && previous.class == hook.class
                    && previous.required_gate == hook.required_gate
                    && previous.endpoint == hook.endpoint
                    && previous.once == hook.once
                    && previous.source == hook.source
                    && previous.observer.is_none()
                    && hook.observer.is_none(),
                "lifecycle outcome mismatches or repeats reservation"
            );
            ensure!(
                serde_json::to_vec(&hook)?.len() <= 256 * 1024,
                "lifecycle output exceeds retention bound"
            );
            let bytes: usize = receipt
                .hooks
                .iter()
                .filter(|h| h.invocation != hook.invocation)
                .map(serde_json::to_vec)
                .collect::<Result<Vec<_>, _>>()?
                .iter()
                .map(Vec::len)
                .sum();
            ensure!(
                bytes + serde_json::to_vec(&hook)?.len() <= 4 * 1024 * 1024,
                "lifecycle history exceeds retention bound"
            );
            super::plugin_once::settle_post_raw(&mut hook);
            if hook.uncertain_effects {
                record.recovery_pending = true;
            }
            let index = hook.invocation as usize;
            record
                .operations
                .iter_mut()
                .find(|o| o.id == id)
                .expect("validated")
                .non_tool_receipt_mut()
                .expect("validated")
                .hooks[index] = hook;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests;
