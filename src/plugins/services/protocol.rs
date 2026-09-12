//! Strict MCP 2025-11-25 wire boundaries; no protocol upgrade by inference.
use anyhow::{Result, ensure};
use serde_json::{Value, json};

pub(super) const VERSION: &str = "2025-11-25";
pub(super) const MAX_MESSAGE: usize = 28 * 1024;
pub(super) const MAX_MESSAGES: usize = 64;
pub(super) type Secrets = std::sync::Arc<std::sync::Mutex<Vec<String>>>;

pub(super) enum Message {
    Result(Value),
    Request { id: Value, ping: bool },
    Notification { tools_changed: bool },
}
pub(super) fn encode(value: &Value) -> Result<Vec<u8>> {
    crate::plugins::wire::measure(value)?;
    struct Bounded(Vec<u8>);
    impl std::io::Write for Bounded {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if self.0.len().saturating_add(bytes.len()) > MAX_MESSAGE {
                return Err(std::io::Error::other("MCP outbound message exceeds bound"));
            }
            self.0.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut out = Bounded(Vec::new());
    serde_json::to_writer(&mut out, value)?;
    Ok(out.0)
}
pub(super) fn parse(bytes: &[u8], expected: u64) -> Result<Message> {
    ensure!(
        bytes.len() <= MAX_MESSAGE,
        "MCP inbound message exceeds bound"
    );
    let value = crate::plugins::wire::parse_json(bytes)?;
    let o = value
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("MCP batch or invalid envelope"))?;
    ensure!(
        o.get("jsonrpc") == Some(&json!("2.0")),
        "MCP invalid JSON-RPC version"
    );
    if let Some(method) = o.get("method") {
        ensure!(
            !o.contains_key("result")
                && !o.contains_key("error")
                && method
                    .as_str()
                    .is_some_and(|s| !s.is_empty() && s.len() <= 128)
                && o.get("params").is_none_or(Value::is_object),
            "MCP invalid server message"
        );
        if let Some(id) = o.get("id") {
            ensure!(valid_id(id), "MCP invalid server request ID");
            Ok(Message::Request {
                id: id.clone(),
                ping: method == "ping",
            })
        } else {
            Ok(Message::Notification {
                tools_changed: method == "notifications/tools/list_changed",
            })
        }
    } else {
        ensure!(
            o.get("id").and_then(Value::as_u64) == Some(expected)
                && o.contains_key("result") != o.contains_key("error")
                && !o.contains_key("params"),
            "MCP stale or malformed response"
        );
        ensure!(!o.contains_key("error"), "MCP server rejected request");
        let result = o.get("result").expect("checked result");
        ensure!(result.is_object(), "MCP result must be an object");
        Ok(Message::Result(result.clone()))
    }
}
fn valid_id(id: &Value) -> bool {
    id.as_i64().is_some()
        || id.as_u64().is_some()
        || id.as_str().is_some_and(|s| !s.is_empty() && s.len() <= 128)
}
pub(super) fn reply(id: Value, ping: bool) -> Value {
    if ping {
        json!({"jsonrpc":"2.0","id":id,"result":{}})
    } else {
        json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"Host capability is not implemented"}})
    }
}
pub(super) fn check_secrets(bytes: &[u8], secrets: &[String]) -> Result<()> {
    let text =
        std::str::from_utf8(bytes).map_err(|_| anyhow::anyhow!("MCP invalid response encoding"))?;
    fn reflects(value: &Value, secret: &str, depth: usize, work: &mut usize) -> Result<bool> {
        ensure!(
            depth <= 8 && *work < MAX_MESSAGE * 8,
            "MCP secret validation exceeds bound"
        );
        *work += 1;
        match value {
            Value::String(s) => {
                *work = work.saturating_add(s.len());
                if s.contains(secret) {
                    return Ok(true);
                }
                // MCP text may itself carry source JSON. Validate its decoded
                // strings too, before that text can become an ordinary receipt.
                if let Ok(inner) = crate::plugins::wire::parse_json(s.as_bytes()) {
                    return reflects(&inner, secret, depth + 1, work);
                }
                if s.contains('\\') {
                    let quoted = format!("\"{s}\"");
                    if let Ok(inner) = crate::plugins::wire::parse_json(quoted.as_bytes())
                        && inner.as_str().is_some_and(|decoded| decoded != s)
                    {
                        return reflects(&inner, secret, depth + 1, work);
                    }
                }
                Ok(false)
            }
            Value::Array(a) => {
                for value in a {
                    if reflects(value, secret, depth, work)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            Value::Object(o) => {
                for (key, value) in o {
                    if key.contains(secret) || reflects(value, secret, depth, work)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            _ => Ok(false),
        }
    }
    ensure!(
        !secrets.iter().any(|s| text.contains(s)),
        "MCP response reflects bound secret"
    );
    let value = crate::plugins::wire::parse_json(bytes)?;
    for secret in secrets {
        ensure!(
            !reflects(&value, secret, 0, &mut 0)?,
            "MCP response reflects bound secret"
        );
    }
    Ok(())
}
pub(super) fn initialize_result(result: &Value) -> Result<()> {
    ensure!(
        result.get("protocolVersion").and_then(Value::as_str) == Some(VERSION),
        "MCP unsupported negotiated version"
    );
    let caps = result
        .get("capabilities")
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow::anyhow!("MCP capabilities missing"))?;
    ensure!(
        caps.get("tools").is_some_and(Value::is_object)
            && caps["tools"]
                .get("listChanged")
                .is_none_or(Value::is_boolean)
            && result
                .get("serverInfo")
                .is_some_and(|info| info.get("name").is_some_and(Value::is_string)
                    && info.get("version").is_some_and(Value::is_string)),
        "MCP invalid initialization result"
    );
    Ok(())
}
#[cfg(test)]
fn response(bytes: &[u8], id: u64) -> Result<Value> {
    match parse(bytes, id)? {
        Message::Result(value) => Ok(value),
        _ => anyhow::bail!("MCP response required"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_ambiguous_stale_and_unsolicited_replies() {
        for invalid in [
            r#"{"jsonrpc":"2.0","id":1,"result":{},"error":{"code":-1,"message":"secret"}}"#,
            r#"{"jsonrpc":"2.0","id":2,"result":{}}"#,
            r#"{"jsonrpc":"2.0","id":null,"result":{}}"#,
            r#"[{"jsonrpc":"2.0","id":1,"result":{}}]"#,
            r#"{"jsonrpc":"2.0","id":1,"method":"sampling/createMessage","params":{}}"#,
        ] {
            assert!(
                response(invalid.as_bytes(), 1).is_err(),
                "accepted malformed RPC envelope"
            );
        }
    }
    #[test]
    fn escaped_secret_in_json_text_content_cannot_enter_a_receipt() {
        let response = json!({"content":[{"type":"text","text":r#"{"reason":"\u0073\u0065\u0063\u0072\u0065\u0074"}"#}]}).to_string();
        assert!(
            check_secrets(response.as_bytes(), &["secret".into()]).is_err(),
            "escaped text credential was accepted"
        );
    }
}
