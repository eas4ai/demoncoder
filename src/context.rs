//! Current-request occupancy is separate from billed usage across requests.
use crate::events::ContextUsage;
use serde_json::Value;
use std::io::{self, Write};

struct ByteCount(u64);
impl Write for ByteCount {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0 = self.0.saturating_add(bytes.len() as u64);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl ContextUsage {
    pub(crate) fn estimate_request(body: &Value) -> Self {
        let mut count = ByteCount(0);
        serde_json::to_writer(&mut count, body).expect("JSON request serializes");
        Self {
            used: Some(count.0.div_ceil(4)),
            capacity: None,
            estimated: true,
        }
    }
    pub(crate) fn openai(usage: &Value) -> Self {
        Self {
            used: usage["input_tokens"]
                .as_u64()
                .and_then(|input| input.checked_add(usage["output_tokens"].as_u64()?)),
            ..Self::default()
        }
    }
    pub(crate) fn anthropic(usage: &Value) -> Self {
        let mut used = usage["input_tokens"].as_u64();
        let mut estimated = false;
        for field in [
            "cache_read_input_tokens",
            "cache_creation_input_tokens",
            "output_tokens",
        ] {
            match usage.get(field) {
                Some(value) => used = used.and_then(|n| n.checked_add(value.as_u64()?)),
                None => estimated = true,
            }
        }
        Self {
            used,
            capacity: None,
            estimated,
        }
    }
    pub(crate) fn codex(usage: &Value) -> Self {
        let last = &usage["last"];
        Self {
            used: last["totalTokens"].as_u64().or_else(|| {
                last["inputTokens"]
                    .as_u64()?
                    .checked_add(last["outputTokens"].as_u64()?)
            }),
            capacity: usage["modelContextWindow"].as_u64().filter(|n| *n > 0),
            estimated: false,
        }
    }
}

/// Anthropic and Claude partial messages share current-request usage semantics.
#[derive(Default)]
pub(crate) struct MessageContext(serde_json::Map<String, Value>);
impl MessageContext {
    pub(crate) fn observe(&mut self, event: &Value) -> Option<ContextUsage> {
        let usage = match event["type"].as_str()? {
            "message_start" => {
                self.0.clear();
                &event["message"]["usage"]
            }
            "message_delta" => &event["usage"],
            _ => return None,
        };
        for key in [
            "input_tokens",
            "cache_read_input_tokens",
            "cache_creation_input_tokens",
            "output_tokens",
        ] {
            if let Some(value) = usage.get(key) {
                // Preserve invalid/missing counts as unknown, without retaining payloads.
                self.0
                    .insert(key.into(), value.as_u64().map_or(Value::Null, Value::from));
            }
        }
        Some(ContextUsage::anthropic(&Value::Object(self.0.clone())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn context_message_start_resets_previous_request_and_deltas_replace_counts() {
        let mut context = MessageContext::default();
        context.observe(&json!({"type":"message_start","message":{"usage":{"input_tokens":100,"cache_read_input_tokens":50,"cache_creation_input_tokens":10,"output_tokens":1}}}));
        assert_eq!(
            context
                .observe(&json!({"type":"message_delta","usage":{"output_tokens":20}}))
                .unwrap()
                .used,
            Some(180)
        );
        let next = context
            .observe(&json!({"type":"message_start","message":{"usage":{"input_tokens":2}}}))
            .unwrap();
        assert_eq!(next.used, Some(2));
        assert!(next.estimated);
        assert_eq!(
            context
                .observe(&json!({"type":"message_delta","usage":{"output_tokens":-1}}))
                .unwrap()
                .used,
            None
        );
    }

    #[test]
    fn context_uses_last_request_and_counts_cached_anthropic_input() {
        assert_eq!(ContextUsage::codex(&json!({"last":{"inputTokens":20,"outputTokens":3},"total":{"totalTokens":90000},"modelContextWindow":1000})).used, Some(23));
        let context = ContextUsage::anthropic(
            &json!({"input_tokens":10,"cache_read_input_tokens":100,"cache_creation_input_tokens":50,"output_tokens":7}),
        );
        assert_eq!(context.used, Some(167));
        assert!(!context.estimated);
        assert!(ContextUsage::anthropic(&json!({"input_tokens":10})).estimated);
        assert_eq!(
            ContextUsage::openai(&json!({"input_tokens":u64::MAX,"output_tokens":1})).used,
            None
        );
        assert_eq!(ContextUsage::openai(&json!({})).used, None);
    }
    #[test]
    fn context_estimate_includes_instructions_and_tools_without_inventing_capacity() {
        let body = json!({"instructions":"rules".repeat(100),"tools":[{"description":"schema"}],"input":[{"content":"hello"}]});
        let estimate = ContextUsage::estimate_request(&body);
        assert_eq!(
            estimate.used,
            Some((body.to_string().len() as u64).div_ceil(4))
        );
        assert!(estimate.estimated);
        assert_eq!(estimate.capacity, None);
    }
}
