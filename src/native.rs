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
