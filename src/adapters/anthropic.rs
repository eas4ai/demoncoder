use super::http;
use crate::{
    config::Connection,
    events::{Event, EventSink},
    native::{Model, NativeSession},
    session::Session,
    tools::{ToolCall, ToolExecutor, ToolResult},
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
    effort: Option<String>,
    max_output_tokens: Option<u32>,
    history: Vec<Value>,
    definitions: Vec<Value>,
}

pub fn open(config: &Connection, workspace: &Path) -> Result<Box<dyn Session>> {
    if config.binary.is_some() {
        bail!("Anthropic API connections do not accept a backend executable");
    }
    config.validate()?;
    let key = config.api_key("ANTHROPIC_API_KEY")?;
    let tools = ToolExecutor::with_policy(workspace, &config.access)?;
    let definitions = tools.definitions();
    Ok(Box::new(NativeSession::with_tools(
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
            effort: config.effort.clone(),
            max_output_tokens: config.max_output_tokens,
            history: Vec::new(),
            definitions,
        }),
        tools,
    )))
}

impl Anthropic {
    async fn output_limit(&mut self) -> Result<u32> {
        if let Some(limit) = self.max_output_tokens {
            return Ok(limit);
        }
        let result = self.discover_output_limit().await;
        let limit = result.map_err(|error| anyhow::anyhow!(
            "Cannot determine the selected model's output limit: {error:#}. Set max_output_tokens in the connection or pass --max-output-tokens for an endpoint without model metadata."
        ))?;
        self.max_output_tokens = Some(limit);
        Ok(limit)
    }

    async fn discover_output_limit(&self) -> Result<u32> {
        let mut url = self.endpoint.clone();
        url.path_segments_mut()
            .map_err(|_| anyhow::anyhow!("provider endpoint cannot address model metadata"))?
            .pop_if_empty()
            .pop()
            .push("models")
            .push(&self.model);
        let request = self
            .client
            .get(url)
            .header("x-api-key", &self.key)
            .header("anthropic-version", "2023-06-01")
            .timeout(std::time::Duration::from_secs(30));
        let mut response = http::response(request).await?;
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| anyhow::anyhow!("model metadata transfer failed"))?
        {
            anyhow::ensure!(
                bytes.len() + chunk.len() <= 64 * 1024,
                "model metadata exceeds 64 KiB"
            );
            bytes.extend_from_slice(&chunk);
        }
        let metadata: Value = serde_json::from_slice(&bytes)
            .map_err(|_| anyhow::anyhow!("model metadata is not valid JSON"))?;
        metadata["max_tokens"]
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .filter(|value| *value > 0)
            .context("model metadata has no valid positive max_tokens value")
    }
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
        // A tool batch has one immediate result message, even when results
        // arrive individually or cancellation closes its remaining calls.
        if let Some(previous) = self.history.last_mut()
            && previous["role"] == "user"
            && let Some(blocks) = previous["content"].as_array_mut()
            && blocks.iter().all(|block| block["type"] == "tool_result")
        {
            blocks.extend(content);
        } else {
            self.history.push(json!({"role":"user", "content":content}));
        }
    }

    async fn response(&mut self, events: &EventSink) -> Result<Vec<ToolCall>> {
        let limit = self.output_limit().await?;
        let mut body = json!({
            "model":self.model,"messages":self.history,"stream":true,"max_tokens":limit,
            "tools":self.definitions.clone(),
        });
        if !self.definitions.is_empty() {
            body["system"] = super::CREATOR_INSTRUCTIONS.into();
        }
        if let Some(effort) = &self.effort {
            body["output_config"] = json!({"effort":effort});
        }
        let request = self
            .client
            .post(self.endpoint.clone())
            .header("x-api-key", &self.key)
            .header("anthropic-version", "2023-06-01")
            .json(&body);
        let stream = http::json_events(http::response(request).await?);
        tokio::pin!(stream);
        let mut blocks: Vec<Value> = Vec::new();
        let mut partial: Vec<String> = Vec::new();
        let (mut input, mut output, mut cached) = (None, None, None);
        let mut stop_reason = None;
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
                Some("message_delta") => {
                    if let Some(value) = event["usage"]["output_tokens"].as_u64() {
                        output = Some(value);
                    }
                    if let Some(reason) = event["delta"]["stop_reason"].as_str() {
                        stop_reason = Some(reason.to_owned());
                    }
                }
                Some("message_stop") => {
                    events
                        .emit(Event::Usage {
                            input,
                            output,
                            cached,
                            cost_usd: None,
                        })
                        .await?;
                    match stop_reason.as_deref() {
                        Some("max_tokens") => bail!(
                            "Anthropic response was truncated at the {limit}-token output limit; no tool calls from this response were executed. Request a smaller continuation or adjust an explicit max_output_tokens setting."
                        ),
                        Some("model_context_window_exceeded") => bail!(
                            "Anthropic response exceeded the model context window; no tool calls from this response were executed. Reduce conversation context before retrying."
                        ),
                        _ => {}
                    }
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
