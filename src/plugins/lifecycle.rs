//! Synchronous lifecycle ownership after a retained, actually admitted tool attempt.
pub(crate) mod claude_content;
use super::{
    admission::{candidate_digest, digest},
    dispatch::{CompletedTool, HookInvocation, PreToolPlan, Registration},
    gate_snapshot::{GateReadSet, GateSnapshot, GateWorkspace},
    hook_types::HookEvent,
    receipts::*,
    results::{ControlRequest, DecisionChoice, ProposedEffect, ResultContext, ResultRole},
};
use crate::{
    events::{Event, EventSink},
    tools::{ToolCall, ToolExecutor, ToolResult},
};
use anyhow::{Context, Result, ensure};
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

pub struct PostToolPlan {
    pub(crate) plan: PreToolPlan,
}
impl PostToolPlan {
    pub fn new(event: HookEvent, registrations: Vec<Registration>) -> Result<Self> {
        ensure!(
            matches!(
                event,
                HookEvent::PostToolUse | HookEvent::PostToolUseFailure
            ),
            "post-tool plan requires a post-tool event"
        );
        Ok(Self {
            plan: PreToolPlan::for_event(event, registrations)?,
        })
    }
}

#[derive(Default)]
pub(crate) struct PostEffects {
    pub(crate) continuation: PostContinuation,
    pub(crate) proposals: Vec<AppliedProposal>,
    pub(crate) messages: Vec<PluginMessage>,
    pub(crate) diagnostics: Vec<String>,
    pub(crate) model_content: Option<serde_json::Value>,
    output: Option<String>,
    pub(crate) consume_correction: bool,
    /// Exact validated source results; retained only when their proposals settle.
    pub(crate) once_successful: Vec<u32>,
}
impl PostEffects {
    fn hold(&mut self, reason: &str) {
        if !matches!(self.continuation, PostContinuation::Held { .. }) {
            self.continuation = PostContinuation::Held {
                reason: reason.chars().take(4096).collect(),
            };
        }
    }
    fn apply_group(
        &mut self,
        decoded: Vec<(HookReceipt, super::results::DecodedResult)>,
        mcp: bool,
        search_policy: Option<bool>,
    ) -> Result<()> {
        let mut replacement: Option<serde_json::Value> = None;
        let mut replacement_proposals = Vec::new();
        let mut replacement_messages = Vec::new();
        let mut conflict = false;
        for (receipt, result) in decoded {
            if receipt.once.is_some() && super::once::succeeded(&receipt, &result) {
                self.once_successful.push(receipt.invocation);
            }
            self.consume_correction |= matches!(
                result.model_outcome,
                Some(
                    super::hook_types::ModelOutcome::ContinueAfterResult
                        | super::hook_types::ModelOutcome::ContinueWithFailure
                        | super::hook_types::ModelOutcome::BoundedCorrection
                )
            );
            if receipt.uncertain_effects {
                self.hold("post-tool handler effects are uncertain; reconciliation required");
            }
            if result.failed() {
                self.diagnostics.push(format!(
                    "plugin {} returned an invalid or failed post-tool result",
                    receipt.declaration.package
                ));
                if receipt.required_gate {
                    self.hold("required post-tool handler failed");
                }
            }
            for (index, effect) in result.effects.into_iter().enumerate() {
                let proposal = PendingProposal {
                    index,
                    kind: (&effect).into(),
                };
                let mut disposition = ProposalDisposition::Applied;
                match effect {
                    ProposedEffect::AdditionalContext(text) | ProposedEffect::ClassifierContext(text)
                    | ProposedEffect::Feedback(text) | ProposedEffect::Warning(text) => {
                        ensure!(text.get().len() <= 65536, "plugin context exceeds bound");
                        self.messages.push(PluginMessage { invocation: receipt.invocation, package: receipt.declaration.package.clone(), kind: proposal.kind.clone(), text: text.get().clone() });
                    }
                    ProposedEffect::ReplaceModelOutput { value, .. } => {
                        if let Some(previous) = &replacement { conflict |= previous != value.get(); }
                        else { replacement = Some(value.get().clone()); }
                        replacement_proposals.push(self.proposals.len());
                        replacement_messages.push(PluginMessage {
                            invocation: receipt.invocation,
                            package: receipt.declaration.package.clone(),
                            kind: proposal.kind.clone(),
                            text: "The tool output is a plugin-provided replacement; the original tool result remains separate execution evidence.".into(),
                        });
                    }
                    ProposedEffect::Control(ControlRequest::Followup(_)) => {
                        if !matches!(self.continuation, PostContinuation::Held { .. }) { self.continuation = PostContinuation::Correction; }
                    }
                    ProposedEffect::Control(_) => self.hold("post-tool handler requires continuation to stop; completed evidence is retained"),
                    ProposedEffect::Decision { choice: DecisionChoice::NoObjection, .. } => {}
                    ProposedEffect::Decision { .. } => {
                        disposition = ProposalDisposition::Held;
                        self.hold("post-tool decision remains unmet");
                    }
                    _ => {
                        disposition = ProposalDisposition::Pending;
                        if receipt.required_gate { self.hold("post-tool proposal owner is not integrated"); }
                    }
                }
                self.proposals.push(AppliedProposal {
                    invocation: receipt.invocation,
                    proposal,
                    disposition,
                });
            }
        }
        if conflict {
            self.hold(
                "concurrent post-tool result replacements disagree; original presentation retained",
            );
            for index in replacement_proposals {
                self.proposals[index].disposition = ProposalDisposition::Held;
            }
            self.output = None;
            self.model_content = None;
            self.messages
                .retain(|m| !matches!(m.kind, ProposalKind::ReplaceModelOutput));
        } else if let Some(value) = replacement {
            let applied = if mcp {
                match claude_content::validate(&value, search_policy) {
                    Ok(()) => {
                        self.model_content = Some(value);
                        true
                    }
                    Err(error) => {
                        self.hold(&format!("unsupported Claude MCP presentation: {error}; original outcome retained"));
                        for index in replacement_proposals {
                            self.proposals[index].disposition = ProposalDisposition::Held;
                        }
                        false
                    }
                }
            } else {
                self.output = Some(
                    value
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or(serde_json::to_string(&value)?),
                );
                true
            };
            if applied {
                self.messages
                    .retain(|m| !matches!(m.kind, ProposalKind::ReplaceModelOutput));
                self.messages.extend(replacement_messages);
            }
        }
        ensure!(
            self.messages.iter().map(|m| m.text.len()).sum::<usize>() <= 1024 * 1024,
            "aggregate plugin context exceeds 1 MiB"
        );
        Ok(())
    }
    fn model_result(&self, original_view: &ToolResult) -> Result<ToolResult> {
        let mut result = original_view.clone();
        if let Some(output) = &self.output {
            result.output = output.clone();
        }
        for message in &self.messages {
            let kind = serde_json::to_string(&message.kind)?;
            result.output.push_str(&format!(
                "\n[Plugin-origin {} {}]\n{}",
                message.package, kind, message.text
            ));
        }
        ensure!(
            result.output.len() <= 2 * 1024 * 1024,
            "model-facing result exceeds retention limit"
        );
        Ok(result)
    }
}

pub(crate) struct PostValidation {
    pub(crate) workspace: Arc<GateWorkspace>,
    pub(crate) plan: Arc<PostToolPlan>,
    pub(crate) snapshots: Vec<(GateReadSet, Arc<GateSnapshot>)>,
}
impl PostValidation {
    pub(crate) async fn validate(&self) -> Result<()> {
        for (reads, previous) in &self.snapshots {
            let current = capture(&self.plan.plan, self.workspace.clone(), reads.clone()).await?;
            ensure!(
                current.root_identity() == previous.root_identity()
                    && current.revision() == previous.revision(),
                "post-tool inspected inputs changed before continuation release"
            );
        }
        Ok(())
    }
}
struct CaptureCancellation(Arc<AtomicBool>);
impl Drop for CaptureCancellation {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}
pub(crate) async fn capture(
    plan: &PreToolPlan,
    workspace: Arc<GateWorkspace>,
    reads: GateReadSet,
) -> Result<Arc<GateSnapshot>> {
    let permit = plan.captures.clone().acquire_owned().await?;
    let cancellation = CaptureCancellation(Arc::new(AtomicBool::new(false)));
    let flag = cancellation.0.clone();
    Ok(Arc::new(
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            workspace.capture(&reads, &flag)
        })
        .await??,
    ))
}

impl PostToolPlan {
    pub(crate) async fn dispatch(
        self: &Arc<Self>,
        call: &ToolCall,
        model_view: &ToolResult,
        events: &EventSink,
        workspace: Arc<GateWorkspace>,
        expected_workspace: (u64, u64),
        executor: &ToolExecutor,
    ) -> Result<ToolResult> {
        let event = self.plan.event;
        let events = events.for_plugin_event(event);
        let (runtime, operation) = events.plugin_context()?;
        let original = events
            .original_tool_evidence(&call.id)?
            .context("post-tool original evidence missing")?;
        let facts = runtime.begin_post_tool(
            operation,
            event,
            self.plan.digest.clone(),
            self.plan
                .handlers
                .iter()
                .map(|h| serde_json::to_value(&h.registration.declaration))
                .collect::<Result<Vec<_>, _>>()?,
            events.tool_representation(),
        )?;
        // Register before any reservation; retain ownership through proposal settlement.
        let _lifecycle_owner = runtime.own_post_lifecycle(operation, event)?;
        let mut groups: Vec<Vec<usize>> = Vec::new();
        let mut named = BTreeMap::new();
        for (index, handler) in self
            .plan
            .handlers
            .iter()
            .enumerate()
            .filter(|(_, h)| h.matches(call))
        {
            let declaration = &handler.registration.declaration;
            if let Some(group) = &declaration.concurrent_group {
                let position = *named
                    .entry((
                        declaration.identity.scope.clone(),
                        declaration.identity.package.clone(),
                        declaration.source.as_ref().map(|s| s.0.identity.clone()),
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
        let mut effects = PostEffects::default();
        let mut retained_bytes = 0usize;
        let mut validation = PostValidation {
            workspace: workspace.clone(),
            plan: self.clone(),
            snapshots: Vec::new(),
        };
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
            let boundary = events
                .mutation_boundary(expected_workspace)?
                .ok_or_else(|| anyhow::anyhow!("post-tool mutation boundary missing"))?;
            let guard =
                Arc::new(tokio::time::timeout(runtime.remaining()?, boundary.lock_owned()).await?);
            let mut snapshots = BTreeMap::new();
            for index in &indices {
                let reads = self.plan.handlers[*index]
                    .registration
                    .declaration
                    .reads
                    .clone();
                let read_id = digest(&reads)?;
                if let std::collections::btree_map::Entry::Vacant(entry) = snapshots.entry(read_id)
                {
                    let snapshot = capture(&self.plan, workspace.clone(), reads).await?;
                    ensure!(
                        snapshot.root_identity() == expected_workspace,
                        "post-tool snapshot belongs to another workspace"
                    );
                    retained_bytes = retained_bytes
                        .saturating_add(
                            snapshot
                                .entries()
                                .map(|(name, entry)| name.len() + entry.bytes().len() + 128)
                                .sum::<usize>(),
                        )
                        .saturating_add(serde_json::to_vec(snapshot.memberships())?.len())
                        .saturating_add(serde_json::to_vec(snapshot.glob_matches())?.len())
                        .saturating_add(serde_json::to_vec(snapshot.absent_paths())?.len());
                    ensure!(
                        retained_bytes <= 16 * 1024 * 1024,
                        "post-tool snapshots exceed aggregate bound"
                    );
                    entry.insert(snapshot);
                }
            }
            let inputs = snapshots
                .iter()
                .map(|(id, s)| (id.clone(), s.revision().to_owned()))
                .collect();
            let key = AdmissionKey {
                session: facts.session.clone(),
                operation,
                source_operation: facts.source_operation,
                event: event.as_str().into(),
                tool: Some(call.name.clone()),
                lifecycle: None,
                arguments: Some(candidate_digest(call)?),
                plan: self.plan.digest.clone(),
                role: facts.role.clone(),
                workspace: expected_workspace,
                inputs,
                external: None,
            };
            let mut jobs = Vec::new();
            for &index in &indices {
                let registration = &self.plan.handlers[index].registration;
                let declaration = &registration.declaration;
                ensure!(
                    declaration.identity.role == facts.role,
                    "post-tool declaration has a different role"
                );
                ensure!(
                    declaration.external_precondition.is_none(),
                    "post-tool external atomic precondition is unavailable"
                );
                let receipt = HookReceipt {
                    required_gate: declaration.required_gate,
                    observer: None,
                    source: declaration.source.as_ref().map(|s| s.0.clone()),
                    once: None,
                    invocation: 0,
                    declaration: declaration.identity.clone(),
                    class: declaration.class,
                    endpoint: None,
                    inspected: key.clone(),
                    outcome: None,
                    uncertain_effects: true,
                    hold: None,
                    questions: Vec::new(),
                    pending_proposals: Vec::new(),
                };
                let invocation = HookInvocation {
                    required_gate: declaration.required_gate,
                    observer: None,
                    invocation: 0,
                    key: key.clone(),
                    declaration: declaration.identity.clone(),
                    endpoint: None,
                    candidate: Some(call.clone()),
                    lifecycle: None,
                    snapshot: snapshots
                        .get(&digest(&declaration.reads)?)
                        .expect("captured")
                        .clone(),
                    completed: Some(CompletedTool {
                        facts: facts.clone(),
                        original: original.clone(),
                    }),
                    events: events.clone(),
                    host: executor.hook_host(),
                    runner_lease: lease.clone(),
                    mutation_guard: registration
                        .runner
                        .mutates_workspace()
                        .then(|| guard.clone()),
                    class: declaration.class,
                };
                jobs.push((
                    receipt,
                    invocation,
                    registration.runner.clone(),
                    declaration.once.as_ref(),
                ));
            }
            drop(guard);
            // A current declaration without once can still have an unresolved
            // historical attempt. No member prepares until every reservation passes.
            let mut runnable = Vec::new();
            for (receipt, mut invocation, runner, binding) in jobs {
                if let super::once::HookReservation::Run(receipt) =
                    runtime.reserve_post_hook(operation, event, receipt, binding, Some(&lease))?
                {
                    invocation.invocation = receipt.invocation;
                    runnable.push((*receipt, invocation, runner));
                }
            }
            for (_, invocation, runner) in &runnable {
                runtime.plugin_runner_owner(operation, event)?;
                tokio::time::timeout(
                    runtime.remaining()?.min(std::time::Duration::from_secs(30)),
                    runner.prepare(invocation),
                )
                .await??;
            }
            let work = runtime.post_model_context(operation, event)?;
            let outcomes = futures_util::future::join_all(runnable.into_iter().map(
                |(mut receipt, invocation, runner)| async move {
                    let Some(mut outcome) =
                        super::observer::dispatch(invocation, runner.clone()).await?
                    else {
                        return Ok::<_, anyhow::Error>(None);
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
            .await;
            let outcomes = outcomes
                .into_iter()
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .flatten()
                .collect::<Vec<_>>();
            for index in &indices {
                let reads = self.plan.handlers[*index]
                    .registration
                    .declaration
                    .reads
                    .clone();
                if self.plan.handlers[*index].registration.declaration.class
                    != HandlerClass::Observer
                    && outcomes.iter().any(|receipt| {
                        receipt.declaration
                            == self.plan.handlers[*index].registration.declaration.identity
                    })
                {
                    validation
                        .snapshots
                        .push((reads.clone(), snapshots[&digest(&reads)?].clone()));
                }
            }
            // All source outcomes are durable before decoding/applying this group's effects.
            for receipt in &outcomes {
                runtime.finish_post_hook(operation, event, receipt.clone())?;
            }
            let decoded = outcomes
                .into_iter()
                .map(|receipt| {
                    let context = ResultContext {
                        role: if !receipt.required_gate {
                            ResultRole::Observer
                        } else {
                            ResultRole::RequiredGate
                        },
                        work,
                        tool_is_mcp: facts.representation.is_mcp(),
                        ..ResultContext::default()
                    };
                    let result = receipt.outcome.as_ref().expect("retained").decode_for(
                        &self.plan.profile,
                        &receipt.declaration,
                        event,
                        &context,
                    );
                    (receipt, result)
                })
                .collect();
            let search_policy = claude_content::retained_search_policy(&runtime.record()?, &facts)?;
            effects.apply_group(decoded, facts.representation.is_mcp(), search_policy)?;
            if matches!(effects.continuation, PostContinuation::Held { .. }) {
                break;
            }
        }
        let result = effects.model_result(model_view)?;
        for message in &effects.messages {
            events.emit_advisory(Event::ToolPresentation {
                call_id: call.id.clone(),
                text: format!("[Plugin-origin {}] {}", message.package, message.text),
            })?;
        }
        for diagnostic in &effects.diagnostics {
            events.emit_advisory(Event::ToolPresentation {
                call_id: call.id.clone(),
                text: diagnostic.clone(),
            })?;
        }
        runtime.settle_post_tool(operation, event, effects)?;
        executor.retain_post_validation(operation, validation)?;
        Ok(result)
    }
}
