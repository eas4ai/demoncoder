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

struct OpenAi {
    client: Client,
    endpoint: Url,
    model: String,
    key: String,
    history: Vec<Value>,
}

pub fn open(config: &Connection, _: &Path) -> Result<Box<dyn Session>> {
    if config.binary.is_some() {
        bail!("OpenAI API connections do not accept a backend executable");
    }
    let key = std::env::var("OPENAI_API_KEY")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .context("OPENAI_API_KEY is required for the API connection")?;
    Ok(Box::new(OpenAi {
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
    }))
}

#[async_trait]
impl Session for OpenAi {
    fn owner(&self) -> &'static str {
        "demoncoder"
    }

    async fn turn(
        &mut self,
        prompt: String,
        commands: &mut mpsc::Receiver<Command>,
        events: &EventSink,
    ) -> Result<TurnEnd> {
        self.history.push(json!({"role":"user", "content":prompt}));
        let request = self
            .client
            .post(self.endpoint.clone())
            .bearer_auth(&self.key)
            .json(&json!({
                "model":self.model,"input":self.history,"stream":true,"store":false,
            }));
        let run = async {
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
                        self.history.extend(output.iter().cloned());
                        let usage = &response["usage"];
                        events
                            .emit(Event::Usage {
                                input: usage["input_tokens"].as_u64(),
                                output: usage["output_tokens"].as_u64(),
                                cached: usage["input_tokens_details"]["cached_tokens"].as_u64(),
                                cost_usd: None,
                            })
                            .await?;
                        return Ok(TurnEnd::Complete);
                    }
                    Some("response.failed" | "response.incomplete" | "error") => {
                        bail!("OpenAI did not complete the response")
                    }
                    _ => {}
                }
            }
            bail!("OpenAI stream ended without a completed response")
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
