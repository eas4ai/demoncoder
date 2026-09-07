use super::process::{BackendProcess, executable};
use crate::{
    config::Connection,
    events::{ContextUsage, Event, EventSink},
    session::{
        Command, Correction, Session, TurnEnd, correction_channel, correction_prompt, relay_command,
    },
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
    effort: Option<String>,
    process: Option<BackendProcess>,
    session: Option<String>,
    subscription_confirmed: bool,
    tools: ToolExecutor,
    next_control_id: u64,
}

pub fn open(config: &Connection, workspace: &Path) -> Result<Box<dyn Session>> {
    config.validate()?;
    if config.endpoint.is_some() {
        bail!("Claude subscription connections use the headless transport, not an API endpoint");
    }
    Ok(Box::new(Claude {
        binary: executable(config.binary.as_deref(), "claude")?,
        workspace: workspace.to_owned(),
        model: config.model.clone(),
        effort: config.effort.clone(),
        process: None,
        session: None,
        subscription_confirmed: false,
        tools: ToolExecutor::with_policy(workspace, &config.access)?,
        next_control_id: 1,
    }))
}

impl Claude {
    async fn run_turn(
        &mut self,
        mut prompt: String,
        steering: &mut mpsc::Receiver<Correction>,
        events: &EventSink,
    ) -> Result<TurnEnd> {
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
            if !self.tools.definitions().is_empty() {
                args.extend([
                    "--append-system-prompt".into(),
                    super::CREATOR_INSTRUCTIONS.into(),
                ]);
            }
            if let Some(model) = &self.model {
                args.extend(["--model".into(), model.clone()]);
            }
            if let Some(effort) = &self.effort {
                args.extend(["--effort".into(), effort.clone()]);
            }
            if let Some(session) = &self.session {
                args.extend(["--resume".into(), session.clone()]);
            }
            self.process = Some(BackendProcess::spawn(
                &self.binary,
                &args,
                &self.workspace,
                &["CLAUDE_CODE_OAUTH_TOKEN", "CLAUDE_CONFIG_DIR"],
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
                        handle_control(&self.tools, process, &message, events, false).await?;
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
        'turns: loop {
            events
                .emit(Event::Context {
                    usage: ContextUsage::default(),
                })
                .await?;
            self.tools.set_intent(&prompt);
            let admission = events.begin_backend()?;
            process.send(json!({"type":"user","message":{"role":"user","content":prompt},"parent_tool_use_id":null,"session_id":self.session.as_deref().unwrap_or("")})).await?;
            let mut context_usage = crate::context::MessageContext::default();
            let mut corrections = Vec::new();
            let mut interrupting = false;
            let interrupt_id = format!("steering-{}", self.next_control_id);
            self.next_control_id += 1;
            let mut interrupt_ack = false;
            let mut completed = false;
            let mut deadline = None;
            loop {
                while let Ok(text) = steering.try_recv() {
                    corrections.push(text);
                }
                if !corrections.is_empty() && !interrupting {
                    process.send(json!({"type":"control_request","request_id":interrupt_id,"request":{"subtype":"interrupt"}})).await?;
                    interrupting = true;
                    deadline =
                        Some(tokio::time::Instant::now() + std::time::Duration::from_secs(30));
                }
                if completed && interrupt_ack {
                    prompt = correction_prompt(corrections);
                    continue 'turns;
                }
                let message = tokio::select! {
                    biased;
                    Some(text) = steering.recv() => { corrections.push(text); continue; },
                    result = process.receive() => result?,
                    _ = async { match deadline {
                        Some(at) => tokio::time::sleep_until(at).await,
                        None => std::future::pending().await,
                    }} => bail!("Claude did not finish the superseded turn within 30 seconds"),
                };
                if message["type"] == "control_response"
                    && message["response"]["request_id"] == interrupt_id
                {
                    anyhow::ensure!(
                        interrupting && message["response"]["subtype"] == "success",
                        "Claude rejected steering interruption"
                    );
                    interrupt_ack = true;
                    continue;
                }
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
                        anyhow::ensure!(
                            message["apiKeySource"].as_str() == Some("none"),
                            "Claude did not confirm subscription authentication; an API-key or unknown route is not accepted"
                        );
                        self.subscription_confirmed = true;
                    }
                    Some("stream_event") => {
                        anyhow::ensure!(
                            self.subscription_confirmed,
                            "Claude returned model output before confirming subscription authentication"
                        );
                        let event = &message["event"];
                        if corrections.is_empty()
                            && message["parent_tool_use_id"].is_null()
                            && let Some(usage) = context_usage.observe(event)
                        {
                            events.emit(Event::Context { usage }).await?;
                        }
                        let delta = &event["delta"];
                        if delta["type"] == "text_delta" && corrections.is_empty() {
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
                    Some("assistant")
                        if corrections.is_empty() && message["parent_tool_use_id"].is_null() =>
                    {
                        anyhow::ensure!(
                            self.subscription_confirmed,
                            "Claude returned usage before confirming subscription authentication"
                        );
                        if message["message"]["usage"].is_object() {
                            events
                                .emit(Event::Context {
                                    usage: ContextUsage::anthropic(&message["message"]["usage"]),
                                })
                                .await?;
                        }
                    }
                    Some("result") => {
                        if !interrupting
                            && (message["is_error"] == true || message["subtype"] != "success")
                        {
                            bail!(
                                "Claude turn failed; check subscription login and model availability"
                            );
                        }
                        anyhow::ensure!(
                            self.subscription_confirmed,
                            "Claude completed a turn before confirming subscription authentication"
                        );
                        let usage = &message["usage"];
                        events
                            .emit(Event::Usage {
                                input: usage["input_tokens"].as_u64(),
                                output: usage["output_tokens"].as_u64(),
                                cached: usage["cache_read_input_tokens"].as_u64(),
                                cost_usd: message["total_cost_usd"].as_f64(),
                            })
                            .await?;
                        events.finish_model(admission)?;
                        if interrupting {
                            completed = true;
                        } else {
                            return Ok(TurnEnd::Complete);
                        }
                    }
                    Some("control_request") => {
                        anyhow::ensure!(
                            self.subscription_confirmed
                                || message["request"]["message"]["method"] != "tools/call",
                            "Claude requested a tool before confirming subscription authentication"
                        );
                        handle_control(
                            &self.tools,
                            process,
                            &message,
                            events,
                            corrections.is_empty() && self.subscription_confirmed,
                        )
                        .await?;
                    }
                    _ => {}
                }
            }
        }
    }
}

async fn handle_control(
    tools: &ToolExecutor,
    process: &mut BackendProcess,
    message: &Value,
    events: &EventSink,
    admit_tools: bool,
) -> Result<()> {
    let request = &message["request"];
    let response = match request["subtype"].as_str() {
        Some("can_use_tool") => {
            let name = request["tool_name"].as_str().unwrap_or("");
            let allowed = tools.definitions().iter().any(|tool| {
                tool["name"]
                    .as_str()
                    .is_some_and(|tool| name == format!("mcp__demoncoder__{tool}"))
            });
            if allowed && admit_tools {
                json!({"behavior":"allow", "updatedInput":request["input"]})
            } else {
                json!({"behavior":"deny", "message":"Only this session's registered DemonCoder tools are authorized."})
            }
        }
        Some("mcp_message") if request["server_name"] == "demoncoder" => {
            let rpc = &request["message"];
            let result = match rpc["method"].as_str() {
                Some("initialize") => {
                    json!({"protocolVersion":"2025-03-26","capabilities":{"tools":{}},"serverInfo":{"name":"demoncoder","version":env!("CARGO_PKG_VERSION")}})
                }
                Some("tools/list") => {
                    let definitions: Vec<Value> = tools.definitions().into_iter().map(|tool| json!({
                        "name":tool["name"], "description":tool["description"], "inputSchema":tool["input_schema"],
                    })).collect();
                    json!({"tools":definitions})
                }
                Some("tools/call") if !admit_tools => {
                    json!({"content":[{"type":"text","text":"Not executed: no active authorized turn or developer correction pending."}],"isError":true})
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
        let (corrections, mut steering) = correction_channel();
        let outcome = {
            let run = self.run_turn(prompt, &mut steering, events);
            tokio::pin!(run);
            loop {
                tokio::select! {
                    biased;
                    command = commands.recv() => if let Some(end) = relay_command(command, &corrections, events)? { break Ok(end); },
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
        self.subscription_confirmed = false;
        if let Some(mut process) = self.process.take() {
            process.stop().await?;
        }
        Ok(())
    }
}
