//! Host binding from the existing exact worker assignment; package roles grant no owner.
use super::{Record, delegation};
use crate::{
    subagents::state::{AgentStatus, OrchestrationStage},
    workflow::runtime::Identity,
};
use anyhow::{Context, Result, ensure};
use std::{os::unix::fs::MetadataExt, path::Path};

pub(in crate::workflow::runtime) struct Owner<'a> {
    pub identity: &'a Identity,
    pub root: &'a Path,
    pub child: Option<String>,
}
pub(in crate::workflow::runtime) fn resolve<'a>(
    record: &'a Record,
    phase: &str,
) -> Result<Owner<'a>> {
    ensure!(
        !record.recovery_pending,
        "lifecycle owner requires reconciliation"
    );
    ensure!(
        record.task.as_ref().is_none_or(|t| t.accepted.is_none()),
        "lifecycle parent task accepted"
    );
    if phase == "worker" {
        ensure!(
            record.phase.as_deref() == Some(phase)
                && record.task.as_ref().is_none_or(|t| !t.stopped),
            "lifecycle requires active parent worker"
        );
        return Ok(Owner {
            identity: &record.identity,
            root: &record.workspace,
            child: None,
        });
    }
    let id: u64 = phase
        .strip_prefix("agent:")
        .and_then(|s| s.strip_suffix(":worker"))
        .context("lifecycle requires exact worker phase")?
        .parse()?;
    ensure!(
        phase == format!("agent:{id}:worker"),
        "noncanonical child worker phase"
    );
    delegation::ensure_agent_active(record, phase)?;
    let child = record
        .agents
        .iter()
        .find(|a| a.id == id)
        .context("child assignment missing")?;
    ensure!(
        child.status == AgentStatus::Running,
        "child lifecycle requires running worker assignment"
    );
    if let Some(state) = &child.orchestration {
        ensure!(
            matches!(
                state.stage,
                OrchestrationStage::Working | OrchestrationStage::Correcting
            ) && (!child.completed
                || (state.stage == OrchestrationStage::Correcting && state.correction_rounds > 0)),
            "child lifecycle outside admitted worker stage"
        );
    } else {
        ensure!(!child.completed, "child worker already completed");
    }
    let worktree = child
        .worktree
        .as_ref()
        .context("child lifecycle worktree missing")?;
    ensure!(
        child.planned_root.as_ref() == Some(&worktree.root),
        "child planned worktree changed"
    );
    for (path, device, inode) in [
        (&worktree.git_dir, worktree.git_device, worktree.git_inode),
        (
            &worktree.common_dir,
            worktree.common_device,
            worktree.common_inode,
        ),
    ] {
        let metadata = std::fs::metadata(path).context("child Git identity unavailable")?;
        ensure!(
            (metadata.dev(), metadata.ino()) == (device, inode),
            "child Git identity changed"
        );
    }
    let allocation = record
        .allocation
        .as_ref()
        .context("child lifecycle requires owning allocation")?;
    ensure!(
        allocation.remaining_ms()? > 0,
        "child lifecycle allocation expired"
    );
    let fingerprint = child_fingerprint(record, child)?;
    Ok(Owner {
        identity: &child.identity,
        root: &worktree.root,
        child: Some(fingerprint),
    })
}
pub(in crate::workflow::runtime) fn child_fingerprint(
    record: &Record,
    child: &crate::subagents::state::AgentRecord,
) -> Result<String> {
    let worktree = child.worktree.as_ref().context("child worktree missing")?;
    let allocation = record
        .allocation
        .as_ref()
        .context("child allocation missing")?;
    crate::plugins::admission::digest(&(
        child.id,
        child.parent_task,
        &child.request,
        &child.identity,
        child.origin,
        &child.planned_root,
        (
            &worktree.root,
            &worktree.git_dir,
            &worktree.common_dir,
            worktree.git_device,
            worktree.git_inode,
            worktree.common_device,
            worktree.common_inode,
            &worktree.repository_head,
            &worktree.baseline_commit,
            &worktree.parent_baseline.digest,
            &worktree.child_baseline.digest,
        ),
        record.task_allocation_epoch,
        allocation.started_ms,
        allocation.deadline_ms,
        &allocation.limits,
        record
            .delegation
            .as_ref()
            .map(|d| (&d.orchestration, d.backend_limit)),
    ))
}
pub(in crate::workflow::runtime) fn validate(
    record: &Record,
    operation: &super::Operation,
    receipt: &super::NonToolReceipt,
) -> Result<()> {
    super::super::budget_accounting::active_operation(record, &receipt.facts.session, operation)?;
    if let Some(id) = receipt.facts.host_session {
        let authority = receipt
            .host_control
            .as_ref()
            .context("settings control authority is no longer executable")?;
        ensure!(authority.lifetime == id, "settings host owner differs");
        let (identity, workspace) =
            super::super::plugin_session::validate_host_control(record, authority)?;
        let (_, lifetime) = super::super::plugin_session::lifetime(record, id)?;
        ensure!(
            matches!(
                receipt.facts.subject.occurrence,
                crate::plugins::receipts::NonToolOccurrence::ConfigChange { .. }
            ) && operation.phase == "settings"
                && receipt.facts.role == operation.phase
                && operation.identity.as_ref() == Some(identity)
                && receipt.facts.workspace == workspace
                && receipt.facts.session == lifetime.session
                && receipt.facts.native_session.is_none()
                && receipt.facts.native_turn.is_none()
                && receipt.facts.callback.is_none()
                && receipt.facts.task.is_none()
                && receipt.facts.child_owner.is_none(),
            "settings control owner, identity, workspace or host session changed"
        );
        return Ok(());
    }
    if let Some(id) = receipt.facts.native_session {
        let (identity, workspace) =
            super::super::plugin_session::validate(record, id, &receipt.facts.subject.occurrence)?;
        let (_, lifetime) = super::super::plugin_session::lifetime(record, id)?;
        ensure!(
            operation.phase == "native-session"
                && receipt.facts.role == operation.phase
                && operation.identity.as_ref() == Some(identity)
                && receipt.facts.workspace == workspace
                && receipt.facts.session == lifetime.session
                && receipt.facts.native_turn.is_none()
                && receipt.facts.callback.is_none()
                && receipt.facts.child_owner.is_none(),
            "native session observation owner differs"
        );
        return Ok(());
    }
    if let crate::plugins::receipts::NonToolOccurrence::PostToolBatch {
        batch: Some(id),
        tool_calls,
    } = &receipt.facts.subject.occurrence
    {
        super::super::tool_batches::validate_observation(
            record,
            &operation.phase,
            *id,
            tool_calls,
        )?;
    }
    if let Some(turn) = receipt.facts.native_turn {
        super::turn::validate(record, turn, &operation.phase)?;
    }
    let owner = if matches!(
        receipt.facts.subject.occurrence,
        crate::plugins::receipts::NonToolOccurrence::PreCompact {
            compaction: Some(_),
            ..
        } | crate::plugins::receipts::NonToolOccurrence::PostCompact {
            compaction: Some(_),
            ..
        }
    ) {
        super::super::compaction::validate_occurrence(
            record,
            &operation.phase,
            &receipt.facts.subject.occurrence,
        )?;
        super::super::compaction::owner(record, &operation.phase)?
    } else if matches!(
        receipt.facts.subject.occurrence,
        crate::plugins::receipts::NonToolOccurrence::PreModelSwitch { .. }
            | crate::plugins::receipts::NonToolOccurrence::PostModelSwitch { .. }
    ) {
        super::super::model_switch::validate_occurrence(
            record,
            receipt
                .facts
                .subject
                .occurrence
                .host_operation()
                .context("model switch operation missing")?,
            &receipt.facts.subject.occurrence,
            &receipt.plan,
        )?;
        super::super::model_switch::owner(
            record,
            receipt
                .facts
                .subject
                .occurrence
                .host_operation()
                .expect("validated"),
            receipt.facts.subject.occurrence.event(),
        )?
    } else if matches!(
        receipt.facts.subject.occurrence,
        crate::plugins::receipts::NonToolOccurrence::CwdChanged { .. }
    ) {
        super::super::workspace_change::validate_occurrence(
            record,
            receipt
                .facts
                .subject
                .occurrence
                .host_operation()
                .context("workspace change operation missing")?,
            &receipt.facts.subject.occurrence,
            &receipt.plan,
        )?;
        super::super::workspace_change::owner(
            record,
            receipt
                .facts
                .subject
                .occurrence
                .host_operation()
                .expect("validated"),
        )?
    } else {
        resolve(record, &operation.phase)?
    };
    if let Some(callback) = &receipt.facts.callback {
        let source = receipt
            .facts
            .source
            .as_ref()
            .context("source callback input missing")?;
        if matches!(
            receipt.facts.subject.occurrence,
            crate::plugins::receipts::NonToolOccurrence::PreModelSwitch { .. }
                | crate::plugins::receipts::NonToolOccurrence::PostModelSwitch { .. }
        ) {
            super::super::model_switch::validate_source(
                record,
                receipt
                    .facts
                    .subject
                    .occurrence
                    .host_operation()
                    .expect("validated"),
                callback,
                source,
            )?;
        } else {
            validate_backend(
                record,
                &operation.phase,
                owner.identity,
                callback.backend_operation,
            )?;
            validate_source(
                record,
                &operation.phase,
                callback,
                &receipt.facts.subject.occurrence,
                source,
            )?;
        }
    }
    let metadata = std::fs::metadata(owner.root)?;
    ensure!(
        operation.identity.as_ref() == Some(owner.identity)
            && receipt.facts.role == operation.phase
            && receipt.facts.child_owner == owner.child
            && receipt
                .facts
                .declaration_role
                .as_deref()
                .unwrap_or("worker")
                == "worker"
            && receipt.facts.workspace == (metadata.dev(), metadata.ino())
            && receipt.facts.task == record.task.as_ref().map(|t| t.id),
        "lifecycle assignment, identity, workspace or allocation changed"
    );
    Ok(())
}

pub(in crate::workflow::runtime) fn validate_backend(
    record: &Record,
    phase: &str,
    identity: &Identity,
    id: u64,
) -> Result<()> {
    let operation = record
        .operations
        .iter()
        .find(|o| o.id == id)
        .context("source lifecycle backend owner missing")?;
    ensure!(
        matches!(
            operation.host_invocation,
            Some(super::HostInvocation::Backend | super::HostInvocation::HookBackend { .. })
        ) && operation.phase == phase
            && operation.identity.as_ref().unwrap_or(&record.identity) == identity
            && (phase == "worker" || operation.identity.is_some())
            && !operation.complete
            && !operation.reconciled
            && operation.call.is_none()
            && operation.tool_receipt.is_none(),
        "source lifecycle backend owner changed or ended"
    );
    Ok(())
}

pub(in crate::workflow::runtime) fn validate_source(
    record: &Record,
    phase: &str,
    callback: &crate::plugins::receipts::SourceCallback,
    occurrence: &crate::plugins::receipts::NonToolOccurrence,
    source: &crate::plugins::receipts::ObservedLifecycle,
) -> Result<Option<u64>> {
    use crate::plugins::receipts::{NonToolOccurrence, SourceOrigin};
    let origin = callback
        .origin
        .as_ref()
        .context("source callback origin is unknown")?;
    if let NonToolOccurrence::UserPromptSubmit { correction, .. } = occurrence {
        ensure!(
            *correction != matches!(origin, SourceOrigin::HostSubmission),
            "source submission origin contradicts its occurrence"
        );
    }
    super::super::plugin_lifecycle::source_correction_owner(
        record, phase, callback, occurrence, source,
    )
}
