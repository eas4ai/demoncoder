//! Host activation and durable one-shot evidence. Source output cannot mint a binding.
use super::{
    hook_types::HookDialect,
    receipts::{AdmissionKey, DeclarationIdentity, Scope},
};
use super::{
    receipts::{HookReceipt, RawOutcome},
    results::{DecisionChoice, GateDisposition, ProposedEffect},
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HookOrigin {
    Native,
    ClaudeSkillFrontmatter,
    ClaudeSettings,
    ClaudeAgent,
    Codex,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActivationChange {
    Reuse,
    ExplicitInvocation,
}

/// Only SharedRuntime mints this capability. In particular it cannot be deserialized
/// from package metadata, model output, or a previously captured plan.
/// ```compile_fail
/// let _: demoncoder::plugins::once::OnceBinding = serde_json::from_str("{}").unwrap();
/// ```
#[derive(Clone, Debug, Serialize)]
#[serde(transparent)]
pub struct OnceBinding(pub(crate) Activation);
impl OnceBinding {
    pub fn source(&self) -> ActivationSource {
        ActivationSource(self.0.source.clone())
    }

    pub(crate) fn validate(&self, id: &DeclarationIdentity) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.0.package == id.package && self.0.scope == id.scope && self.0.role == id.role,
            "one-shot activation belongs to another package, scope, or role"
        );
        anyhow::ensure!(
            matches!(
                (self.0.origin, id.dialect),
                (HookOrigin::Native, HookDialect::Native)
                    | (HookOrigin::ClaudeSkillFrontmatter, HookDialect::Claude)
            ),
            "one-shot origin does not support this dialect"
        );
        anyhow::ensure!(
            self.0.origin == HookOrigin::Native || self.0.source.packaged,
            "source skill activation requires a captured package"
        );
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Activation {
    pub(crate) session: String,
    pub(crate) source: CapturedSource,
    pub(crate) origin: HookOrigin,
    pub(crate) scope: Scope,
    pub(crate) package: String,
    pub(crate) component: String,
    pub(crate) role: String,
    pub(crate) epoch: u64,
}
impl Activation {
    pub(crate) fn same_owner(&self, other: &Self) -> bool {
        self.origin == other.origin && self.same_component(other)
    }
    pub(crate) fn same_component(&self, other: &Self) -> bool {
        self.session == other.session
            && self.source == other.source
            && self.scope == other.scope
            && self.package == other.package
            && self.component == other.component
            && self.role == other.role
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OnceState {
    Reserved,
    Unknown,
    Failed,
    Succeeded,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct OnceAttempt {
    pub(crate) activation: Activation,
    pub(crate) state: OnceState,
    /// Developer attestation does not change the observed outcome or uncertainty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) reconciliation: Option<FailedAttestation>,
}
/// A reference to an actual settled execution; a skip is never an execution.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct Consumption {
    pub session: String,
    pub operation: u64,
    pub event: String,
    pub invocation: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OnceSkip {
    pub declaration: DeclarationIdentity,
    pub inspected: AdmissionKey,
    pub consumed: Consumption,
    pub(crate) activation: Activation,
}
pub(crate) enum HookReservation {
    Run(Box<super::receipts::HookReceipt>),
    Skipped,
}

/// A completed transport alone is insufficient: logical blocks and invalid
/// responses remain eligible at a later event, while unknown effects stay held.
pub(crate) fn succeeded(
    hook: &HookReceipt,
    decoded: &crate::plugins::results::DecodedResult,
) -> bool {
    let transport = match hook.outcome.as_ref() {
        Some(
            RawOutcome::Command {
                exit_code: Some(0), ..
            }
            | RawOutcome::Callback { .. }
            | RawOutcome::Model { .. },
        ) => true,
        Some(RawOutcome::Http { status, .. }) => (200..300).contains(status),
        Some(RawOutcome::Mcp { is_error, .. }) => !is_error,
        _ => false,
    };
    transport
        && !hook.uncertain_effects
        && hook.hold.is_none()
        && hook.questions.is_empty()
        && !decoded.failed()
        && decoded.gate != GateDisposition::Held
        && !decoded.effects.iter().any(|effect| {
            matches!(effect,
            ProposedEffect::Decision { choice, .. } if *choice != DecisionChoice::NoObjection)
        })
        && !decoded.effects.iter().any(|effect| {
            matches!(
                effect,
                ProposedEffect::Control(_) | ProposedEffect::ScheduleObserver { .. }
            )
        })
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct FailedAttestation {
    pub actor: String,
    pub reason: String,
    pub evidence_digest: String,
}
/// Host-created target for an exact unresolved attempt. It cannot be created by
/// deserializing plugin output, and contains no API for certifying success.
#[derive(Clone, Debug, Serialize)]
pub struct UnresolvedOnce {
    pub reference: Consumption,
    pub declaration: DeclarationIdentity,
    pub(crate) activation: Activation,
    pub(crate) evidence_digest: String,
}

/// A host-created source capability. Package identities hash the raw canonical
/// path bytes, not the display name, content revision, inode or lossy Unicode.
#[derive(Clone, Debug, Serialize)]
#[serde(transparent)]
pub struct ActivationSource(pub(crate) CapturedSource);
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapturedSource {
    pub(crate) package: String,
    pub(crate) identity: String,
    pub(crate) packaged: bool,
}
impl ActivationSource {
    pub fn from_package(package: &super::Package) -> anyhow::Result<Self> {
        use std::os::unix::ffi::OsStrExt;
        let identity = super::admission::digest(&(
            "captured-package-source",
            package.source().canonical_root.as_os_str().as_bytes(),
        ))?;
        Ok(Self(CapturedSource {
            package: package.name().into(),
            identity,
            packaged: true,
        }))
    }
    /// Reserved for unpackaged native host handlers. Package runner constructors
    /// always require a source captured from their own immutable Package.
    pub fn host_namespace(name: &str) -> anyhow::Result<Self> {
        anyhow::ensure!(
            !name.is_empty() && name.len() <= 256,
            "invalid native host namespace"
        );
        Ok(Self(CapturedSource {
            package: name.into(),
            identity: super::admission::digest(&("native-host", name))?,
            packaged: false,
        }))
    }
}
