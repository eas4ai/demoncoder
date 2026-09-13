//! Durable exact-root lineage for one explicit developer workspace replacement.
use std::{
    fs::File,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use super::{HostInvocation, Identity, Message, Operation, Record, SharedRuntime};
use crate::tools::{ToolCall, ToolResult};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RootOccurrence {
    pub path: PathBuf,
    pub device: u64,
    pub inode: u64,
    pub generation: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Prepared,
    Teardown,
    Applied,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HandoffDelivery {
    Pending,
    Sent,
    Delivered,
    Uncertain,
    Superseded,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceHandoff {
    pub old_root: PathBuf,
    pub messages: Vec<Message>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_evidence: Vec<WorkspaceToolEvidence>,
    pub delivery: HandoffDelivery,
    /// The exact first provider invocation carrying this handoff. A missing
    /// value proves that no provider owned it before denial or supersession.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_operation: Option<u64>,
    /// A later applied workspace change replaced this still-pending handoff.
    /// The retained receipt remains factual but can no longer reach a provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub superseded_by: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceToolEvidence {
    pub operation: u64,
    pub root: RootOccurrence,
    pub identity: Option<Identity>,
    pub provenance: String,
    pub call: ToolCall,
    pub result: ToolResult,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceAllowancePin {
    pub limits: crate::workflow::allocation::Limits,
    pub started_ms: u64,
    pub deadline_ms: u64,
    pub model_calls: u64,
    pub tool_calls: u64,
    pub backend_invocations: u64,
}

impl WorkspaceAllowancePin {
    fn capture(record: &Record) -> Option<Self> {
        record
            .session_hook_allowance
            .as_ref()
            .map(|allowance| Self {
                limits: allowance.allocation.limits.clone(),
                started_ms: allowance.allocation.started_ms,
                deadline_ms: allowance.allocation.deadline_ms,
                model_calls: allowance.allocation.model_calls,
                tool_calls: allowance.allocation.tool_calls,
                backend_invocations: allowance.backend_invocations,
            })
    }

    fn validate(&self, record: &Record) -> Result<()> {
        let current = record
            .session_hook_allowance
            .as_ref()
            .context("original session-hook allowance missing")?;
        ensure!(
            current.allocation.limits == self.limits
                && current.allocation.started_ms == self.started_ms
                && current.allocation.deadline_ms == self.deadline_ms
                && current.allocation.model_calls >= self.model_calls
                && current.allocation.tool_calls >= self.tool_calls
                && current.backend_invocations >= self.backend_invocations,
            "original session-hook allowance was replaced, reset, or changed"
        );
        ensure!(
            current.allocation.remaining_ms()? > 0,
            "original session-hook allowance deadline expired before workspace release"
        );
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceChange {
    pub version: u32,
    pub from: RootOccurrence,
    pub to: RootOccurrence,
    pub predecessor: u64,
    pub native_session: u64,
    pub allowance: Option<WorkspaceAllowancePin>,
    pub identity: Identity,
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd_plan: Option<String>,
    pub handoff: Option<WorkspaceHandoff>,
    pub pins: String,
    pub stage: Stage,
    pub hold: Option<String>,
}

/// An open directory descriptor binds preparation to one exact filesystem object.
#[derive(Debug)]
pub struct WorkspaceCandidate {
    occurrence: RootOccurrence,
    descriptor: File,
}

impl RootOccurrence {
    fn capture(path: &Path, generation: u64) -> Result<Self> {
        let path = path.canonicalize().context("resolve selected workspace")?;
        let metadata = std::fs::metadata(&path).context("inspect selected workspace")?;
        ensure!(metadata.is_dir(), "selected workspace is not a directory");
        Ok(Self {
            path,
            device: metadata.dev(),
            inode: metadata.ino(),
            generation,
        })
    }
}

impl WorkspaceCandidate {
    pub(crate) fn capture(path: &Path, generation: u64) -> Result<Self> {
        let occurrence = RootOccurrence::capture(path, generation)?;
        let descriptor = File::open(&occurrence.path).context("open selected workspace")?;
        let metadata = descriptor
            .metadata()
            .context("inspect selected workspace descriptor")?;
        ensure!(
            (metadata.dev(), metadata.ino()) == (occurrence.device, occurrence.inode),
            "selected workspace changed while it was opened"
        );
        Ok(Self {
            occurrence,
            descriptor,
        })
    }

    pub(crate) fn occurrence(&self) -> &RootOccurrence {
        &self.occurrence
    }

    pub(crate) fn host_descriptor(&self) -> Result<std::sync::Arc<File>> {
        Ok(std::sync::Arc::new(
            self.descriptor
                .try_clone()
                .context("clone selected workspace descriptor")?,
        ))
    }

    fn revalidate(&self) -> Result<()> {
        let descriptor = self
            .descriptor
            .metadata()
            .context("reinspect selected workspace descriptor")?;
        let path = std::fs::metadata(&self.occurrence.path)
            .context("reinspect selected workspace path")?;
        ensure!(
            path.is_dir()
                && (descriptor.dev(), descriptor.ino())
                    == (self.occurrence.device, self.occurrence.inode)
                && (path.dev(), path.ino()) == (self.occurrence.device, self.occurrence.inode),
            "selected workspace was replaced after preparation"
        );
        Ok(())
    }
}

fn operation(record: &Record, id: u64) -> Result<(&Operation, &WorkspaceChange)> {
    let operation = record
        .operations
        .iter()
        .find(|operation| operation.id == id)
        .context("workspace change owner missing")?;
    let Some(HostInvocation::WorkspaceChange(change)) = &operation.host_invocation else {
        anyhow::bail!("operation is not a workspace change")
    };
    ensure!(
        change.version == 1
            && operation.phase == "workspace-change"
            && operation.identity.as_ref() == Some(&change.identity)
            && !operation.complete
            && !operation.reconciled
            && change.hold.is_none(),
        "workspace change owner changed, ended, or is held"
    );
    ensure!(
        change.pins == pins(record, change)?,
        "workspace change lineage or policy changed"
    );
    let chain = applied_chain_before(record, change.native_session, id)?;
    ensure!(
        change.from == chain.root
            && change.to.generation == chain.root.generation + 1
            && change.predecessor == chain.last_applied,
        "workspace change is disconnected from the lifetime's applied-root chain"
    );
    match change.stage {
        Stage::Prepared | Stage::Teardown => ensure!(
            record.workspace == change.from.path,
            "workspace change lost its old root"
        ),
        Stage::Applied => ensure!(
            record.workspace == change.to.path,
            "workspace change does not own installed root"
        ),
    }
    Ok((operation, change))
}

fn pins(record: &Record, change: &WorkspaceChange) -> Result<String> {
    let (lifetime_operation, lifetime) =
        super::plugin_session::lifetime(record, change.native_session)?;
    let handoff = change
        .handoff
        .as_ref()
        .map(|handoff| (&handoff.old_root, &handoff.messages, &handoff.tool_evidence));
    crate::plugins::admission::digest(&(
        change.version,
        &change.from,
        &change.to,
        change.predecessor,
        change.native_session,
        &change.allowance,
        &(
            &lifetime_operation.budget,
            &lifetime_operation.identity,
            lifetime.workspace,
            &lifetime.workspace_path,
            lifetime.source,
            &lifetime.plans,
        ),
        &change.identity,
        &change.source,
        &change.cwd_plan,
        handoff,
        &record.plugin_activations,
    ))
}

fn original_root(record: &Record, lifetime_id: u64) -> Result<RootOccurrence> {
    let (operation, lifetime) = super::plugin_session::lifetime(record, lifetime_id)?;
    ensure!(
        operation.id == lifetime_id && operation.phase == "native-session" && lifetime.version == 1,
        "workspace root origin is not an exact native lifetime"
    );
    Ok(RootOccurrence {
        path: lifetime
            .workspace_path
            .clone()
            .context("native lifetime original workspace path missing")?,
        device: lifetime.workspace.0,
        inode: lifetime.workspace.1,
        generation: 0,
    })
}

struct AppliedRootChain {
    root: RootOccurrence,
    last_applied: u64,
}

fn applied_chain_before(
    record: &Record,
    lifetime_id: u64,
    before: u64,
) -> Result<AppliedRootChain> {
    let mut current = original_root(record, lifetime_id)?;
    let mut predecessor = 0;
    for operation in record
        .operations
        .iter()
        .filter(|operation| operation.id > lifetime_id && operation.id < before)
    {
        let Some(HostInvocation::WorkspaceChange(change)) = &operation.host_invocation else {
            continue;
        };
        if change.stage != Stage::Applied {
            continue;
        }
        ensure!(
            change.version == 1
                && operation.phase == "workspace-change"
                && operation.identity.as_ref() == Some(&change.identity)
                && change.native_session == lifetime_id
                && change.from == current
                && change.to.generation == current.generation + 1
                && change.predecessor == predecessor
                && change.pins == pins(record, change)?,
            "workspace root transition is disconnected from its exact original lifetime"
        );
        current = change.to.clone();
        predecessor = operation.id;
    }
    Ok(AppliedRootChain {
        root: current,
        last_applied: predecessor,
    })
}

fn recorded_root_before(record: &Record, lifetime_id: u64, before: u64) -> Result<RootOccurrence> {
    Ok(applied_chain_before(record, lifetime_id, before)?.root)
}

fn lifetime_before(record: &Record, before: u64) -> Option<u64> {
    record.operations.iter().rev().find_map(|operation| {
        (operation.id < before
            && matches!(
                operation.host_invocation,
                Some(HostInvocation::NativeSession(_))
            ))
        .then_some(operation.id)
    })
}

fn current_root_before(record: &Record, before: u64) -> Result<RootOccurrence> {
    let Some(lifetime_id) = lifetime_before(record, before) else {
        let path = record
            .workspace
            .canonicalize()
            .context("resolve workspace without native lifetime")?;
        let metadata =
            std::fs::metadata(&path).context("inspect workspace without native lifetime")?;
        return Ok(RootOccurrence {
            path,
            device: metadata.dev(),
            inode: metadata.ino(),
            generation: 0,
        });
    };
    recorded_root_before(record, lifetime_id, before)
}

pub(super) fn validate_physical_root(root: &RootOccurrence) -> Result<()> {
    let metadata =
        std::fs::metadata(&root.path).context("inspect admitted workspace occurrence")?;
    ensure!(
        metadata.is_dir() && (metadata.dev(), metadata.ino()) == (root.device, root.inode),
        "admitted workspace occurrence changed without an applied transition"
    );
    Ok(())
}

fn validate_selected_old_root(
    current: &RootOccurrence,
    candidate: &WorkspaceCandidate,
) -> Result<()> {
    let metadata = std::fs::metadata(&current.path).context("reinspect selected old workspace")?;
    if metadata.is_dir() && (metadata.dev(), metadata.ino()) == (current.device, current.inode) {
        return Ok(());
    }
    ensure!(
        candidate.occurrence.path == current.path
            && (metadata.dev(), metadata.ino())
                == (candidate.occurrence.device, candidate.occurrence.inode)
            && (candidate.occurrence.device, candidate.occurrence.inode)
                != (current.device, current.inode),
        "old workspace occurrence changed outside the explicitly selected same-path replacement"
    );
    Ok(())
}

pub(crate) fn current_root(record: &Record) -> Result<RootOccurrence> {
    let current = current_root_before(record, u64::MAX)?;
    ensure!(
        record.workspace == current.path,
        "record workspace changed without an applied transition"
    );
    validate_physical_root(&current)?;
    Ok(current)
}

fn current_handoff_boundary(record: &Record) -> Result<Option<(u64, RootOccurrence)>> {
    let Some(native_session) = lifetime_before(record, u64::MAX) else {
        return Ok(None);
    };
    Ok(Some((native_session, current_root(record)?)))
}

pub(super) fn validate_lifetime_lineage(
    record: &Record,
    lifetime_id: u64,
) -> Result<RootOccurrence> {
    let current = recorded_root_before(record, lifetime_id, u64::MAX)?;
    ensure!(
        record.workspace == current.path,
        "record workspace changed without an applied transition"
    );
    Ok(current)
}

pub(super) fn interrupt_restored(record: &mut Record) {
    for operation in &mut record.operations {
        let Some(HostInvocation::WorkspaceChange(change)) = &mut operation.host_invocation else {
            continue;
        };
        if let Some(handoff) = &mut change.handoff
            && handoff.delivery == HandoffDelivery::Sent
        {
            handoff.delivery = HandoffDelivery::Uncertain;
            change.hold = Some(
                "workspace conversation delivery was interrupted; inspect before continuing".into(),
            );
        }
        if change.hold.is_some() {
            record.recovery_pending = true;
        }
    }
}

pub(super) fn owner(record: &Record, id: u64) -> Result<super::plugin_non_tool::owner::Owner<'_>> {
    let (_, change) = operation(record, id)?;
    ensure!(
        change.stage == Stage::Applied,
        "CwdChanged precedes workspace application"
    );
    Ok(super::plugin_non_tool::owner::Owner {
        identity: &record.identity,
        root: &change.to.path,
        child: None,
    })
}

pub(super) fn validate_occurrence(
    record: &Record,
    id: u64,
    occurrence: &crate::plugins::receipts::NonToolOccurrence,
    plan: &str,
) -> Result<()> {
    let (_, change) = operation(record, id)?;
    let crate::plugins::receipts::NonToolOccurrence::CwdChanged {
        workspace_change,
        old_cwd,
        new_cwd,
    } = occurrence
    else {
        anyhow::bail!("occurrence is not CwdChanged")
    };
    ensure!(
        *workspace_change == id
            && change.stage == Stage::Applied
            && old_cwd == &change.from.path.to_string_lossy()
            && new_cwd == &change.to.path.to_string_lossy()
            && change.cwd_plan.as_deref() == Some(plan),
        "CwdChanged differs from applied workspace replacement"
    );
    Ok(())
}

pub(super) fn hook_lifetime(record: &Record, id: u64) -> Result<u64> {
    let (change_operation, change) = operation(record, id)?;
    let (lifetime_operation, lifetime) =
        super::plugin_session::validate_host_lifetime(record, change.native_session)?;
    ensure!(
        lifetime.end.is_none() && lifetime_operation.budget == change_operation.budget,
        "workspace observation lost original host lifetime funding"
    );
    Ok(change.native_session)
}

impl SharedRuntime {
    pub(crate) fn prepare_workspace_candidate(
        &self,
        selected: &Path,
        native_session: u64,
    ) -> Result<(RootOccurrence, WorkspaceCandidate)> {
        let record = self.record()?;
        super::plugin_session::validate_host_authority(&record, native_session)?;
        let current = recorded_root_before(&record, native_session, u64::MAX)?;
        ensure!(
            record.workspace == current.path,
            "record workspace changed without an applied transition"
        );
        let candidate = WorkspaceCandidate::capture(selected, current.generation + 1)?;
        validate_selected_old_root(&current, &candidate)?;
        Ok((current, candidate))
    }

    pub(crate) fn validate_workspace_change_request(
        &self,
        candidate: &WorkspaceCandidate,
        source: &str,
        native_session: u64,
    ) -> Result<()> {
        let session = self.plugin_session()?;
        let record = self.record()?;
        super::plugin_session::validate_host_authority(&record, native_session)?;
        let from = recorded_root_before(&record, native_session, u64::MAX)?;
        ensure!(
            record.workspace == from.path,
            "workspace release lost its recorded root"
        );
        candidate.revalidate()?;
        validate_selected_old_root(&from, candidate)?;
        ensure!(
            candidate.occurrence.generation == from.generation + 1 && candidate.occurrence != from,
            "selected workspace is unchanged or has a stale generation"
        );
        ensure!(
            !record.recovery_pending && record.phase.is_none(),
            "workspace replacement requires an idle reconciled session"
        );
        super::ensure_children_settled(&record)?;
        ensure!(
            record
                .task
                .as_ref()
                .is_none_or(|task| task.accepted.is_some()),
            "accept or cancel current work before replacing the workspace"
        );
        ensure!(
            source == "developer",
            "invalid workspace replacement source"
        );
        let (operation, lifetime) =
            super::plugin_session::validate_host_authority(&record, native_session)?;
        ensure!(
            lifetime.end.is_none(),
            "original native session already ended"
        );
        super::budget_accounting::active(
            &record,
            &session,
            operation
                .budget
                .as_ref()
                .context("native session funding missing")?,
        )?;
        ensure!(
            record.operations.iter().all(|operation| {
                operation.complete
                    || operation.non_tool_receipt().is_some_and(|receipt| {
                        receipt.facts.subject.occurrence.event()
                            == crate::plugins::hook_types::HookEvent::ConfigChange
                    })
            }),
            "finish active or unresolved operations before replacing the workspace"
        );
        Ok(())
    }

    pub(crate) fn workspace_handoff(&self) -> Result<WorkspaceHandoff> {
        let record = self.record()?;
        let old_root = record.workspace.clone();
        let messages = record
            .messages
            .iter()
            .cloned()
            .map(|mut message| {
                if message.provenance.is_none() {
                    message.provenance = Some("legacy_session_record".into());
                }
                if message.root.is_none() {
                    message.root = Some(old_root.clone());
                }
                message
            })
            .collect::<Vec<_>>();
        let tool_evidence = record
            .operations
            .iter()
            .filter_map(|operation| {
                if operation.phase != "worker"
                    || !operation.complete
                    || operation.reconciled
                    || operation.host_invocation.is_some()
                {
                    return None;
                }
                let (Some(call), Some(result)) = (&operation.call, &operation.result) else {
                    return None;
                };
                let root = match current_root_before(&record, operation.id) {
                    Ok(root) => root,
                    Err(error) => return Some(Err(error)),
                };
                let identity = operation.identity.clone().or_else(|| {
                    operation.tool_receipt.as_ref().and_then(|receipt| {
                        record
                            .operations
                            .iter()
                            .find(|owner| owner.id == receipt.invocation)
                            .and_then(|owner| owner.identity.clone())
                    })
                });
                Some(Ok(WorkspaceToolEvidence {
                    operation: operation.id,
                    root,
                    identity,
                    provenance: "retained_completed_tool_operation".into(),
                    call: call.clone(),
                    result: result.clone(),
                }))
            })
            .collect::<Result<Vec<_>>>()?;
        let encoded = serde_json::to_vec(&(&messages, &tool_evidence))?;
        ensure!(
            encoded.len() <= 2 * 1024 * 1024 && messages.len() < 4096 && tool_evidence.len() < 4096,
            "retained conversation and tool evidence exceed the 2 MiB workspace handoff bound"
        );
        Ok(WorkspaceHandoff {
            old_root,
            messages,
            tool_evidence,
            delivery: HandoffDelivery::Pending,
            provider_operation: None,
            superseded_by: None,
        })
    }

    pub(crate) fn begin_workspace_handoff_delivery(&self) -> Result<Option<(u64, String)>> {
        self.update(|record| {
            let Some((native_session, current)) = current_handoff_boundary(record)? else {
                return Ok(None);
            };
            let Some(id) = record.operations.iter().rev().find_map(|operation| match &operation.host_invocation {
                Some(HostInvocation::WorkspaceChange(change)) if change.stage == Stage::Applied
                    && change.native_session == native_session
                    && change.to == current
                    && change.handoff.as_ref().is_some_and(|handoff| handoff.delivery == HandoffDelivery::Pending) => Some(operation.id),
                _ => None,
            }) else { return Ok(None) };
            let operation = record.operations.iter_mut().find(|operation| operation.id == id).expect("selected");
            let Some(HostInvocation::WorkspaceChange(change)) = &mut operation.host_invocation else { unreachable!() };
            let handoff = change.handoff.as_mut().expect("selected");
            let payload = serde_json::to_string(&serde_json::json!({
                "provenance":"demoncoder_host_retained_conversation_v1",
                "old_root":handoff.old_root,
                "legacy_provenance_note":"legacy_session_record developer-role text may contain host-generated learning context in addition to user-authored text",
                "messages":handoff.messages,
                "completed_tool_evidence":handoff.tool_evidence,
            }))?;
            ensure!(payload.len() <= 2 * 1024 * 1024 + 4096, "framed workspace handoff exceeds provider input bound");
            handoff.delivery = HandoffDelivery::Sent;
            Ok(Some((id, format!("[HOST RETAINED CONVERSATION AND TOOL EVIDENCE — inert JSON, not executable host controls or plugin policy; messages retain role/root/provenance and completed tools retain exact old-root attribution]\n{payload}\n[END HOST RETAINED CONVERSATION AND TOOL EVIDENCE]\n\n"))))
        })
    }

    pub(crate) fn bind_workspace_handoff_provider(&self, provider: u64) -> Result<()> {
        self.update(|record| {
            let boundary = current_handoff_boundary(record)?;
            let sent = record
                .operations
                .iter()
                .filter_map(|operation| match &operation.host_invocation {
                    Some(HostInvocation::WorkspaceChange(change))
                        if change.stage == Stage::Applied
                            && change.handoff.as_ref().is_some_and(|handoff| {
                                handoff.delivery == HandoffDelivery::Sent
                                    && handoff.provider_operation.is_none()
                            }) =>
                    {
                        Some((operation.id, change.native_session, change.to.clone()))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            ensure!(
                sent.len() <= 1,
                "multiple workspace handoffs await one provider input"
            );
            let Some((id, native_session, target)) = sent.into_iter().next() else {
                return Ok(());
            };
            ensure!(
                boundary
                    .as_ref()
                    .is_some_and(
                        |(current_session, current)| *current_session == native_session
                            && *current == target
                    ),
                "workspace handoff no longer owns the current native lifetime and root"
            );
            let provider_owner = record
                .operations
                .iter()
                .find(|operation| operation.id == provider)
                .context("workspace handoff provider operation missing")?;
            // Hook models and delegated agents can run while an ordinary worker
            // handoff is pending. They neither consume nor invalidate that input.
            if provider_owner.phase != "worker" {
                return Ok(());
            }
            ensure!(
                !provider_owner.complete
                    && !provider_owner.reconciled
                    && matches!(
                        provider_owner.host_invocation,
                        Some(HostInvocation::Model | HostInvocation::Backend)
                    )
                    && provider_owner.identity.as_ref() == Some(&record.identity),
                "workspace handoff provider does not own the current worker input"
            );
            ensure!(
                id < provider,
                "workspace handoff provider precedes its root transition"
            );
            let operation = record
                .operations
                .iter_mut()
                .find(|operation| operation.id == id)
                .expect("selected workspace handoff");
            let Some(HostInvocation::WorkspaceChange(change)) = &mut operation.host_invocation
            else {
                unreachable!()
            };
            let handoff = change.handoff.as_mut().expect("selected workspace handoff");
            handoff.provider_operation = Some(provider);
            Ok(())
        })
    }

    pub(crate) fn finish_workspace_handoff_delivery(&self, id: u64, completed: bool) -> Result<()> {
        self.update(|record| {
            let boundary = current_handoff_boundary(record)?;
            let (delivery, provider, native_session, target) = {
                let operation = record.operations.iter().find(|operation| operation.id == id).context("workspace handoff owner missing")?;
                let Some(HostInvocation::WorkspaceChange(change)) = &operation.host_invocation else { anyhow::bail!("handoff owner is not a workspace change") };
                let handoff = change.handoff.as_ref().context("workspace handoff missing")?;
                (handoff.delivery, handoff.provider_operation, change.native_session, change.to.clone())
            };
            ensure!(delivery == HandoffDelivery::Sent, "workspace handoff delivery was not reserved exactly once");
            ensure!(
                boundary
                    .as_ref()
                    .is_some_and(|(current_session, current)| *current_session == native_session
                        && *current == target),
                "workspace handoff no longer owns the current native lifetime and root"
            );
            let provider_delivered = if let Some(provider) = provider {
                let owner = record.operations.iter().find(|operation| operation.id == provider)
                    .context("workspace handoff provider operation disappeared")?;
                ensure!(provider > id
                    && owner.phase == "worker"
                    && matches!(owner.host_invocation, Some(HostInvocation::Model | HostInvocation::Backend)),
                    "workspace handoff provider owner changed");
                completed
            } else {
                false
            };
            let operation = record.operations.iter_mut().find(|operation| operation.id == id).expect("validated");
            let Some(HostInvocation::WorkspaceChange(change)) = &mut operation.host_invocation else { unreachable!() };
            let handoff = change.handoff.as_mut().expect("validated");
            if provider.is_none() {
                // The command or gate returned before any provider operation.
                // This exact handoff remains eligible for the next ordinary prompt.
                handoff.delivery = HandoffDelivery::Pending;
            } else if provider_delivered {
                handoff.delivery = HandoffDelivery::Delivered;
            } else {
                handoff.delivery = HandoffDelivery::Uncertain;
                change.hold = Some("workspace conversation delivery outcome is uncertain; inspect before continuing".into());
                record.recovery_pending = true;
            }
            Ok(())
        })
    }

    pub(crate) fn workspace_root(&self) -> Result<RootOccurrence> {
        current_root(&self.record()?)
    }

    pub(crate) fn begin_workspace_change(
        &self,
        candidate: &WorkspaceCandidate,
        source: &str,
        native_session: u64,
        cwd_plan: Option<String>,
        handoff: Option<WorkspaceHandoff>,
    ) -> Result<u64> {
        candidate.revalidate()?;
        let session = self.plugin_session()?;
        self.update(|record| {
            super::plugin_session::validate_host_authority(record, native_session)?;
            let chain = applied_chain_before(record, native_session, u64::MAX)?;
            let from = chain.root;
            ensure!(
                record.workspace == from.path,
                "workspace admission lost its recorded root"
            );
            validate_selected_old_root(&from, candidate)?;
            ensure!(
                candidate.occurrence.generation == from.generation + 1
                    && candidate.occurrence != from,
                "selected workspace is unchanged or has a stale generation"
            );
            ensure!(
                !record.recovery_pending && record.phase.is_none(),
                "workspace replacement requires an idle reconciled session"
            );
            super::ensure_children_settled(record)?;
            ensure!(
                record
                    .task
                    .as_ref()
                    .is_none_or(|task| task.accepted.is_some()),
                "accept or cancel current work before replacing the workspace"
            );
            ensure!(
                !record
                    .operations
                    .iter()
                    .any(|operation| !operation.complete || operation.needs_reconciliation()),
                "finish active or unresolved operations before replacing the workspace"
            );
            ensure!(
                source == "developer" && record.operations.len() < 4096,
                "invalid or unbounded workspace replacement source"
            );
            let (lifetime_operation, lifetime) =
                super::plugin_session::validate_host_authority(record, native_session)?;
            ensure!(
                lifetime.end.is_none(),
                "original native session already ended"
            );
            let budget = lifetime_operation
                .budget
                .clone()
                .context("native session funding missing")?;
            super::budget_accounting::active(record, &session, &budget)?;
            let allowance = WorkspaceAllowancePin::capture(record);
            ensure!(
                record.archived.len() < 32,
                "session task history is full; start a new session"
            );
            if let Some(task) = record.task.take() {
                record.archived.push(super::ArchivedTask {
                    task,
                    allocation_epoch: record
                        .allocation
                        .as_ref()
                        .map(|_| record.task_allocation_epoch),
                    allocation: record.allocation.clone(),
                });
            }
            let identity = record.identity.clone();
            let mut change = WorkspaceChange {
                version: 1,
                from,
                to: candidate.occurrence.clone(),
                predecessor: chain.last_applied,
                native_session,
                allowance,
                identity: identity.clone(),
                source: source.into(),
                cwd_plan,
                handoff,
                pins: String::new(),
                stage: Stage::Prepared,
                hold: None,
            };
            change.pins = pins(record, &change)?;
            let id = record.operations.len() as u64 + 1;
            record.operations.push(Operation {
                id,
                budget: Some(budget),
                usage_receipt: None,
                phase: "workspace-change".into(),
                verification: None,
                call: None,
                result: None,
                tool_receipt: None,
                host_invocation: Some(HostInvocation::WorkspaceChange(Box::new(change))),
                complete: false,
                reconciled: false,
                usage_reported: true,
                identity: Some(identity),
            });
            Ok(id)
        })
    }

    pub(crate) fn begin_workspace_change_teardown(&self, id: u64) -> Result<()> {
        self.update(|record| {
            let (_, change) = operation(record, id)?;
            ensure!(
                change.stage == Stage::Prepared
                    && !record.recovery_pending
                    && record.phase.is_none(),
                "workspace replacement is no longer at its release boundary"
            );
            let Some(HostInvocation::WorkspaceChange(change)) = &mut record
                .operations
                .iter_mut()
                .find(|operation| operation.id == id)
                .expect("validated")
                .host_invocation
            else {
                unreachable!()
            };
            change.stage = Stage::Teardown;
            Ok(())
        })
    }

    pub(crate) fn apply_workspace_change(
        &self,
        id: u64,
        candidate: &WorkspaceCandidate,
    ) -> Result<()> {
        let session = self.plugin_session()?;
        candidate.revalidate()?;
        self.update_without_observers(false, |record| {
            let (_, change) = operation(record, id)?;
            ensure!(
                change.stage == Stage::Teardown && &change.to == candidate.occurrence(),
                "workspace application differs from admitted target"
            );
            ensure!(
                !record.recovery_pending
                    && record.phase.is_none()
                    && record.identity == change.identity,
                "workspace replacement lost its final recovery, idle, or model boundary"
            );
            let (lifetime_operation, lifetime) =
                super::plugin_session::validate_host_authority(record, change.native_session)?;
            ensure!(
                lifetime.end.is_none()
                    && lifetime_operation.budget == record.operations[id as usize - 1].budget,
                "workspace replacement lost its original live lifetime or funding reference"
            );
            let active = super::budget_accounting::active(
                record,
                &session,
                lifetime_operation
                    .budget
                    .as_ref()
                    .context("native session funding missing")?,
            )?;
            match (&change.allowance, active) {
                (Some(pin), Some(_)) => pin.validate(record)?,
                (None, None) => {}
                _ => anyhow::bail!("workspace replacement original allowance identity changed"),
            }
            let current = change.from.clone();
            let native_session = change.native_session;
            let replacement_has_handoff = change.handoff.is_some();
            candidate.revalidate()?;
            validate_selected_old_root(&current, candidate)?;
            let superseded = record
                .operations
                .iter()
                .filter_map(|operation| match &operation.host_invocation {
                    Some(HostInvocation::WorkspaceChange(previous))
                        if operation.id < id
                            && previous.native_session == native_session
                            && previous.stage == Stage::Applied
                            && previous.handoff.as_ref().is_some_and(|handoff| {
                                handoff.delivery == HandoffDelivery::Pending
                            }) => Some(operation.id),
                    _ => None,
                })
                .collect::<Vec<_>>();
            ensure!(
                superseded.is_empty() || replacement_has_handoff,
                "workspace replacement cannot supersede retained conversation without a newer handoff"
            );
            for superseded_id in superseded {
                let previous = record
                    .operations
                    .iter_mut()
                    .find(|operation| operation.id == superseded_id)
                    .expect("selected pending workspace handoff");
                let Some(HostInvocation::WorkspaceChange(previous)) =
                    &mut previous.host_invocation
                else {
                    unreachable!()
                };
                let handoff = previous.handoff.as_mut().expect("selected pending handoff");
                ensure!(
                    handoff.provider_operation.is_none() && handoff.superseded_by.is_none(),
                    "pending workspace handoff already has a delivery owner"
                );
                handoff.delivery = HandoffDelivery::Superseded;
                handoff.superseded_by = Some(id);
            }
            record.workspace = candidate.occurrence.path.clone();
            let Some(HostInvocation::WorkspaceChange(change)) = &mut record
                .operations
                .iter_mut()
                .find(|operation| operation.id == id)
                .expect("validated")
                .host_invocation
            else {
                unreachable!()
            };
            change.stage = Stage::Applied;
            Ok(())
        })
    }

    pub(crate) fn end_workspace_change(&self, id: u64, hold: Option<String>) -> Result<()> {
        self.update(|record| {
            let (_, change) = operation(record, id)?;
            ensure!(
                change.stage == Stage::Applied,
                "workspace replacement ended before application"
            );
            record.recovery_pending |= hold.is_some();
            let operation = record
                .operations
                .iter_mut()
                .find(|operation| operation.id == id)
                .expect("validated");
            let Some(HostInvocation::WorkspaceChange(change)) = &mut operation.host_invocation
            else {
                unreachable!()
            };
            change.hold = hold;
            operation.complete = true;
            Ok(())
        })
    }

    pub(crate) fn interrupt_workspace_change(&self, id: u64) -> Result<()> {
        self.update(|record| {
            let (_, change) = operation(record, id)?;
            let uncertain = change.stage != Stage::Prepared;
            let operation = record.operations.iter_mut().find(|operation| operation.id == id).expect("validated");
            let Some(HostInvocation::WorkspaceChange(change)) = &mut operation.host_invocation else { unreachable!() };
            change.hold = Some(if uncertain { "workspace replacement interrupted after old-root teardown; inspect before continuing" } else { "workspace replacement cancelled before old-root teardown" }.into());
            operation.complete = true;
            record.recovery_pending |= uncertain;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        session::SessionStart,
        workflow::{
            allocation::{Allocation, Limits},
            runtime::SharedRuntime,
            state::Task,
            workspace,
        },
    };

    fn runtime(root: &std::path::Path) -> SharedRuntime {
        let mut record = crate::inspection::tests::record(root);
        record.allocation = Some(Allocation::new(Limits::default()).unwrap());
        let snapshot = workspace::capture(root).unwrap();
        let mut task = Task::new(1, "accepted work".into(), vec![], snapshot, 0).unwrap();
        task.accepted = Some("accepted-snapshot".into());
        record.task = Some(task);
        SharedRuntime::for_test(&root.join("record"), record).unwrap()
    }

    #[test]
    fn admitted_change_archives_accepted_task_but_preserves_allocation_and_lifetime() {
        let parent = tempfile::tempdir().unwrap();
        let old = parent.path().join("old");
        let new = parent.path().join("new path; literal");
        std::fs::create_dir(&old).unwrap();
        std::fs::create_dir(&new).unwrap();
        let runtime = runtime(&old);
        let lifetime = runtime
            .begin_native_session(SessionStart::Startup, None, vec![])
            .unwrap();
        let mut before = serde_json::to_value(runtime.record().unwrap().allocation).unwrap();
        before.as_object_mut().unwrap().remove("observed_ms");
        let candidate = WorkspaceCandidate::capture(&new, 1).unwrap();

        let operation = runtime
            .begin_workspace_change(&candidate, "developer", lifetime, None, None)
            .unwrap();

        let record = runtime.record().unwrap();
        assert!(record.task.is_none());
        assert_eq!(record.archived.len(), 1);
        let mut after = serde_json::to_value(&record.allocation).unwrap();
        after.as_object_mut().unwrap().remove("observed_ms");
        assert_eq!(after, before);
        assert_eq!(
            current_root(&record).unwrap(),
            RootOccurrence::capture(&old, 0).unwrap()
        );
        assert_eq!(operation, record.operations.len() as u64);
        assert_eq!(
            record.operations[lifetime as usize - 1].budget,
            record.operations[operation as usize - 1].budget
        );
    }

    #[test]
    fn retargeted_destination_after_teardown_is_never_published() {
        let parent = tempfile::tempdir().unwrap();
        let old = parent.path().join("old");
        let new = parent.path().join("new");
        std::fs::create_dir(&old).unwrap();
        std::fs::create_dir(&new).unwrap();
        let runtime = runtime(&old);
        let lifetime = runtime
            .begin_native_session(SessionStart::Startup, None, vec![])
            .unwrap();
        let candidate = WorkspaceCandidate::capture(&new, 1).unwrap();
        let operation = runtime
            .begin_workspace_change(&candidate, "developer", lifetime, None, None)
            .unwrap();
        runtime.begin_workspace_change_teardown(operation).unwrap();
        std::fs::remove_dir(&new).unwrap();
        std::fs::create_dir(&new).unwrap();

        assert!(
            runtime
                .apply_workspace_change(operation, &candidate)
                .is_err()
        );
        assert_eq!(runtime.record().unwrap().workspace, old);
        runtime.interrupt_workspace_change(operation).unwrap();
        assert!(runtime.record().unwrap().recovery_pending);
    }

    #[test]
    fn explicit_same_path_new_inode_is_an_applied_new_generation() {
        let parent = tempfile::tempdir().unwrap();
        let selected = parent.path().join("workspace");
        let displaced = parent.path().join("displaced-workspace");
        std::fs::create_dir(&selected).unwrap();
        let runtime = runtime(&selected);
        let lifetime = runtime
            .begin_native_session(SessionStart::Startup, None, vec![])
            .unwrap();
        let original = RootOccurrence::capture(&selected, 0).unwrap();

        std::fs::rename(&selected, &displaced).unwrap();
        std::fs::create_dir(&selected).unwrap();
        let candidate = WorkspaceCandidate::capture(&selected, 1).unwrap();
        assert_ne!(
            (candidate.occurrence.device, candidate.occurrence.inode),
            (original.device, original.inode)
        );
        runtime
            .validate_workspace_change_request(&candidate, "developer", lifetime)
            .unwrap();

        let operation = runtime
            .begin_workspace_change(&candidate, "developer", lifetime, None, None)
            .unwrap();
        runtime.begin_workspace_change_teardown(operation).unwrap();
        runtime
            .apply_workspace_change(operation, &candidate)
            .unwrap();
        runtime.end_workspace_change(operation, None).unwrap();

        assert_eq!(
            current_root(&runtime.record().unwrap()).unwrap(),
            candidate.occurrence
        );
        let record = runtime.record().unwrap();
        let (_, owner) = super::super::plugin_session::lifetime(&record, lifetime).unwrap();
        assert_eq!(owner.workspace, (original.device, original.inode));
        assert_eq!(owner.workspace_path.as_deref(), Some(selected.as_path()));
    }

    #[derive(Clone, Copy)]
    enum FinalAuthorityFault {
        Recovery,
        Revoked,
        Ended,
        ExpiredAllowance,
        RefreshedAllowance,
        ResetAllowanceCounters,
    }

    fn final_authority_fixture() -> (
        tempfile::TempDir,
        PathBuf,
        SharedRuntime,
        WorkspaceCandidate,
        u64,
        u64,
    ) {
        let parent = tempfile::tempdir().unwrap();
        let old = parent.path().join("old");
        let new = parent.path().join("new");
        std::fs::create_dir(&old).unwrap();
        std::fs::create_dir(&new).unwrap();
        let runtime = runtime(&old);
        runtime
            .update(|record| {
                record.session_hook_allowance = Some(
                    super::super::session_budget::SessionHookAllowance::new(Limits {
                        seconds: 60,
                        model_calls: 4,
                        tool_calls: 4,
                    })?,
                );
                record
                    .session_hook_allowance
                    .as_mut()
                    .unwrap()
                    .allocation
                    .model_calls = 1;
                Ok(())
            })
            .unwrap();
        let lifetime = runtime
            .begin_native_session(SessionStart::Startup, None, vec![])
            .unwrap();
        let candidate = WorkspaceCandidate::capture(&new, 1).unwrap();
        let operation = runtime
            .begin_workspace_change(&candidate, "developer", lifetime, None, None)
            .unwrap();
        runtime.begin_workspace_change_teardown(operation).unwrap();
        (parent, old, runtime, candidate, lifetime, operation)
    }

    fn assert_final_authority_fault_refuses(fault: FinalAuthorityFault) {
        let (_parent, old, runtime, candidate, lifetime, operation) = final_authority_fixture();
        let original_budget = runtime.record().unwrap().operations[operation as usize - 1]
            .budget
            .clone();
        match fault {
            FinalAuthorityFault::Recovery => runtime.hold().unwrap(),
            FinalAuthorityFault::Revoked => runtime.finalize_native_session(lifetime).unwrap(),
            FinalAuthorityFault::Ended => runtime
                .end_native_session(lifetime, crate::session::SessionEnd::HostError)
                .unwrap(),
            FinalAuthorityFault::ExpiredAllowance => runtime
                .update(|record| {
                    record
                        .session_hook_allowance
                        .as_mut()
                        .unwrap()
                        .allocation
                        .deadline_ms = crate::workflow::allocation::now_ms()?;
                    Ok(())
                })
                .unwrap(),
            FinalAuthorityFault::RefreshedAllowance => runtime
                .update(|record| {
                    record.session_hook_allowance = Some(
                        super::super::session_budget::SessionHookAllowance::new(Limits {
                            seconds: 60,
                            model_calls: 4,
                            tool_calls: 4,
                        })?,
                    );
                    Ok(())
                })
                .unwrap(),
            FinalAuthorityFault::ResetAllowanceCounters => runtime
                .update(|record| {
                    record
                        .session_hook_allowance
                        .as_mut()
                        .unwrap()
                        .allocation
                        .model_calls = 0;
                    Ok(())
                })
                .unwrap(),
        }

        let error = runtime
            .apply_workspace_change(operation, &candidate)
            .expect_err("changed final authority unexpectedly published the new root");
        assert!(!error.to_string().is_empty());
        let record = runtime.record().unwrap();
        assert_eq!(record.workspace, old);
        assert_eq!(
            record.operations[operation as usize - 1].budget,
            original_budget,
            "final release must not renew or replace operation funding"
        );
        assert!(matches!(
            &record.operations[operation as usize - 1].host_invocation,
            Some(HostInvocation::WorkspaceChange(change)) if change.stage == Stage::Teardown
        ));
        assert!(!record.operations.iter().any(|candidate| {
            candidate.non_tool_receipt().is_some_and(|receipt| {
                receipt.facts.subject.occurrence.event()
                    == crate::plugins::hook_types::HookEvent::CwdChanged
            })
        }));
    }

    #[test]
    fn final_release_rejects_recovery_after_teardown() {
        assert_final_authority_fault_refuses(FinalAuthorityFault::Recovery);
    }

    #[test]
    fn final_release_rejects_revoked_lifetime_after_teardown() {
        assert_final_authority_fault_refuses(FinalAuthorityFault::Revoked);
    }

    #[test]
    fn final_release_rejects_ended_lifetime_after_teardown() {
        assert_final_authority_fault_refuses(FinalAuthorityFault::Ended);
    }

    #[test]
    fn final_release_rejects_expired_allowance_after_teardown() {
        assert_final_authority_fault_refuses(FinalAuthorityFault::ExpiredAllowance);
    }

    #[test]
    fn final_release_rejects_refreshed_allowance_after_teardown() {
        assert_final_authority_fault_refuses(FinalAuthorityFault::RefreshedAllowance);
    }

    #[test]
    fn final_release_rejects_reset_allowance_counters_after_teardown() {
        assert_final_authority_fault_refuses(FinalAuthorityFault::ResetAllowanceCounters);
    }

    #[test]
    fn final_release_accepts_unchanged_original_authority() {
        let (_parent, _old, runtime, candidate, _lifetime, operation) = final_authority_fixture();
        runtime
            .apply_workspace_change(operation, &candidate)
            .unwrap();
        assert_eq!(
            runtime.record().unwrap().workspace,
            candidate.occurrence.path
        );
    }

    #[test]
    fn final_release_accepts_monotonic_allowance_settlement() {
        let (_parent, _old, runtime, candidate, _lifetime, operation) = final_authority_fixture();
        runtime
            .update(|record| {
                let allowance = record.session_hook_allowance.as_mut().unwrap();
                allowance.allocation.model_calls += 1;
                allowance.allocation.tool_calls += 1;
                allowance.backend_invocations += 1;
                Ok(())
            })
            .unwrap();
        runtime
            .apply_workspace_change(operation, &candidate)
            .unwrap();
        assert_eq!(
            runtime.record().unwrap().workspace,
            candidate.occurrence.path
        );
    }

    fn applied_chain_fixture() -> (tempfile::TempDir, SharedRuntime, u64, u64, u64) {
        let parent = tempfile::tempdir().unwrap();
        let old = parent.path().join("old");
        let new = parent.path().join("new");
        std::fs::create_dir(&old).unwrap();
        std::fs::create_dir(&new).unwrap();
        let runtime = runtime(&old);
        let older_lifetime = runtime
            .begin_native_session(SessionStart::Startup, None, vec![])
            .unwrap();
        let current_lifetime = runtime
            .begin_native_session(SessionStart::Startup, None, vec![])
            .unwrap();
        let candidate = WorkspaceCandidate::capture(&new, 1).unwrap();
        let operation = runtime
            .begin_workspace_change(&candidate, "developer", current_lifetime, None, None)
            .unwrap();
        runtime.begin_workspace_change_teardown(operation).unwrap();
        runtime
            .apply_workspace_change(operation, &candidate)
            .unwrap();
        runtime.end_workspace_change(operation, None).unwrap();
        (parent, runtime, older_lifetime, current_lifetime, operation)
    }

    fn repin_workspace_change(record: &mut Record, id: u64) {
        let mut changed = match &record.operations[id as usize - 1].host_invocation {
            Some(HostInvocation::WorkspaceChange(change)) => change.as_ref().clone(),
            _ => panic!("workspace change missing"),
        };
        changed.pins = pins(record, &changed).unwrap();
        record.operations[id as usize - 1].host_invocation =
            Some(HostInvocation::WorkspaceChange(Box::new(changed)));
    }

    fn assert_generic_lifetime_rejects_invalid_root_chain(runtime: &SharedRuntime, lifetime: u64) {
        let record = runtime.record().unwrap();
        assert!(
            current_root(&record).is_err(),
            "invalid applied-root chain resolved a current workspace"
        );
        assert!(
            super::super::plugin_session::validate_host_lifetime(&record, lifetime).is_err(),
            "generic native lifetime accepted an invalid applied-root chain"
        );
    }

    #[test]
    fn generic_lifetime_rejects_workspace_changed_without_applied_transition() {
        let parent = tempfile::tempdir().unwrap();
        let old = parent.path().join("old");
        let unadmitted = parent.path().join("unadmitted");
        std::fs::create_dir(&old).unwrap();
        std::fs::create_dir(&unadmitted).unwrap();
        let runtime = runtime(&old);
        let lifetime = runtime
            .begin_native_session(SessionStart::Startup, None, vec![])
            .unwrap();
        runtime
            .update(|record| {
                record.workspace = unadmitted.clone();
                Ok(())
            })
            .unwrap();
        assert_generic_lifetime_rejects_invalid_root_chain(&runtime, lifetime);
    }

    #[test]
    fn generic_lifetime_rejects_applied_transition_with_wrong_original_owner() {
        let (_parent, runtime, older, current, operation) = applied_chain_fixture();
        runtime
            .update(|record| {
                let Some(HostInvocation::WorkspaceChange(change)) =
                    &mut record.operations[operation as usize - 1].host_invocation
                else {
                    unreachable!()
                };
                change.native_session = older;
                repin_workspace_change(record, operation);
                Ok(())
            })
            .unwrap();
        assert_generic_lifetime_rejects_invalid_root_chain(&runtime, current);
    }

    #[test]
    fn generic_lifetime_rejects_disconnected_applied_transition() {
        let (parent, runtime, _older, current, operation) = applied_chain_fixture();
        let disconnected = parent.path().join("disconnected");
        std::fs::create_dir(&disconnected).unwrap();
        let disconnected = RootOccurrence::capture(&disconnected, 0).unwrap();
        runtime
            .update(|record| {
                let Some(HostInvocation::WorkspaceChange(change)) =
                    &mut record.operations[operation as usize - 1].host_invocation
                else {
                    unreachable!()
                };
                change.from = disconnected.clone();
                repin_workspace_change(record, operation);
                Ok(())
            })
            .unwrap();
        assert_generic_lifetime_rejects_invalid_root_chain(&runtime, current);
    }

    #[test]
    fn generic_lifetime_rejects_wrong_applied_predecessor() {
        let (_parent, runtime, older, current, operation) = applied_chain_fixture();
        runtime
            .update(|record| {
                let Some(HostInvocation::WorkspaceChange(change)) =
                    &mut record.operations[operation as usize - 1].host_invocation
                else {
                    unreachable!()
                };
                change.predecessor = older;
                repin_workspace_change(record, operation);
                Ok(())
            })
            .unwrap();
        assert_generic_lifetime_rejects_invalid_root_chain(&runtime, current);
    }

    #[test]
    fn generic_lifetime_rejects_skipped_root_generation() {
        let (_parent, runtime, _older, current, operation) = applied_chain_fixture();
        runtime
            .update(|record| {
                let Some(HostInvocation::WorkspaceChange(change)) =
                    &mut record.operations[operation as usize - 1].host_invocation
                else {
                    unreachable!()
                };
                change.to.generation += 1;
                repin_workspace_change(record, operation);
                Ok(())
            })
            .unwrap();
        assert_generic_lifetime_rejects_invalid_root_chain(&runtime, current);
    }

    #[test]
    fn reconciled_failed_teardown_is_not_the_predecessor_of_a_fresh_applied_change() {
        let parent = tempfile::tempdir().unwrap();
        let a = parent.path().join("a");
        let abandoned = parent.path().join("abandoned");
        let c = parent.path().join("c");
        for root in [&a, &abandoned, &c] {
            std::fs::create_dir(root).unwrap();
        }
        let runtime = runtime(&a);
        let lifetime = runtime
            .begin_native_session(SessionStart::Startup, None, vec![])
            .unwrap();
        let funding = runtime.record().unwrap().operations[lifetime as usize - 1]
            .budget
            .clone();

        let abandoned_candidate = WorkspaceCandidate::capture(&abandoned, 1).unwrap();
        let abandoned_change = runtime
            .begin_workspace_change(&abandoned_candidate, "developer", lifetime, None, None)
            .unwrap();
        runtime
            .begin_workspace_change_teardown(abandoned_change)
            .unwrap();
        runtime
            .interrupt_workspace_change(abandoned_change)
            .unwrap();
        runtime
            .reconcile(
                "inspected interrupted teardown; destination was never applied",
                None,
            )
            .unwrap();

        let candidate = WorkspaceCandidate::capture(&c, 1).unwrap();
        let change = runtime
            .begin_workspace_change(&candidate, "developer", lifetime, None, None)
            .unwrap();
        let before_apply = runtime.record().unwrap();
        let Some(HostInvocation::WorkspaceChange(owner)) =
            &before_apply.operations[change as usize - 1].host_invocation
        else {
            panic!("fresh workspace owner missing")
        };
        assert_eq!(owner.predecessor, 0);
        assert_eq!(before_apply.operations[change as usize - 1].budget, funding);

        runtime.begin_workspace_change_teardown(change).unwrap();
        runtime.apply_workspace_change(change, &candidate).unwrap();
        runtime.end_workspace_change(change, None).unwrap();
        assert_eq!(runtime.workspace_root().unwrap(), candidate.occurrence);
        let record = runtime.record().unwrap();
        assert!(matches!(
            &record.operations[abandoned_change as usize - 1].host_invocation,
            Some(HostInvocation::WorkspaceChange(owner))
                if owner.stage == Stage::Teardown && owner.hold.is_some()
        ));
    }

    #[test]
    fn failed_attempt_after_an_applied_root_does_not_replace_its_predecessor() {
        let parent = tempfile::tempdir().unwrap();
        let a = parent.path().join("a");
        let b = parent.path().join("b");
        let abandoned = parent.path().join("abandoned");
        let d = parent.path().join("d");
        for root in [&a, &b, &abandoned, &d] {
            std::fs::create_dir(root).unwrap();
        }
        let runtime = runtime(&a);
        let lifetime = runtime
            .begin_native_session(SessionStart::Startup, None, vec![])
            .unwrap();

        let candidate_b = WorkspaceCandidate::capture(&b, 1).unwrap();
        let applied = runtime
            .begin_workspace_change(&candidate_b, "developer", lifetime, None, None)
            .unwrap();
        runtime.begin_workspace_change_teardown(applied).unwrap();
        runtime
            .apply_workspace_change(applied, &candidate_b)
            .unwrap();
        runtime.end_workspace_change(applied, None).unwrap();

        let abandoned_candidate = WorkspaceCandidate::capture(&abandoned, 2).unwrap();
        let abandoned_change = runtime
            .begin_workspace_change(&abandoned_candidate, "developer", lifetime, None, None)
            .unwrap();
        runtime
            .begin_workspace_change_teardown(abandoned_change)
            .unwrap();
        runtime
            .interrupt_workspace_change(abandoned_change)
            .unwrap();
        runtime
            .reconcile("inspected second interrupted teardown", None)
            .unwrap();

        let candidate_d = WorkspaceCandidate::capture(&d, 2).unwrap();
        let change = runtime
            .begin_workspace_change(&candidate_d, "developer", lifetime, None, None)
            .unwrap();
        let record = runtime.record().unwrap();
        let Some(HostInvocation::WorkspaceChange(owner)) =
            &record.operations[change as usize - 1].host_invocation
        else {
            panic!("fresh workspace owner missing")
        };
        assert_eq!(owner.predecessor, applied);
        runtime.begin_workspace_change_teardown(change).unwrap();
        runtime
            .apply_workspace_change(change, &candidate_d)
            .unwrap();
        runtime.end_workspace_change(change, None).unwrap();
        assert_eq!(runtime.workspace_root().unwrap(), candidate_d.occurrence);
    }

    #[test]
    fn pending_change_with_wrong_applied_predecessor_refuses_before_publication() {
        let parent = tempfile::tempdir().unwrap();
        let a = parent.path().join("a");
        let b = parent.path().join("b");
        let c = parent.path().join("c");
        for root in [&a, &b, &c] {
            std::fs::create_dir(root).unwrap();
        }
        let runtime = runtime(&a);
        let lifetime = runtime
            .begin_native_session(SessionStart::Startup, None, vec![])
            .unwrap();
        let candidate_b = WorkspaceCandidate::capture(&b, 1).unwrap();
        let applied = runtime
            .begin_workspace_change(&candidate_b, "developer", lifetime, None, None)
            .unwrap();
        runtime.begin_workspace_change_teardown(applied).unwrap();
        runtime
            .apply_workspace_change(applied, &candidate_b)
            .unwrap();
        runtime.end_workspace_change(applied, None).unwrap();

        let candidate_c = WorkspaceCandidate::capture(&c, 2).unwrap();
        let change = runtime
            .begin_workspace_change(&candidate_c, "developer", lifetime, None, None)
            .unwrap();
        runtime.begin_workspace_change_teardown(change).unwrap();
        runtime
            .update(|record| {
                let Some(HostInvocation::WorkspaceChange(owner)) =
                    &mut record.operations[change as usize - 1].host_invocation
                else {
                    unreachable!()
                };
                owner.predecessor = 0;
                repin_workspace_change(record, change);
                Ok(())
            })
            .unwrap();

        assert!(
            runtime
                .apply_workspace_change(change, &candidate_c)
                .is_err(),
            "a repinned but disconnected pending predecessor was published"
        );
        assert_eq!(
            runtime.record().unwrap().workspace,
            b.canonicalize().unwrap()
        );
    }

    #[test]
    fn a_new_native_lifetime_starts_its_own_applied_predecessor_chain() {
        let parent = tempfile::tempdir().unwrap();
        let a = parent.path().join("a");
        let b = parent.path().join("b");
        let c = parent.path().join("c");
        for root in [&a, &b, &c] {
            std::fs::create_dir(root).unwrap();
        }
        let runtime = runtime(&a);
        let first_lifetime = runtime
            .begin_native_session(SessionStart::Startup, None, vec![])
            .unwrap();
        let candidate_b = WorkspaceCandidate::capture(&b, 1).unwrap();
        let first_change = runtime
            .begin_workspace_change(&candidate_b, "developer", first_lifetime, None, None)
            .unwrap();
        runtime
            .begin_workspace_change_teardown(first_change)
            .unwrap();
        runtime
            .apply_workspace_change(first_change, &candidate_b)
            .unwrap();
        runtime.end_workspace_change(first_change, None).unwrap();
        runtime
            .end_native_session(first_lifetime, crate::session::SessionEnd::Shutdown)
            .unwrap();

        let second_lifetime = runtime
            .begin_native_session(SessionStart::Resume, None, vec![])
            .unwrap();
        let candidate_c = WorkspaceCandidate::capture(&c, 1).unwrap();
        let second_change = runtime
            .begin_workspace_change(&candidate_c, "developer", second_lifetime, None, None)
            .unwrap();
        let record = runtime.record().unwrap();
        let Some(HostInvocation::WorkspaceChange(owner)) =
            &record.operations[second_change as usize - 1].host_invocation
        else {
            panic!("second-lifetime workspace owner missing")
        };
        assert_eq!(owner.predecessor, 0);
        runtime
            .begin_workspace_change_teardown(second_change)
            .unwrap();
        runtime
            .apply_workspace_change(second_change, &candidate_c)
            .unwrap();
        runtime.end_workspace_change(second_change, None).unwrap();
        assert_eq!(runtime.workspace_root().unwrap(), candidate_c.occurrence);
    }

    fn apply_handoff_change(
        runtime: &SharedRuntime,
        root: &std::path::Path,
        generation: u64,
        lifetime: u64,
    ) -> u64 {
        let candidate = WorkspaceCandidate::capture(root, generation).unwrap();
        let handoff = runtime.workspace_handoff().unwrap();
        let change = runtime
            .begin_workspace_change(&candidate, "developer", lifetime, None, Some(handoff))
            .unwrap();
        runtime.begin_workspace_change_teardown(change).unwrap();
        runtime.apply_workspace_change(change, &candidate).unwrap();
        runtime.end_workspace_change(change, None).unwrap();
        change
    }

    #[test]
    fn handoff_reserve_bind_and_finish_require_the_current_native_lifetime() {
        let parent = tempfile::tempdir().unwrap();
        let a = parent.path().join("a");
        let x = parent.path().join("x");
        let b = parent.path().join("b");
        for root in [&a, &x, &b] {
            std::fs::create_dir(root).unwrap();
        }
        let runtime = runtime(&a);
        let first_lifetime = runtime
            .begin_native_session(SessionStart::Startup, None, vec![])
            .unwrap();
        apply_handoff_change(&runtime, &x, 1, first_lifetime);
        let first_b = apply_handoff_change(&runtime, &b, 2, first_lifetime);
        runtime
            .end_native_session(first_lifetime, crate::session::SessionEnd::Shutdown)
            .unwrap();

        let second_lifetime = runtime
            .begin_native_session(SessionStart::Resume, None, vec![])
            .unwrap();
        apply_handoff_change(&runtime, &x, 1, second_lifetime);
        let second_b = apply_handoff_change(&runtime, &b, 2, second_lifetime);
        assert_eq!(
            runtime
                .record()
                .unwrap()
                .operations
                .iter()
                .filter(|operation| matches!(
                    &operation.host_invocation,
                    Some(HostInvocation::WorkspaceChange(change))
                        if change.to == RootOccurrence::capture(&b, 2).unwrap()
                            && change.handoff.as_ref().is_some_and(|handoff| handoff.delivery == HandoffDelivery::Pending)
                ))
                .count(),
            2
        );

        runtime
            .update(|record| {
                let Some(HostInvocation::WorkspaceChange(change)) =
                    &mut record.operations[first_b as usize - 1].host_invocation
                else {
                    unreachable!()
                };
                change.handoff.as_mut().unwrap().delivery = HandoffDelivery::Sent;
                Ok(())
            })
            .unwrap();
        runtime
            .begin_phase("worker", Some("current input"))
            .unwrap();
        let provider = runtime.begin_model("worker").unwrap();
        let bind_error = runtime
            .bind_workspace_handoff_provider(provider)
            .expect_err("prior-lifetime handoff bound to the current provider");
        assert!(bind_error.to_string().contains("native lifetime"));

        runtime
            .update(|record| {
                let Some(HostInvocation::WorkspaceChange(change)) =
                    &mut record.operations[first_b as usize - 1].host_invocation
                else {
                    unreachable!()
                };
                change.handoff.as_mut().unwrap().provider_operation = Some(provider);
                Ok(())
            })
            .unwrap();
        let finish_error = runtime
            .finish_workspace_handoff_delivery(first_b, true)
            .expect_err("prior-lifetime handoff was completed by the current provider");
        assert!(finish_error.to_string().contains("native lifetime"));
        runtime
            .update(|record| {
                let Some(HostInvocation::WorkspaceChange(change)) =
                    &mut record.operations[first_b as usize - 1].host_invocation
                else {
                    unreachable!()
                };
                let handoff = change.handoff.as_mut().unwrap();
                handoff.delivery = HandoffDelivery::Pending;
                handoff.provider_operation = None;
                Ok(())
            })
            .unwrap();

        let (delivery, _) = runtime.begin_workspace_handoff_delivery().unwrap().unwrap();
        assert_eq!(delivery, second_b);
        runtime.bind_workspace_handoff_provider(provider).unwrap();
        runtime.finish_model(provider).unwrap();
        runtime
            .finish_workspace_handoff_delivery(delivery, true)
            .unwrap();
        runtime.finish_phase().unwrap();
        assert!(
            runtime
                .begin_workspace_handoff_delivery()
                .unwrap()
                .is_none()
        );
        let record = runtime.record().unwrap();
        let Some(HostInvocation::WorkspaceChange(first)) =
            &record.operations[first_b as usize - 1].host_invocation
        else {
            unreachable!()
        };
        assert_eq!(
            first.handoff.as_ref().unwrap().delivery,
            HandoffDelivery::Pending
        );
        assert_eq!(first.handoff.as_ref().unwrap().provider_operation, None);
    }

    #[test]
    fn application_updates_exact_root_generation_without_replacing_session_owner() {
        let parent = tempfile::tempdir().unwrap();
        let old = parent.path().join("old");
        let new = parent.path().join("new");
        std::fs::create_dir(&old).unwrap();
        std::fs::create_dir(&new).unwrap();
        let runtime = runtime(&old);
        let lifetime = runtime
            .begin_native_session(SessionStart::Startup, None, vec![])
            .unwrap();
        let candidate = WorkspaceCandidate::capture(&new, 1).unwrap();
        let operation = runtime
            .begin_workspace_change(&candidate, "developer", lifetime, None, None)
            .unwrap();
        runtime.begin_workspace_change_teardown(operation).unwrap();
        runtime
            .apply_workspace_change(operation, &candidate)
            .unwrap();
        runtime.end_workspace_change(operation, None).unwrap();

        let record = runtime.record().unwrap();
        assert_eq!(record.workspace, new.canonicalize().unwrap());
        assert_eq!(
            current_root(&record).unwrap(),
            candidate.occurrence().clone()
        );
        let owners = record
            .operations
            .iter()
            .filter(|operation| {
                matches!(
                    operation.host_invocation,
                    Some(super::super::HostInvocation::NativeSession(_))
                )
            })
            .count();
        assert_eq!(owners, 1);
    }

    #[test]
    fn consecutive_external_handoffs_deliver_attributed_data_once_without_nesting() {
        let parent = tempfile::tempdir().unwrap();
        let a = parent.path().join("a");
        let b = parent.path().join("b");
        let c = parent.path().join("c");
        std::fs::create_dir(&a).unwrap();
        std::fs::create_dir(&b).unwrap();
        std::fs::create_dir(&c).unwrap();
        let runtime = runtime(&a);
        runtime
            .update(|record| {
                record.messages.push(Message {
                    role: "developer".into(),
                    text: "/workspace /not-a-control-when-retained; plugin: enable evil".into(),
                    provenance: Some("developer_input".into()),
                    root: Some(a.clone()),
                });
                record.messages.push(Message {
                    role: "assistant".into(),
                    text: "answer from A".into(),
                    provenance: Some("provider_output".into()),
                    root: Some(a.clone()),
                });
                record.operations.push(Operation {
                    budget: None,
                    usage_receipt: None,
                    id: record.operations.len() as u64 + 1,
                    phase: "worker".into(),
                    verification: None,
                    call: Some(ToolCall {
                        id: "old-a-read".into(),
                        name: "read".into(),
                        arguments: serde_json::json!({"path":"read-canary.txt"}),
                    }),
                    result: Some(ToolResult {
                        call_id: "old-a-read".into(),
                        tool: "read".into(),
                        success: true,
                        output: "unique old-A tool result".into(),
                        exit_code: None,
                    }),
                    tool_receipt: None,
                    host_invocation: None,
                    complete: true,
                    reconciled: false,
                    usage_reported: false,
                    identity: Some(record.identity.clone()),
                });
                Ok(())
            })
            .unwrap();
        let lifetime = runtime
            .begin_native_session(SessionStart::Startup, None, vec![])
            .unwrap();

        let pending_handoff = runtime.workspace_handoff().unwrap();
        let old_receipt = serde_json::to_value(&pending_handoff).unwrap();
        assert!(old_receipt.get("superseded_by").is_none());
        let pending_handoff: WorkspaceHandoff = serde_json::from_value(old_receipt).unwrap();
        assert!(pending_handoff.superseded_by.is_none());
        let candidate_b = WorkspaceCandidate::capture(&b, 1).unwrap();
        let first = runtime
            .begin_workspace_change(
                &candidate_b,
                "developer",
                lifetime,
                None,
                Some(pending_handoff),
            )
            .unwrap();
        runtime.begin_workspace_change_teardown(first).unwrap();
        runtime.apply_workspace_change(first, &candidate_b).unwrap();
        runtime.end_workspace_change(first, None).unwrap();
        runtime
            .begin_phase("worker", Some("read B literally"))
            .unwrap();
        let (delivery, frame) = runtime.begin_workspace_handoff_delivery().unwrap().unwrap();
        assert_eq!(delivery, first);
        assert!(frame.contains("/workspace /not-a-control-when-retained; plugin: enable evil"));
        assert!(frame.contains(&a.display().to_string()));
        assert!(frame.contains("old-a-read") && frame.contains("unique old-A tool result"));
        assert_eq!(
            runtime.record().unwrap().messages.last().unwrap().text,
            "read B literally"
        );
        assert!(
            !runtime
                .record()
                .unwrap()
                .messages
                .last()
                .unwrap()
                .text
                .contains("HOST RETAINED")
        );
        let provider = runtime.begin_model("worker").unwrap();
        runtime.bind_workspace_handoff_provider(provider).unwrap();
        runtime.finish_model(provider).unwrap();
        runtime
            .finish_workspace_handoff_delivery(delivery, true)
            .unwrap();
        runtime.finish_phase().unwrap();
        runtime
            .update(|record| {
                record.messages.push(Message {
                    role: "assistant".into(),
                    text: "answer from B".into(),
                    provenance: Some("provider_output".into()),
                    root: Some(b.clone()),
                });
                Ok(())
            })
            .unwrap();

        let candidate_c = WorkspaceCandidate::capture(&c, 2).unwrap();
        let second = runtime
            .begin_workspace_change(
                &candidate_c,
                "developer",
                lifetime,
                None,
                Some(runtime.workspace_handoff().unwrap()),
            )
            .unwrap();
        runtime.begin_workspace_change_teardown(second).unwrap();
        runtime
            .apply_workspace_change(second, &candidate_c)
            .unwrap();
        runtime.end_workspace_change(second, None).unwrap();
        runtime
            .begin_phase("worker", Some("read C literally"))
            .unwrap();
        let (_, second_frame) = runtime.begin_workspace_handoff_delivery().unwrap().unwrap();
        assert!(second_frame.contains("answer from A"));
        assert!(second_frame.contains("answer from B"));
        assert!(
            second_frame.contains("old-a-read")
                && second_frame.contains("unique old-A tool result")
        );
        assert_eq!(
            second_frame
                .matches("[HOST RETAINED CONVERSATION AND TOOL EVIDENCE")
                .count(),
            1
        );
        let retained = runtime.record().unwrap();
        assert_eq!(
            retained
                .messages
                .iter()
                .filter(|message| message.text == "read B literally")
                .count(),
            1
        );
        assert_eq!(
            retained
                .messages
                .iter()
                .filter(|message| message.text == "read C literally")
                .count(),
            1
        );
        assert!(
            retained
                .messages
                .iter()
                .all(|message| !message.text.contains("HOST RETAINED"))
        );
    }

    #[test]
    fn failed_newer_change_does_not_supersede_the_current_pending_handoff() {
        let parent = tempfile::tempdir().unwrap();
        let a = parent.path().join("a");
        let b = parent.path().join("b");
        let c = parent.path().join("c");
        let retired_c = parent.path().join("retired-c");
        std::fs::create_dir(&a).unwrap();
        std::fs::create_dir(&b).unwrap();
        std::fs::create_dir(&c).unwrap();
        let runtime = runtime(&a);
        let lifetime = runtime
            .begin_native_session(SessionStart::Startup, None, vec![])
            .unwrap();

        let candidate_b = WorkspaceCandidate::capture(&b, 1).unwrap();
        let first = runtime
            .begin_workspace_change(
                &candidate_b,
                "developer",
                lifetime,
                None,
                Some(runtime.workspace_handoff().unwrap()),
            )
            .unwrap();
        runtime.begin_workspace_change_teardown(first).unwrap();
        runtime.apply_workspace_change(first, &candidate_b).unwrap();
        runtime.end_workspace_change(first, None).unwrap();

        let candidate_c = WorkspaceCandidate::capture(&c, 2).unwrap();
        let second = runtime
            .begin_workspace_change(
                &candidate_c,
                "developer",
                lifetime,
                None,
                Some(runtime.workspace_handoff().unwrap()),
            )
            .unwrap();
        runtime.begin_workspace_change_teardown(second).unwrap();
        std::fs::rename(&c, &retired_c).unwrap();
        std::fs::create_dir(&c).unwrap();
        assert!(
            runtime
                .apply_workspace_change(second, &candidate_c)
                .is_err()
        );

        let record = runtime.record().unwrap();
        let Some(HostInvocation::WorkspaceChange(first)) =
            &record.operations[first as usize - 1].host_invocation
        else {
            panic!("first workspace owner")
        };
        let handoff = first.handoff.as_ref().unwrap();
        assert_eq!(handoff.delivery, HandoffDelivery::Pending);
        assert!(handoff.provider_operation.is_none());
        assert!(handoff.superseded_by.is_none());
        assert!(matches!(
            &record.operations[second as usize - 1].host_invocation,
            Some(HostInvocation::WorkspaceChange(change)) if change.stage == Stage::Teardown
        ));
        assert_eq!(record.workspace, b.canonicalize().unwrap());
    }

    #[test]
    fn pre_provider_denial_keeps_pending_handoff_but_started_provider_is_uncertain() {
        let parent = tempfile::tempdir().unwrap();
        let a = parent.path().join("a");
        let b = parent.path().join("b");
        std::fs::create_dir(&a).unwrap();
        std::fs::create_dir(&b).unwrap();
        let runtime = runtime(&a);
        runtime
            .update(|record| {
                record.messages.push(Message {
                    role: "developer".into(),
                    text: "retained before denial".into(),
                    provenance: Some("developer_input".into()),
                    root: Some(a.clone()),
                });
                Ok(())
            })
            .unwrap();
        let lifetime = runtime
            .begin_native_session(SessionStart::Startup, None, vec![])
            .unwrap();
        let candidate = WorkspaceCandidate::capture(&b, 1).unwrap();
        let change = runtime
            .begin_workspace_change(
                &candidate,
                "developer",
                lifetime,
                None,
                Some(runtime.workspace_handoff().unwrap()),
            )
            .unwrap();
        runtime.begin_workspace_change_teardown(change).unwrap();
        runtime.apply_workspace_change(change, &candidate).unwrap();
        runtime.end_workspace_change(change, None).unwrap();
        runtime
            .begin_phase("worker", Some("denied before provider"))
            .unwrap();

        let (delivery, _) = runtime.begin_workspace_handoff_delivery().unwrap().unwrap();
        runtime
            .finish_workspace_handoff_delivery(delivery, false)
            .unwrap();
        let record = runtime.record().unwrap();
        let Some(HostInvocation::WorkspaceChange(owner)) =
            &record.operations[change as usize - 1].host_invocation
        else {
            panic!("workspace owner")
        };
        assert_eq!(
            owner.handoff.as_ref().unwrap().delivery,
            HandoffDelivery::Pending
        );
        assert!(owner.handoff.as_ref().unwrap().provider_operation.is_none());
        assert!(!record.recovery_pending);

        let (delivery, _) = runtime.begin_workspace_handoff_delivery().unwrap().unwrap();
        let provider = runtime.begin_model("worker").unwrap();
        runtime.bind_workspace_handoff_provider(provider).unwrap();
        runtime
            .finish_workspace_handoff_delivery(delivery, false)
            .unwrap();
        let record = runtime.record().unwrap();
        let Some(HostInvocation::WorkspaceChange(owner)) =
            &record.operations[change as usize - 1].host_invocation
        else {
            panic!("workspace owner")
        };
        assert_eq!(
            owner.handoff.as_ref().unwrap().delivery,
            HandoffDelivery::Uncertain
        );
        assert_eq!(
            owner.handoff.as_ref().unwrap().provider_operation,
            Some(provider)
        );
        assert!(record.recovery_pending);
        assert!(
            runtime
                .begin_workspace_handoff_delivery()
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn oversized_history_refuses_before_admission() {
        let parent = tempfile::tempdir().unwrap();
        let old = parent.path().join("old");
        let new = parent.path().join("new");
        std::fs::create_dir(&old).unwrap();
        std::fs::create_dir(&new).unwrap();
        let runtime = runtime(&old);
        runtime
            .update(|record| {
                record.messages.push(Message {
                    role: "developer".into(),
                    text: "x".repeat(2 * 1024 * 1024),
                    provenance: Some("developer_input".into()),
                    root: Some(old.clone()),
                });
                Ok(())
            })
            .unwrap();
        let before = runtime.record().unwrap();
        assert!(runtime.workspace_handoff().is_err());
        let after = runtime.record().unwrap();
        assert_eq!(after.operations.len(), before.operations.len());
        assert_eq!(after.workspace, old);
        assert!(!after.recovery_pending);
    }

    #[test]
    fn oversized_tool_evidence_refuses_before_admission() {
        let parent = tempfile::tempdir().unwrap();
        let old = parent.path().join("old");
        std::fs::create_dir(&old).unwrap();
        let runtime = runtime(&old);
        runtime
            .update(|record| {
                record.operations.push(Operation {
                    budget: None,
                    usage_receipt: None,
                    id: record.operations.len() as u64 + 1,
                    phase: "worker".into(),
                    verification: None,
                    call: Some(ToolCall {
                        id: "oversized-read".into(),
                        name: "read".into(),
                        arguments: serde_json::json!({"path":"canary"}),
                    }),
                    result: Some(ToolResult {
                        call_id: "oversized-read".into(),
                        tool: "read".into(),
                        success: true,
                        output: "x".repeat(2 * 1024 * 1024),
                        exit_code: None,
                    }),
                    tool_receipt: None,
                    host_invocation: None,
                    complete: true,
                    reconciled: false,
                    usage_reported: false,
                    identity: Some(record.identity.clone()),
                });
                Ok(())
            })
            .unwrap();

        let before = runtime.record().unwrap();
        assert!(runtime.workspace_handoff().is_err());
        let after = runtime.record().unwrap();
        assert_eq!(after.operations.len(), before.operations.len());
        assert_eq!(after.workspace, old);
        assert!(!after.recovery_pending);
    }

    #[test]
    fn restored_sent_handoff_becomes_uncertain_and_never_replays() {
        let parent = tempfile::tempdir().unwrap();
        let old = parent.path().join("old");
        let new = parent.path().join("new");
        std::fs::create_dir(&old).unwrap();
        std::fs::create_dir(&new).unwrap();
        let runtime = runtime(&old);
        runtime
            .update(|record| {
                record.messages.push(Message {
                    role: "developer".into(),
                    text: "retained".into(),
                    provenance: Some("developer_input".into()),
                    root: Some(old.clone()),
                });
                Ok(())
            })
            .unwrap();
        let lifetime = runtime
            .begin_native_session(SessionStart::Startup, None, vec![])
            .unwrap();
        let candidate = WorkspaceCandidate::capture(&new, 1).unwrap();
        let change = runtime
            .begin_workspace_change(
                &candidate,
                "developer",
                lifetime,
                None,
                Some(runtime.workspace_handoff().unwrap()),
            )
            .unwrap();
        runtime.begin_workspace_change_teardown(change).unwrap();
        runtime.apply_workspace_change(change, &candidate).unwrap();
        runtime.end_workspace_change(change, None).unwrap();
        assert_eq!(
            runtime
                .begin_workspace_handoff_delivery()
                .unwrap()
                .unwrap()
                .0,
            change
        );
        let mut restored = runtime.record().unwrap();

        interrupt_restored(&mut restored);

        assert!(restored.recovery_pending);
        let Some(HostInvocation::WorkspaceChange(change)) =
            &restored.operations[change as usize - 1].host_invocation
        else {
            panic!("workspace owner")
        };
        assert_eq!(
            change.handoff.as_ref().unwrap().delivery,
            HandoffDelivery::Uncertain
        );
        let reopened = SharedRuntime::for_test(&parent.path().join("reopened"), restored).unwrap();
        assert_eq!(
            reopened.workspace_root().unwrap(),
            candidate.occurrence().clone(),
            "uncertain handoff must retain the already-applied root fact"
        );
        assert!(
            reopened
                .begin_phase("worker", Some("must remain held"))
                .is_err(),
            "uncertain delivery must still deny ordinary work"
        );
        assert!(
            reopened
                .begin_workspace_handoff_delivery()
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn reconciled_applied_transition_remains_the_inspectable_root_fact() {
        let parent = tempfile::tempdir().unwrap();
        let old = parent.path().join("old");
        let new = parent.path().join("new");
        std::fs::create_dir(&old).unwrap();
        std::fs::create_dir(&new).unwrap();
        let runtime = runtime(&old);
        let lifetime = runtime
            .begin_native_session(SessionStart::Startup, None, vec![])
            .unwrap();
        let candidate = WorkspaceCandidate::capture(&new, 1).unwrap();
        let change = runtime
            .begin_workspace_change(&candidate, "developer", lifetime, None, None)
            .unwrap();
        runtime.begin_workspace_change_teardown(change).unwrap();
        runtime.apply_workspace_change(change, &candidate).unwrap();
        runtime.hold().unwrap();

        assert_eq!(
            runtime.workspace_root().unwrap(),
            candidate.occurrence().clone()
        );
        runtime
            .reconcile("inspected interrupted applied workspace publication", None)
            .unwrap();
        let record = runtime.record().unwrap();
        assert!(record.operations[change as usize - 1].reconciled);
        assert_eq!(
            runtime.workspace_root().unwrap(),
            candidate.occurrence().clone(),
            "reconciliation cannot erase an Applied root occurrence"
        );
    }

    #[test]
    fn invalid_busy_and_unresolved_states_refuse_without_archival_or_owner() {
        for case in ["unaccepted", "phase", "recovery", "child", "operation"] {
            let parent = tempfile::tempdir().unwrap();
            let old = parent.path().join("old");
            let new = parent.path().join("new");
            std::fs::create_dir(&old).unwrap();
            std::fs::create_dir(&new).unwrap();
            let runtime = runtime(&old);
            let lifetime = runtime
                .begin_native_session(SessionStart::Startup, None, vec![])
                .unwrap();
            runtime
                .update(|record| {
                    match case {
                        "unaccepted" => record.task.as_mut().unwrap().accepted = None,
                        "phase" => record.phase = Some("worker".into()),
                        "recovery" => record.recovery_pending = true,
                        "child" => record.agents.push(crate::inspection::tests::agent(
                            1,
                            crate::subagents::state::AgentStatus::Running,
                            &record.identity,
                        )),
                        "operation" => record.operations.push(Operation {
                            id: record.operations.len() as u64 + 1,
                            budget: None,
                            usage_receipt: None,
                            phase: "worker".into(),
                            verification: None,
                            call: None,
                            result: None,
                            tool_receipt: None,
                            host_invocation: Some(HostInvocation::Backend),
                            complete: false,
                            reconciled: false,
                            usage_reported: false,
                            identity: Some(record.identity.clone()),
                        }),
                        _ => unreachable!(),
                    }
                    Ok(())
                })
                .unwrap();
            let candidate = WorkspaceCandidate::capture(&new, 1).unwrap();
            let before = runtime.record().unwrap();

            assert!(
                runtime
                    .validate_workspace_change_request(&candidate, "developer", lifetime)
                    .is_err(),
                "{case} passed preflight"
            );
            assert!(
                runtime
                    .begin_workspace_change(&candidate, "developer", lifetime, None, None)
                    .is_err(),
                "{case} passed final admission"
            );

            let after = runtime.record().unwrap();
            assert_eq!(after.workspace, old);
            assert_eq!(after.archived.len(), before.archived.len());
            assert_eq!(after.operations.len(), before.operations.len());
            assert!(!after.operations.iter().any(|operation| matches!(
                operation.host_invocation,
                Some(HostInvocation::WorkspaceChange(_))
            )));
        }
    }

    #[test]
    fn applied_observer_failure_is_held_at_the_new_root_atomically() {
        let parent = tempfile::tempdir().unwrap();
        let old = parent.path().join("old");
        let new = parent.path().join("new");
        std::fs::create_dir(&old).unwrap();
        std::fs::create_dir(&new).unwrap();
        let runtime = runtime(&old);
        let lifetime = runtime
            .begin_native_session(SessionStart::Startup, None, vec![])
            .unwrap();
        let candidate = WorkspaceCandidate::capture(&new, 1).unwrap();
        let change = runtime
            .begin_workspace_change(&candidate, "developer", lifetime, None, None)
            .unwrap();
        runtime.begin_workspace_change_teardown(change).unwrap();
        runtime.apply_workspace_change(change, &candidate).unwrap();

        runtime
            .end_workspace_change(change, Some("injected CwdChanged observer failure".into()))
            .unwrap();

        let record = runtime.record().unwrap();
        assert_eq!(record.workspace, new.canonicalize().unwrap());
        assert!(record.recovery_pending);
        assert_eq!(
            runtime.workspace_root().unwrap(),
            candidate.occurrence().clone(),
            "an applied root remains a fact after its CwdChanged observer is held"
        );
        assert!(
            runtime
                .begin_phase("worker", Some("must remain held"))
                .is_err(),
            "factual root access must not clear recovery admission"
        );
        let operation = &record.operations[change as usize - 1];
        assert!(operation.complete);
        assert!(
            matches!(&operation.host_invocation, Some(HostInvocation::WorkspaceChange(owner))
            if owner.stage == Stage::Applied && owner.hold.as_deref() == Some("injected CwdChanged observer failure"))
        );
    }

    fn applied_workspace_change(value: &serde_json::Value) -> bool {
        value["operations"].as_array().is_some_and(|operations| {
            operations.iter().any(|operation| {
                operation["host_invocation"]["workspace_change"]["stage"] == "applied"
            })
        })
    }

    #[test]
    fn application_persistence_failure_retains_teardown_and_never_claims_rollback() {
        let parent = tempfile::tempdir().unwrap();
        let old = parent.path().join("old");
        let new = parent.path().join("new");
        std::fs::create_dir(&old).unwrap();
        std::fs::create_dir(&new).unwrap();
        let state = parent.path().join("state");
        let runtime = runtime(&old);
        let lifetime = runtime
            .begin_native_session(SessionStart::Startup, None, vec![])
            .unwrap();
        let candidate = WorkspaceCandidate::capture(&new, 1).unwrap();
        let change = runtime
            .begin_workspace_change(&candidate, "developer", lifetime, None, None)
            .unwrap();
        runtime.begin_workspace_change_teardown(change).unwrap();
        runtime.fail_config_change_receipt_before_rename_when(applied_workspace_change);

        let error = runtime
            .apply_workspace_change(change, &candidate)
            .unwrap_err();
        assert!(format!("{error:#}").contains("execution is held"));
        let persisted: Record = serde_json::from_value(
            crate::workflow::store::Store::read_snapshot(&runtime.directory().unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(persisted.workspace, old);
        assert!(
            matches!(&persisted.operations[change as usize - 1].host_invocation,
            Some(HostInvocation::WorkspaceChange(owner)) if owner.stage == Stage::Teardown)
        );
        let reopened = SharedRuntime::for_test(&state, persisted).unwrap();
        assert!(
            reopened
                .begin_workspace_change(&candidate, "developer", lifetime, None, None)
                .is_err()
        );
    }

    #[tokio::test]
    async fn original_session_end_stays_bound_to_a_and_physical_replacement_only_blocks_observation()
     {
        let parent = tempfile::tempdir().unwrap();
        let old = parent.path().join("old");
        let new = parent.path().join("new");
        std::fs::create_dir(&old).unwrap();
        std::fs::create_dir(&new).unwrap();
        let first_runtime = runtime(&old);
        let plans = vec![(
            crate::plugins::hook_types::HookEvent::SessionEnd,
            "original-end-policy".into(),
        )];
        let lifetime = first_runtime
            .begin_native_session(SessionStart::Startup, None, plans.clone())
            .unwrap();
        let candidate = WorkspaceCandidate::capture(&new, 1).unwrap();
        let change = first_runtime
            .begin_workspace_change(&candidate, "developer", lifetime, None, None)
            .unwrap();
        first_runtime
            .begin_workspace_change_teardown(change)
            .unwrap();
        first_runtime
            .apply_workspace_change(change, &candidate)
            .unwrap();
        first_runtime.end_workspace_change(change, None).unwrap();
        first_runtime
            .end_native_session(lifetime, crate::session::SessionEnd::Shutdown)
            .unwrap();

        first_runtime
            .validate_native_end_policy(lifetime, crate::session::SessionEnd::Shutdown, &plans)
            .unwrap();
        let facts = first_runtime
            .begin_non_tool_owned(
                "native-session",
                None,
                crate::plugins::receipts::LifecycleOrigin {
                    native_session: Some(lifetime),
                    ..Default::default()
                },
                crate::plugins::receipts::NonToolOccurrence::SessionEnd {
                    reason: crate::session::SessionEnd::Shutdown,
                },
                "original-end-policy".into(),
                vec![],
            )
            .unwrap();
        assert_eq!(facts.workspace, {
            let metadata = std::fs::metadata(&old).unwrap();
            (metadata.dev(), metadata.ino())
        });
        first_runtime
            .settle_non_tool(
                facts.operation,
                crate::plugins::hook_types::HookEvent::SessionEnd,
                Default::default(),
            )
            .unwrap();

        let physical_parent = tempfile::tempdir().unwrap();
        let physical_old = physical_parent.path().join("old");
        std::fs::create_dir(&physical_old).unwrap();
        let second = runtime(&physical_old);
        let second_lifetime = second
            .begin_native_session(SessionStart::Startup, None, plans.clone())
            .unwrap();
        std::fs::rename(&physical_old, physical_parent.path().join("renamed-old")).unwrap();
        std::fs::create_dir(&physical_old).unwrap();
        second
            .end_native_session(second_lifetime, crate::session::SessionEnd::Shutdown)
            .unwrap();
        let error = second
            .validate_native_end_policy(
                second_lifetime,
                crate::session::SessionEnd::Shutdown,
                &plans,
            )
            .unwrap_err();
        assert!(
            format!("{error:#}").contains("original SessionEnd workspace identity was replaced")
        );
        assert!(!second.record().unwrap().operations.iter().any(|operation| {
            matches!(&operation.host_invocation, Some(HostInvocation::Lifecycle(receipt))
                if receipt.facts.subject.occurrence.event() == crate::plugins::hook_types::HookEvent::SessionEnd)
        }));
        second
            .cancel_native_session_services(second_lifetime)
            .unwrap();
        second
            .drain_native_session_services(
                second_lifetime,
                tokio::time::Instant::now() + std::time::Duration::from_secs(1),
            )
            .await
            .unwrap();
        second.finalize_native_session(second_lifetime).unwrap();
        let record = second.record().unwrap();
        let Some(HostInvocation::NativeSession(owner)) =
            &record.operations[second_lifetime as usize - 1].host_invocation
        else {
            panic!("native owner")
        };
        assert_eq!(
            owner.workspace_path.as_deref(),
            Some(physical_old.as_path())
        );
        assert_eq!(owner.plans, plans);
        assert_eq!(owner.end, Some(crate::session::SessionEnd::Shutdown));
    }
}
