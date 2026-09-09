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
        let outcome = self.run_turn(prompt, commands, events).await;
        self.settle_interruption()?;
        if !matches!(outcome, Ok(TurnEnd::Complete)) {
            self.tools.stop_language_services().await?;
        }
        events.checkpoint(self.checkpoint())?;
        outcome
    }

    async fn close(&mut self) -> Result<()> {
        self.tools.stop_language_services().await
    }
}

impl NativeSession {
    async fn run_turn(
        &mut self,
        prompt: String,
        commands: &mut mpsc::Receiver<Command>,
        events: &EventSink,
    ) -> Result<TurnEnd> {
        self.tools.set_intent(&prompt);
        self.model.prompt(prompt);
        events.checkpoint(self.checkpoint())?;
        loop {
            let mut corrections = Vec::new();
            let admission = events.begin_model()?;
            let calls = {
                let response = self.model.response(events);
                tokio::pin!(response);
                loop {
                    tokio::select! {
                        biased;
                        command = commands.recv() => if let Some(end) = control(command, &mut corrections, events)? { return Ok(end); },
                        result = &mut response => break result,
                    }
                }
            };
            events.finish_model(admission)?;
            // A returned provider error has no pending native tool effects. Keep
            // the error, but settle its admission; cancellation exits above and
            // deliberately leaves the interrupted request uncertain.
            let calls = calls?;
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
                let operation = self.tools.execute(call, events);
                tokio::pin!(operation);
                let result = loop {
                    tokio::select! {
                        biased;
                        command = commands.recv() => if let Some(end) = control(command, &mut corrections, events)? { return Ok(end); },
                        result = &mut operation => break result?,
                    }
                };
                self.model.results(vec![result]);
                self.pending.pop_front();
                self.tools.take_completed();
                events.checkpoint(self.checkpoint())?;
            }
            let corrected = !corrections.is_empty();
            for correction in corrections {
                self.tools.set_intent(&correction);
                self.model.prompt(correction);
            }
            events.checkpoint(self.checkpoint())?;
            if finished && !corrected {
                return Ok(TurnEnd::Complete);
            }
        }
    }
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
