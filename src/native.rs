//! One small model/tool loop shared by direct API providers.
use crate::{
    events::{Event, EventSink},
    session::{Command, Session, TurnEnd},
    tools::{ToolCall, ToolExecutor, ToolResult},
};
use anyhow::Result;
use async_trait::async_trait;
use std::path::Path;
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
}

impl NativeSession {
    pub fn new(model: Box<dyn Model>, workspace: &Path) -> Result<Self> {
        Ok(Self {
            model,
            tools: ToolExecutor::new(workspace)?,
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
            let mut results = Vec::new();
            for call in calls {
                while let Ok(command) = commands.try_recv() {
                    if let Some(end) = control(Some(command), &mut corrections, events).await? {
                        return Ok(end);
                    }
                }
                if !corrections.is_empty() {
                    results.push(ToolResult {
                        call_id: call.id,
                        tool: call.name,
                        success: false,
                        output: "Not executed: developer corrected this response.".into(),
                        exit_code: None,
                    });
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
                results.push(result);
            }
            if !results.is_empty() {
                self.model.results(results);
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
