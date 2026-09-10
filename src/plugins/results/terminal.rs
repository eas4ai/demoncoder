//! Parse the complete terminal value into safe notices. Never pass raw controls on.
use super::{TerminalNotice, Untrusted};

pub(super) fn parse(mut text: &str) -> Option<Vec<TerminalNotice>> {
    let mut notices = Vec::new();
    while !text.is_empty() {
        if let Some(rest) = text.strip_prefix('\u{7}') {
            notices.push(TerminalNotice::Bell);
            text = rest;
            continue;
        }
        let sequence = text.strip_prefix("\u{1b}]")?;
        let (code, rest) = sequence.split_once(';')?;
        let code = match code {
            "0" => 0,
            "1" => 1,
            "2" => 2,
            "9" => 9,
            "99" => 99,
            "777" => 777,
            _ => return None,
        };
        let end = rest.find(['\u{7}', '\u{1b}'])?;
        let payload = &rest[..end];
        if payload.chars().any(char::is_control) {
            return None;
        }
        let terminator = &rest[end..];
        text = if let Some(rest) = terminator.strip_prefix('\u{7}') {
            rest
        } else {
            terminator.strip_prefix("\u{1b}\\")?
        };
        notices.push(TerminalNotice::Osc {
            code,
            text: Untrusted::new(payload.into()),
        });
    }
    Some(notices)
}
