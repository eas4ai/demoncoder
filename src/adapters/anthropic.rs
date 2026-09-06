use super::http;
use crate::{
    config::Connection,
    events::{Event, EventSink},
    session::{Command, Session, TurnEnd},
};
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::{Client, Url};
use serde_json::{Value, json};
use std::path::Path;
use tokio::sync::mpsc;

struct Anthropic {
    client: Client,
    endpoint: Url,
    model: String,
    key: String,
    history: Vec<Value>,
}

pub fn open(config: &Connection, _: &Path) -> Result<Box<dyn Session>> {
    if config.binary.is_some() {
        bail!("Anthropic API connections do not accept a backend executable");
    }
    let key = std::env::var("ANTHROPIC_API_KEY")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .context("ANTHROPIC_API_KEY is required for the API connection")?;
    Ok(Box::new(Anthropic {
        client: http::client()?,
        endpoint: http::endpoint(
            config
                .endpoint
                .as_deref()
                .unwrap_or("https://api.anthropic.com/v1/messages"),
        )?,
        model: config
            .model
            .clone()
            .context("select --model for the Anthropic API connection")?,
        key,
        history: Vec::new(),
    }))
}

#[async_trait]
impl Session for Anthropic {
    fn owner(&self) -> &'static str {
        "demoncoder"
    }

    async fn turn(
        &mut self,
        prompt: String,
        commands: &mut mpsc::Receiver<Command>,
        events: &EventSink,
    ) -> Result<TurnEnd> {
        self.history.push(json!({"role":"user","content":prompt}));
        let request = self
            .client
            .post(self.endpoint.clone())
            .header("x-api-key", &self.key)
            .header("anthropic-version", "2023-06-01")
            .json(&json!({
                "model":self.model,"messages":self.history,"stream":true,"max_tokens":4096,
            }));
        let run = async {
            let stream = http::json_events(http::response(request).await?);
            tokio::pin!(stream);
            let mut text = String::new();
            let (mut input, mut output, mut cached) = (None, None, None);
            while let Some(event) = stream.next().await {
                let event = event?;
                match event["type"].as_str() {
                    Some("message_start") => {
                        let usage = &event["message"]["usage"];
                        input = usage["input_tokens"].as_u64();
                        cached = usage["cache_read_input_tokens"].as_u64();
                    }
                    Some("content_block_delta") if event["delta"]["type"] == "text_delta" => {
                        let delta = event["delta"]["text"]
                            .as_str()
                            .context("missing text delta")?;
                        text.push_str(delta);
                        events
                            .emit(Event::Text {
                                text: delta.to_owned(),
                            })
                            .await?;
                    }
                    Some("message_delta") => {
                        output = event["usage"]["output_tokens"].as_u64();
                    }
                    Some("message_stop") => {
                        self.history
                            .push(json!({"role":"assistant","content":text}));
                        events
                            .emit(Event::Usage {
                                input,
                                output,
                                cached,
                                cost_usd: None,
                            })
                            .await?;
                        return Ok(TurnEnd::Complete);
                    }
                    Some("error") => bail!("Anthropic returned a stream error"),
                    _ => {}
                }
            }
            bail!("Anthropic stream ended without message_stop")
        };
        tokio::pin!(run);
        loop {
            tokio::select! {
                biased;
                command = commands.recv() => match command {
                    Some(Command::Cancel) => return Ok(TurnEnd::Cancelled),
                    Some(Command::Shutdown) | None => return Ok(TurnEnd::Shutdown),
                    Some(Command::Prompt(_)) => events.emit(Event::Error { message: "steering is not implemented for this connection yet".into() }).await?,
                },
                result = &mut run => return result,
            }
        }
    }
}
