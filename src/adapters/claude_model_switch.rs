//! Genuine Claude SDK model transition callbacks owned by one host transaction.
use crate::{
    events::EventSink,
    plugins::{
        hook_types::HookEvent,
        non_tool::NonToolValidation,
        profile::CompatibilityProfile,
        receipts::{NonToolOccurrence, ObservedCallback, ObservedLifecycle, SourceCallback},
    },
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

const MAX_RETAINED_CALLBACK_IDS: usize = 4096;

pub(super) struct Callbacks {
    ids: BTreeMap<String, HookEvent>,
    profile: CompatibilityProfile,
    workspace: PathBuf,
    active: Option<Active>,
    seen_requests: BTreeSet<String>,
    seen_envelopes: BTreeSet<String>,
}

struct Active {
    switch: u64,
    requested: Option<String>,
    actual_from: String,
    candidate: Option<String>,
    sequence: u64,
    pre_seen: bool,
    post_seen: bool,
    control_succeeded: bool,
}

pub(super) struct Reply {
    pub(super) event: HookEvent,
    pub(super) operation: u64,
    pub(super) response: Value,
    pub(super) hold: Option<String>,
    pub(super) candidate: String,
    pub(super) validation: Option<NonToolValidation>,
}

impl Reply {
    pub(super) fn deny(&mut self, reason: String) {
        self.hold = Some(reason);
        self.response = json!({"hookSpecificOutput":{
            "hookEventName":"PreModelSwitch",
            "permissionDecision":"deny",
            "permissionDecisionReason":"Host model switch final validation failed; the transition was refused"
        }});
    }
}

impl Callbacks {
    pub(super) fn new(workspace: PathBuf) -> Result<Self> {
        let mut random = [0u8; 32];
        std::fs::File::open("/dev/urandom")?.read_exact(&mut random)?;
        let nonce = random
            .iter()
            .map(|value| format!("{value:02x}"))
            .collect::<String>();
        Ok(Self {
            ids: [HookEvent::PreModelSwitch, HookEvent::PostModelSwitch]
                .into_iter()
                .map(|event| (format!("{nonce}:{}", event.as_str()), event))
                .collect(),
            profile: CompatibilityProfile::embedded()?,
            workspace,
            active: None,
            seen_requests: BTreeSet::new(),
            seen_envelopes: BTreeSet::new(),
        })
    }

    fn claim_callback_ids(&mut self, request: &str, envelope: &str) -> Result<()> {
        ensure!(
            !self.seen_requests.contains(request) && !self.seen_envelopes.contains(envelope),
            "Claude model switch callback identity was replayed"
        );
        ensure!(
            self.seen_requests.len() < MAX_RETAINED_CALLBACK_IDS
                && self.seen_envelopes.len() < MAX_RETAINED_CALLBACK_IDS,
            "Claude model switch callback replay protection is full"
        );
        self.seen_requests.insert(request.into());
        self.seen_envelopes.insert(envelope.into());
        Ok(())
    }

    fn validate_before_claim(&self, message: &Value, session: Option<&str>) -> Result<()> {
        let request = &message["request"];
        let callback = bounded(&request["callback_id"], "callback")?;
        let event = *self
            .ids
            .get(callback)
            .context("unregistered Claude model switch callback")?;
        let input = &request["input"];
        ensure!(
            input["hook_event_name"] == event.as_str()
                && input["source"] == "sdk"
                && input["cwd"].as_str() == self.workspace.to_str()
                && session == Some(bounded(&input["session_id"], "session")?),
            "Claude model switch callback source or session differs"
        );
        self.profile.validate_claude_input(event, input)?;
        bounded(&message["request_id"], "request")?;
        bounded(&request["tool_use_id"], "envelope")?;
        let active = self
            .active
            .as_ref()
            .context("unexpected Claude model switch callback")?;
        let requested = active
            .requested
            .as_ref()
            .map_or(Value::Null, |model| Value::String(model.clone()));
        let candidate = bounded(&input["to_model"], "resolved model")?;
        ensure!(
            input["requested_model"] == requested
                && input["from_model"] == active.actual_from
                && active
                    .candidate
                    .as_deref()
                    .is_none_or(|model| model == candidate),
            "Claude requested or resolved model differs from the admitted candidate"
        );
        match event {
            HookEvent::PreModelSwitch => ensure!(
                !active.pre_seen && !active.control_succeeded && !active.post_seen,
                "Claude PreModelSwitch repeated or arrived out of order"
            ),
            HookEvent::PostModelSwitch => ensure!(
                active.pre_seen && active.control_succeeded && !active.post_seen,
                "Claude PostModelSwitch preceded set_model success or repeated"
            ),
            _ => unreachable!(),
        }
        Ok(())
    }

    pub(super) fn registration(&self, mut hooks: Value) -> Value {
        if !hooks.is_object() {
            hooks = json!({});
        }
        for (id, event) in &self.ids {
            hooks[event.as_str()] = json!([{"hookCallbackIds":[id],"timeout":3600}]);
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
        requested: Option<String>,
        actual_from: String,
        events: &EventSink,
    ) -> Result<()> {
        ensure!(self.active.is_none(), "Claude model switch already pending");
        let (_, switch) = events.model_switch_context()?;
        ensure!(
            !actual_from.is_empty(),
            "Claude current resolved model missing"
        );
        self.active = Some(Active {
            switch,
            requested,
            actual_from,
            candidate: None,
            sequence: 0,
            pre_seen: false,
            post_seen: false,
            control_succeeded: false,
        });
        Ok(())
    }

    pub(super) fn control_succeeded(&mut self) -> Result<String> {
        let active = self
            .active
            .as_mut()
            .context("Claude model switch owner missing")?;
        ensure!(
            active.pre_seen && !active.post_seen && !active.control_succeeded,
            "Claude set_model success lacks its exact PreModelSwitch"
        );
        active.control_succeeded = true;
        active
            .candidate
            .clone()
            .context("Claude resolved model candidate missing")
    }

    pub(super) fn refused(&mut self) -> Result<()> {
        let active = self
            .active
            .take()
            .context("Claude model switch owner missing")?;
        ensure!(
            active.pre_seen && !active.control_succeeded && !active.post_seen,
            "Claude refusal sequence differs"
        );
        Ok(())
    }

    pub(super) fn finish(&mut self) -> Result<()> {
        let active = self
            .active
            .take()
            .context("Claude model switch owner missing")?;
        ensure!(
            active.pre_seen && active.control_succeeded && active.post_seen,
            "Claude model switch completed without exact Pre/Post callbacks"
        );
        Ok(())
    }

    pub(super) fn reset(&mut self) {
        self.active = None;
    }

    pub(super) fn explicit_pre_denial(&self, message: &Value) -> Result<Value> {
        let request = &message["request"];
        ensure!(
            request["subtype"] == "hook_callback"
                && self.owns(request)
                && self.ids.get(bounded(&request["callback_id"], "callback")?)
                    == Some(&HookEvent::PreModelSwitch),
            "cannot correlate a Claude PreModelSwitch denial"
        );
        let request_id = bounded(&message["request_id"], "request")?;
        Ok(json!({
            "type":"control_response",
            "response":{
                "subtype":"success",
                "request_id":request_id,
                "response":{
                    "hookSpecificOutput":{
                        "hookEventName":"PreModelSwitch",
                        "permissionDecision":"deny",
                        "permissionDecisionReason":"Host model switch callback failed; the transition was refused"
                    }
                }
            }
        }))
    }

    pub(super) async fn handle(
        &mut self,
        message: &Value,
        session: Option<&str>,
        events: &EventSink,
        tools: &ToolExecutor,
    ) -> Result<Reply> {
        ensure!(
            serde_json::to_vec(message)?.len() <= 1024 * 1024,
            "Claude model switch callback exceeds bound"
        );
        self.validate_before_claim(message, session)?;
        let request = &message["request"];
        let callback = bounded(&request["callback_id"], "callback")?;
        let event = *self
            .ids
            .get(callback)
            .context("unregistered Claude model switch callback")?;
        let input = &request["input"];
        ensure!(
            input["hook_event_name"] == event.as_str()
                && input["source"] == "sdk"
                && input["cwd"].as_str() == self.workspace.to_str()
                && session == Some(bounded(&input["session_id"], "session")?),
            "Claude model switch callback source or session differs"
        );
        self.profile.validate_claude_input(event, input)?;
        let id = bounded(&message["request_id"], "request")?;
        let envelope = bounded(&request["tool_use_id"], "envelope")?;
        let active = self
            .active
            .as_ref()
            .context("unexpected Claude model switch callback")?;
        let requested = active
            .requested
            .as_ref()
            .map_or(Value::Null, |model| Value::String(model.clone()));
        let candidate = bounded(&input["to_model"], "resolved model")?.to_owned();
        ensure!(
            input["requested_model"] == requested
                && input["from_model"] == active.actual_from
                && active
                    .candidate
                    .as_ref()
                    .is_none_or(|model| model == &candidate),
            "Claude requested or resolved model differs from the admitted candidate"
        );
        match event {
            HookEvent::PreModelSwitch => ensure!(
                !active.pre_seen && !active.control_succeeded && !active.post_seen,
                "Claude PreModelSwitch repeated or arrived out of order"
            ),
            HookEvent::PostModelSwitch => ensure!(
                active.pre_seen && active.control_succeeded && !active.post_seen,
                "Claude PostModelSwitch preceded set_model success or repeated"
            ),
            _ => unreachable!(),
        }
        let switch = active.switch;
        let actual_from = active.actual_from.clone();
        self.claim_callback_ids(id, envelope)?;
        if event == HookEvent::PreModelSwitch {
            let (runtime, event_switch) = events.model_switch_context()?;
            ensure!(event_switch == switch, "Claude model switch owner changed");
            runtime.resolve_model_switch_candidate(switch, &actual_from, &candidate)?;
        }
        let active = self.active.as_mut().expect("validated above");
        active.sequence += 1;
        active.candidate = Some(candidate.clone());
        active.pre_seen |= event == HookEvent::PreModelSwitch;
        active.post_seen |= event == HookEvent::PostModelSwitch;
        let observed = events.for_observed_lifecycle(ObservedCallback {
            input: ObservedLifecycle::Claude(input.clone()),
            correlation: SourceCallback {
                origin: None,
                backend_operation: active.switch,
                sequence: active.sequence,
                request_id: id.into(),
                command_uuid: None,
                command_request_id: None,
                envelope_id: Some(envelope.into()),
                model: Some(candidate.clone()),
            },
        })?;
        let occurrence = match event {
            HookEvent::PreModelSwitch => NonToolOccurrence::PreModelSwitch {
                model_switch: active.switch,
                requested_model: active.requested.clone(),
                resolved_model: Some(candidate.clone()),
                source: "settings".into(),
            },
            HookEvent::PostModelSwitch => NonToolOccurrence::PostModelSwitch {
                model_switch: active.switch,
                model: Some(candidate.clone()),
                source: "settings".into(),
            },
            _ => unreachable!(),
        };
        let outcome = tokio::time::timeout(
            Duration::from_secs(30),
            tools.dispatch_non_tool(occurrence, &observed),
        )
        .await
        .context("Claude model switch handler timed out")??
        .context("authenticated model switch observation missing")?;
        let hold = outcome.hold;
        let context = outcome.context;
        let response = if event == HookEvent::PreModelSwitch {
            let decision = if hold.is_some() { "deny" } else { "allow" };
            json!({"hookSpecificOutput":{
                "hookEventName":"PreModelSwitch",
                "permissionDecision":decision,
                "permissionDecisionReason":hold.as_deref().unwrap_or("Host model switch policy allowed the candidate")
            }})
        } else {
            let mut specific = json!({"hookEventName":"PostModelSwitch"});
            if !context.is_empty() {
                specific["additionalContext"] = json!(context);
            }
            json!({"hookSpecificOutput":specific})
        };
        Ok(Reply {
            event,
            operation: outcome.operation,
            response,
            hold,
            candidate,
            validation: outcome.validation,
        })
    }
}

fn bounded<'a>(value: &'a Value, name: &str) -> Result<&'a str> {
    value
        .as_str()
        .filter(|value| !value.is_empty() && value.len() <= 256)
        .with_context(|| format!("invalid Claude model switch {name}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callback_replay_ids_survive_switch_reset_and_new_owner_is_fresh() {
        let root = tempfile::tempdir().unwrap();
        let mut callbacks = Callbacks::new(root.path().into()).unwrap();
        callbacks
            .claim_callback_ids("old-request", "old-envelope")
            .unwrap();
        callbacks.reset();
        assert!(
            callbacks
                .claim_callback_ids("old-request", "new-envelope")
                .is_err()
        );
        assert!(
            callbacks
                .claim_callback_ids("new-request", "old-envelope")
                .is_err()
        );
        let mut replacement = Callbacks::new(root.path().into()).unwrap();
        replacement
            .claim_callback_ids("old-request", "old-envelope")
            .unwrap();
    }

    #[test]
    fn callback_replay_protection_never_evicts_on_overflow() {
        let root = tempfile::tempdir().unwrap();
        let mut callbacks = Callbacks::new(root.path().into()).unwrap();
        for index in 0..MAX_RETAINED_CALLBACK_IDS {
            callbacks
                .claim_callback_ids(&format!("request-{index}"), &format!("envelope-{index}"))
                .unwrap();
        }
        assert!(
            callbacks
                .claim_callback_ids("overflow", "overflow")
                .is_err()
        );
        assert!(callbacks.claim_callback_ids("request-0", "fresh").is_err());
    }

    fn callback_fixture(callbacks: &Callbacks, event: HookEvent) -> Value {
        let callback = callbacks
            .ids
            .iter()
            .find_map(|(id, candidate)| (*candidate == event).then(|| id.clone()))
            .unwrap();
        json!({
            "request_id":"request-id",
            "request":{
                "subtype":"hook_callback",
                "callback_id":callback,
                "tool_use_id":"envelope-id",
                "input":{
                    "session_id":"source-session",
                    "transcript_path":"/source/transcript.jsonl",
                    "cwd":callbacks.workspace,
                    "hook_event_name":event.as_str(),
                    "from_model":"model-a",
                    "to_model":"model-b",
                    "requested_model":"alias-b",
                    "source":"sdk",
                    "context_tokens":12,
                    "prompt_cache_warm":true,
                    "cache_ttl":"1h",
                    "estimated_cache_write_usd":0.001,
                    "pricing":"catalog"
                }
            }
        })
    }

    fn active(callbacks: &mut Callbacks) {
        callbacks.active = Some(Active {
            switch: 7,
            requested: Some("alias-b".into()),
            actual_from: "model-a".into(),
            candidate: None,
            sequence: 0,
            pre_seen: false,
            post_seen: false,
            control_succeeded: false,
        });
    }

    #[test]
    fn forged_session_model_registration_and_order_do_not_mutate_active_callback() {
        let root = tempfile::tempdir().unwrap();
        let mut callbacks = Callbacks::new(root.path().into()).unwrap();
        active(&mut callbacks);
        let valid = callback_fixture(&callbacks, HookEvent::PreModelSwitch);
        assert!(
            callbacks
                .validate_before_claim(&valid, Some("source-session"))
                .is_ok()
        );

        let mut wrong_session = valid.clone();
        wrong_session["request"]["input"]["session_id"] = json!("other-session");
        assert!(
            callbacks
                .validate_before_claim(&wrong_session, Some("source-session"))
                .is_err()
        );
        let mut wrong_model = valid.clone();
        wrong_model["request"]["input"]["to_model"] = json!("model-c");
        callbacks.active.as_mut().unwrap().candidate = Some("model-b".into());
        assert!(
            callbacks
                .validate_before_claim(&wrong_model, Some("source-session"))
                .is_err()
        );
        callbacks.active.as_mut().unwrap().candidate = None;
        let mut forged = valid.clone();
        forged["request"]["callback_id"] = json!("forged-registration");
        assert!(
            callbacks
                .validate_before_claim(&forged, Some("source-session"))
                .is_err()
        );
        let post = callback_fixture(&callbacks, HookEvent::PostModelSwitch);
        assert!(
            callbacks
                .validate_before_claim(&post, Some("source-session"))
                .is_err()
        );

        let active = callbacks.active.as_ref().unwrap();
        assert_eq!(active.sequence, 0);
        assert!(!active.pre_seen && !active.post_seen && !active.control_succeeded);
        assert!(callbacks.seen_requests.is_empty() && callbacks.seen_envelopes.is_empty());
    }

    #[tokio::test]
    async fn handle_rejects_old_pre_and_post_before_current_occurrence_dispatch() {
        let root = tempfile::tempdir().unwrap();
        let tools = ToolExecutor::new(root.path()).unwrap();
        let (sender, mut receiver) = tokio::sync::mpsc::channel(4);
        let events = EventSink::new("host-session".into(), sender, None).unwrap();

        for event in [HookEvent::PreModelSwitch, HookEvent::PostModelSwitch] {
            let mut callbacks = Callbacks::new(root.path().into()).unwrap();
            active(&mut callbacks);
            if event == HookEvent::PostModelSwitch {
                let active = callbacks.active.as_mut().unwrap();
                active.candidate = Some("model-b".into());
                active.pre_seen = true;
                active.control_succeeded = true;
            }
            let old = callback_fixture(&callbacks, event);
            callbacks
                .claim_callback_ids("request-id", "envelope-id")
                .unwrap();
            let before = callbacks.active.as_ref().unwrap();
            let before = (
                before.sequence,
                before.candidate.clone(),
                before.pre_seen,
                before.post_seen,
                before.control_succeeded,
            );
            let error = match callbacks
                .handle(&old, Some("source-session"), &events, &tools)
                .await
            {
                Ok(_) => panic!("replayed callback reached current dispatch"),
                Err(error) => error,
            };
            assert!(format!("{error:#}").contains("callback identity was replayed"));
            let after = callbacks.active.as_ref().unwrap();
            assert_eq!(
                (
                    after.sequence,
                    after.candidate.clone(),
                    after.pre_seen,
                    after.post_seen,
                    after.control_succeeded,
                ),
                before
            );
            assert!(receiver.try_recv().is_err());
        }
    }
}
