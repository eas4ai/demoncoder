//! Durable orchestration projections and supervision state helpers.

use anyhow::{Context, Result, ensure};
use serde_json::json;

use crate::{
    events::EventSink,
    session::Session,
    workflow::review::{Role, Verdict},
};

use super::{
    manager::Manager,
    schedule::{self, Node, Status},
    state::{AgentRecord, AgentStatus, OrchestrationStage},
    worktree,
};

pub fn dependency_nodes(agents: &[AgentRecord]) -> Result<Vec<Node>> {
    let nodes: Vec<_> = agents
        .iter()
        .filter_map(|agent| {
            agent.orchestration.as_ref().map(|state| Node {
                id: agent.id,
                dependencies: state.dependencies.clone(),
                status: match agent.status {
                    AgentStatus::Queued => Status::Queued,
                    AgentStatus::Preparing
                    | AgentStatus::Running
                    | AgentStatus::Validating
                    | AgentStatus::Integrating => Status::Active,
                    AgentStatus::Integrated => Status::Integrated,
                    AgentStatus::Ready | AgentStatus::Stopped => Status::AwaitingIntegration,
                    AgentStatus::Failed | AgentStatus::Cancelled | AgentStatus::Uncertain => {
                        Status::Blocked
                    }
                },
            })
        })
        .collect();
    schedule::validate_graph(&nodes).context("invalid retained dependency graph")?;
    Ok(nodes)
}

pub(super) async fn run(
    manager: &Manager,
    id: u64,
    events: &EventSink,
    worker_session: &mut Option<Box<dyn Session>>,
) -> Result<()> {
    loop {
        let current = match manager.collect_current_evidence(id, events).await {
            Ok(current) => current,
            Err(error) => {
                manager.hold(
                    id,
                    format!("Current source or checks could not be captured: {error:#}"),
                )?;
                return Ok(());
            }
        };
        let advisor = match manager
            .role_receipt(id, Role::Advisor, &current, None, events)
            .await
        {
            Ok(receipt) => receipt,
            Err(error) => {
                manager.hold(id, format!("Advisor was not admitted: {error:#}"))?;
                return Ok(());
            }
        };
        match advisor.verdict {
            Verdict::Clear if current.checks_pass => {
                manager.ready(id, &advisor)?;
                return Ok(());
            }
            Verdict::Clear => {
                manager.hold(
                    id,
                    "Advisor was clear, but one or more selected checks failed.".into(),
                )?;
                return Ok(());
            }
            Verdict::Blocked => {
                manager.hold(
                    id,
                    format!("Advisor blocked supervision: {}", advisor.explanation),
                )?;
                return Ok(());
            }
            Verdict::Findings => {}
        }

        let response = match manager
            .role_receipt(
                id,
                Role::WorkerResponse,
                &current,
                Some(json!({"advisor": advisor})),
                events,
            )
            .await
        {
            Ok(receipt) => receipt,
            Err(error) => {
                manager.hold(id, format!("Worker response was not admitted: {error:#}"))?;
                return Ok(());
            }
        };
        if response.verdict == Verdict::Blocked {
            manager.hold(
                id,
                format!(
                    "Worker response was blocked or invalid: {}",
                    response.explanation
                ),
            )?;
            return Ok(());
        }

        let judge = match manager
            .role_receipt(
                id,
                Role::Judge,
                &current,
                Some(json!({"advisor": advisor, "response": response})),
                events,
            )
            .await
        {
            Ok(receipt) => receipt,
            Err(error) => {
                manager.hold(id, format!("Judge was not admitted: {error:#}"))?;
                return Ok(());
            }
        };
        match judge.verdict {
            Verdict::Clear if current.checks_pass => {
                let identity = manager
                    .record(id)?
                    .worktree
                    .context("agent has no worktree")?;
                ensure!(
                    worktree::inspect(&identity).await?.digest == current.snapshot,
                    "child changed after judge evidence; assignment is held"
                );
                manager.ready(id, &judge)?;
                return Ok(());
            }
            Verdict::Clear => {
                manager.hold(id, "Judge cannot clear failed selected checks.".into())?;
                return Ok(());
            }
            Verdict::Blocked => {
                manager.hold(
                    id,
                    format!("Judge blocked supervision: {}", judge.explanation),
                )?;
                return Ok(());
            }
            Verdict::Findings => {
                let correction_limit = manager
                    .settings
                    .orchestration
                    .as_ref()
                    .context("orchestration settings disappeared")?
                    .correction_limit;
                let correction = manager.runtime.update_agent(id, |agent| {
                    let state = agent
                        .orchestration
                        .as_mut()
                        .context("orchestration state disappeared")?;
                    match state.admit_correction(correction_limit) {
                        Ok(round) => {
                            agent.status = AgentStatus::Running;
                            state.stage = OrchestrationStage::Correcting;
                            state.reason =
                                format!("Correction round {round} admitted before worker effects.");
                            agent.outcome = state.reason.clone();
                            Ok(Some(round))
                        }
                        Err(_) => Ok(None),
                    }
                })?;
                let Some(round) = correction else {
                    manager.hold(
                        id,
                        "Two correction rounds were spent; unresolved findings remain.".into(),
                    )?;
                    return Ok(());
                };
                manager.publish(id, events)?;
                let correction_evidence = serde_json::to_string(&json!({
                    "runtime_evidence": current.value,
                    "advisor": advisor,
                    "response": response,
                    "judge": judge,
                }))?;
                manager
                    .worker_turn(
                        id,
                        events,
                        worker_session,
                        Some((round, correction_evidence)),
                    )
                    .await?;
                manager.runtime.update_agent(id, |agent| {
                    agent.completed = true;
                    agent.status = AgentStatus::Validating;
                    let state = agent
                        .orchestration
                        .as_mut()
                        .context("orchestration state disappeared")?;
                    state.stage = OrchestrationStage::Checking;
                    state.reason =
                        format!("Correction round {round} completed; collecting fresh evidence.");
                    agent.outcome = state.reason.clone();
                    Ok(())
                })?;
            }
        }
    }
}
