//! Legacy requirements came from handler class. Absence must preserve that policy.
use super::*;
impl<'de> Deserialize<'de> for HookReceipt {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut value = Value::deserialize(deserializer)?;
        if let Some(object) = value.as_object_mut()
            && !object.contains_key("required_gate")
        {
            let required = object.get("class").and_then(Value::as_str) != Some("observer");
            object.insert("required_gate".into(), Value::Bool(required));
        }
        WireReceipt::deserialize(value).map_err(serde::de::Error::custom)
    }
}
// Remote derive constructs HookReceipt directly and catches added fields at compile time.
#[derive(Deserialize)]
#[serde(remote = "HookReceipt")]
struct WireReceipt {
    #[serde(default = "required_gate_default")]
    pub required_gate: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observer: Option<crate::plugins::observer::ObserverReceipt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<crate::plugins::once::CapturedSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub once: Option<crate::plugins::once::OnceAttempt>,
    pub invocation: u32,
    pub declaration: DeclarationIdentity,
    pub class: HandlerClass,
    pub endpoint: Option<String>,
    pub inspected: AdmissionKey,
    /// None means the runner may have executed: never automatically replay.
    pub outcome: Option<RawOutcome>,
    /// Reserved invocations and interrupted/failed transports need reconciliation.
    pub uncertain_effects: bool,
    pub hold: Option<String>,
    pub questions: Vec<Question>,
    /// The validated wire remains in outcome; these names identify proposals whose
    /// host owners have not integrated yet. They never imply execution.
    pub pending_proposals: Vec<PendingProposal>,
}
