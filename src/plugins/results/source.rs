//! Source shape and source-rejected semantic fields are separate checks.
//! Behavior is derived from the frozen contract and the read-only Codex v0.153.4
//! output_parser/event parsers. No upstream implementation code is copied here.
use super::*;
use crate::plugins::{profile::SchemaKey, wire::WireResult};

pub(super) fn validate(
    profile: &CompatibilityProfile,
    dialect: HookDialect,
    event: HookEvent,
    context: &ResultContext,
    value: &Value,
) -> WireResult {
    match dialect {
        HookDialect::Native => super::native::validate(profile, event, value),
        HookDialect::Claude => profile.validate_claude_output(event, value),
        HookDialect::Codex => {
            if event == HookEvent::SessionEnd {
                return Ok(());
            } // Codex does not read stdout.
            let name = match event {
                HookEvent::Interrupt => "interrupt",
                HookEvent::PermissionRequest => "permission-request",
                HookEvent::PostCompact => "post-compact",
                HookEvent::PostToolUse => "post-tool-use",
                HookEvent::PreCompact => "pre-compact",
                HookEvent::PreToolUse => "pre-tool-use",
                HookEvent::SessionStart => "session-start",
                HookEvent::Stop => "stop",
                HookEvent::SubagentStart => "subagent-start",
                HookEvent::SubagentStop => "subagent-stop",
                HookEvent::UserPromptSubmit => "user-prompt-submit",
                _ => return Err(WireError::new("/event", "no Codex response schema")),
            };
            profile.validate_schema(
                &SchemaKey::Codex {
                    path: format!(
                        "codex-rs/hooks/schema/generated/{name}.command.output.schema.json"
                    ),
                    definition: None,
                },
                value,
            )?;
            // Codex event readers apply semantic control rejection only to
            // synchronous handlers. Schema errors remain failures for observers.
            if context.asynchronous || codex_stopping(event, value) {
                Ok(())
            } else {
                validate_codex_effects(event, value)
            }
        }
    }
}

/// These four Codex readers stop before inspecting semantic control errors.
/// Their context eligibility still depends on those errors (checked separately).
pub(super) fn codex_stopping(event: HookEvent, value: &Value) -> bool {
    value["continue"] == false
        && matches!(
            event,
            HookEvent::Stop
                | HookEvent::SubagentStop
                | HookEvent::UserPromptSubmit
                | HookEvent::PostToolUse
        )
}

pub(super) fn validate_codex_effects(event: HookEvent, value: &Value) -> WireResult {
    use HookEvent::*;
    let specific = &value["hookSpecificOutput"];
    if matches!(event, PreToolUse | PermissionRequest) {
        if value["continue"] == false {
            return rejected("/continue");
        }
        if value.get("stopReason").is_some() {
            return rejected("/stopReason");
        }
    }
    if matches!(event, PreToolUse | PermissionRequest | PostToolUse)
        && value["suppressOutput"] == true
    {
        return rejected("/suppressOutput");
    }
    match event {
        PreToolUse => validate_codex_pretool(value),
        PermissionRequest => {
            let decision = &specific["decision"];
            for key in ["updatedInput", "updatedPermissions"] {
                if codex_value_present(decision, key) {
                    return rejected("/hookSpecificOutput/decision");
                }
            }
            if decision["interrupt"] == true {
                return rejected("/hookSpecificOutput/decision/interrupt");
            }
            Ok(())
        }
        PostToolUse => {
            if codex_value_present(specific, "updatedMCPToolOutput") {
                return rejected("/hookSpecificOutput/updatedMCPToolOutput");
            }
            require_block_reason(value)?;
            if value.get("reason").is_some()
                && value.get("decision").is_none()
                && value["continue"] != false
            {
                return rejected("/reason");
            }
            Ok(())
        }
        UserPromptSubmit | Stop | SubagentStop => require_block_reason(value),
        _ => Ok(()),
    }
}
fn validate_codex_pretool(value: &Value) -> WireResult {
    let specific = &value["hookSpecificOutput"];
    if specific_decision(specific) {
        let decision = specific["permissionDecision"].as_str();
        if codex_value_present(specific, "updatedInput") && decision != Some("allow") {
            return rejected("/hookSpecificOutput/updatedInput");
        }
        match decision {
            Some("allow") if !codex_value_present(specific, "updatedInput") => {
                rejected("/hookSpecificOutput/permissionDecision")
            }
            Some("ask") => rejected("/hookSpecificOutput/permissionDecision"),
            Some("deny") if nonempty(&specific["permissionDecisionReason"]).is_none() => {
                rejected("/hookSpecificOutput/permissionDecisionReason")
            }
            None if specific.get("permissionDecisionReason").is_some() => {
                rejected("/hookSpecificOutput/permissionDecisionReason")
            }
            _ => Ok(()),
        }
    } else {
        if value["decision"] == "approve" {
            return rejected("/decision");
        }
        if value.get("reason").is_some() && value.get("decision").is_none() {
            return rejected("/reason");
        }
        require_block_reason(value)
    }
}
pub(super) fn specific_decision(value: &Value) -> bool {
    [
        "permissionDecision",
        "permissionDecisionReason",
        "updatedInput",
    ]
    .iter()
    .any(|key| codex_value_present(value, key))
}
fn require_block_reason(value: &Value) -> WireResult {
    if value["decision"] == "block" && nonempty(&value["reason"]).is_none() {
        return rejected("/reason");
    }
    Ok(())
}
fn rejected(path: &'static str) -> WireResult {
    Err(WireError::new(
        path,
        "field is source-rejected for this event",
    ))
}
pub(super) fn nonempty(value: &Value) -> Option<&str> {
    value.as_str().map(str::trim).filter(|s| !s.is_empty())
}

/// Codex uses Option<Value> for untyped fields: an explicit null is absent.
/// Apply only after the exact frozen schema validated all other field types.
pub(super) fn codex_value_present(value: &Value, key: &str) -> bool {
    value.get(key).is_some_and(|value| !value.is_null())
}
