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

struct OpenAi {
    client: Client,
    endpoint: Url,
    model: String,
    key: String,
    history: Vec<Value>,
}

pub fn open(config: &Connection, workspace: &Path) -> Result<Box<dyn Session>> {
    if config.binary.is_some() {
        bail!("OpenAI API connections do not accept a backend executable");
    }
    let key = std::env::var("OPENAI_API_KEY")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .context("OPENAI_API_KEY is required for the API connection")?;
    Ok(Box::new(NativeSession::new(
        Box::new(OpenAi {
            client: http::client()?,
            endpoint: http::endpoint(
                config
                    .endpoint
                    .as_deref()
                    .unwrap_or("https://api.openai.com/v1/responses"),
            )?,
            model: config
                .model
                .clone()
                .context("select --model for the OpenAI API connection")?,
            key,
            history: Vec::new(),
        }),
        workspace,
    )?))
}

#[async_trait]
impl Model for OpenAi {
    fn prompt(&mut self, text: String) {
        self.history.push(json!({"role":"user", "content":text}));
    }

    fn results(&mut self, results: Vec<ToolResult>) {
        self.history.extend(results.into_iter().map(|result| {
            json!({
                "type":"function_call_output", "call_id":result.call_id,
                "output": serde_json::to_string(&result).expect("tool result serializes"),
            })
        }));
    }

    async fn response(&mut self, events: &EventSink) -> Result<Vec<ToolCall>> {
        let tools: Vec<Value> = crate::tools::definitions()
            .into_iter()
            .map(|tool| {
                json!({
                    "type":"function", "name":tool["name"], "description":tool["description"],
                    "parameters":tool["input_schema"], "strict":true,
                })
            })
            .collect();
        let request = self.client.post(self.endpoint.clone()).bearer_auth(&self.key)
            .json(&json!({"model":self.model,"input":self.history,"stream":true,"store":false,"tools":tools}));
        let stream = http::json_events(http::response(request).await?);
        tokio::pin!(stream);
        while let Some(event) = stream.next().await {
            let event = event?;
            match event["type"].as_str() {
                Some("response.output_text.delta") => {
                    events
                        .emit(Event::Text {
                            text: event["delta"]
                                .as_str()
                                .context("missing text delta")?
                                .to_owned(),
                        })
                        .await?
                }
                Some("response.completed") => {
                    let response = &event["response"];
                    let output = response["output"]
                        .as_array()
                        .context("missing response output")?;
                    let mut calls = Vec::new();
                    for item in output {
                        if item["type"] == "function_call" {
                            calls.push(ToolCall {
                                id: item["call_id"]
                                    .as_str()
                                    .context("missing OpenAI call ID")?
                                    .into(),
                                name: item["name"]
                                    .as_str()
                                    .context("missing OpenAI tool name")?
                                    .into(),
                                arguments: serde_json::from_str(
                                    item["arguments"]
                                        .as_str()
                                        .context("missing OpenAI tool arguments")?,
                                )?,
                            });
                        }
                    }
                    let usage = &response["usage"];
                    events
                        .emit(Event::Usage {
                            input: usage["input_tokens"].as_u64(),
                            output: usage["output_tokens"].as_u64(),
                            cached: usage["input_tokens_details"]["cached_tokens"].as_u64(),
                            cost_usd: None,
                        })
                        .await?;
                    // Publish calls to history only when response() can return
                    // them without another cancellation point.
                    self.history.extend(output.iter().cloned());
                    return Ok(calls);
                }
                Some("response.failed" | "response.incomplete" | "error") => {
                    bail!("OpenAI did not complete the response")
                }
                _ => {}
            }
        }
        bail!("OpenAI stream ended without a completed response")
    }
}
