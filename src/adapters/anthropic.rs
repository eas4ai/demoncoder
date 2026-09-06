use super::http;
use crate::{
    config::Connection,
    events::{Event, EventSink},
    native::{Model, NativeSession},
    session::Session,
    tools::{ToolCall, ToolResult},
};
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::{Client, Url};
use serde_json::{Value, json};
use std::path::Path;

struct Anthropic {
    client: Client,
    endpoint: Url,
    model: String,
    key: String,
    history: Vec<Value>,
}

pub fn open(config: &Connection, workspace: &Path) -> Result<Box<dyn Session>> {
    if config.binary.is_some() {
        bail!("Anthropic API connections do not accept a backend executable");
    }
    let key = std::env::var("ANTHROPIC_API_KEY")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .context("ANTHROPIC_API_KEY is required for the API connection")?;
    Ok(Box::new(NativeSession::new(
        Box::new(Anthropic {
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
        }),
        workspace,
    )?))
}

#[async_trait]
impl Model for Anthropic {
    fn prompt(&mut self, text: String) {
        self.history.push(json!({"role":"user", "content":text}));
    }

    fn results(&mut self, results: Vec<ToolResult>) {
        let content: Vec<Value> = results
            .into_iter()
            .map(|result| {
                json!({
                    "type":"tool_result", "tool_use_id":result.call_id, "is_error": !result.success,
                    "content":serde_json::to_string(&result).expect("tool result serializes"),
                })
            })
            .collect();
        self.history.push(json!({"role":"user", "content":content}));
    }

    async fn response(&mut self, events: &EventSink) -> Result<Vec<ToolCall>> {
        let request = self
            .client
            .post(self.endpoint.clone())
            .header("x-api-key", &self.key)
            .header("anthropic-version", "2023-06-01")
            .json(&json!({
                "model":self.model,"messages":self.history,"stream":true,"max_tokens":4096,
                "tools":crate::tools::definitions(),
            }));
        let stream = http::json_events(http::response(request).await?);
        tokio::pin!(stream);
        let mut blocks: Vec<Value> = Vec::new();
        let mut partial: Vec<String> = Vec::new();
        let (mut input, mut output, mut cached) = (None, None, None);
        while let Some(event) = stream.next().await {
            let event = event?;
            match event["type"].as_str() {
                Some("message_start") => {
                    let usage = &event["message"]["usage"];
                    input = usage["input_tokens"].as_u64();
                    cached = usage["cache_read_input_tokens"].as_u64();
                }
                Some("content_block_start") => {
                    let index = event["index"]
                        .as_u64()
                        .context("missing content block index")?
                        as usize;
                    anyhow::ensure!(
                        index == blocks.len(),
                        "out of order Anthropic content block"
                    );
                    blocks.push(event["content_block"].clone());
                    partial.push(String::new());
                }
                Some("content_block_delta") => {
                    let index = event["index"].as_u64().unwrap_or(0) as usize;
                    let delta = &event["delta"];
                    // Some compatible text streams omit the empty block-start event.
                    if blocks.is_empty() && index == 0 && delta["type"] == "text_delta" {
                        blocks.push(json!({"type":"text", "text":""}));
                        partial.push(String::new());
                    }
                    let block = blocks
                        .get_mut(index)
                        .context("unknown Anthropic content block")?;
                    match delta["type"].as_str() {
                        Some("text_delta") => {
                            let text = delta["text"].as_str().context("missing text delta")?;
                            append(block, "text", text)?;
                            events.emit(Event::Text { text: text.into() }).await?;
                        }
                        Some("input_json_delta") => partial[index].push_str(
                            delta["partial_json"]
                                .as_str()
                                .context("missing tool input delta")?,
                        ),
                        Some("thinking_delta") => append(
                            block,
                            "thinking",
                            delta["thinking"]
                                .as_str()
                                .context("missing thinking delta")?,
                        )?,
                        Some("signature_delta") => append(
                            block,
                            "signature",
                            delta["signature"]
                                .as_str()
                                .context("missing signature delta")?,
                        )?,
                        _ => {}
                    }
                }
                Some("message_delta") => output = event["usage"]["output_tokens"].as_u64(),
                Some("message_stop") => {
                    let mut calls = Vec::new();
                    for (block, partial) in blocks.iter_mut().zip(partial) {
                        if block["type"] == "tool_use" {
                            if !partial.is_empty() {
                                block["input"] = serde_json::from_str(&partial)?;
                            }
                            calls.push(ToolCall {
                                id: block["id"]
                                    .as_str()
                                    .context("missing Anthropic call ID")?
                                    .into(),
                                name: block["name"]
                                    .as_str()
                                    .context("missing Anthropic tool name")?
                                    .into(),
                                arguments: block["input"].clone(),
                            });
                        }
                    }
                    self.history
                        .push(json!({"role":"assistant", "content":blocks}));
                    events
                        .emit(Event::Usage {
                            input,
                            output,
                            cached,
                            cost_usd: None,
                        })
                        .await?;
                    return Ok(calls);
                }
                Some("error") => bail!("Anthropic returned a stream error"),
                _ => {}
            }
        }
        bail!("Anthropic stream ended without message_stop")
    }
}

fn append(block: &mut Value, field: &str, delta: &str) -> Result<()> {
    let current = block[field].as_str().unwrap_or("");
    anyhow::ensure!(
        current.len() + delta.len() <= 4 * 1024 * 1024,
        "Anthropic block exceeds 4 MiB"
    );
    block[field] = Value::String(format!("{current}{delta}"));
    Ok(())
}
