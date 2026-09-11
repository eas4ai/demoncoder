//! Host binding from the existing exact worker assignment; package roles grant no owner.
use super::{Record, delegation};
use crate::{
    subagents::state::{AgentStatus, OrchestrationStage},
    workflow::runtime::Identity,
};
use anyhow::{Context, Result, ensure};
use std::{os::unix::fs::MetadataExt, path::Path};

pub(super) struct Owner<'a> {
    pub identity: &'a Identity,
    pub root: &'a Path,
    pub child: Option<String>,
}
pub(super) fn resolve<'a>(record: &'a Record, phase: &str) -> Result<Owner<'a>> {
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
    let fingerprint = crate::plugins::admission::digest(&(
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
        allocation.started_ms,
        allocation.deadline_ms,
        &allocation.limits,
        record
            .delegation
            .as_ref()
            .map(|d| (&d.orchestration, d.backend_limit)),
    ))?;
    Ok(Owner {
        identity: &child.identity,
        root: &worktree.root,
        child: Some(fingerprint),
    })
}
pub(super) fn validate(
    record: &Record,
    operation: &super::Operation,
    receipt: &super::NonToolReceipt,
) -> Result<()> {
    let owner = resolve(record, &operation.phase)?;
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
