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
    prompt_origin: Option<PromptOrigin>,
    connection: String,
    sender: mpsc::Sender<Envelope>,
    log: Option<Arc<Mutex<File>>>,
    runtime: Option<crate::workflow::runtime::SharedRuntime>,
    observer_runtime: Option<crate::workflow::runtime::RuntimeReference>,
    phase: String,
    identity: Option<crate::workflow::runtime::Identity>,
    invocation: Option<u64>,
    tool_operation: Option<u64>,
    hook_model: Option<crate::workflow::runtime::plugin_admission::ModelAdmission>,
    plugin_event: crate::plugins::hook_types::HookEvent,
    tool_representation: crate::plugins::receipts::ToolRepresentation,
}

#[derive(Clone)]
enum PromptOrigin {
    Developer(String),
    PluginContext,
}

/// A session retains cancellation authority without keeping its runtime alive.
#[derive(Default)]
pub(crate) struct ObserverOwner {
    owner: Option<(crate::workflow::runtime::RuntimeReference, String)>,
}
impl ObserverOwner {
    pub(crate) fn capture(&mut self, events: &EventSink) {
        self.owner = events
            .runtime
            .as_ref()
            .map(|runtime| (runtime.downgrade(), events.phase.clone()));
    }
    pub(crate) async fn stop(&self) -> Result<()> {
        if let Some((owner, phase)) = &self.owner
            && let Ok(runtime) = owner.upgrade()
        {
            runtime.stop_observers(Some(phase), false).await?;
        }
        Ok(())
    }
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
            prompt_origin: None,
            connection,
            sender,
            log,
            runtime: None,
            observer_runtime: None,
            phase: "worker".into(),
            identity: None,
            invocation: None,
            tool_operation: None,
            hook_model: None,
            plugin_event: crate::plugins::hook_types::HookEvent::PreToolUse,
            tool_representation: Default::default(),
        })
    }

    pub(crate) fn with_submitted_prompt(&self, text: String) -> Self {
        Self {
            prompt_origin: Some(PromptOrigin::Developer(text)),
            ..self.clone()
        }
    }
    pub(crate) fn with_plugin_prompt(&self) -> Self {
        Self {
            prompt_origin: Some(PromptOrigin::PluginContext),
            ..self.clone()
        }
    }
    pub(crate) fn is_plugin_prompt(&self) -> bool {
        matches!(self.prompt_origin, Some(PromptOrigin::PluginContext))
    }
    pub(crate) fn submitted_prompt<'a>(&'a self, fallback: &'a str) -> &'a str {
        match &self.prompt_origin {
            Some(PromptOrigin::Developer(text)) => text,
            _ => fallback,
        }
    }
    pub(crate) fn observer_context(
        &self,
    ) -> Result<Option<crate::workflow::runtime::plugin_observer::ContextDelivery>> {
        self.runtime
            .as_ref()
            .map(|r| {
                let ordinary =
                    r.reserve_observer_context(&self.phase, self.identity.as_ref(), false)?;
                if ordinary.is_some() {
                    Ok(ordinary)
                } else {
                    r.reserve_observer_context(&self.phase, self.identity.as_ref(), true)
                }
            })
            .transpose()
            .map(Option::flatten)
    }
    pub(crate) fn complete_observer_context(
        &self,
        delivery: &crate::workflow::runtime::plugin_observer::ContextDelivery,
    ) -> Result<()> {
        self.runtime
            .as_ref()
            .context("observer delivery runtime missing")?
            .complete_observer_context(delivery)
    }
    pub(crate) fn for_observer(&self) -> Self {
        Self {
            runtime: None,
            observer_runtime: self
                .runtime
                .as_ref()
                .map(|r| r.downgrade())
                .or_else(|| self.observer_runtime.clone()),
            ..self.clone()
        }
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
            prompt_origin: None,
            connection: phase.into(),
            sender,
            log: None,
            runtime: self.runtime.clone(),
            observer_runtime: self.observer_runtime.clone(),
            identity: None,
            invocation: None,
            tool_operation: None,
            phase: if self.phase.starts_with("agent:") {
                format!("{}:{phase}", self.phase)
            } else {
                phase.into()
            },
            hook_model: self.hook_model.clone(),
            plugin_event: self.plugin_event,
            tool_representation: self.tool_representation.clone(),
        }
    }

    pub(crate) fn begin_model(&self) -> Result<Option<u64>> {
        self.validate_hook_delivery()?;
        self.runtime
            .as_ref()
            .map(|r| {
                r.begin_model_owned(
                    &self.phase,
                    self.identity.as_ref(),
                    self.hook_model.as_ref(),
                )
            })
            .transpose()
    }

    pub(crate) fn begin_backend(&self) -> Result<Option<u64>> {
        self.validate_hook_delivery()?;
        self.runtime
            .as_ref()
            .map(|runtime| {
                runtime.begin_backend_owned(
                    &self.phase,
                    self.identity.as_ref(),
                    self.hook_model.as_ref(),
                )
            })
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

    pub(crate) fn for_non_tool_context(
        &self,
        operation: u64,
    ) -> Result<(crate::workflow::runtime::SharedRuntime, u64)> {
        let runtime = self
            .runtime
            .as_ref()
            .context("lifecycle requires a durable runtime")?;
        anyhow::ensure!(
            runtime
                .record()?
                .operations
                .iter()
                .any(|o| o.id == operation
                    && o.phase == self.phase
                    && o.non_tool_receipt().is_some()),
            "lifecycle belongs to another phase"
        );
        Ok((runtime.clone(), operation))
    }
    pub(crate) fn for_non_tool(
        &self,
        occurrence: crate::plugins::receipts::NonToolOccurrence,
        plan: String,
        declarations: Vec<serde_json::Value>,
    ) -> Result<(Self, crate::plugins::receipts::NonToolFacts)> {
        anyhow::ensure!(
            self.hook_model.is_none(),
            "recursive or non-worker lifecycle dispatch is unavailable"
        );
        let runtime = self
            .runtime
            .as_ref()
            .context("lifecycle requires a durable runtime")?;
        let facts = runtime.begin_non_tool_as(
            &self.phase,
            self.identity.as_ref(),
            occurrence,
            plan,
            declarations,
        )?;
        Ok((
            Self {
                tool_operation: Some(facts.operation),
                plugin_event: facts.subject.occurrence.event(),
                ..self.clone()
            },
            facts,
        ))
    }
    pub(crate) fn plugin_context(&self) -> Result<(crate::workflow::runtime::SharedRuntime, u64)> {
        Ok((
            self.runtime
                .clone()
                .map(Ok)
                .or_else(|| self.observer_runtime.as_ref().map(|r| r.upgrade()))
                .transpose()?
                .context("plugin admission requires a durable runtime")?,
            self.tool_operation
                .context("plugin admission requires a correlated operation")?,
        ))
    }

    pub(crate) fn backend_invocation_id(&self) -> Option<u64> {
        self.invocation
    }

    pub(crate) fn plugin_event(&self) -> crate::plugins::hook_types::HookEvent {
        self.plugin_event
    }
    pub(crate) fn for_plugin_event(&self, event: crate::plugins::hook_types::HookEvent) -> Self {
        Self {
            plugin_event: event,
            ..self.clone()
        }
    }
    pub(crate) fn tool_representation(&self) -> crate::plugins::receipts::ToolRepresentation {
        self.tool_representation.clone()
    }
    pub(crate) fn with_tool_representation(
        &self,
        representation: crate::plugins::receipts::ToolRepresentation,
    ) -> Self {
        Self {
            tool_representation: representation,
            ..self.clone()
        }
    }
    pub(crate) fn ensure_continuation(&self) -> Result<()> {
        if self.hook_model.is_none()
            && let Some(runtime) = &self.runtime
        {
            runtime.ensure_post_continuation(&self.phase)?;
        }
        Ok(())
    }
    pub(crate) fn original_tool_evidence(
        &self,
        call_id: &str,
    ) -> Result<Option<crate::tools::ToolResult>> {
        let (Some(runtime), Some(invocation)) = (&self.runtime, self.invocation) else {
            return Ok(None);
        };
        let record = runtime.record()?;
        let mut matches = record.operations.iter().filter(|o| {
            o.tool_receipt
                .as_ref()
                .is_some_and(|r| r.invocation == invocation && r.original_call.id == call_id)
        });
        let operation = matches.next().context("completed tool evidence missing")?;
        anyhow::ensure!(
            matches.next().is_none(),
            "completed tool evidence correlation ambiguous"
        );
        Ok(Some(
            operation
                .result
                .clone()
                .context("original tool evidence missing")?,
        ))
    }
    pub(crate) fn post_delivery_context(
        &self,
        call_id: &str,
    ) -> Result<Option<(crate::workflow::runtime::SharedRuntime, u64)>> {
        let Some(runtime) = &self.runtime else {
            return Ok(None);
        };
        let Some(invocation) = self.invocation else {
            return Ok(None);
        };
        Ok(runtime
            .post_delivery_operation(invocation, call_id)?
            .map(|id| (runtime.clone(), id)))
    }
    pub(crate) fn complete_local_post_release(&self, call_id: &str) -> Result<()> {
        if let Some((runtime, id)) = self.post_delivery_context(call_id)? {
            runtime.complete_local_post_release(id)?;
        }
        Ok(())
    }
    pub(crate) fn reserve_post_correction(&self, call_id: &str) -> Result<u64> {
        let (runtime, id) = self
            .post_delivery_context(call_id)?
            .context("post-tool correction lacks a durable owner")?;
        runtime.reserve_post_correction(id, &self.phase, self.identity.as_ref())
    }
    pub(crate) fn reserve_post_delivery(&self, call_id: &str) -> Result<()> {
        if let Some((runtime, id)) = self.post_delivery_context(call_id)? {
            runtime.reserve_post_delivery(id)?;
        }
        Ok(())
    }
    pub(crate) fn ack_post_delivery(&self, call_id: &str) -> Result<()> {
        if let Some((runtime, id)) = self.post_delivery_context(call_id)? {
            runtime.ack_post_delivery(id)?;
        }
        Ok(())
    }
    pub(crate) fn post_continuation(
        &self,
        call_id: &str,
    ) -> Result<crate::plugins::receipts::PostContinuation> {
        let Some(runtime) = &self.runtime else {
            return Ok(Default::default());
        };
        let record = runtime.record()?;
        Ok(record
            .operations
            .iter()
            .find(|o| {
                o.tool_receipt.as_ref().is_some_and(|r| {
                    Some(r.invocation) == self.invocation && r.original_call.id == call_id
                })
            })
            .and_then(|o| o.tool_receipt.as_ref())
            .and_then(|r| r.plugin_lifecycle.as_ref())
            .map(|p| p.continuation.clone())
            .unwrap_or_default())
    }

    pub(crate) fn for_hook_model(
        &self,
        invocation: u32,
        maximum: u32,
        snapshot: Arc<crate::plugins::runners::SnapshotInspection>,
        cancelled: Arc<std::sync::atomic::AtomicBool>,
        sender: mpsc::Sender<Envelope>,
    ) -> Result<Self> {
        let (runtime, owner) = self.plugin_context()?;
        runtime.plugin_runner_owner(owner, self.plugin_event)?;
        let mut sink = self.child(&format!("hook:{owner}:{invocation}"), sender);
        sink.hook_model = Some(crate::workflow::runtime::plugin_admission::ModelAdmission {
            key: runtime.plugin_hook_key(owner, self.plugin_event, invocation)?,
            owner,
            event: self.plugin_event,
            invocation,
            maximum,
            snapshot,
            cancelled,
        });
        Ok(sink)
    }

    pub(crate) fn validate_hook_delivery(&self) -> Result<()> {
        self.ensure_continuation()?;
        if let Some(hook) = &self.hook_model {
            self.validate_model_owner()?;
            hook.snapshot.validate_delivery()?;
        }
        Ok(())
    }

    pub(crate) fn validate_hook_request(&self, value: &serde_json::Value) -> Result<()> {
        if let Some(hook) = &self.hook_model {
            self.validate_model_owner()?;
            hook.snapshot.validate_request(value)?;
        }
        Ok(())
    }

    pub(crate) fn validate_model_owner(&self) -> Result<()> {
        let hook = self
            .hook_model
            .as_ref()
            .context("model hook authority missing")?;
        anyhow::ensure!(
            !hook.cancelled.load(std::sync::atomic::Ordering::Acquire),
            "model hook cancelled"
        );
        let runtime = self
            .runtime
            .as_ref()
            .context("model hook runtime missing")?;
        runtime.plugin_runner_owner(hook.owner, hook.event)?;
        anyhow::ensure!(
            !runtime.remaining()?.is_zero(),
            "model hook owner deadline expired"
        );
        Ok(())
    }

    pub(crate) fn settle_hook_models(&self) -> Result<()> {
        if self.hook_model.is_some() {
            self.runtime
                .as_ref()
                .context("hook runtime missing")?
                .settle_hook_models(&self.phase)?;
        }
        Ok(())
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

    pub(crate) fn refuse_tool_execution(&self) -> Result<()> {
        if let (Some(runtime), Some(id)) = (&self.runtime, self.tool_operation) {
            runtime.refuse_tool_execution(id)?;
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
        // Hook contexts are fresh isolated assignments. In particular an
        // agent:<id>:hook phase must never overwrite that child's checkpoint.
        if self.hook_model.is_some() {
            return Ok(());
        }
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

    /// Retain original completion before post hooks, but deliver it only after
    /// the caller has settled model feedback and observer-free completion.
    pub(crate) fn retain_tool_completion(
        &self,
        result: crate::tools::ToolResult,
    ) -> Result<impl std::future::Future<Output = Result<()>> + '_> {
        let envelope = Envelope {
            connection: self.connection.clone(),
            event: Event::ToolFinished { result },
        };
        self.retain(&envelope)?;
        Ok(async move {
            self.sender
                .send(envelope)
                .await
                .context("terminal event receiver closed")
        })
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
