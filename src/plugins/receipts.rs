//! Bounded durable tool lifecycle history. Every field originates in the host except raw outcomes.
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<super::once::CapturedSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub once: Option<super::once::OnceAttempt>,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub once_skips: Vec<super::once::OnceSkip>,
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

/// Host-captured tool representation. This is never accepted from plugin output.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolRepresentation {
    #[default]
    Native,
    ClaudeMcp {
        tool_name: String,
        tool_use_id: String,
        /// Validated SDK PreToolUse base and exact source tool correlation.
        source_input: Value,
    },
    CodexDynamic {
        tool_use_id: String,
        turn_id: String,
        session_id: String,
        model: Option<String>,
        permission_mode: String,
        transcript_path: Option<String>,
    },
}
impl ToolRepresentation {
    pub fn is_mcp(&self) -> bool {
        matches!(self, Self::ClaudeMcp { .. })
    }
}

/// Versioned host facts refer to the immutable call/result in the same operation.
/// Source translations are host operation events, never observed SDK callbacks.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PostToolFacts {
    pub version: u32,
    pub session: String,
    pub task: Option<u64>,
    pub role: String,
    pub operation: u64,
    pub source_operation: u64,
    pub event: super::hook_types::HookEvent,
    /// A host translation, not an observed backend post callback.
    pub provenance: String,
    pub host_transcript_path: String,
    pub host_model: Option<String>,
    pub host_permission_mode: String,
    pub representation: ToolRepresentation,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum PostContinuation {
    #[default]
    Continue,
    Correction,
    Held {
        reason: String,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalDisposition {
    Applied,
    Held,
    Pending,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppliedProposal {
    pub invocation: u32,
    pub proposal: PendingProposal,
    pub disposition: ProposalDisposition,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PluginMessage {
    pub invocation: u32,
    pub package: String,
    pub kind: ProposalKind,
    pub text: String,
}
/// Post-operation ownership is distinct from pre-tool approval. Historical tool
/// receipts omit this entire record and retain their previous semantics.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LifecycleReceipt {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub once_skips: Vec<super::once::OnceSkip>,
    #[serde(default)]
    pub delivery: PostDelivery,
    pub version: u32,
    pub facts: PostToolFacts,
    pub plan: String,
    pub declarations: Vec<Value>,
    pub hooks: Vec<HookReceipt>,
    pub proposals: Vec<AppliedProposal>,
    pub messages: Vec<PluginMessage>,
    pub diagnostics: Vec<String>,
    pub model_content: Option<Value>,
    pub continuation: PostContinuation,
    #[serde(default)]
    pub correction_required: bool,
    pub correction_admitted: bool,
    /// None is the historical text-only corrective prompt representation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correction_presentation: Option<CorrectionPresentation>,
    pub settled: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CorrectionPresentation {
    ClaudeProviderBlocksV1,
}

/// Reservation precedes an external write. An unacknowledged reservation is never replayed.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PostDelivery {
    #[default]
    Local,
    LocalPending,
    Staged,
    Reserved,
    Acknowledged,
    /// Original backend response is withheld while its turn is interrupted.
    Superseding,
    /// Correlated interrupt acknowledgment and terminal completion are retained.
    Superseded,
    /// A new backend invocation owns the one charged correction. The old result
    /// was never delivered as a normal tool response.
    CorrectionReserved {
        invocation: u64,
    },
    CorrectionAcknowledged {
        invocation: u64,
        acknowledgment: CorrectionAcknowledgment,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum CorrectionAcknowledgment {
    ClaudeUser {
        session_id: String,
        uuid: String,
        content_digest: String,
    },
    CodexTurn {
        thread_id: String,
        turn_id: String,
        request_id: u64,
    },
}
