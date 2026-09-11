//! Correction authority belongs to the exact task or supervised assignment.
use super::{PostToolFacts, Record, delegation};
use crate::subagents::state::{AgentStatus, OrchestrationStage};
use anyhow::{Context, Result, ensure};

enum Owner {
    Task,
    Child { id: u64, limit: u32 },
}

fn resolve(record: &Record, facts: &PostToolFacts, available: bool) -> Result<Owner> {
    resolve_owner(
        record,
        facts.task,
        &facts.role,
        facts.source_operation,
        available,
        false,
    )
}
fn resolve_owner(
    record: &Record,
    task_id: Option<u64>,
    role: &str,
    source_operation: u64,
    available: bool,
    observer: bool,
) -> Result<Owner> {
    ensure!(
        task_id == record.task.as_ref().map(|t| t.id)
            && record.task.as_ref().is_none_or(|t| t.accepted.is_none()),
        "post-tool correction parent owner changed or accepted"
    );
    if role == "worker" {
        let task = record
            .task
            .as_ref()
            .context("post-tool correction task missing")?;
        ensure!(
            (observer || !task.stopped) && (!available || task.corrections < task.correction_limit),
            "post-tool task correction unavailable"
        );
        return Ok(Owner::Task);
    }
    let id: u64 = role
        .strip_prefix("agent:")
        .and_then(|s| s.strip_suffix(":worker"))
        .context("post-tool correction requires an exact worker phase")?
        .parse()?;
    ensure!(
        role == format!("agent:{id}:worker"),
        "post-tool child worker phase is not canonical"
    );
    delegation::ensure_agent_active(record, role)?;
    let child = record
        .agents
        .iter()
        .find(|a| a.id == id)
        .context("post-tool child missing")?;
    let source = record
        .operations
        .iter()
        .find(|o| o.id == source_operation)
        .context("post-tool child source missing")?;
    ensure!(
        child.status == AgentStatus::Running
            && child.parent_task == task_id
            && source.phase == role
            && source.identity.as_ref().unwrap_or(&record.identity) == &child.identity,
        "post-tool child identity, assignment or worker status changed"
    );
    let limit = record
        .delegation
        .as_ref()
        .and_then(|d| d.orchestration.as_ref())
        .context("post-tool child lacks retained supervision authority")?
        .correction_limit;
    ensure!(limit == 2, "post-tool child supervision limit changed");
    let state = child
        .orchestration
        .as_ref()
        .context("post-tool child lacks a correction ledger")?;
    // The supervisor retains completion of the initial worker while running
    // an admitted corrective worker. Completion alone does not end that phase.
    ensure!(
        !child.completed
            || (state.stage == OrchestrationStage::Correcting && state.correction_rounds > 0),
        "post-tool child completed outside an admitted corrective worker phase"
    );
    ensure!(
        matches!(
            state.stage,
            OrchestrationStage::Working | OrchestrationStage::Correcting
        ) && (!available || state.correction_rounds < limit),
        "post-tool child correction is unavailable in this supervision stage"
    );
    Ok(Owner::Child { id, limit })
}

pub(super) fn available(record: &Record, facts: &PostToolFacts) -> bool {
    resolve(record, facts, true).is_ok()
}

pub(super) fn validate(record: &Record, facts: &PostToolFacts) -> Result<()> {
    resolve(record, facts, false).map(|_| ())
}

pub(super) fn objective<'a>(record: &'a Record, facts: &PostToolFacts) -> Result<&'a str> {
    Ok(match resolve(record, facts, true)? {
        Owner::Task => &record.task.as_ref().expect("validated").objective,
        Owner::Child { id, .. } => {
            &record
                .agents
                .iter()
                .find(|a| a.id == id)
                .expect("validated")
                .request
                .objective
        }
    })
}

pub(super) fn charge(record: &mut Record, facts: &PostToolFacts) -> Result<()> {
    let owner = resolve(record, facts, true)?;
    charge_owner(record, owner)
}
fn charge_owner(record: &mut Record, owner: Owner) -> Result<()> {
    match owner {
        Owner::Task => record.task.as_mut().expect("validated").start_work(true)?,
        Owner::Child { id, limit } => {
            let child = record
                .agents
                .iter_mut()
                .find(|a| a.id == id)
                .expect("validated");
            let state = child.orchestration.as_mut().expect("validated");
            let round = state.admit_correction(limit)?;
            state.stage = OrchestrationStage::Correcting;
            state.reason = format!(
                "Plugin-origin correction round {round} admitted before worker continuation; retained supervision findings still require verification."
            );
            child.outcome = state.reason.clone();
        }
    }
    Ok(())
}

pub(in crate::workflow::runtime) fn observer_available(
    record: &Record,
    hook: &crate::plugins::receipts::HookReceipt,
) -> bool {
    hook.observer.as_ref().is_some_and(|o| {
        resolve_owner(
            record,
            o.task,
            &hook.inspected.role,
            hook.inspected.source_operation,
            true,
            true,
        )
        .is_ok()
    })
}
pub(in crate::workflow::runtime) fn charge_observer(
    record: &mut Record,
    hook: &crate::plugins::receipts::HookReceipt,
) -> Result<()> {
    let observer = hook.observer.as_ref().context("observer owner missing")?;
    let owner = resolve_owner(
        record,
        observer.task,
        &hook.inspected.role,
        hook.inspected.source_operation,
        true,
        true,
    )?;
    charge_owner(record, owner)
}
