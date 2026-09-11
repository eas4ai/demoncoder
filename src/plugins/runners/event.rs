//! Shared bounded host event framing for every synchronous tool runner.
use crate::plugins::{
    dispatch::HookInvocation,
    hook_types::{HookDialect, HookEvent},
    profile::{CompatibilityProfile, SchemaKey},
};
use anyhow::{Context, Result, ensure};
pub(super) struct EventInput<'a> {
    pub dialect: HookDialect,
    pub maximum: usize,
    pub model: Option<&'a str>,
    pub permission_mode: &'a str,
    pub transcript_path: Option<&'a str>,
}
pub(super) fn input(
    invocation: &HookInvocation,
    profile: &CompatibilityProfile,
    config: EventInput<'_>,
) -> Result<Vec<u8>> {
    let EventInput {
        dialect,
        maximum,
        model,
        permission_mode,
        transcript_path,
    } = config;
    use serde_json::json;
    if let Some(facts) = &invocation.lifecycle {
        return non_tool_input(invocation, facts, profile, dialect, maximum);
    }
    let candidate = invocation
        .candidate
        .as_ref()
        .context("tool event lacks a tool candidate")?;
    crate::plugins::wire::measure(&candidate.arguments)?;
    let mut preflight = LimitedInput {
        bytes: Vec::new(),
        maximum,
    };
    serde_json::to_writer(&mut preflight, &candidate.arguments)
        .context("hook event input exceeds configured bound")?;
    drop(preflight);
    let event = invocation.events.plugin_event();
    ensure!(
        invocation.key.event == event.as_str(),
        "hook event capability mismatch"
    );
    let mut input = json!({"session_id":invocation.key.session,"cwd":invocation.host.workspace,"hook_event_name":event.as_str(),"tool_name":candidate.name,"tool_input":candidate.arguments,"tool_use_id":candidate.id});
    if event == HookEvent::PreToolUse {
        ensure!(
            invocation.completed.is_none(),
            "pre-tool input cannot contain completed authority"
        );
        if dialect == HookDialect::Codex {
            input["model"] = json!(model);
            input["permission_mode"] = json!(permission_mode);
            input["transcript_path"] = json!(transcript_path);
            input["turn_id"] = json!(invocation.key.source_operation.to_string());
        } else if dialect == HookDialect::Claude {
            input["transcript_path"] = json!(transcript_path.unwrap_or(""));
            input["permission_mode"] = json!(permission_mode);
        }
    } else {
        ensure!(
            matches!(
                event,
                HookEvent::PostToolUse | HookEvent::PostToolUseFailure
            ),
            "runner event is not integrated"
        );
        let completed = invocation
            .completed
            .as_ref()
            .context("post-tool input requires completed host evidence")?;
        ensure!(
            completed.facts.event == event
                && completed.facts.operation == invocation.key.operation
                && completed.original.call_id == candidate.id
                && completed.original.tool == candidate.name
                && completed.original.success == (event == HookEvent::PostToolUse),
            "post-tool evidence identity mismatch"
        );
        let facts = &completed.facts;
        input["permission_mode"] = json!(facts.host_permission_mode);
        input["transcript_path"] = json!(facts.host_transcript_path);
        if dialect == HookDialect::Codex {
            input["model"] = json!(facts.host_model);
            input["turn_id"] = json!(facts.source_operation.to_string());
        }
        let mut response = serde_json::to_value(&completed.original)?;
        let mut error = completed.original.output.clone();
        use crate::plugins::receipts::ToolRepresentation;
        match &facts.representation {
            ToolRepresentation::Native => {}
            ToolRepresentation::ClaudeMcp {
                tool_name,
                tool_use_id,
                source_input,
            } => {
                // These fields were observed on the adapter's private SDK channel;
                // the host post event is still a translation of actual completion.
                if dialect == HookDialect::Claude {
                    for name in [
                        "session_id",
                        "transcript_path",
                        "cwd",
                        "permission_mode",
                        "agent_id",
                        "agent_type",
                        "prompt_id",
                        "effort",
                    ] {
                        if let Some(value) = source_input.get(name) {
                            input[name] = value.clone();
                        } else {
                            input.as_object_mut().expect("object").remove(name);
                        }
                    }
                }
                input["tool_name"] = json!(tool_name);
                input["tool_use_id"] = json!(tool_use_id);
                error = serde_json::to_string(&completed.original)?;
                response = json!([{"type":"text", "text":error}]);
            }
            ToolRepresentation::CodexDynamic {
                tool_use_id,
                turn_id,
                session_id,
                model,
                permission_mode,
                transcript_path,
            } => {
                input["session_id"] = json!(session_id);
                input["tool_use_id"] = json!(tool_use_id);
                if dialect == HookDialect::Codex {
                    input["turn_id"] = json!(turn_id);
                    input["model"] = json!(model);
                    input["permission_mode"] = json!(permission_mode);
                    input["transcript_path"] = json!(transcript_path);
                }
                response = json!(serde_json::to_string(&completed.original)?);
            }
        }
        if event == HookEvent::PostToolUse {
            input["tool_response"] = response;
        } else {
            input["error"] = json!(error);
            if dialect == HookDialect::Claude {
                input["is_interrupt"] = json!(false);
            }
        }
        if dialect == HookDialect::Native {
            input["demoncoder"] = serde_json::to_value(facts)?;
        }
    }
    match dialect {
        HookDialect::Codex => {
            let name = match event {
                HookEvent::PreToolUse => "pre-tool-use",
                HookEvent::PostToolUse => "post-tool-use",
                _ => anyhow::bail!("Codex has no source post-tool failure event"),
            };
            profile.validate_schema(
                &SchemaKey::Codex {
                    path: format!(
                        "codex-rs/hooks/schema/generated/{name}.command.input.schema.json"
                    ),
                    definition: None,
                },
                &input,
            )?;
        }
        HookDialect::Claude => profile.validate_claude_input(event, &input)?,
        HookDialect::Native => {}
    }
    let mut bytes = LimitedInput {
        bytes: Vec::new(),
        maximum,
    };
    serde_json::to_writer(&mut bytes, &input)
        .context("hook event input exceeds configured bound")?;
    Ok(bytes.bytes)
}
pub(super) struct LimitedInput {
    pub(super) bytes: Vec<u8>,
    pub(super) maximum: usize,
}
impl std::io::Write for LimitedInput {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.maximum.saturating_sub(self.bytes.len()) {
            return Err(std::io::Error::other("hook input limit"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn non_tool_input(
    invocation: &HookInvocation,
    facts: &crate::plugins::receipts::NonToolFacts,
    profile: &CompatibilityProfile,
    dialect: HookDialect,
    maximum: usize,
) -> Result<Vec<u8>> {
    use crate::plugins::receipts::{NonToolOccurrence, ObservedLifecycle};
    use serde_json::json;
    let event = facts.subject.occurrence.event();
    ensure!(
        invocation.candidate.is_none()
            && invocation.completed.is_none()
            && invocation.key.tool.is_none()
            && invocation.key.arguments.is_none()
            && invocation.key.lifecycle.as_ref() == Some(&facts.subject)
            && invocation.key.event == event.as_str()
            && invocation.events.plugin_event() == event
            && invocation.key.operation == facts.operation
            && invocation.key.session == facts.session,
        "lifecycle input conflicts with its typed event capability"
    );
    let input = match dialect {
        HookDialect::Native => {
            let mut value = json!({"session_id":facts.session,"cwd":invocation.host.workspace,
                "hook_event_name":event.as_str(),"demoncoder":facts});
            match &facts.subject.occurrence {
                NonToolOccurrence::UserPromptSubmit { prompt, .. } => {
                    value["prompt"] = json!(prompt)
                }
                NonToolOccurrence::Stop {
                    stop_hook_active,
                    last_assistant_message,
                } => {
                    value["stop_hook_active"] = json!(stop_hook_active);
                    if let Some(text) = last_assistant_message {
                        value["last_assistant_message"] = json!(text);
                    }
                }
            }
            value
        }
        HookDialect::Claude => {
            let input = match &facts.source {
                Some(ObservedLifecycle::Claude(input)) => input.clone(),
                _ => translated_non_tool_input(invocation, facts, dialect)?,
            };
            ensure!(
                input["hook_event_name"] == event.as_str(),
                "Claude observed lifecycle event differs"
            );
            profile.validate_claude_input(event, &input)?;
            input
        }
        HookDialect::Codex => {
            let input = match &facts.source {
                Some(ObservedLifecycle::Codex(input)) => input.clone(),
                _ => translated_non_tool_input(invocation, facts, dialect)?,
            };
            ensure!(
                input["hook_event_name"] == event.as_str(),
                "Codex observed lifecycle event differs"
            );
            let name = match event {
                HookEvent::UserPromptSubmit => "user-prompt-submit",
                HookEvent::Stop => "stop",
                _ => unreachable!(),
            };
            profile.validate_schema(
                &SchemaKey::Codex {
                    path: format!(
                        "codex-rs/hooks/schema/generated/{name}.command.input.schema.json"
                    ),
                    definition: None,
                },
                &input,
            )?;
            input
        }
    };
    crate::plugins::wire::measure(&input)?;
    let mut bytes = LimitedInput {
        bytes: Vec::new(),
        maximum,
    };
    serde_json::to_writer(&mut bytes, &input)
        .context("hook lifecycle input exceeds configured bound")?;
    Ok(bytes.bytes)
}

fn translated_non_tool_input(
    invocation: &HookInvocation,
    facts: &crate::plugins::receipts::NonToolFacts,
    dialect: HookDialect,
) -> Result<serde_json::Value> {
    use crate::plugins::receipts::NonToolOccurrence;
    use serde_json::json;
    if let Some(source) = &facts.source {
        return translated_source_input(facts, source, invocation.key.source_operation, dialect);
    }
    let turn = facts
        .native_turn
        .context("source lifecycle translation requires an actual native turn")?;
    ensure!(
        invocation.key.source_operation == turn
            && facts.provenance.as_deref() == Some("native_host_translation_v1")
            && !facts.host_transcript_path.is_empty(),
        "native lifecycle translation provenance or turn binding missing"
    );
    let mut input = json!({"session_id":facts.session,"cwd":invocation.host.workspace,
        "hook_event_name":facts.subject.occurrence.event().as_str(),
        "transcript_path":facts.host_transcript_path,"permission_mode":facts.host_permission_mode});
    if dialect == HookDialect::Codex {
        input["turn_id"] = json!(turn.to_string());
        input["model"] = json!(
            facts
                .host_model
                .as_deref()
                .filter(|s| !s.is_empty())
                .context("Codex lifecycle translation requires the actual native model")?
        );
    }
    match &facts.subject.occurrence {
        NonToolOccurrence::UserPromptSubmit { prompt, .. } => input["prompt"] = json!(prompt),
        NonToolOccurrence::Stop {
            stop_hook_active,
            last_assistant_message,
        } => {
            input["stop_hook_active"] = json!(stop_hook_active);
            if dialect == HookDialect::Codex || last_assistant_message.is_some() {
                input["last_assistant_message"] = json!(last_assistant_message);
            }
        }
    }
    Ok(input)
}

fn translated_source_input(
    facts: &crate::plugins::receipts::NonToolFacts,
    source: &crate::plugins::receipts::ObservedLifecycle,
    source_operation: u64,
    dialect: HookDialect,
) -> Result<serde_json::Value> {
    use crate::plugins::receipts::NonToolOccurrence;
    use serde_json::json;
    let callback = facts
        .callback
        .as_ref()
        .context("source translation lacks callback ownership")?;
    ensure!(
        facts.native_turn.is_none()
            && source_operation == callback.backend_operation
            && facts.provenance.as_deref() == Some("authenticated_source_callback_v1"),
        "source translation ownership differs"
    );
    let source = match source {
        crate::plugins::receipts::ObservedLifecycle::Claude(v)
        | crate::plugins::receipts::ObservedLifecycle::Codex(v) => v,
    };
    let mut input = json!({});
    for key in [
        "session_id",
        "transcript_path",
        "cwd",
        "permission_mode",
        "hook_event_name",
    ] {
        input[key] = source
            .get(key)
            .cloned()
            .with_context(|| format!("source translation lacks {key}"))?;
    }
    if dialect == HookDialect::Codex {
        // This is the host causal backend invocation, not an invented source turn.
        input["turn_id"] = json!(callback.backend_operation.to_string());
        input["model"] = json!(
            callback
                .model
                .as_deref()
                .or(facts.host_model.as_deref())
                .filter(|s| !s.is_empty())
                .context("source translation lacks an observed or configured model")?
        );
    }
    match &facts.subject.occurrence {
        NonToolOccurrence::UserPromptSubmit { prompt, .. } => input["prompt"] = json!(prompt),
        NonToolOccurrence::Stop {
            stop_hook_active,
            last_assistant_message,
        } => {
            input["stop_hook_active"] = json!(stop_hook_active);
            if dialect == HookDialect::Codex || last_assistant_message.is_some() {
                input["last_assistant_message"] = json!(last_assistant_message);
            }
        }
    }
    Ok(input)
}

#[cfg(test)]
mod source_tests {
    use super::*;
    use crate::plugins::receipts::*;
    use serde_json::json;
    fn facts() -> NonToolFacts {
        serde_json::from_value(json!({
            "callback":{"backend_operation":7,"sequence":1,"request_id":"request","command_uuid":"command","envelope_id":"envelope","model":"configured-model"},
            "provenance":"authenticated_source_callback_v1","host_transcript_path":"/host/state.json","host_model":null,"host_permission_mode":"default",
            "session":"host-session","operation":8,"task":null,"role":"worker","workspace":[1,2],
            "subject":{"version":1,"occurrence":{"event":"UserPromptSubmit","prompt":"actual prompt","correction":false}}
        })).unwrap()
    }
    #[test]
    fn translation_retains_actual_source_facts_and_requires_known_model() -> Result<()> {
        let mut facts = facts();
        let source = ObservedLifecycle::Claude(
            json!({"session_id":"source-session","transcript_path":"/source/transcript.jsonl","cwd":"/source/work","permission_mode":"default","hook_event_name":"UserPromptSubmit","prompt":"actual prompt","prompt_id":"source-prompt"}),
        );
        let value = translated_source_input(&facts, &source, 7, HookDialect::Codex)?;
        assert_eq!(value["session_id"], "source-session");
        assert_eq!(value["transcript_path"], "/source/transcript.jsonl");
        assert_eq!(value["turn_id"], "7");
        assert!(value.get("prompt_id").is_none() && value.get("demoncoder").is_none());
        CompatibilityProfile::embedded()?.validate_schema(
            &SchemaKey::Codex {
                path:
                    "codex-rs/hooks/schema/generated/user-prompt-submit.command.input.schema.json"
                        .into(),
                definition: None,
            },
            &value,
        )?;
        assert!(translated_source_input(&facts, &source, 99, HookDialect::Codex).is_err());
        facts.callback.as_mut().unwrap().model = None;
        assert!(translated_source_input(&facts, &source, 7, HookDialect::Codex).is_err());
        facts.host_model = Some("explicit-model".into());
        assert_eq!(
            translated_source_input(&facts, &source, 7, HookDialect::Codex)?["model"],
            "explicit-model"
        );
        facts.provenance = None;
        assert!(translated_source_input(&facts, &source, 7, HookDialect::Codex).is_err());
        Ok(())
    }
}
