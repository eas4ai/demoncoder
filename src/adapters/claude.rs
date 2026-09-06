use super::process::{BackendProcess, executable};
use crate::{
    config::Connection,
    events::{Event, EventSink},
    session::{Command, Session, TurnEnd},
    tools::{ToolCall, ToolExecutor},
};
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;

struct Claude {
    binary: PathBuf,
    workspace: PathBuf,
    model: Option<String>,
    process: Option<BackendProcess>,
    session: Option<String>,
    tools: ToolExecutor,
}

pub fn open(config: &Connection, workspace: &Path) -> Result<Box<dyn Session>> {
    if config.endpoint.is_some() {
        bail!("Claude subscription connections use the headless transport, not an API endpoint");
    }
    Ok(Box::new(Claude {
        binary: executable(config.binary.as_deref(), "claude")?,
        workspace: workspace.to_owned(),
        model: config.model.clone(),
        process: None,
        session: None,
        tools: ToolExecutor::new(workspace)?,
    }))
}

impl Claude {
    async fn run_turn(&mut self, prompt: String, events: &EventSink) -> Result<TurnEnd> {
        if self.process.is_none() {
            let mut args: Vec<String> = [
                "-p",
                "--input-format",
                "stream-json",
                "--output-format",
                "stream-json",
                "--verbose",
                "--include-partial-messages",
                "--tools",
                "",
                "--strict-mcp-config",
                "--mcp-config",
                "{\"mcpServers\":{\"demoncoder\":{\"type\":\"sdk\",\"name\":\"demoncoder\"}}}",
                "--setting-sources",
                "",
                "--permission-prompt-tool",
                "stdio",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect();
            if let Some(model) = &self.model {
                args.extend(["--model".into(), model.clone()]);
            }
            self.process = Some(BackendProcess::spawn(
                &self.binary,
                &args,
                &self.workspace,
                &["CLAUDE_CODE_OAUTH_TOKEN"],
            )?);
            let process = self
                .process
                .as_mut()
                .context("Claude process unavailable")?;
            process.send(json!({"type":"control_request","request_id":"initialize","request":{"subtype":"initialize","hooks":null,"skills":[]}})).await?;
            tokio::time::timeout(std::time::Duration::from_secs(30), async {
                loop {
                    let message = process.receive().await?;
                    if message["type"] == "control_response"
                        && message["response"]["request_id"] == "initialize"
                    {
                        anyhow::ensure!(
                            message["response"]["subtype"] == "success",
                            "Claude initialization failed"
                        );
                        return Ok::<(), anyhow::Error>(());
                    }
                    if message["type"] == "control_request" {
                        handle_control(&self.tools, process, &message, events).await?;
                    }
                }
            })
            .await
            .context("Claude initialization timed out")??;
        }
        let process = self
            .process
            .as_mut()
            .context("Claude process unavailable")?;
        process.send(json!({"type":"user","message":{"role":"user","content":prompt},"parent_tool_use_id":null,"session_id":self.session.as_deref().unwrap_or("")})).await?;
        loop {
            let message = process.receive().await?;
            if let Some(session) = message["session_id"].as_str().filter(|id| !id.is_empty()) {
                if self
                    .session
                    .as_deref()
                    .is_some_and(|current| current != session)
                {
                    bail!("Claude event belongs to a different session");
                }
                self.session = Some(session.into());
            }
            match message["type"].as_str() {
                Some("system") if message["subtype"] == "init" => {
                    if let Some(source) = message["apiKeySource"].as_str()
                        && source != "none"
                    {
                        bail!(
                            "Claude selected API-key authentication for a subscription connection"
                        );
                    }
                }
                Some("stream_event") => {
                    let delta = &message["event"]["delta"];
                    if delta["type"] == "text_delta" {
                        events
                            .emit(Event::Text {
                                text: delta["text"]
                                    .as_str()
                                    .context("Claude text delta missing")?
                                    .into(),
                            })
                            .await?;
                    }
                }
                Some("result") => {
                    if message["is_error"] == true || message["subtype"] != "success" {
                        bail!(
                            "Claude turn failed; check subscription login and model availability"
                        );
                    }
                    let usage = &message["usage"];
                    events
                        .emit(Event::Usage {
                            input: usage["input_tokens"].as_u64(),
                            output: usage["output_tokens"].as_u64(),
                            cached: usage["cache_read_input_tokens"].as_u64(),
                            cost_usd: message["total_cost_usd"].as_f64(),
                        })
                        .await?;
                    return Ok(TurnEnd::Complete);
                }
                Some("control_request") => {
                    handle_control(&self.tools, process, &message, events).await?;
                }
                _ => {}
            }
        }
    }
}

async fn handle_control(
    tools: &ToolExecutor,
    process: &mut BackendProcess,
    message: &Value,
    events: &EventSink,
) -> Result<()> {
    let request = &message["request"];
    let response = match request["subtype"].as_str() {
        Some("can_use_tool") => {
            let name = request["tool_name"].as_str().unwrap_or("");
            let allowed = ["read", "write", "edit", "bash"]
                .iter()
                .any(|tool| name == format!("mcp__demoncoder__{tool}"));
            if allowed {
                json!({"behavior":"allow", "updatedInput":request["input"]})
            } else {
                json!({"behavior":"deny", "message":"Only DemonCoder's four coding tools are authorized."})
            }
        }
        Some("mcp_message") if request["server_name"] == "demoncoder" => {
            let rpc = &request["message"];
            let result = match rpc["method"].as_str() {
                Some("initialize") => {
                    json!({"protocolVersion":"2025-03-26","capabilities":{"tools":{}},"serverInfo":{"name":"demoncoder","version":env!("CARGO_PKG_VERSION")}})
                }
                Some("tools/list") => {
                    let definitions: Vec<Value> = crate::tools::definitions().into_iter().map(|tool| json!({
                        "name":tool["name"], "description":tool["description"], "inputSchema":tool["input_schema"],
                    })).collect();
                    json!({"tools":definitions})
                }
                Some("tools/call") => {
                    let result = tools
                        .execute(
                            ToolCall {
                                id: format!(
                                    "claude-mcp-{}",
                                    message["request_id"]
                                        .as_str()
                                        .context("missing Claude control request ID")?
                                ),
                                name: rpc["params"]["name"]
                                    .as_str()
                                    .context("missing Claude tool name")?
                                    .into(),
                                arguments: rpc["params"]["arguments"].clone(),
                            },
                            events,
                        )
                        .await?;
                    json!({"content":[{"type":"text", "text":serde_json::to_string(&result)?}],"isError":!result.success})
                }
                Some("notifications/initialized" | "ping") => json!({}),
                _ => {
                    return process.send(json!({"type":"control_response","response":{"subtype":"error","request_id":message["request_id"],"error":"unsupported MCP method"}})).await;
                }
            };
            json!({"mcp_response":{"jsonrpc":"2.0","id":rpc["id"],"result":result}})
        }
        _ => {
            return process.send(json!({"type":"control_response","response":{"subtype":"error","request_id":message["request_id"],"error":"unsupported control request"}})).await;
        }
    };
    process.send(json!({"type":"control_response","response":{"subtype":"success","request_id":message["request_id"],"response":response}})).await
}

#[async_trait]
impl Session for Claude {
    fn owner(&self) -> &'static str {
        "claude"
    }

    async fn turn(
        &mut self,
        prompt: String,
        commands: &mut mpsc::Receiver<Command>,
        events: &EventSink,
    ) -> Result<TurnEnd> {
        let outcome = {
            let run = self.run_turn(prompt, events);
            tokio::pin!(run);
            loop {
                tokio::select! {
                    biased;
                    command = commands.recv() => match command {
                        Some(Command::Cancel) => break Ok(TurnEnd::Cancelled),
                        Some(Command::Shutdown) | None => break Ok(TurnEnd::Shutdown),
                        Some(Command::Prompt(_)) => events.emit(Event::Error { message:"steering is not implemented for this connection yet".into() }).await?,
                    },
                    result = &mut run => break result,
                }
            }
        };
        if !matches!(outcome, Ok(TurnEnd::Complete)) {
            self.close().await?;
        }
        outcome
    }

    async fn close(&mut self) -> Result<()> {
        if let Some(mut process) = self.process.take() {
            process.stop().await?;
        }
        self.session = None;
        Ok(())
    }
}
