//! Native wire v1 deliberately uses the documented synchronous envelope vocabulary.
//! It has its own exhaustive event field table and strict validation, rather than
//! treating absent upstream events as a source fallback. All fields are optional
//! except hookEventName in a specific object. WorktreeCreate requires worktreePath;
//! Elicitation/Result require action or content, and MessageDisplay requires
//! displayContent. Generic Boolean model verdicts cannot supply these results.
//! Permission update entries reuse the frozen PermissionUpdate data vocabulary.
//! suppressOutput is accepted only for compatibility and has no Native effect.
//! SuppressOriginalPrompt and displayContent are the explicit display proposals.
use super::*;
use crate::plugins::wire::{self, WireResult};

pub(super) const UNIVERSAL_FIELDS: &[&str] = &[
    "continue",
    "stopReason",
    "suppressOutput",
    "decision",
    "reason",
    "systemMessage",
    "terminalSequence",
    "hookSpecificOutput",
    "async",
    "asyncTimeout",
];

/// This is the complete Native event-specific field vocabulary. Missing source
/// events still have explicit Native rules; these fields do not select authority.
pub(super) fn specific_fields(event: HookEvent) -> &'static [&'static str] {
    use HookEvent::*;
    match event {
        SessionStart => &[
            "additionalContext",
            "initialUserMessage",
            "sessionTitle",
            "watchPaths",
            "reloadSkills",
        ],
        UserPromptSubmit => &[
            "additionalContext",
            "sessionTitle",
            "suppressOriginalPrompt",
        ],
        UserPromptExpansion => &["additionalContext", "suppressOriginalPrompt"],
        PreToolUse => &[
            "permissionDecision",
            "permissionDecisionReason",
            "updatedInput",
            "additionalContext",
        ],
        PreModelSwitch => &["permissionDecision", "permissionDecisionReason"],
        PermissionRequest => &["decision"],
        PermissionDenied => &["retry"],
        PostToolUse => &[
            "additionalContext",
            "classifierContext",
            "updatedToolOutput",
            "updatedMCPToolOutput",
        ],
        PostToolUseFailure | PostToolBatch | Stop | SubagentStart | SubagentStop
        | PostModelSwitch | Setup | Notification => &["additionalContext"],
        WorktreeCreate => &["worktreePath"],
        FileChanged | CwdChanged => &["watchPaths"],
        MessageDisplay => &["displayContent"],
        Elicitation | ElicitationResult => &["action", "content"],
        InstructionsLoaded | StopFailure | Interrupt | SessionEnd | TaskCreated | TaskCompleted
        | TeammateIdle | WorktreeRemove | PreCompact | PostCompact | ConfigChange
        | DirectoryAdded => &[],
    }
}

pub(super) fn validate(
    profile: &CompatibilityProfile,
    event: HookEvent,
    value: &Value,
) -> WireResult {
    wire::measure(value)?;
    let object = value
        .as_object()
        .ok_or_else(|| WireError::new("/", "hook response must be an object"))?;
    for (key, value) in object {
        if !UNIVERSAL_FIELDS.contains(&key.as_str()) {
            return Err(WireError::new("/", "unknown native output field"));
        }
        let valid = match key.as_str() {
            "continue" | "suppressOutput" => value.is_boolean(),
            "stopReason" | "reason" | "systemMessage" | "terminalSequence" => value.is_string(),
            "decision" => matches!(value.as_str(), Some("approve" | "block")),
            "async" => value == true,
            "asyncTimeout" => value.is_number(),
            "hookSpecificOutput" => value.is_object(),
            _ => false,
        };
        if !valid {
            return Err(WireError::new("/", "invalid native output field type"));
        }
    }
    if object.contains_key("async") {
        if object
            .keys()
            .any(|key| !matches!(key.as_str(), "async" | "asyncTimeout"))
        {
            return Err(WireError::new(
                "/async",
                "async response cannot contain synchronous effects",
            ));
        }
        return Ok(());
    }
    if object.contains_key("asyncTimeout") {
        return Err(WireError::new(
            "/asyncTimeout",
            "asyncTimeout requires async:true",
        ));
    }
    if let Some(specific) = object.get("hookSpecificOutput") {
        if specific["hookEventName"] != event.as_str() {
            return Err(WireError::new(
                "/hookSpecificOutput/hookEventName",
                "event identity missing or mismatched",
            ));
        }
        for (key, value) in specific.as_object().expect("validated specific object") {
            if key == "hookEventName" {
                continue;
            }
            if !specific_fields(event).contains(&key.as_str()) {
                return Err(WireError::new(
                    "/hookSpecificOutput",
                    "field is not part of this native event",
                ));
            }
            validate_field(profile, event, key, value)?;
        }
    }
    Ok(())
}

fn validate_field(
    profile: &CompatibilityProfile,
    event: HookEvent,
    key: &str,
    value: &Value,
) -> WireResult {
    let valid = match key {
        "additionalContext"
        | "classifierContext"
        | "initialUserMessage"
        | "sessionTitle"
        | "displayContent"
        | "permissionDecisionReason"
        | "worktreePath" => value.is_string(),
        "suppressOriginalPrompt" | "reloadSkills" | "retry" => value.is_boolean(),
        "updatedInput" | "content" => value.is_object(),
        "updatedToolOutput" | "updatedMCPToolOutput" => true,
        "watchPaths" => value
            .as_array()
            .is_some_and(|a| a.iter().all(Value::is_string)),
        "permissionDecision" => {
            matches!(value.as_str(), Some("allow" | "deny" | "ask"))
                || (event == HookEvent::PreToolUse && value == "defer")
        }
        "action" => matches!(value.as_str(), Some("accept" | "decline" | "cancel")),
        "decision" => {
            // The decision is data only; source permission changes cannot grant access.
            profile.validate_claude_type(
                "PermissionRequestHookSpecificOutput",
                &serde_json::json!({
                    "hookEventName":"PermissionRequest", "decision":value
                }),
            )?;
            true
        }
        _ => false,
    };
    if !valid {
        return Err(WireError::new(
            "/hookSpecificOutput",
            "invalid native event field type",
        ));
    }
    Ok(())
}

pub(super) fn observation_only(event: HookEvent) -> bool {
    use HookEvent::*;
    match event {
        SessionStart | InstructionsLoaded | PermissionDenied | PostToolBatch | StopFailure
        | Interrupt | SessionEnd | SubagentStart | WorktreeRemove | PostCompact
        | PostModelSwitch | Setup | Notification | FileChanged | CwdChanged | DirectoryAdded
        | MessageDisplay => true,
        UserPromptSubmit | UserPromptExpansion | PreToolUse | PermissionRequest | PostToolUse
        | PostToolUseFailure | Stop | TaskCreated | TaskCompleted | SubagentStop | TeammateIdle
        | WorktreeCreate | PreCompact | PreModelSwitch | ConfigChange | Elicitation
        | ElicitationResult => false,
    }
}
