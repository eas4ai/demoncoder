//! One small model/tool loop shared by direct API providers.
use crate::{
    events::{Event, EventSink},
    session::{CORRECTION_CAPACITY, CORRECTION_REJECTION, Command, Session, TurnEnd},
    tools::{ToolCall, ToolExecutor, ToolResult},
};
use anyhow::Result;
use async_trait::async_trait;
use std::{collections::VecDeque, path::Path};
use tokio::sync::mpsc;

/// Explicit origin marker for errors returned by a provider transport or protocol boundary.
/// Adapters must leave local configuration, event delivery and persistence errors unmarked.
#[derive(Debug)]
pub struct ProviderResponseFailure(anyhow::Error);
impl std::fmt::Display for ProviderResponseFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.0, f)
    }
}
impl std::error::Error for ProviderResponseFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.0.as_ref())
    }
}
pub fn provider_response_failure(error: impl Into<anyhow::Error>) -> anyhow::Error {
    anyhow::Error::new(ProviderResponseFailure(error.into()))
}
/// Add adapter guidance inside the origin marker, retaining the original cause.
pub(crate) fn provider_failure_context(error: anyhow::Error, guidance: String) -> anyhow::Error {
    match error.downcast::<ProviderResponseFailure>() {
        Ok(failure) => provider_response_failure(failure.0.context(guidance)),
        Err(error) => error.context(guidance),
    }
}

#[async_trait]
pub trait Model: Send {
    fn checkpoint(&self) -> Option<serde_json::Value> {
        None
    }
    fn restore(&mut self, _checkpoint: &serde_json::Value) -> Result<()> {
        anyhow::bail!("model does not support conversation recovery")
    }
    fn prompt(&mut self, text: String);
    fn results(&mut self, results: Vec<ToolResult>);
    async fn response(&mut self, events: &EventSink) -> Result<Vec<ToolCall>>;
}

pub struct NativeSession {
    observer_owner: crate::events::ObserverOwner,
    model: Box<dyn Model>,
    tools: ToolExecutor,
    pending: VecDeque<ToolCall>,
}

impl NativeSession {
    pub fn new(model: Box<dyn Model>, workspace: &Path) -> Result<Self> {
        Ok(Self::with_tools(model, ToolExecutor::new(workspace)?))
    }

    pub fn with_tools(model: Box<dyn Model>, tools: ToolExecutor) -> Self {
        Self {
            observer_owner: Default::default(),
            model,
            tools,
            pending: VecDeque::new(),
        }
    }
}

#[async_trait]
impl Session for NativeSession {
    fn owner(&self) -> &'static str {
        "demoncoder"
    }

    fn supports_workflow(&self) -> bool {
        self.model.checkpoint().is_some()
    }

    fn checkpoint(&self) -> Option<serde_json::Value> {
        self.model
            .checkpoint()
            .map(|model| serde_json::json!({ "model":model, "pending":self.pending }))
    }
    fn settle_interruption(&mut self) -> Result<()> {
        if let Some(result) = self.tools.take_completed()
            && self.pending.front().is_some_and(|c| c.id == result.call_id)
        {
            self.model.results(vec![result]);
            self.pending.pop_front();
        }
        if !self.pending.is_empty() {
            self.model.results(self.pending.drain(..).map(|call| ToolResult { call_id:call.id, tool:call.name, success:false, output:"Execution stopped before this tool had a known result. Inspect partial effects before retrying.".into(), exit_code:None }).collect());
        }
        Ok(())
    }

    fn restore(&mut self, checkpoint: &serde_json::Value, results: &[ToolResult]) -> Result<()> {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Saved {
            model: serde_json::Value,
            pending: Vec<ToolCall>,
        }
        let saved: Saved = serde_json::from_value(checkpoint.clone())
            .map_err(|_| anyhow::anyhow!("invalid native conversation checkpoint"))?;
        self.model.restore(&saved.model)?;
        if !saved.pending.is_empty() {
            self.model.results(saved.pending.into_iter().map(|call| {
            results.iter().rev().find(|r| r.call_id == call.id && r.tool == call.name).cloned().unwrap_or(ToolResult {
                call_id: call.id, tool: call.name, success: false,
                output: "Session was interrupted before a durable result was recorded for this call. It was not replayed; inspect possible effects before continuing.".into(), exit_code: None,
            })
        }).collect());
        }
        self.pending.clear();
        Ok(())
    }

    async fn turn(
        &mut self,
        prompt: String,
        commands: &mut mpsc::Receiver<Command>,
        events: &EventSink,
    ) -> Result<TurnEnd> {
        self.observer_owner.capture(events);
        let mut provider_failed = false;
        let turn = events.begin_native_turn();
        let failure_events = turn.as_ref().unwrap_or(events).clone();
        let outcome = match turn {
            Ok(turn_events) => {
                let outcome = self
                    .run_turn(prompt, commands, &turn_events, &mut provider_failed)
                    .await;
                use crate::plugins::receipts::NativeTurnEnd;
                let end = match &outcome {
                    Ok(TurnEnd::Complete) => NativeTurnEnd::Complete,
                    Ok(TurnEnd::Cancelled) => NativeTurnEnd::Cancelled,
                    Ok(TurnEnd::Shutdown) => NativeTurnEnd::Shutdown,
                    Err(_) => NativeTurnEnd::Failed,
                };
                let finished = turn_events.finish_native_turn(end);
                preserve_provider_failure(finished, provider_failed, &turn_events).and(outcome)
            }
            Err(error) => Err(error),
        };
        preserve_provider_failure(self.settle_interruption(), provider_failed, &failure_events)?;
        if !matches!(outcome, Ok(TurnEnd::Complete)) {
            preserve_provider_failure(self.close().await, provider_failed, &failure_events)?;
        }
        preserve_provider_failure(
            events.checkpoint(self.checkpoint()),
            provider_failed,
            &failure_events,
        )?;
        outcome
    }

    async fn cancel_background(&mut self) -> Result<()> {
        self.observer_owner.stop().await
    }

    async fn close(&mut self) -> Result<()> {
        let observers = self.observer_owner.stop().await;
        let services = self.tools.stop_language_services().await;
        observers.and(services)
    }
}

impl NativeSession {
    async fn observe_provider_failure(
        &mut self,
        error: &anyhow::Error,
        commands: &mut mpsc::Receiver<Command>,
        events: &EventSink,
        response_events: &EventSink,
    ) -> Result<()> {
        use crate::plugins::{hook_types::HookEvent, receipts::NonToolOccurrence};
        if !self.tools.has_non_tool_plan(HookEvent::StopFailure) {
            return Ok(());
        }
        let occurrence = NonToolOccurrence::StopFailure {
            error: "unknown".into(),
            error_details: format!("{error:#}"),
            last_assistant_message: response_events.assistant_text()?,
        };
        // Failure is already decided. Controls may cancel observation, but neither
        // plugin output nor newly submitted text can start a correction or retry.
        while let Ok(command) = commands.try_recv() {
            failure_control(Some(command), commands)?;
        }
        let dispatch = self.tools.dispatch_non_tool(occurrence, events);
        tokio::pin!(dispatch);
        loop {
            tokio::select! {
                biased;
                command = commands.recv() => failure_control(command, commands)?,
                result = &mut dispatch => return result.map(|_| ()),
            }
        }
    }
    async fn lifecycle(
        &mut self,
        occurrence: crate::plugins::receipts::NonToolOccurrence,
        commands: &mut mpsc::Receiver<Command>,
        corrections: &mut Vec<String>,
        events: &EventSink,
    ) -> Result<std::result::Result<Option<crate::plugins::non_tool::NonToolOutcome>, TurnEnd>>
    {
        // Controls queued before entry win even if the hook future is immediately ready.
        while let Ok(command) = commands.try_recv() {
            if let Some(end) = control(Some(command), corrections, events)? {
                return Ok(Err(end));
            }
        }
        let dispatch = self.tools.dispatch_non_tool(occurrence, events);
        tokio::pin!(dispatch);
        loop {
            tokio::select! {
                biased;
                command = commands.recv() => if let Some(end) = control(command, corrections, events)? { return Ok(Err(end)); },
                result = &mut dispatch => return result.map(Ok),
            }
        }
    }
    async fn submit_prompt(
        &mut self,
        prompt: String,
        correction: bool,
        commands: &mut mpsc::Receiver<Command>,
        corrections: &mut Vec<String>,
        events: &EventSink,
    ) -> Result<Option<TurnEnd>> {
        let submitted = if correction {
            prompt.clone()
        } else {
            events.submitted_prompt(&prompt).to_owned()
        };
        let outcome = match self
            .lifecycle(
                crate::plugins::receipts::NonToolOccurrence::UserPromptSubmit {
                    prompt: submitted,
                    correction,
                },
                commands,
                corrections,
                events,
            )
            .await?
        {
            Ok(outcome) => outcome,
            Err(end) => return Ok(Some(end)),
        };
        if let Some(reason) = outcome.as_ref().and_then(|o| o.hold.as_ref()) {
            anyhow::bail!("UserPromptSubmit blocked: {reason}");
        }
        self.tools.set_intent(&prompt);
        self.model.prompt(prompt);
        if let Some(outcome) = outcome
            && !outcome.context.is_empty()
        {
            self.model.prompt(outcome.context);
        }
        Ok(None)
    }
    async fn run_turn(
        &mut self,
        prompt: String,
        commands: &mut mpsc::Receiver<Command>,
        events: &EventSink,
        provider_failed: &mut bool,
    ) -> Result<TurnEnd> {
        let mut corrections = Vec::new();
        if events.is_plugin_prompt() {
            self.model.prompt(prompt);
        } else if let Some(end) = self
            .submit_prompt(prompt, false, commands, &mut corrections, events)
            .await?
        {
            return Ok(end);
        }
        let mut stop_hook_active = false;
        events.checkpoint(self.checkpoint())?;
        loop {
            while let Ok(command) = commands.try_recv() {
                if let Some(end) = control(Some(command), &mut corrections, events)? {
                    return Ok(end);
                }
            }
            while !corrections.is_empty() {
                for correction in std::mem::take(&mut corrections) {
                    if let Some(end) = self
                        .submit_prompt(correction, true, commands, &mut corrections, events)
                        .await?
                    {
                        return Ok(end);
                    }
                    stop_hook_active = false;
                }
            }
            if let Some(delivery) = events.observer_context()? {
                self.model.prompt(delivery.text.clone());
                events.checkpoint(self.checkpoint())?;
                events.complete_observer_context(&delivery)?;
            }
            let admission = events.begin_model()?;
            let invocation_events = events.for_invocation(admission);
            let response_events = if self
                .tools
                .has_non_tool_plan(crate::plugins::hook_types::HookEvent::Stop)
                || self
                    .tools
                    .has_non_tool_plan(crate::plugins::hook_types::HookEvent::StopFailure)
            {
                invocation_events.capture_assistant_text()
            } else {
                invocation_events.clone()
            };
            let calls = {
                let response = self.model.response(&response_events);
                tokio::pin!(response);
                loop {
                    tokio::select! {
                        biased;
                        command = commands.recv() => if let Some(end) = control(command, &mut corrections, events)? { return Ok(end); },
                        result = &mut response => break result,
                    }
                }
            };
            let settled = events.finish_model(admission);
            // A returned provider error has no pending native tool effects. Keep
            // the error, but settle its admission; cancellation exits above and
            // deliberately leaves the interrupted request uncertain.
            let calls = match calls {
                Ok(calls) => {
                    settled?;
                    calls
                }
                Err(error) => {
                    let error = match error.downcast::<ProviderResponseFailure>() {
                        Ok(failure) => failure.0,
                        Err(error) => return Err(error),
                    };
                    *provider_failed = true;
                    let observation = match settled {
                        Ok(()) => self.observe_provider_failure(&error, commands, events, &response_events).await,
                        Err(error) => Err(error.context("provider admission could not be settled; StopFailure was not dispatched")),
                    };
                    preserve_provider_failure(observation, true, events)?;
                    return Err(error);
                }
            };
            let finished = calls.is_empty();
            anyhow::ensure!(
                finished || self.tools.tools_enabled(),
                "Oracle requested a tool; review refused"
            );
            self.pending = calls.into();
            events.checkpoint(self.checkpoint())?;
            while let Some(call) = self.pending.front().cloned() {
                while let Ok(command) = commands.try_recv() {
                    if let Some(end) = control(Some(command), &mut corrections, events)? {
                        return Ok(end);
                    }
                }
                if !corrections.is_empty() {
                    self.model.results(vec![ToolResult {
                        call_id: call.id,
                        tool: call.name,
                        success: false,
                        output: "Not executed: developer corrected this response.".into(),
                        exit_code: None,
                    }]);
                    self.pending.pop_front();
                    continue;
                }
                let operation = self.tools.execute(call, &invocation_events);
                tokio::pin!(operation);
                let result = loop {
                    tokio::select! {
                        biased;
                        command = commands.recv() => if let Some(end) = control(command, &mut corrections, events)? { return Ok(end); },
                        result = &mut operation => break result?,
                    }
                };
                let release_call_id = result.call_id.clone();
                let post = invocation_events.post_continuation(&result.call_id)?;
                self.model.results(vec![result]);
                self.pending.pop_front();
                self.tools.take_completed();
                events.checkpoint(self.checkpoint())?;
                if !matches!(
                    post,
                    crate::plugins::receipts::PostContinuation::Held { .. }
                ) {
                    let release = self
                        .tools
                        .validate_post_release(&release_call_id, &invocation_events);
                    tokio::pin!(release);
                    loop {
                        tokio::select! {
                            biased;
                            command = commands.recv() => if let Some(end) = control(command, &mut corrections, events)? { return Ok(end); },
                            result = &mut release => { result?; break; },
                        }
                    }
                    // Validation may finish in the same poll that queues a
                    // command. Cancellation still precedes correction charge.
                    while let Ok(command) = commands.try_recv() {
                        if let Some(end) = control(Some(command), &mut corrections, events)? {
                            return Ok(end);
                        }
                    }
                    invocation_events.complete_local_post_release(&release_call_id)?;
                }
                match post {
                    crate::plugins::receipts::PostContinuation::Held { reason } => {
                        anyhow::bail!("post-tool continuation held: {reason}")
                    }
                    crate::plugins::receipts::PostContinuation::Correction => {
                        while let Some(skipped) = self.pending.pop_front() {
                            self.model.results(vec![ToolResult { call_id: skipped.id, tool: skipped.name, success: false,
                                output: "Not executed: plugin-origin correction superseded the remaining response.".into(), exit_code: None }]);
                        }
                        break;
                    }
                    crate::plugins::receipts::PostContinuation::Continue => {}
                }
            }
            let delivered = if let Some(delivery) = events.observer_context()? {
                self.model.prompt(delivery.text.clone());
                events.checkpoint(self.checkpoint())?;
                events.complete_observer_context(&delivery)?;
                true
            } else {
                false
            };
            let corrected = !corrections.is_empty() || delivered;
            while !corrections.is_empty() {
                for correction in std::mem::take(&mut corrections) {
                    if let Some(end) = self
                        .submit_prompt(correction, true, commands, &mut corrections, events)
                        .await?
                    {
                        return Ok(end);
                    }
                    stop_hook_active = false;
                }
            }
            events.checkpoint(self.checkpoint())?;
            if finished && !corrected {
                let outcome = match self
                    .lifecycle(
                        crate::plugins::receipts::NonToolOccurrence::Stop {
                            stop_hook_active,
                            last_assistant_message: response_events.assistant_text()?,
                        },
                        commands,
                        &mut corrections,
                        events,
                    )
                    .await?
                {
                    Ok(outcome) => outcome,
                    Err(end) => return Ok(end),
                };
                // A control arriving with hook completion wins before any correction is charged.
                while let Ok(command) = commands.try_recv() {
                    if let Some(end) = control(Some(command), &mut corrections, events)? {
                        return Ok(end);
                    }
                }
                if let Some(outcome) = outcome {
                    if let Some(reason) = outcome.hold {
                        anyhow::bail!("Stop gate unmet: {reason}");
                    }
                    if outcome.correction {
                        let (runtime, _) = events.for_non_tool_context(outcome.operation)?;
                        runtime.admit_non_tool_correction(outcome.operation)?;
                        // Internal plugin correction is attributed context, not a developer submission or control.
                        self.model.prompt(format!("[Plugin-origin Stop correction] Continue the original task to address the unmet Stop gate.\n{}", outcome.context));
                        stop_hook_active = true;
                        events.checkpoint(self.checkpoint())?;
                        continue;
                    }
                }
                if corrections.is_empty() {
                    return Ok(TurnEnd::Complete);
                }
            }
        }
    }
}

fn preserve_provider_failure(
    result: Result<()>,
    provider_failed: bool,
    events: &EventSink,
) -> Result<()> {
    if !provider_failed {
        return result;
    }
    if let Err(error) = result {
        // Retention failure latches the runtime itself. Never replace the actual
        // provider error with an observer, transport or cleanup error.
        let _ = events.native_failure_diagnostic(format!(
            "StopFailure observation or cleanup incomplete: {error:#}"
        ));
    }
    Ok(())
}

fn failure_control(command: Option<Command>, commands: &mut mpsc::Receiver<Command>) -> Result<()> {
    match command {
        Some(Command::Shutdown) => {
            // Returning the provider error must not swallow the outer session's
            // shutdown signal. Closing this same receiver makes its next recv
            // take the existing command-channel shutdown path.
            commands.close();
            while let Ok(command) = commands.try_recv() {
                if let Command::Submit { reply, .. } = command {
                    let _ = reply.send(Err("Session is shutting down"));
                }
            }
            anyhow::bail!(
                "StopFailure observation stopped for shutdown; original provider failure retained"
            );
        }
        Some(Command::Cancel) | None => {
            anyhow::bail!("StopFailure observation cancelled; original provider failure retained")
        }
        Some(Command::Submit { reply, .. }) => {
            let _ = reply.send(Err(
                "Provider failed; submit a new turn after failure cleanup",
            ));
        }
        Some(Command::Prompt(_)) => anyhow::bail!(
            "StopFailure observation stopped by new input; original provider failure retained"
        ),
    }
    Ok(())
}

fn control(
    command: Option<Command>,
    corrections: &mut Vec<String>,
    events: &EventSink,
) -> Result<Option<TurnEnd>> {
    match command {
        Some(Command::Cancel) => Ok(Some(TurnEnd::Cancelled)),
        Some(Command::Shutdown) | None => Ok(Some(TurnEnd::Shutdown)),
        Some(Command::Prompt(text)) => {
            if crate::workflow::is_control(&text) {
                events.emit_advisory(Event::Error {
                    message: crate::workflow::BUSY_CONTROL.into(),
                })?;
                return Ok(None);
            }
            let admitted = corrections.len() < CORRECTION_CAPACITY;
            if admitted {
                corrections.push(text);
            }
            correction_notice(events, admitted)?;
            Ok(None)
        }
        Some(Command::Submit { text, reply }) => {
            if crate::workflow::is_control(&text) {
                let _ = reply.send(Err(crate::workflow::BUSY_CONTROL));
                return Ok(None);
            }
            if corrections.len() >= CORRECTION_CAPACITY {
                let _ = reply.send(Err(CORRECTION_REJECTION));
                correction_notice(events, false)?;
            } else if reply.send(Ok(())).is_ok() {
                corrections.push(text);
                correction_notice(events, true)?;
            }
            Ok(None)
        }
    }
}

fn correction_notice(events: &EventSink, admitted: bool) -> Result<()> {
    events.emit_advisory(if admitted {
        Event::Text {
            text: "\n[Correction queued for the next tool boundary]\n".into(),
        }
    } else {
        Event::Error {
            message: CORRECTION_REJECTION.into(),
        }
    })
}

#[cfg(test)]
#[path = "native/post_tests.rs"]
mod post_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::ToolHook;
    use serde_json::json;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    struct ScriptedModel {
        responses: VecDeque<Vec<ToolCall>>,
        received: Arc<Mutex<Vec<ToolResult>>>,
    }

    #[async_trait]
    impl Model for ScriptedModel {
        fn prompt(&mut self, _text: String) {}
        fn results(&mut self, results: Vec<ToolResult>) {
            self.received.lock().unwrap().extend(results);
        }
        async fn response(&mut self, _events: &EventSink) -> Result<Vec<ToolCall>> {
            Ok(self.responses.pop_front().unwrap_or_default())
        }
    }

    fn call(id: &str, name: &str, arguments: serde_json::Value) -> ToolCall {
        ToolCall {
            id: id.into(),
            name: name.into(),
            arguments,
        }
    }

    #[tokio::test]
    async fn cancellation_during_result_publication_preserves_completed_receipt() {
        let workspace = tempfile::tempdir().unwrap();
        let log = workspace.path().join("events.jsonl");
        let received = Arc::new(Mutex::new(Vec::new()));
        let model = ScriptedModel {
            responses: [vec![
                call(
                    "completed",
                    "write",
                    json!({"path":"completed.txt","content":"retained"}),
                ),
                call(
                    "unstarted",
                    "write",
                    json!({"path":"unstarted.txt","content":"must not run"}),
                ),
            ]]
            .into(),
            received: received.clone(),
        };
        let mut session = NativeSession::new(Box::new(model), workspace.path()).unwrap();
        // ToolStarted fills the only slot. ToolFinished is logged but waits
        // for UI delivery after the real file operation has completed.
        let (tx, mut rx) = mpsc::channel(1);
        let events = EventSink::new("test".into(), tx, Some(&log)).unwrap();
        let (commands, mut command_rx) = mpsc::channel(4);
        let running = tokio::spawn(async move {
            let result = session
                .turn("create".into(), &mut command_rx, &events)
                .await;
            (result, session)
        });
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if std::fs::read_to_string(&log)
                    .unwrap()
                    .contains("tool_finished")
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        commands.send(Command::Cancel).await.unwrap();
        let (result, mut session) = tokio::time::timeout(Duration::from_secs(2), running)
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(result.unwrap(), TurnEnd::Cancelled));
        let results = received.lock().unwrap().clone();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].call_id, "completed");
        assert!(results[0].success, "{}", results[0].output);
        assert_eq!(results[1].call_id, "unstarted");
        assert!(!results[1].success);
        assert_eq!(
            std::fs::read_to_string(workspace.path().join("completed.txt")).unwrap(),
            "retained"
        );
        assert!(!workspace.path().join("unstarted.txt").exists());
        let records: Vec<serde_json::Value> = std::fs::read_to_string(log)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let actual: Vec<_> = records
            .iter()
            .filter(|row| row["event"]["type"] == "tool_finished")
            .collect();
        assert_eq!(actual.len(), 1);
        assert_eq!(
            actual[0]["event"]["result"],
            serde_json::to_value(&results[0]).unwrap()
        );
        rx.recv().await.unwrap();
        let (tx, _rx) = mpsc::channel(8);
        let events = EventSink::new("next".into(), tx, None).unwrap();
        let (_commands, mut command_rx) = mpsc::channel(4);
        assert!(matches!(
            session
                .turn("continue".into(), &mut command_rx, &events)
                .await
                .unwrap(),
            TurnEnd::Complete
        ));
        assert_eq!(
            received.lock().unwrap().len(),
            2,
            "receipt was delivered twice"
        );
    }

    struct FailedPresentation;

    #[tokio::test]
    async fn cancellation_during_language_feedback_preserves_the_completed_edit() {
        use std::os::unix::fs::PermissionsExt;
        let workspace = tempfile::tempdir().unwrap();
        let binary = workspace.path().join("language-server");
        std::fs::copy(
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/lsp_fixture.py"),
            &binary,
        )
        .unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::write(workspace.path().join(".fixture-mode"), "pending").unwrap();
        let received = Arc::new(Mutex::new(Vec::new()));
        let model = ScriptedModel {
            responses: [vec![
                call(
                    "completed",
                    "write",
                    json!({"path":"main.rs","content":"fn main() {}"}),
                ),
                call(
                    "unstarted",
                    "write",
                    json!({"path":"unstarted.rs","content":"must not run"}),
                ),
            ]]
            .into(),
            received: received.clone(),
        };
        let tools = ToolExecutor::with_policy(
            workspace.path(),
            &crate::tools::AccessPolicy {
                language_servers: crate::language_services::LanguageServers {
                    rust: Some(binary),
                    typescript: None,
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .unwrap();
        let mut session = NativeSession::with_tools(Box::new(model), tools);
        let (tx, _rx) = mpsc::channel(128);
        let events = EventSink::new("language-cancel".into(), tx, None).unwrap();
        let (commands, mut command_rx) = mpsc::channel(4);
        let running =
            tokio::spawn(
                async move { session.turn("write".into(), &mut command_rx, &events).await },
            );
        tokio::time::timeout(Duration::from_secs(5), async {
            while !workspace.path().join("main.rs").exists() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        commands.send(Command::Cancel).await.unwrap();
        assert!(matches!(
            tokio::time::timeout(Duration::from_secs(2), running)
                .await
                .unwrap()
                .unwrap()
                .unwrap(),
            TurnEnd::Cancelled
        ));
        let results = received.lock().unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].call_id, "completed");
        assert!(results[0].success, "{}", results[0].output);
        assert!(!results[0].output.contains("current"));
        assert!(!results[1].success);
        assert_eq!(
            std::fs::read_to_string(workspace.path().join("main.rs")).unwrap(),
            "fn main() {}"
        );
        assert!(!workspace.path().join("unstarted.rs").exists());
    }

    impl ToolHook for FailedPresentation {
        fn before(&self, _call: &mut ToolCall) -> Result<()> {
            Ok(())
        }
        fn present(&self, result: &ToolResult) -> Result<String> {
            anyhow::ensure!(result.success, "fixture presentation failed");
            Ok("Presentation only".into())
        }
    }

    #[tokio::test]
    async fn presentation_error_preserves_actual_failure_for_the_next_prompt() {
        let workspace = tempfile::tempdir().unwrap();
        std::fs::write(workspace.path().join("answer.py"), "value = 1\n").unwrap();
        let received = Arc::new(Mutex::new(Vec::new()));
        let check =
            json!({"command":"python3 -B -c 'from answer import value; assert value == 2'"});
        let model = ScriptedModel {
            responses: [
                vec![call("failed-check", "bash", check.clone())],
                vec![
                    call(
                        "correction",
                        "edit",
                        json!({"path":"answer.py","old_text":"value = 1","new_text":"value = 2"}),
                    ),
                    call("passed-check", "bash", check),
                ],
            ]
            .into(),
            received: received.clone(),
        };
        let mut session = NativeSession::new(Box::new(model), workspace.path()).unwrap();
        session.tools.add_hook(Box::new(FailedPresentation));
        let (tx, mut rx) = mpsc::channel(128);
        let events = EventSink::new("test".into(), tx, None).unwrap();
        let (_commands, mut command_rx) = mpsc::channel(4);
        let error = session
            .turn("check".into(), &mut command_rx, &events)
            .await
            .err()
            .expect("presentation must fail");
        assert!(error.to_string().contains("fixture presentation failed"));
        assert!(matches!(
            session
                .turn("fix".into(), &mut command_rx, &events)
                .await
                .unwrap(),
            TurnEnd::Complete
        ));
        let results = received.lock().unwrap();
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].call_id, "failed-check");
        assert_eq!(results[0].exit_code, Some(1));
        assert!(!results[0].success);
        assert!(results[0].output.contains("AssertionError"));
        assert_eq!(results[2].call_id, "passed-check");
        assert_eq!(results[2].exit_code, Some(0));
        assert!(results[2].success);
        let mut actual = Vec::new();
        while let Ok(envelope) = rx.try_recv() {
            if let Event::ToolFinished { result } = envelope.event {
                actual.push(result);
            }
        }
        assert_eq!(
            serde_json::to_value(actual).unwrap(),
            serde_json::to_value(&*results).unwrap()
        );
    }
}

#[cfg(test)]
mod non_tool_tests;
