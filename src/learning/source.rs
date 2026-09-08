//! Resolve citations only from original private session records.
use super::state::{Receipt, Source, Workspace, digest};
use crate::workflow::{
    runtime::Record,
    state::{CheckReceipt, Task},
    store::Store,
};
use anyhow::{Context, Result, bail, ensure};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::{Component, Path, PathBuf},
};

pub fn validate_source(source: &Source) -> Result<()> {
    valid_session(&source.session)?;
    ensure!(
        source.digest.len() == 64 && source.digest.bytes().all(|b| b.is_ascii_hexdigit()),
        "invalid source receipt digest"
    );
    Ok(())
}

pub fn valid_session(name: &str) -> Result<()> {
    ensure!(
        !name.is_empty()
            && name.len() <= 128
            && name.bytes().all(|b| b.is_ascii_digit() || b == b'-')
            && Path::new(name).components().count() == 1
            && matches!(
                Path::new(name).components().next(),
                Some(Component::Normal(_))
            ),
        "source must name a private session identifier, never a path"
    );
    Ok(())
}

pub fn task(record: &Record, id: u64) -> Result<(&Task, bool)> {
    if let Some(task) = record.task.as_ref().filter(|t| t.id == id) {
        return Ok((task, false));
    }
    record
        .archived
        .iter()
        .find(|t| t.task.id == id)
        .map(|t| (&t.task, true))
        .context("source task is missing; restore the original private session evidence")
}

pub fn cite(session: &str, receipt: Receipt, value: &impl serde::Serialize) -> Result<Source> {
    valid_session(session)?;
    Ok(Source {
        session: session.into(),
        receipt,
        digest: digest(value)?,
    })
}

pub fn select(record: &Record, session: &str, selector: &str) -> Result<Source> {
    let parts: Vec<_> = selector.split(':').collect();
    ensure!(parts.len() <= 4, "source selector has too many components");
    let number = |index: usize| -> Result<u64> {
        parts
            .get(index)
            .context("source selector is incomplete")?
            .parse()
            .context("source selector requires numeric identifiers")
    };
    let identifier = number(1)?;
    match parts[0] {
        "task-check" if parts.len() == 4 => {
            let (task, _) = task(record, identifier)?;
            let round = usize::try_from(number(2)?)?;
            let index = usize::try_from(number(3)?)?;
            let checks = if round == task.check_history.len() {
                &task.checks
            } else {
                task.check_history
                    .get(round)
                    .context("source round missing")?
            };
            cite(
                session,
                Receipt::TaskCheck {
                    task: identifier,
                    round,
                    index,
                },
                checks.get(index).context("source check missing")?,
            )
        }
        "task-review" if parts.len() == 2 => {
            let (task, _) = task(record, identifier)?;
            cite(
                session,
                Receipt::TaskReview { task: identifier },
                task.review
                    .as_ref()
                    .context("source task has no current review")?,
            )
        }
        kind => {
            let agent = record
                .agents
                .iter()
                .find(|a| a.id == identifier)
                .context("source agent missing")?;
            match kind {
                "agent-check" if parts.len() == 4 => {
                    let generation = number(2)?;
                    let index = usize::try_from(number(3)?)?;
                    let check = if generation == agent.validation_generation {
                        serde_json::to_value(
                            agent.checks.get(index).context("source check missing")?,
                        )?
                    } else {
                        agent
                            .activity
                            .iter()
                            .find(|e| {
                                e["type"] == "previous_validation"
                                    && e["generation"].as_u64() == Some(generation)
                            })
                            .and_then(|e| e.get("checks"))
                            .and_then(Value::as_array)
                            .and_then(|c| c.get(index))
                            .context("source check missing")?
                            .clone()
                    };
                    cite(
                        session,
                        Receipt::AgentCheck {
                            agent: identifier,
                            generation,
                            index,
                        },
                        &check,
                    )
                }
                "agent-review" if parts.len() == 2 => cite(
                    session,
                    Receipt::AgentReview { agent: identifier },
                    agent
                        .review
                        .as_ref()
                        .context("source agent has no review")?,
                ),
                "agent-role" if parts.len() == 3 => {
                    let index = usize::try_from(number(2)?)?;
                    let role = agent
                        .orchestration
                        .as_ref()
                        .and_then(|o| o.receipts.get(index))
                        .context("source role receipt missing")?;
                    cite(session, Receipt::AgentRole { agent: identifier }, role)
                }
                _ => bail!(
                    "use task-check:ID:ROUND:INDEX, task-review:ID, agent-check:ID:GENERATION:INDEX, agent-review:ID or agent-role:ID:INDEX (zero-based receipt indexes)"
                ),
            }
        }
    }
}

/// The cache bounds both repeated disk access and aggregate source material.
pub struct Resolver {
    sessions: PathBuf,
    workspace: Workspace,
    records: BTreeMap<String, Record>,
    bytes: usize,
}

impl Resolver {
    pub fn new(sessions: PathBuf, workspace: Workspace) -> Self {
        Self {
            sessions,
            workspace,
            records: BTreeMap::new(),
            bytes: 0,
        }
    }

    pub fn record(&mut self, session: &str) -> Result<&Record> {
        valid_session(session)?;
        if !self.records.contains_key(session) {
            ensure!(
                self.records.len() < 8,
                "source retrieval incomplete: at most 8 sessions per operation; inspect fewer items"
            );
            let value = Store::read_snapshot(&self.sessions.join(session)).context(
                "original source unavailable or damaged; restore its private session record",
            )?;
            let bytes = serde_json::to_vec(&value)?.len();
            ensure!(
                self.bytes + bytes <= 64 * 1024 * 1024,
                "source retrieval incomplete: aggregate evidence exceeds 64 MiB"
            );
            let record: Record = serde_json::from_value(value)
                .map_err(|_| anyhow::anyhow!("invalid original session evidence"))?;
            ensure!(
                record.workspace == self.workspace.path,
                "source belongs to a different workspace; no learning claim is authorized"
            );
            // Task baselines retain the root's device/inode. Old ordinary sessions
            // without a task can still cite child evidence whose parent baseline does.
            self.bytes += bytes;
            self.records.insert(session.into(), record);
        }
        Ok(self.records.get(session).expect("source inserted"))
    }

    pub fn resolve(&mut self, source: &Source) -> Result<Value> {
        validate_source(source)?;
        let workspace = self.workspace.clone();
        let record = self.record(&source.session)?;
        let value = match source.receipt {
            Receipt::TaskCheck {
                task: id,
                round,
                index,
            } => {
                let (task, _) = task(record, id)?;
                check_root(&task.baseline, &workspace)?;
                let checks = if round == task.check_history.len() {
                    &task.checks
                } else {
                    task.check_history
                        .get(round)
                        .context("source verification round is missing")?
                };
                serde_json::to_value(checks.get(index).context("source check is missing")?)?
            }
            Receipt::TaskReview { task: id } => {
                let (task, _) = task(record, id)?;
                check_root(&task.baseline, &workspace)?;
                matching(
                    task.review.iter().chain(task.review_history.iter()),
                    &source.digest,
                )?
            }
            Receipt::AgentCheck {
                agent: id,
                generation,
                index,
            } => {
                let agent = agent(record, id, &workspace)?;
                if agent.validation_generation == generation
                    && let Some(check) = agent.checks.get(index)
                    && digest(check)? == source.digest
                {
                    serde_json::to_value(check)?
                } else {
                    retained_agent_check(agent, generation, index, &source.digest)?
                }
            }
            Receipt::AgentReview { agent: id } => {
                let agent = agent(record, id, &workspace)?;
                let current = agent
                    .review
                    .as_ref()
                    .map(serde_json::to_value)
                    .transpose()?;
                matching(
                    current.iter().chain(
                        agent
                            .activity
                            .iter()
                            .filter(|e| e["type"] == "previous_validation")
                            .filter_map(|e| e.get("review")),
                    ),
                    &source.digest,
                )?
            }
            Receipt::AgentRole { agent: id } => {
                let agent = agent(record, id, &workspace)?;
                matching(
                    agent.orchestration.iter().flat_map(|o| &o.receipts),
                    &source.digest,
                )?
            }
        };
        ensure!(
            digest(&value)? == source.digest,
            "original source receipt changed; copied summaries cannot support this claim"
        );
        Ok(value)
    }

    pub fn check(&mut self, source: &Source) -> Result<CheckReceipt> {
        ensure!(
            matches!(
                source.receipt,
                Receipt::TaskCheck { .. } | Receipt::AgentCheck { .. }
            ),
            "citation is not an executed check"
        );
        serde_json::from_value(self.resolve(source)?).context("original check shape is invalid")
    }
}

fn check_root(
    snapshot: &crate::workflow::workspace::Snapshot,
    workspace: &Workspace,
) -> Result<()> {
    ensure!(
        snapshot.root_identity() == (workspace.device, workspace.inode),
        "source workspace directory identity changed; original evidence is outside this workspace"
    );
    Ok(())
}

fn agent<'a>(
    record: &'a Record,
    id: u64,
    workspace: &Workspace,
) -> Result<&'a crate::subagents::state::AgentRecord> {
    let agent = record
        .agents
        .iter()
        .find(|a| a.id == id)
        .context("source agent is missing")?;
    check_root(
        &agent
            .worktree
            .as_ref()
            .context("source agent has no retained workspace")?
            .parent_baseline,
        workspace,
    )?;
    Ok(agent)
}

fn matching<'a, T: serde::Serialize + 'a>(
    items: impl Iterator<Item = &'a T>,
    expected: &str,
) -> Result<Value> {
    for item in items {
        if digest(item)? == expected {
            return Ok(serde_json::to_value(item)?);
        }
    }
    bail!("original receipt is no longer retained; restore its source before using this claim")
}

fn retained_agent_check(
    agent: &crate::subagents::state::AgentRecord,
    generation: u64,
    index: usize,
    expected: &str,
) -> Result<Value> {
    for event in &agent.activity {
        if event["type"] == "previous_validation"
            && event["generation"].as_u64() == Some(generation)
            && let Some(check) = event
                .get("checks")
                .and_then(Value::as_array)
                .and_then(|checks| checks.get(index))
            && digest(check)? == expected
        {
            return Ok(check.clone());
        }
    }
    for receipt in agent.orchestration.iter().flat_map(|o| &o.receipts) {
        let evidence: Value = serde_json::from_str(&receipt.evidence)
            .map_err(|_| anyhow::anyhow!("invalid retained agent evidence"))?;
        if let Some(checks) = evidence.get("checks").and_then(Value::as_array) {
            for check in checks {
                if digest(check)? == expected {
                    return Ok(check.clone());
                }
            }
        }
    }
    bail!("original agent check is no longer retained; a later result cannot replace it")
}

/// All checks are listed for annotations; only failures become detector observations.
pub fn retained_checks(record: &Record, session: &str) -> Result<Vec<(Source, CheckReceipt)>> {
    let mut found = Vec::new();
    for task in record
        .task
        .iter()
        .chain(record.archived.iter().map(|a| &a.task))
    {
        for (round, checks) in task
            .check_history
            .iter()
            .chain(std::iter::once(&task.checks))
            .enumerate()
        {
            for (index, check) in checks.iter().enumerate() {
                ensure!(
                    found.len() < 4096,
                    "discovery incomplete: more than 4096 retained checks; inspect a source directly"
                );
                found.push((
                    cite(
                        session,
                        Receipt::TaskCheck {
                            task: task.id,
                            round,
                            index,
                        },
                        check,
                    )?,
                    check.clone(),
                ));
            }
        }
    }
    for agent in &record.agents {
        for event in &agent.activity {
            if event["type"] != "previous_validation" {
                continue;
            }
            let generation = event["generation"]
                .as_u64()
                .context("retained validation generation missing")?;
            let checks = event["checks"]
                .as_array()
                .context("retained validation checks missing")?;
            for (index, check) in checks.iter().enumerate() {
                ensure!(
                    found.len() < 4096,
                    "discovery incomplete: more than 4096 retained checks"
                );
                let check: CheckReceipt = serde_json::from_value(check.clone())
                    .context("invalid retained agent check")?;
                found.push((
                    cite(
                        session,
                        Receipt::AgentCheck {
                            agent: agent.id,
                            generation,
                            index,
                        },
                        &check,
                    )?,
                    check,
                ));
            }
        }
        for (index, check) in agent.checks.iter().enumerate() {
            ensure!(
                found.len() < 4096,
                "discovery incomplete: more than 4096 retained checks"
            );
            found.push((
                cite(
                    session,
                    Receipt::AgentCheck {
                        agent: agent.id,
                        generation: agent.validation_generation,
                        index,
                    },
                    check,
                )?,
                check.clone(),
            ));
        }
    }
    Ok(found)
}
