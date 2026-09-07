//! Durable assignment identity and the gate for integrating child work.
use std::path::{Component, Path, PathBuf};

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::workflow::{
    review::{Decision, Role, Verdict},
    runtime::Identity,
    state::{CheckReceipt, ReviewReceipt},
    workspace::Snapshot,
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AssignmentRequest {
    pub connection: String,
    pub objective: String,
    #[serde(default)]
    pub context: String,
    pub owned_paths: Vec<String>,
}

impl AssignmentRequest {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.connection.is_empty() && self.connection.len() <= 128,
            "assignment requires a configured connection name of 1 to 128 bytes"
        );
        ensure!(
            !self.objective.trim().is_empty() && self.objective.len() <= 65536,
            "assignment objective must contain 1 to 65536 bytes"
        );
        ensure!(
            self.context.len() <= 65536,
            "assignment context exceeds 64 KiB"
        );
        ensure!(
            !self.owned_paths.is_empty() && self.owned_paths.len() <= 128,
            "assignment requires 1 to 128 owned paths"
        );
        for path in &self.owned_paths {
            validate_owned_path(path)?;
        }
        Ok(())
    }

    pub fn owns(&self, path: &str) -> bool {
        valid_content_path(path)
            && self
                .owned_paths
                .iter()
                .any(|owned| owned == "." || Path::new(path).starts_with(owned))
    }
}

pub fn valid_content_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 4096
        && !path.contains(['\0', '\n', '\r'])
        && Path::new(path)
            .components()
            .all(|part| matches!(part, Component::Normal(name) if name != ".git"))
}

fn validate_owned_path(path: &str) -> Result<()> {
    ensure!(
        path == "." || valid_content_path(path),
        "owned paths must stay inside the worktree and exclude .git"
    );
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    Queued,
    Preparing,
    Running,
    Stopped,
    Failed,
    Cancelled,
    Uncertain,
    Validating,
    Ready,
    Integrating,
    Integrated,
}

impl AgentStatus {
    pub fn active(self) -> bool {
        matches!(
            self,
            Self::Preparing | Self::Running | Self::Validating | Self::Integrating
        )
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorktreeIdentity {
    pub root: PathBuf,
    pub git_dir: PathBuf,
    pub common_dir: PathBuf,
    pub git_device: u64,
    pub git_inode: u64,
    pub common_device: u64,
    pub common_inode: u64,
    pub repository_head: String,
    pub baseline_commit: String,
    pub parent_baseline: Snapshot,
    pub child_baseline: Snapshot,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentRecord {
    pub id: u64,
    pub parent_task: Option<u64>,
    #[serde(default)]
    pub origin: AssignmentOrigin,
    #[serde(default)]
    pub completed: bool,
    pub request: AssignmentRequest,
    pub identity: Identity,
    pub worktree: Option<WorktreeIdentity>,
    #[serde(default)]
    pub planned_root: Option<PathBuf>,
    pub status: AgentStatus,
    pub outcome: String,
    pub commands: Vec<String>,
    pub reviewer: Option<Identity>,
    pub checks: Vec<CheckReceipt>,
    pub review: Option<ReviewReceipt>,
    pub validation_generation: u64,
    #[serde(default)]
    pub validation_snapshot: Option<String>,
    pub activity: Vec<Value>,
    pub checkpoint: Option<Value>,
    pub checkpoint_cursor: u64,
    pub integration: Option<Value>,
    pub decisions: Vec<String>,
    #[serde(default)]
    pub orchestration: Option<OrchestrationState>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AssignmentOrigin {
    Developer,
    #[default]
    ParentAgent,
}

impl AgentRecord {
    pub fn orchestration_allows_integration(&self) -> bool {
        self.orchestration
            .as_ref()
            .is_none_or(|state| state.stage == OrchestrationStage::Ready)
    }

    pub fn can_integrate(&self, digest: &str) -> bool {
        self.status == AgentStatus::Ready
            && self.completed
            && self.worktree.is_some()
            && self.reviewer.is_some()
            && !self.commands.is_empty()
            && self.commands.len() == self.checks.len()
            && self
                .checks
                .iter()
                .zip(&self.commands)
                .all(|(receipt, command)| {
                    receipt.success && receipt.command == *command && receipt.snapshot == digest
                })
            && self.review.as_ref().is_some_and(|review| {
                review.clear
                    && review.findings.is_empty()
                    && review.snapshot == digest
                    && review.verification_generation == self.validation_generation
            })
            && self.orchestration_allows_integration()
    }

    pub fn retain_activity(&mut self, event: Value) -> Result<()> {
        ensure!(self.activity.len() < 2048, "agent activity history is full");
        let current = serde_json::to_vec(&self.activity)?.len();
        ensure!(
            current + serde_json::to_vec(&event)?.len() <= 2 * 1024 * 1024,
            "agent activity exceeds 2 MiB; preserve the record before starting new work"
        );
        self.activity.push(event);
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DelegationIdentity {
    pub connections: std::collections::BTreeMap<String, Identity>,
    pub reviewer: Option<Identity>,
    pub max_active: u32,
    pub backend_limit: u64,
    #[serde(default)]
    pub orchestration: Option<OrchestrationIdentity>,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OrchestrationIdentity {
    pub judge: Identity,
    pub correction_limit: u32,
    #[serde(default)]
    pub checks: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OrchestrationStage {
    Queued,
    Working,
    Checking,
    Advisor,
    WorkerResponse,
    Judge,
    Correcting,
    Ready,
    Held,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OrchestrationState {
    pub dependencies: Vec<u64>,
    pub correction_rounds: u32,
    pub stage: OrchestrationStage,
    pub receipts: Vec<RoleReceipt>,
    pub reason: String,
}

impl OrchestrationState {
    pub fn new(dependencies: Vec<u64>) -> Self {
        Self {
            dependencies,
            correction_rounds: 0,
            stage: OrchestrationStage::Queued,
            receipts: Vec::new(),
            reason: "Waiting for admission.".into(),
        }
    }

    pub fn admit_correction(&mut self, limit: u32) -> Result<u32> {
        ensure!(
            self.correction_rounds < limit,
            "correction limit exhausted; assignment is held"
        );
        self.correction_rounds += 1;
        Ok(self.correction_rounds)
    }

    pub fn retain_receipt(&mut self, receipt: RoleReceipt) -> Result<()> {
        ensure!(
            self.receipts.len() < 64,
            "supervision receipt history is full; preserve the record before continuing"
        );
        self.receipts.push(receipt);
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RoleReceipt {
    pub role: Role,
    pub correction_round: u32,
    pub connection: Identity,
    pub snapshot: String,
    pub evidence: String,
    pub verdict: Verdict,
    pub findings: Vec<String>,
    pub explanation: String,
}

impl RoleReceipt {
    pub fn new(
        role: Role,
        correction_round: u32,
        connection: Identity,
        snapshot: String,
        evidence: String,
        decision: Decision,
    ) -> Result<Self> {
        ensure!(
            evidence.len() <= 1024 * 1024,
            "supervision evidence exceeds 1 MiB"
        );
        Ok(Self {
            role,
            correction_round,
            connection,
            snapshot,
            evidence,
            verdict: decision.verdict,
            findings: decision.findings,
            explanation: decision.explanation,
        })
    }
}
