//! Genuine SDK prompt and Stop boundaries; no tool authority is inferred from
//! the SDK's unfortunately named outer `tool_use_id` correlation field.
use crate::{
    events::EventSink,
    plugins::{profile::CompatibilityProfile, receipts::*},
    tools::ToolExecutor,
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    path::PathBuf,
    time::Duration,
};

pub(super) struct Callbacks {
    ids: BTreeMap<String, &'static str>,
    requests: BTreeSet<String>,
    envelopes: BTreeSet<String>,
    profile: CompatibilityProfile,
    workspace: PathBuf,
    backend: Option<u64>,
    prompt: String,
    command_uuid: String,
    command_started: bool,
    prompt_id: Option<String>,
    transcript: Option<String>,
    origin: SourceOrigin,
    retired_command: Option<(String, &'static str)>,
    sequence: u64,
    corrections: u32,
    assistant: bool,
    assistant_text: Option<String>,
    response_open: bool,
    text_block: Option<(u64, String)>,
    accepted: bool,
    awaiting_correction: bool,
    pending: Option<u64>,
    model: Option<String>,
}
impl Callbacks {
    pub(super) fn new(workspace: PathBuf, model: Option<String>) -> Result<Self> {
        let mut random = [0u8; 32];
        std::fs::File::open("/dev/urandom")?.read_exact(&mut random)?;
        let nonce = random
            .iter()
            .map(|v| format!("{v:02x}"))
            .collect::<String>();
        Ok(Self {
            ids: ["UserPromptSubmit", "Stop"]
                .into_iter()
                .map(|event| (format!("{nonce}:{event}"), event))
                .collect(),
            requests: BTreeSet::new(),
            envelopes: BTreeSet::new(),
            profile: CompatibilityProfile::embedded()?,
            workspace,
            backend: None,
            prompt: String::new(),
            command_uuid: String::new(),
            command_started: false,
            prompt_id: None,
            transcript: None,
            origin: SourceOrigin::HostSubmission,
            retired_command: None,
            sequence: 0,
            corrections: 0,
            assistant: false,
            assistant_text: None,
            response_open: false,
            text_block: None,
            accepted: false,
            awaiting_correction: false,
            pending: None,
            model,
        })
    }
    pub(super) fn registration(&self, mut hooks: Value) -> Value {
        if !hooks.is_object() {
            hooks = json!({});
        }
        for (id, event) in &self.ids {
            // SDK timeout fails open. The owning host deadline expires first.
            hooks[*event] = json!([{"hookCallbackIds":[id],"timeout":3600}]);
        }
        hooks
    }
    pub(super) fn owns(&self, request: &Value) -> bool {
        request["callback_id"]
            .as_str()
            .is_some_and(|id| self.ids.contains_key(id))
    }
    pub(super) fn begin(
        &mut self,
        user: &Value,
        origin: SourceOrigin,
        events: &EventSink,
    ) -> Result<()> {
        ensure!(
            self.pending.is_none(),
            "previous source callback continuation is uncertain"
        );
        let backend = events
            .backend_invocation_id()
            .context("source lifecycle requires backend ownership")?;
        ensure!(
            self.backend != Some(backend),
            "source backend invocation repeated"
        );
        self.backend = Some(backend);
        self.prompt = source_prompt(&user["message"]["content"])?;
        self.command_uuid = bounded_id(&user["uuid"], "command")?.into();
        self.command_started = false;
        self.origin = origin;
        self.prompt_id = None;
        self.transcript = None;
        self.corrections = 0;
        self.assistant = false;
        self.assistant_text = None;
        self.response_open = false;
        self.text_block = None;
        self.accepted = false;
        self.awaiting_correction = false;
        Ok(())
    }
    pub(super) fn superseded(&mut self) {
        self.retired_command = Some((self.command_uuid.clone(), "cancelled"));
    }
    pub(super) fn permits_handoff_submit(&self, request: &Value) -> bool {
        self.owns(request)
            && request["input"]["hook_event_name"] == "UserPromptSubmit"
            && matches!(self.origin, SourceOrigin::PluginPostCorrection { .. })
    }
    fn backend(&self, events: &EventSink) -> Result<u64> {
        let backend = self.backend.context("source callback outside a turn")?;
        ensure!(
            events.backend_invocation_id() == Some(backend),
            "source callback belongs to another backend invocation"
        );
        Ok(backend)
    }
    pub(super) async fn handle(
        &mut self,
        message: &Value,
        session: Option<&str>,
        events: &EventSink,
        tools: &ToolExecutor,
    ) -> Result<Value> {
        let backend = self.backend(events)?;
        ensure!(
            self.command_started,
            "source callback precedes its exact command start"
        );
        ensure!(
            self.pending.is_none() && !self.accepted,
            "source callback precedes prior continuation or follows accepted Stop"
        );
        ensure!(
            serde_json::to_vec(message)?.len() <= 1024 * 1024,
            "source lifecycle callback exceeds bound"
        );
        let request = &message["request"];
        let id = bounded_id(&message["request_id"], "request")?;
        ensure!(
            !self.requests.contains(id),
            "source lifecycle request repeated; never replay a callback"
        );
        ensure!(
            self.requests.len() < 1024,
            "source lifecycle callback history is full"
        );
        let callback = bounded_id(&request["callback_id"], "callback")?;
        let event = *self
            .ids
            .get(callback)
            .context("unregistered source lifecycle callback")?;
        let envelope = bounded_id(&request["tool_use_id"], "callback envelope")?;
        ensure!(
            !self.envelopes.contains(envelope),
            "source lifecycle envelope repeated"
        );
        let input = &request["input"];
        ensure!(
            input["hook_event_name"] == event,
            "source lifecycle event differs from registration"
        );
        ensure!(
            session == Some(bounded_id(&input["session_id"], "session")?),
            "source lifecycle session differs"
        );
        ensure!(
            input["cwd"].as_str() == self.workspace.to_str(),
            "source lifecycle workspace differs"
        );
        let prompt_id = bounded_id(&input["prompt_id"], "prompt")?;
        let transcript = input["transcript_path"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 8192)
            .context("source transcript missing")?;
        let occurrence = if event == "UserPromptSubmit" {
            ensure!(
                self.prompt_id.is_none() && input["prompt"] == self.prompt,
                "source submission repeated or prompt differs"
            );
            NonToolOccurrence::UserPromptSubmit {
                prompt: self.prompt.clone(),
                correction: !matches!(self.origin, SourceOrigin::HostSubmission),
            }
        } else {
            ensure!(
                self.prompt_id.as_deref() == Some(prompt_id)
                    && self.transcript.as_deref() == Some(transcript),
                "source Stop prompt or transcript differs"
            );
            ensure!(
                self.assistant && !self.awaiting_correction,
                "source Stop lacks a new assistant response"
            );
            ensure!(
                input.get("last_assistant_message").and_then(Value::as_str)
                    == self.assistant_text.as_deref(),
                "source Stop assistant text differs from observed response"
            );
            let active = input["stop_hook_active"]
                .as_bool()
                .context("source Stop state missing")?;
            ensure!(
                active == (self.corrections > 0),
                "source Stop correction state differs"
            );
            NonToolOccurrence::Stop {
                stop_hook_active: active,
                last_assistant_message: input
                    .get("last_assistant_message")
                    .map(|v| {
                        v.as_str()
                            .map(str::to_owned)
                            .context("source assistant text invalid")
                    })
                    .transpose()?,
            }
        };
        self.profile
            .validate_claude_input(occurrence.event(), input)?;
        self.requests.insert(id.into());
        self.envelopes.insert(envelope.into());
        self.sequence += 1;
        self.prompt_id = Some(prompt_id.into());
        self.transcript = Some(transcript.into());
        let observed = events.for_observed_lifecycle(ObservedCallback {
            input: ObservedLifecycle::Claude(input.clone()),
            correlation: SourceCallback {
                origin: Some(self.origin.clone()),
                backend_operation: backend,
                sequence: self.sequence,
                request_id: id.into(),
                command_uuid: Some(self.command_uuid.clone()),
                command_request_id: None,
                envelope_id: Some(envelope.into()),
                model: self.model.clone(),
            },
        })?;
        let outcome = tokio::time::timeout(
            Duration::from_secs(30),
            tools.dispatch_non_tool(occurrence, &observed),
        )
        .await
        .context("source lifecycle handler timed out; continuation remains held")??;
        let mut response = json!({});
        if let Some(outcome) = outcome {
            self.pending = Some(outcome.operation);
            if let Some(reason) = outcome.hold {
                return Ok(
                    json!({"decision":"block","reason":reason,"continue":false,"stopReason":"Host lifecycle gate remains unmet"}),
                );
            }
            if outcome.correction {
                let (runtime, operation) = events.for_non_tool_context(outcome.operation)?;
                runtime.admit_non_tool_correction(operation)?;
                self.corrections += 1;
                self.awaiting_correction = true;
                self.assistant = false;
                response = json!({"decision":"block","reason":outcome.context});
            } else if event == "UserPromptSubmit" && !outcome.context.is_empty() {
                response = json!({"hookSpecificOutput":{"hookEventName":event,"additionalContext":outcome.context}});
            }
        }
        if event == "Stop" && !self.awaiting_correction {
            self.accepted = true;
        }
        Ok(response)
    }
    pub(super) fn prepare(&self, events: &EventSink) -> Result<()> {
        if let Some(operation) = self.pending {
            let (runtime, _) = events.for_non_tool_context(operation)?;
            runtime.prepare_source_continuation(operation, self.backend(events)?)
        } else {
            events.ensure_continuation()
        }
    }
    pub(super) fn sent(&self, events: &EventSink) -> Result<()> {
        if let Some(operation) = self.pending {
            let (runtime, _) = events.for_non_tool_context(operation)?;
            runtime.source_lifecycle_delivery(operation, self.backend(events)?, true)?;
            return runtime.ensure_source_continuation(operation, self.backend(events)?);
        }
        events.ensure_continuation()
    }
    pub(super) fn observe(&mut self, message: &Value, events: &EventSink) -> Result<()> {
        if message["type"] == "command_lifecycle" {
            self.backend(events)?;
            if self.retired_command.as_ref().is_some_and(|(uuid, state)| {
                message["command_uuid"] == *uuid && message["state"] == *state
            }) {
                self.retired_command = None;
                return Ok(());
            }
            ensure!(
                message["command_uuid"] == self.command_uuid,
                "source lifecycle command identity differs"
            );
            if message["state"] == "started" {
                ensure!(
                    !self.command_started,
                    "source lifecycle command start repeated"
                );
                self.command_started = true;
            }
        }
        if message["type"] == "system"
            && message["subtype"] == "init"
            && let Some(model) = message["model"].as_str().filter(|s| !s.is_empty())
        {
            self.model = Some(model.into());
        }
        let root_stream =
            message["parent_tool_use_id"].is_null() && message["type"] == "stream_event";
        let assistant = root_stream && message["event"]["type"] == "message_start";
        let terminal = message["type"] == "result";
        if root_stream && !assistant {
            self.observe_stream(&message["event"])?;
            return Ok(());
        }
        if !assistant && !terminal {
            return Ok(());
        }
        self.backend(events)?;
        ensure!(
            self.prompt_id.is_some(),
            "source continued without its submission callback"
        );
        if terminal {
            ensure!(
                self.accepted
                    && !self.awaiting_correction
                    && message["user_message_uuid"] == self.command_uuid,
                "source completed without admitted Stop or exact command identity"
            );
        }
        if assistant {
            // The source can omit command UUIDs on its internal Stop retry.
            // Only our admitted, still-pending correction owns that response.
            let stop_retry = self.awaiting_correction
                && self.pending.is_some()
                && message.get("user_message_uuid").is_none()
                && message.get("user_message_uuids").is_none();
            ensure!(
                message["user_message_uuid"] == self.command_uuid || stop_retry,
                "source response command identity differs"
            );
            ensure!(
                !self.accepted && !self.response_open,
                "source continued after admitted Stop or before response end"
            );
            self.assistant = false;
            self.assistant_text = None;
            self.response_open = true;
            self.awaiting_correction = false;
        }
        if terminal {
            self.retired_command = Some((self.command_uuid.clone(), "completed"));
        }
        if let Some(operation) = self.pending.take() {
            let (runtime, _) = events.for_non_tool_context(operation)?;
            runtime.source_lifecycle_delivery(operation, self.backend(events)?, false)?;
        }
        Ok(())
    }
    fn observe_stream(&mut self, event: &Value) -> Result<()> {
        ensure!(
            self.response_open,
            "source stream lacks correlated response start"
        );
        match event["type"].as_str() {
            Some("content_block_start") => {
                ensure!(self.text_block.is_none(), "source text blocks overlap");
                if event["content_block"]["type"] == "text" {
                    let index = event["index"]
                        .as_u64()
                        .context("source text index missing")?;
                    let text = event["content_block"]["text"]
                        .as_str()
                        .context("source text missing")?;
                    ensure!(text.len() <= 65536, "source assistant text exceeds bound");
                    self.text_block = Some((index, text.into()));
                }
            }
            Some("content_block_delta") if event["delta"]["type"] == "text_delta" => {
                let (index, text) = self
                    .text_block
                    .as_mut()
                    .context("source text delta lacks block")?;
                ensure!(
                    event["index"].as_u64() == Some(*index),
                    "source text delta index differs"
                );
                let delta = event["delta"]["text"]
                    .as_str()
                    .context("source text delta missing")?;
                ensure!(
                    text.len() + delta.len() <= 65536,
                    "source assistant text exceeds bound"
                );
                text.push_str(delta);
            }
            Some("content_block_stop") => {
                if let Some((index, text)) = self.text_block.take() {
                    ensure!(
                        event["index"].as_u64() == Some(index),
                        "source text stop index differs"
                    );
                    self.assistant_text = source_text(&text);
                }
            }
            Some("message_stop") => {
                ensure!(
                    self.text_block.is_none(),
                    "source response ended with unfinished text"
                );
                self.response_open = false;
                self.assistant = true;
            }
            _ => {}
        }
        Ok(())
    }
}
fn source_prompt(content: &Value) -> Result<String> {
    if let Some(text) = content.as_str() {
        return Ok(text.to_owned());
    }
    let blocks = content
        .as_array()
        .context("source submission content is neither text nor provider blocks")?;
    let text = blocks
        .iter()
        .filter(|block| block["type"] == "text")
        .map(|block| {
            block["text"]
                .as_str()
                .context("source submission text block is invalid")
        })
        .collect::<Result<Vec<_>>>()?
        .join("\n");
    Ok(source_text(&text).unwrap_or_default())
}
fn source_text(text: &str) -> Option<String> {
    // ECMAScript String.trim, as used by the qualified source callback.
    let text = text.trim_matches(|c: char| matches!(c, '\u{0009}'..='\u{000d}' | '\u{0020}' | '\u{00a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}'));
    (!text.is_empty()).then(|| text.to_owned())
}
fn bounded_id<'a>(value: &'a Value, name: &str) -> Result<&'a str> {
    value
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 256)
        .with_context(|| format!("invalid source lifecycle {name} identity"))
}

#[cfg(test)]
#[path = "claude_non_tool_tests.rs"]
mod tests;
