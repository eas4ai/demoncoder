#[path = "claude_non_tool.rs"]
mod non_tool;
#[path = "claude_post.rs"]
mod post;
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
use std::sync::Arc;
use tokio::sync::mpsc;

struct Claude {
    observer_owner: crate::events::ObserverOwner,
    binary: PathBuf,
    supervisor: Option<PathBuf>,
    workspace: PathBuf,
    model: Option<String>,
    effort: Option<String>,
    process: Option<BackendProcess>,
    session: Option<String>,
    subscription_confirmed: bool,
    tools: ToolExecutor,
    model_hook: bool,
    next_control_id: u64,
    lifecycle: Option<Arc<crate::plugins::bridge::Lifecycle>>,
    callbacks: Option<crate::plugins::bridge::Callbacks>,
    non_tool_enabled: bool,
    non_tool_callbacks: Option<non_tool::Callbacks>,
    post_enabled: bool,
    post_callbacks: Option<post::Callbacks>,
}

pub fn open(config: &Connection, workspace: &Path) -> Result<Box<dyn Session>> {
    config.validate()?;
    if config.endpoint.is_some() {
        bail!("Claude subscription connections use the headless transport, not an API endpoint");
    }
    Ok(Box::new(Claude {
        observer_owner: Default::default(),
        binary: executable(config.binary.as_deref(), "claude")?,
        supervisor: if config.access.lifecycle.is_some()
            || config.access.snapshot.is_some()
            || !config.access.post_tools.is_empty()
            || !config.access.non_tools.is_empty()
        {
            Some(
                config
                    .access
                    .supervisor
                    .clone()
                    .map(Ok)
                    .unwrap_or_else(std::env::current_exe)?,
            )
        } else {
            None
        },
        workspace: workspace.to_owned(),
        model: config.model.clone(),
        effort: config.effort.clone(),
        process: None,
        session: None,
        subscription_confirmed: false,
        tools: ToolExecutor::with_policy(workspace, &config.access)?,
        model_hook: config.access.snapshot.is_some(),
        next_control_id: 1,
        lifecycle: config.access.lifecycle.clone(),
        callbacks: None,
        non_tool_enabled: !config.access.non_tools.is_empty(),
        non_tool_callbacks: None,
        post_enabled: !config.access.post_tools.is_empty(),
        post_callbacks: None,
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
            self.callbacks = self.lifecycle.as_ref().map(|l| l.callbacks()).transpose()?;
            self.post_callbacks = self.post_enabled.then(post::Callbacks::new).transpose()?;
            self.non_tool_callbacks = self
                .non_tool_enabled
                .then(|| non_tool::Callbacks::new(self.workspace.clone(), self.model.clone()))
                .transpose()?;
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
            if self.post_enabled || self.non_tool_enabled {
                args.push("--replay-user-messages".into());
            }
            if self.model_hook {
                args.extend([
                    "--safe-mode".into(),
                    "--append-system-prompt".into(),
                    super::MODEL_HOOK_INSTRUCTIONS.into(),
                ]);
            } else if !self.tools.definitions().is_empty() {
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
            self.process = Some(match &self.supervisor {
                Some(supervisor) => BackendProcess::spawn_supervised(
                    &self.binary,
                    &args,
                    &self.workspace,
                    &["CLAUDE_CODE_OAUTH_TOKEN", "CLAUDE_CONFIG_DIR"],
                    supervisor,
                )?,
                None => BackendProcess::spawn(
                    &self.binary,
                    &args,
                    &self.workspace,
                    &["CLAUDE_CODE_OAUTH_TOKEN", "CLAUDE_CONFIG_DIR"],
                )?,
            });
            let process = self
                .process
                .as_mut()
                .context("Claude process unavailable")?;
            let mut hooks = self
                .callbacks
                .as_ref()
                .map(|c| c.registration())
                .unwrap_or(Value::Null);
            if let Some(ordinary) = &self.non_tool_callbacks {
                hooks = ordinary.registration(hooks);
            }
            if let Some(post) = &self.post_callbacks {
                hooks = post.registration(hooks);
            }
            process.send(json!({"type":"control_request","request_id":"initialize","request":{"subtype":"initialize","hooks":hooks,"skills":[]}})).await?;
            tokio::time::timeout(std::time::Duration::from_secs(30), async {
                loop {
                    let message = process.receive().await?;
                    capture_session(&mut self.session, &message)?;
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
                        let correction = handle_control(
                            &self.tools,
                            process,
                            &message,
                            events,
                            false,
                            (
                                self.callbacks.as_mut(),
                                self.post_callbacks.as_mut(),
                                self.non_tool_callbacks.as_mut(),
                            ),
                            self.session.as_deref(),
                        )
                        .await?;
                        anyhow::ensure!(
                            correction.is_none(),
                            "correction requested during Claude initialization"
                        );
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
        let mut developer_intent = prompt.clone();
        let mut next_correction: Option<super::post_correction::ExternalCorrection> = None;
        'turns: loop {
            let mut handoff = next_correction.take();
            let source_correction_turn = handoff.is_some();
            let mut deadline = super::post_correction::CorrectionDeadline::new(handoff.is_some());
            deadline
                .during(events.emit(Event::Context {
                    usage: ContextUsage::default(),
                }))
                .await?;
            self.tools.set_intent(&developer_intent);
            let admission = if let Some(correction) = &handoff {
                correction.invocation
            } else {
                events.begin_backend()?
            };
            let invocation_events = events.for_invocation(admission);
            let events = &invocation_events;
            if let Some(post) = &mut self.post_callbacks {
                post.begin_invocation(events, handoff.is_some())?;
            }
            let observer_delivery = if handoff.is_none() && !crate::workflow::is_control(&prompt) {
                events.observer_context()?
            } else {
                None
            };
            let outgoing_prompt = observer_delivery.as_ref().map_or_else(
                || prompt.clone(),
                |delivery| format!("{}\n{}", prompt, delivery.text),
            );
            let mut user = if let Some(correction) = &handoff {
                correction
                    .request
                    .clone()
                    .context("Claude correction frame was not prepared")?
            } else {
                let mut user = json!({"type":"user","message":{"role":"user","content":outgoing_prompt},"parent_tool_use_id":null});
                if let Some(session) = &self.session {
                    user["session_id"] = json!(session);
                }
                user
            };
            if let Some(ordinary) = &mut self.non_tool_callbacks {
                if user.get("uuid").is_none() {
                    user["uuid"] = json!(post::user_uuid()?);
                }
                let origin = if let Some(correction) = &handoff {
                    correction.source_origin(events, &user)?
                } else if events.is_plugin_prompt() {
                    crate::plugins::receipts::SourceOrigin::PluginContext
                } else {
                    crate::plugins::receipts::SourceOrigin::HostSubmission
                };
                ordinary.begin(&user, origin, events)?;
            }
            deadline.during(process.send(user.clone())).await?;
            if let Some(delivery) = &observer_delivery {
                events.complete_observer_context(delivery)?;
            }
            let mut context_usage = crate::context::MessageContext::default();
            let mut corrections = Vec::new();
            let mut plugin_correction: Option<super::post_correction::ExternalCorrection> = None;
            let mut withheld_callback: Option<String> = None;
            let mut interrupting = false;
            let interrupt_id = format!("steering-{}", self.next_control_id);
            self.next_control_id += 1;
            let mut interrupt_ack = false;
            let mut completed = false;
            loop {
                deadline.check()?;
                while let Ok(text) = steering.try_recv() {
                    deadline.check()?;
                    corrections.push(text);
                }
                if (!corrections.is_empty() || plugin_correction.is_some()) && !interrupting {
                    deadline.supersede();
                    deadline.during(process.send(json!({"type":"control_request","request_id":interrupt_id,"request":{"subtype":"interrupt"}}))).await?;
                    interrupting = true;
                }
                if completed && interrupt_ack {
                    let steering_prompt = correction_prompt(corrections);
                    if !steering_prompt.is_empty() {
                        developer_intent = steering_prompt.clone();
                    }
                    if let Some(mut correction) = plugin_correction.take() {
                        prompt = correction.prompt.clone();
                        if !steering_prompt.is_empty() {
                            prompt.push_str("\n[Developer correction]\n");
                            prompt.push_str(&steering_prompt);
                        }
                        anyhow::ensure!(
                            prompt.len() <= 4 * 1024 * 1024,
                            "correction prompt exceeds bound"
                        );
                        correction.prepare_claude_user(
                            &prompt,
                            self.session.as_deref().unwrap_or(""),
                            &post::user_uuid()?,
                        )?;
                        deadline.during(correction.reserve(&self.tools)).await?;
                        next_correction = Some(correction);
                    } else {
                        prompt = steering_prompt;
                    }
                    if let Some(ordinary) = &mut self.non_tool_callbacks {
                        ordinary.superseded();
                    }
                    continue 'turns;
                }
                let message = tokio::select! {
                    biased;
                    expired = deadline.wait() => { expired?; continue; },
                    Some(text) = steering.recv() => { corrections.push(text); continue; },
                    result = process.receive() => result?,
                };
                deadline.check()?;
                let dispatch_deadline = deadline.clone();
                let end = dispatch_deadline.during(async {
                if message["type"] == "control_response"
                    && message["response"]["request_id"] == interrupt_id
                {
                    anyhow::ensure!(
                        interrupting
                            && !interrupt_ack
                            && message["response"]["subtype"] == "success",
                        "Claude rejected steering interruption"
                    );
                    interrupt_ack = true;
                    return Ok(None);
                }
                capture_session(&mut self.session, &message)?;
                if !interrupting && let Some(ordinary) = &mut self.non_tool_callbacks { ordinary.observe(&message, events)?; }
                match message["type"].as_str() {
                    Some("user") if message["isReplay"] == true && user["uuid"].is_string() && source_correction_turn => {
                        anyhow::ensure!(
                            message["uuid"] == user["uuid"]
                                && message["session_id"] == user["session_id"]
                                && message["message"] == user["message"]
                                && message["parent_tool_use_id"].is_null(),
                            "Claude correction user acknowledgment differs"
                        );
                        handoff
                            .take()
                            .context("Claude correction user acknowledgment repeated")?
                            .acknowledge(
                                crate::plugins::receipts::CorrectionAcknowledgment::ClaudeUser {
                                    session_id: self
                                        .session
                                        .clone()
                                        .context("Claude correction session missing")?,
                                    uuid: user["uuid"]
                                        .as_str()
                                        .context("Claude correction UUID missing")?
                                        .into(),
                                    content_digest: crate::plugins::admission::digest(
                                        &user["message"],
                                    )?,
                                },
                            )?;
                        deadline.acknowledged()?;
                    }
                    Some("control_cancel_request") if plugin_correction.is_some() => {
                        anyhow::ensure!(
                            message["request_id"].as_str() == withheld_callback.as_deref(),
                            "Claude cancelled an unrelated post callback"
                        );
                    }
                    Some("system") if message["subtype"] == "init" => {
                        anyhow::ensure!(
                            message["apiKeySource"].as_str() == Some("none"),
                            "Claude did not confirm subscription authentication; an API-key or unknown route is not accepted"
                        );
                        self.subscription_confirmed = true;
                    }
                    Some("stream_event") => {
                        anyhow::ensure!(
                            handoff.is_none(),
                            "Claude correction output precedes exact user acknowledgment"
                        );
                        anyhow::ensure!(
                            self.subscription_confirmed,
                            "Claude returned model output before confirming subscription authentication"
                        );
                        if plugin_correction.is_none() {
                            events.ensure_continuation()?;
                        }
                        let event = &message["event"];
                        if corrections.is_empty()
                            && plugin_correction.is_none()
                            && message["parent_tool_use_id"].is_null()
                            && let Some(usage) = context_usage.observe(event)
                        {
                            events.emit(Event::Context { usage }).await?;
                        }
                        let delta = &event["delta"];
                        if delta["type"] == "text_delta"
                            && corrections.is_empty()
                            && plugin_correction.is_none()
                        {
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
                        if corrections.is_empty()
                            && plugin_correction.is_none()
                            && message["parent_tool_use_id"].is_null() =>
                    {
                        anyhow::ensure!(
                            handoff.is_none(),
                            "Claude correction usage precedes exact user acknowledgment"
                        );
                        anyhow::ensure!(
                            self.subscription_confirmed,
                            "Claude returned usage before confirming subscription authentication"
                        );
                        events.ensure_continuation()?;
                        if message["message"]["usage"].is_object() {
                            events
                                .emit(Event::Context {
                                    usage: ContextUsage::anthropic(&message["message"]["usage"]),
                                })
                                .await?;
                        }
                    }
                    Some("result") => {
                        anyhow::ensure!(
                            handoff.is_none(),
                            "Claude correction completed without exact user acknowledgment"
                        );
                        if plugin_correction.is_some() {
                            anyhow::ensure!(
                                interrupting
                                    && !completed
                                    && message["session_id"].as_str() == self.session.as_deref()
                                    && message["is_error"] == true
                                    && message["subtype"] == "error_during_execution"
                                    && message["terminal_reason"] == "aborted_tools",
                                "Claude correction lacks the qualified interrupted terminal result"
                            );
                        } else {
                            events.ensure_continuation()?;
                        }
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
                            return Ok(Some(TurnEnd::Complete));
                        }
                    }
                    Some("control_request") => {
                        anyhow::ensure!(
                            handoff.is_none()
                                || self.non_tool_callbacks.as_ref().is_some_and(|ordinary| ordinary.permits_handoff_submit(&message["request"]))
                                || message["request"]["subtype"] == "mcp_message"
                                    && message["request"]["message"]["method"] != "tools/call",
                            "Claude requested a tool or hook before correction acknowledgment"
                        );
                        anyhow::ensure!(
                            self.subscription_confirmed
                                || message["request"]["message"]["method"] != "tools/call",
                            "Claude requested a tool before confirming subscription authentication"
                        );
                        let correction = handle_control(
                            &self.tools,
                            process,
                            &message,
                            events,
                            corrections.is_empty()
                                && plugin_correction.is_none()
                                && self.subscription_confirmed,
                            (self.callbacks.as_mut(), self.post_callbacks.as_mut(), self.non_tool_callbacks.as_mut()),
                            self.session.as_deref(),
                        )
                        .await?;
                        if let Some(correction) = correction {
                            anyhow::ensure!(
                                plugin_correction.is_none(),
                                "Claude repeated post-tool correction"
                            );
                            withheld_callback = Some(
                                message["request_id"]
                                    .as_str()
                                    .context("Claude post callback ID missing")?
                                    .into(),
                            );
                            plugin_correction = Some(correction);
                        }
                    }
                    _ => {}
                }
                    Ok(None)
                }).await?;
                if let Some(end) = end {
                    return Ok(end);
                }
            }
        }
    }
}

fn capture_session(current: &mut Option<String>, message: &Value) -> Result<()> {
    if let Some(session) = message["session_id"].as_str().filter(|id| !id.is_empty()) {
        anyhow::ensure!(
            session.len() <= 256 && current.as_deref().is_none_or(|id| id == session),
            "Claude event belongs to a different session"
        );
        *current = Some(session.into());
    }
    Ok(())
}

async fn handle_control(
    tools: &ToolExecutor,
    process: &mut BackendProcess,
    message: &Value,
    events: &EventSink,
    admit_tools: bool,
    callbacks: (
        Option<&mut crate::plugins::bridge::Callbacks>,
        Option<&mut post::Callbacks>,
        Option<&mut non_tool::Callbacks>,
    ),
    session_id: Option<&str>,
) -> Result<Option<super::post_correction::ExternalCorrection>> {
    let (callbacks, mut post, ordinary) = callbacks;
    let request = &message["request"];
    let response = match request["subtype"].as_str() {
        Some("hook_callback") => {
            if let Some(ordinary) = ordinary.filter(|c| c.owns(request)) {
                let response = tokio::select! {
                    result = ordinary.handle(message, session_id, events, tools) => result?,
                    result = process.wait_for_exit() => { result?; unreachable!() },
                };
                process.send(json!({"type":"control_response","response":{"subtype":"success","request_id":message["request_id"],"response":response}})).await?;
                ordinary.sent(events)?;
                return Ok(None);
            }
            if let Some(post) = post.as_mut().filter(|p| p.owns(request)) {
                if request["input"]["hook_event_name"] == "PreToolUse" {
                    let response = post.metadata(message, session_id, events)?;
                    process.send(json!({"type":"control_response","response":{"subtype":"success","request_id":message["request_id"],"response":response}})).await?;
                    return Ok(None);
                }
                let presentation = tokio::time::timeout(
                    std::time::Duration::from_secs(30),
                    post.presentation(message, session_id, events, tools),
                )
                .await
                .context("post-tool delivery validation timed out")??;
                let (response, call) = match presentation {
                    post::Presentation::Response { value, call } => (value, call),
                    post::Presentation::Correction { call } => {
                        return Ok(Some(super::post_correction::ExternalCorrection::start(
                            events, &call,
                        )?));
                    }
                };
                process.send(json!({"type":"control_response","response":{"subtype":"success","request_id":message["request_id"],"response":response}})).await?;
                events.ack_post_delivery(&call)?;
                return Ok(None);
            }
            // The owning turn closes and reaps the backend on every error. Do
            // not send an SDK error or close stdin: both can fail open upstream.
            let callback = callbacks.context("unexpected backend lifecycle callback")?;
            let decision = tokio::select! {
                result = callback.handle(message, session_id, events) => result,
                result = process.wait_for_exit() => { result?; unreachable!() },
            };
            match decision {
                Ok(response) => response,
                Err(error) => {
                    if request["input"]["hook_event_name"] == "PreCompact" {
                        // Give a live relay a typed denial, then terminate the
                        // failed turn. PostCompact has no Claude veto response.
                        let denial = json!({"decision":"block","reason":"Host lifecycle admission failed; work remains held"});
                        process.send(json!({"type":"control_response","response":{"subtype":"success","request_id":message["request_id"],"response":denial}})).await?;
                    }
                    return Err(error);
                }
            }
        }
        Some("can_use_tool") => {
            let name = request["tool_name"].as_str().unwrap_or("");
            let allowed = tools.definitions().iter().any(|tool| {
                tool["name"]
                    .as_str()
                    .is_some_and(|tool| name == format!("mcp__demoncoder__{tool}"))
            });
            if allowed && admit_tools {
                if let Some(post) = &post {
                    post.permission(request, events)?;
                }
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
                    let call = ToolCall {
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
                    };
                    let scoped = if let Some(post) = post.as_mut() {
                        post.correlate(rpc, &call, events)?
                    } else {
                        events.clone()
                    };
                    let result = tools.execute(call, &scoped).await?;
                    let original = if let Some(post) = post.as_mut() {
                        post.completed(&result.call_id, events, &result)?
                    } else {
                        events.validate_hook_delivery()?;
                        result
                    };
                    json!({"content":[{"type":"text", "text":serde_json::to_string(&original)?}],"isError":!original.success})
                }
                Some("notifications/initialized" | "notifications/cancelled" | "ping") => json!({}),
                _ => {
                    process.send(json!({"type":"control_response","response":{"subtype":"error","request_id":message["request_id"],"error":"unsupported MCP method"}})).await?;
                    return Ok(None);
                }
            };
            json!({"mcp_response":{"jsonrpc":"2.0","id":rpc["id"],"result":result}})
        }
        _ => {
            process.send(json!({"type":"control_response","response":{"subtype":"error","request_id":message["request_id"],"error":"unsupported control request"}})).await?;
            return Ok(None);
        }
    };
    process.send(json!({"type":"control_response","response":{"subtype":"success","request_id":message["request_id"],"response":response}})).await?;
    Ok(None)
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
        self.observer_owner.capture(events);
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

    async fn cancel_background(&mut self) -> Result<()> {
        self.observer_owner.stop().await
    }

    async fn close(&mut self) -> Result<()> {
        let observers = self.observer_owner.stop().await;
        self.subscription_confirmed = false;
        let result = super::process::stop_backend_and_services(
            &mut self.process,
            self.tools.stop_language_services(),
        )
        .await;
        self.callbacks = None;
        self.post_callbacks = None;
        self.non_tool_callbacks = None;
        observers.and(result)
    }
}
