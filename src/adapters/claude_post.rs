//! Private SDK metadata capture and one-shot presentation delivery. Plugins run
//! at host completion, never again when Claude requests its source callback.
use crate::{
    events::EventSink,
    plugins::receipts::{LifecycleReceipt, ToolRepresentation},
    tools::{ToolCall, ToolExecutor, ToolResult},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
};
const EVENTS: [&str; 3] = ["PreToolUse", "PostToolUse", "PostToolUseFailure"];
pub(super) enum Presentation {
    Response { value: Value, call: String },
    Correction { call: String },
}

pub(super) fn user_uuid() -> Result<String> {
    let mut bytes = [0u8; 16];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let encoded = bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &encoded[..8],
        &encoded[8..12],
        &encoded[12..16],
        &encoded[16..20],
        &encoded[20..]
    ))
}
struct Entry {
    source: Value,
    invocation: u64,
    call: Option<String>,
    original: Option<ToolResult>,
}
pub(super) struct Callbacks {
    ids: BTreeMap<String, &'static str>,
    entries: BTreeMap<String, Entry>,
    invocation: Option<u64>,
    requests: BTreeSet<String>,
    profile: crate::plugins::profile::CompatibilityProfile,
}
impl Callbacks {
    pub(super) fn new() -> Result<Self> {
        let mut random = [0u8; 32];
        std::fs::File::open("/dev/urandom")?.read_exact(&mut random)?;
        let nonce = random
            .iter()
            .map(|v| format!("{v:02x}"))
            .collect::<String>();
        Ok(Self {
            ids: EVENTS
                .into_iter()
                .map(|e| (format!("{nonce}:{e}"), e))
                .collect(),
            entries: BTreeMap::new(),
            invocation: None,
            requests: BTreeSet::new(),
            profile: crate::plugins::profile::CompatibilityProfile::embedded()?,
        })
    }
    pub(super) fn begin_invocation(
        &mut self,
        events: &EventSink,
        correction_reserved: bool,
    ) -> Result<()> {
        // The reserved correction already owns a specific backend invocation;
        // metadata/tool admission still requires its exact user echo below.
        if !correction_reserved {
            events.ensure_continuation()?;
        }
        let invocation = Self::invocation(events)?;
        ensure!(
            self.invocation != Some(invocation),
            "source backend invocation repeated"
        );
        self.entries.clear();
        self.requests.clear();
        self.invocation = Some(invocation);
        Ok(())
    }
    pub(super) fn registration(&self, mut hooks: Value) -> Value {
        if !hooks.is_object() {
            hooks = json!({});
        }
        for (id, event) in &self.ids {
            hooks[*event] =
                json!([{"matcher":"mcp__demoncoder__.*","hookCallbackIds":[id],"timeout":3600}]);
        }
        hooks
    }
    pub(super) fn owns(&self, request: &Value) -> bool {
        request["callback_id"]
            .as_str()
            .is_some_and(|id| self.ids.contains_key(id))
    }
    fn invocation(events: &EventSink) -> Result<u64> {
        events
            .backend_invocation_id()
            .context("source callback lacks active backend invocation")
    }
    fn source_id(input: &Value) -> Result<&str> {
        input["tool_use_id"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 256)
            .context("source tool identity missing")
    }
    pub(super) fn metadata(
        &mut self,
        message: &Value,
        session: Option<&str>,
        events: &EventSink,
    ) -> Result<Value> {
        events.ensure_continuation()?;
        ensure!(
            self.invocation == Some(Self::invocation(events)?),
            "source metadata invocation is not active"
        );
        let input = self.validate_callback(message, session)?;
        ensure!(
            input["hook_event_name"] == "PreToolUse",
            "expected source metadata callback"
        );
        for field in ["cwd", "transcript_path"] {
            ensure!(
                input[field]
                    .as_str()
                    .is_some_and(|s| !s.is_empty() && s.len() <= 16384),
                "source metadata field missing"
            );
        }
        ensure!(
            input["tool_name"]
                .as_str()
                .is_some_and(|s| s.starts_with("mcp__demoncoder__"))
                && input["tool_input"].is_object(),
            "source tool metadata malformed"
        );
        self.profile
            .validate_claude_input(crate::plugins::hook_types::HookEvent::PreToolUse, &input)?;
        let id = Self::source_id(&input)?.to_owned();
        ensure!(
            self.entries.len() < 1024 && !self.entries.contains_key(&id),
            "source tool correlation duplicated or exhausted"
        );
        self.entries.insert(
            id,
            Entry {
                source: input,
                invocation: Self::invocation(events)?,
                call: None,
                original: None,
            },
        );
        Ok(json!({}))
    }
    fn validate_callback(&mut self, message: &Value, session: Option<&str>) -> Result<Value> {
        ensure!(
            serde_json::to_vec(message)?.len() <= 1024 * 1024,
            "source callback exceeds bound"
        );
        let request = &message["request"];
        let event = self
            .ids
            .get(
                request["callback_id"]
                    .as_str()
                    .context("callback identity missing")?,
            )
            .context("source callback unregistered")?;
        let input = &request["input"];
        ensure!(
            input["hook_event_name"] == *event
                && input["session_id"].as_str() == session
                && session.is_some(),
            "source callback event/session differs"
        );
        let id = Self::source_id(input)?;
        ensure!(
            request["tool_use_id"] == id,
            "source outer tool identity differs"
        );
        let request_id = message["request_id"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 256)
            .context("source callback request identity missing")?;
        ensure!(
            self.requests.len() < 3072 && self.requests.insert(request_id.into()),
            "source callback repeated or exhausted"
        );
        Ok(input.clone())
    }
    pub(super) fn permission(&self, request: &Value, events: &EventSink) -> Result<()> {
        events.ensure_continuation()?;
        let entry = self
            .entries
            .get(Self::source_id(request)?)
            .context("permission lacks source metadata")?;
        ensure!(
            entry.invocation == Self::invocation(events)?
                && entry.call.is_none()
                && entry.source["tool_name"] == request["tool_name"]
                && entry.source["tool_input"] == request["input"],
            "source permission correlation differs or is stale"
        );
        Ok(())
    }
    pub(super) fn correlate(
        &mut self,
        rpc: &Value,
        call: &ToolCall,
        events: &EventSink,
    ) -> Result<EventSink> {
        events.ensure_continuation()?;
        let id = rpc["params"]["_meta"]["claudecode/toolUseId"]
            .as_str()
            .context("MCP source tool identity missing")?;
        let entry = self
            .entries
            .get_mut(id)
            .context("MCP source metadata missing")?;
        ensure!(
            entry.invocation == Self::invocation(events)?
                && entry.call.is_none()
                && entry.source["tool_name"] == format!("mcp__demoncoder__{}", call.name)
                && entry.source["tool_input"] == call.arguments,
            "MCP source tool correlation differs or repeats"
        );
        entry.call = Some(call.id.clone());
        Ok(
            events.with_tool_representation(ToolRepresentation::ClaudeMcp {
                tool_name: entry.source["tool_name"].as_str().expect("checked").into(),
                tool_use_id: id.into(),
                source_input: entry.source.clone(),
            }),
        )
    }
    pub(super) fn completed(
        &mut self,
        call_id: &str,
        events: &EventSink,
        fallback: &ToolResult,
    ) -> Result<ToolResult> {
        let original = if let Some((runtime, id)) = events.post_delivery_context(call_id)? {
            runtime.validate_post_delivery_owner(id)?;
            runtime
                .record()?
                .operations
                .into_iter()
                .find(|o| o.id == id)
                .and_then(|o| o.result)
                .context("completed original missing")?
        } else {
            events.validate_hook_delivery()?;
            fallback.clone()
        };
        let entry = self
            .entries
            .values_mut()
            .find(|e| e.call.as_deref() == Some(call_id))
            .context("completed source correlation missing")?;
        ensure!(entry.original.is_none(), "completed source tool repeated");
        entry.original = Some(original.clone());
        Ok(original)
    }
    pub(super) async fn presentation(
        &mut self,
        message: &Value,
        session: Option<&str>,
        events: &EventSink,
        tools: &ToolExecutor,
    ) -> Result<Presentation> {
        let input = self.validate_callback(message, session)?;
        let entry = self
            .entries
            .get(Self::source_id(&input)?)
            .context("source completion metadata missing")?;
        ensure!(
            entry.invocation == Self::invocation(events)?,
            "source completion invocation stale"
        );
        for field in [
            "session_id",
            "transcript_path",
            "cwd",
            "permission_mode",
            "agent_id",
            "agent_type",
            "prompt_id",
            "effort",
            "tool_name",
            "tool_input",
            "tool_use_id",
        ] {
            ensure!(
                input.get(field) == entry.source.get(field),
                "source completion metadata differs: {field}"
            );
        }
        let original = entry
            .original
            .as_ref()
            .context("source callback precedes completed host tool")?;
        let expected = if original.success {
            "PostToolUse"
        } else {
            "PostToolUseFailure"
        };
        ensure!(
            input["hook_event_name"] == expected,
            "source completion differs from original outcome"
        );
        if original.success {
            ensure!(
                input["tool_response"]
                    == json!([{"type":"text","text":serde_json::to_string(original)?}]),
                "source completion response differs from retained original"
            );
        } else {
            ensure!(
                input["error"] == serde_json::to_string(original)?
                    && input["is_interrupt"] == false,
                "source failure differs from retained original"
            );
        }
        let call = entry
            .call
            .as_ref()
            .context("source host correlation missing")?
            .clone();
        if events.post_continuation(&call)?
            == crate::plugins::receipts::PostContinuation::Correction
        {
            return Ok(Presentation::Correction { call });
        }
        tools.validate_post_release(&call, events).await?;
        let receipt: Option<LifecycleReceipt> = events
            .post_delivery_context(&call)?
            .map(|(runtime, id)| runtime.post_tool_receipt(id))
            .transpose()?
            .flatten();
        let mut response = json!({});
        if let Some(receipt) = receipt {
            let mut specific = json!({"hookEventName":expected});
            if let Some(value) = receipt.model_content {
                specific["updatedMCPToolOutput"] = value;
            }
            if !receipt.messages.is_empty() {
                specific["additionalContext"] = json!(
                    receipt
                        .messages
                        .iter()
                        .map(|m| format!("[Plugin-origin {}] {}", m.package, m.text))
                        .collect::<Vec<_>>()
                        .join("\n")
                );
            }
            response["hookSpecificOutput"] = specific;
        }
        events.reserve_post_delivery(&call)?;
        Ok(Presentation::Response {
            value: response,
            call,
        })
    }
}
