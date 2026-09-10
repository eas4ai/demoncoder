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

struct Codex {
    binary: PathBuf,
    workspace: PathBuf,
    model: Option<String>,
    effort: Option<String>,
    process: Option<BackendProcess>,
    thread: Option<String>,
    next_id: u64,
    tools: ToolExecutor,
    access: crate::tools::AccessPolicy,
    lifecycle: Option<std::sync::Arc<crate::plugins::bridge::Lifecycle>>,
    relay: Option<crate::plugins::codex_relay::Owner>,
    relay_binary: PathBuf,
}

pub fn open(config: &Connection, workspace: &Path) -> Result<Box<dyn Session>> {
    config.validate()?;
    if config.endpoint.is_some() {
        bail!("Codex subscription connections use the app-server transport, not an API endpoint");
    }
    Ok(Box::new(Codex {
        binary: executable(config.binary.as_deref(), "codex")?,
        workspace: workspace.to_owned(),
        model: config.model.clone(),
        effort: config.effort.clone(),
        process: None,
        thread: None,
        next_id: 1,
        tools: ToolExecutor::with_policy(workspace, &config.access)?,
        access: config.access.clone(),
        lifecycle: config.access.lifecycle.clone(),
        relay: None,
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
        let environment = self
            .relay
            .as_ref()
            .map(|relay| vec![("CODEX_DEMONCODER_COMPACTION_RELAY", relay.requirement())])
            .unwrap_or_default();
        self.process = Some(BackendProcess::spawn_with_environment(
            &self.binary,
            &args,
            &self.workspace,
            &["CODEX_HOME"],
            &environment,
        )?);
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
        if !dynamic_tools.is_empty() {
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
        Ok(())
    }

    async fn run_turn(
        &mut self,
        mut prompt: String,
        steering: &mut mpsc::Receiver<Correction>,
        events: &EventSink,
    ) -> Result<TurnEnd> {
        self.connect().await?;
        'turns: loop {
            events
                .emit(Event::Context {
                    usage: ContextUsage::default(),
                })
                .await?;
            self.tools.set_intent(&prompt);
            let id = self.next_id;
            self.next_id += 1;
            let process = self.process.as_mut().context("Codex process unavailable")?;
            let admission = events.begin_backend()?;
            let request = if prompt == "/compact" {
                json!({"id":id,"method":"thread/compact/start","params":{"threadId":self.thread}})
            } else {
                json!({"id":id,"method":"turn/start","params":{
                    "threadId":self.thread,"input":[{"type":"text","text":prompt}],
                    "environments":[],"effort":self.effort,
                }})
            };
            process.send(request).await?;
            let mut turn: Option<String> = None;
            let mut corrections = Vec::new();
            let mut interrupt_id = None;
            let mut interrupt_ack = false;
            let mut completed = false;
            let mut deadline = None;
            loop {
                while let Ok(text) = steering.try_recv() {
                    corrections.push(text);
                }
                if !corrections.is_empty() && turn.is_some() && interrupt_id.is_none() {
                    let request = self.next_id;
                    self.next_id += 1;
                    process
                        .send(json!({"id":request,"method":"turn/interrupt","params":{
                            "threadId":self.thread,"turnId":turn,
                        }}))
                        .await?;
                    interrupt_id = Some(request);
                    deadline =
                        Some(tokio::time::Instant::now() + std::time::Duration::from_secs(30));
                }
                if completed && interrupt_ack {
                    prompt = correction_prompt(corrections);
                    continue 'turns;
                }
                enum Incoming {
                    Backend(Value),
                    Relay(tokio::net::UnixStream),
                }
                let incoming = tokio::select! {
                    biased;
                    Some(text) = steering.recv() => { corrections.push(text); continue; },
                    result = process.receive() => Incoming::Backend(result?),
                    stream = async {
                        match (&self.relay, &turn) {
                            (Some(relay), Some(_)) => relay.accept().await,
                            _ => std::future::pending().await,
                        }
                    } => Incoming::Relay(stream?),
                    _ = async { match deadline {
                        Some(at) => tokio::time::sleep_until(at).await,
                        None => std::future::pending().await,
                    }} => bail!("Codex did not finish the superseded turn within 30 seconds"),
                };
                let message = match incoming {
                    Incoming::Backend(message) => message,
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
                        continue;
                    }
                };
                let is_reply = message.get("method").is_none();
                if is_reply && interrupt_id.is_some_and(|request| message["id"] == request) {
                    anyhow::ensure!(
                        message.get("error").is_none(),
                        "Codex rejected steering interruption"
                    );
                    interrupt_ack = true;
                    continue;
                }
                if is_reply && message["id"] == id && message.get("error").is_some() {
                    bail!("Codex rejected the turn");
                }
                if is_reply
                    && message["id"] == id
                    && let Some(returned) = message["result"]["turn"]["id"].as_str()
                {
                    anyhow::ensure!(
                        turn.as_deref().is_none_or(|current| current == returned),
                        "Codex replied with an unrelated turn"
                    );
                    turn = Some(returned.to_owned());
                }
                let params = &message["params"];
                if let Some(thread) = params["threadId"].as_str()
                    && Some(thread) != self.thread.as_deref()
                {
                    bail!("Codex event belongs to a different thread");
                }
                match message["method"].as_str() {
                    Some("item/tool/call") => {
                        if !corrections.is_empty()
                            || params["turnId"].as_str() != turn.as_deref()
                            || turn.is_none()
                        {
                            process.send(json!({"id":message["id"],"result":{
                                "success":false,"contentItems":[{"type":"inputText","text":"Not executed: superseded or unrelated turn."}],
                            }})).await?;
                            continue;
                        }
                        anyhow::ensure!(
                            params["namespace"].is_null(),
                            "unknown Codex tool namespace"
                        );
                        let result = self
                            .tools
                            .execute(
                                ToolCall {
                                    id: params["callId"]
                                        .as_str()
                                        .context("missing Codex tool call ID")?
                                        .into(),
                                    name: params["tool"]
                                        .as_str()
                                        .context("missing Codex tool name")?
                                        .into(),
                                    arguments: params["arguments"].clone(),
                                },
                                events,
                            )
                            .await?;
                        // Deliver completed evidence before interrupting its backend turn.
                        process.send(json!({"id":message["id"],"result":{
                            "success":result.success,"contentItems":[{"type":"inputText","text":serde_json::to_string(&result)?}],
                        }})).await?;
                    }
                    Some("turn/started") => {
                        let started = params["turn"]["id"]
                            .as_str()
                            .context("Codex turn identifier missing")?;
                        if turn.as_deref().is_some_and(|current| current != started) {
                            bail!("Codex started an unrelated turn");
                        }
                        turn = Some(started.into());
                    }
                    Some("item/agentMessage/delta") if corrections.is_empty() => {
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
                            params["turn"]["id"].as_str() == turn.as_deref() && turn.is_some(),
                            "Codex completed an unrelated turn"
                        );
                        events.finish_model(admission)?;
                        if !corrections.is_empty() {
                            anyhow::ensure!(
                                matches!(
                                    params["turn"]["status"].as_str(),
                                    Some("completed" | "interrupted")
                                ),
                                "Codex superseded turn failed"
                            );
                            completed = true;
                        } else {
                            return match params["turn"]["status"].as_str() {
                                Some("completed") => Ok(TurnEnd::Complete),
                                Some("interrupted") => Ok(TurnEnd::Cancelled),
                                _ => bail!("Codex turn failed"),
                            };
                        }
                    }
                    Some("thread/tokenUsage/updated") => {
                        if !corrections.is_empty()
                            || params["turnId"]
                                .as_str()
                                .is_some_and(|id| Some(id) != turn.as_deref())
                        {
                            continue;
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
        let result = super::process::stop_backend_and_services(
            &mut self.process,
            self.tools.stop_language_services(),
        )
        .await;
        self.relay = None;
        // Keep backend-owned context for the next prompt after cancellation.
        result
    }
}
