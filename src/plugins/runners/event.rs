//! Shared bounded source PreToolUse framing for command and HTTP transports.
use crate::plugins::{
    dispatch::HookInvocation,
    hook_types::{HookDialect, HookEvent},
    profile::{CompatibilityProfile, SchemaKey},
};
use anyhow::{Context, Result};
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
    crate::plugins::wire::measure(&invocation.candidate.arguments)?;
    let mut preflight = LimitedInput {
        bytes: Vec::new(),
        maximum,
    };
    serde_json::to_writer(&mut preflight, &invocation.candidate.arguments)
        .context("hook event input exceeds configured bound")?;
    drop(preflight);
    let mut input = json!({"session_id":invocation.key.session,"cwd":invocation.host.workspace,"hook_event_name":"PreToolUse","tool_name":invocation.candidate.name,"tool_input":invocation.candidate.arguments,"tool_use_id":invocation.candidate.id});
    match dialect {
        HookDialect::Codex => {
            input["model"] = json!(model);
            input["permission_mode"] = json!(permission_mode);
            input["transcript_path"] = json!(transcript_path);
            input["turn_id"] = json!(invocation.key.source_operation.to_string());
            profile.validate_schema(
                &SchemaKey::Codex {
                    path: "codex-rs/hooks/schema/generated/pre-tool-use.command.input.schema.json"
                        .into(),
                    definition: None,
                },
                &input,
            )?;
        }
        HookDialect::Claude => {
            input["transcript_path"] = json!(transcript_path.unwrap_or(""));
            input["permission_mode"] = json!(permission_mode);
            profile.validate_claude_input(HookEvent::PreToolUse, &input)?;
        }
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
