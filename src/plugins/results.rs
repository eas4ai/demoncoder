//! Typed hook result proposals. Decoding never performs an effect or grants authority.
//!
//! All payloads remain plugin-origin and untrusted. The caller retains original tool
//! receipts and response bytes separately; neither is accepted as an input to mutate.
mod decisions;
mod effects;
mod native;
mod source;
mod terminal;
mod transport;
use super::{hook_types::*, profile::CompatibilityProfile, wire::WireError};
use serde_json::Value;
use std::{fmt, path::PathBuf};

/// Payload data must not be copied to diagnostic logs. Explicit access is required.
#[derive(Clone, PartialEq, Eq)]
pub struct Untrusted<T>(T);
impl<T> Untrusted<T> {
    pub fn new(value: T) -> Self {
        Self(value)
    }
    pub fn get(&self) -> &T {
        &self.0
    }
}
impl<T> fmt::Debug for Untrusted<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<plugin-origin payload>")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultRole {
    RequiredGate,
    Observer,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStartSource {
    Startup,
    Resume,
    Fork,
    Clear,
    Compact,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectoryAddedSource {
    Command,
    Sdk,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceDecision {
    NoSourceDecision,
    NoObjection,
    Objection,
    SpecialResult,
}

/// Trusted invocation facts, never deserialized from a plugin response.
#[derive(Debug, Clone)]
pub struct ResultContext {
    pub role: ResultRole,
    pub work: ModelCallContext,
    pub retry_eligible: bool,
    pub session_source: SessionStartSource,
    pub interactive_display: bool,
    pub interactive_session: bool,
    pub directory_source: DirectoryAddedSource,
    pub config_is_managed_policy: bool,
    pub model_switch_can_ask: bool,
    pub tool_is_mcp: bool,
    pub compaction_recovery: bool,
    pub elicitation_action: Option<ElicitationAction>,
    pub working_directory: Option<PathBuf>,
    pub asynchronous: bool,
    pub observer_timeout_ms: u64,
}
impl Default for ResultContext {
    fn default() -> Self {
        Self {
            role: ResultRole::RequiredGate,
            work: ModelCallContext::default(),
            retry_eligible: false,
            session_source: SessionStartSource::Startup,
            interactive_display: false,
            interactive_session: false,
            directory_source: DirectoryAddedSource::Sdk,
            config_is_managed_policy: false,
            model_switch_can_ask: false,
            tool_is_mcp: false,
            compaction_recovery: false,
            elicitation_action: None,
            working_directory: None,
            asynchronous: false,
            observer_timeout_ms: 30_000,
        }
    }
}

/// A callback is a control/transport boundary, not an additional package handler kind.
/// `handler` passed to `decode_response` remains the actual admitted package handler.
pub enum HookResponse<'a> {
    Command {
        exit_code: Option<i32>,
        stdout: &'a [u8],
        stderr: &'a [u8],
    },
    Http {
        status: u16,
        body: &'a [u8],
    },
    Mcp {
        structured: Option<&'a Value>,
        text: &'a [&'a str],
        is_error: bool,
    },
    Callback(&'a Value),
    Failure(TransportFailure),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportFailure {
    Timeout,
    Cancelled,
    Execution,
    Network,
    InvalidEncoding,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateDisposition {
    Held,
    NoObjection,
    NotAGate,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionChoice {
    NoObjection,
    Deny,
    Ask,
    Defer,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElicitationAction {
    Accept,
    Decline,
    Cancel,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FollowupTarget {
    Task,
    Subagent,
    Teammate,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlRequest {
    HoldAction,
    HoldContinuation,
    EndTurn,
    Followup(FollowupTarget),
    StopUnmet,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelOutputKind {
    Tool,
    McpTool,
}

/// Only a validated vocabulary reaches terminal consumers; no raw control bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalNotice {
    Bell,
    Osc { code: u16, text: Untrusted<String> },
}

/// Proposed changes require owner validation/admission. None is a receipt, an
/// access grant, a new allocation, verification, review, or developer acceptance.
#[derive(Clone, PartialEq, Eq)]
pub enum ProposedEffect {
    Decision {
        choice: DecisionChoice,
        reason: Option<Untrusted<String>>,
    },
    Control(ControlRequest),
    RewriteInput(Untrusted<Value>),
    PermissionChanges(Untrusted<Value>),
    AdditionalContext(Untrusted<String>),
    ClassifierContext(Untrusted<String>),
    Warning(Untrusted<String>),
    TransientNotice(Untrusted<String>),
    /// Feedback beside a retained result, not a request to undo it.
    Feedback(Untrusted<String>),
    InitialMessage(Untrusted<String>),
    SessionTitle(Untrusted<String>),
    ReplaceModelOutput {
        kind: ModelOutputKind,
        value: Untrusted<Value>,
    },
    /// Paths require host file admission. The list replaces only dynamic watches;
    /// an empty list explicitly clears them. Static matcher paths are retained.
    ReplaceDynamicWatches(Vec<PathBuf>),
    StageSkillRescan,
    RetainContextLimitFailure,
    /// A lexical path proposal, never proof of a real owned Git worktree.
    WorktreePath(PathBuf),
    Elicitation {
        action: Option<ElicitationAction>,
        content: Option<Untrusted<Value>>,
    },
    DisplayContent(Untrusted<String>),
    SuppressOriginalPrompt,
    RetryDeniedOperation,
    TerminalNotification(Vec<TerminalNotice>),
    ScheduleObserver {
        timeout_ms: u64,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticKind {
    Failure,
    Ignored,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultDiagnostic {
    pub kind: DiagnosticKind,
    pub detail: WireError,
}
#[derive(Debug, Clone)]
pub struct DecodedResult {
    pub dialect: HookDialect,
    pub event: HookEvent,
    pub handler: HandlerKind,
    pub gate: GateDisposition,
    pub source_decision: SourceDecision,
    pub stderr: Option<Untrusted<String>>,
    pub effects: Vec<ProposedEffect>,
    pub diagnostics: Vec<ResultDiagnostic>,
}
impl DecodedResult {
    pub fn failed(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.kind == DiagnosticKind::Failure)
    }
}

/// Decode one actual admitted non-model invocation. A NoObjection gate disposition
/// only says this result is not holding a gate: source_decision may still be
/// NoSourceDecision. Neither field is admission, permission, or completion evidence.
/// The caller must bind this value to the original invocation/candidate and retain
/// original receipts before executing any proposed effects.
pub fn decode_response(
    profile: &CompatibilityProfile,
    dialect: HookDialect,
    event: HookEvent,
    handler: HandlerKind,
    context: &ResultContext,
    response: HookResponse<'_>,
) -> DecodedResult {
    let mut result = DecodedResult {
        dialect,
        event,
        handler,
        gate: if context.role == ResultRole::Observer
            || matches!(event, HookEvent::Interrupt | HookEvent::SessionEnd)
        {
            GateDisposition::NotAGate
        } else {
            GateDisposition::NoObjection
        },
        source_decision: SourceDecision::NoSourceDecision,
        stderr: None,
        effects: Vec::new(),
        diagnostics: Vec::new(),
    };
    if let Err(error) = validate_invocation(profile, dialect, event, handler, context) {
        result.fail(error);
        return result;
    }
    match transport::frame(dialect, event, handler, response) {
        Err(error) => result.fail(error),
        Ok(frame) => {
            result.stderr = frame.stderr;
            for error in frame.failures {
                result.fail(error);
            }
            interpret_frame(profile, context, frame.payload, frame.exit_two, &mut result);
        }
    }
    result.finish(context);
    result
}

fn validate_invocation(
    profile: &CompatibilityProfile,
    dialect: HookDialect,
    event: HookEvent,
    handler: HandlerKind,
    context: &ResultContext,
) -> Result<(), WireError> {
    profile.require_runner(dialect, event, handler)?;
    if matches!(event, HookEvent::Interrupt | HookEvent::SessionEnd)
        && context.role == ResultRole::RequiredGate
    {
        return Err(WireError::new(
            "/role",
            "cancellation and shutdown cannot register a required gate",
        ));
    }
    if matches!(handler, HandlerKind::Prompt | HandlerKind::Agent) {
        return Err(WireError::new(
            "/handler",
            "model handlers require validated model verdict decoding",
        ));
    }
    if dialect == HookDialect::Native
        && native::observation_only(event)
        && context.role == ResultRole::RequiredGate
    {
        return Err(WireError::new(
            "/role",
            "native observation-only event cannot carry a required gate",
        ));
    }
    if context.asynchronous && context.role == ResultRole::RequiredGate {
        return Err(WireError::new(
            "/role",
            "asynchronous observer cannot carry a required gate",
        ));
    }
    Ok(())
}
fn interpret_frame(
    profile: &CompatibilityProfile,
    context: &ResultContext,
    payload: transport::Payload,
    exit_two: bool,
    result: &mut DecodedResult,
) {
    use transport::Payload;
    let interpreted = match payload {
        Payload::Json(value) => {
            source::validate(profile, result.dialect, result.event, context, &value)
                .and_then(|()| effects::interpret(result, context, &value, exit_two))
        }
        Payload::Worktree(path) => effects::worktree(result, context, &path),
        Payload::Plain(text) => effects::plain(result, context, &text),
        Payload::Empty => Ok(()),
    };
    if let Err(error) = interpreted {
        // A malformed nested field invalidates that response atomically.
        result.effects.clear();
        result.fail(error);
    }
    if exit_two {
        effects::exit_two(result, context);
    }
    if result.event == HookEvent::WorktreeCreate
        && !result
            .effects
            .iter()
            .any(|effect| matches!(effect, ProposedEffect::WorktreePath(_)))
    {
        result.fail(WireError::new(
            "/worktreePath",
            "creation requires a valid worktree path",
        ));
    }
    if result.dialect == HookDialect::Native {
        effects::require_native_special(result);
    }
}

impl DecodedResult {
    fn fail(&mut self, detail: WireError) {
        if self.gate != GateDisposition::NotAGate {
            self.gate = GateDisposition::Held;
        }
        self.diagnostics.push(ResultDiagnostic {
            kind: DiagnosticKind::Failure,
            detail,
        });
    }
    fn ignored(&mut self, path: &'static str, problem: &'static str) {
        self.diagnostics.push(ResultDiagnostic {
            kind: DiagnosticKind::Ignored,
            detail: WireError::new(path, problem),
        });
    }
    fn push(&mut self, effect: ProposedEffect) {
        self.effects.push(effect);
    }
    fn finish(&mut self, context: &ResultContext) {
        if context.asynchronous {
            let old = self.effects.len();
            self.effects.retain(|effect| {
                !matches!(
                    effect,
                    ProposedEffect::Decision { .. }
                        | ProposedEffect::Control(_)
                        | ProposedEffect::RewriteInput(_)
                        | ProposedEffect::PermissionChanges(_)
                        | ProposedEffect::ReplaceModelOutput { .. }
                        | ProposedEffect::Elicitation { .. }
                        | ProposedEffect::WorktreePath(_)
                        | ProposedEffect::DisplayContent(_)
                        | ProposedEffect::SuppressOriginalPrompt
                        | ProposedEffect::RetryDeniedOperation
                )
            });
            if old != self.effects.len() {
                self.ignored(
                    "/",
                    "late observer control or result transformation ignored",
                );
            }
        }
        self.source_decision = if self.effects.iter().any(|e| {
            matches!(
                e,
                ProposedEffect::Decision {
                    choice: DecisionChoice::Deny | DecisionChoice::Ask | DecisionChoice::Defer,
                    ..
                } | ProposedEffect::Control(_)
                    | ProposedEffect::Feedback(_)
            )
        }) {
            SourceDecision::Objection
        } else if self.effects.iter().any(|e| {
            matches!(
                e,
                ProposedEffect::Decision {
                    choice: DecisionChoice::NoObjection,
                    ..
                }
            )
        }) {
            SourceDecision::NoObjection
        } else if self.effects.iter().any(|e| {
            matches!(
                e,
                ProposedEffect::WorktreePath(_)
                    | ProposedEffect::Elicitation { .. }
                    | ProposedEffect::DisplayContent(_)
            )
        }) {
            SourceDecision::SpecialResult
        } else {
            SourceDecision::NoSourceDecision
        };
        if self.gate != GateDisposition::NotAGate
            && self.source_decision == SourceDecision::Objection
        {
            self.gate = GateDisposition::Held;
        }
    }
}

impl fmt::Debug for ProposedEffect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Decision { choice, reason } => f
                .debug_struct("Decision")
                .field("choice", choice)
                .field("reason", reason)
                .finish(),
            Self::Control(request) => f.debug_tuple("Control").field(request).finish(),
            Self::ScheduleObserver { timeout_ms } => f
                .debug_struct("ScheduleObserver")
                .field("timeout_ms", timeout_ms)
                .finish(),
            Self::ReplaceDynamicWatches(paths) => f
                .debug_struct("ReplaceDynamicWatches")
                .field("path_count", &paths.len())
                .finish(),
            _ => f.write_str(match self {
                Self::RewriteInput(_) => "RewriteInput(<plugin-origin>)",
                Self::PermissionChanges(_) => "PermissionChanges(<plugin-origin>)",
                Self::AdditionalContext(_) => "AdditionalContext(<plugin-origin>)",
                Self::ClassifierContext(_) => "ClassifierContext(<plugin-origin>)",
                Self::Warning(_) => "Warning(<plugin-origin>)",
                Self::TransientNotice(_) => "TransientNotice(<plugin-origin>)",
                Self::Feedback(_) => "Feedback(<plugin-origin>)",
                Self::InitialMessage(_) => "InitialMessage(<plugin-origin>)",
                Self::SessionTitle(_) => "SessionTitle(<plugin-origin>)",
                Self::ReplaceModelOutput { .. } => "ReplaceModelOutput(<plugin-origin>)",
                Self::StageSkillRescan => "StageSkillRescan",
                Self::RetainContextLimitFailure => "RetainContextLimitFailure",
                Self::WorktreePath(_) => "WorktreePath(<plugin-origin>)",
                Self::Elicitation { .. } => "Elicitation(<plugin-origin>)",
                Self::DisplayContent(_) => "DisplayContent(<plugin-origin>)",
                Self::SuppressOriginalPrompt => "SuppressOriginalPrompt",
                Self::RetryDeniedOperation => "RetryDeniedOperation",
                Self::TerminalNotification(_) => "TerminalNotification(<plugin-origin>)",
                _ => unreachable!("metadata-only variants formatted above"),
            }),
        }
    }
}
