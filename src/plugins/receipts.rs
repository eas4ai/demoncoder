//! Bounded durable pre-tool history. Every field originates in the host except raw outcomes.
use super::hook_types::{HandlerKind, HookDialect};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    Managed,
    Bundled,
    User,
    Project,
    LocalProject,
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HandlerClass {
    Transformer,
    DecisionGate,
    Observer,
    #[default]
    Combined,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeclarationIdentity {
    pub package: String,
    pub code: String,
    pub policy: String,
    pub configuration: String,
    pub generation: String,
    pub scope: Scope,
    pub role: String,
    pub declaration: String,
    pub index: u32,
    pub dialect: HookDialect,
    pub runner: HandlerKind,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AdmissionKey {
    pub session: String,
    pub operation: u64,
    pub source_operation: u64,
    pub event: String,
    pub tool: String,
    pub arguments: String,
    pub plan: String,
    pub role: String,
    pub workspace: (u64, u64),
    pub inputs: Vec<(String, String)>,
    pub external: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PendingDecision {
    Deny,
    Ask,
    Defer,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Question {
    pub choice: PendingDecision,
    pub reason: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "transport", rename_all = "snake_case")]
pub enum RawOutcome {
    Command {
        exit_code: Option<i32>,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    },
    CommandFailure {
        reason: String,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    },
    Http {
        status: u16,
        body: Vec<u8>,
    },
    Mcp {
        structured: Option<Value>,
        text: Vec<String>,
        is_error: bool,
    },
    Callback {
        value: Value,
    },
    Model {
        value: Value,
        /// Host-selected source configuration, never read from model output.
        continue_on_block: bool,
    },
    Failure {
        reason: String,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HookReceipt {
    pub invocation: u32,
    pub declaration: DeclarationIdentity,
    pub class: HandlerClass,
    pub endpoint: Option<String>,
    pub inspected: AdmissionKey,
    /// None means the runner may have executed: never automatically replay.
    pub outcome: Option<RawOutcome>,
    /// Reserved invocations and interrupted/failed transports need reconciliation.
    pub uncertain_effects: bool,
    pub hold: Option<String>,
    pub questions: Vec<Question>,
    /// The validated wire remains in outcome; these names identify proposals whose
    /// host owners have not integrated yet. They never imply execution.
    pub pending_proposals: Vec<PendingProposal>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AdmissionReceipt {
    pub plan: String,
    pub declarations: Vec<Value>,
    pub hooks: Vec<HookReceipt>,
    pub final_key: Option<AdmissionKey>,
    pub hold: Option<String>,
}

/// Typed references into the retained original outcome. The payload is retained
/// once; later owners decode that outcome and select this exact proposal index.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PendingProposal {
    pub index: usize,
    pub kind: ProposalKind,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalKind {
    Decision,
    Control,
    RewriteInput,
    PermissionChanges,
    AdditionalContext,
    ClassifierContext,
    Warning,
    TransientNotice,
    Feedback,
    InitialMessage,
    SessionTitle,
    ReplaceModelOutput,
    ReplaceDynamicWatches,
    StageSkillRescan,
    RetainContextLimitFailure,
    WorktreePath,
    Elicitation,
    DisplayContent,
    SuppressOriginalPrompt,
    RetryDeniedOperation,
    TerminalNotification,
    ScheduleObserver,
}
impl From<&super::results::ProposedEffect> for ProposalKind {
    fn from(effect: &super::results::ProposedEffect) -> Self {
        use super::results::ProposedEffect as P;
        match effect {
            P::Decision { .. } => Self::Decision,
            P::Control(_) => Self::Control,
            P::RewriteInput(_) => Self::RewriteInput,
            P::PermissionChanges(_) => Self::PermissionChanges,
            P::AdditionalContext(_) => Self::AdditionalContext,
            P::ClassifierContext(_) => Self::ClassifierContext,
            P::Warning(_) => Self::Warning,
            P::TransientNotice(_) => Self::TransientNotice,
            P::Feedback(_) => Self::Feedback,
            P::InitialMessage(_) => Self::InitialMessage,
            P::SessionTitle(_) => Self::SessionTitle,
            P::ReplaceModelOutput { .. } => Self::ReplaceModelOutput,
            P::ReplaceDynamicWatches(_) => Self::ReplaceDynamicWatches,
            P::StageSkillRescan => Self::StageSkillRescan,
            P::RetainContextLimitFailure => Self::RetainContextLimitFailure,
            P::WorktreePath(_) => Self::WorktreePath,
            P::Elicitation { .. } => Self::Elicitation,
            P::DisplayContent(_) => Self::DisplayContent,
            P::SuppressOriginalPrompt => Self::SuppressOriginalPrompt,
            P::RetryDeniedOperation => Self::RetryDeniedOperation,
            P::TerminalNotification(_) => Self::TerminalNotification,
            P::ScheduleObserver { .. } => Self::ScheduleObserver,
        }
    }
}
