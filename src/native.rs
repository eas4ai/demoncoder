//! One small model/tool loop shared by direct API providers.
use crate::{
    events::{Event, EventSink},
    session::{Command, Session, TurnEnd},
    tools::{ToolCall, ToolExecutor, ToolResult},
};
use anyhow::Result;
use async_trait::async_trait;
use std::{collections::VecDeque, path::Path};
use tokio::sync::mpsc;

#[async_trait]
pub trait Model: Send {
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
        Ok(Self {
            model,
            tools: ToolExecutor::new(workspace)?,
            pending: VecDeque::new(),
        })
    }
}

#[async_trait]
impl Session for NativeSession {
    fn owner(&self) -> &'static str {
        "demoncoder"
    }

    async fn turn(
        &mut self,
        prompt: String,
        commands: &mut mpsc::Receiver<Command>,
        events: &EventSink,
    ) -> Result<TurnEnd> {
        let outcome = self.run_turn(prompt, commands, events).await;
        if let Some(result) = self.tools.take_completed()
            && self
                .pending
                .front()
                .is_some_and(|call| call.id == result.call_id)
        {
            self.model.results(vec![result]);
            self.pending.pop_front();
        }
        if !self.pending.is_empty() {
            // Close the provider's outstanding call records without inventing
            // an execution result for an interrupted or unstarted operation.
            self.model.results(self.pending.drain(..).map(|call| ToolResult {
                call_id: call.id,
                tool: call.name,
                success: false,
                output: "Turn ended before this tool returned a result. It may have partial effects; inspect the workspace before retrying.".into(),
                exit_code: None,
            }).collect());
        }
        outcome
    }
}

impl NativeSession {
    async fn run_turn(
        &mut self,
        prompt: String,
        commands: &mut mpsc::Receiver<Command>,
        events: &EventSink,
    ) -> Result<TurnEnd> {
        self.model.prompt(prompt);
        loop {
            let mut corrections = Vec::new();
            let calls = {
                let response = self.model.response(events);
                tokio::pin!(response);
                loop {
                    tokio::select! {
                        biased;
                        command = commands.recv() => if let Some(end) = control(command, &mut corrections, events).await? { return Ok(end); },
                        result = &mut response => break result?,
                    }
                }
            };
            let finished = calls.is_empty();
            self.pending = calls.into();
            while let Some(call) = self.pending.front().cloned() {
                while let Ok(command) = commands.try_recv() {
                    if let Some(end) = control(Some(command), &mut corrections, events).await? {
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
                        command = commands.recv() => if let Some(end) = control(command, &mut corrections, events).await? { return Ok(end); },
                        result = &mut operation => break result?,
                    }
                };
                self.model.results(vec![result]);
                self.pending.pop_front();
                self.tools.take_completed();
            }
            let corrected = !corrections.is_empty();
            for correction in corrections {
                self.model.prompt(correction);
            }
            if finished && !corrected {
                return Ok(TurnEnd::Complete);
            }
        }
    }
}

async fn control(
    command: Option<Command>,
    corrections: &mut Vec<String>,
    events: &EventSink,
) -> Result<Option<TurnEnd>> {
    match command {
        Some(Command::Cancel) => Ok(Some(TurnEnd::Cancelled)),
        Some(Command::Shutdown) | None => Ok(Some(TurnEnd::Shutdown)),
        Some(Command::Prompt(text)) => {
            corrections.push(text);
            events
                .emit(Event::Text {
                    text: "\n[Correction queued for the next tool boundary]\n".into(),
                })
                .await?;
            Ok(None)
        }
    }
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
