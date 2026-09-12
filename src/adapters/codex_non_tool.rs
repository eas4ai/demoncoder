//! Ordinary managed source occurrences. Native run IDs can repeat; the ordered
//! run and its private delivery UUID are distinct from the durable host owner.
use crate::{
    events::EventSink,
    plugins::{profile::CompatibilityProfile, receipts::*},
    tools::ToolExecutor,
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{collections::BTreeSet, path::PathBuf, time::Duration};

pub(super) struct Callbacks {
    workspace: PathBuf,
    source: PathBuf,
    session: String,
    transcript: String,
    model: Option<String>,
    backend: Option<u64>,
    request: u64,
    turn: Option<String>,
    prompt: String,
    origin: SourceOrigin,
    sequence: u64,
    deliveries: BTreeSet<String>,
    run: Option<Value>,
    pending: Option<u64>,
    sent: bool,
    submitted: bool,
    accepted: bool,
    correction: bool,
    corrections: u32,
    assistant: Option<(String, String)>,
    assistant_complete: bool,
    profile: CompatibilityProfile,
}
impl Callbacks {
    pub(super) fn new(
        workspace: PathBuf,
        source: PathBuf,
        session: String,
        transcript: String,
        model: Option<String>,
    ) -> Result<Self> {
        Ok(Self {
            workspace,
            source,
            session,
            transcript,
            model,
            backend: None,
            request: 0,
            turn: None,
            prompt: String::new(),
            origin: SourceOrigin::HostSubmission,
            sequence: 0,
            deliveries: BTreeSet::new(),
            run: None,
            pending: None,
            sent: false,
            submitted: false,
            accepted: false,
            correction: false,
            corrections: 0,
            assistant: None,
            assistant_complete: false,
            profile: CompatibilityProfile::embedded()?,
        })
    }
    pub(super) fn begin(
        &mut self,
        request: &Value,
        origin: SourceOrigin,
        events: &EventSink,
    ) -> Result<()> {
        ensure!(
            self.run.is_none() && self.pending.is_none(),
            "previous ordinary delivery is uncertain"
        );
        let backend = events
            .backend_invocation_id()
            .context("ordinary callback lacks backend owner")?;
        ensure!(
            self.backend != Some(backend),
            "ordinary backend owner repeated"
        );
        ensure!(
            request["method"] == "turn/start" && request["params"]["threadId"] == self.session,
            "ordinary submission frame differs"
        );
        self.request = request["id"]
            .as_u64()
            .context("ordinary request identity missing")?;
        let input = request["params"]["input"]
            .as_array()
            .context("ordinary submission input missing")?;
        ensure!(
            input.len() == 1 && input[0]["type"] == "text",
            "ordinary submission is not the admitted text frame"
        );
        self.prompt = input[0]["text"]
            .as_str()
            .context("ordinary prompt missing")?
            .into();
        self.backend = Some(backend);
        self.origin = origin;
        self.turn = None;
        self.submitted = false;
        self.accepted = false;
        self.correction = false;
        self.corrections = 0;
        self.assistant = None;
        self.assistant_complete = false;
        self.sent = false;
        Ok(())
    }
    fn backend(&self, events: &EventSink) -> Result<u64> {
        let backend = self
            .backend
            .context("ordinary callback outside invocation")?;
        ensure!(
            events.backend_invocation_id() == Some(backend),
            "ordinary backend owner differs"
        );
        Ok(backend)
    }
    pub(super) fn ready(&self) -> bool {
        self.run.is_some() && !self.sent
    }
    pub(super) fn observe(&mut self, message: &Value, events: &EventSink) -> Result<()> {
        self.backend(events)?;
        let params = &message["params"];
        let method = message["method"].as_str().unwrap_or_default();
        if method == "turn/started"
            || (message.get("method").is_none()
                && message["id"] == self.request
                && message["result"]["turn"]["id"].is_string())
        {
            let turn = if method == "turn/started" {
                ensure!(
                    params["threadId"] == self.session,
                    "ordinary start thread differs"
                );
                id(&params["turn"]["id"])?
            } else {
                id(&message["result"]["turn"]["id"])?
            };
            ensure!(
                self.turn.as_deref().is_none_or(|v| v == turn),
                "ordinary turn identity differs"
            );
            self.turn = Some(turn.into());
        }
        if !matches!(
            method,
            "hook/started"
                | "hook/completed"
                | "item/started"
                | "item/completed"
                | "turn/completed"
                | "item/agentMessage/delta"
                | "item/tool/call"
        ) {
            return Ok(());
        }
        ensure!(
            params["threadId"] == self.session,
            "ordinary event thread differs"
        );
        let turn = if method == "turn/completed" {
            &params["turn"]["id"]
        } else {
            &params["turnId"]
        };
        ensure!(
            turn.as_str().is_some() && turn.as_str() == self.turn.as_deref(),
            "ordinary event turn differs"
        );
        if method.starts_with("hook/") {
            let run = &params["run"];
            if !matches!(run["eventName"].as_str(), Some("userPromptSubmit" | "stop")) {
                return Ok(());
            }
            ensure!(
                run["sourcePath"].as_str() == self.source.to_str()
                    && run["source"] == "sessionFlags"
                    && run["handlerType"] == "command"
                    && run["executionMode"] == "sync"
                    && run["scope"] == "turn",
                "ordinary native hook registration differs"
            );
            id(&run["id"])?;
            let submit = run["eventName"] == "userPromptSubmit";
            let order = if submit { 0 } else { 1 };
            let prefix = if submit { "user-prompt-submit" } else { "stop" };
            ensure!(
                run["displayOrder"] == order
                    && run["id"] == format!("{prefix}:{order}:{}", self.source.display()),
                "ordinary native run identity differs from private registration"
            );
            if method == "hook/started" {
                ensure!(
                    self.run.is_none()
                        && self.pending.is_none()
                        && !self.accepted
                        && run["status"] == "running",
                    "ordinary hook overlaps or follows accepted Stop"
                );
                ensure!(
                    if submit {
                        !self.submitted
                    } else {
                        self.submitted && (self.assistant_complete || self.assistant.is_none())
                    },
                    "ordinary hook is outside its source boundary"
                );
                self.run = Some(run.clone());
                self.sent = false;
            } else {
                let active = self
                    .run
                    .as_ref()
                    .context("ordinary completion lacks active run")?;
                ensure!(
                    self.sent
                        && run["id"] == active["id"]
                        && run["eventName"] == active["eventName"]
                        && run["displayOrder"] == active["displayOrder"],
                    "ordinary completion differs from sent occurrence"
                );
                ensure!(
                    run["status"]
                        == if self.correction {
                            "blocked"
                        } else {
                            "completed"
                        },
                    "ordinary source did not accept the exact delivery"
                );
                if let Some(operation) = self.pending.take() {
                    let (runtime, _) = events.for_non_tool_context(operation)?;
                    runtime.source_lifecycle_delivery(operation, self.backend(events)?, false)?;
                }
                self.run = None;
                if submit {
                    self.submitted = true;
                } else if self.correction {
                    self.assistant = None;
                    self.assistant_complete = false;
                    self.correction = false;
                } else {
                    self.accepted = true;
                }
            }
            return Ok(());
        }
        if params["item"]["type"] == "agentMessage" {
            ensure!(
                self.submitted && self.run.is_none() && !self.accepted,
                "ordinary assistant crossed an unacknowledged boundary"
            );
            let item = &params["item"];
            let item_id = id(&item["id"])?;
            let text = item["text"]
                .as_str()
                .context("ordinary assistant text missing")?;
            ensure!(text.len() <= 65536, "ordinary assistant text exceeds bound");
            if method == "item/started" {
                ensure!(
                    self.assistant.is_none() || self.assistant_complete,
                    "ordinary assistant overlaps"
                );
                self.assistant = Some((item_id.into(), text.into()));
                self.assistant_complete = false;
            } else {
                ensure!(
                    self.assistant
                        .as_ref()
                        .is_some_and(|(current, _)| current == item_id)
                        && !self.assistant_complete,
                    "ordinary assistant completion differs"
                );
                self.assistant = Some((item_id.into(), text.into()));
                self.assistant_complete = true;
            }
        }
        if method == "item/tool/call" {
            ensure!(
                self.submitted && self.run.is_none() && !self.accepted,
                "ordinary tool crossed an unacknowledged boundary"
            );
        }
        if method == "turn/completed" && params["turn"]["status"] == "completed" {
            ensure!(
                self.accepted && self.run.is_none() && self.pending.is_none(),
                "ordinary turn completed without admitted Stop"
            );
        }
        Ok(())
    }
    pub(super) async fn handle(
        &mut self,
        mut input: Value,
        events: &EventSink,
        tools: &ToolExecutor,
    ) -> Result<Value> {
        let backend = self.backend(events)?;
        let run = self
            .run
            .as_ref()
            .context("ordinary callback lacks native run")?;
        ensure!(
            !self.sent && self.pending.is_none() && !self.accepted,
            "ordinary callback repeated before continuation"
        );
        ensure!(
            serde_json::to_vec(&input)?.len() <= 1024 * 1024,
            "ordinary callback input exceeds bound"
        );
        let event = if run["eventName"] == "userPromptSubmit" {
            "UserPromptSubmit"
        } else {
            "Stop"
        };
        let envelope = input
            .as_object_mut()
            .context("ordinary callback is not an object")?
            .remove("demonCoderOrdinary")
            .context("ordinary envelope missing")?;
        let delivery = id(&envelope["delivery_id"])?;
        ensure!(
            delivery.len() == 36
                && delivery
                    .bytes()
                    .enumerate()
                    .all(|(i, c)| if matches!(i, 8 | 13 | 18 | 23) {
                        c == b'-'
                    } else {
                        c.is_ascii_hexdigit() && !c.is_ascii_uppercase()
                    }),
            "ordinary delivery UUID invalid"
        );
        ensure!(
            envelope
                == json!({"protocol":"demoncoder-ordinary-v1","delivery_id":delivery,"hook_event_name":event,"session_id":self.session,"turn_id":self.turn}),
            "ordinary envelope identity differs"
        );
        ensure!(
            self.deliveries.len() < 1024 && !self.deliveries.contains(delivery),
            "ordinary delivery repeated or history full"
        );
        ensure!(
            input["hook_event_name"] == event
                && input["session_id"] == self.session
                && input["turn_id"].as_str() == self.turn.as_deref()
                && input["cwd"].as_str() == self.workspace.to_str()
                && input["transcript_path"] == self.transcript,
            "ordinary callback source owner differs"
        );
        let occurrence = if event == "UserPromptSubmit" {
            ensure!(
                !self.submitted && input["prompt"] == self.prompt,
                "ordinary submission prompt differs"
            );
            NonToolOccurrence::UserPromptSubmit {
                prompt: self.prompt.clone(),
                correction: !matches!(self.origin, SourceOrigin::HostSubmission),
            }
        } else {
            ensure!(
                self.submitted
                    && (self.assistant_complete || self.assistant.is_none())
                    && input["last_assistant_message"].as_str()
                        == self.assistant.as_ref().map(|(_, text)| text.as_str()),
                "ordinary Stop response differs"
            );
            let active = input["stop_hook_active"]
                .as_bool()
                .context("ordinary Stop state missing")?;
            ensure!(
                active == (self.corrections > 0),
                "ordinary Stop correction state differs"
            );
            NonToolOccurrence::Stop {
                stop_hook_active: active,
                last_assistant_message: input["last_assistant_message"].as_str().map(str::to_owned),
            }
        };
        self.profile.validate_schema(
            &crate::plugins::profile::SchemaKey::Codex {
                path: format!(
                    "codex-rs/hooks/schema/generated/{}.command.input.schema.json",
                    if event == "Stop" {
                        "stop"
                    } else {
                        "user-prompt-submit"
                    }
                ),
                definition: None,
            },
            &input,
        )?;
        self.sequence += 1;
        self.deliveries.insert(delivery.into());
        let observed = events.for_observed_lifecycle(ObservedCallback {
            input: ObservedLifecycle::Codex(input),
            correlation: SourceCallback {
                origin: Some(self.origin.clone()),
                backend_operation: backend,
                sequence: self.sequence,
                request_id: id(&run["id"])?.into(),
                command_uuid: None,
                command_request_id: Some(self.request),
                envelope_id: Some(delivery.into()),
                model: self.model.clone(),
            },
        })?;
        let outcome = tokio::time::timeout(
            Duration::from_secs(30),
            tools.dispatch_non_tool(occurrence, &observed),
        )
        .await
        .context("ordinary callback timed out; continuation remains held")??;
        let mut response = json!({"continue":true,"demonCoderOrdinary":envelope});
        let mut correction = None;
        if let Some(outcome) = outcome {
            self.pending = Some(outcome.operation);
            if let Some(reason) = outcome.hold {
                response["continue"] = false.into();
                response["stopReason"] = reason.into();
            } else if outcome.correction {
                response["decision"] = "block".into();
                response["reason"] = outcome.context.into();
                correction = Some(outcome.operation);
            } else if event == "UserPromptSubmit" && !outcome.context.is_empty() {
                response["hookSpecificOutput"] =
                    json!({"hookEventName":event,"additionalContext":outcome.context});
            }
        }
        response_bytes(&response)?;
        if let Some(operation) = correction {
            let (runtime, _) = events.for_non_tool_context(operation)?;
            runtime.admit_non_tool_correction(operation)?;
            self.corrections += 1;
            self.correction = true;
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
    pub(super) fn sent(&mut self, events: &EventSink) -> Result<()> {
        ensure!(
            self.run.is_some() && !self.sent,
            "ordinary delivery repeated"
        );
        self.sent = true;
        if let Some(operation) = self.pending {
            let (runtime, _) = events.for_non_tool_context(operation)?;
            runtime.source_lifecycle_delivery(operation, self.backend(events)?, true)?;
            runtime.ensure_source_continuation(operation, self.backend(events)?)
        } else {
            events.ensure_continuation()
        }
    }
}
pub(super) fn response_bytes(response: &Value) -> Result<Vec<u8>> {
    let bytes = serde_json::to_vec(response)?;
    ensure!(
        bytes.len() < 65536,
        "ordinary serialized response exceeds source output bound"
    );
    Ok(bytes)
}
fn id(value: &Value) -> Result<&str> {
    value
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 8192)
        .context("ordinary source identity missing or oversized")
}

#[cfg(test)]
#[path = "codex_non_tool_tests.rs"]
mod tests;
