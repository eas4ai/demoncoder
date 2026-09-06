use super::process::{BackendProcess, executable};
use crate::{
    config::Connection,
    events::{Event, EventSink},
    session::{Command, Session, TurnEnd},
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
    process: Option<BackendProcess>,
    thread: Option<String>,
    next_id: u64,
}

pub fn open(config: &Connection, workspace: &Path) -> Result<Box<dyn Session>> {
    if config.endpoint.is_some() {
        bail!("Codex subscription connections use the app-server transport, not an API endpoint");
    }
    Ok(Box::new(Codex {
        binary: executable(config.binary.as_deref(), "codex")?,
        workspace: workspace.to_owned(),
        model: config.model.clone(),
        process: None,
        thread: None,
        next_id: 1,
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
                if message["id"] == id {
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
        ] {
            args.extend(["--disable".into(), feature.into()]);
        }
        for setting in [
            "mcp_servers={}",
            "web_search=\"disabled\"",
            "forced_login_method=\"chatgpt\"",
        ] {
            args.extend(["-c".into(), setting.into()]);
        }
        self.process = Some(BackendProcess::spawn(
            &self.binary,
            &args,
            &self.workspace,
            &["CODEX_HOME"],
        )?);
        self.rpc(
            "initialize",
            json!({"clientInfo":{"name":"demoncoder","version":env!("CARGO_PKG_VERSION")}}),
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
        if account["account"]["type"] != "chatgpt" {
            bail!("Codex subscription connection requires a ChatGPT login; run codex login");
        }
        let thread = self.rpc("thread/start", json!({
            "model":self.model,"cwd":self.workspace,"sandbox":"read-only","approvalPolicy":"never",
            "experimentalRawEvents":false,
        })).await?;
        self.thread = Some(
            thread["thread"]["id"]
                .as_str()
                .context("Codex did not return a thread identifier")?
                .into(),
        );
        Ok(())
    }

    async fn run_turn(&mut self, prompt: String, events: &EventSink) -> Result<TurnEnd> {
        self.connect().await?;
        let id = self.next_id;
        self.next_id += 1;
        let process = self.process.as_mut().context("Codex process unavailable")?;
        process
            .send(json!({"id":id,"method":"turn/start","params":{
                "threadId":self.thread,"input":[{"type":"text","text":prompt}],
            }}))
            .await?;
        let mut turn: Option<String> = None;
        loop {
            let message = process.receive().await?;
            if message["id"] == id && message.get("error").is_some() {
                bail!("Codex rejected the turn");
            }
            if message["id"] == id {
                turn = message["result"]["turn"]["id"].as_str().map(str::to_owned);
            }
            let params = &message["params"];
            if let Some(thread) = params["threadId"].as_str()
                && Some(thread) != self.thread.as_deref()
            {
                bail!("Codex event belongs to a different thread");
            }
            match message["method"].as_str() {
                Some("turn/started") => {
                    let started = params["turn"]["id"]
                        .as_str()
                        .context("Codex turn identifier missing")?;
                    if turn.as_deref().is_some_and(|current| current != started) {
                        bail!("Codex started an unrelated turn");
                    }
                    turn = Some(started.into());
                }
                Some("item/agentMessage/delta") => {
                    events
                        .emit(Event::Text {
                            text: params["delta"]
                                .as_str()
                                .context("Codex text delta missing")?
                                .into(),
                        })
                        .await?
                }
                Some("turn/completed") => {
                    if params["turn"]["id"].as_str() != turn.as_deref() || turn.is_none() {
                        bail!("Codex completed an unrelated turn");
                    }
                    return match params["turn"]["status"].as_str() {
                        Some("completed") => Ok(TurnEnd::Complete),
                        Some("interrupted") => Ok(TurnEnd::Cancelled),
                        _ => bail!("Codex turn failed"),
                    };
                }
                Some("thread/tokenUsage/updated") => {
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
        // Reattachment after cancellation is introduced with the session-continuation contract.
        self.thread = None;
        Ok(())
    }
}
