//! Host registration and controlled dispatch. This does not activate installed packages.
use super::{
    gate_snapshot::{GateReadSet, GateSnapshot},
    hook_types::*,
    profile::CompatibilityProfile,
    receipts::*,
    results::{self, HookResponse, ResultContext},
};
use crate::tools::ToolCall;
use anyhow::{Result, ensure};
use globset::{GlobBuilder, GlobMatcher};
use serde::Serialize;
use std::sync::Arc;

pub use super::receipts::{DeclarationIdentity, HandlerClass, RawOutcome, Scope};

#[derive(Clone, Default, Serialize)]
pub struct Matcher {
    pub tool: Option<String>,
    pub path: Option<String>,
}
#[derive(Clone, Serialize)]
pub struct Declaration {
    /// Trusted requirement policy; source output cannot downgrade a gate.
    pub required_gate: bool,
    /// Captured independently of once so removing once cannot erase an unknown fence.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<super::once::ActivationSource>,
    /// Bound only by an explicit host activation; absent preserves ordinary hooks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub once: Option<super::once::OnceBinding>,
    pub identity: DeclarationIdentity,
    pub class: HandlerClass,
    pub priority: i32,
    pub matcher: Matcher,
    pub reads: GateReadSet,
    /// Required for Claude source concurrency; scoped to package and scope.
    pub concurrent_group: Option<String>,
    pub read_only_endpoint: Option<String>,
    /// A provider transaction is not implemented; any requirement visibly holds.
    pub external_precondition: Option<String>,
}
pub struct Registration {
    pub declaration: Declaration,
    pub runner: Arc<dyn HookRunner>,
    pub revalidation: Option<Arc<dyn HookRunner>>,
}
/// Facts are supplied by the host. Responses contain no identity or authority selector.
pub struct HookInvocation {
    pub(crate) required_gate: bool,
    pub(crate) observer: Option<Arc<crate::workflow::runtime::plugin_observer::ObserverLease>>,
    pub invocation: u32,
    pub key: AdmissionKey,
    pub declaration: DeclarationIdentity,
    pub endpoint: Option<String>,
    pub candidate: ToolCall,
    pub snapshot: Arc<GateSnapshot>,
    /// Present only on host-created post-operation invocations.
    pub completed: Option<CompletedTool>,
    pub(crate) events: crate::events::EventSink,
    pub(crate) host: super::runners::HookHost,
    pub(crate) runner_lease: Arc<tokio::sync::OwnedSemaphorePermit>,
    pub(crate) mutation_guard: Option<Arc<tokio::sync::OwnedMutexGuard<()>>>,
    pub(crate) class: HandlerClass,
}
#[derive(Clone)]
pub struct CompletedTool {
    pub facts: PostToolFacts,
    pub original: crate::tools::ToolResult,
}
#[async_trait::async_trait]
pub trait HookRunner: Send + Sync {
    fn observer_config(&self) -> Option<super::observer::ObserverConfig> {
        None
    }
    /// Production runners bind event identity into their immutable configuration.
    fn bound_event(&self) -> Option<HookEvent> {
        None
    }
    /// Host-only dependency admission, completed before any group hook is dispatched.
    async fn prepare(&self, _invocation: &HookInvocation) -> Result<()> {
        Ok(())
    }
    /// Host implementation capability, never a flag supplied by hook output.
    fn mutates_workspace(&self) -> bool {
        false
    }
    /// Host implementation fact. Unknown outcomes otherwise require reconciliation.
    fn side_effect_free(&self) -> bool {
        false
    }
    async fn run(&self, invocation: &HookInvocation) -> Result<RawOutcome>;
}
pub(crate) struct Handler {
    pub registration: Registration,
    path: Option<GlobMatcher>,
}
pub struct PreToolPlan {
    pub(crate) event: HookEvent,
    pub(crate) handlers: Vec<Handler>,
    pub(crate) digest: String,
    pub(crate) profile: CompatibilityProfile,
    pub(crate) captures: Arc<tokio::sync::Semaphore>,
    pub(crate) runners: Arc<tokio::sync::Semaphore>,
}
impl PreToolPlan {
    pub fn new(registrations: Vec<Registration>) -> Result<Self> {
        Self::for_event(HookEvent::PreToolUse, registrations)
    }

    pub(crate) fn for_event(
        event: HookEvent,
        mut registrations: Vec<Registration>,
    ) -> Result<Self> {
        ensure!(
            !registrations.is_empty() && registrations.len() <= 32,
            "pre-tool plan requires 1 to 32 declarations"
        );
        let profile = CompatibilityProfile::embedded()?;
        registrations.sort_by(|a, b| {
            let (a, b) = (&a.declaration, &b.declaration);
            (
                a.priority,
                &a.identity.scope,
                &a.identity.package,
                a.identity.index,
                a.source.as_ref().map(|s| &s.0.identity),
                a.once.as_ref().map(|b| &b.0.component),
            )
                .cmp(&(
                    b.priority,
                    &b.identity.scope,
                    &b.identity.package,
                    b.identity.index,
                    b.source.as_ref().map(|s| &s.0.identity),
                    b.once.as_ref().map(|b| &b.0.component),
                ))
        });
        let mut seen = std::collections::BTreeMap::new();
        let mut handlers = Vec::new();
        let mut positions = std::collections::BTreeSet::new();
        let mut declarations = Vec::new();
        for registration in registrations {
            ensure!(
                registration
                    .runner
                    .bound_event()
                    .is_none_or(|bound| bound == event)
                    && registration
                        .revalidation
                        .as_ref()
                        .is_none_or(|runner| runner
                            .bound_event()
                            .is_none_or(|bound| bound == event)),
                "runner configuration belongs to a different hook event"
            );
            let d = &registration.declaration;
            let id = &d.identity;
            for text in [
                &id.package,
                &id.code,
                &id.policy,
                &id.configuration,
                &id.generation,
                &id.role,
                &id.declaration,
            ] {
                ensure!(
                    !text.is_empty() && text.len() <= 256,
                    "invalid immutable declaration identity"
                );
            }
            profile.require_runner(id.dialect, event, id.runner)?;
            if let Some(source) = &d.source {
                ensure!(
                    source.0.package == id.package,
                    "declaration source package mismatch"
                );
                ensure!(
                    id.dialect == HookDialect::Native || source.0.packaged,
                    "source hooks require a captured package source"
                );
            }
            if let Some(binding) = &d.once {
                binding.validate(id)?;
                ensure!(
                    d.source.as_ref().is_some_and(|s| s.0 == binding.0.source),
                    "one-shot binding has a different canonical source"
                );
            }
            ensure!(
                id.dialect == HookDialect::Native
                    || d.class == HandlerClass::Combined
                    || (d.class == HandlerClass::Observer
                        && registration.runner.observer_config().is_some()),
                "source declarations retain combined semantics; explicit conversion is not integrated"
            );
            ensure!(
                event == HookEvent::PreToolUse || registration.revalidation.is_none(),
                "post-tool handlers cannot revalidate pre-tool admission"
            );
            ensure!(
                d.read_only_endpoint.is_some() == registration.revalidation.is_some(),
                "read-only endpoint requires an explicit runner"
            );
            ensure!(
                d.class == HandlerClass::Combined || registration.revalidation.is_none(),
                "only combined handlers need a separate revalidation endpoint"
            );
            ensure!(
                id.dialect == HookDialect::Claude || d.concurrent_group.is_none(),
                "concurrent source groups require Claude semantics"
            );
            ensure!(
                id.dialect != HookDialect::Claude || d.concurrent_group.is_some(),
                "Claude source declarations require their concurrent group identity"
            );
            for value in [
                &d.concurrent_group,
                &d.read_only_endpoint,
                &d.external_precondition,
                &d.matcher.tool,
                &d.matcher.path,
            ]
            .into_iter()
            .flatten()
            {
                ensure!(
                    !value.is_empty() && value.len() <= 256,
                    "declaration field exceeds bounds"
                );
            }
            let encoded = serde_json::to_vec(d)?;
            ensure!(encoded.len() <= 65536, "declaration exceeds bounds");
            let canonical = (
                id.scope.clone(),
                id.package.clone(),
                d.source.as_ref().map(|s| s.0.identity.clone()),
                d.once.as_ref().map(|b| b.0.component.clone()),
                id.declaration.clone(),
            );
            if let Some(previous) = seen.get(&canonical) {
                ensure!(
                    previous == &encoded,
                    "canonical declaration has conflicting registrations"
                );
                continue;
            }
            ensure!(
                positions.insert((
                    id.scope.clone(),
                    id.package.clone(),
                    d.source.as_ref().map(|s| s.0.identity.clone()),
                    d.once.as_ref().map(|b| b.0.component.clone()),
                    id.index
                )),
                "duplicate declaration index makes ordering ambiguous"
            );
            seen.insert(canonical, encoded.clone());
            declarations.push(encoded);
            let path = d
                .matcher
                .path
                .as_ref()
                .map(|p| {
                    GlobBuilder::new(p)
                        .literal_separator(true)
                        .backslash_escape(false)
                        .build()
                        .map(|g| g.compile_matcher())
                })
                .transpose()?;
            handlers.push(Handler { registration, path });
        }
        let digest = super::admission::digest(&(event, &declarations))?;
        Ok(Self {
            event,
            handlers,
            digest,
            profile,
            captures: Arc::new(tokio::sync::Semaphore::new(1)),
            runners: Arc::new(tokio::sync::Semaphore::new(32)),
        })
    }
}
impl Handler {
    pub(crate) fn matches(&self, call: &ToolCall) -> bool {
        let m = &self.registration.declaration.matcher;
        m.tool.as_ref().is_none_or(|tool| tool == &call.name)
            && self.path.as_ref().is_none_or(|path| {
                call.arguments["path"]
                    .as_str()
                    .is_some_and(|p| p.len() <= 4096 && path.is_match(p))
            })
    }
}
impl RawOutcome {
    pub(crate) fn within_retention_bound(&self) -> bool {
        fn value_bytes(value: &serde_json::Value) -> usize {
            super::wire::measure(value).unwrap_or(usize::MAX)
        }
        let measured = match self {
            Self::Command { stdout, stderr, .. } => {
                stdout.len().saturating_add(stderr.len()).saturating_mul(4)
            }
            Self::CommandFailure {
                reason,
                stdout,
                stderr,
            } => reason
                .len()
                .saturating_mul(6)
                .saturating_add(stdout.len().saturating_add(stderr.len()).saturating_mul(4)),
            Self::Http { body, .. } => body.len().saturating_mul(4),
            Self::Mcp {
                structured, text, ..
            } => {
                if text.len() > 128 {
                    return false;
                }
                text.iter()
                    .fold(structured.as_ref().map_or(0, value_bytes), |n, t| {
                        n.saturating_add(t.len().saturating_mul(6))
                    })
            }
            Self::Callback { value } | Self::Model { value, .. } => value_bytes(value),
            Self::Failure { reason } => reason.len().saturating_mul(6),
        };
        measured <= 120 * 1024
    }

    pub(crate) fn decode(
        &self,
        profile: &CompatibilityProfile,
        id: &DeclarationIdentity,
    ) -> results::DecodedResult {
        self.decode_for(
            profile,
            id,
            HookEvent::PreToolUse,
            &ResultContext::default(),
        )
    }

    pub(crate) fn decode_for(
        &self,
        profile: &CompatibilityProfile,
        id: &DeclarationIdentity,
        event: HookEvent,
        context: &ResultContext,
    ) -> results::DecodedResult {
        if let Self::Model {
            value,
            continue_on_block,
        } = self
        {
            let mut context = context.clone();
            context.work.continue_on_block = *continue_on_block;
            return results::decode_model(profile, id.dialect, event, id.runner, value, &context);
        }
        let texts;
        let response = match self {
            Self::Command {
                exit_code,
                stdout,
                stderr,
            } => HookResponse::Command {
                exit_code: *exit_code,
                stdout,
                stderr,
            },
            Self::Http { status, body } => HookResponse::Http {
                status: *status,
                body,
            },
            Self::Mcp {
                structured,
                text,
                is_error,
            } => {
                texts = text.iter().map(String::as_str).collect::<Vec<_>>();
                HookResponse::Mcp {
                    structured: structured.as_ref(),
                    text: &texts,
                    is_error: *is_error,
                }
            }
            Self::Callback { value } => HookResponse::Callback(value),
            Self::Model { .. } => unreachable!("model response decoded above"),
            Self::Failure { .. } | Self::CommandFailure { .. } => {
                HookResponse::Failure(results::TransportFailure::Execution)
            }
        };
        results::decode_response(profile, id.dialect, event, id.runner, context, response)
    }
}

/// The caller reserved the invocation durably before polling this future. Source
/// group owners poll every member together; this function never allocates an ID.
pub(crate) async fn run_owned(invocation: &HookInvocation, runner: &dyn HookRunner) -> RawOutcome {
    // All source group futures are polled together against the same candidate.
    let owner = invocation.events.plugin_context().and_then(|(r, id)| {
        r.plugin_runner_owner(id, invocation.events.plugin_event())?;
        r.remaining()
    });
    match owner {
        Ok(remaining) if !remaining.is_zero() => {
            match tokio::time::timeout(
                remaining.min(std::time::Duration::from_secs(30)),
                runner.run(invocation),
            )
            .await
            {
                Ok(Ok(raw)) => raw,
                Ok(Err(_)) => RawOutcome::Failure {
                    reason: "handler execution failed; effects may be unknown".into(),
                },
                Err(_) => RawOutcome::Failure {
                    reason: "handler timed out; effects may be unknown".into(),
                },
            }
        }
        _ => RawOutcome::Failure {
            reason: "handler owner or deadline unavailable".into(),
        },
    }
}

impl Declaration {
    pub(crate) fn bind_package_source(&mut self, package: &super::Package) -> Result<()> {
        let source = super::once::ActivationSource::from_package(package)?;
        ensure!(
            self.source.as_ref().is_none_or(|s| s.0 == source.0),
            "registration has a different captured source"
        );
        ensure!(
            self.once.as_ref().is_none_or(|b| b.0.source == source.0),
            "one-shot binding does not belong to this captured package source"
        );
        self.source = Some(source);
        Ok(())
    }
}
