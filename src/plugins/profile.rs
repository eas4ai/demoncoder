//! Frozen compatibility profile. Shape validation does not execute hook effects.
use super::{
    hook_types::*,
    wire::{self, ClaudeGraph, SchemaValidator, WireError, WireResult},
};
use serde_json::Value;
use std::collections::BTreeMap;

const EMBEDDED: &str = include_str!("../../docs/spec/compatibility/plugin-profile-v1.json");

/// Codex configuration entries share a path; definition is part of their identity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum SchemaKey {
    Portable(String),
    Codex {
        path: String,
        definition: Option<String>,
    },
}

pub struct CompatibilityProfile {
    schemas: BTreeMap<SchemaKey, SchemaValidator>,
    models: BTreeMap<(HookDialect, HandlerKind), SchemaValidator>,
    applicability: BTreeMap<(HookDialect, HookEvent, HandlerKind), Applicability>,
    graph: ClaudeGraph,
    rules: Value,
}
impl CompatibilityProfile {
    pub fn embedded() -> WireResult<Self> {
        let profile: Value = serde_json::from_str(EMBEDDED)
            .map_err(|_| WireError::new("/profile", "invalid embedded inventory"))?;
        Self::from_inventory(profile)
    }
    // Only trusted embedded inventory reaches this constructor in production.
    fn from_inventory(profile: Value) -> WireResult<Self> {
        if profile["profile"] != "demoncoder-plugin-compatibility-v1"
            || profile["revision"] != 2
            || profile["status"] != "Agreed"
        {
            return Err(WireError::new("/profile", "unsupported profile identity"));
        }
        let mut schemas = BTreeMap::new();
        for entry in profile["source_revisions"]["portable"]
            .as_array()
            .ok_or_else(|| {
                WireError::new("/source_revisions/portable", "missing portable schemas")
            })?
        {
            let key = SchemaKey::Portable(text(&entry["name"])?.into());
            insert_schema(&mut schemas, key, &entry["schema"])?;
        }
        for entry in profile["codex_wire"]
            .as_array()
            .ok_or_else(|| WireError::new("/codex_wire", "missing Codex schemas"))?
        {
            let definition = entry
                .get("definition")
                .map(text)
                .transpose()?
                .map(str::to_owned);
            let key = SchemaKey::Codex {
                path: text(&entry["path"])?.into(),
                definition,
            };
            insert_schema(&mut schemas, key, &entry["schema"])?;
        }
        if schemas.len() != 29 {
            return Err(WireError::new("/schemas", "incomplete schema inventory"));
        }
        let mut applicability = BTreeMap::new();
        for &dialect in HookDialect::ALL {
            for &event in HookEvent::ALL {
                let group = text(
                    &profile["hook_applicability"]["dialects"][dialect.as_str()][event.as_str()],
                )?;
                for &handler in HandlerKind::ALL {
                    let cell = &profile["hook_applicability"]["groups"][group][handler.as_str()];
                    let status: Applicability =
                        serde_json::from_value(cell.clone()).map_err(|_| {
                            WireError::new(
                                "/hook_applicability",
                                "missing or invalid applicability cell",
                            )
                        })?;
                    applicability.insert((dialect, event, handler), status);
                }
            }
        }
        let mut models = BTreeMap::new();
        for dialect in [HookDialect::Native, HookDialect::Claude] {
            for handler in [HandlerKind::Prompt, HandlerKind::Agent] {
                let key = format!("{}-{}", dialect.as_str(), handler.as_str());
                models.insert(
                    (dialect, handler),
                    SchemaValidator::compile(&profile["model_response_schemas"][key])?,
                );
            }
        }
        let rules = profile["model_result_rules"].clone();
        let graph = ClaudeGraph::new(&profile["claude_wire"])?;
        let result = Self {
            schemas,
            models,
            applicability,
            graph,
            rules,
        };
        // Every applicable model cell must have an explicit, understood rule.
        for &dialect in HookDialect::ALL {
            for &event in HookEvent::ALL {
                for handler in [HandlerKind::Prompt, HandlerKind::Agent] {
                    if result.applicability(dialect, event, handler) == Applicability::Run {
                        let rule = &result.rules[dialect.as_str()][event.as_str()];
                        for ok in [true, false] {
                            for impossible in [true, false] {
                                for continue_on_block in [true, false] {
                                    for task_boundary in
                                        [TaskBoundary::ToolTransition, TaskBoundary::TeammateStop]
                                    {
                                        let verdict = ModelVerdict {
                                            ok,
                                            impossible,
                                            reason: None,
                                            dialect,
                                            handler,
                                        };
                                        let context = ModelCallContext {
                                            continue_on_block,
                                            task_boundary,
                                            ..Default::default()
                                        };
                                        result.resolve_rule(rule, &verdict, &context)?;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(result)
    }
    pub fn schema_keys(&self) -> impl Iterator<Item = &SchemaKey> {
        self.schemas.keys()
    }
    pub fn validate_schema(&self, key: &SchemaKey, value: &Value) -> WireResult {
        self.schemas
            .get(key)
            .ok_or_else(|| WireError::new("/schema", "unknown or ambiguous schema key"))?
            .validate(value)
    }
    pub fn validate_schema_bytes(&self, key: &SchemaKey, bytes: &[u8]) -> WireResult<Value> {
        let value = wire::parse_json(bytes)?;
        self.validate_schema(key, &value)?;
        Ok(value)
    }
    pub fn applicability(
        &self,
        dialect: HookDialect,
        event: HookEvent,
        handler: HandlerKind,
    ) -> Applicability {
        // Constructor proves all 510 cells before exposing the immutable profile.
        self.applicability[&(dialect, event, handler)]
    }
    pub fn require_runner(
        &self,
        dialect: HookDialect,
        event: HookEvent,
        handler: HandlerKind,
    ) -> WireResult {
        if self.applicability(dialect, event, handler) == Applicability::Run {
            Ok(())
        } else {
            Err(WireError::new(
                "/hook_applicability",
                "source pair cannot execute; explicit native conversion required",
            ))
        }
    }
    pub fn validate_claude_type(&self, name: &str, value: &Value) -> WireResult {
        self.graph.validate(name, value)
    }
    pub fn validate_claude_input(&self, event: HookEvent, value: &Value) -> WireResult {
        if value["hook_event_name"] != event.as_str() {
            return Err(WireError::new(
                "/hook_event_name",
                "event identity mismatch",
            ));
        }
        self.graph.validate("HookInput", value)
    }
    pub fn validate_claude_output(&self, event: HookEvent, value: &Value) -> WireResult {
        if event == HookEvent::Interrupt {
            return Err(WireError::new(
                "/hookEventName",
                "event absent from Claude source",
            ));
        }
        if let Some(identity) = value.pointer("/hookSpecificOutput/hookEventName")
            && identity != event.as_str()
        {
            return Err(WireError::new(
                "/hookSpecificOutput/hookEventName",
                "event identity mismatch",
            ));
        }
        self.graph.validate("HookJSONOutput", value)
    }
    /// Validate serializable callback arguments alongside the actual control
    /// handle. The host runner owns invocation, deadlines and cancellation.
    /// Its resolved Promise result must pass `validate_claude_output`.
    pub fn validate_callback_input(
        &self,
        event: HookEvent,
        input: &Value,
        tool_use_id: Option<&str>,
        signal: &wire::CallbackSignal,
    ) -> WireResult {
        self.validate_claude_input(event, input)?;
        if let Some(id) = tool_use_id {
            wire::measure_string(id)?;
        }
        if signal.is_cancelled() {
            return Err(WireError::new(
                "/callback/options/signal",
                "callback cancelled",
            ));
        }
        Ok(())
    }
    pub fn validate_callback_matcher(&self, matcher: &wire::CallbackMatcher) -> WireResult {
        if matcher.hooks.len() > wire::MAX_WIRE_NODES
            || matcher.timeout.is_some_and(|v| !v.is_finite())
        {
            return Err(WireError::new(
                "/HookCallbackMatcher",
                "invalid callback matcher",
            ));
        }
        if let Some(matcher) = &matcher.matcher {
            wire::measure_string(matcher)?;
        }
        Ok(())
    }
    pub fn validate_model(
        &self,
        dialect: HookDialect,
        handler: HandlerKind,
        value: &Value,
    ) -> WireResult<ModelVerdict> {
        self.models
            .get(&(dialect, handler))
            .ok_or_else(|| {
                WireError::new("/model_response_schemas", "no source model response schema")
            })?
            .validate(value)?;
        Ok(ModelVerdict {
            ok: value["ok"]
                .as_bool()
                .ok_or_else(|| WireError::new("/ok", "missing verdict"))?,
            reason: value["reason"].as_str().map(str::to_owned),
            impossible: value["impossible"].as_bool().unwrap_or(false),
            dialect,
            handler,
        })
    }
    pub fn validate_model_bytes(
        &self,
        dialect: HookDialect,
        handler: HandlerKind,
        bytes: &[u8],
    ) -> WireResult<ModelVerdict> {
        self.validate_model(dialect, handler, &wire::parse_json(bytes)?)
    }
    pub fn model_can_gate(
        &self,
        dialect: HookDialect,
        event: HookEvent,
        handler: HandlerKind,
    ) -> WireResult<bool> {
        self.require_runner(dialect, event, handler)?;
        if !matches!(handler, HandlerKind::Prompt | HandlerKind::Agent) {
            return Err(WireError::new("/handler", "expected a model handler"));
        }
        let rule = &self.rules[dialect.as_str()][event.as_str()];
        Ok(rule["ok_true"] != "no-source-decision"
            && rule["ok_false"] != "attributed-observation-only")
    }
    pub fn model_outcome(
        &self,
        event: HookEvent,
        verdict: &ModelVerdict,
        context: &ModelCallContext,
    ) -> WireResult<ModelOutcome> {
        self.require_runner(verdict.dialect, event, verdict.handler)?;
        if context.continue_on_block
            && (verdict.handler != HandlerKind::Prompt || verdict.dialect != HookDialect::Claude)
        {
            return Err(WireError::new(
                "/configuration/continueOnBlock",
                "continuation setting is Claude prompt configuration only",
            ));
        }
        if !verdict.ok
            && verdict.dialect == HookDialect::Native
            && verdict.handler == HandlerKind::Prompt
            && verdict.impossible
            && matches!(event, HookEvent::Stop | HookEvent::SubagentStop)
        {
            return Ok(ModelOutcome::StopUnmet);
        }
        let outcome = self.resolve_rule(
            &self.rules[verdict.dialect.as_str()][event.as_str()],
            verdict,
            context,
        )?;
        let needs_followup = matches!(
            outcome,
            ModelOutcome::ContinueAfterResult
                | ModelOutcome::ContinueWithFailure
                | ModelOutcome::DenyToolAndContinue
                | ModelOutcome::BoundedCorrection
                | ModelOutcome::RejectAndContinue
                | ModelOutcome::KeepWorking
        );
        if needs_followup
            && (context.cancelled || !context.allocation_available || !context.correction_available)
        {
            Ok(ModelOutcome::StopUnmet)
        } else {
            Ok(outcome)
        }
    }
    fn resolve_rule(
        &self,
        rule: &Value,
        verdict: &ModelVerdict,
        context: &ModelCallContext,
    ) -> WireResult<ModelOutcome> {
        let mut selected = if verdict.ok {
            &rule["ok_true"]
        } else {
            &rule["ok_false"]
        };
        if !verdict.ok && verdict.dialect == HookDialect::Claude {
            selected = &selected[verdict.handler.as_str()];
        }
        for _ in 0..8 {
            if let Some(name) = selected.as_str() {
                return outcome(name);
            }
            let condition = match selected["when"].as_str() {
                Some("continueOnBlock") => context.continue_on_block,
                Some("impossible") => verdict.impossible,
                Some("teammate-stop") => context.task_boundary == TaskBoundary::TeammateStop,
                _ => {
                    return Err(WireError::new(
                        "/model_result_rules",
                        "missing or unknown model outcome condition",
                    ));
                }
            };
            selected = &selected[if condition { "then" } else { "else" }];
        }
        Err(WireError::new(
            "/model_result_rules",
            "model rule depth exceeded",
        ))
    }
}
fn text(value: &Value) -> WireResult<&str> {
    value
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| WireError::new("/profile", "missing or invalid inventory key"))
}
fn insert_schema(
    schemas: &mut BTreeMap<SchemaKey, SchemaValidator>,
    key: SchemaKey,
    schema: &Value,
) -> WireResult {
    if schemas
        .insert(key, SchemaValidator::compile(schema)?)
        .is_some()
    {
        return Err(WireError::new("/schemas", "duplicate schema identity"));
    }
    Ok(())
}
fn outcome(name: &str) -> WireResult<ModelOutcome> {
    use ModelOutcome::*;
    Ok(match name {
        "no-model-objection; host admission still required" => NoModelObjection,
        "no-source-decision" => NoSourceDecision,
        "end-turn-unmet" => EndTurnUnmet,
        "continue-after-result" => ContinueAfterResult,
        "continue-with-failure" => ContinueWithFailure,
        "deny-tool-and-continue" => DenyToolAndContinue,
        "deny-tool-and-end-turn" => DenyToolAndEndTurn,
        "stop-unmet" => StopUnmet,
        "bounded-correction" | "bounded-correction-or-stop-unmet" => BoundedCorrection,
        "reject-and-continue" => RejectAndContinue,
        "keep-working" => KeepWorking,
        "hold-pending-action" => HoldPendingAction,
        "attributed-observation-only" => AttributedObservationOnly,
        _ => {
            return Err(WireError::new(
                "/model_result_rules",
                "unknown model outcome",
            ));
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn inventory() -> Value {
        serde_json::from_str(EMBEDDED).unwrap()
    }
    #[test]
    fn absent_cell_duplicate_identity_and_external_references_fail_closed() {
        let mut p = inventory();
        p["hook_applicability"]["groups"]["native"]
            .as_object_mut()
            .unwrap()
            .remove("agent");
        assert!(CompatibilityProfile::from_inventory(p).is_err());
        for dialect in HookDialect::ALL {
            for event in HookEvent::ALL {
                let mut p = inventory();
                p["hook_applicability"]["dialects"][dialect.as_str()]
                    .as_object_mut()
                    .unwrap()
                    .remove(event.as_str());
                assert!(CompatibilityProfile::from_inventory(p).is_err());
            }
        }
        let mut p = inventory();
        let duplicate = p["codex_wire"][0].clone();
        p["codex_wire"].as_array_mut().unwrap().push(duplicate);
        assert!(CompatibilityProfile::from_inventory(p).is_err());
        for reference in [
            "https://example.test/schema",
            "file:///etc/passwd",
            "#/missing",
        ] {
            let mut p = inventory();
            p["codex_wire"][0]["schema"]["$ref"] = json!(reference);
            assert!(CompatibilityProfile::from_inventory(p).is_err());
        }
    }
    #[test]
    fn deleting_nested_field_or_union_branch_breaks_unchanged_production_fixture() {
        let fixtures: Vec<Value> = serde_json::from_str(include_str!(
            "../../tests/fixtures/plugin-wire/claude-graph.json"
        ))
        .unwrap();
        let input = fixtures
            .iter()
            .find(|f| f["type"] == "BaseHookInput" && f["value"]["effort"].is_object())
            .unwrap();
        let mut profile = inventory();
        let p = CompatibilityProfile::from_inventory(profile.clone()).unwrap();
        p.validate_claude_type("BaseHookInput", &input["value"])
            .unwrap();
        profile["claude_wire"]["types"]["BaseHookInput"]["properties"]["effort"]["type"]["properties"].as_object_mut().unwrap().remove("level");
        let mutated = CompatibilityProfile::from_inventory(profile).unwrap();
        assert!(
            mutated
                .validate_claude_type("BaseHookInput", &input["value"])
                .is_err()
        );
        let mut profile = inventory();
        let branches = profile["claude_wire"]["types"]["ExitReason"]["branches"]
            .as_object_mut()
            .unwrap();
        let branch = branches
            .iter()
            .find(|(_, v)| v["value"] == "logout")
            .unwrap()
            .0
            .clone();
        branches.remove(&branch);
        let mutated = CompatibilityProfile::from_inventory(profile).unwrap();
        p.validate_claude_type("ExitReason", &json!("logout"))
            .unwrap();
        assert!(
            mutated
                .validate_claude_type("ExitReason", &json!("logout"))
                .is_err()
        );
    }

    fn schema_for_key<'a>(profile: &'a Value, key: &Value) -> &'a Value {
        let entries = if key.get("portable").is_some() {
            profile["source_revisions"]["portable"].as_array().unwrap()
        } else {
            profile["codex_wire"].as_array().unwrap()
        };
        &entries
            .iter()
            .find(|entry| {
                if key.get("portable").is_some() {
                    entry["name"] == key["portable"]
                } else {
                    entry["path"] == key["codex"] && entry["definition"] == key["definition"]
                }
            })
            .expect("exact frozen schema identity")["schema"]
    }
    fn schema_key(value: &Value) -> SchemaKey {
        if let Some(name) = value["portable"].as_str() {
            SchemaKey::Portable(name.into())
        } else {
            SchemaKey::Codex {
                path: value["codex"].as_str().unwrap().into(),
                definition: value["definition"].as_str().map(str::to_owned),
            }
        }
    }
    fn escaped(value: &str) -> String {
        value.replace('~', "~0").replace('/', "~1")
    }

    /// Enumerate the retained schema's reachable vocabulary independently of the
    /// frozen witnesses. Unknown keywords or missing identities fail this test.
    fn schema_obligations(schema: &Value) -> std::collections::BTreeSet<String> {
        fn walk(
            root: &Value,
            node: &Value,
            path: &str,
            field: bool,
            seen: &mut std::collections::BTreeSet<String>,
            out: &mut std::collections::BTreeSet<String>,
        ) {
            if field {
                out.insert(format!("{path}::field"));
            }
            if !seen.insert(path.into()) {
                return;
            }
            let Some(object) = node.as_object() else {
                out.insert(format!("{path}::unconstrained"));
                return;
            };
            if ![
                "type",
                "$ref",
                "enum",
                "const",
                "oneOf",
                "allOf",
                "not",
                "properties",
                "items",
            ]
            .iter()
            .any(|keyword| object.contains_key(*keyword))
            {
                out.insert(format!("{path}::unconstrained"));
            }
            for (keyword, value) in object {
                match keyword.as_str() {
                    "$schema" | "$id" | "title" | "description" | "default" | "definitions"
                    | "$defs" => {}
                    "properties" => {
                        for (name, property) in value.as_object().unwrap() {
                            walk(
                                root,
                                property,
                                &format!("{path}/properties/{}", escaped(name)),
                                true,
                                seen,
                                out,
                            );
                        }
                    }
                    "required" => {
                        for (i, _) in value.as_array().unwrap().iter().enumerate() {
                            out.insert(format!("{path}/required/{i}::required"));
                        }
                    }
                    "$ref" => {
                        out.insert(format!("{path}::$ref"));
                        let pointer = value
                            .as_str()
                            .unwrap()
                            .strip_prefix('#')
                            .expect("local reference");
                        walk(
                            root,
                            root.pointer(pointer).unwrap(),
                            pointer,
                            false,
                            seen,
                            out,
                        );
                    }
                    "type" => {
                        out.insert(format!("{path}::type"));
                        if let Some(types) = value.as_array() {
                            for i in 0..types.len() {
                                out.insert(format!("{path}/type/{i}::type-alternative"));
                            }
                        }
                    }
                    "enum" => {
                        out.insert(format!("{path}::enum"));
                        for i in 0..value.as_array().unwrap().len() {
                            out.insert(format!("{path}/enum/{i}::alternative"));
                        }
                    }
                    "oneOf" | "allOf" => {
                        out.insert(format!("{path}::{keyword}"));
                        if keyword == "oneOf" {
                            out.insert(format!("{path}::oneOf-exclusivity"));
                        }
                        for (i, branch) in value.as_array().unwrap().iter().enumerate() {
                            let pointer = format!("{path}/{keyword}/{i}");
                            out.insert(format!("{pointer}::branch"));
                            walk(root, branch, &pointer, false, seen, out);
                        }
                    }
                    "items" | "propertyNames" | "not" => {
                        out.insert(format!("{path}::{keyword}"));
                        walk(root, value, &format!("{path}/{keyword}"), false, seen, out);
                    }
                    "additionalProperties" => {
                        out.insert(format!("{path}::{keyword}"));
                        if value.is_object() {
                            walk(root, value, &format!("{path}/{keyword}"), false, seen, out);
                        }
                    }
                    "const" | "minimum" | "minLength" | "maxLength" | "pattern" | "format" => {
                        out.insert(format!("{path}::{keyword}"));
                    }
                    other => panic!("new schema vocabulary needs probes: {path}/{other}"),
                }
            }
        }
        let mut result = std::collections::BTreeSet::new();
        walk(
            schema,
            schema,
            "",
            false,
            &mut std::collections::BTreeSet::new(),
            &mut result,
        );
        result
    }

    fn apply_probe(value: &Value, patch: &Value) -> Value {
        let mut value = value.clone();
        let pointer = patch["path"].as_str().unwrap();
        let replacement = if patch["op"] == "bound" {
            Value::String("x".repeat(wire::MAX_WIRE_BYTES + 1))
        } else {
            patch["value"].clone()
        };
        if pointer.is_empty() {
            assert_eq!(patch["op"], "set");
            return replacement;
        }
        let (parent, key) = pointer.rsplit_once('/').unwrap();
        let key = key.replace("~1", "/").replace("~0", "~");
        let parent = value.pointer_mut(parent).expect("frozen patch location");
        match patch["op"].as_str().unwrap() {
            "remove" => {
                if let Some(array) = parent.as_array_mut() {
                    array.remove(key.parse::<usize>().unwrap());
                } else {
                    assert!(parent.as_object_mut().unwrap().remove(&key).is_some());
                }
            }
            "append" => {
                parent
                    .as_object_mut()
                    .unwrap()
                    .get_mut(&key)
                    .unwrap()
                    .as_array_mut()
                    .unwrap()
                    .push(replacement);
            }
            "rename" => {
                let object = parent.as_object_mut().unwrap();
                let entry = object.remove(&key).unwrap();
                object.insert(replacement.as_str().unwrap().into(), entry);
            }
            "set" | "bound" => {
                if let Some(array) = parent.as_array_mut() {
                    array[key.parse::<usize>().unwrap()] = replacement;
                } else {
                    parent.as_object_mut().unwrap().insert(key, replacement);
                }
            }
            other => panic!("unknown probe operation {other}"),
        }
        value
    }

    #[test]
    fn frozen_full_schema_negatives_and_mutations_cover_every_reachable_constraint() {
        let inventory = inventory();
        let profile = CompatibilityProfile::embedded().unwrap();
        let fixtures: Vec<Value> = serde_json::from_str(include_str!(
            "../../tests/fixtures/plugin-wire/full-schemas.json"
        ))
        .unwrap();
        let groups: Vec<Value> = serde_json::from_str(include_str!(
            "../../tests/fixtures/plugin-wire/full-schema-probes.json"
        ))
        .unwrap();
        let mut seen = std::collections::BTreeSet::new();
        for group in groups {
            let key = schema_key(&group["key"]);
            assert!(seen.insert(key.clone()), "duplicate schema group");
            let schema = schema_for_key(&inventory, &group["key"]);
            let probes = group["probes"].as_array().unwrap();
            let actual: std::collections::BTreeSet<String> = probes
                .iter()
                .map(|p| p["id"].as_str().unwrap().into())
                .collect();
            assert_eq!(
                actual.len(),
                probes.len(),
                "duplicate probe identity {key:?}"
            );
            assert_eq!(
                actual,
                schema_obligations(schema),
                "uncovered retained schema constraint {key:?}"
            );
            for probe in probes {
                let id = probe["id"].as_str().unwrap();
                let fixture = &fixtures[probe["fixture"].as_u64().unwrap() as usize];
                assert_eq!(
                    fixture["key"], group["key"],
                    "probe uses a different schema identity"
                );
                let baseline = if probe.get("valid").is_some() {
                    apply_probe(&fixture["value"], &probe["valid"])
                } else {
                    fixture["value"].clone()
                };
                profile
                    .validate_schema(&key, &baseline)
                    .unwrap_or_else(|e| panic!("positive {key:?} {id}: {e}"));
                let negative = probe
                    .get("invalid")
                    .map(|change| apply_probe(&baseline, change));
                if let Some(negative) = &negative {
                    assert!(
                        profile.validate_schema(&key, negative).is_err(),
                        "accepted frozen negative {key:?} {id}"
                    );
                }
                let changed = SchemaValidator::compile(&apply_probe(schema, &probe["mutation"]))
                    .unwrap_or_else(|e| panic!("invalid mutation {key:?} {id}: {e}"));
                match probe["effect"].as_str().unwrap() {
                    "negative-accepted" => assert!(
                        changed
                            .validate(negative.as_ref().expect("negative witness"))
                            .is_ok(),
                        "constraint deletion did not admit frozen negative {key:?} {id}"
                    ),
                    "positive-rejected" => assert!(
                        changed.validate(&baseline).is_err(),
                        "constraint/branch mutation survived positive {key:?} {id}"
                    ),
                    "annotation-no-effect" => {
                        let pointer = id.strip_suffix("::format").unwrap();
                        assert!(
                            matches!(
                                schema.pointer(pointer).unwrap()["format"].as_str(),
                                Some("uint" | "uint64")
                            ),
                            "new format semantics need dedicated probes"
                        );
                        assert!(changed.validate(&baseline).is_ok());
                    }
                    other => panic!("unknown mutation effect {other}"),
                }
            }
        }
        assert_eq!(
            seen,
            profile.schema_keys().cloned().collect(),
            "missing full-schema probe group"
        );
    }
}
