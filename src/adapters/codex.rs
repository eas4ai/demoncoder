#[path = "codex_non_tool.rs"]
mod non_tool;
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
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::sync::mpsc;

/// User-message item notifications have no effect in the normal dispatcher.
/// Before the exact correction reply, retain only the correlation fields that
/// dispatcher would inspect. Keep their queue position/count and byte budget;
/// unknown events and server requests retain their complete original payload.
fn pre_acknowledgment_frame(message: Value) -> Result<Value> {
    if message.get("id").is_some()
        || !matches!(
            message["method"].as_str(),
            Some("item/started" | "item/completed")
        )
        || message["params"]["item"]["type"] != "userMessage"
    {
        return Ok(message);
    }
    let params = &message["params"];
    let thread = params["threadId"]
        .as_str()
        .context("Codex user notification thread identifier missing")?;
    let turn = params["turnId"]
        .as_str()
        .context("Codex user notification turn identifier missing")?;
    Ok(json!({"method":message["method"],"params":{"threadId":thread,"turnId":turn}}))
}

struct Codex {
    observer_owner: crate::events::ObserverOwner,
    binary: PathBuf,
    workspace: PathBuf,
    model: Option<String>,
    effort: Option<String>,
    process: Option<BackendProcess>,
    thread: Option<String>,
    transcript_path: Option<String>,
    effective_model: Option<String>,
    next_id: u64,
    tools: ToolExecutor,
    access: crate::tools::AccessPolicy,
    lifecycle: Option<std::sync::Arc<crate::plugins::bridge::Lifecycle>>,
    relay: Option<crate::plugins::codex_relay::Owner>,
    relay_binary: PathBuf,
    ordinary_relay: Option<crate::plugins::codex_relay::Owner>,
    ordinary_callbacks: Option<non_tool::Callbacks>,
}

pub fn open(config: &Connection, workspace: &Path) -> Result<Box<dyn Session>> {
    config.validate()?;
    if config.endpoint.is_some() {
        bail!("Codex subscription connections use the app-server transport, not an API endpoint");
    }
    Ok(Box::new(Codex {
        observer_owner: Default::default(),
        binary: executable(config.binary.as_deref(), "codex")?,
        workspace: workspace.to_owned(),
        model: config.model.clone(),
        effort: config.effort.clone(),
        process: None,
        thread: None,
        transcript_path: None,
        effective_model: None,
        next_id: 1,
        tools: ToolExecutor::with_policy(workspace, &config.access)?,
        access: config.access.clone(),
        lifecycle: config.access.lifecycle.clone(),
        relay: None,
        ordinary_relay: None,
        ordinary_callbacks: None,
        relay_binary: config
            .access
            .supervisor
            .clone()
            .map(Ok)
            .unwrap_or_else(std::env::current_exe)?,
    }))
}

impl Codex {
    async fn rpc(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        let process = self.process.as_mut().context("Codex process unavailable")?;
        process
            .send(json!({"id":id,"method":method,"params":params}))
            .await?;
        tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                let message = process.receive().await?;
                  if message["id"] == id && message.get("method").is_none() {
                    if message.get("error").is_some() { bail!("Codex rejected {method}"); }
                    return message.get("result").cloned().context("Codex response has no result");
                }
                if message.get("id").is_some() && message.get("method").is_some() {
                    process.send(json!({"id":message["id"],"error":{"code":-32601,"message":"request is not supported by this client"}})).await?;
                }
            }
        }).await.context("Codex initialization timed out")?
    }

    async fn connect(&mut self) -> Result<()> {
        if self.process.is_some() {
            return Ok(());
        }
        if self.access.snapshot.is_some() {
            let mut probe = BackendProcess::spawn(
                &self.binary,
                &["--demoncoder-model-hook-capability".into()],
                &self.workspace,
                &[],
            )?;
            let capability =
                tokio::time::timeout(Duration::from_secs(5), probe.finite_json(4096)).await;
            probe.stop().await?;
            let capability =
                capability.context("managed Codex model-hook qualification timed out")??;
            anyhow::ensure!(
                capability["protocol"] == "demoncoder-model-hook-v1"
                    && capability["source_version"] == "0.153.4",
                "Codex lacks the qualified model-hook instruction isolation"
            );
        }
        if let Some(lifecycle) = &self.lifecycle {
            let mut probe = BackendProcess::spawn(
                &self.binary,
                &["--demoncoder-compaction-capability".into()],
                &self.workspace,
                &[],
            )?;
            let capability =
                tokio::time::timeout(Duration::from_secs(5), probe.finite_json(4096)).await;
            probe.stop().await?;
            let capability = capability.context("managed Codex qualification timed out")??;
            anyhow::ensure!(
                capability["protocol"] == "demoncoder-compaction-v1"
                    && capability["source_version"] == "0.153.4"
                    && capability["patch_version"] == 1,
                "Codex lacks the qualified managed compaction integration"
            );
            let relay = crate::plugins::codex_relay::Owner::new(lifecycle, &self.relay_binary)?;
            let mut access = self.access.clone();
            access
                .credential_paths
                .push(relay.protected_root().to_path_buf());
            self.tools.stop_language_services().await?;
            self.tools = ToolExecutor::with_policy(&self.workspace, &access)?;
            self.relay = Some(relay);
        }
        if !self.access.non_tools.is_empty() {
            anyhow::ensure!(
                self.access.snapshot.is_none(),
                "model-hook executor cannot own ordinary hooks"
            );
            let mut probe = BackendProcess::spawn(
                &self.binary,
                &["--demoncoder-ordinary-capability".into()],
                &self.workspace,
                &[],
            )?;
            let capability =
                tokio::time::timeout(Duration::from_secs(5), probe.finite_json(4096)).await;
            probe.stop().await?;
            let capability = capability.context("managed ordinary capability timed out")??;
            anyhow::ensure!(
                capability
                    == json!({"protocol":"demoncoder-ordinary-v1","source_version":"0.153.4","patch_version":1}),
                "Codex lacks qualified ordinary integration"
            );
            self.ordinary_relay = Some(crate::plugins::codex_relay::Owner::ordinary(
                &self.relay_binary,
            )?);
        }
        let mut args = vec!["app-server".into(), "--stdio".into()];
        for feature in [
            "shell_tool",
            "view_image",
            "apps",
            "plugins",
            "hooks",
            "multi_agent",
            "browser_use",
            "computer_use",
            "image_generation",
            "skill_mcp_dependency_install",
            "skill_search",
            "workspace_dependencies",
            "memories",
            "goals",
            "request_permissions_tool",
        ] {
            args.extend(["--disable".into(), feature.into()]);
        }
        for setting in [
            "mcp_servers={}",
            "web_search=\"disabled\"",
            "forced_login_method=\"chatgpt\"",
            "tools.experimental_request_user_input.enabled=false",
            "tools.update_plan.enabled=false",
            "orchestrator.skills.enabled=false",
            "orchestrator.mcp.enabled=false",
        ] {
            args.extend(["-c".into(), setting.into()]);
        }
        let mut environment = self
            .relay
            .as_ref()
            .map(|relay| vec![("CODEX_DEMONCODER_COMPACTION_RELAY", relay.requirement())])
            .unwrap_or_default();
        if let Some(relay) = &self.ordinary_relay {
            environment.push(("CODEX_DEMONCODER_ORDINARY_RELAY", relay.requirement()));
        }
        if self.access.snapshot.is_some() {
            environment.push(("CODEX_DEMONCODER_MODEL_HOOK", "v1"));
        }
        self.process = Some(
            if self.access.snapshot.is_some() || self.ordinary_relay.is_some() {
                BackendProcess::spawn_supervised_with_environment(
                    &self.binary,
                    &args,
                    &self.workspace,
                    &["CODEX_HOME"],
                    &environment,
                    &self.relay_binary,
                )?
            } else {
                BackendProcess::spawn_with_environment(
                    &self.binary,
                    &args,
                    &self.workspace,
                    &["CODEX_HOME"],
                    &environment,
                )?
            },
        );
        self.rpc(
            "initialize",
            json!({"clientInfo":{"name":"demoncoder","version":env!("CARGO_PKG_VERSION")},"capabilities":{"experimentalApi":true}}),
        )
        .await?;
        self.process
            .as_mut()
            .context("Codex process unavailable")?
            .send(json!({"method":"initialized","params":{}}))
            .await?;
        let account = self
            .rpc("account/read", json!({"refreshToken":false}))
            .await?;
        if account["account"]["type"] != "chatgpt" || account["requiresOpenaiAuth"] != true {
            bail!("Codex subscription connection requires a ChatGPT login; run codex login");
        }
        // Empty TOML tables merge with inherited tables; mcp_servers={} does
        // not disable configured servers. Resolve names without starting a
        // thread, then explicitly disable each inherited server for this one.
        let configuration = self
            .rpc("config/read", json!({"cwd":self.workspace}))
            .await?;
        crate::settings::probe::validate_codex_route(&configuration["config"])?;
        let configuration = configuration["config"]
            .as_object()
            .context("Codex did not return its effective configuration")?;
        let mut disabled_servers = serde_json::Map::new();
        if let Some(servers) = configuration.get("mcp_servers") {
            for name in servers
                .as_object()
                .context("Codex MCP configuration is not an object")?
                .keys()
            {
                disabled_servers.insert(name.clone(), json!({"enabled":false}));
            }
        }
        let dynamic_tools: Vec<Value> = self.tools.definitions().into_iter().map(|tool| json!({
            "type":"function", "name":tool["name"], "description":tool["description"], "inputSchema":tool["input_schema"],
        })).collect();
        let mut params = json!({
            "model":self.model,"cwd":self.workspace,"sandbox":if self.tools.unrestricted() {"danger-full-access"} else {"workspace-write"},"approvalPolicy":"never",
            "config":{"mcp_servers":disabled_servers},
        });
        if self.access.snapshot.is_some() {
            params["developerInstructions"] = super::MODEL_HOOK_INSTRUCTIONS.into();
        } else if !dynamic_tools.is_empty() {
            params["developerInstructions"] = super::CREATOR_INSTRUCTIONS.into();
        }
        let method = if let Some(thread) = &self.thread {
            params["threadId"] = json!(thread);
            "thread/resume"
        } else {
            params["experimentalRawEvents"] = json!(false);
            params["dynamicTools"] = json!(dynamic_tools);
            params["environments"] = json!([]);
            "thread/start"
        };
        let response = self.rpc(method, params).await?;
        let thread = response["thread"]["id"]
            .as_str()
            .context("Codex did not return a thread identifier")?;
        anyhow::ensure!(
            self.thread
                .as_deref()
                .is_none_or(|previous| previous == thread),
            "Codex resumed a different thread"
        );
        self.thread = Some(thread.into());
        self.transcript_path = response["thread"]["path"].as_str().map(str::to_owned);
        self.effective_model = response["model"]
            .as_str()
            .map(str::to_owned)
            .or_else(|| self.model.clone());
        if let Some(relay) = &self.ordinary_relay {
            self.ordinary_callbacks = Some(non_tool::Callbacks::new(
                self.workspace.clone(),
                relay.source_path(),
                self.thread.clone().context("ordinary thread missing")?,
                self.transcript_path
                    .clone()
                    .context("ordinary transcript missing")?,
                self.effective_model.clone(),
            )?);
        }
        Ok(())
    }

    async fn run_turn(
        &mut self,
        mut prompt: String,
        steering: &mut mpsc::Receiver<Correction>,
        events: &EventSink,
    ) -> Result<TurnEnd> {
        self.connect().await?;
        let mut developer_intent = prompt.clone();
        let mut next_correction: Option<super::post_correction::ExternalCorrection> = None;
        'turns: loop {
            let mut handoff = next_correction.take();
            let mut deadline = super::post_correction::CorrectionDeadline::new(handoff.is_some());
            deadline
                .during(events.emit(Event::Context {
                    usage: ContextUsage::default(),
                }))
                .await?;
            self.tools.set_intent(&developer_intent);
            let id = self.next_id;
            self.next_id += 1;
            let process = self.process.as_mut().context("Codex process unavailable")?;
            let correction_turn = handoff.is_some();
            let mut start_acknowledged = false;
            let admission = if let Some(correction) = &handoff {
                correction.invocation
            } else {
                events.begin_backend()?
            };
            let invocation_events = events.for_invocation(admission);
            let events = &invocation_events;
            let observer_delivery = if handoff.is_none() && !crate::workflow::is_control(&prompt) {
                events.observer_context()?
            } else {
                None
            };
            let outgoing_prompt = observer_delivery.as_ref().map_or_else(
                || prompt.clone(),
                |delivery| format!("{}\n{}", prompt, delivery.text),
            );
            let request = if let Some(correction) = &handoff {
                correction
                    .request
                    .clone()
                    .context("Codex correction frame was not prepared")?
            } else if prompt == "/compact" {
                json!({"id":id,"method":"thread/compact/start","params":{"threadId":self.thread}})
            } else {
                json!({"id":id,"method":"turn/start","params":{
                    "threadId":self.thread,"input":[{"type":"text","text":outgoing_prompt}],
                    "environments":[],"effort":self.effort,
                }})
            };
            if let Some(callbacks) = &mut self.ordinary_callbacks
                && request["method"] == "turn/start"
            {
                let origin = if let Some(correction) = &handoff {
                    correction.source_origin(events, &request)?
                } else if observer_delivery.is_some() {
                    crate::plugins::receipts::SourceOrigin::PluginContext
                } else {
                    crate::plugins::receipts::SourceOrigin::HostSubmission
                };
                callbacks.begin(&request, origin, events)?;
            }
            let ordinary_turn = request["method"] == "turn/start";
            deadline.during(process.send(request)).await?;
            if let Some(delivery) = &observer_delivery {
                events.complete_observer_context(delivery)?;
            }
            let mut turn: Option<String> = None;
            let mut corrections = Vec::new();
            let mut plugin_correction: Option<super::post_correction::ExternalCorrection> = None;
            let mut interrupt_id = None;
            let mut interrupt_ack = false;
            let mut correction_started = false;
            let mut pending_frames = std::collections::VecDeque::new();
            let mut pending_bytes = 0usize;
            let mut replaying_observed = false;
            let mut completed = false;
            loop {
                deadline.check()?;
                while let Ok(text) = steering.try_recv() {
                    deadline.check()?;
                    corrections.push(text);
                }
                if (!corrections.is_empty() || plugin_correction.is_some())
                    && turn.is_some()
                    && interrupt_id.is_none()
                {
                    let request = self.next_id;
                    self.next_id += 1;
                    deadline.supersede();
                    deadline
                        .during(process.send(
                            json!({"id":request,"method":"turn/interrupt","params":{
                                "threadId":self.thread,"turnId":turn,
                            }}),
                        ))
                        .await?;
                    interrupt_id = Some(request);
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
                        correction.prepare_codex_request(
                            &prompt,
                            self.thread
                                .as_deref()
                                .context("Codex correction thread missing")?,
                            self.effort.as_deref(),
                            self.next_id,
                        )?;
                        deadline.during(correction.reserve(&self.tools)).await?;
                        next_correction = Some(correction);
                    } else {
                        prompt = steering_prompt;
                    }
                    continue 'turns;
                }
                enum Incoming {
                    Backend(Value),
                    Relay(tokio::net::UnixStream),
                    Ordinary(tokio::net::UnixStream),
                }
                let incoming = if handoff.is_none() && !pending_frames.is_empty() {
                    {
                        replaying_observed = true;
                        Incoming::Backend(pending_frames.pop_front().expect("queued frame"))
                    }
                } else {
                    tokio::select! {
                        biased;
                        expired = deadline.wait() => { expired?; continue; },
                        Some(text) = steering.recv() => { corrections.push(text); continue; },
                        result = process.receive() => Incoming::Backend(result?),
                        stream = async {
                            match (&self.relay, &turn) {
                                (Some(relay), Some(_)) => relay.accept().await,
                                _ => std::future::pending().await,
                            }
                        } => Incoming::Relay(stream?),
                        stream = async {
                            match (&self.ordinary_relay, &self.ordinary_callbacks) {
                                (Some(relay), Some(callbacks)) if callbacks.ready() => relay.accept().await,
                                _ => std::future::pending().await,
                            }
                        } => Incoming::Ordinary(stream?),
                    }
                };
                deadline.check()?;
                let dispatch_deadline = deadline.clone();
                let end = dispatch_deadline.during(async {
                let message = match incoming {
                    Incoming::Backend(message) => message,
                    Incoming::Ordinary(mut stream) => {
                        let relay = self.ordinary_relay.as_ref().context("ordinary relay missing")?;
                        let input = relay.read(&mut stream).await?;
                        let callbacks = self.ordinary_callbacks.as_mut().context("ordinary owner missing")?;
                        let response = callbacks.handle(input, events, &self.tools).await?;
                        let bytes = non_tool::response_bytes(&response)?;
                        if response["continue"] == true { callbacks.prepare(events)?; }
                        tokio::time::timeout(Duration::from_secs(5), tokio::io::AsyncWriteExt::write_all(&mut stream, &bytes)).await.context("ordinary delivery timed out")??;
                        callbacks.sent(events)?;
                        return Ok(None);
                    },
                    Incoming::Relay(stream) => {
                        self.relay
                            .as_mut()
                            .context("managed relay unavailable")?
                            .handle(
                                stream,
                                self.thread
                                    .as_deref()
                                    .context("managed thread unavailable")?,
                                turn.as_deref().context("managed turn unavailable")?,
                                events,
                            )
                            .await?;
                        return Ok(None);
                    }
                };
                if ordinary_turn && !replaying_observed && let Some(callbacks) = &mut self.ordinary_callbacks {
                    callbacks.observe(&message, events)?;
                }
                replaying_observed = false;
                let is_reply = message.get("method").is_none();
                if handoff.is_some() && !is_reply {
                    let message = pre_acknowledgment_frame(message)?;
                    pending_bytes =
                        pending_bytes.saturating_add(serde_json::to_vec(&message)?.len());
                    anyhow::ensure!(
                        pending_frames.len() < 64 && pending_bytes <= 1024 * 1024,
                        "Codex pre-acknowledgment frames exceed bound"
                    );
                    pending_frames.push_back(message);
                    return Ok(None);
                }
                if is_reply && handoff.is_some() && message["result"]["turn"]["id"].is_string() {
                    anyhow::ensure!(
                        message["id"] == id,
                        "Codex correction acknowledgment request differs"
                    );
                }
                if is_reply && interrupt_id.is_some_and(|request| message["id"] == request) {
                    anyhow::ensure!(
                        !interrupt_ack
                            && message.get("error").is_none()
                            && message.get("result").is_some(),
                        "Codex rejected steering interruption"
                    );
                    interrupt_ack = true;
                    return Ok(None);
                }
                if is_reply && message["id"] == id && message.get("error").is_some() {
                    bail!("Codex rejected the turn");
                }
                if is_reply
                    && message["id"] == id
                    && let Some(returned) = message["result"]["turn"]["id"].as_str()
                {
                    anyhow::ensure!(
                        !correction_turn || !start_acknowledged,
                        "Codex correction acknowledgment repeated"
                    );
                    let mut retained_start = false;
                    for frame in &pending_frames {
                        let params = &frame["params"];
                        anyhow::ensure!(
                            params["threadId"]
                                .as_str()
                                .is_none_or(|thread| Some(thread) == self.thread.as_deref()),
                            "Codex buffered frame belongs to another thread"
                        );
                        let frame_turn = params["turnId"]
                            .as_str()
                            .or_else(|| params["turn"]["id"].as_str());
                        anyhow::ensure!(
                            frame_turn.is_none_or(|turn| turn == returned),
                            "Codex buffered frame belongs to another turn"
                        );
                        if frame["method"] == "turn/started" {
                            anyhow::ensure!(
                                !retained_start,
                                "Codex buffered start notification repeated"
                            );
                            retained_start = true;
                        }
                    }
                    start_acknowledged = true;
                    anyhow::ensure!(
                        turn.as_deref().is_none_or(|current| current == returned),
                        "Codex replied with an unrelated turn"
                    );
                    turn = Some(returned.to_owned());
                    if let Some(correction) = handoff.take() {
                        correction.acknowledge(
                            crate::plugins::receipts::CorrectionAcknowledgment::CodexTurn {
                                thread_id: self
                                    .thread
                                    .clone()
                                    .context("Codex correction thread missing")?,
                                turn_id: returned.into(),
                                request_id: id,
                            },
                        )?;
                        deadline.acknowledged()?;
                    }
                }
                let params = &message["params"];
                if let Some(thread) = params["threadId"].as_str()
                    && Some(thread) != self.thread.as_deref()
                {
                    bail!("Codex event belongs to a different thread");
                }
                if correction_turn && let Some(frame_turn) = params["turnId"].as_str() {
                    anyhow::ensure!(
                        Some(frame_turn) == turn.as_deref(),
                        "Codex correction frame belongs to a different turn"
                    );
                }
                match message["method"].as_str() {
                    Some("item/tool/call") => {
                        anyhow::ensure!(
                            handoff.is_none(),
                            "Codex requested a tool before correction acknowledgment"
                        );
                        if !corrections.is_empty()
                            || plugin_correction.is_some()
                            || params["turnId"].as_str() != turn.as_deref()
                            || turn.is_none()
                        {
                            process.send(json!({"id":message["id"],"result":{
                                "success":false,"contentItems":[{"type":"inputText","text":"Not executed: superseded or unrelated turn."}],
                            }})).await?;
                            return Ok(None);
                        }
                        anyhow::ensure!(
                            params["namespace"].is_null(),
                            "unknown Codex tool namespace"
                        );
                        let call_id = params["callId"]
                            .as_str()
                            .context("missing Codex tool call ID")?;
                        let tool_events = events.with_tool_representation(
                            crate::plugins::receipts::ToolRepresentation::CodexDynamic {
                                tool_use_id: call_id.into(),
                                turn_id: turn.clone().context("missing Codex turn")?,
                                session_id: self.thread.clone().context("missing Codex thread")?,
                                model: self.effective_model.clone(),
                                permission_mode: "bypassPermissions".into(),
                                transcript_path: self.transcript_path.clone(),
                            },
                        );
                        let result = self
                            .tools
                            .execute(
                                ToolCall {
                                    id: call_id.into(),
                                    name: params["tool"]
                                        .as_str()
                                        .context("missing Codex tool name")?
                                        .into(),
                                    arguments: params["arguments"].clone(),
                                },
                                &tool_events,
                            )
                            .await?;
                        if tool_events.post_continuation(call_id)?
                            == crate::plugins::receipts::PostContinuation::Correction
                        {
                            plugin_correction =
                                Some(super::post_correction::ExternalCorrection::start(
                                    &tool_events,
                                    call_id,
                                )?);
                            // Keep the dynamic response pending until this turn
                            // is terminal. Sending it first can start old work.
                            return Ok(None);
                        }
                        self.tools
                            .validate_post_release(call_id, &tool_events)
                            .await?;
                        tool_events.reserve_post_delivery(call_id)?;
                        process.send(json!({"id":message["id"],"result":{
                            "success":result.success,"contentItems":[{"type":"inputText","text":serde_json::to_string(&result)?}],
                        }})).await?;
                        tool_events.ack_post_delivery(call_id)?;
                    }
                    Some("turn/started") => {
                        anyhow::ensure!(
                            !correction_turn || !correction_started,
                            "Codex correction start notification repeated"
                        );
                        correction_started = true;
                        let started = params["turn"]["id"]
                            .as_str()
                            .context("Codex turn identifier missing")?;
                        if turn.as_deref().is_some_and(|current| current != started) {
                            bail!("Codex started an unrelated turn");
                        }
                        turn = Some(started.into());
                    }
                    Some("item/agentMessage/delta")
                        if corrections.is_empty() && plugin_correction.is_none() =>
                    {
                        anyhow::ensure!(
                            handoff.is_none(),
                            "Codex correction output precedes acknowledgment"
                        );
                        events
                            .emit(Event::Text {
                                text: params["delta"]
                                    .as_str()
                                    .context("Codex text delta missing")?
                                    .into(),
                            })
                            .await?;
                    }
                    Some("turn/completed") => {
                        anyhow::ensure!(
                            handoff.is_none(),
                            "Codex correction completed without acknowledgment"
                        );
                        anyhow::ensure!(
                            params["turn"]["id"].as_str() == turn.as_deref() && turn.is_some(),
                            "Codex completed an unrelated turn"
                        );
                        if plugin_correction.is_some() {
                            anyhow::ensure!(
                                !completed && params["turn"]["status"] == "interrupted",
                                "Codex correction did not supersede its original turn exactly once"
                            );
                            events.finish_model(admission)?;
                            completed = true;
                        } else if !corrections.is_empty() {
                            anyhow::ensure!(
                                matches!(
                                    params["turn"]["status"].as_str(),
                                    Some("completed" | "interrupted")
                                ),
                                "Codex superseded turn failed"
                            );
                            events.finish_model(admission)?;
                            completed = true;
                        } else {
                            events.finish_model(admission)?;
                            return match params["turn"]["status"].as_str() {
                                Some("completed") => Ok(Some(TurnEnd::Complete)),
                                Some("interrupted") => Ok(Some(TurnEnd::Cancelled)),
                                _ => bail!("Codex turn failed"),
                            };
                        }
                    }
                    Some("thread/tokenUsage/updated") => {
                        if !corrections.is_empty()
                            || plugin_correction.is_some()
                            || params["turnId"]
                                .as_str()
                                .is_some_and(|id| Some(id) != turn.as_deref())
                        {
                            return Ok(None);
                        }
                        events
                            .emit(Event::Context {
                                usage: ContextUsage::codex(&params["tokenUsage"]),
                            })
                            .await?;
                        let usage = &params["tokenUsage"]["last"];
                        events
                            .emit(Event::Usage {
                                input: usage["inputTokens"].as_u64(),
                                output: usage["outputTokens"].as_u64(),
                                cached: usage["cachedInputTokens"].as_u64(),
                                cost_usd: None,
                            })
                            .await?;
                    }
                    Some(_) if message.get("id").is_some() => {
                        process.send(json!({"id":message["id"],"error":{"code":-32601,"message":"request is not supported by this client"}})).await?;
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

#[async_trait]
impl Session for Codex {
    fn owner(&self) -> &'static str {
        "codex"
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
        let result = super::process::stop_backend_and_services(
            &mut self.process,
            self.tools.stop_language_services(),
        )
        .await;
        self.relay = None;
        self.ordinary_relay = None;
        self.ordinary_callbacks = None;
        // Keep backend-owned context for the next prompt after cancellation.
        observers.and(result)
    }
}
