use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::Path,
    sync::{Arc, Mutex},
};

use anyhow::{Context, Result};
use serde::Serialize;
use tokio::sync::mpsc;

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct ContextUsage {
    pub used: Option<u64>,
    pub capacity: Option<u64>,
    pub estimated: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    ModelAssignment {
        connection: String,
        model: Option<String>,
        explanation: String,
    },
    AgentAllocation {
        active: usize,
        active_limit: u32,
        backend_invocations: u64,
        backend_limit: u64,
    },
    AgentState {
        id: u64,
        connection: String,
        worktree: Option<String>,
        status: crate::subagents::state::AgentStatus,
        objective: String,
        outcome: String,
        stage: Option<crate::subagents::state::OrchestrationStage>,
        correction_rounds: Option<u32>,
        reason: Option<String>,
    },
    AgentActivity {
        id: u64,
        connection: String,
        event: serde_json::Value,
    },
    RetainedTool {
        result: crate::tools::ToolResult,
    },
    SessionRecord {
        path: String,
        resumed: bool,
    },
    RetainedMessage {
        role: String,
        text: String,
    },
    TaskAllocation {
        remaining_seconds: u64,
        model_calls: u64,
        model_limit: u64,
        tool_calls: u64,
        tool_limit: u64,
        usage: crate::workflow::allocation::Usage,
    },
    ReviewUsage {
        reviewer: String,
        input: Option<u64>,
        output: Option<u64>,
        cached: Option<u64>,
        cost_usd: Option<f64>,
    },
    TaskState {
        task_id: u64,
        stopped: bool,
        verification: String,
        review: String,
        accepted: bool,
    },
    Ready {
        owner: &'static str,
    },
    TurnStarted,
    Context {
        usage: ContextUsage,
    },
    Text {
        text: String,
    },
    ToolStarted {
        call: crate::tools::ToolCall,
    },
    ToolOutput {
        call_id: String,
        stream: &'static str,
        text: String,
    },
    ToolFinished {
        result: crate::tools::ToolResult,
    },
    ToolPresentation {
        call_id: String,
        text: String,
    },
    ToolReview {
        call_id: String,
        reviewer: String,
        decision: &'static str,
        reason: String,
    },
    OracleUsage {
        reviewer: String,
        input: Option<u64>,
        output: Option<u64>,
        cached: Option<u64>,
        cost_usd: Option<f64>,
    },
    Usage {
        input: Option<u64>,
        output: Option<u64>,
        cached: Option<u64>,
        cost_usd: Option<f64>,
    },
    TurnFinished {
        status: &'static str,
    },
    Error {
        message: String,
    },
}

#[derive(Clone, Debug, Serialize)]
pub struct Envelope {
    pub connection: String,
    pub event: Event,
}

/// One ordered publication path for both retained events and the UI.
#[derive(Clone)]
pub struct EventSink {
    connection: String,
    sender: mpsc::Sender<Envelope>,
    log: Option<Arc<Mutex<File>>>,
    runtime: Option<crate::workflow::runtime::SharedRuntime>,
    phase: String,
    identity: Option<crate::workflow::runtime::Identity>,
    invocation: Option<u64>,
    tool_operation: Option<u64>,
}

impl EventSink {
    pub fn new(
        connection: String,
        sender: mpsc::Sender<Envelope>,
        path: Option<&Path>,
    ) -> Result<Self> {
        let log = path
            .map(|path| {
                let mut options = OpenOptions::new();
                options.write(true).create_new(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.mode(0o600);
                }
                options
                    .open(path)
                    .context("create a new event log")
                    .map(|file| Arc::new(Mutex::new(file)))
            })
            .transpose()?;
        Ok(Self {
            connection,
            sender,
            log,
            runtime: None,
            phase: "worker".into(),
            identity: None,
            invocation: None,
            tool_operation: None,
        })
    }

    pub fn with_runtime(mut self, runtime: crate::workflow::runtime::SharedRuntime) -> Self {
        self.runtime = Some(runtime);
        self
    }

    pub(crate) fn for_connection(&self, connection: &str) -> Self {
        Self {
            connection: connection.into(),
            invocation: None,
            tool_operation: None,
            ..self.clone()
        }
    }

    pub(crate) fn with_identity(&self, connection: &crate::config::Connection) -> Self {
        Self {
            identity: Some(crate::workflow::runtime::Identity::from(connection)),
            ..self.clone()
        }
    }

    pub(crate) fn for_phase(&self, phase: &str) -> Self {
        Self {
            phase: phase.into(),
            invocation: None,
            tool_operation: None,
            ..self.clone()
        }
    }

    pub(crate) fn child(&self, phase: &str, sender: mpsc::Sender<Envelope>) -> Self {
        Self {
            connection: phase.into(),
            sender,
            log: None,
            runtime: self.runtime.clone(),
            identity: None,
            invocation: None,
            tool_operation: None,
            phase: if self.phase.starts_with("agent:") {
                format!("{}:{phase}", self.phase)
            } else {
                phase.into()
            },
        }
    }

    pub(crate) fn begin_model(&self) -> Result<Option<u64>> {
        self.runtime
            .as_ref()
            .map(|r| r.begin_model_as(&self.phase, self.identity.as_ref()))
            .transpose()
    }

    pub(crate) fn begin_backend(&self) -> Result<Option<u64>> {
        self.runtime
            .as_ref()
            .map(|runtime| runtime.begin_backend_as(&self.phase, self.identity.as_ref()))
            .transpose()
    }

    pub(crate) fn for_invocation(&self, invocation: Option<u64>) -> Self {
        Self {
            invocation,
            tool_operation: None,
            ..self.clone()
        }
    }

    pub(crate) fn for_commands(&self) -> Result<Self> {
        let invocation = self
            .runtime
            .as_ref()
            .map(|runtime| runtime.begin_commands(&self.phase, self.identity.as_ref()))
            .transpose()?;
        Ok(self.for_invocation(invocation))
    }

    pub(crate) fn begin_tool(
        &self,
        call: &crate::tools::ToolCall,
    ) -> Result<(Self, Option<crate::tools::ToolResult>)> {
        let Some(runtime) = &self.runtime else {
            return Ok((self.clone(), None));
        };
        let invocation = self
            .invocation
            .context("durable tool execution requires a host invocation")?;
        match runtime.begin_tool(&self.phase, invocation, call)? {
            crate::workflow::runtime::ToolAdmission::Fresh(id) => Ok((
                Self {
                    tool_operation: Some(id),
                    ..self.clone()
                },
                None,
            )),
            crate::workflow::runtime::ToolAdmission::Replay(result) => {
                Ok((self.clone(), Some(result)))
            }
            crate::workflow::runtime::ToolAdmission::Denied(id, result) => {
                let scoped = Self {
                    tool_operation: Some(id),
                    ..self.clone()
                };
                scoped.emit_advisory(Event::ToolFinished {
                    result: result.clone(),
                })?;
                Ok((scoped, Some(result)))
            }
            crate::workflow::runtime::ToolAdmission::Held(reason) => anyhow::bail!(reason),
        }
    }

    pub(crate) fn plugin_context(&self) -> Result<(crate::workflow::runtime::SharedRuntime, u64)> {
        Ok((
            self.runtime
                .clone()
                .context("plugin admission requires a durable runtime")?,
            self.tool_operation
                .context("plugin admission requires a correlated tool operation")?,
        ))
    }

    pub(crate) fn mutation_boundary(
        &self,
        identity: (u64, u64),
    ) -> Result<Option<Arc<tokio::sync::Mutex<()>>>> {
        self.runtime
            .as_ref()
            .map(|r| r.mutation_boundary(identity))
            .transpose()
    }

    pub(crate) fn admit_tool(&self, call: &crate::tools::ToolCall) -> Result<()> {
        if let (Some(runtime), Some(id)) = (&self.runtime, self.tool_operation) {
            runtime.admit_tool(id, call)?;
        }
        Ok(())
    }

    pub(crate) fn tool_effect(&self) -> Result<()> {
        if let (Some(runtime), Some(id)) = (&self.runtime, self.tool_operation) {
            runtime.tool_effect(id)?;
        }
        Ok(())
    }

    pub(crate) fn original_tool_result(&self, result: &crate::tools::ToolResult) -> Result<()> {
        if let (Some(runtime), Some(id)) = (&self.runtime, self.tool_operation) {
            runtime.original_tool_result(id, result)?;
        }
        Ok(())
    }

    pub(crate) fn model_tool_result(&self, result: &crate::tools::ToolResult) -> Result<()> {
        if let (Some(runtime), Some(id)) = (&self.runtime, self.tool_operation) {
            runtime.model_tool_result(id, result)?;
        }
        Ok(())
    }

    pub(crate) fn tool_observer(
        &self,
        index: usize,
        outcome: Option<Result<&str, &str>>,
    ) -> Result<()> {
        if let (Some(runtime), Some(id)) = (&self.runtime, self.tool_operation) {
            runtime.tool_observer(id, index, outcome)?;
        }
        Ok(())
    }

    pub(crate) fn settle_tool(&self) -> Result<()> {
        if let (Some(runtime), Some(id)) = (&self.runtime, self.tool_operation) {
            runtime.settle_tool(id)?;
        }
        Ok(())
    }

    pub(crate) fn finish_model(&self, id: Option<u64>) -> Result<()> {
        if let (Some(runtime), Some(id)) = (&self.runtime, id) {
            runtime.finish_model(id)?;
        }
        Ok(())
    }

    pub(crate) fn checkpoint(&self, state: Option<serde_json::Value>) -> Result<()> {
        if let (Some(runtime), Some(state)) = (&self.runtime, state) {
            if self.phase == "worker" {
                runtime.checkpoint(state)?;
            } else {
                runtime.agent_checkpoint(&self.phase, state)?;
            }
        }
        Ok(())
    }

    pub async fn emit(&self, event: Event) -> Result<()> {
        let envelope = Envelope {
            connection: self.connection.clone(),
            event,
        };
        self.retain(&envelope)?;
        self.sender
            .send(envelope)
            .await
            .context("terminal event receiver closed")
    }

    /// Retain a control acknowledgement without letting UI backpressure delay
    /// cancellation. Advisory events may be omitted from the live copy when the
    /// terminal queue is full; the ordered event log remains authoritative.
    pub(crate) fn emit_advisory(&self, event: Event) -> Result<()> {
        let envelope = Envelope {
            connection: self.connection.clone(),
            event,
        };
        self.retain(&envelope)?;
        match self.sender.try_send(envelope) {
            Ok(()) | Err(mpsc::error::TrySendError::Full(_)) => Ok(()),
            Err(mpsc::error::TrySendError::Closed(_)) => {
                anyhow::bail!("terminal event receiver closed")
            }
        }
    }

    fn retain(&self, envelope: &Envelope) -> Result<()> {
        if let Some(runtime) = &self.runtime {
            // Scoped tools publish state synchronously at the effect boundary.
            // Live delivery and duplicate notices cannot allocate or replace it.
            if self.tool_operation.is_none()
                || !matches!(
                    envelope.event,
                    Event::ToolStarted { .. }
                        | Event::ToolFinished { .. }
                        | Event::ToolPresentation { .. }
                )
            {
                runtime.observe(&envelope.event, &self.phase)?;
            }
        }
        if let Some(log) = &self.log {
            let mut log = log
                .lock()
                .map_err(|_| anyhow::anyhow!("event log lock failed"))?;
            serde_json::to_writer(&mut *log, &envelope).context("serialize session event")?;
            log.write_all(b"\n").context("append session event")?;
            log.flush().context("flush session event")?;
        }
        Ok(())
    }
}
