//! Exact admitted Streamable HTTP authority; no replay, redirects or GET recovery.
use super::protocol::{self, Message};
use crate::plugins::runners::HttpConfig;
use anyhow::{Result, ensure};
use reqwest::{
    Client, Url,
    header::{HeaderMap, HeaderValue},
};
use serde_json::Value;
use std::time::Duration;

pub(super) struct Http {
    client: Client,
    url: Url,
    headers: HeaderMap,
    session: Option<String>,
    secrets: protocol::Secrets,
}
impl Http {
    pub(super) fn new(config: &HttpConfig, secrets: protocol::Secrets) -> Result<Self> {
        let (url, headers) = config.validate()?;
        ensure!(
            ![
                "accept",
                "mcp-protocol-version",
                "mcp-session-id",
                "last-event-id"
            ]
            .iter()
            .any(|name| headers.contains_key(*name)),
            "MCP header overrides protocol authority"
        );
        let client = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .no_gzip()
            .no_brotli()
            .no_deflate()
            .no_zstd()
            .timeout(Duration::from_millis(config.timeout_ms))
            .connect_timeout(Duration::from_millis(config.timeout_ms))
            .build()
            .map_err(|_| anyhow::anyhow!("MCP HTTP client initialization failed"))?;
        Ok(Self {
            client,
            url,
            headers,
            session: None,
            secrets,
        })
    }
    fn request(&self, method: reqwest::Method) -> reqwest::RequestBuilder {
        let mut request = self
            .client
            .request(method, self.url.clone())
            .headers(self.headers.clone())
            .header("accept", "application/json, text/event-stream")
            .header("mcp-protocol-version", protocol::VERSION);
        if let Some(session) = &self.session {
            let mut header = HeaderValue::from_str(session).expect("validated session header");
            header.set_sensitive(true);
            request = request.header("mcp-session-id", header);
        }
        request
    }
    pub(super) async fn send(&mut self, message: &Value, expected: Option<u64>) -> Result<Value> {
        let initialized = message.get("method").and_then(Value::as_str) == Some("initialize");
        let bytes = protocol::encode(message)?;
        let mut response = self
            .request(reqwest::Method::POST)
            .header("content-type", "application/json")
            .body(bytes)
            .send()
            .await
            .map_err(|_| anyhow::anyhow!("MCP HTTP transport failed; effects may be unknown"))?;
        ensure!(
            response.headers().get_all("mcp-session-id").iter().count() <= 1,
            "MCP ambiguous session identity"
        );
        if let Some(session) = response.headers().get("mcp-session-id") {
            let session = session
                .to_str()
                .map_err(|_| anyhow::anyhow!("MCP invalid session ID"))?;
            ensure!(
                !session.is_empty()
                    && session.len() <= 256
                    && session.bytes().all(|b| (0x21..=0x7e).contains(&b)),
                "MCP invalid session ID"
            );
            ensure!(
                initialized || self.session.as_deref() == Some(session),
                "MCP session identity changed"
            );
            if initialized {
                self.session = Some(session.into());
                self.secrets
                    .lock()
                    .map_err(|_| anyhow::anyhow!("MCP secret owner failed"))?
                    .push(session.into());
            }
        }
        let status = response.status().as_u16();
        ensure!(
            if expected.is_some() {
                status == 200
            } else {
                status == 202
            },
            "MCP unexpected HTTP response status"
        );
        ensure!(
            response.headers().get("content-encoding").is_none(),
            "MCP compressed response is unsupported"
        );
        let content = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_owned();
        ensure!(
            expected.is_none()
                || ["application/json", "text/event-stream"].contains(&content.as_str()),
            "MCP invalid response media type"
        );
        let maximum = if content == "text/event-stream" {
            protocol::MAX_MESSAGE * 4
        } else {
            protocol::MAX_MESSAGE
        };
        ensure!(
            response
                .content_length()
                .is_none_or(|n| n <= maximum as u64),
            "MCP response exceeds bound"
        );
        let mut body = Vec::new();
        let mut sse = Frames::default();
        let mut result = None;
        let mut messages = 0;
        let mut total = 0usize;
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| anyhow::anyhow!("MCP HTTP response interrupted; effects may be unknown"))?
        {
            total = total.saturating_add(chunk.len());
            ensure!(total <= maximum, "MCP aggregate response exceeds bound");
            if content == "text/event-stream" {
                for frame in sse.push(&chunk)? {
                    messages += 1;
                    ensure!(
                        messages <= protocol::MAX_MESSAGES,
                        "MCP message count exceeds bound"
                    );
                    self.check_secrets(&frame)?;
                    match protocol::parse(&frame, expected.unwrap_or(0))? {
                        Message::Result(value) => {
                            ensure!(result.is_none(), "MCP duplicate response ID");
                            result = Some(value);
                        }
                        Message::Notification { tools_changed } => {
                            ensure!(
                                !tools_changed,
                                "MCP tool catalog changed; readmission required"
                            );
                        }
                        Message::Request { id, ping } => {
                            self.reject_or_ping(id, ping).await?;
                            ensure!(ping, "MCP server requested an unadvertised host capability");
                        }
                    }
                }
            } else {
                body.extend_from_slice(&chunk);
            }
        }
        if expected.is_none() {
            ensure!(total == 0, "MCP accepted notification has a response body");
            return Ok(serde_json::json!({}));
        }
        if content == "text/event-stream" {
            sse.finish()?;
            result.ok_or_else(|| {
                anyhow::anyhow!("MCP stream ended without response; effects may be unknown")
            })
        } else {
            self.check_secrets(&body)?;
            match protocol::parse(&body, expected.expect("checked ID"))? {
                Message::Result(value) => Ok(value),
                _ => anyhow::bail!("MCP JSON body did not contain the request response"),
            }
        }
    }
    async fn reject_or_ping(&self, id: Value, ping: bool) -> Result<()> {
        let mut response = self
            .request(reqwest::Method::POST)
            .header("content-type", "application/json")
            .body(protocol::encode(&protocol::reply(id, ping))?)
            .send()
            .await
            .map_err(|_| anyhow::anyhow!("MCP server request reply failed"))?;
        ensure!(
            response.status().as_u16() == 202,
            "MCP server request reply was rejected"
        );
        ensure!(
            response
                .chunk()
                .await
                .map_err(|_| anyhow::anyhow!("MCP reply transport failed"))?
                .is_none(),
            "MCP reply acceptance has a body"
        );
        Ok(())
    }
    pub(super) fn check_secrets(&self, bytes: &[u8]) -> Result<()> {
        super::protocol::check_secrets(
            bytes,
            &self
                .secrets
                .lock()
                .map_err(|_| anyhow::anyhow!("MCP secret owner failed"))?,
        )
    }
    pub(super) async fn close(&self) -> Result<()> {
        if self.session.is_some() {
            let response = self
                .request(reqwest::Method::DELETE)
                .send()
                .await
                .map_err(|_| anyhow::anyhow!("MCP session teardown failed"))?;
            ensure!(
                matches!(response.status().as_u16(), 200 | 202 | 204 | 404 | 405),
                "MCP session teardown was rejected"
            );
        }
        Ok(())
    }
}

#[derive(Default)]
struct Frames {
    pending: Vec<u8>,
    data: Vec<u8>,
    event: Option<String>,
}
impl Frames {
    fn push(&mut self, bytes: &[u8]) -> Result<Vec<Vec<u8>>> {
        let mut frames = Vec::new();
        for byte in bytes {
            ensure!(
                self.pending.len() + self.data.len() < protocol::MAX_MESSAGE,
                "MCP SSE frame exceeds bound"
            );
            if *byte != b'\n' {
                self.pending.push(*byte);
                continue;
            }
            if self.pending.last() == Some(&b'\r') {
                self.pending.pop();
            }
            let line = std::str::from_utf8(&self.pending)
                .map_err(|_| anyhow::anyhow!("MCP SSE invalid encoding"))?;
            if line.is_empty() {
                if !self.data.is_empty() {
                    ensure!(
                        self.event.as_deref().is_none_or(|e| e == "message"),
                        "MCP legacy or unknown SSE event"
                    );
                    self.data.pop();
                    ensure!(
                        frames.len() < protocol::MAX_MESSAGES,
                        "MCP SSE message count exceeds bound"
                    );
                    frames.push(std::mem::take(&mut self.data));
                }
                self.event = None;
            } else if !line.starts_with(':') {
                let (field, value) = line.split_once(':').unwrap_or((line, ""));
                let value = value.strip_prefix(' ').unwrap_or(value);
                match field {
                    "data" => {
                        self.data.extend_from_slice(value.as_bytes());
                        self.data.push(b'\n');
                    }
                    "event" => self.event = Some(value.into()),
                    // Resumption is intentionally disabled; these cannot authorize traffic.
                    "id" | "retry" => {}
                    _ => {}
                }
            }
            self.pending.clear();
        }
        Ok(frames)
    }
    fn finish(&self) -> Result<()> {
        ensure!(
            self.pending.is_empty() && self.data.is_empty(),
            "MCP incomplete SSE frame"
        );
        Ok(())
    }
}
