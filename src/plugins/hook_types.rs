//! Hook source semantics are distinct from package import layout.
use serde::{Deserialize, Serialize};

macro_rules! names {
    ($name:ident { $($variant:ident => $wire:literal),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        pub enum $name { $(#[serde(rename = $wire)] $variant),+ }
        impl $name {
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];
            pub const fn as_str(self) -> &'static str { match self { $(Self::$variant => $wire),+ } }
        }
    };
}
names!(HookDialect { Native => "native", Claude => "claude", Codex => "codex" });
names!(HandlerKind { Command => "command", Http => "http", McpTool => "mcp_tool", Prompt => "prompt", Agent => "agent" });
names!(HookEvent {
   PreToolUse => "PreToolUse",
   PostToolUse => "PostToolUse",
   PostToolUseFailure => "PostToolUseFailure",
   PostToolBatch => "PostToolBatch",
   Notification => "Notification",
   UserPromptSubmit => "UserPromptSubmit",
   UserPromptExpansion => "UserPromptExpansion",
   SessionStart => "SessionStart",
   SessionEnd => "SessionEnd",
   Stop => "Stop",
   StopFailure => "StopFailure",
   SubagentStart => "SubagentStart",
   SubagentStop => "SubagentStop",
   PreCompact => "PreCompact",
   PostCompact => "PostCompact",
   PreModelSwitch => "PreModelSwitch",
   PostModelSwitch => "PostModelSwitch",
   PermissionRequest => "PermissionRequest",
   PermissionDenied => "PermissionDenied",
   Setup => "Setup",
   TeammateIdle => "TeammateIdle",
   TaskCreated => "TaskCreated",
   TaskCompleted => "TaskCompleted",
   Elicitation => "Elicitation",
   ElicitationResult => "ElicitationResult",
   ConfigChange => "ConfigChange",
   WorktreeCreate => "WorktreeCreate",
   WorktreeRemove => "WorktreeRemove",
   InstructionsLoaded => "InstructionsLoaded",
   CwdChanged => "CwdChanged",
   FileChanged => "FileChanged",
   DirectoryAdded => "DirectoryAdded",
   MessageDisplay => "MessageDisplay",
   Interrupt => "Interrupt",
});
names!(Applicability { Run => "run", SourceNonexecuting => "source-nonexecuting", NoSourceHandler => "no-source-handler", NoSourceEvent => "no-source-event" });

/// Outcomes describe requested behavior, never execute it or grant host authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelOutcome {
    NoModelObjection,
    NoSourceDecision,
    EndTurnUnmet,
    ContinueAfterResult,
    ContinueWithFailure,
    DenyToolAndContinue,
    DenyToolAndEndTurn,
    StopUnmet,
    BoundedCorrection,
    RejectAndContinue,
    KeepWorking,
    HoldPendingAction,
    AttributedObservationOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskBoundary {
    ToolTransition,
    TeammateStop,
}

/// Constructed by the event owner from validated configuration and allocation state.
/// This is intentionally not deserializable from a handler response.
#[derive(Debug, Clone, Copy)]
pub struct ModelCallContext {
    pub continue_on_block: bool,
    pub task_boundary: TaskBoundary,
    pub cancelled: bool,
    pub allocation_available: bool,
    pub correction_available: bool,
}
impl Default for ModelCallContext {
    fn default() -> Self {
        Self {
            continue_on_block: false,
            task_boundary: TaskBoundary::ToolTransition,
            cancelled: false,
            allocation_available: true,
            correction_available: true,
        }
    }
}

/// Validated verdict. Fields are private so callers cannot bypass response validation.
#[derive(Debug, Clone)]
pub struct ModelVerdict {
    pub(crate) ok: bool,
    pub(crate) reason: Option<String>,
    pub(crate) impossible: bool,
    pub(crate) dialect: HookDialect,
    pub(crate) handler: HandlerKind,
}
impl ModelVerdict {
    pub fn ok(&self) -> bool {
        self.ok
    }
    pub fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }
    pub fn impossible(&self) -> bool {
        self.impossible
    }
}
