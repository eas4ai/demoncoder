//! Durable assignment identity and the gate for integrating child work.
use std::path::{Component, Path, PathBuf};

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::workflow::{
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
    pub request: AssignmentRequest,
    pub identity: Identity,
    pub worktree: Option<WorktreeIdentity>,
    pub status: AgentStatus,
    pub outcome: String,
    pub commands: Vec<String>,
    pub reviewer: Option<Identity>,
    pub checks: Vec<CheckReceipt>,
    pub review: Option<ReviewReceipt>,
    pub validation_generation: u64,
    pub activity: Vec<Value>,
    pub checkpoint: Option<Value>,
    pub checkpoint_cursor: u64,
    pub integration: Option<Value>,
    pub decisions: Vec<String>,
}

impl AgentRecord {
    pub fn can_integrate(&self, digest: &str) -> bool {
        self.status == AgentStatus::Ready
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
}
