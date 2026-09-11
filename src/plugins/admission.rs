//! Final-candidate admission. Only the host executor can release a guarded effect.
mod outcomes;
use super::{
    dispatch::{HookInvocation, PreToolPlan},
    gate_snapshot::{GateReadSet, GateSnapshot, GateWorkspace},
    receipts::*,
};
use crate::{events::EventSink, tools::ToolCall, workflow::runtime::SharedRuntime};
use anyhow::{Result, bail, ensure};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

pub(crate) fn digest(value: &impl Serialize) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(value)?)))
}
pub(crate) fn candidate_digest(call: &ToolCall) -> Result<String> {
    super::wire::measure(&call.arguments)?;
    // serde_json's default map is ordered; recursively canonicalize explicitly so
    // enabling preserve_order later cannot change admission identity.
    fn canonical(v: &serde_json::Value) -> serde_json::Value {
        match v {
            serde_json::Value::Object(o) => {
                let sorted = o.iter().collect::<BTreeMap<_, _>>();
                serde_json::Value::Object(
                    sorted
                        .into_iter()
                        .map(|(k, v)| (k.clone(), canonical(v)))
                        .collect(),
                )
            }
            serde_json::Value::Array(a) => {
                serde_json::Value::Array(a.iter().map(canonical).collect())
            }
            _ => v.clone(),
        }
    }
    digest(&(&call.name, canonical(&call.arguments)))
}
struct Cancellation(Arc<AtomicBool>);
impl Drop for Cancellation {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

pub(crate) struct AdmittedCandidate {
    pub(crate) target: Option<crate::tools::PluginTarget>,
    workspace: Arc<GateWorkspace>,
    snapshots: Vec<Arc<GateSnapshot>>,
    runtime: SharedRuntime,
    operation: u64,
    captures: Arc<tokio::sync::Semaphore>,
}
impl AdmittedCandidate {
    pub(crate) async fn validate(
        &self,
        boundary: Option<Arc<tokio::sync::OwnedMutexGuard<()>>>,
    ) -> Result<()> {
        let permit = self.captures.clone().acquire_owned().await?;
        let cancellation = Cancellation(Arc::new(AtomicBool::new(false)));
        let flag = cancellation.0.clone();
        let snapshots = self.snapshots.clone();
        let workspace = self.workspace.clone();
        let current = tokio::task::spawn_blocking(move || {
            let _boundary = boundary;
            let _permit = permit;
            snapshots
                .iter()
                .all(|snapshot| matches!(workspace.is_current(snapshot, &flag), Ok(true)))
        })
        .await?;
        let target_current = self
            .target
            .as_ref()
            .is_none_or(|target| target.verify().is_ok());
        if !current || !target_current {
            self.runtime.hold_plugin(
                self.operation,
                "gate inputs changed or became unavailable before effect",
            )?;
            bail!("gate inputs changed or became unavailable before effect");
        }
        Ok(())
    }
}
struct Admission<'a> {
    plan: &'a PreToolPlan,
    executor: &'a crate::tools::ToolExecutor,
    events: &'a EventSink,
    runtime: SharedRuntime,
    operation: u64,
    source: u64,
    session: String,
    role: String,
    workspace: Arc<GateWorkspace>,
    expected_workspace: (u64, u64),
    retained: BTreeMap<String, Arc<GateSnapshot>>,
    bytes: usize,
    combined: BTreeMap<usize, AdmissionKey>,
    ran_combined: BTreeSet<usize>,
    holds: Vec<String>,
}
impl PreToolPlan {
    pub(crate) async fn admit(
        &self,
        call: &mut ToolCall,
        events: &EventSink,
        workspace: Arc<GateWorkspace>,
        expected_workspace: (u64, u64),
        executor: &crate::tools::ToolExecutor,
    ) -> Result<AdmittedCandidate> {
        let (runtime, operation) = events.plugin_context()?;
        let (source, role) = runtime.plugin_owner(operation)?;
        let session = runtime.plugin_session()?;
        runtime.begin_plugin_plan(
            operation,
            self.digest.clone(),
            self.handlers
                .iter()
                .map(|h| serde_json::to_value(&h.registration.declaration))
                .collect::<Result<Vec<_>, _>>()?,
        )?;
        let mut admission = Admission {
            plan: self,
            executor,
            events,
            runtime: runtime.clone(),
            operation,
            source,
            session,
            role,
            workspace,
            expected_workspace,
            retained: BTreeMap::new(),
            bytes: 0,
            combined: BTreeMap::new(),
            ran_combined: BTreeSet::new(),
            holds: Vec::new(),
        };
        let result = admission.run(call).await;
        if let Err(error) = &result {
            runtime.hold_plugin(operation, &format!("{error:#}"))?;
        }
        result
    }
}
impl Admission<'_> {
    async fn capture(&mut self, reads: &GateReadSet) -> Result<Arc<GateSnapshot>> {
        let permit = self.plan.captures.clone().acquire_owned().await?;
        let cancellation = Cancellation(Arc::new(AtomicBool::new(false)));
        let flag = cancellation.0.clone();
        let workspace = self.workspace.clone();
        let reads = reads.clone();
        let snapshot = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            workspace.capture(&reads, &flag)
        })
        .await??;
        ensure!(
            snapshot.root_identity() == self.expected_workspace,
            "gate snapshot belongs to a different pinned executor workspace"
        );
        let revision = snapshot.revision().to_owned();
        if let Some(existing) = self.retained.get(&revision) {
            return Ok(existing.clone());
        }
        let mut size = 0usize;
        for (name, entry) in snapshot.entries() {
            size = size
                .saturating_add(name.len())
                .saturating_add(entry.bytes().len())
                .saturating_add(serde_json::to_vec(entry.access())?.len())
                .saturating_add(128);
        }
        size = size
            .saturating_add(serde_json::to_vec(snapshot.memberships())?.len())
            .saturating_add(serde_json::to_vec(snapshot.glob_matches())?.len())
            .saturating_add(serde_json::to_vec(snapshot.absent_paths())?.len());
        ensure!(
            self.bytes.saturating_add(size) <= 16 * 1024 * 1024,
            "aggregate gate snapshot evidence exceeds 16 MiB"
        );
        self.bytes += size;
        let snapshot = Arc::new(snapshot);
        self.retained.insert(revision, snapshot.clone());
        Ok(snapshot)
    }
    fn key(
        &self,
        call: &ToolCall,
        snapshots: &[(String, Arc<GateSnapshot>)],
    ) -> Result<AdmissionKey> {
        let first = snapshots
            .first()
            .ok_or_else(|| anyhow::anyhow!("gate input missing"))?;
        let mut inputs = snapshots
            .iter()
            .map(|(reads, snapshot)| (reads.clone(), snapshot.revision().to_owned()))
            .collect::<Vec<_>>();
        inputs.sort();
        inputs.dedup();
        Ok(AdmissionKey {
            session: self.session.clone(),
            operation: self.operation,
            source_operation: self.source,
            event: "PreToolUse".into(),
            tool: call.name.clone(),
            arguments: candidate_digest(call)?,
            plan: self.plan.digest.clone(),
            role: self.role.clone(),
            workspace: first.1.root_identity(),
            inputs,
            external: None,
        })
    }
    fn groups(&self, indices: Vec<usize>) -> Vec<Vec<usize>> {
        let mut groups: Vec<Vec<usize>> = Vec::new();
        let mut source_groups = BTreeMap::new();
        for index in indices {
            let d = &self.plan.handlers[index].registration.declaration;
            if let Some(group) = &d.concurrent_group {
                let key = (
                    d.identity.scope.clone(),
                    d.identity.package.clone(),
                    group.clone(),
                );
                let position = *source_groups.entry(key).or_insert_with(|| {
                    groups.push(Vec::new());
                    groups.len() - 1
                });
                groups[position].push(index);
            } else {
                groups.push(vec![index]);
            }
        }
        groups
    }
    async fn inputs(&mut self, indices: &[usize]) -> Result<Vec<(String, Arc<GateSnapshot>)>> {
        let mut inputs = BTreeMap::new();
        for index in indices {
            let reads = self.plan.handlers[*index]
                .registration
                .declaration
                .reads
                .clone();
            let identity = digest(&reads)?;
            if let std::collections::btree_map::Entry::Vacant(entry) = inputs.entry(identity) {
                entry.insert(self.capture(&reads).await?);
            }
        }
        // Even a plan with no applicable handlers binds its pinned workspace.
        if inputs.is_empty() {
            let reads = GateReadSet::new(Vec::new(), Vec::new(), Vec::new())?;
            inputs.insert(digest(&reads)?, self.capture(&reads).await?);
        }
        Ok(inputs.into_iter().collect())
    }
    async fn group(
        &mut self,
        indices: &[usize],
        call: &ToolCall,
        key: &AdmissionKey,
        inputs: &[(String, Arc<GateSnapshot>)],
        revalidate: bool,
    ) -> Result<Option<serde_json::Value>> {
        // Reserve transient work capacity for dependency bootstrap and dispatch
        // together, so members cannot deadlock waiting for one another's slot.
        // This is not a durable hook invocation or a new task allowance.
        let runner_lease = Arc::new(
            tokio::time::timeout(
                self.runtime.remaining()?,
                self.plan
                    .runners
                    .clone()
                    .acquire_many_owned(indices.len() as u32),
            )
            .await??,
        );
        self.runtime.plugin_owner(self.operation)?;
        let mutating = !revalidate
            && indices.iter().any(|index| {
                self.plan.handlers[*index]
                    .registration
                    .runner
                    .mutates_workspace()
            });
        let mutation_guard = if mutating {
            let boundary = self
                .events
                .mutation_boundary(self.expected_workspace)?
                .ok_or_else(|| anyhow::anyhow!("hook mutation boundary unavailable"))?;
            let guard =
                tokio::time::timeout(self.runtime.remaining()?, boundary.lock_owned()).await?;
            self.runtime.plugin_owner(self.operation)?;
            Some(Arc::new(guard))
        } else {
            None
        };
        let mut jobs = Vec::new();
        for index in indices {
            let handler = &self.plan.handlers[*index].registration;
            let d = &handler.declaration;
            ensure!(
                d.identity.role == self.role,
                "plugin role does not match host operation"
            );
            ensure!(
                d.external_precondition.is_none(),
                "required external atomic precondition provider is unavailable"
            );
            let read_id = digest(&d.reads)?;
            let snapshot = inputs
                .iter()
                .find(|(id, _)| *id == read_id)
                .expect("captured read set")
                .1
                .clone();
            let receipt = HookReceipt {
                invocation: 0,
                declaration: d.identity.clone(),
                class: if revalidate {
                    HandlerClass::DecisionGate
                } else {
                    d.class
                },
                endpoint: if revalidate {
                    d.read_only_endpoint.clone()
                } else {
                    None
                },
                inspected: key.clone(),
                outcome: None,
                uncertain_effects: true,
                hold: None,
                questions: Vec::new(),
                pending_proposals: Vec::new(),
            };
            let invocation = HookInvocation {
                invocation: receipt.invocation,
                key: key.clone(),
                declaration: d.identity.clone(),
                endpoint: receipt.endpoint.clone(),
                candidate: call.clone(),
                snapshot,
                completed: None,
                events: self.events.clone(),
                host: self.executor.hook_host(),
                class: receipt.class,
                runner_lease: runner_lease.clone(),
                mutation_guard: if !revalidate && handler.runner.mutates_workspace() {
                    mutation_guard.clone()
                } else {
                    None
                },
            };
            let runner = if revalidate {
                handler
                    .revalidation
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("needs-revalidation: no read-only endpoint"))?
            } else {
                handler.runner.clone()
            };
            jobs.push((*index, receipt, invocation, runner));
        }
        // Service initialization is a separately recorded dependency operation.
        // Finish every dependency before reserving durable hook invocations or
        // dispatching this group's hooks. Bootstrap shares the transient lease above.
        for (_, _, invocation, runner) in &jobs {
            self.runtime.plugin_owner(self.operation)?;
            tokio::time::timeout(
                self.runtime
                    .remaining()?
                    .min(std::time::Duration::from_secs(30)),
                runner.prepare(invocation),
            )
            .await??;
        }
        for (_, receipt, invocation, _) in &mut jobs {
            receipt.invocation =
                self.runtime
                    .begin_plugin_hook(self.operation, receipt.clone(), call)?;
            invocation.invocation = receipt.invocation;
        }
        let outcomes = futures_util::future::join_all(jobs.into_iter().map(
            |(index, mut receipt, invocation, runner)| async move {
                let outcome = super::dispatch::run_owned(&invocation, runner.as_ref()).await;
                receipt.outcome = Some(outcome);
                (index, receipt)
            },
        ))
        .await;
        let mut rewrite = None;
        for (index, mut receipt) in outcomes {
            outcomes::decode(&self.plan.profile, &mut receipt, &mut rewrite);
            if let Some(hold) = &receipt.hold {
                self.holds.push(hold.clone());
            }
            if receipt.class == HandlerClass::Combined {
                self.ran_combined.insert(index);
                self.combined.insert(index, key.clone());
            }
            self.runtime.finish_plugin_hook(self.operation, receipt)?;
        }
        Ok(rewrite)
    }
    async fn run(&mut self, call: &mut ToolCall) -> Result<AdmittedCandidate> {
        self.executor.plugin_candidate(call)?;
        let mut seen = BTreeSet::from([candidate_digest(call)?]);
        let mut revisions = 0;
        loop {
            self.executor.plugin_candidate(call)?;
            let indices = self
                .plan
                .handlers
                .iter()
                .enumerate()
                .filter(|(i, h)| {
                    h.matches(call)
                        && matches!(
                            h.registration.declaration.class,
                            HandlerClass::Transformer | HandlerClass::Combined
                        )
                        && !self.ran_combined.contains(i)
                })
                .map(|(i, _)| i)
                .collect();
            let mut changed = false;
            for group in self.groups(indices) {
                self.executor.plugin_candidate(call)?;
                let applicable = group
                    .into_iter()
                    .filter(|i| self.plan.handlers[*i].matches(call))
                    .collect::<Vec<_>>();
                if applicable.is_empty() {
                    continue;
                }
                let inputs = self.inputs(&applicable).await?;
                let key = self.key(call, &inputs)?;
                if let Some(arguments) = self.group(&applicable, call, &key, &inputs, false).await?
                {
                    let mut next = ToolCall {
                        arguments,
                        ..call.clone()
                    };
                    self.executor.plugin_candidate(&mut next)?;
                    let hash = candidate_digest(&next)?;
                    if hash != candidate_digest(call)? {
                        revisions += 1;
                        ensure!(
                            revisions < 4 && seen.insert(hash),
                            "candidate rewrite cycle or four-revision limit; held"
                        );
                        *call = next;
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
        let target = self.executor.plugin_candidate(call)?;
        let applicable = self
            .plan
            .handlers
            .iter()
            .enumerate()
            .filter(|(_, h)| h.matches(call))
            .map(|(i, _)| i)
            .collect::<Vec<_>>();
        let inputs = self.inputs(&applicable).await?;
        let key = self.key(call, &inputs)?;
        let decisions = applicable
            .iter()
            .copied()
            .filter(|i| {
                self.plan.handlers[*i].registration.declaration.class == HandlerClass::DecisionGate
            })
            .collect();
        for group in self.groups(decisions) {
            self.group(&group, call, &key, &inputs, false).await?;
        }
        let mut revalidate = Vec::new();
        for index in applicable {
            let handler = &self.plan.handlers[index].registration;
            if handler.declaration.class != HandlerClass::Combined {
                continue;
            }
            if self.combined.get(&index) == Some(&key) {
                continue;
            }
            if handler.revalidation.is_some() {
                revalidate.push(index);
            } else {
                self.holds.push(
                    "needs-revalidation: combined handler pass is not for the frozen key".into(),
                );
            }
        }
        for group in self.groups(revalidate) {
            self.group(&group, call, &key, &inputs, true).await?;
        }
        let hold = self.holds.first().cloned();
        self.runtime
            .freeze_plugin(self.operation, key, call, hold.clone())?;
        if let Some(hold) = hold {
            bail!(hold);
        }
        let admitted = AdmittedCandidate {
            target,
            workspace: self.workspace.clone(),
            snapshots: inputs.into_iter().map(|(_, s)| s).collect(),
            runtime: self.runtime.clone(),
            operation: self.operation,
            captures: self.plan.captures.clone(),
        };
        admitted.validate(None).await?;
        Ok(admitted)
    }
}

#[cfg(test)]
mod tests;
