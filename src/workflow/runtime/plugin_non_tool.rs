//! Typed host lifecycle boundaries in the existing durable operation ledger.
use super::{HostInvocation, Operation, Record, SharedRuntime, delegation};
use crate::plugins::{hook_types::HookEvent, receipts::*};
use anyhow::{Context, Result, ensure};
pub(super) mod owner;
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
    /// Hold the runtime identity/policy boundary while the caller performs the
    /// short synchronous Settings compare-and-publish transaction.
    pub(crate) fn publish_config_change(
        &self,
        operation: u64,
        expected: &LifecycleSubject,
        publish: impl FnOnce() -> Result<crate::settings::SaveStatus>,
    ) -> Result<crate::settings::SaveStatus> {
        let mut runtime = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("runtime lock failed"))?;
        ensure!(
            !runtime.failed,
            "session persistence failed; execution is held until recovery"
        );
        let record = &mut runtime.record;
        let operation = record
            .operations
            .iter()
            .find(|candidate| candidate.id == operation)
            .context("settings gate receipt missing")?;
        let receipt = operation
            .non_tool_receipt()
            .context("settings gate receipt missing")?;
        owner::validate(record, operation, receipt)?;
        ensure!(
            receipt.settled
                && receipt.hold.is_none()
                && receipt.publication.is_none()
                && receipt.facts.subject == *expected
                && receipt.facts.subject.occurrence.event() == HookEvent::ConfigChange,
            "settings proposal was denied, unsettled, or changed after inspection"
        );
        let operation_id = operation.id;
        let result = publish()?;
        let publication = match result {
            crate::settings::SaveStatus::Applied => ConfigPublication::Published,
            crate::settings::SaveStatus::AppliedUncertain(_)
            | crate::settings::SaveStatus::PublicationUncertain(_) => ConfigPublication::Uncertain,
        };
        record
            .operations
            .iter_mut()
            .find(|candidate| candidate.id == operation_id)
            .and_then(Operation::non_tool_receipt_mut)
            .expect("validated settings receipt")
            .publication = Some(publication);
        let persistence = serde_json::to_value(&*record)
            .map_err(anyhow::Error::from)
            .and_then(|payload| runtime.store.write(&payload));
        if let Err(error) = persistence {
            runtime.failed = true;
            return Ok(crate::settings::SaveStatus::AppliedUncertain(format!(
                "settings were applied but their policy receipt could not be recorded; restart the session and inspect the saved revision before continuing: {error}"
            )));
        }
        Ok(result)
    }

    fn non_tool_admission<T>(
        &self,
        event: HookEvent,
        f: impl FnOnce(&mut Record) -> Result<T>,
    ) -> Result<T> {
        if matches!(
            event,
            HookEvent::SessionStart
                | HookEvent::SessionEnd
                | HookEvent::ConfigChange
                | HookEvent::PreModelSwitch
                | HookEvent::PostModelSwitch
        ) {
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
        let callback = receipt
            .facts
            .callback
            .as_ref()
            .context("source callback missing")?;
        let input = receipt
            .facts
            .source
            .as_ref()
            .context("source callback input missing")?;
        let post = if receipt.facts.subject.occurrence.host_operation().is_some()
            && matches!(
                receipt.facts.subject.occurrence,
                NonToolOccurrence::PreModelSwitch { .. }
                    | NonToolOccurrence::PostModelSwitch { .. }
            ) {
            super::model_switch::validate_source(
                &record,
                receipt
                    .facts
                    .subject
                    .occurrence
                    .host_operation()
                    .expect("checked"),
                callback,
                input,
            )?;
            None
        } else {
            owner::validate_source(
                &record,
                &operation.phase,
                callback,
                &receipt.facts.subject.occurrence,
                input,
            )?
        };
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

    /// A replaced or cancelled Settings owner cannot publish. Once every runner
    /// has drained, retain known local outcomes and close the occurrence only
    /// when no effect is uncertain; unknown effects continue to require recovery.
    pub(crate) fn settle_cancelled_settings_control(&self, id: u64) -> Result<()> {
        self.update(|record| {
            let causal_work_unfinished = record.operations.iter().any(|operation| {
                operation.settings_causal_owner() == Some(id) && operation.needs_reconciliation()
            });
            let operation = record
                .operations
                .iter_mut()
                .find(|operation| operation.id == id)
                .context("settings control operation missing")?;
            if operation.complete || operation.reconciled {
                return Ok(());
            }
            let receipt = operation
                .non_tool_receipt_mut()
                .context("settings control receipt missing")?;
            let authority = receipt
                .host_control
                .as_ref()
                .context("settings control authority missing")?;
            ensure!(
                receipt.facts.subject.occurrence.event() == HookEvent::ConfigChange
                    && authority
                        .operation
                        .load(std::sync::atomic::Ordering::Acquire)
                        == id
                    && receipt.facts.host_session == Some(authority.lifetime)
                    && !authority.live.load(std::sync::atomic::Ordering::Acquire)
                    && receipt.publication.is_none(),
                "settings control is still live, published, or belongs to another operation"
            );
            if receipt
                .hooks
                .iter()
                .all(|hook| hook.outcome.is_some() && !hook.uncertain_effects)
                && receipt.source_delivery.is_none()
                && !causal_work_unfinished
            {
                receipt.settled = true;
                if receipt.hold.is_none() {
                    receipt.hold = Some("Settings attempt invalidated before publication".into());
                }
                super::plugin_once::settle_hooks(&mut receipt.hooks, &receipt.proposals, &[]);
                operation.complete = true;
            } else {
                record.recovery_pending = true;
            }
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
                    && matches!(
                        receipt.facts.subject.occurrence.event(),
                        HookEvent::Stop | HookEvent::PostCompact
                    )
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
                host_control: None,
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
        let host_operation = occurrence.host_operation();
        let native_turn = origin.native_turn;
        let native_session = origin.native_session;
        let host_control = origin.host_control.clone();
        ensure!(
            host_control.is_some() == matches!(occurrence, NonToolOccurrence::ConfigChange { .. }),
            "ConfigChange requires exact host control authority"
        );
        ensure!(
            native_session.is_some()
                == matches!(
                    occurrence,
                    NonToolOccurrence::SessionStart { .. } | NonToolOccurrence::SessionEnd { .. }
                ),
            "native lifetime event requires exact session authority"
        );
        ensure!(
            native_session.is_none()
                || (native_turn.is_none() && origin.source.is_none() && host_control.is_none()),
            "native session cannot borrow turn or source authority"
        );
        ensure!(
            occurrence.event() != HookEvent::StopFailure
                || (native_turn.is_some() && origin.source.is_none()),
            "StopFailure requires its original native turn"
        );
        ensure!(
            native_turn.is_none() || (origin.source.is_none() && host_control.is_none()),
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
            let owner = if let Some(authority) = &host_control {
                ensure!(
                    phase == "settings",
                    "settings control belongs to another phase"
                );
                let (identity, _workspace) =
                    super::plugin_session::validate_host_control(record, authority)?;
                let (_, lifetime) = super::plugin_session::lifetime(record, authority.lifetime)?;
                ensure!(
                    lifetime.plans.iter().any(|(event, digest)| {
                        *event == HookEvent::ConfigChange && digest == &plan
                    }),
                    "settings hook policy differs from the original host session"
                );
                owner::Owner {
                    identity,
                    root: &record.workspace,
                    child: None,
                }
            } else if let Some(id) = native_session {
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
            } else if matches!(
                occurrence,
                NonToolOccurrence::PreCompact {
                    compaction: Some(_),
                    ..
                } | NonToolOccurrence::PostCompact {
                    compaction: Some(_),
                    ..
                }
            ) {
                super::compaction::owner(record, phase)?
            } else if matches!(
                occurrence,
                NonToolOccurrence::PreModelSwitch { .. }
                    | NonToolOccurrence::PostModelSwitch { .. }
            ) {
                super::model_switch::owner(
                    record,
                    host_operation.context("model switch operation missing")?,
                    occurrence.event(),
                )?
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
            if matches!(
                occurrence,
                NonToolOccurrence::PreCompact {
                    compaction: Some(_),
                    ..
                } | NonToolOccurrence::PostCompact {
                    compaction: Some(_),
                    ..
                }
            ) {
                ensure!(
                    native_turn.is_none(),
                    "compaction must use its exact transaction owner"
                );
                let c = super::compaction::validate(record, host_operation.expect("typed"), phase)?;
                ensure!(
                    c.external_backend
                        == origin
                            .source
                            .as_ref()
                            .map(|s| s.correlation.backend_operation),
                    "compaction source backend differs"
                );
                super::compaction::validate_occurrence(record, phase, &occurrence)?;
                ensure!(
                    !record
                        .operations
                        .iter()
                        .filter_map(Operation::non_tool_receipt)
                        .any(
                            |r| r.facts.subject.occurrence.host_operation() == host_operation
                                && r.facts.subject.occurrence.event() == occurrence.event()
                        ),
                    "compaction event already recorded; never replay"
                );
            }
            if matches!(
                occurrence,
                NonToolOccurrence::PreModelSwitch { .. }
                    | NonToolOccurrence::PostModelSwitch { .. }
            ) {
                ensure!(
                    native_turn.is_none(),
                    "model switch cannot borrow a native turn"
                );
                super::model_switch::validate_occurrence(
                    record,
                    host_operation.context("model switch operation missing")?,
                    &occurrence,
                    &plan,
                )?;
                ensure!(
                    !record
                        .operations
                        .iter()
                        .filter_map(Operation::non_tool_receipt)
                        .any(|receipt| {
                            receipt.facts.subject.occurrence.host_operation() == host_operation
                                && receipt.facts.subject.occurrence.event() == occurrence.event()
                        }),
                    "model switch event already recorded; never replay"
                );
            }
            if let NonToolOccurrence::PostToolBatch {
                batch: Some(id),
                tool_calls,
            } = &occurrence
            {
                ensure!(
                    origin.source.is_none(),
                    "host batch cannot borrow source callback authority"
                );
                super::tool_batches::validate_observation(record, phase, *id, tool_calls)?;
                ensure!(
                    !record
                        .operations
                        .iter()
                        .filter_map(Operation::non_tool_receipt)
                        .any(|r| r.facts.subject.occurrence.host_operation() == Some(*id)),
                    "batch observation already recorded; never replay"
                );
            }
            if let Some(source) = &origin.source {
                if matches!(
                    occurrence,
                    NonToolOccurrence::PreModelSwitch { .. }
                        | NonToolOccurrence::PostModelSwitch { .. }
                ) {
                    ensure!(phase == "model-switch", "model callback phase differs");
                    super::model_switch::validate_source(
                        record,
                        host_operation.context("model switch operation missing")?,
                        &source.correlation,
                        &source.input,
                    )?;
                } else {
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
                host_session: host_control.as_ref().map(|authority| authority.lifetime),
                callback: origin.source.as_ref().map(|s| s.correlation.clone()),
                native_turn,
                provenance: if host_control.is_some() {
                    Some("explicit_host_control_v1".into())
                } else if origin.source.is_some() {
                    Some("authenticated_source_callback_v1".into())
                } else if host_operation.is_some() {
                    Some("explicit_host_operation_v1".into())
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
                task: if host_control.is_some() {
                    None
                } else {
                    record.task.as_ref().map(|t| t.id)
                },
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
                publication: None,
                host_control: host_control.clone(),
            };
            let compact_lifetime = if matches!(
                occurrence,
                NonToolOccurrence::PreCompact {
                    compaction: Some(_),
                    ..
                } | NonToolOccurrence::PostCompact {
                    compaction: Some(_),
                    ..
                }
            ) {
                super::compaction::hook_lifetime(
                    record,
                    host_operation.context("compaction missing")?,
                    phase,
                )?
            } else {
                None
            };
            let source = compact_lifetime
                .or(host_operation)
                .or_else(|| host_control.as_ref().map(|authority| authority.lifetime))
                .or(native_session)
                .or(native_turn)
                .or_else(|| {
                    origin
                        .source
                        .as_ref()
                        .map(|s| s.correlation.backend_operation)
                });
            let budget = match source {
                Some(source) => super::budget_accounting::inherited(record, source)?,
                None => super::budget_accounting::capture(record, &session),
            };
            if let Some(authority) = &host_control {
                authority
                    .operation
                    .compare_exchange(
                        0,
                        id,
                        std::sync::atomic::Ordering::AcqRel,
                        std::sync::atomic::Ordering::Acquire,
                    )
                    .map_err(|_| anyhow::anyhow!("settings control already owns an operation"))?;
            }
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
impl SharedRuntime {
    pub(crate) fn fail_config_change_receipt_sync_when(
        &self,
        predicate: fn(&serde_json::Value) -> bool,
    ) {
        self.0
            .lock()
            .unwrap()
            .store
            .fail_directory_sync_when(predicate);
    }

    pub(crate) fn fail_config_change_receipt_before_rename_when(
        &self,
        predicate: fn(&serde_json::Value) -> bool,
    ) {
        self.0
            .lock()
            .unwrap()
            .store
            .fail_before_rename_when(predicate);
    }
}

#[cfg(test)]
mod tests;
