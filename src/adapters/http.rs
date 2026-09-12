use std::{net::IpAddr, time::Duration};

use anyhow::{Context, Result, bail};
use futures_util::{Stream, StreamExt};
use reqwest::{Client, Response, Url};
use serde_json::Value;

pub fn endpoint(value: &str) -> Result<Url> {
    let url = Url::parse(value).map_err(|_| anyhow::anyhow!("invalid provider endpoint"))?;
    let loopback = url
        .host_str()
        .and_then(|s| s.parse::<IpAddr>().ok())
        .is_some_and(|ip| ip.is_loopback());
    if !(url.scheme() == "https" || (url.scheme() == "http" && loopback))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        bail!(
            "provider endpoint must use HTTPS (or a literal loopback HTTP address) without embedded credentials, query, or fragment"
        );
    }
    Ok(url)
}

pub fn client() -> Result<Client> {
    Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(120))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .context("initialize HTTPS client")
}

pub async fn response(request: reqwest::RequestBuilder) -> Result<Response> {
    let response = request
        .send()
        .await
        .map_err(|_| anyhow::anyhow!("provider request failed; check connection and endpoint"))?;
    successful_response(response)
}

/// Build locally before attributing transport/status errors to the provider.
/// Never retain reqwest's request-builder error: it can contain credentials.
pub async fn provider_response(request: reqwest::RequestBuilder) -> Result<Response> {
    let (client, request) = request.build_split();
    let request =
        request.map_err(|_| anyhow::anyhow!("provider request configuration is invalid"))?;
    let response = client.execute(request).await.map_err(|error| {
        if error.is_builder() {
            anyhow::anyhow!("provider request configuration is invalid")
        } else {
            crate::native::provider_response_failure(anyhow::anyhow!(
                "provider request failed; check connection and endpoint"
            ))
        }
    })?;
    successful_response(response).map_err(crate::native::provider_response_failure)
}

fn successful_response(response: Response) -> Result<Response> {
    if !response.status().is_success() {
        // Never echo error bodies or headers: an endpoint may reflect credentials.
        bail!(
            "provider returned HTTP {}; no authentication fallback was attempted",
            response.status().as_u16()
        );
    }
    Ok(response)
}

/// Decode SSE at byte boundaries so split UTF-8 code points are preserved.
pub fn json_events(response: Response) -> impl Stream<Item = Result<Value>> {
    futures_util::stream::try_unfold(
        (response.bytes_stream(), Vec::new(), String::new()),
        |(mut stream, mut bytes, mut data)| async move {
            loop {
                while let Some(end) = bytes.iter().position(|b| *b == b'\n') {
                    let line: Vec<_> = bytes.drain(..=end).collect();
                    let line = std::str::from_utf8(&line)
                        .context("provider stream is not UTF-8")?
                        .trim_end_matches(['\r', '\n']);
                    if line.is_empty() && !data.is_empty() {
                        let item = std::mem::take(&mut data);
                        if item.trim() == "[DONE]" {
                            continue;
                        }
                        let value =
                            serde_json::from_str(&item).context("invalid provider stream event")?;
                        return Ok(Some((value, (stream, bytes, data))));
                    }
                    if let Some(value) = line.strip_prefix("data:") {
                        if !data.is_empty() {
                            data.push('\n');
                        }
                        data.push_str(value.strip_prefix(' ').unwrap_or(value));
                    }
                }
                if bytes.len() + data.len() > 4 * 1024 * 1024 {
                    bail!("provider event exceeds 4 MiB");
                }
                match stream.next().await {
                    Some(Ok(chunk)) => bytes.extend_from_slice(&chunk),
                    Some(Err(_)) => bail!("provider stream was interrupted"),
                    None if bytes.is_empty() && data.is_empty() => return Ok(None),
                    None => bail!("provider stream ended inside an event"),
                }
            }
        },
    )
}
