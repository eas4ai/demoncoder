//! PreToolUse calls bind admitted managed tools; event data never chooses authority.
use super::{
    event::{self, EventInput},
    mcp_input,
};
use crate::plugins::{
    Dialect, Package,
    dispatch::{Declaration, HookInvocation, HookRunner, Registration},
    hook_types::{HandlerKind, HookDialect, HookEvent},
    profile::CompatibilityProfile,
    receipts::{DeclarationIdentity, HandlerClass, RawOutcome},
    services::ManagedService,
};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use serde_json::Value;
use std::sync::Arc;

#[derive(Clone)]
pub struct McpBinding {
    pub service: Arc<ManagedService>,
    pub tool: String,
    pub input: Value,
}
#[derive(Clone, Serialize)]
pub struct McpConfig {
    pub model: Option<String>,
    pub permission_mode: String,
    pub transcript_path: Option<String>,
}
impl Default for McpConfig {
    fn default() -> Self {
        Self {
            model: None,
            permission_mode: "default".into(),
            transcript_path: None,
        }
    }
}
pub struct McpRunner {
    identity: DeclarationIdentity,
    class: HandlerClass,
    endpoint: Option<String>,
    binding: McpBinding,
    config: McpConfig,
    profile: Arc<CompatibilityProfile>,
    protected_services: Arc<Vec<Arc<ManagedService>>>,
}
impl McpRunner {
    pub fn registration(
        package: Arc<Package>,
        mut declaration: Declaration,
        binding: McpBinding,
        revalidation: Option<McpBinding>,
        config: McpConfig,
    ) -> Result<Registration> {
        ensure!(
            declaration.identity.runner == HandlerKind::McpTool,
            "MCP registration has different runner kind"
        );
        let dialect = declaration.identity.dialect;
        ensure!(
            dialect == HookDialect::Native
                || matches!(
                    (dialect, package.dialect()),
                    (HookDialect::Claude, Dialect::Claude)
                        | (HookDialect::Codex, Dialect::Codex | Dialect::Portable)
                ),
            "MCP source dialect differs"
        );
        let profile = Arc::new(CompatibilityProfile::embedded()?);
        profile.require_runner(dialect, HookEvent::PreToolUse, HandlerKind::McpTool)?;
        ensure!(
            binding.service.package().digest() == package.digest()
                && binding.service.package().name() == package.name(),
            "MCP hook package differs from managed service"
        );
        ensure!(
            declaration.read_only_endpoint.is_some() == revalidation.is_some(),
            "MCP revalidation requires explicit read-only binding"
        );
        for selected in std::iter::once(&binding).chain(revalidation.iter()) {
            selected.service.tool(&selected.tool)?;
            ensure!(
                selected.service.config.identity.role == declaration.identity.role
                    && selected.service.config.identity.generation
                        == declaration.identity.generation,
                "MCP service role or generation differs"
            );
            crate::plugins::wire::measure(&selected.input)?;
            ensure!(
                selected.input.is_object(),
                "MCP input template must be an object"
            );
        }
        if let Some(read) = &revalidation {
            ensure!(
                read.service.tool(&read.tool)?.read_only
                    && (read.service.identity != binding.service.identity
                        || read.tool != binding.tool),
                "MCP revalidation must use separate admitted read-only tool"
            );
        }
        ensure!(
            declaration
                .read_only_endpoint
                .as_ref()
                .is_none_or(|s| !s.is_empty()
                    && s.len() <= 128
                    && s.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))),
            "MCP endpoint identity invalid"
        );
        ensure!(
            [
                "default",
                "acceptEdits",
                "plan",
                "dontAsk",
                "bypassPermissions"
            ]
            .contains(&config.permission_mode.as_str())
                && config
                    .model
                    .as_ref()
                    .is_none_or(|v| !v.is_empty() && v.len() <= 16384 && !v.contains('\0'))
                && config
                    .transcript_path
                    .as_ref()
                    .is_none_or(|v| v.len() <= 16384 && !v.contains('\0'))
                && (dialect != HookDialect::Codex || config.model.is_some()),
            "MCP source framing invalid"
        );
        declaration.identity.package = package.name().into();
        declaration.identity.code = package.digest().into();
        declaration.identity.configuration = crate::plugins::admission::digest(&(
            &binding.service.identity,
            &binding.tool,
            &binding.input,
            revalidation
                .as_ref()
                .map(|b| (&b.service.identity, &b.tool, &b.input)),
            &config,
            &declaration.read_only_endpoint,
        ))?;
        let protected_services: Arc<Vec<Arc<ManagedService>>> = Arc::new(
            std::iter::once(binding.service.clone())
                .chain(revalidation.iter().map(|b| b.service.clone()))
                .collect(),
        );
        let runner = Arc::new(Self {
            identity: declaration.identity.clone(),
            class: declaration.class,
            endpoint: None,
            binding,
            config: config.clone(),
            profile: profile.clone(),
            protected_services: protected_services.clone(),
        });
        let revalidation = revalidation.map(|binding| {
            Arc::new(Self {
                identity: declaration.identity.clone(),
                class: HandlerClass::DecisionGate,
                endpoint: declaration.read_only_endpoint.clone(),
                binding,
                config,
                profile,
                protected_services,
            }) as Arc<dyn HookRunner>
        });
        Ok(Registration {
            declaration,
            runner,
            revalidation,
        })
    }
    fn input(&self, invocation: &HookInvocation) -> Result<Value> {
        ensure!(
            invocation.declaration == self.identity
                && invocation.class == self.class
                && invocation.endpoint == self.endpoint,
            "MCP declaration/configuration identity mismatch"
        );
        let bytes = event::input(
            invocation,
            &self.profile,
            EventInput {
                dialect: self.identity.dialect,
                maximum: 65536,
                model: self.config.model.as_deref(),
                permission_mode: &self.config.permission_mode,
                transcript_path: self.config.transcript_path.as_deref(),
            },
        )?;
        let event = crate::plugins::wire::parse_json(&bytes)?;
        mcp_input::resolve(&self.binding.input, &event, self.identity.dialect)
    }
    async fn execute(&self, invocation: &HookInvocation) -> Result<RawOutcome> {
        let input = self.input(invocation)?;
        let result = self
            .binding
            .service
            .call(invocation, &self.binding.tool, input)
            .await?;
        for service in self.protected_services.iter() {
            service.protect(&result)?;
        }
        let is_error = result
            .get("isError")
            .map(|v| v.as_bool().context("MCP isError must be boolean"))
            .transpose()?
            .unwrap_or(false);
        ensure!(
            !is_error,
            "MCP tool reported an error; effects may be unknown"
        );
        let structured = result.get("structuredContent").cloned();
        ensure!(
            structured.as_ref().is_none_or(Value::is_object),
            "MCP structured content must be an object"
        );
        let content = result
            .get("content")
            .and_then(Value::as_array)
            .context("MCP result content missing")?;
        ensure!(content.len() <= 64, "MCP result content exceeds bound");
        let text = content
            .iter()
            .map(|block| {
                ensure!(
                    block.get("type").and_then(Value::as_str) == Some("text"),
                    "MCP result content type is unsupported"
                );
                Ok(block
                    .get("text")
                    .and_then(Value::as_str)
                    .context("MCP text block malformed")?
                    .to_owned())
            })
            .collect::<Result<Vec<_>>>()?;
        let raw = RawOutcome::Mcp {
            structured,
            text,
            is_error,
        };
        ensure!(
            !raw.decode(&self.profile, &self.identity).failed(),
            "MCP returned an invalid source result"
        );
        Ok(raw)
    }
}
#[async_trait::async_trait]
impl HookRunner for McpRunner {
    async fn prepare(&self, invocation: &HookInvocation) -> Result<()> {
        let input = self.input(invocation)?;
        crate::plugins::wire::SchemaValidator::compile(
            &self.binding.service.tool(&self.binding.tool)?.metadata["inputSchema"],
        )?
        .validate(&input)?;
        self.binding.service.bootstrap(invocation).await
    }
    async fn run(&self, invocation: &HookInvocation) -> Result<RawOutcome> {
        // No server payload, schema paths, argument values or transport URL enters a failure.
        Ok(self
            .execute(invocation)
            .await
            .unwrap_or_else(|_| RawOutcome::Failure {
                reason: "MCP hook failed or became unavailable; effects may be unknown".into(),
            }))
    }
}
