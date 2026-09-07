//! Durable admissions and results shared by worker, checks, Oracle and reviewer.
mod delegation;

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
        }
    }
}

impl Identity {
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
    pub complete: bool,
    pub reconciled: bool,
    pub usage_reported: bool,
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
pub struct Record {
    pub workspace: PathBuf,
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
}

struct Runtime {
    store: Store,
    record: Record,
    failed: bool,
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
        Ok(Some(crate::inspection::project(&runtime.record, request)))
    }

    #[cfg(test)]
    pub(crate) fn for_test(directory: &Path, record: Record) -> Result<Self> {
        let mut store = Store::create(directory)?;
        store.write(&serde_json::to_value(&record)?)?;
        Ok(Self(Arc::new(Mutex::new(Runtime {
            store,
            record,
            failed: false,
        }))))
    }

    pub fn open(
        workspace: &Path,
        connection: &Connection,
        resume: Option<&Path>,
    ) -> Result<(Self, bool)> {
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
            ensure!(
                record.workspace == workspace && record.identity == Identity::from(connection),
                "resume requires the original workspace, connection, model and access mode"
            );
            (store, record, true)
        } else {
            let name = format!("{}-{}", super::allocation::now_ms()?, std::process::id());
            let store = Store::create(&sessions.join(name))?;
            let record = Record {
                workspace: workspace.into(),
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
                    .any(|o| !o.complete && !o.reconciled))
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
                .any(|o| !o.complete && !o.reconciled && delegation::agent_id(&o.phase).is_none())
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
                if !operation.complete {
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
                complete: false,
                reconciled: false,
                usage_reported: false,
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
                    call: Some(call.clone()),
                    result: None,
                    complete: false,
                    reconciled: false,
                    usage_reported: false,
                });
                Ok(())
            }),
            Event::ToolFinished { result } => self.update(|r| {
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
                        phase: phase.into(),
                        verification: verification_attribution(r, phase)?,
                        call: Some(ToolCall {
                            id: result.call_id.clone(),
                            name: result.tool.clone(),
                            arguments: Value::Null,
                        }),
                        result: Some(result.clone()),
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
