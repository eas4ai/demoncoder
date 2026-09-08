//! Bounded catalog schema. Original source receipts remain authoritative.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

pub const MAX_CATALOG_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_OBSERVATIONS: usize = 128;
pub const MAX_CANDIDATES: usize = 64;
pub const MAX_LESSONS: usize = 64;
pub const MAX_HISTORY: usize = 128;

pub fn digest(value: &impl Serialize) -> Result<String> {
    // Normalize object ordering identically for typed receipts and decoded JSON.
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&serde_json::to_value(value)?)?)
    ))
}

pub fn text(value: &str, limit: usize, field: &str) -> Result<()> {
    ensure!(
        !value.trim().is_empty() && value.len() <= limit,
        "{field} must contain 1 to {limit} bytes"
    );
    ensure!(
        !value
            .chars()
            .any(|c| c.is_control() && c != '\n' && c != '\t'),
        "{field} contains unsupported control characters"
    );
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Workspace {
    pub path: PathBuf,
    pub device: u64,
    pub inode: u64,
}

impl Workspace {
    pub fn read(path: &Path) -> Result<Self> {
        let canonical = path.canonicalize().context("resolve learning workspace")?;
        ensure!(canonical == path, "learning workspace must be canonical");
        let meta = std::fs::symlink_metadata(path).context("inspect learning workspace")?;
        ensure!(meta.is_dir(), "learning workspace must be a directory");
        Ok(Self {
            path: canonical,
            device: meta.dev(),
            inode: meta.ino(),
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Receipt {
    TaskCheck {
        task: u64,
        round: usize,
        index: usize,
    },
    TaskReview {
        task: u64,
    },
    AgentCheck {
        agent: u64,
        generation: u64,
        index: usize,
    },
    AgentReview {
        agent: u64,
    },
    AgentRole {
        agent: u64,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub session: String,
    pub receipt: Receipt,
    pub digest: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Annotation {
    pub author: String,
    pub text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub id: u64,
    pub source: Source,
    pub failed_check: bool,
    pub snapshot: String,
    pub annotations: Vec<Annotation>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Proposal {
    pub objective: String,
    pub scope: String,
    pub benefit: String,
    pub behavioral_check: String,
    pub risks: String,
}

impl Proposal {
    pub fn validate(&self) -> Result<()> {
        for (field, value) in [
            ("objective", &self.objective),
            ("scope", &self.scope),
            ("benefit", &self.benefit),
            ("risks", &self.risks),
        ] {
            text(value, 4096, field)?;
        }
        text(&self.behavioral_check, 8192, "behavioral_check")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Authorization {
    pub session: String,
    pub task: u64,
    pub at_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeStatus {
    Supported,
    Unresolved,
    Insufficient,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Outcome {
    pub status: OutcomeStatus,
    pub reason: String,
    pub generation: u64,
    pub snapshot: String,
    pub checks: Vec<Source>,
    pub review: Option<Source>,
    pub abandoned: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Candidate {
    pub id: u64,
    pub observation: u64,
    pub proposal: Proposal,
    pub author: String,
    pub authorization: Option<Authorization>,
    pub outcomes: Vec<Outcome>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LessonProposal {
    pub claim: String,
    pub keywords: Vec<String>,
}

impl LessonProposal {
    pub fn validate(&self) -> Result<()> {
        text(&self.claim, 4096, "lesson claim")?;
        ensure!(
            !self.keywords.is_empty() && self.keywords.len() <= 8,
            "lesson applicability requires 1 to 8 keywords"
        );
        for keyword in &self.keywords {
            ensure!(
                !keyword.is_empty()
                    && keyword.len() <= 128
                    && keyword.chars().all(char::is_alphanumeric),
                "lesson keywords must be individual alphanumeric words of at most 128 bytes"
            );
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lesson {
    pub id: u64,
    pub candidate: u64,
    pub outcome: usize,
    pub proposal: LessonProposal,
    pub enabled: bool,
    pub superseded_by: Option<u64>,
    pub history: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub version: u32,
    pub workspace: Workspace,
    pub observations: Vec<Observation>,
    pub candidates: Vec<Candidate>,
    pub lessons: Vec<Lesson>,
}

impl Catalog {
    pub fn new(workspace: Workspace) -> Self {
        Self {
            version: 1,
            workspace,
            observations: vec![],
            candidates: vec![],
            lessons: vec![],
        }
    }

    pub fn validate(&self, workspace: &Workspace) -> Result<()> {
        ensure!(
            self.version == 1 && &self.workspace == workspace,
            "learning catalog version or workspace identity differs; preserve it for inspection"
        );
        ensure!(
            self.observations.len() <= MAX_OBSERVATIONS
                && self.candidates.len() <= MAX_CANDIDATES
                && self.lessons.len() <= MAX_LESSONS,
            "learning catalog retention limit exceeded"
        );
        for (index, observation) in self.observations.iter().enumerate() {
            ensure!(
                observation.id == index as u64 + 1 && observation.annotations.len() <= MAX_HISTORY,
                "invalid observation identity or annotation retention"
            );
            super::source::validate_source(&observation.source)?;
            for annotation in &observation.annotations {
                ensure!(
                    annotation.author == "developer",
                    "invalid annotation author"
                );
                text(&annotation.text, 4096, "annotation")?;
            }
        }
        for (index, candidate) in self.candidates.iter().enumerate() {
            ensure!(
                candidate.id == index as u64 + 1 && candidate.outcomes.len() <= MAX_HISTORY,
                "invalid candidate identity or outcome retention"
            );
            self.observation(candidate.observation)?;
            candidate.proposal.validate()?;
        }
        for (index, lesson) in self.lessons.iter().enumerate() {
            ensure!(
                lesson.id == index as u64 + 1 && lesson.history.len() <= MAX_HISTORY,
                "invalid lesson identity or history retention"
            );
            let candidate = self.candidate(lesson.candidate)?;
            ensure!(
                lesson.outcome < candidate.outcomes.len(),
                "lesson outcome is missing"
            );
            lesson.proposal.validate()?;
        }
        ensure!(
            serde_json::to_vec(self)?.len() <= MAX_CATALOG_BYTES,
            "learning catalog exceeds 8 MiB; preserve evidence before further work"
        );
        Ok(())
    }

    pub fn observation(&self, id: u64) -> Result<&Observation> {
        self.observations
            .iter()
            .find(|o| o.id == id)
            .context("observation not retained; use /improvements")
    }
    pub fn candidate(&self, id: u64) -> Result<&Candidate> {
        self.candidates
            .iter()
            .find(|c| c.id == id)
            .context("candidate not retained; use /improvements")
    }
    pub fn candidate_mut(&mut self, id: u64) -> Result<&mut Candidate> {
        self.candidates
            .iter_mut()
            .find(|c| c.id == id)
            .context("candidate not retained; use /improvements")
    }
    pub fn lesson(&self, id: u64) -> Result<&Lesson> {
        self.lessons
            .iter()
            .find(|l| l.id == id)
            .context("lesson not retained; use /improvements")
    }
    pub fn lesson_mut(&mut self, id: u64) -> Result<&mut Lesson> {
        self.lessons
            .iter_mut()
            .find(|l| l.id == id)
            .context("lesson not retained; use /improvements")
    }
}
