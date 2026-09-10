//! Durable admissions and results shared by worker, checks, Oracle and reviewer.
mod delegation;
mod plugin_admission;
mod tool_operations;
pub use tool_operations::HostInvocation;
pub(crate) use tool_operations::ToolAdmission;
pub use tool_operations::ToolReceipt;

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::{
    allocation::{Allocation, Limits},
    state::Task,
    store::{Store, private_directory},
};
use crate::{
    config::Connection,
    events::Event,
    tools::{ToolCall, ToolResult},
};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    adapter: String,
    model: Option<String>,
    endpoint: Option<String>,
    binary: Option<PathBuf>,
    effort: Option<String>,
    max_output_tokens: Option<u32>,
    unrestricted: bool,
    #[serde(default)]
    strict_worktree: bool,
    tools_enabled: bool,
    credential_paths: Vec<PathBuf>,
    oracle: Option<Box<Identity>>,
    #[serde(default)]
    credential_revision: Option<String>,
}

impl From<&Connection> for Identity {
    fn from(c: &Connection) -> Self {
        let mut credential_paths = c.access.credential_paths.clone();
        credential_paths.sort();
        credential_paths.dedup();
        Self {
            adapter: c.adapter.clone(),
            model: c.model.clone(),
            endpoint: c
                .endpoint
                .as_ref()
                .map(|url| format!("{:x}", Sha256::digest(url.as_bytes()))),
            binary: c.binary.clone(),
            effort: c.effort.clone(),
            max_output_tokens: c.max_output_tokens,
            unrestricted: c.access.unrestricted,
            strict_worktree: c.access.strict_worktree,
            tools_enabled: c.access.tools_enabled,
            credential_paths,
            oracle: c
                .access
                .oracle
                .as_ref()
                .map(|oracle| Box::new(Self::from(oracle.as_ref()))),
            credential_revision: match c.adapter.as_str() {
                "openai-api" => c.api_key("OPENAI_API_KEY").ok(),
                "anthropic-api" => c.api_key("ANTHROPIC_API_KEY").ok(),
                _ => None,
            }
            .map(|key| format!("{:x}", Sha256::digest(key.as_bytes()))),
        }
    }
}

impl Identity {
    pub(crate) fn restore_connection<'a>(
        &self,
        candidates: impl Iterator<Item = &'a Connection>,
        access: &crate::tools::AccessPolicy,
    ) -> Result<Connection> {
        for candidate in candidates {
            let mut connection = candidate.clone();
            connection.model = self.model.clone();
            connection.effort = self.effort.clone();
            connection.max_output_tokens = self.max_output_tokens;
            connection.access = access.clone();
            if self.matches(&connection) {
                return Ok(connection);
            }
        }
        anyhow::bail!(
            "original connection credentials or endpoint are unavailable; restore them before resuming this assignment"
        )
    }
    pub(crate) fn matches(&self, connection: &Connection) -> bool {
        let mut current = Self::from(connection);
        // Older records did not retain a credential revision. Keep their previous
        // recovery contract; newly captured identities compare it exactly.
        if self.credential_revision.is_none() {
            current.credential_revision = None;
        }
        if let (Some(old), Some(new)) = (&self.oracle, &mut current.oracle)
            && old.credential_revision.is_none()
        {
            new.credential_revision = None;
        }
        *self == current
    }
    pub(crate) fn display_model(&self) -> &str {
        self.model.as_deref().unwrap_or("backend-default")
    }

    pub(crate) fn display_adapter(&self) -> &str {
        &self.adapter
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationAttribution {
    pub task_id: u64,
    pub generation: u64,
    pub snapshot: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Operation {
    pub id: u64,
    pub phase: String,
    #[serde(default)]
    pub verification: Option<VerificationAttribution>,
    pub call: Option<ToolCall>,
    pub result: Option<ToolResult>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_receipt: Option<ToolReceipt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_invocation: Option<HostInvocation>,
    pub complete: bool,
    pub reconciled: bool,
    pub usage_reported: bool,
    #[serde(default)]
    pub identity: Option<Identity>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Message {
    pub role: String,
    pub text: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArchivedTask {
    pub task: Task,
    pub allocation: Option<Allocation>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextBinding {
    pub identity: Identity,
    pub through_operation: u64,
    pub checkpoint: Option<Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub workspace: PathBuf,
    #[serde(
        default,
        skip_serializing_if = "super::workspace::CaptureScope::is_empty"
    )]
    pub capture_scope: super::workspace::CaptureScope,
    pub identity: Identity,
    pub reviewer_identity: Option<Identity>,
    pub task: Option<Task>,
    pub archived: Vec<ArchivedTask>,
    pub next_task: u64,
    pub allocation: Option<Allocation>,
    pub checkpoint: Option<Value>,
    pub checkpoint_cursor: u64,
    pub operations: Vec<Operation>,
    pub messages: Vec<Message>,
    pub phase: Option<String>,
    pub recovery_pending: bool,
    pub decisions: Vec<String>,
    pub last_snapshot: Option<String>,
    #[serde(default)]
    pub agents: Vec<crate::subagents::state::AgentRecord>,
    #[serde(default)]
    pub backend_invocations: u64,
    #[serde(default)]
    pub delegation: Option<crate::subagents::state::DelegationIdentity>,
    #[serde(default)]
    pub learning_context: Vec<crate::learning::context::ContextReceipt>,
    #[serde(default)]
    pub prior_contexts: Vec<ContextBinding>,
}

struct Runtime {
    store: Store,
    record: Record,
    failed: bool,
    learning_view: Option<Arc<crate::learning::control::View>>,
    mutation_boundaries: std::collections::BTreeMap<(u64, u64), Arc<tokio::sync::Mutex<()>>>,
}

#[derive(Clone)]
pub struct SharedRuntime(Arc<Mutex<Runtime>>);

impl SharedRuntime {
    pub(crate) fn inspection(
        &self,
        request: Option<crate::inspection::Request>,
    ) -> Result<Option<crate::inspection::Snapshot>> {
        let runtime = match self.0.try_lock() {
            Ok(runtime) => runtime,
            Err(std::sync::TryLockError::WouldBlock) => return Ok(None),
            Err(std::sync::TryLockError::Poisoned(_)) => {
                anyhow::bail!("session record lock failed; inspection unavailable")
            }
        };
        ensure!(
            !runtime.failed,
            "session persistence failed; state uncertain until recovery"
        );
        let mut snapshot = crate::inspection::project(&runtime.record, request);
        if let Some(request) = request.filter(|r| r.target == crate::inspection::Target::Learning)
            && let Some(view) = &runtime.learning_view
        {
            snapshot.page = Some(crate::inspection::learning_page(view, request));
        }
        Ok(Some(snapshot))
    }

    pub(crate) fn learning_view(&self, view: crate::learning::control::View) -> Result<()> {
        let mut runtime = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("session record lock failed"))?;
        runtime.learning_view = Some(Arc::new(view));
        Ok(())
    }

    pub(crate) fn clear_learning_view(&self) -> Result<()> {
        let mut runtime = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("session record lock failed"))?;
        runtime.learning_view = None;
        Ok(())
    }

    pub(crate) fn retain_learning_context(
        &self,
        receipt: crate::learning::context::ContextReceipt,
    ) -> Result<()> {
        self.update(|record| {
            ensure!(
                record.learning_context.len() < 128,
                "coding context history is full (128); preserve this session before new work"
            );
            record.learning_context.push(receipt);
            Ok(())
        })
    }

    #[cfg(test)]
    pub(crate) fn for_test(directory: &Path, record: Record) -> Result<Self> {
        let mut store = Store::create(directory)?;
        store.write(&serde_json::to_value(&record)?)?;
        Ok(Self(Arc::new(Mutex::new(Runtime {
            store,
            record,
            failed: false,
            learning_view: None,
            mutation_boundaries: Default::default(),
        }))))
    }

    pub fn open(
        workspace: &Path,
        connection: &Connection,
        resume: Option<&Path>,
    ) -> Result<(Self, bool)> {
        Self::open_with_scope(
            workspace,
            connection,
            resume,
            &super::workspace::CaptureScope::default(),
        )
    }

    pub fn open_with_scope(
        workspace: &Path,
        connection: &Connection,
        resume: Option<&Path>,
        capture_scope: &super::workspace::CaptureScope,
    ) -> Result<(Self, bool)> {
        capture_scope.validate()?;
        let home = PathBuf::from(
            std::env::var_os("HOME")
                .context("HOME is required for private session recovery records")?,
        )
        .canonicalize()
        .context("resolve session home")?;
        let parent = home.join(".demoncoder");
        private_directory(&parent)?;
        let sessions = parent.join("sessions");
        private_directory(&sessions)?;
        let (mut store, mut record, resumed) = if let Some(path) = resume {
            ensure!(
                matches!(connection.adapter.as_str(), "openai-api" | "anthropic-api"),
                "this external backend cannot restore its internal conversation; select a native API session for --resume"
            );
            ensure!(
                path.parent() == Some(sessions.as_path()),
                "resume must name a session directly under the private sessions directory"
            );
            let store = Store::open(path)?;
            let record: Record = serde_json::from_value(store.read()?)
                .map_err(|_| anyhow::anyhow!("invalid session recovery state"))?;
            record.capture_scope.validate()?;
            ensure!(
                record.capture_scope == *capture_scope,
                "resume requires the original source and review scope; restore its --generated-output, --review-context and --review-changes-only options, or start a new session"
            );
            ensure!(
                record.workspace == workspace && record.identity.matches(connection),
                "resume requires the original workspace, connection, model and access mode"
            );
            (store, record, true)
        } else {
            let name = format!("{}-{}", super::allocation::now_ms()?, std::process::id());
            let store = Store::create(&sessions.join(name))?;
            let record = Record {
                workspace: workspace.into(),
                capture_scope: capture_scope.clone(),
                identity: Identity::from(connection),
                reviewer_identity: None,
                task: None,
                archived: Vec::new(),
                next_task: 1,
                allocation: None,
                checkpoint: None,
                checkpoint_cursor: 0,
                operations: Vec::new(),
                messages: Vec::new(),
                phase: None,
                recovery_pending: false,
                decisions: Vec::new(),
                last_snapshot: None,
                agents: Vec::new(),
                backend_invocations: 0,
                delegation: None,
                learning_context: Vec::new(),
                prior_contexts: Vec::new(),
            };
            (store, record, false)
        };
        if resumed {
            for agent in &mut record.agents {
                if agent.status.active() {
                    agent.status = crate::subagents::state::AgentStatus::Uncertain;
                    agent.outcome = "Interrupted child operation; inspect before continuing. No work was replayed.".into();
                    if let Some(state) = &mut agent.orchestration {
                        state.stage = crate::subagents::state::OrchestrationStage::Held;
                        state.reason = agent.outcome.clone();
                    }
                    record.recovery_pending = true;
                }
            }
        }
        if resumed
            && (record.phase.is_some()
                || record
                    .operations
                    .iter()
                    .any(Operation::needs_reconciliation))
        {
            record.recovery_pending = true;
            if let Some(allocation) = &mut record.allocation {
                allocation.usage.uncertain();
            }
        }
        if let Some(allocation) = &mut record.allocation {
            allocation.checkpoint_time();
        }
        store.write(&serde_json::to_value(&record)?)?;
        Ok((
            Self(Arc::new(Mutex::new(Runtime {
                store,
                record,
                failed: false,
                learning_view: None,
                mutation_boundaries: Default::default(),
            }))),
            resumed,
        ))
    }

    pub(crate) fn update<T>(&self, f: impl FnOnce(&mut Record) -> Result<T>) -> Result<T> {
        let mut runtime = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("session record lock failed"))?;
        ensure!(
            !runtime.failed,
            "session persistence failed; execution is held until recovery"
        );
        let result = f(&mut runtime.record)?;
        if let Some(allocation) = &mut runtime.record.allocation {
            allocation.checkpoint_time();
        }
        let payload = serde_json::to_value(&runtime.record)?;
        if let Err(error) = runtime.store.write(&payload) {
            runtime.failed = true;
            return Err(error).context("persist session transition; execution is held");
        }
        Ok(result)
    }

    pub fn record(&self) -> Result<Record> {
        Ok(self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("session record lock failed"))?
            .record
            .clone())
    }

    pub(crate) fn bind_creator(
        &self,
        connection: &Connection,
        checkpoint: Option<Value>,
    ) -> Result<()> {
        self.update(|record| {
            ensure!(
                !record.recovery_pending && record.phase.is_none(),
                "reconcile interrupted work before changing its model"
            );
            ensure!(
                record.task.as_ref().is_none_or(|t| t.accepted.is_some()),
                "an admitted task keeps its original model"
            );
            ensure!(
                record.prior_contexts.len() < 64,
                "model change history is full; start a new session"
            );
            record.prior_contexts.push(ContextBinding {
                identity: record.identity.clone(),
                through_operation: record.operations.len() as u64,
                checkpoint: record.checkpoint.clone(),
            });
            if let Some(task) = &mut record.task
                && task.creator_identity.is_none()
            {
                task.creator_identity = Some(record.identity.clone());
            }
            record.identity = Identity::from(connection);
            record.checkpoint = checkpoint;
            record.checkpoint_cursor = record.operations.len() as u64;
            Ok(())
        })
    }

    fn admission<T>(&self, f: impl FnOnce(&mut Record) -> Result<T>) -> Result<T> {
        let result = self.update(f)?;
        // The durable clock checkpoint can detect rollback after the closure's
        // initial check. Keep that admission recorded but withhold execution.
        ensure!(
            !self.remaining()?.is_zero(),
            "cumulative task deadline exhausted"
        );
        Ok(result)
    }

    pub fn directory(&self) -> Result<PathBuf> {
        Ok(self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("session record lock failed"))?
            .store
            .directory()
            .to_owned())
    }

    pub fn save_task(
        &self,
        task: &Option<Task>,
        next_task: u64,
        snapshot: Option<&str>,
    ) -> Result<()> {
        self.update(|r| {
            r.task = task.clone();
            r.next_task = next_task;
            if let Some(digest) = snapshot {
                r.last_snapshot = Some(digest.into());
            }
            Ok(())
        })
    }

    pub fn allocate(&self, limits: Limits, reviewer: Option<&Connection>) -> Result<()> {
        self.update(|r| {
            ensure_children_settled(r)?;
            r.allocation = Some(Allocation::new(limits)?);
            r.reviewer_identity = reviewer.map(Identity::from);
            Ok(())
        })
    }

    pub fn archive(&self) -> Result<()> {
        self.update(|r| {
            ensure_children_settled(r)?;
            ensure!(
                r.archived.len() < 32,
                "session task history is full; start a new session"
            );
            if let Some(task) = r.task.take() {
                r.archived.push(ArchivedTask {
                    task,
                    allocation: r.allocation.clone(),
                });
            }
            if r.delegation.is_none() {
                r.allocation = None;
            }
            Ok(())
        })
    }

    pub fn begin_phase(&self, phase: &str, prompt: Option<&str>) -> Result<()> {
        self.update(|r| {
            ensure!(!r.recovery_pending, "interrupted or changed work needs /reconcile with an inspection explanation before continuing");
            r.phase = Some(phase.into());
            if let Some(prompt) = prompt { append_message(r, "developer", prompt)?; }
            Ok(())
        })
    }

    pub fn finish_phase(&self) -> Result<()> {
        self.update(|r| {
            r.phase = None;
            // Cancellation does not establish a remote request's outcome or
            // billing. The developer reconciles every incomplete admission.
            if r.operations
                .iter()
                .any(|o| o.needs_reconciliation() && delegation::agent_id(&o.phase).is_none())
            {
                r.recovery_pending = true;
                if let Some(a) = &mut r.allocation {
                    a.usage.uncertain();
                }
            }
            Ok(())
        })
    }

    pub fn hold(&self) -> Result<()> {
        self.update(|r| {
            r.recovery_pending = true;
            Ok(())
        })
    }

    pub fn reconcile(&self, explanation: &str, digest: Option<&str>) -> Result<()> {
        ensure!(
            !explanation.trim().is_empty() && explanation.len() <= 4096,
            "reconciliation needs an inspection explanation of 1 to 4096 bytes"
        );
        self.update(|r| {
            ensure!(
                !r.agents.iter().any(|a| a.status.active()),
                "stop active agents before reconciling the parent session"
            );
            ensure!(r.decisions.len() < 128, "decision history is full");
            r.decisions.push(format!(
                "Inspected workspace {} ({}): {explanation}",
                r.workspace.display(),
                digest.unwrap_or("ordinary conversation; no acceptance snapshot")
            ));
            for operation in &mut r.operations {
                if operation.needs_reconciliation() {
                    operation.reconciled = true;
                }
            }
            r.phase = None;
            r.recovery_pending = false;
            r.last_snapshot = digest.map(str::to_owned);
            Ok(())
        })
    }

    pub fn remaining(&self) -> Result<Duration> {
        let runtime = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("session record lock failed"))?;
        ensure!(
            !runtime.failed,
            "session persistence failed; execution is held"
        );
        Ok(Duration::from_millis(match &runtime.record.allocation {
            Some(a) => a.remaining_ms()?,
            None => 86400 * 1000,
        }))
    }

    pub fn begin_model(&self, phase: &str) -> Result<u64> {
        self.begin_model_as(phase, None)
    }

    pub(crate) fn begin_model_as(&self, phase: &str, identity: Option<&Identity>) -> Result<u64> {
        self.admission(|r| {
            delegation::ensure_agent_active(r, phase)?;
            ensure!(
                !r.recovery_pending,
                "uncertain work needs reconciliation before model admission"
            );
            ensure!(
                r.operations.len() < 4096,
                "session operation history is full"
            );
            if let Some(a) = &mut r.allocation {
                a.admit(true)?;
            }
            let id = r.operations.len() as u64 + 1;
            r.operations.push(Operation {
                id,
                phase: phase.into(),
                verification: None,
                call: None,
                result: None,
                tool_receipt: None,
                host_invocation: Some(HostInvocation::Model),
                complete: false,
                reconciled: false,
                usage_reported: false,
                identity: identity.cloned().or_else(|| Some(r.identity.clone())),
            });
            Ok(id)
        })
    }

    pub fn finish_model(&self, id: u64) -> Result<()> {
        self.update(|r| {
            let operation = r
                .operations
                .iter_mut()
                .find(|o| o.id == id)
                .context("model admission was not recorded")?;
            operation.complete = true;
            if !operation.usage_reported
                && let Some(a) = &mut r.allocation
            {
                a.usage.uncertain();
            }
            Ok(())
        })
    }

    pub fn checkpoint(&self, state: Value) -> Result<()> {
        self.update(|r| {
            r.checkpoint = Some(state);
            r.checkpoint_cursor = r.operations.len() as u64;
            Ok(())
        })
    }

    pub fn observe(&self, event: &Event, phase: &str) -> Result<()> {
        match event {
            Event::ToolReview { .. } => self.update(|r| {
                ensure!(r.decisions.len() < 128, "session decision history is full");
                r.decisions.push(serde_json::to_string(event)?);
                Ok(())
            }),
            Event::ToolStarted { call } => self.admission(|r| {
                if let Some(operation) = r.operations.iter().rev().find(|operation| {
                    operation.phase == phase
                        && operation
                            .call
                            .as_ref()
                            .is_some_and(|saved| saved.id == call.id)
                }) {
                    ensure!(
                        operation.call.as_ref() == Some(call),
                        "tool notice changed an existing request"
                    );
                    return Ok(());
                }
                delegation::ensure_agent_active(r, phase)?;
                ensure!(
                    !r.recovery_pending,
                    "uncertain work needs reconciliation before tool admission"
                );
                ensure!(
                    r.operations.len() < 4096,
                    "session operation history is full"
                );
                if let Some(a) = &mut r.allocation {
                    a.admit(false)?;
                }
                let id = r.operations.len() as u64 + 1;
                r.operations.push(Operation {
                    id,
                    phase: phase.into(),
                    verification: verification_attribution(r, phase)?,
                    identity: None,
                    call: Some(call.clone()),
                    result: None,
                    tool_receipt: None,
                    host_invocation: None,
                    complete: false,
                    reconciled: false,
                    usage_reported: false,
                });
                Ok(())
            }),
            Event::ToolFinished { result } => self.update(|r| {
                if let Some(operation) = r.operations.iter().rev().find(|operation| {
                    operation.phase == phase
                        && operation
                            .call
                            .as_ref()
                            .is_some_and(|call| call.id == result.call_id)
                }) && (operation.complete || operation.tool_receipt.is_some())
                {
                    ensure!(
                        operation.result.as_ref() == Some(result),
                        "late tool event cannot replace the original receipt"
                    );
                    return Ok(());
                }
                if let Some(operation) = r.operations.iter_mut().rev().find(|o| {
                    !o.complete
                        && o.phase == phase
                        && o.call.as_ref().is_some_and(|c| c.id == result.call_id)
                }) {
                    operation.result = Some(result.clone());
                    operation.complete = true;
                } else {
                    ensure!(
                        !result.success,
                        "successful tool result has no durable admission"
                    );
                    ensure!(
                        r.operations.len() < 4096,
                        "session operation history is full"
                    );
                    r.operations.push(Operation {
                        id: r.operations.len() as u64 + 1,
                        identity: None,
                        phase: phase.into(),
                        verification: verification_attribution(r, phase)?,
                        call: Some(ToolCall {
                            id: result.call_id.clone(),
                            name: result.tool.clone(),
                            arguments: Value::Null,
                        }),
                        result: Some(result.clone()),
                        tool_receipt: None,
                        host_invocation: None,
                        complete: true,
                        reconciled: false,
                        usage_reported: false,
                    });
                }
                Ok(())
            }),
            Event::Usage {
                input,
                output,
                cached,
                cost_usd,
            } => self.update(|r| {
                if let Some(a) = &mut r.allocation {
                    a.usage.add(*input, *output, *cached, *cost_usd)?;
                }
                if let Some(operation) = r
                    .operations
                    .iter_mut()
                    .rev()
                    .find(|o| !o.complete && o.call.is_none() && o.phase == phase)
                {
                    operation.usage_reported = true;
                }
                Ok(())
            }),
            Event::Text { text } if phase == "worker" => {
                // Streaming fragments are memory-only until the next durable transition.
                // A crash still retains the admission and the last completed model checkpoint.
                let mut runtime = self
                    .0
                    .lock()
                    .map_err(|_| anyhow::anyhow!("session record lock failed"))?;
                ensure!(!runtime.failed, "session persistence failed");
                append_message(&mut runtime.record, "assistant", text)
            }
            _ => Ok(()),
        }
    }
}

fn verification_attribution(
    record: &Record,
    phase: &str,
) -> Result<Option<VerificationAttribution>> {
    if phase != "verification" {
        return Ok(None);
    }
    let task = record
        .task
        .as_ref()
        .context("verification requires an active task")?;
    Ok(Some(VerificationAttribution {
        task_id: task.id,
        generation: task.verification_generation,
        snapshot: record
            .last_snapshot
            .clone()
            .context("verification requires a captured snapshot")?,
    }))
}

fn append_message(record: &mut Record, role: &str, text: &str) -> Result<()> {
    let bytes: usize = record.messages.iter().map(|m| m.text.len()).sum();
    ensure!(
        bytes.saturating_add(text.len()) <= 8 * 1024 * 1024 && record.messages.len() < 4096,
        "session transcript retention is full; preserve this session and start a new one"
    );
    if role == "assistant"
        && let Some(previous) = record.messages.last_mut().filter(|m| m.role == role)
    {
        previous.text.push_str(text);
        return Ok(());
    }
    record.messages.push(Message {
        role: role.into(),
        text: text.into(),
    });
    Ok(())
}

fn ensure_children_settled(record: &Record) -> Result<()> {
    use crate::subagents::state::AgentStatus;
    ensure!(
        record.agents.iter().all(|agent| matches!(
            agent.status,
            AgentStatus::Integrated | AgentStatus::Cancelled | AgentStatus::Failed
        )),
        "integrate or cancel outstanding agents before replacing the task allocation"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inspection_never_waits_for_a_writer_or_claims_failed_persistence_is_current() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("record");
        let record = crate::inspection::tests::record(root.path());
        let runtime = SharedRuntime::for_test(&directory, record).unwrap();
        let mut writer = runtime.0.lock().unwrap();
        let started = std::time::Instant::now();
        assert!(runtime.inspection(None).unwrap().is_none());
        assert!(started.elapsed() < Duration::from_millis(50));
        writer.failed = true;
        drop(writer);
        assert!(
            runtime
                .inspection(None)
                .unwrap_err()
                .to_string()
                .contains("persistence failed")
        );
    }

    #[tokio::test]
    async fn inspection_recovers_authoritative_counts_when_live_notices_are_dropped() {
        let root = tempfile::tempdir().unwrap();
        let record = crate::inspection::tests::record(root.path());
        let runtime = SharedRuntime::for_test(&root.path().join("record"), record).unwrap();
        let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
        let events = crate::events::EventSink::new("fixture".into(), sender, None)
            .unwrap()
            .with_runtime(runtime.clone());
        events
            .emit(Event::TurnFinished { status: "complete" })
            .await
            .unwrap();
        runtime
            .update(|record| {
                record.agents.push(crate::inspection::tests::agent(
                    1,
                    crate::subagents::state::AgentStatus::Running,
                    &record.identity,
                ));
                Ok(())
            })
            .unwrap();
        events
            .emit_advisory(Event::AgentAllocation {
                active: 1,
                active_limit: 2,
                backend_invocations: 0,
                backend_limit: 8,
            })
            .unwrap();
        assert!(matches!(
            receiver.try_recv().unwrap().event,
            Event::TurnFinished { .. }
        ));
        assert!(receiver.try_recv().is_err());
        let snapshot = runtime.inspection(None).unwrap().unwrap();
        assert_eq!(snapshot.summary.active, 1);
    }

    #[test]
    fn clock_failure_during_admission_is_persisted_without_granting_execution() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("record");
        let connection: Connection =
            serde_json::from_value(serde_json::json!({"adapter":"openai-api"})).unwrap();
        let record: Record = serde_json::from_value(serde_json::json!({
            "workspace":root.path(), "identity":Identity::from(&connection),
            "archived":[], "next_task":1, "checkpoint_cursor":0, "operations":[],
            "messages":[], "recovery_pending":false, "decisions":[],
            "allocation":Allocation::new(Limits::default()).unwrap()
        }))
        .unwrap();
        let runtime = SharedRuntime(Arc::new(Mutex::new(Runtime {
            store: Store::create(&directory).unwrap(),
            record,
            failed: false,
            learning_view: None,
            mutation_boundaries: Default::default(),
        })));
        let result = runtime.admission(|record| {
            let allocation = record.allocation.as_mut().unwrap();
            allocation.admit(true)?;
            // Simulate a wall rollback between the initial admission and its
            // durable time checkpoint, without changing the machine clock.
            allocation.observed_ms += 60_000;
            Ok(())
        });
        assert!(result.is_err());
        assert!(runtime.record().unwrap().allocation.unwrap().clock_invalid);
        // An already completed result can still be saved despite the clock hold.
        runtime
            .update(|record| append_message(record, "assistant", "retained completion"))
            .unwrap();
        drop(runtime);
        let saved = Store::open(&directory).unwrap().read().unwrap();
        assert_eq!(saved["allocation"]["clock_invalid"], true);
        assert_eq!(saved["allocation"]["model_calls"], 1);
        assert_eq!(saved["messages"][0]["text"], "retained completion");
    }

    #[test]
    fn recovery_identity_freezes_authority_without_retaining_credentials() {
        let mut connection: Connection = serde_json::from_value(serde_json::json!({
            "adapter": "openai-api", "model": "model-a", "api_key": "fixture-secret",
            "endpoint": "http://127.0.0.1/responses?token=fixture-secret"
        }))
        .unwrap();
        connection.access.credential_paths = vec![PathBuf::from("/private/settings")];
        let original = Identity::from(&connection);
        assert!(
            !serde_json::to_string(&original)
                .unwrap()
                .contains("fixture-secret")
        );
        connection.access.tools_enabled = !connection.access.tools_enabled;
        assert_ne!(original, Identity::from(&connection));
        connection.access.tools_enabled = !connection.access.tools_enabled;
        connection.access.credential_paths.clear();
        assert_ne!(original, Identity::from(&connection));
        connection.access.credential_paths = vec![PathBuf::from("/private/settings")];
        let mut oracle = connection.clone();
        oracle.model = Some("oracle-a".into());
        connection.access.oracle = Some(Box::new(oracle));
        let with_oracle = Identity::from(&connection);
        assert_ne!(original, with_oracle);
        connection.access.oracle.as_mut().unwrap().model = Some("oracle-b".into());
        assert_ne!(with_oracle, Identity::from(&connection));
    }
}
