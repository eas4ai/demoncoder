//! Acceptance belongs to the developer and names executed evidence and exact files.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

use super::workspace::Snapshot;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CheckReceipt {
    pub command: String,
    pub snapshot: String,
    pub success: bool,
    pub output: String,
    pub exit_code: Option<i32>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ReviewReceipt {
    pub evidence: String,
    pub snapshot: String,
    pub verification_generation: u64,
    pub reviewer: String,
    pub findings: Vec<String>,
    pub clear: bool,
    pub explanation: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Task {
    pub id: u64,
    pub objective: String,
    pub commands: Vec<String>,
    pub baseline: Snapshot,
    pub checks: Vec<CheckReceipt>,
    pub check_history: Vec<Vec<CheckReceipt>>,
    pub review: Option<ReviewReceipt>,
    pub review_history: Vec<ReviewReceipt>,
    pub verification_generation: u64,
    pub corrections: u32,
    pub correction_limit: u32,
    pub stopped: bool,
    pub accepted: Option<String>,
}

impl Task {
    pub fn new(
        id: u64,
        objective: String,
        commands: Vec<String>,
        baseline: Snapshot,
        correction_limit: u32,
    ) -> Result<Self> {
        ensure!(
            !objective.trim().is_empty() && objective.len() <= 64 * 1024,
            "task objective must contain 1 to 65536 bytes"
        );
        ensure!(commands.len() <= 16, "a task supports at most 16 checks");
        ensure!(
            commands
                .iter()
                .all(|c| !c.trim().is_empty() && c.len() <= 8192),
            "each check must contain 1 to 8192 bytes"
        );
        ensure!(
            correction_limit <= 20,
            "correction limit must not exceed 20"
        );
        Ok(Self {
            id,
            objective,
            commands,
            baseline,
            checks: Vec::new(),
            check_history: Vec::new(),
            review: None,
            review_history: Vec::new(),
            verification_generation: 0,
            corrections: 0,
            correction_limit,
            stopped: false,
            accepted: None,
        })
    }

    pub fn verified(&self, digest: &str) -> bool {
        !self.commands.is_empty()
            && self.checks.len() == self.commands.len()
            && self
                .checks
                .iter()
                .zip(&self.commands)
                .all(|(check, command)| {
                    check.success && &check.command == command && check.snapshot == digest
                })
    }

    pub fn reviewed(&self, digest: &str) -> bool {
        self.verified(digest)
            && self.review.as_ref().is_some_and(|review| {
                review.clear
                    && review.findings.is_empty()
                    && review.snapshot == digest
                    && review.verification_generation == self.verification_generation
            })
    }

    pub fn accept(&mut self, digest: &str) -> Result<()> {
        ensure!(self.stopped, "work is still running");
        ensure!(
            self.verified(digest),
            "acceptance requires selected checks to pass on the current workspace"
        );
        ensure!(
            self.reviewed(digest),
            "acceptance requires a clear review of the current checks and workspace"
        );
        self.accepted = Some(digest.to_owned());
        Ok(())
    }

    pub fn start_verification(&mut self) -> Result<()> {
        ensure!(
            self.accepted.is_none(),
            "task was accepted; start a new task"
        );
        ensure!(
            !self.commands.is_empty(),
            "no checks selected; task is unverified"
        );
        self.invalidate()?;
        self.verification_generation += 1;
        Ok(())
    }

    pub fn start_work(&mut self, correction: bool) -> Result<()> {
        ensure!(
            self.accepted.is_none(),
            "task was accepted; start a new task"
        );
        if correction {
            ensure!(
                self.corrections < self.correction_limit,
                "correction allowance exhausted; findings remain unresolved"
            );
            self.corrections += 1;
        } else {
            ensure!(
                self.verification_generation == 0
                    && self.review.is_none()
                    && self.checks.is_empty()
                    && self.check_history.is_empty()
                    && self.review_history.is_empty(),
                "use /correct to change work after verification or review"
            );
        }
        self.invalidate()?;
        self.stopped = false;
        Ok(())
    }

    pub fn start_review(&mut self) -> Result<()> {
        ensure!(
            self.accepted.is_none(),
            "task was accepted; start a new task"
        );
        ensure!(self.review_history.len() < 128, "review history is full");
        if let Some(previous) = self.review.take() {
            self.review_history.push(previous);
        }
        Ok(())
    }

    fn invalidate(&mut self) -> Result<()> {
        // Refuse new work before discarding any evidence when retention fills.
        ensure!(
            self.check_history.len() < 128 && self.review_history.len() < 128,
            "task evidence retention is full; preserve this task and start a new one"
        );
        if !self.checks.is_empty() {
            self.check_history.push(std::mem::take(&mut self.checks));
        }
        if let Some(review) = self.review.take() {
            self.review_history.push(review);
        }
        Ok(())
    }

    pub(crate) fn invalidate_for_integration(&mut self) -> Result<()> {
        self.invalidate()?;
        self.accepted = None;
        Ok(())
    }
}
