//! Source data substitution. The source profiles intentionally disagree on types/missing paths.
use super::event::LimitedInput;
use crate::plugins::hook_types::HookDialect;
use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::io::Write;

/// Independent implementation of pinned Codex v0.153.4 template semantics and
/// observed Claude 2.1.267 semantics. No upstream SDK implementation is copied.
/// Native deliberately selects the typed Codex policy.
pub(super) fn resolve(template: &Value, event: &Value, dialect: HookDialect) -> Result<Value> {
    crate::plugins::wire::measure(template)?;
    crate::plugins::wire::measure(event)?;
    fn visit(
        template: &Value,
        event: &Value,
        dialect: HookDialect,
        left: &mut usize,
    ) -> Result<Value> {
        ensure!(*left > 0, "MCP resolved input exceeds bound");
        *left -= 1;
        match template {
            Value::Object(o) => {
                let mut result = serde_json::Map::new();
                for (key, value) in o {
                    ensure!(
                        dialect != HookDialect::Claude || key != "__proto__",
                        "MCP source template requires non-JSON prototype assignment"
                    );
                    ensure!(key.len() <= *left, "MCP resolved input exceeds bound");
                    *left -= key.len();
                    result.insert(key.clone(), visit(value, event, dialect, left)?);
                }
                Ok(Value::Object(result))
            }
            Value::Array(a) => a
                .iter()
                .map(|v| visit(v, event, dialect, left))
                .collect::<Result<Vec<_>>>()
                .map(Value::Array),
            Value::String(s) => string(s, event, dialect, left),
            _ => Ok(template.clone()),
        }
    }
    let resolved = visit(template, event, dialect, &mut 65536)?;
    let mut encoded = LimitedInput {
        bytes: Vec::new(),
        maximum: 65536,
    };
    serde_json::to_writer(&mut encoded, &resolved).context("MCP resolved input exceeds bound")?;
    Ok(resolved)
}
fn string(text: &str, event: &Value, dialect: HookDialect, left: &mut usize) -> Result<Value> {
    let mut matches = Vec::new();
    let mut at = 0;
    while let Some(relative) = text[at..].find("${") {
        let start = at + relative;
        at = start + 2;
        let Some(end) = text[at..].find('}').map(|end| at + end) else {
            break;
        };
        let path = &text[at..end];
        let matches_source = if dialect == HookDialect::Claude {
            path.bytes().enumerate().all(|(i, b)| {
                b.is_ascii_alphabetic() || b == b'_' || (i > 0 && (b.is_ascii_digit() || b == b'.'))
            })
        } else {
            !path.contains('{')
        };
        if !path.is_empty() && matches_source {
            ensure!(matches.len() < 1024, "MCP placeholder count exceeds bound");
            matches.push((start, end + 1, path));
            at = end + 1;
        }
    }
    if dialect != HookDialect::Claude
        && matches.len() == 1
        && matches[0].0 == 0
        && matches[0].1 == text.len()
    {
        let value =
            lookup(event, matches[0].2, false)?.context("MCP placeholder field is missing")?;
        let mut encoded = LimitedInput {
            bytes: Vec::new(),
            maximum: *left,
        };
        serde_json::to_writer(&mut encoded, value.as_ref())
            .context("MCP resolved input exceeds bound")?;
        *left -= encoded.bytes.len();
        return Ok(value.into_owned());
    }
    let mut result = LimitedInput {
        bytes: Vec::new(),
        maximum: *left,
    };
    let mut previous = 0;
    for (start, end, path) in matches {
        result.write_all(&text.as_bytes()[previous..start])?;
        let referenced = lookup(event, path, dialect == HookDialect::Claude)?;
        let value = referenced.as_deref();
        if dialect != HookDialect::Claude {
            ensure!(value.is_some(), "MCP placeholder field is missing");
        }
        match value {
            Some(Value::String(s)) => result.write_all(s.as_bytes())?,
            None | Some(Value::Null) if dialect == HookDialect::Claude => {}
            Some(value) if dialect == HookDialect::Claude => js_json(value, &mut result)?,
            Some(value) => serde_json::to_writer(&mut result, &value)?,
            None => unreachable!("typed source checked missing value"),
        }
        previous = end;
    }
    result.write_all(&text.as_bytes()[previous..])?;
    *left -= result.bytes.len();
    Ok(Value::String(
        String::from_utf8(result.bytes).expect("UTF-8 source fragments"),
    ))
}
fn lookup<'a>(
    event: &'a Value,
    path: &str,
    claude: bool,
) -> Result<Option<std::borrow::Cow<'a, Value>>> {
    let mut value = event;
    let mut fields = path.split('.').peekable();
    while let Some(field) = fields.next() {
        let selected = match value {
            Value::Object(o) => o.get(field),
            Value::Array(a) if claude && field == "length" => {
                return Ok(fields
                    .peek()
                    .is_none()
                    .then(|| std::borrow::Cow::Owned(Value::from(a.len()))));
            }
            Value::Array(a) if claude => index(field).and_then(|n| a.get(n as usize)),
            _ => return Ok(None),
        };
        if selected.is_none() && claude {
            let object_member = [
                "__proto__",
                "constructor",
                "toString",
                "toLocaleString",
                "valueOf",
                "hasOwnProperty",
                "isPrototypeOf",
                "propertyIsEnumerable",
                "__defineGetter__",
                "__defineSetter__",
                "__lookupGetter__",
                "__lookupSetter__",
            ]
            .contains(&field);
            let array_member = value.is_array()
                && [
                    "at",
                    "concat",
                    "copyWithin",
                    "fill",
                    "find",
                    "findIndex",
                    "findLast",
                    "findLastIndex",
                    "lastIndexOf",
                    "pop",
                    "push",
                    "reverse",
                    "shift",
                    "unshift",
                    "slice",
                    "sort",
                    "splice",
                    "includes",
                    "indexOf",
                    "join",
                    "keys",
                    "entries",
                    "values",
                    "every",
                    "some",
                    "forEach",
                    "map",
                    "filter",
                    "flat",
                    "flatMap",
                    "reduce",
                    "reduceRight",
                    "toReversed",
                    "toSorted",
                    "toSpliced",
                    "with",
                ]
                .contains(&field);
            ensure!(
                !object_member && !array_member,
                "MCP source placeholder requires a non-JSON inherited property"
            );
        }
        let Some(selected) = selected else {
            return Ok(None);
        };
        value = selected;
    }
    Ok(Some(std::borrow::Cow::Borrowed(value)))
}
fn index(key: &str) -> Option<u32> {
    let index = key.parse::<u32>().ok()?;
    (index < u32::MAX && index.to_string() == key).then_some(index)
}
fn js_json(value: &Value, output: &mut LimitedInput) -> Result<()> {
    match value {
        Value::Number(n) => {
            let n = n.as_f64().context("MCP source number is invalid")?;
            output.write_all(ryu_js::Buffer::new().format(n).as_bytes())?;
        }
        Value::Array(a) => {
            output.write_all(b"[")?;
            for (i, v) in a.iter().enumerate() {
                if i > 0 {
                    output.write_all(b",")?;
                }
                js_json(v, output)?;
            }
            output.write_all(b"]")?;
        }
        Value::Object(o) => {
            // Values came from the exact canonical host-emitted event. Integer
            // indices are enumerated first by JS, even when event keys are sorted.
            let mut keys = o.keys().collect::<Vec<_>>();
            keys.sort_by_key(|key| (index(key).is_none(), index(key).unwrap_or(0)));
            output.write_all(b"{")?;
            for (i, key) in keys.iter().enumerate() {
                if i > 0 {
                    output.write_all(b",")?;
                }
                serde_json::to_writer(&mut *output, key)?;
                output.write_all(b":")?;
                js_json(&o[*key], output)?;
            }
            output.write_all(b"}")?;
        }
        _ => serde_json::to_writer(output, value)?,
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn codex_and_native_preserve_types_and_never_expand_substituted_data() {
        let event = json!({"tool_input":{"count":3,"text":"$(touch bad) ${tool_name}","nil":null},"tool_name":"write"});
        let template = json!({"${tool_name}":["${tool_input.count}","prefix ${tool_input.count}","${tool_input.text}","${tool_input.nil}"]});
        for dialect in [HookDialect::Codex, HookDialect::Native] {
            assert_eq!(
                resolve(&template, &event, dialect).unwrap(),
                json!({"${tool_name}":[3,"prefix 3","$(touch bad) ${tool_name}",null]})
            );
            assert!(resolve(&json!({"x":"${tool_input.missing}"}), &event, dialect).is_err());
            assert!(resolve(&json!({"x":"${a.0}"}), &json!({"a":["value"]}), dialect).is_err());
        }
    }
    #[test]
    fn claude_uses_strings_empty_missing_and_array_object_paths() {
        let event = json!({"tool_input":{"count":3,"yes":false,"nil":null,"items":["zero","one"],"text":"${tool_name}"},"tool_name":"write"});
        let template = json!({"${tool_name}":["${tool_input.count}","${tool_input.yes}","${tool_input.nil}","${tool_input.missing}","${tool_input.items.0}","${tool_input.items.00}","${tool_input.items.length}","${tool_input.text.0}","${tool_input.text}","${}","${tool_input.-}",7,null]});
        assert_eq!(
            resolve(&template, &event, HookDialect::Claude).unwrap(),
            json!({"${tool_name}":["3","false","","","zero","","2","","${tool_name}","${}","${tool_input.-}",7,null]})
        );
    }
    #[test]
    fn claude_numbers_and_whole_objects_follow_js_for_the_host_emitted_event() {
        let event = json!({"tool_input":{"z":1.0,"a":1e-7,"large":9007199254740993_u64,"2":"two","1":"one","10":"ten"}});
        assert_eq!(
            resolve(&json!({"x":"${tool_input}"}), &event, HookDialect::Claude).unwrap(),
            json!({"x":"{\"1\":\"one\",\"2\":\"two\",\"10\":\"ten\",\"a\":1e-7,\"large\":9007199254740992,\"z\":1}"})
        );
    }
    #[test]
    fn claude_non_json_prototypes_hold_but_own_fields_are_data() {
        for path in [
            "tool_input.constructor",
            "tool_input.__proto__",
            "tool_input.toString",
            "items.map",
        ] {
            assert!(
                resolve(
                    &json!({"x":format!("${{{path}}}")}),
                    &json!({"tool_input":{},"items":[]}),
                    HookDialect::Claude
                )
                .is_err()
            );
        }
        assert_eq!(
            resolve(
                &json!({"x":"${tool_input.constructor}","y":"${tool_input.__proto__}"}),
                &json!({"tool_input":{"constructor":7,"__proto__":"own"}}),
                HookDialect::Claude
            )
            .unwrap(),
            json!({"x":"7","y":"own"})
        );
        assert!(resolve(&json!({"__proto__":{}}), &json!({}), HookDialect::Claude).is_err());
    }
}
