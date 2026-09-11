//! Completed-operation capabilities never reopen pre-tool admission or effects.
use super::{Record, SharedRuntime, delegation};
use crate::plugins::{
    hook_types::{HookEvent, ModelCallContext},
    receipts::*,
};
use anyhow::{Context, Result, ensure};

pub(super) mod owner;

#[cfg(test)]
#[path = "plugin_lifecycle/tests.rs"]
mod tests;

#[cfg(test)]
mod provider_content_tests;

pub(super) fn active(record: &Record, id: u64, event: HookEvent) -> Result<&LifecycleReceipt> {
    ensure!(
        matches!(
            event,
            HookEvent::PostToolUse | HookEvent::PostToolUseFailure
        ),
        "invalid post-tool owner event"
    );
    let operation = record
        .operations
        .iter()
        .find(|o| o.id == id)
        .context("post-tool operation missing")?;
    ensure!(
        !operation.reconciled,
        "quarantined post-tool operation has no execution authority"
    );
    let receipt = operation
        .tool_receipt
        .as_ref()
        .context("post-tool receipt missing")?;
    let original = operation
        .result
        .as_ref()
        .context("post-tool owner lacks completed evidence")?;
    ensure!(
        receipt.admitted && original.success == (event == HookEvent::PostToolUse),
        "post-tool owner does not match an actual admitted outcome"
    );
    ensure!(
        !record.recovery_pending,
        "post-tool owner needs reconciliation"
    );
    delegation::ensure_agent_active(record, &operation.phase)?;
    let lifecycle = receipt
        .plugin_lifecycle
        .as_ref()
        .context("post-tool lifecycle was not reserved")?;
    ensure!(
        lifecycle.facts.event == event && lifecycle.facts.operation == id && !lifecycle.settled,
        "post-tool capability event differs or is settled"
    );
    ensure!(
        lifecycle.facts.task == record.task.as_ref().map(|t| t.id)
            && record.task.as_ref().is_none_or(|t| (!t.stopped
                || lifecycle.facts.role == "verification"
                || lifecycle.facts.role.starts_with("agent:"))
                && t.accepted.is_none()),
        "post-tool task owner changed, stopped or accepted"
    );
    Ok(lifecycle)
}

pub(super) fn ensure_continuation(record: &Record, phase: &str) -> Result<()> {
    ensure_continuation_except(record, phase, None, None)
}

pub(super) fn ensure_continuation_except(
    record: &Record,
    phase: &str,
    correction: Option<u64>,
    source: Option<u64>,
) -> Result<()> {
    if let Some(receipt) = record
        .operations
        .iter()
        .rev()
        .filter(|o| o.phase == phase)
        .find_map(|o| o.non_tool_receipt())
    {
        ensure!(receipt.settled, "lifecycle gate is unfinished");
        if let Some(reason) = &receipt.hold {
            anyhow::bail!("lifecycle gate remains unmet: {reason}");
        }
        ensure!(
            !receipt.correction_required || receipt.correction_admitted,
            "Stop correction has not been admitted"
        );
    }
    for operation in record
        .operations
        .iter()
        .filter(|o| o.phase == phase && Some(o.id) != correction)
    {
        if Some(operation.id) != source
            && let Some(receipt) = operation.non_tool_receipt()
        {
            ensure!(
                !matches!(
                    receipt.source_delivery,
                    Some(
                        crate::plugins::receipts::SourceDelivery::Pending
                            | crate::plugins::receipts::SourceDelivery::Sent
                    )
                ),
                "source lifecycle continuation is pending or uncertain; never resend automatically"
            );
        }
        if let Some(lifecycle) = operation
            .tool_receipt
            .as_ref()
            .and_then(|r| r.plugin_lifecycle.as_ref())
        {
            ensure!(
                lifecycle.settled,
                "post-tool lifecycle is unfinished; inspect retained evidence before continuing"
            );
            ensure!(
                matches!(
                    lifecycle.delivery,
                    PostDelivery::Local
                        | PostDelivery::Acknowledged
                        | PostDelivery::CorrectionAcknowledged { .. }
                ),
                "post-tool presentation is pending or uncertain; never resend automatically"
            );
            if let PostContinuation::Held { reason } = &lifecycle.continuation {
                anyhow::bail!("post-tool continuation held: {reason}");
            }
        }
    }
    Ok(())
}

pub(super) fn source_correction_owner(
    record: &Record,
    phase: &str,
    callback: &SourceCallback,
    occurrence: &NonToolOccurrence,
    source: &ObservedLifecycle,
) -> Result<Option<u64>> {
    let origin = callback
        .origin
        .as_ref()
        .context("source callback origin is unknown")?;
    let SourceOrigin::PluginPostCorrection {
        post_operation,
        content_digest,
    } = origin
    else {
        return Ok(None);
    };
    let post = record
        .operations
        .iter()
        .find(|o| o.id == *post_operation && o.phase == phase)
        .and_then(|o| o.tool_receipt.as_ref())
        .and_then(|r| r.plugin_lifecycle.as_ref())
        .context("source callback post-correction owner missing")?;
    ensure!(
        post.settled
            && post.continuation == PostContinuation::Correction
            && post.correction_required
            && post.correction_admitted,
        "source callback lacks admitted post-correction authority"
    );
    owner::validate(record, &post.facts)?;
    ensure!(
        matches!((&post.facts.representation, source),
        (ToolRepresentation::ClaudeMcp { source_input, .. }, ObservedLifecycle::Claude(input))
            if source_input["session_id"].is_string() && source_input["session_id"] == input["session_id"]),
        "source callback differs from its post-correction source session"
    );
    ensure!(
        callback
            .command_uuid
            .as_ref()
            .is_some_and(|id| id.len() == 36)
            && content_digest.len() == 64,
        "source post-correction frame identity missing"
    );
    match &post.delivery {
        PostDelivery::CorrectionReserved { invocation } => {
            ensure!(
                *invocation == callback.backend_operation
                    && matches!(
                        occurrence,
                        NonToolOccurrence::UserPromptSubmit {
                            correction: true,
                            ..
                        }
                    ),
                "source callback differs from pending post-correction submission"
            );
            Ok(Some(*post_operation))
        }
        PostDelivery::CorrectionAcknowledged {
            invocation,
            acknowledgment:
                CorrectionAcknowledgment::ClaudeUser {
                    uuid,
                    content_digest: acknowledged,
                    ..
                },
        } => {
            ensure!(
                *invocation == callback.backend_operation
                    && callback.command_uuid.as_ref() == Some(uuid)
                    && content_digest == acknowledged,
                "source callback differs from acknowledged post-correction frame"
            );
            Ok(None)
        }
        _ => anyhow::bail!("source callback post-correction delivery is stale or unrelated"),
    }
}

impl SharedRuntime {
    pub(crate) fn plugin_runner_owner(&self, id: u64, event: HookEvent) -> Result<(u64, String)> {
        if event == HookEvent::PreToolUse {
            return self.plugin_owner(id);
        }
        let runtime = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("runtime lock failed"))?;
        ensure!(!runtime.failed, "session persistence failed");
        if matches!(event, HookEvent::UserPromptSubmit | HookEvent::Stop) {
            let receipt = super::plugin_non_tool::active(&runtime.record, id, event)?;
            return Ok((receipt.facts.operation, receipt.facts.role.clone()));
        }
        let lifecycle = active(&runtime.record, id, event)?;
        Ok((
            lifecycle.facts.source_operation,
            lifecycle.facts.role.clone(),
        ))
    }
    pub(crate) fn ensure_post_continuation(&self, phase: &str) -> Result<()> {
        let runtime = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("runtime lock failed"))?;
        ensure!(!runtime.failed, "session persistence failed");
        ensure_continuation(&runtime.record, phase)
    }
    pub(crate) fn post_model_context(&self, id: u64, event: HookEvent) -> Result<ModelCallContext> {
        let record = self.record()?;
        let lifecycle = active(&record, id, event)?;
        let allocation_available = record.allocation.as_ref().is_some_and(|a| {
            a.remaining_ms().is_ok_and(|ms| ms > 0)
                && a.model_calls < a.limits.model_calls
                && a.tool_calls < a.limits.tool_calls
        });
        let correction_available = owner::available(&record, &lifecycle.facts);
        Ok(ModelCallContext {
            allocation_available,
            correction_available,
            ..ModelCallContext::default()
        })
    }
    pub(crate) fn begin_post_tool(
        &self,
        id: u64,
        event: HookEvent,
        plan: String,
        declarations: Vec<serde_json::Value>,
        representation: ToolRepresentation,
    ) -> Result<PostToolFacts> {
        let session = self.plugin_session()?;
        let host_transcript_path = {
            let runtime = self
                .0
                .lock()
                .map_err(|_| anyhow::anyhow!("runtime lock failed"))?;
            runtime
                .store
                .state_path()
                .to_str()
                .context("host transcript path is not UTF-8")?
                .to_owned()
        };
        self.admission(|record| {
            ensure!(
                !record.recovery_pending,
                "post-tool admission needs reconciliation"
            );
            ensure!(
                matches!(
                    event,
                    HookEvent::PostToolUse | HookEvent::PostToolUseFailure
                ),
                "invalid post-tool event"
            );
            ensure!(
                declarations.len() <= 32,
                "post-tool declaration bound exceeded"
            );
            let operation = record
                .operations
                .iter()
                .find(|o| o.id == id)
                .context("post-tool operation missing")?;
            delegation::ensure_agent_active(record, &operation.phase)?;
            let receipt = operation
                .tool_receipt
                .as_ref()
                .context("post-tool correlation missing")?;
            let original = operation
                .result
                .as_ref()
                .context("post-tool requires retained original result")?;
            ensure!(
                receipt.admitted && original.success == (event == HookEvent::PostToolUse),
                "post-tool event differs from actual admitted attempt"
            );
            ensure!(
                receipt.plugin_lifecycle.is_none(),
                "post-tool already reserved; never replay"
            );
            let source = record
                .operations
                .iter()
                .find(|o| o.id == receipt.invocation)
                .and_then(|o| o.identity.as_ref())
                .unwrap_or(&record.identity);
            let facts = PostToolFacts {
                version: 1,
                session,
                task: record.task.as_ref().map(|t| t.id),
                role: operation.phase.clone(),
                operation: id,
                source_operation: receipt.invocation,
                event,
                representation,
                provenance: "host_operation_translation_v1".into(),
                host_transcript_path,
                host_model: source.model.clone(),
                host_permission_mode: if source.unrestricted {
                    "bypassPermissions"
                } else {
                    "default"
                }
                .into(),
            };
            let receipt = record
                .operations
                .iter_mut()
                .find(|o| o.id == id)
                .expect("validated")
                .tool_receipt
                .as_mut()
                .expect("validated");
            receipt.plugin_lifecycle = Some(LifecycleReceipt {
                once_skips: Vec::new(),
                delivery: if matches!(facts.representation, ToolRepresentation::Native) {
                    PostDelivery::LocalPending
                } else {
                    PostDelivery::Staged
                },
                version: 1,
                facts: facts.clone(),
                plan,
                declarations,
                hooks: Vec::new(),
                proposals: Vec::new(),
                messages: Vec::new(),
                diagnostics: Vec::new(),
                model_content: None,
                continuation: PostContinuation::Continue,
                correction_required: false,
                correction_admitted: false,
                correction_presentation: None,
                settled: false,
            });
            Ok(facts)
        })
    }
    #[cfg(test)]
    pub(crate) fn begin_post_hook(
        &self,
        id: u64,
        event: HookEvent,
        hook: HookReceipt,
    ) -> Result<u32> {
        match self.reserve_post_hook(id, event, hook, None, None)? {
            crate::plugins::once::HookReservation::Run(hook) => Ok(hook.invocation),
            crate::plugins::once::HookReservation::Skipped => {
                anyhow::bail!("unexpected one-shot skip")
            }
        }
    }
    pub(crate) fn reserve_post_hook(
        &self,
        id: u64,
        event: HookEvent,
        mut hook: HookReceipt,
        binding: Option<&crate::plugins::once::OnceBinding>,
        lease: Option<&std::sync::Arc<tokio::sync::OwnedSemaphorePermit>>,
    ) -> Result<crate::plugins::once::HookReservation> {
        let tracker = self.once_live()?;
        self.admission(|record| {
            let lifecycle = active(record, id, event)?;
            ensure!(
                lifecycle.hooks.len() + lifecycle.once_skips.len() < 32,
                "post-tool hook limit reached"
            );
            let operation = record
                .operations
                .iter()
                .find(|o| o.id == id)
                .expect("validated");
            let call = operation
                .call
                .as_ref()
                .context("admitted tool call missing")?;
            ensure!(
                hook.inspected.operation == id
                    && hook.inspected.event == event.as_str()
                    && hook.inspected.source_operation == lifecycle.facts.source_operation
                    && hook.inspected.session == lifecycle.facts.session
                    && hook.inspected.role == lifecycle.facts.role
                    && hook.declaration.role == lifecycle.facts.role
                    && hook.inspected.plan == lifecycle.plan
                    && hook.inspected.lifecycle.is_none()
                    && hook.inspected.tool.as_deref() == Some(call.name.as_str())
                    && hook.inspected.arguments.as_deref()
                        == Some(crate::plugins::admission::candidate_digest(call)?.as_str()),
                "post-tool hook binding mismatch"
            );
            let declaration = serde_json::to_value(&hook.declaration)?;
            let encoded_binding = serde_json::to_value(binding)?;
            let encoded_source = serde_json::to_value(&hook.source)?;
            ensure!(
                lifecycle
                    .declarations
                    .iter()
                    .any(|d| d["identity"] == declaration
                        && d["once"] == encoded_binding
                        && d["source"] == encoded_source
                        && d["required_gate"] == hook.required_gate),
                "post-tool declaration is not in frozen plan"
            );
            hook.invocation = lifecycle.hooks.len() as u32;
            let skipped = super::plugin_once::reserve(record, &mut hook, binding)?;
            if skipped.is_none() {
                super::plugin_once::track_live(&tracker, &hook, lease)?;
            }
            let lifecycle = record
                .operations
                .iter_mut()
                .find(|o| o.id == id)
                .expect("validated")
                .tool_receipt
                .as_mut()
                .expect("validated")
                .plugin_lifecycle
                .as_mut()
                .expect("validated");
            if let Some(skip) = skipped {
                lifecycle.once_skips.push(skip);
                Ok(crate::plugins::once::HookReservation::Skipped)
            } else {
                lifecycle.hooks.push(hook.clone());
                Ok(crate::plugins::once::HookReservation::Run(Box::new(hook)))
            }
        })
    }
    /// Retain raw source facts before any proposal can be applied. Settling this
    /// reserved invocation does not require a live owner and grants no next action.
    pub(crate) fn finish_post_hook(
        &self,
        id: u64,
        event: HookEvent,
        mut hook: HookReceipt,
    ) -> Result<()> {
        self.update(|record| {
            let lifecycle = record
                .operations
                .iter()
                .find(|o| o.id == id)
                .and_then(|o| o.tool_receipt.as_ref())
                .and_then(|r| r.plugin_lifecycle.as_ref())
                .context("post-tool lifecycle missing")?;
            ensure!(
                lifecycle.facts.event == event && !lifecycle.settled,
                "post-tool settlement event mismatch"
            );
            let previous = lifecycle
                .hooks
                .get(hook.invocation as usize)
                .context("post-tool invocation missing")?;
            ensure!(
                previous.outcome.is_none()
                    && hook.outcome.is_some()
                    && previous.declaration == hook.declaration
                    && previous.inspected == hook.inspected
                    && previous.class == hook.class
                    && previous.required_gate == hook.required_gate
                    && previous.endpoint == hook.endpoint
                    && previous.once == hook.once
                    && previous.source == hook.source,
                "post-tool outcome mismatches or repeats a reservation"
            );
            ensure!(
                serde_json::to_vec(&hook)?.len() <= 256 * 1024,
                "post-tool outcome retention limit"
            );
            let retained = lifecycle
                .hooks
                .iter()
                .enumerate()
                .map(|(i, h)| {
                    serde_json::to_vec(if i == hook.invocation as usize {
                        &hook
                    } else {
                        h
                    })
                    .map(|v| v.len())
                })
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .sum::<usize>();
            ensure!(retained <= 4 * 1024 * 1024, "post-tool history limit");
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
                .tool_receipt
                .as_mut()
                .expect("validated")
                .plugin_lifecycle
                .as_mut()
                .expect("validated")
                .hooks[index] = hook;
            Ok(())
        })
    }
    pub(crate) fn settle_post_tool(
        &self,
        id: u64,
        event: HookEvent,
        effects: crate::plugins::lifecycle::PostEffects,
    ) -> Result<PostContinuation> {
        self.update(|target| {
            let mut staged = target.clone();
            let record = &mut staged;
            let lifecycle = record
                .operations
                .iter()
                .find(|o| o.id == id)
                .and_then(|o| o.tool_receipt.as_ref())
                .and_then(|r| r.plugin_lifecycle.as_ref())
                .context("post-tool lifecycle missing")?;
            ensure!(
                lifecycle.facts.event == event
                    && !lifecycle.settled
                    && lifecycle
                        .hooks
                        .iter()
                        .all(|h| h.outcome.is_some() || h.transferred()),
                "post-tool effects lack retained outcomes"
            );
            super::plugin_once::validate_skips(record, &lifecycle.once_skips)?;
            let mut continuation = effects.continuation;
            if lifecycle.hooks.iter().any(|h| h.unresolved_effects()) {
                continuation = PostContinuation::Held {
                    reason: "post-tool effects are uncertain; reconcile before continuing".into(),
                };
            }
            let mut correction_required = false;
            if !matches!(continuation, PostContinuation::Held { .. })
                && (effects.consume_correction || continuation == PostContinuation::Correction)
            {
                let allowed = record.allocation.as_ref().is_some_and(|a| {
                    a.remaining_ms().is_ok_and(|ms| ms > 0)
                        && a.model_calls < a.limits.model_calls
                        && a.tool_calls < a.limits.tool_calls
                });
                if allowed && owner::available(record, &lifecycle.facts) {
                    correction_required = true;
                } else {
                    continuation = PostContinuation::Held {
                        reason:
                            "post-tool correction lacks remaining task authority; work is unmet"
                                .into(),
                    };
                }
            }
            let lifecycle = record
                .operations
                .iter_mut()
                .find(|o| o.id == id)
                .expect("validated")
                .tool_receipt
                .as_mut()
                .expect("validated")
                .plugin_lifecycle
                .as_mut()
                .expect("validated");
            lifecycle.proposals = effects.proposals;
            lifecycle.messages = effects.messages;
            lifecycle.diagnostics = effects.diagnostics;
            lifecycle.model_content = effects.model_content;
            lifecycle.continuation = continuation.clone();
            lifecycle.correction_required = correction_required;
            lifecycle.settled = true;
            super::plugin_once::settle_post(lifecycle, &effects.once_successful);
            ensure!(
                serde_json::to_vec(lifecycle)?.len() <= 6 * 1024 * 1024,
                "post-tool lifecycle retention limit"
            );
            *target = staged;
            Ok(continuation)
        })
    }
    pub(crate) fn post_tool_receipt(&self, id: u64) -> Result<Option<LifecycleReceipt>> {
        Ok(self
            .record()?
            .operations
            .iter()
            .find(|o| o.id == id)
            .and_then(|o| o.tool_receipt.as_ref())
            .and_then(|r| r.plugin_lifecycle.clone()))
    }

    pub(crate) fn post_correction_objective(&self, id: u64) -> Result<String> {
        let record = self.record()?;
        let facts = &record
            .operations
            .iter()
            .find(|o| o.id == id)
            .and_then(|o| o.tool_receipt.as_ref())
            .and_then(|r| r.plugin_lifecycle.as_ref())
            .context("post-tool correction owner missing")?
            .facts;
        Ok(owner::objective(&record, facts)?.into())
    }

    pub(crate) fn post_delivery_operation(
        &self,
        invocation: u64,
        call_id: &str,
    ) -> Result<Option<u64>> {
        let record = self.record()?;
        let mut matches = record.operations.iter().filter(|o| {
            o.tool_receipt
                .as_ref()
                .is_some_and(|r| r.invocation == invocation && r.original_call.id == call_id)
        });
        let found = matches.next();
        ensure!(
            matches.next().is_none(),
            "ambiguous post-tool delivery correlation"
        );
        Ok(found
            .filter(|o| {
                o.tool_receipt
                    .as_ref()
                    .is_some_and(|r| r.plugin_lifecycle.is_some())
            })
            .map(|o| o.id))
    }
    pub(crate) fn validate_post_delivery_owner(&self, id: u64) -> Result<()> {
        let record = self.record()?;
        ensure!(
            !record.recovery_pending,
            "post-tool delivery needs reconciliation"
        );
        let operation = record
            .operations
            .iter()
            .find(|o| o.id == id)
            .context("post-tool delivery missing")?;
        ensure!(
            !operation.reconciled,
            "quarantined post-tool delivery has no release authority"
        );
        delegation::ensure_agent_active(&record, &operation.phase)?;
        let lifecycle = operation
            .tool_receipt
            .as_ref()
            .and_then(|r| r.plugin_lifecycle.as_ref())
            .context("post-tool receipt missing")?;
        ensure!(
            lifecycle.facts.task == record.task.as_ref().map(|t| t.id)
                && record.task.as_ref().is_none_or(|t| (!t.stopped
                    || lifecycle.facts.role == "verification"
                    || lifecycle.facts.role.starts_with("agent:"))
                    && t.accepted.is_none()),
            "post-tool delivery task owner changed, stopped or accepted"
        );
        ensure!(
            lifecycle.settled
                && matches!(
                    lifecycle.delivery,
                    PostDelivery::LocalPending | PostDelivery::Staged | PostDelivery::Superseded
                ),
            "post-tool delivery is unfinished, repeated or uncertain"
        );
        if let PostContinuation::Held { reason } = &lifecycle.continuation {
            anyhow::bail!("post-tool continuation held: {reason}");
        }
        Ok(())
    }
    pub(crate) fn complete_local_post_release(&self, id: u64) -> Result<()> {
        self.update(|record| release(record, id, PostDelivery::LocalPending, PostDelivery::Local))
    }

    pub(crate) fn start_post_supersession(&self, id: u64) -> Result<()> {
        self.validate_post_delivery_owner(id)?;
        self.update(|record| {
            let operation = record
                .operations
                .iter_mut()
                .find(|o| o.id == id)
                .context("post-tool correction owner missing")?;
            let post = operation
                .tool_receipt
                .as_mut()
                .and_then(|r| r.plugin_lifecycle.as_mut())
                .context("post-tool correction missing")?;
            ensure!(
                post.delivery == PostDelivery::Staged
                    && post.continuation == PostContinuation::Correction
                    && post.correction_required
                    && !post.correction_admitted
                    && !matches!(post.facts.representation, ToolRepresentation::Native),
                "post-tool correction is not an unspent external continuation"
            );
            post.delivery = PostDelivery::Superseding;
            Ok(())
        })
    }

    pub(crate) fn finish_post_supersession(&self, id: u64) -> Result<()> {
        self.update(|record| {
            let post = record
                .operations
                .iter()
                .find(|o| o.id == id)
                .and_then(|o| o.tool_receipt.as_ref())
                .and_then(|r| r.plugin_lifecycle.as_ref())
                .context("post-tool correction missing")?;
            ensure!(
                post.delivery == PostDelivery::Superseding
                    && post.continuation == PostContinuation::Correction
                    && record
                        .operations
                        .iter()
                        .any(|o| o.id == post.facts.source_operation
                            && o.complete
                            && matches!(o.host_invocation, Some(super::HostInvocation::Backend))),
                "post-tool supersession lacks a completed original backend invocation"
            );
            record
                .operations
                .iter_mut()
                .find(|o| o.id == id)
                .expect("validated")
                .tool_receipt
                .as_mut()
                .expect("validated")
                .plugin_lifecycle
                .as_mut()
                .expect("validated")
                .delivery = PostDelivery::Superseded;
            Ok(())
        })
    }

    pub(crate) fn reserve_post_correction(
        &self,
        id: u64,
        phase: &str,
        identity: Option<&super::Identity>,
    ) -> Result<u64> {
        self.admission(|target| {
            let mut staged = target.clone();
            ensure_continuation_except(&staged, phase, Some(id), None)?;
            let post = staged
                .operations
                .iter()
                .find(|o| o.id == id && o.phase == phase)
                .and_then(|o| o.tool_receipt.as_ref())
                .and_then(|r| r.plugin_lifecycle.as_ref())
                .context("post-tool correction belongs to another phase")?;
            ensure!(
                post.continuation == PostContinuation::Correction && post.correction_required,
                "post-tool continuation has no correction authority"
            );
            if let Some(error) =
                hold_incompatible_presentation(&mut staged, id, PostDelivery::Superseded)?
            {
                *target = staged;
                return Ok(Err(error));
            }
            let invocation = staged.operations.len() as u64 + 1;
            release(
                &mut staged,
                id,
                PostDelivery::Superseded,
                PostDelivery::CorrectionReserved { invocation },
            )?;
            let admitted = delegation::begin_backend_record(&mut staged, phase, identity, None)?;
            ensure!(
                admitted == invocation,
                "correction backend reservation changed"
            );
            let post = staged
                .operations
                .iter_mut()
                .find(|operation| operation.id == id)
                .and_then(|operation| operation.tool_receipt.as_mut())
                .and_then(|receipt| receipt.plugin_lifecycle.as_mut())
                .expect("validated");
            if post.facts.representation.is_mcp()
                && post
                    .model_content
                    .as_ref()
                    .is_some_and(serde_json::Value::is_array)
            {
                post.correction_presentation = Some(CorrectionPresentation::ClaudeProviderBlocksV1);
            }
            *target = staged;
            Ok(Ok(invocation))
        })?
    }

    pub(crate) fn ack_post_correction(
        &self,
        id: u64,
        invocation: u64,
        acknowledgment: CorrectionAcknowledgment,
    ) -> Result<()> {
        self.update(|record| {
            ensure!(
                !record.recovery_pending,
                "post-tool correction needs reconciliation"
            );
            let operation = record
                .operations
                .iter()
                .find(|o| o.id == id)
                .context("post-tool correction owner missing")?;
            let post = operation
                .tool_receipt
                .as_ref()
                .and_then(|r| r.plugin_lifecycle.as_ref())
                .context("post-tool correction missing")?;
            ensure!(
                post.delivery == (PostDelivery::CorrectionReserved { invocation })
                    && post.continuation == PostContinuation::Correction
                    && post.correction_admitted
                    && owner::validate(record, &post.facts).is_ok()
                    && record.operations.iter().any(|o| o.id == invocation
                        && o.phase == operation.phase
                        && matches!(o.host_invocation, Some(super::HostInvocation::Backend))
                        && !o.complete
                        && !o.reconciled),
                "post-tool correction acknowledgment is stale, repeated or unrelated"
            );
            ensure!(
                match (&post.facts.representation, &acknowledgment) {
                    (
                        ToolRepresentation::ClaudeMcp { source_input, .. },
                        CorrectionAcknowledgment::ClaudeUser {
                            session_id,
                            uuid,
                            content_digest,
                        },
                    ) =>
                        source_input["session_id"] == *session_id
                            && uuid.len() == 36
                            && content_digest.len() == 64,
                    (
                        ToolRepresentation::CodexDynamic {
                            session_id,
                            turn_id: original_turn,
                            ..
                        },
                        CorrectionAcknowledgment::CodexTurn {
                            thread_id, turn_id, ..
                        },
                    ) => session_id == thread_id && !turn_id.is_empty() && turn_id != original_turn,
                    _ => false,
                },
                "post-tool correction source acknowledgment differs from its owner"
            );
            record
                .operations
                .iter_mut()
                .find(|o| o.id == id)
                .expect("validated")
                .tool_receipt
                .as_mut()
                .expect("validated")
                .plugin_lifecycle
                .as_mut()
                .expect("validated")
                .delivery = PostDelivery::CorrectionAcknowledged {
                invocation,
                acknowledgment,
            };
            Ok(())
        })
    }
    pub(crate) fn reserve_post_delivery(&self, id: u64) -> Result<()> {
        // A compatibility rejection must itself be durable. Return the nested
        // error only after admission persists the held receipt under this lock.
        self.admission(|record| {
            if let Some(error) = hold_incompatible_presentation(record, id, PostDelivery::Staged)? {
                return Ok(Err(error));
            }
            release(record, id, PostDelivery::Staged, PostDelivery::Reserved)?;
            Ok(Ok(()))
        })?
    }
    pub(crate) fn ack_post_delivery(&self, id: u64) -> Result<()> {
        self.update(|record| {
            let lifecycle = record
                .operations
                .iter_mut()
                .find(|o| o.id == id)
                .and_then(|o| o.tool_receipt.as_mut())
                .and_then(|r| r.plugin_lifecycle.as_mut())
                .context("post-tool receipt missing")?;
            ensure!(
                lifecycle.delivery == PostDelivery::Reserved,
                "post-tool presentation was not reserved"
            );
            lifecycle.delivery = PostDelivery::Acknowledged;
            Ok(())
        })
    }
    pub(crate) fn hold_post_delivery(&self, id: u64, reason: &str) -> Result<()> {
        self.update(|record| {
            let lifecycle = record
                .operations
                .iter_mut()
                .find(|o| o.id == id)
                .and_then(|o| o.tool_receipt.as_mut())
                .and_then(|r| r.plugin_lifecycle.as_mut())
                .context("post-tool receipt missing")?;
            lifecycle.continuation = PostContinuation::Held {
                reason: reason.chars().take(4096).collect(),
            };
            Ok(())
        })
    }
}

/// Publish release and spend any correction atomically, only after fresh evidence
/// was validated. A failed candidate leaves both task and receipt unchanged.
fn release(target: &mut Record, id: u64, expected: PostDelivery, next: PostDelivery) -> Result<()> {
    let mut staged = target.clone();
    ensure!(
        !staged.recovery_pending,
        "post-tool release needs reconciliation"
    );
    let operation = staged
        .operations
        .iter()
        .find(|o| o.id == id)
        .context("post-tool release operation missing")?;
    ensure!(
        !operation.reconciled,
        "quarantined post-tool delivery cannot be released"
    );
    delegation::ensure_agent_active(&staged, &operation.phase)?;
    let lifecycle = operation
        .tool_receipt
        .as_ref()
        .and_then(|r| r.plugin_lifecycle.as_ref())
        .context("post-tool receipt missing")?;
    ensure!(
        lifecycle.delivery == expected
            && lifecycle.settled
            && !matches!(lifecycle.continuation, PostContinuation::Held { .. }),
        "post-tool release repeated, held or unfinished"
    );
    ensure!(
        lifecycle.facts.task == staged.task.as_ref().map(|t| t.id)
            && staged.task.as_ref().is_none_or(|t| (!t.stopped
                || lifecycle.facts.role == "verification"
                || lifecycle.facts.role.starts_with("agent:"))
                && t.accepted.is_none()),
        "post-tool release task owner changed or stopped"
    );
    let correction = lifecycle.correction_required;
    if correction {
        ensure!(
            !lifecycle.correction_admitted
                && staged.allocation.as_ref().is_some_and(|a| a
                    .remaining_ms()
                    .is_ok_and(|ms| ms > 0)
                    && a.model_calls < a.limits.model_calls
                    && a.tool_calls < a.limits.tool_calls),
            "post-tool correction allocation unavailable"
        );
        let facts = lifecycle.facts.clone();
        owner::charge(&mut staged, &facts)?;
    }
    let lifecycle = staged
        .operations
        .iter_mut()
        .find(|o| o.id == id)
        .expect("validated")
        .tool_receipt
        .as_mut()
        .expect("validated")
        .plugin_lifecycle
        .as_mut()
        .expect("validated");
    lifecycle.correction_admitted = correction;
    lifecycle.delivery = next;
    *target = staged;
    Ok(())
}

/// Persist a compatibility hold through the caller's nested-result transaction.
/// No reservation, invocation or correction charge occurs on this path.
fn hold_incompatible_presentation(
    record: &mut Record,
    id: u64,
    expected: PostDelivery,
) -> Result<Option<anyhow::Error>> {
    let post = record
        .operations
        .iter()
        .find(|operation| operation.id == id)
        .and_then(|operation| operation.tool_receipt.as_ref())
        .and_then(|receipt| receipt.plugin_lifecycle.as_ref())
        .context("post-tool receipt missing")?;
    if post.delivery != expected {
        return Ok(None);
    }
    let Err(error) = crate::plugins::lifecycle::claude_content::validate_release(record, post)
    else {
        return Ok(None);
    };
    let reason = format!("post-tool presentation held before delivery: {error}");
    let post = record
        .operations
        .iter_mut()
        .find(|operation| operation.id == id)
        .and_then(|operation| operation.tool_receipt.as_mut())
        .and_then(|receipt| receipt.plugin_lifecycle.as_mut())
        .expect("validated");
    post.continuation = PostContinuation::Held { reason };
    post.model_content = None;
    for proposal in &mut post.proposals {
        if matches!(proposal.proposal.kind, ProposalKind::ReplaceModelOutput) {
            proposal.disposition = ProposalDisposition::Held;
        }
    }
    post.messages
        .retain(|message| !matches!(message.kind, ProposalKind::ReplaceModelOutput));
    Ok(Some(error))
}
