//! Trusted backend callbacks. The backend owner retains its lifetime pipe while
//! awaiting a handler; callback failure must never be converted into backend EOF.
use anyhow::{Context, Result, ensure};
use async_trait::async_trait;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, io::Read, sync::Arc, time::Duration};

use crate::events::EventSink;
use crate::plugins::receipts::*;
use crate::tools::{AccessPolicy, ToolExecutor};

const EVENTS: [&str; 2] = ["PreCompact", "PostCompact"];
const MAX_CALLBACKS: usize = 1024;
const MAX_INPUT: usize = 1024 * 1024;
const MAX_REASON: usize = 4096;

#[derive(Clone, Debug)]
pub enum Decision {
    Continue,
    Block(String),
}

/// Attribution comes from the session owner and authenticated callback channel,
/// not from model text. The handler owns durable policy admission and accounting.
pub struct Invocation {
    pub version: u32,
    pub operation_id: String,
    pub sequence: u64,
    pub event: String,
    pub session: String,
    pub input: Value,
}

#[async_trait]
pub trait Handler: Send + Sync {
    async fn handle(&self, request: &Invocation, events: &EventSink) -> Result<Decision>;
}

pub struct Lifecycle {
    handler: Arc<dyn Handler>,
    deadline: Duration,
}

struct Observe;
#[async_trait]
impl Handler for Observe {
    async fn handle(&self, _: &Invocation, _: &EventSink) -> Result<Decision> {
        Ok(Decision::Continue)
    }
}
impl Lifecycle {
    pub(crate) fn for_access(access: &AccessPolicy) -> Result<Option<Arc<Self>>> {
        if let Some(lifecycle) = &access.lifecycle {
            return Ok(Some(lifecycle.clone()));
        }
        if access.non_tools.iter().any(|p| {
            matches!(
                p.plan.event,
                crate::plugins::hook_types::HookEvent::PreCompact
                    | crate::plugins::hook_types::HookEvent::PostCompact
            )
        }) {
            return Ok(Some(Arc::new(Self::new(
                Arc::new(Observe),
                Duration::from_secs(30),
            )?)));
        }
        Ok(None)
    }
    pub fn new(handler: Arc<dyn Handler>, deadline: Duration) -> Result<Self> {
        ensure!(
            !deadline.is_zero() && deadline <= Duration::from_secs(60),
            "backend lifecycle deadline must be positive and no more than 60 seconds"
        );
        Ok(Self { handler, deadline })
    }

    pub(crate) fn callbacks(self: &Arc<Self>) -> Result<Callbacks> {
        let mut entropy = [0u8; 32];
        std::fs::File::open("/dev/urandom")?
            .read_exact(&mut entropy)
            .context("create private lifecycle callback identities")?;
        let nonce = entropy
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        Ok(Callbacks {
            lifecycle: self.clone(),
            ids: EVENTS
                .into_iter()
                .map(|event| (format!("{nonce}:{event}"), event))
                .collect(),
            responses: BTreeMap::new(),
            sequence: 0,
            managed: None,
        })
    }
}

pub(crate) struct Callbacks {
    lifecycle: Arc<Lifecycle>,
    ids: BTreeMap<String, &'static str>,
    responses: BTreeMap<String, ([u8; 32], Value)>,
    sequence: u64,
    managed: Option<Managed>,
}
struct Managed {
    backend: u64,
    transaction: u64,
    pre: u64,
    post: Option<u64>,
}

impl Callbacks {
    pub(crate) async fn handle_codex_managed(
        &mut self,
        input: Value,
        session: &str,
        turn: &str,
        events: &EventSink,
        tools: &ToolExecutor,
    ) -> Result<Value> {
        self.handle_codex_inner(input, session, turn, events, Some(tools))
            .await
    }
    pub(crate) async fn handle_codex(
        &mut self,
        input: Value,
        session: &str,
        turn: &str,
        events: &EventSink,
    ) -> Result<Value> {
        self.handle_codex_inner(input, session, turn, events, None)
            .await
    }
    async fn handle_codex_inner(
        &mut self,
        input: Value,
        session: &str,
        turn: &str,
        events: &EventSink,
        tools: Option<&ToolExecutor>,
    ) -> Result<Value> {
        let event = input["hook_event_name"]
            .as_str()
            .context("missing managed event")?;
        let callback = self
            .ids
            .iter()
            .find(|(_, value)| **value == event)
            .map(|(id, _)| id.clone())
            .context("unregistered managed event")?;
        let challenge = input["demonCoderCompaction"]["challenge"]
            .as_str()
            .filter(|value| !value.is_empty() && value.len() <= 256)
            .context("invalid managed compaction challenge")?
            .to_owned();
        ensure!(
            input["demonCoderCompaction"]["protocol"] == "demoncoder-compaction-v1",
            "unknown managed compaction protocol"
        );
        ensure!(
            input["turn_id"] == turn,
            "managed compaction belongs to another turn"
        );
        let acknowledgement = json!({"protocol":"demoncoder-compaction-v1", "challenge":challenge,
            "hook_event_name":event,"session_id":session,"turn_id":turn});
        let message =
            json!({"request_id":challenge,"request":{"callback_id":callback,"input":input}});
        let response = self
            .handle_inner(&message, Some(session), events, tools, true)
            .await?;
        let mut output = json!({"continue":response["decision"] != "block", "demonCoderCompaction":acknowledgement});
        if let Some(reason) = response.get("reason") {
            output["stopReason"] = reason.clone();
        }
        Ok(output)
    }

    pub(crate) fn registration(&self) -> Value {
        let mut hooks = serde_json::Map::new();
        for (id, event) in &self.ids {
            // Host timeout is always shorter. The SDK's own timeout fails open,
            // so it must never be the lifecycle owner's failure mechanism.
            hooks.insert(
                (*event).into(),
                json!([{"hookCallbackIds":[id],"timeout":3600}]),
            );
        }
        Value::Object(hooks)
    }

    pub(crate) async fn handle_managed(
        &mut self,
        message: &Value,
        session: Option<&str>,
        events: &EventSink,
        tools: &ToolExecutor,
    ) -> Result<Value> {
        self.handle_inner(message, session, events, Some(tools), false)
            .await
    }
    #[cfg(test)]
    pub(crate) async fn handle(
        &mut self,
        message: &Value,
        session: Option<&str>,
        events: &EventSink,
    ) -> Result<Value> {
        self.handle_inner(message, session, events, None, false)
            .await
    }
    async fn handle_inner(
        &mut self,
        message: &Value,
        expected_session: Option<&str>,
        events: &EventSink,
        tools: Option<&ToolExecutor>,
        codex: bool,
    ) -> Result<Value> {
        let request = &message["request"];
        let id = message["request_id"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 256)
            .context("invalid lifecycle request identity")?;
        let callback = request["callback_id"]
            .as_str()
            .context("missing lifecycle callback identity")?;
        let event = *self
            .ids
            .get(callback)
            .context("unregistered lifecycle callback")?;
        let input = &request["input"];
        ensure!(
            input["hook_event_name"] == event,
            "lifecycle event does not match its callback"
        );
        let session = input["session_id"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 256)
            .context("missing lifecycle session identity")?;
        ensure!(
            expected_session == Some(session),
            "lifecycle callback belongs to a different session"
        );
        let bytes = serde_json::to_vec(request)?;
        ensure!(
            bytes.len() <= MAX_INPUT,
            "lifecycle callback exceeds input limit"
        );
        let digest: [u8; 32] = Sha256::digest(&bytes).into();
        if let Some((old, response)) = self.responses.get(id) {
            ensure!(
                *old == digest,
                "repeated lifecycle request changed its input"
            );
            return Ok(response.clone());
        }
        ensure!(
            self.responses.len() < MAX_CALLBACKS,
            "lifecycle callback history is full; start a new backend session"
        );
        self.sequence += 1;
        let invocation = Invocation {
            version: 1,
            operation_id: id.into(),
            sequence: self.sequence,
            event: event.into(),
            session: session.into(),
            input: input.clone(),
        };
        let decision = tokio::time::timeout(
            self.lifecycle.deadline,
            self.lifecycle.handler.handle(&invocation, events),
        )
        .await
        .context("backend lifecycle handler timed out; guarded work remains held")??;
        let decision = if matches!(decision, Decision::Continue) {
            if let Some(tools) = tools.filter(|t| {
                t.has_non_tool_plan(crate::plugins::hook_types::HookEvent::PreCompact)
                    || t.has_non_tool_plan(crate::plugins::hook_types::HookEvent::PostCompact)
            }) {
                self.dispatch_managed(&invocation, events, tools, codex)
                    .await?
            } else {
                decision
            }
        } else {
            decision
        };
        let response = match decision {
            Decision::Continue => json!({}),
            Decision::Block(reason) => {
                ensure!(
                    !reason.is_empty() && reason.len() <= MAX_REASON,
                    "invalid lifecycle block reason"
                );
                // PostCompact has already happened. An objection can stop future
                // work but cannot masquerade as a veto of the completed compaction.
                ensure!(
                    event == "PreCompact",
                    "continuation held after completed compaction: {reason}"
                );
                json!({"decision":"block","reason":reason})
            }
        };
        self.responses.insert(id.into(), (digest, response.clone()));
        Ok(response)
    }
    async fn dispatch_managed(
        &mut self,
        request: &Invocation,
        events: &EventSink,
        tools: &ToolExecutor,
        codex: bool,
    ) -> Result<Decision> {
        use crate::plugins::hook_types::HookEvent;
        let backend = events
            .backend_invocation_id()
            .context("managed compaction backend missing")?;
        let event = if request.event == "PreCompact" {
            HookEvent::PreCompact
        } else {
            HookEvent::PostCompact
        };
        let mut input = request.input.clone();
        if codex {
            input
                .as_object_mut()
                .context("source input missing")?
                .remove("demonCoderCompaction");
        }
        ensure!(
            input["cwd"].as_str() == tools.hook_host().workspace.to_str(),
            "source compaction workspace differs"
        );
        let profile = crate::plugins::profile::CompatibilityProfile::embedded()?;
        if codex {
            profile.validate_schema(
                &crate::plugins::profile::SchemaKey::Codex {
                    path: format!(
                        "codex-rs/hooks/schema/generated/{}.command.input.schema.json",
                        if event == HookEvent::PreCompact {
                            "pre-compact"
                        } else {
                            "post-compact"
                        }
                    ),
                    definition: None,
                },
                &input,
            )?;
        } else {
            profile.validate_claude_input(event, &input)?;
        }
        let transaction = if event == HookEvent::PreCompact {
            ensure!(
                self.managed.is_none(),
                "source compaction overlaps an unfinished transaction"
            );
            events.begin_external_compaction(&input)?
        } else {
            let managed = self.managed.as_ref().context("orphan source PostCompact")?;
            ensure!(
                managed.backend == backend && managed.post.is_none(),
                "source compaction owner changed or repeated"
            );
            let (runtime, _) = events.for_non_tool_context(managed.pre)?;
            runtime.source_lifecycle_delivery(managed.pre, backend, false)?;
            events.observe_external_compaction(managed.transaction, &input)?;
            managed.transaction
        };
        let trigger = input["trigger"]
            .as_str()
            .context("compaction trigger missing")?
            .to_owned();
        let occurrence = if event == HookEvent::PreCompact {
            NonToolOccurrence::PreCompact {
                compaction: Some(transaction),
                trigger,
                custom_instructions: input["custom_instructions"].as_str().map(str::to_owned),
            }
        } else {
            NonToolOccurrence::PostCompact {
                compaction: Some(transaction),
                trigger,
                compact_summary: input["compact_summary"].as_str().map(str::to_owned),
            }
        };
        let observed = events.for_observed_lifecycle(ObservedCallback {
            input: if codex {
                ObservedLifecycle::Codex(input)
            } else {
                ObservedLifecycle::Claude(input)
            },
            correlation: SourceCallback {
                origin: Some(if events.is_plugin_prompt() {
                    SourceOrigin::PluginContext
                } else {
                    SourceOrigin::HostSubmission
                }),
                backend_operation: backend,
                sequence: request.sequence,
                request_id: request.operation_id.clone(),
                command_uuid: None,
                command_request_id: None,
                envelope_id: None,
                model: None,
            },
        })?;
        let outcome = tools
            .dispatch_non_tool(occurrence, &observed)
            .await?
            .context("managed source occurrence missing")?;
        if event == HookEvent::PreCompact {
            self.managed = Some(Managed {
                backend,
                transaction,
                pre: outcome.operation,
                post: None,
            });
        } else {
            self.managed.as_mut().expect("validated").post = Some(outcome.operation);
        }
        if let Some(reason) = outcome.hold {
            return Ok(Decision::Block(reason));
        }
        ensure!(
            !outcome.correction,
            "source compaction cannot create native correction authority"
        );
        if let Some(validation) = outcome.validation {
            let _guard = validation.validate().await?;
        }
        Ok(Decision::Continue)
    }
    pub(crate) fn managed_sent(&self, events: &EventSink) -> Result<()> {
        if let Some(managed) = &self.managed {
            let id = managed.post.unwrap_or(managed.pre);
            let (runtime, _) = events.for_non_tool_context(id)?;
            runtime.source_lifecycle_delivery(id, managed.backend, true)?;
        }
        Ok(())
    }
    pub(crate) fn observe_managed(&mut self, message: &Value, events: &EventSink) -> Result<()> {
        let terminal = message["type"] == "result"
            || (message["type"] == "system" && message["subtype"] == "compact_boundary")
            || matches!(
                message["method"].as_str(),
                Some("thread/compacted" | "turn/completed")
            );
        if terminal && self.managed.as_ref().is_some_and(|m| m.post.is_some()) {
            let managed = self.managed.as_ref().expect("checked");
            ensure!(
                events.backend_invocation_id() == Some(managed.backend),
                "compaction acknowledgment backend differs"
            );
            let (runtime, _) = events.for_non_tool_context(managed.post.expect("checked"))?;
            runtime.source_lifecycle_delivery(
                managed.post.expect("checked"),
                managed.backend,
                false,
            )?;
            runtime.end_compaction(managed.transaction, None)?;
            self.managed = None;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Count(Arc<AtomicUsize>);
    #[async_trait]
    impl Handler for Count {
        async fn handle(&self, _: &Invocation, _: &EventSink) -> Result<Decision> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(Decision::Continue)
        }
    }

    #[tokio::test]
    async fn callback_retry_is_idempotent_and_cannot_change_admitted_input() -> Result<()> {
        let count = Arc::new(AtomicUsize::new(0));
        let lifecycle = Arc::new(Lifecycle::new(
            Arc::new(Count(count.clone())),
            Duration::from_secs(1),
        )?);
        let mut callbacks = lifecycle.callbacks()?;
        let id = callbacks.registration()["PreCompact"][0]["hookCallbackIds"][0].clone();
        let (tx, _rx) = tokio::sync::mpsc::channel(16);
        let events = EventSink::new("bridge-retry".into(), tx, None)?;
        let mut message = json!({"request_id":"one","request":{"callback_id":id,"input":{"hook_event_name":"PreCompact","session_id":"session","trigger":"manual"}}});
        assert_eq!(
            callbacks.handle(&message, Some("session"), &events).await?,
            json!({})
        );
        assert_eq!(
            callbacks.handle(&message, Some("session"), &events).await?,
            json!({})
        );
        assert_eq!(count.load(Ordering::SeqCst), 1);
        message["request"]["input"]["trigger"] = json!("auto");
        assert!(
            callbacks
                .handle(&message, Some("session"), &events)
                .await
                .is_err()
        );
        assert_eq!(count.load(Ordering::SeqCst), 1);
        Ok(())
    }

    #[tokio::test]
    async fn callback_identity_cannot_cross_event_session_or_backend_lifetime() -> Result<()> {
        let count = Arc::new(AtomicUsize::new(0));
        let lifecycle = Arc::new(Lifecycle::new(
            Arc::new(Count(count.clone())),
            Duration::from_secs(1),
        )?);
        let mut callbacks = lifecycle.callbacks()?;
        let other = lifecycle.callbacks()?;
        let registration = callbacks.registration();
        let id = registration["PreCompact"][0]["hookCallbackIds"][0].clone();
        let (tx, _rx) = tokio::sync::mpsc::channel(16);
        let events = EventSink::new("bridge-identity".into(), tx, None)?;
        for (callback, event, session) in [
            (id.clone(), "PostCompact", "session"),
            (id.clone(), "PreCompact", "other-session"),
            (
                other.registration()["PreCompact"][0]["hookCallbackIds"][0].clone(),
                "PreCompact",
                "session",
            ),
        ] {
            let message = json!({"request_id":"one","request":{"callback_id":callback,"input":{"hook_event_name":event,"session_id":session,"trigger":"manual"}}});
            assert!(
                callbacks
                    .handle(&message, Some("session"), &events)
                    .await
                    .is_err()
            );
        }
        assert_eq!(count.load(Ordering::SeqCst), 0);
        Ok(())
    }
}
