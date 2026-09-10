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
    pub invocation: u32,
    pub key: AdmissionKey,
    pub declaration: DeclarationIdentity,
    pub endpoint: Option<String>,
    pub candidate: ToolCall,
    pub snapshot: Arc<GateSnapshot>,
    pub(crate) events: crate::events::EventSink,
}
#[async_trait::async_trait]
pub trait HookRunner: Send + Sync {
    async fn run(&self, invocation: &HookInvocation) -> Result<RawOutcome>;
}
pub(crate) struct Handler {
    pub registration: Registration,
    path: Option<GlobMatcher>,
}
pub struct PreToolPlan {
    pub(crate) handlers: Vec<Handler>,
    pub(crate) digest: String,
    pub(crate) profile: CompatibilityProfile,
    pub(crate) captures: Arc<tokio::sync::Semaphore>,
}
impl PreToolPlan {
    pub fn new(mut registrations: Vec<Registration>) -> Result<Self> {
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
            )
                .cmp(&(
                    b.priority,
                    &b.identity.scope,
                    &b.identity.package,
                    b.identity.index,
                ))
        });
        let mut seen = std::collections::BTreeMap::new();
        let mut handlers = Vec::new();
        let mut positions = std::collections::BTreeSet::new();
        let mut declarations = Vec::new();
        for registration in registrations {
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
            profile.require_runner(id.dialect, HookEvent::PreToolUse, id.runner)?;
            ensure!(
                !matches!(id.runner, HandlerKind::Prompt | HandlerKind::Agent),
                "model runner allocation integration is unavailable"
            );
            ensure!(
                d.class != HandlerClass::Observer,
                "pre-tool observers require later owned observer integration"
            );
            ensure!(
                id.dialect == HookDialect::Native || d.class == HandlerClass::Combined,
                "source declarations retain combined semantics; explicit conversion is not integrated"
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
            let canonical = (id.scope.clone(), id.package.clone(), id.declaration.clone());
            if let Some(previous) = seen.get(&canonical) {
                ensure!(
                    previous == &encoded,
                    "canonical declaration has conflicting registrations"
                );
                continue;
            }
            ensure!(
                positions.insert((id.scope.clone(), id.package.clone(), id.index)),
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
        let digest = super::admission::digest(&declarations)?;
        Ok(Self {
            handlers,
            digest,
            profile,
            captures: Arc::new(tokio::sync::Semaphore::new(1)),
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
            Self::Callback { value } => value_bytes(value),
            Self::Failure { reason } => reason.len().saturating_mul(6),
        };
        measured <= 120 * 1024
    }

    pub(crate) fn decode(
        &self,
        profile: &CompatibilityProfile,
        id: &DeclarationIdentity,
    ) -> results::DecodedResult {
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
            Self::Failure { .. } => HookResponse::Failure(results::TransportFailure::Execution),
        };
        results::decode_response(
            profile,
            id.dialect,
            HookEvent::PreToolUse,
            id.runner,
            &ResultContext::default(),
            response,
        )
    }
}

/// The caller reserved the invocation durably before polling this future. Source
/// group owners poll every member together; this function never allocates an ID.
pub(crate) async fn run_owned(invocation: &HookInvocation, runner: &dyn HookRunner) -> RawOutcome {
    // All source group futures are polled together against the same candidate.
    let owner = invocation.events.plugin_context().and_then(|(r, id)| {
        r.plugin_owner(id)?;
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
