//! Transport framing precedes source shape and effect interpretation.
use super::*;
use crate::plugins::wire::{self, WireResult};

pub(super) enum Payload {
    Empty,
    Json(Value),
    Plain(String),
    Worktree(String),
}
pub(super) struct Framed {
    pub payload: Payload,
    pub exit_two: bool,
    pub stderr: Option<Untrusted<String>>,
    pub failures: Vec<WireError>,
}

pub(super) fn frame(
    dialect: HookDialect,
    event: HookEvent,
    handler: HandlerKind,
    response: HookResponse<'_>,
) -> WireResult<Framed> {
    let mut result = Framed {
        payload: Payload::Empty,
        exit_two: false,
        stderr: None,
        failures: Vec::new(),
    };
    match response {
        HookResponse::Failure(kind) => {
            return Err(WireError::new(
                "/transport",
                match kind {
                    TransportFailure::Timeout => "handler timed out",
                    TransportFailure::Cancelled => "handler cancelled",
                    TransportFailure::Execution => "handler execution failed",
                    TransportFailure::Network => "handler transport failed",
                    TransportFailure::InvalidEncoding => "handler returned invalid encoding",
                },
            ));
        }
        HookResponse::Command {
            exit_code,
            stdout,
            stderr,
        } => {
            require_kind(handler, HandlerKind::Command)?;
            if stdout.len().saturating_add(stderr.len()) > wire::MAX_WIRE_BYTES {
                return Err(WireError::new("/command", "combined output limit exceeded"));
            }
            let stdout = bounded_text(stdout)?;
            let stderr = bounded_text(stderr)?;
            result.stderr = (!stderr.is_empty()).then(|| Untrusted::new(stderr.to_owned()));
            result.exit_two = exit_code == Some(2);
            if exit_code != Some(0) {
                result.failures.push(WireError::new(
                    "/command/exit",
                    "command did not exit successfully",
                ));
            }
            if event == HookEvent::WorktreeCreate {
                if exit_code == Some(0) {
                    let output = if dialect == HookDialect::Claude {
                        strip_worktree_ansi(stdout)
                    } else {
                        Ok(stdout.to_owned())
                    };
                    match output {
                        Ok(output) => {
                            result.payload = Payload::Worktree(
                                output
                                    .lines()
                                    .rev()
                                    .map(str::trim)
                                    .find(|line| !line.is_empty())
                                    .unwrap_or_default()
                                    .into(),
                            )
                        }
                        Err(error) => result.failures.push(error),
                    }
                }
            } else if dialect != HookDialect::Codex || exit_code == Some(0) {
                // Codex ignores all stdout on failed exits, including otherwise valid JSON.
                result.payload = if dialect == HookDialect::Codex && event == HookEvent::SessionEnd
                {
                    Payload::Empty
                } else {
                    match command_payload(dialect, stdout) {
                        Ok(Payload::Plain(_)) if exit_code != Some(0) => Payload::Empty,
                        Ok(payload) => payload,
                        Err(error) => {
                            result.failures.push(error);
                            Payload::Empty
                        }
                    }
                };
            }
        }
        HookResponse::Http { status, body } => {
            require_kind(handler, HandlerKind::Http)?;
            let body = bounded_text(body)?;
            if !(200..300).contains(&status) {
                return Err(WireError::new(
                    "/http/status",
                    "HTTP response was not successful",
                ));
            }
            result.payload = if body.trim().is_empty() {
                Payload::Empty
            } else {
                Payload::Json(wire::parse_json(body.as_bytes())?)
            };
        }
        HookResponse::Mcp {
            structured,
            text,
            is_error,
        } => {
            require_kind(handler, HandlerKind::McpTool)?;
            // Bound even ignored alternative content, before joining or cloning.
            if text.len() > wire::MAX_WIRE_NODES {
                return Err(WireError::new("/mcp/content", "MCP content count exceeded"));
            }
            let mut bytes = text.len();
            for part in text {
                wire::measure_string(part)?;
                bytes = bytes.saturating_add(part.len());
            }
            if bytes > wire::MAX_WIRE_BYTES {
                return Err(WireError::new(
                    "/mcp/content",
                    "MCP content byte limit exceeded",
                ));
            }
            if let Some(value) = structured {
                wire::measure(value)?;
            }
            if is_error {
                return Err(WireError::new("/mcp/isError", "MCP tool reported failure"));
            }
            result.payload = if let Some(value) = structured {
                if !value.is_object() {
                    return Err(WireError::new(
                        "/mcp/structuredContent",
                        "structured hook result must be an object",
                    ));
                }
                Payload::Json(value.clone())
            } else {
                command_payload(dialect, &text.join("\n"))?
            };
        }
        HookResponse::Callback(value) => {
            wire::measure(value)?;
            result.payload = Payload::Json(value.clone());
        }
    }
    Ok(result)
}

fn require_kind(actual: HandlerKind, expected: HandlerKind) -> WireResult {
    if actual != expected {
        return Err(WireError::new(
            "/transport",
            "transport does not match admitted handler kind",
        ));
    }
    Ok(())
}
fn bounded_text(bytes: &[u8]) -> WireResult<&str> {
    if bytes.len() > wire::MAX_WIRE_BYTES {
        return Err(WireError::new("/transport", "wire byte limit exceeded"));
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| WireError::new("/transport", "response is not UTF-8"))?;
    wire::measure_string(text)?;
    Ok(text)
}

fn command_payload(dialect: HookDialect, stdout: &str) -> WireResult<Payload> {
    let text = stdout.trim();
    if text.is_empty() {
        return Ok(Payload::Empty);
    }
    if dialect == HookDialect::Claude {
        // Claude treats arrays, quoted strings and unfinished braces as plain text.
        if !text.starts_with('{') || !text.ends_with('}') {
            return Ok(Payload::Plain(text.into()));
        }
        // A stream of unrelated JSON log objects is plain text. A line containing
        // an output field makes the entire stream an invalid hook response.
        if text.lines().count() > 1 && json_log_lines(text)? {
            return Ok(Payload::Plain(text.into()));
        }
        return Ok(Payload::Json(wire::parse_json(text.as_bytes())?));
    }
    if text.starts_with('{') || text.starts_with('[') {
        Ok(Payload::Json(wire::parse_json(text.as_bytes())?))
    } else {
        Ok(Payload::Plain(text.into()))
    }
}
fn json_log_lines(text: &str) -> WireResult<bool> {
    let mut count = 0;
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let Ok(value) = wire::parse_json(line.as_bytes()) else {
            return Ok(false);
        };
        count += 1;
        if let Some(object) = value.as_object()
            && object
                .keys()
                .any(|key| super::native::UNIVERSAL_FIELDS.contains(&key.as_str()))
        {
            return Ok(false);
        }
    }
    Ok(count > 1)
}

/// Source WorktreeCreate strips decorations before selecting its last path line.
/// This parser consumes complete ANSI sequences; it never emits any controls.
/// Incomplete or malformed sequences fail rather than becoming a hidden path.
fn strip_worktree_ansi(text: &str) -> WireResult<String> {
    let mut output = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '\u{1b}' => match chars.next() {
                Some('[') => consume_csi(&mut chars)?,
                Some(']') => consume_osc(&mut chars)?,
                Some(ch) if (' '..='/').contains(&ch) => {
                    let mut final_byte = chars.next().ok_or_else(invalid_ansi)?;
                    while (' '..='/').contains(&final_byte) {
                        final_byte = chars.next().ok_or_else(invalid_ansi)?;
                    }
                    if !('0'..='~').contains(&final_byte) {
                        return Err(invalid_ansi());
                    }
                }
                Some(ch) if ('0'..='~').contains(&ch) => {}
                _ => return Err(invalid_ansi()),
            },
            '\u{9b}' => consume_csi(&mut chars)?,
            '\u{9d}' => consume_osc(&mut chars)?,
            _ => output.push(ch),
        }
    }
    Ok(output)
}
fn consume_csi(chars: &mut std::str::Chars<'_>) -> WireResult {
    let mut intermediate = false;
    for ch in chars.by_ref() {
        match ch {
            '@'..='~' => return Ok(()),
            ' '..='/' => intermediate = true,
            '0'..='?' if !intermediate => {}
            _ => return Err(invalid_ansi()),
        }
    }
    Err(invalid_ansi())
}
fn consume_osc(chars: &mut std::str::Chars<'_>) -> WireResult {
    while let Some(ch) = chars.next() {
        match ch {
            '\u{7}' | '\u{9c}' => return Ok(()),
            '\u{1b}' if chars.next() == Some('\\') => return Ok(()),
            ch if ch.is_control() => return Err(invalid_ansi()),
            _ => {}
        }
    }
    Err(invalid_ansi())
}
fn invalid_ansi() -> WireError {
    WireError::new("/stdout", "malformed worktree ANSI decoration")
}
