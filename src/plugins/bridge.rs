//! Trusted backend callbacks. The backend owner retains its lifetime pipe while
//! awaiting a handler; callback failure must never be converted into backend EOF.
use anyhow::{Context, Result, ensure};
use async_trait::async_trait;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, io::Read, sync::Arc, time::Duration};

use crate::events::EventSink;

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

impl Lifecycle {
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
        })
    }
}

pub(crate) struct Callbacks {
    lifecycle: Arc<Lifecycle>,
    ids: BTreeMap<String, &'static str>,
    responses: BTreeMap<String, ([u8; 32], Value)>,
    sequence: u64,
}

impl Callbacks {
    pub(crate) async fn handle_codex(
        &mut self,
        input: Value,
        session: &str,
        turn: &str,
        events: &EventSink,
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
        let response = self.handle(&message, Some(session), events).await?;
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

    pub(crate) async fn handle(
        &mut self,
        message: &Value,
        expected_session: Option<&str>,
        events: &EventSink,
    ) -> Result<Value> {
        let request = &message["request"];
        let id = message["request_id"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 256)
            .context("invalid lifecycle request identity")?;
        let callback = request["callback_id"]
            .as_str()
            .context("missing lifecycle callback identity")?;
        let event = self
            .ids
            .get(callback)
            .context("unregistered lifecycle callback")?;
        let input = &request["input"];
        ensure!(
            input["hook_event_name"] == *event,
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
            event: (*event).into(),
            session: session.into(),
            input: input.clone(),
        };
        let decision = tokio::time::timeout(
            self.lifecycle.deadline,
            self.lifecycle.handler.handle(&invocation, events),
        )
        .await
        .context("backend lifecycle handler timed out; guarded work remains held")??;
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
                    *event == "PreCompact",
                    "continuation held after completed compaction: {reason}"
                );
                json!({"decision":"block","reason":reason})
            }
        };
        self.responses.insert(id.into(), (digest, response.clone()));
        Ok(response)
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
