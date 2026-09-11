//! Explicit host-bound HTTP authority under the existing admitted hook owner.
use super::event::{self, EventInput, LimitedInput};
use crate::plugins::{
    Dialect, Package, SourceValidity,
    dispatch::{Declaration, HookInvocation, HookRunner, Registration},
    hook_types::{HandlerKind, HookDialect, HookEvent},
    profile::CompatibilityProfile,
    receipts::{DeclarationIdentity, HandlerClass, RawOutcome},
};
use anyhow::{Result, ensure};
use reqwest::{
    Client, Url,
    header::{HeaderMap, HeaderName, HeaderValue},
};
use serde::Serialize;
use std::{collections::BTreeMap, sync::Arc, time::Duration};

/// Secret material is distinct from ordinary literal header data. Prefix is
/// framing (for example `Bearer `); reflection checks protect the secret itself.
#[derive(Clone, Serialize)]
pub struct HttpCredential {
    pub prefix: String,
    pub secret: String,
}
impl HttpCredential {
    pub fn bearer(secret: String) -> Self {
        Self {
            prefix: "Bearer ".into(),
            secret,
        }
    }
}
/// Trusted host binding, never constructed from hook input or response.
/// Deliberately has no Debug implementation: endpoints and headers may be private.
#[derive(Clone, Serialize)]
pub struct HttpConfig {
    pub endpoint: String,
    pub headers: BTreeMap<String, String>,
    pub credentials: BTreeMap<String, HttpCredential>,
    pub timeout_ms: u64,
    pub max_input_bytes: usize,
    pub max_output_bytes: usize,
    pub permission_mode: String,
    pub transcript_path: Option<String>,
}
impl HttpConfig {
    pub fn new(endpoint: String) -> Self {
        Self {
            endpoint,
            headers: BTreeMap::new(),
            credentials: BTreeMap::new(),
            timeout_ms: 10_000,
            max_input_bytes: 65536,
            max_output_bytes: 28 * 1024,
            permission_mode: "default".into(),
            transcript_path: None,
        }
    }
    pub(crate) fn validate(&self) -> Result<(Url, HeaderMap)> {
        ensure!(
            (1..=20_000).contains(&self.timeout_ms)
                && (1..=65536).contains(&self.max_input_bytes)
                && (1..=28 * 1024).contains(&self.max_output_bytes),
            "HTTP hook limits are invalid"
        );
        ensure!(
            self.endpoint.len() <= 8192
                && !self
                    .endpoint
                    .chars()
                    .any(|c| c.is_whitespace() || c.is_control() || c == '\\')
                && self.endpoint.split_once("://").is_some_and(|(_, rest)| rest
                    .split(['/', '?', '#'])
                    .next()
                    .is_some_and(|authority| !authority.is_empty() && !authority.contains('@'))),
            "HTTP hook endpoint is invalid"
        );
        let url = Url::parse(&self.endpoint)
            .map_err(|_| anyhow::anyhow!("HTTP hook endpoint is invalid"))?;
        ensure!(
            matches!(url.scheme(), "http" | "https")
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none()
                && url.fragment().is_none(),
            "HTTP hook endpoint authority is invalid"
        );
        ensure!(
            self.headers.len().saturating_add(self.credentials.len()) <= 64,
            "HTTP hook headers exceed bounds"
        );
        let mut headers = HeaderMap::new();
        for (name, value) in &self.headers {
            insert_header(&mut headers, name, value, false)?;
        }
        for (name, credential) in &self.credentials {
            ensure!(
                !credential.secret.trim().is_empty()
                    && credential.secret.len() <= 8192
                    && credential.prefix.len() <= 128,
                "HTTP hook credential is invalid"
            );
            insert_header(
                &mut headers,
                name,
                &format!("{}{}", credential.prefix, credential.secret),
                true,
            )?;
        }
        ensure!(
            [
                "default",
                "acceptEdits",
                "plan",
                "dontAsk",
                "bypassPermissions"
            ]
            .contains(&self.permission_mode.as_str())
                && self
                    .transcript_path
                    .as_ref()
                    .is_none_or(|s| s.len() <= 16384 && !s.contains('\0')),
            "HTTP hook source framing is invalid"
        );
        let mut encoded = LimitedInput {
            bytes: Vec::new(),
            maximum: 32768,
        };
        serde_json::to_writer(&mut encoded, self)
            .map_err(|_| anyhow::anyhow!("HTTP hook configuration exceeds bounds"))?;
        Ok((url, headers))
    }
}
fn insert_header(headers: &mut HeaderMap, name: &str, value: &str, secret: bool) -> Result<()> {
    ensure!(
        name.len() <= 128 && value.len() <= 16384,
        "HTTP hook header exceeds bounds"
    );
    let name = HeaderName::from_bytes(name.as_bytes())
        .map_err(|_| anyhow::anyhow!("HTTP hook header name is invalid"))?;
    ensure!(
        ![
            "host",
            "content-length",
            "transfer-encoding",
            "connection",
            "trailer",
            "te",
            "upgrade",
            "expect",
            "content-type",
            "content-encoding",
            "accept-encoding",
            "proxy-authorization",
            "proxy-connection"
        ]
        .contains(&name.as_str())
            && !headers.contains_key(&name),
        "HTTP hook header overrides host framing or another binding"
    );
    ensure!(
        secret || !["authorization", "cookie"].contains(&name.as_str()),
        "HTTP hook authentication requires a credential binding"
    );
    let mut value = HeaderValue::from_str(value)
        .map_err(|_| anyhow::anyhow!("HTTP hook header value is invalid"))?;
    value.set_sensitive(secret);
    headers.insert(name, value);
    Ok(())
}
pub struct HttpRunner {
    event: HookEvent,
    identity: DeclarationIdentity,
    class: HandlerClass,
    endpoint_identity: Option<String>,
    config: HttpConfig,
    url: Url,
    headers: HeaderMap,
    profile: Arc<CompatibilityProfile>,
    secrets: Arc<Vec<String>>,
}
impl HttpRunner {
    pub fn registration(
        package: Arc<Package>,
        declaration: Declaration,
        config: HttpConfig,
        revalidation: Option<HttpConfig>,
    ) -> Result<Registration> {
        Self::registration_for_event(
            package,
            declaration,
            HookEvent::PreToolUse,
            config,
            revalidation,
        )
    }
    pub fn registration_for_event(
        package: Arc<Package>,
        mut declaration: Declaration,
        event: HookEvent,
        config: HttpConfig,
        revalidation: Option<HttpConfig>,
    ) -> Result<Registration> {
        ensure!(
            package.source_validity() == SourceValidity::Valid,
            "HTTP package has invalid or unvalidated components"
        );
        ensure!(
            declaration.identity.runner == HandlerKind::Http,
            "HTTP registration has a different runner kind"
        );
        let dialect = declaration.identity.dialect;
        ensure!(
            dialect == HookDialect::Native
                || (dialect == HookDialect::Claude && package.dialect() == Dialect::Claude),
            "HTTP source dialect differs or is nonexecuting"
        );
        let profile = Arc::new(CompatibilityProfile::embedded()?);
        profile.require_runner(dialect, event, HandlerKind::Http)?;
        let (url, headers) = config.validate()?;
        ensure!(
            declaration.read_only_endpoint.is_some() == revalidation.is_some(),
            "HTTP revalidation requires an explicit read-only binding"
        );
        ensure!(
            declaration
                .read_only_endpoint
                .as_ref()
                .is_none_or(|id| !id.is_empty()
                    && id.len() <= 128
                    && id
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))),
            "HTTP revalidation identity is invalid"
        );
        let read_only = revalidation
            .as_ref()
            .map(HttpConfig::validate)
            .transpose()?;
        if let Some((read_url, _)) = &read_only {
            ensure!(
                read_url != &url,
                "HTTP revalidation must bind a separate read-only endpoint"
            );
        }
        declaration.identity.package = package.name().into();
        declaration.bind_package_source(&package)?;
        declaration.identity.code = package.digest().into();
        declaration.identity.configuration = crate::plugins::admission::digest(&(
            event,
            &config,
            &revalidation,
            &declaration.read_only_endpoint,
        ))?;
        // Retention protects both admitted endpoints' credentials without granting
        // either endpoint the other's request headers.
        let secrets: Arc<Vec<String>> = Arc::new(
            config
                .credentials
                .values()
                .chain(revalidation.iter().flat_map(|c| c.credentials.values()))
                .map(|c| c.secret.clone())
                .collect(),
        );
        let runner = Arc::new(Self {
            event,
            identity: declaration.identity.clone(),
            class: declaration.class,
            endpoint_identity: None,
            config,
            url,
            headers,
            profile: profile.clone(),
            secrets: secrets.clone(),
        });
        let revalidation = revalidation.zip(read_only).map(|(config, (url, headers))| {
            Arc::new(Self {
                event,
                identity: declaration.identity.clone(),
                class: HandlerClass::DecisionGate,
                endpoint_identity: declaration.read_only_endpoint.clone(),
                config,
                url,
                headers,
                profile,
                secrets,
            }) as Arc<dyn HookRunner>
        });
        Ok(Registration {
            declaration,
            runner,
            revalidation,
        })
    }
    async fn exchange(&self, input: Vec<u8>) -> Result<RawOutcome> {
        // A fresh client owns no cached connection or cookies from another hook.
        // Compression stays disabled even if another dependency enables a feature.
        let client = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .no_gzip()
            .no_brotli()
            .no_deflate()
            .no_zstd()
            .connect_timeout(Duration::from_millis(self.config.timeout_ms))
            .timeout(Duration::from_millis(self.config.timeout_ms))
            .build()
            .map_err(|_| anyhow::anyhow!("HTTP hook client initialization failed"))?;
        let mut response = client
            .post(self.url.clone())
            .headers(self.headers.clone())
            .header("content-type", "application/json")
            .body(input)
            .send()
            .await
            .map_err(|_| {
                anyhow::anyhow!("HTTP hook transport failed; remote effects may be unknown")
            })?;
        let status = response.status().as_u16();
        ensure!(
            (200..300).contains(&status),
            "HTTP hook returned status {status}; remote effects may be unknown"
        );
        ensure!(
            response
                .content_length()
                .is_none_or(|n| n <= self.config.max_output_bytes as u64),
            "HTTP hook response exceeds bound"
        );
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| anyhow::anyhow!("HTTP hook response transport failed"))?
        {
            ensure!(
                chunk.len() <= self.config.max_output_bytes.saturating_sub(body.len()),
                "HTTP hook response exceeds bound"
            );
            body.extend_from_slice(&chunk);
        }
        let text = std::str::from_utf8(&body)
            .map_err(|_| anyhow::anyhow!("HTTP hook returned invalid encoding"))?;
        ensure!(
            !self.secrets.iter().any(|secret| text.contains(secret)),
            "HTTP hook response reflects a bound credential"
        );
        // Parsing here is only a safe retention boundary. The dispatcher still
        // consumes the original bytes through its frozen source result decoder.
        if !text.trim().is_empty() {
            let value = crate::plugins::wire::parse_json(&body)
                .map_err(|_| anyhow::anyhow!("HTTP hook returned invalid JSON or encoding"))?;
            ensure!(
                !self.secrets.iter().any(|secret| reflects(&value, secret)),
                "HTTP hook response reflects a bound credential"
            );
        }
        let raw = RawOutcome::Http { status, body };
        ensure!(
            !raw.decode_for(
                &self.profile,
                &self.identity,
                self.event,
                &crate::plugins::results::ResultContext {
                    role: if self.class == HandlerClass::Observer {
                        crate::plugins::results::ResultRole::Observer
                    } else {
                        crate::plugins::results::ResultRole::RequiredGate
                    },
                    ..Default::default()
                }
            )
            .failed(),
            "HTTP hook returned an invalid source result"
        );
        Ok(raw)
    }
}
fn reflects(value: &serde_json::Value, secret: &str) -> bool {
    match value {
        serde_json::Value::String(s) => s.contains(secret),
        serde_json::Value::Array(a) => a.iter().any(|v| reflects(v, secret)),
        serde_json::Value::Object(o) => o
            .iter()
            .any(|(k, v)| k.contains(secret) || reflects(v, secret)),
        _ => false,
    }
}
#[async_trait::async_trait]
impl HookRunner for HttpRunner {
    fn bound_event(&self) -> Option<HookEvent> {
        Some(self.event)
    }
    async fn run(&self, invocation: &HookInvocation) -> Result<RawOutcome> {
        let run = async {
            ensure!(
                invocation.key.event == self.event.as_str()
                    && invocation.events.plugin_event() == self.event
                    && invocation.declaration == self.identity
                    && invocation.endpoint == self.endpoint_identity
                    && invocation.class == self.class,
                "HTTP declaration/configuration identity mismatch"
            );
            let (runtime, operation) = invocation.events.plugin_context()?;
            runtime.plugin_runner_owner(operation, self.event)?;
            ensure!(
                runtime.record()?.allocation.is_some(),
                "HTTP hook requires an owning allowance"
            );
            let available = runtime
                .remaining()?
                .min(Duration::from_secs(30))
                .saturating_sub(Duration::from_secs(3));
            ensure!(
                !available.is_zero(),
                "HTTP hook deadline has no cleanup allowance"
            );
            let input = event::input(
                invocation,
                &self.profile,
                EventInput {
                    dialect: self.identity.dialect,
                    maximum: self.config.max_input_bytes,
                    model: None,
                    permission_mode: &self.config.permission_mode,
                    transcript_path: self.config.transcript_path.as_deref(),
                },
            )
            .map_err(|_| anyhow::anyhow!("HTTP hook input is invalid or exceeds bound"))?;
            let _lease = invocation.runner_lease.clone();
            let exchange = self.exchange(input);
            tokio::pin!(exchange);
            let mut owner = tokio::time::interval(Duration::from_millis(20));
            tokio::time::timeout(
                available.min(Duration::from_millis(self.config.timeout_ms)),
                async {
                    loop {
                        tokio::select! {
                            biased;
                            _ = owner.tick() => {
                                runtime.plugin_runner_owner(operation, self.event)?;
                                ensure!(!runtime.remaining()?.is_zero(), "HTTP hook owner expired");
                            }
                            result = &mut exchange => return result,
                        }
                    }
                },
            )
            .await
            .map_err(|_| anyhow::anyhow!("HTTP hook timed out; remote effects may be unknown"))?
        }
        .await;
        // Neither URL-bearing reqwest errors nor response bodies enter failures.
        Ok(run.unwrap_or_else(|error| RawOutcome::Failure {
            reason: error.to_string().chars().take(512).collect(),
        }))
    }
}
