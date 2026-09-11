//! Owned UserPromptSubmit and ordinary Stop dispatch. Source events require actual callbacks.
use super::{
    admission::digest,
    dispatch::{HookInvocation, PreToolPlan, Registration},
    gate_snapshot::GateWorkspace,
    hook_types::{HookDialect, HookEvent},
    receipts::*,
    results::{ControlRequest, DecisionChoice, ProposedEffect, ResultContext, ResultRole},
};
use crate::{
    events::{Event, EventSink},
    tools::ToolExecutor,
};
use anyhow::{Result, ensure};
use std::{collections::BTreeMap, sync::Arc};

pub struct NonToolPlan {
    pub(crate) plan: PreToolPlan,
}
impl NonToolPlan {
    pub fn new(event: HookEvent, registrations: Vec<Registration>) -> Result<Self> {
        ensure!(
            matches!(event, HookEvent::UserPromptSubmit | HookEvent::Stop),
            "non-tool plan requires UserPromptSubmit or Stop"
        );
        ensure!(
            registrations
                .iter()
                .all(|r| r.declaration.matcher.tool.is_none()
                    && r.declaration.matcher.path.is_none()),
            "tool and path matchers do not apply to prompt or Stop events"
        );
        Ok(Self {
            plan: PreToolPlan::for_event(event, registrations)?,
        })
    }
}

#[derive(Default)]
pub(crate) struct NonToolEffects {
    pub(crate) hold: Option<String>,
    pub(crate) correction: bool,
    pub(crate) proposals: Vec<AppliedProposal>,
    pub(crate) messages: Vec<PluginMessage>,
    pub(crate) diagnostics: Vec<String>,
    pub(crate) successful: Vec<u32>,
}
impl NonToolEffects {
    fn hold(&mut self, reason: impl Into<String>) {
        self.hold
            .get_or_insert_with(|| reason.into().chars().take(4096).collect());
    }
    fn apply(
        &mut self,
        receipt: HookReceipt,
        decoded: super::results::DecodedResult,
        event: HookEvent,
    ) -> Result<()> {
        if receipt.once.is_some() && super::once::succeeded(&receipt, &decoded) {
            self.successful.push(receipt.invocation);
        }
        if decoded.failed() {
            self.diagnostics.push(format!(
                "Plugin-origin {}: invalid or failed lifecycle response",
                receipt.declaration.package
            ));
            if receipt.required_gate {
                self.hold("required lifecycle handler failed");
            }
        }
        for (index, effect) in decoded.effects.into_iter().enumerate() {
            let proposal = PendingProposal {
                index,
                kind: (&effect).into(),
            };
            let mut disposition = ProposalDisposition::Applied;
            match effect {
                ProposedEffect::AdditionalContext(text)
                | ProposedEffect::ClassifierContext(text)
                | ProposedEffect::Feedback(text)
                | ProposedEffect::Warning(text)
                | ProposedEffect::TransientNotice(text) => {
                    ensure!(
                        text.get().len() <= 65536,
                        "plugin context exceeds lifecycle bound"
                    );
                    self.messages.push(PluginMessage {
                        invocation: receipt.invocation,
                        package: receipt.declaration.package.clone(),
                        kind: proposal.kind.clone(),
                        text: text.get().clone(),
                    });
                }
                ProposedEffect::Decision {
                    choice: DecisionChoice::NoObjection,
                    ..
                } => {}
                ProposedEffect::Decision { reason, .. } => {
                    disposition = ProposalDisposition::Held;
                    if receipt.required_gate {
                        self.hold(
                            reason
                                .map(|r| r.get().clone())
                                .unwrap_or_else(|| "lifecycle gate denied continuation".into()),
                        );
                    }
                }
                ProposedEffect::Control(ControlRequest::Followup(_))
                    if event == HookEvent::Stop && receipt.required_gate =>
                {
                    self.correction = true
                }
                ProposedEffect::Control(_) if receipt.required_gate => {
                    disposition = ProposalDisposition::Held;
                    self.hold("lifecycle continuation unmet or correction allowance exhausted");
                }
                _ => {
                    disposition = ProposalDisposition::Pending;
                    if receipt.required_gate {
                        self.hold("lifecycle proposal owner is unavailable");
                    }
                }
            }
            self.proposals.push(AppliedProposal {
                invocation: receipt.invocation,
                proposal,
                disposition,
            });
        }
        Ok(())
    }
}

pub(crate) struct NonToolOutcome {
    pub(crate) operation: u64,
    pub(crate) hold: Option<String>,
    pub(crate) correction: bool,
    pub(crate) context: String,
}
impl NonToolPlan {
    pub(crate) async fn dispatch(
        &self,
        occurrence: NonToolOccurrence,
        events: &EventSink,
        workspace: Arc<GateWorkspace>,
        expected_workspace: (u64, u64),
        executor: &ToolExecutor,
    ) -> Result<NonToolOutcome> {
        let event = self.plan.event;
        ensure!(
            occurrence.event() == event,
            "lifecycle plan event differs from occurrence"
        );
        let (events, facts) = events.for_non_tool(
            occurrence,
            self.plan.digest.clone(),
            self.plan
                .handlers
                .iter()
                .map(|h| serde_json::to_value(&h.registration.declaration))
                .collect::<Result<Vec<_>, _>>()?,
        )?;
        let (runtime, operation) = events.plugin_context()?;
        let _owner = runtime.own_post_lifecycle(operation, event)?;
        ensure!(
            facts.workspace == expected_workspace,
            "lifecycle workspace owner differs"
        );
        let mut effects = NonToolEffects::default();
        let mut groups: Vec<Vec<usize>> = Vec::new();
        let mut named = BTreeMap::new();
        for (index, handler) in self.plan.handlers.iter().enumerate() {
            let d = &handler.registration.declaration;
            if let Some(group) = &d.concurrent_group {
                let position = *named
                    .entry((
                        d.identity.scope.clone(),
                        d.identity.package.clone(),
                        d.source.as_ref().map(|s| s.0.identity.clone()),
                        group.clone(),
                    ))
                    .or_insert_with(|| {
                        groups.push(Vec::new());
                        groups.len() - 1
                    });
                groups[position].push(index);
            } else {
                groups.push(vec![index]);
            }
        }
        let mut retained_bytes = 0usize;
        let mut validation = Vec::new();
        for indices in groups {
            let lease = Arc::new(
                tokio::time::timeout(
                    runtime.remaining()?,
                    self.plan
                        .runners
                        .clone()
                        .acquire_many_owned(indices.len() as u32),
                )
                .await??,
            );
            runtime.plugin_runner_owner(operation, event)?;
            let boundary = runtime.mutation_boundary(expected_workspace)?;
            let guard =
                Arc::new(tokio::time::timeout(runtime.remaining()?, boundary.lock_owned()).await?);
            let mut snapshots = BTreeMap::new();
            for &index in &indices {
                let d = &self.plan.handlers[index].registration.declaration;
                ensure!(
                    d.identity.role == facts.declaration_role.as_deref().unwrap_or("worker"),
                    "lifecycle declaration has a different owner"
                );
                // The native host occurrence cannot masquerade as a backend event, even for a callback runner.
                if d.identity.dialect != HookDialect::Native && facts.source.is_none() {
                    effects.hold(
                        "source lifecycle handler requires an actual observed backend callback",
                    );
                    break;
                }
                ensure!(
                    d.external_precondition.is_none(),
                    "lifecycle external atomic precondition unavailable"
                );
                let key = digest(&d.reads)?;
                if let std::collections::btree_map::Entry::Vacant(entry) = snapshots.entry(key) {
                    let snapshot =
                        super::lifecycle::capture(&self.plan, workspace.clone(), d.reads.clone())
                            .await?;
                    ensure!(
                        snapshot.root_identity() == expected_workspace,
                        "lifecycle snapshot belongs to another workspace"
                    );
                    retained_bytes = retained_bytes
                        .saturating_add(
                            snapshot
                                .entries()
                                .map(|(n, e)| n.len() + e.bytes().len() + 128)
                                .sum::<usize>(),
                        )
                        .saturating_add(serde_json::to_vec(snapshot.memberships())?.len())
                        .saturating_add(serde_json::to_vec(snapshot.glob_matches())?.len())
                        .saturating_add(serde_json::to_vec(snapshot.absent_paths())?.len());
                    ensure!(
                        retained_bytes <= 16 * 1024 * 1024,
                        "lifecycle snapshots exceed aggregate bound"
                    );
                    entry.insert(snapshot);
                }
            }
            if effects.hold.is_some() {
                break;
            }
            let key = AdmissionKey {
                session: facts.session.clone(),
                operation,
                source_operation: operation,
                event: event.as_str().into(),
                tool: None,
                arguments: None,
                lifecycle: Some(facts.subject.clone()),
                plan: self.plan.digest.clone(),
                role: facts.role.clone(),
                workspace: expected_workspace,
                inputs: snapshots
                    .iter()
                    .map(|(k, s)| (k.clone(), s.revision().into()))
                    .collect(),
                external: None,
            };
            let mut runnable = Vec::new();
            for &index in &indices {
                let registration = &self.plan.handlers[index].registration;
                let d = &registration.declaration;
                let hook = HookReceipt {
                    required_gate: d.required_gate,
                    observer: None,
                    source: d.source.as_ref().map(|s| s.0.clone()),
                    once: None,
                    invocation: 0,
                    declaration: d.identity.clone(),
                    class: d.class,
                    endpoint: None,
                    inspected: key.clone(),
                    outcome: None,
                    uncertain_effects: true,
                    hold: None,
                    questions: vec![],
                    pending_proposals: vec![],
                };
                if let super::once::HookReservation::Run(hook) = runtime.reserve_non_tool_hook(
                    operation,
                    event,
                    hook,
                    d.once.as_ref(),
                    Some(&lease),
                )? {
                    let invocation = HookInvocation {
                        required_gate: d.required_gate,
                        observer: None,
                        invocation: hook.invocation,
                        key: key.clone(),
                        declaration: d.identity.clone(),
                        endpoint: None,
                        candidate: None,
                        lifecycle: Some(facts.clone()),
                        completed: None,
                        snapshot: snapshots[&digest(&d.reads)?].clone(),
                        events: events.clone(),
                        host: executor.hook_host(),
                        runner_lease: lease.clone(),
                        mutation_guard: registration
                            .runner
                            .mutates_workspace()
                            .then(|| guard.clone()),
                        class: d.class,
                    };
                    runnable.push((*hook, invocation, registration.runner.clone()));
                }
            }
            drop(guard);
            let mut prepared = Vec::with_capacity(runnable.len());
            for (receipt, invocation, runner) in runnable {
                runtime.plugin_runner_owner(operation, event)?;
                let preparation = tokio::time::timeout(
                    runtime.remaining()?.min(std::time::Duration::from_secs(30)),
                    runner.prepare(&invocation),
                )
                .await;
                let failure = match preparation {
                    Ok(Ok(())) => None,
                    Ok(Err(error)) => {
                        Some(format!("lifecycle handler preparation failed: {error:#}"))
                    }
                    Err(_) => Some("lifecycle handler preparation timed out".into()),
                };
                prepared.push((receipt, invocation, runner, failure));
            }
            let work = runtime.non_tool_model_context(operation, event)?;
            let outcomes = futures_util::future::join_all(prepared.into_iter().map(
                |(mut receipt, invocation, runner, failure)| async move {
                    let mut outcome = if let Some(reason) = failure {
                        RawOutcome::Failure { reason }
                    } else {
                        let Some(outcome) =
                            super::observer::dispatch(invocation, runner.clone()).await?
                        else {
                            return Ok::<_, anyhow::Error>(None);
                        };
                        outcome
                    };
                    if !outcome.within_retention_bound() {
                        outcome = RawOutcome::Failure {
                            reason: "handler output exceeded retention bound".into(),
                        };
                    }
                    receipt.uncertain_effects = !runner.side_effect_free()
                        && matches!(
                            outcome,
                            RawOutcome::Failure { .. } | RawOutcome::CommandFailure { .. }
                        );
                    receipt.outcome = Some(outcome);
                    Ok(Some(receipt))
                },
            ))
            .await
            .into_iter()
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
            for receipt in &outcomes {
                runtime.finish_non_tool_hook(operation, event, receipt.clone())?;
            }
            for &index in &indices {
                let d = &self.plan.handlers[index].registration.declaration;
                if d.class != HandlerClass::Observer
                    && outcomes.iter().any(|r| r.declaration == d.identity)
                {
                    validation.push((d.reads.clone(), snapshots[&digest(&d.reads)?].clone()));
                }
            }
            for receipt in outcomes {
                let context = ResultContext {
                    role: if receipt.required_gate {
                        ResultRole::RequiredGate
                    } else {
                        ResultRole::Observer
                    },
                    work,
                    ..Default::default()
                };
                let decoded = receipt.outcome.as_ref().expect("retained").decode_for(
                    &self.plan.profile,
                    &receipt.declaration,
                    event,
                    &context,
                );
                effects.apply(receipt, decoded, event)?;
            }
            if effects.hold.is_some() {
                break;
            }
        }
        let boundary = runtime.mutation_boundary(expected_workspace)?;
        let _guard = tokio::time::timeout(runtime.remaining()?, boundary.lock_owned()).await?;
        for (reads, previous) in validation {
            let current = super::lifecycle::capture(&self.plan, workspace.clone(), reads).await?;
            if current.root_identity() != previous.root_identity()
                || current.revision() != previous.revision()
            {
                effects.hold("lifecycle inspected inputs changed before continuation");
                break;
            }
        }
        let context = effects
            .messages
            .iter()
            .map(|m| format!("[Plugin-origin {}] {}", m.package, m.text))
            .collect::<Vec<_>>()
            .join("\n");
        ensure!(
            context.len() <= 2 * 1024 * 1024,
            "lifecycle context exceeds aggregate bound"
        );
        for diagnostic in &effects.diagnostics {
            events.emit_advisory(Event::Error {
                message: diagnostic.clone(),
            })?;
        }
        if let Some(reason) = &effects.hold {
            events.emit_advisory(Event::Error {
                message: format!("{} blocked: {reason}", event.as_str()),
            })?;
        }
        let result = NonToolOutcome {
            operation,
            hold: effects.hold.clone(),
            correction: effects.correction,
            context,
        };
        runtime.settle_non_tool(operation, event, effects)?;
        Ok(result)
    }
}
