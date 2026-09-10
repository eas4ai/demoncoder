//! Bounded wire validation. Diagnostics contain schema paths, never payload values.
use serde_json::Value;
use std::collections::BTreeSet;

pub const MAX_WIRE_BYTES: usize = 1024 * 1024;
pub const MAX_WIRE_DEPTH: usize = 48;
pub const MAX_WIRE_NODES: usize = 16_384;
pub const MAX_VALIDATION_WORK: usize = 8_000_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireError {
    pub path: String,
    pub problem: &'static str,
}
impl WireError {
    pub(crate) fn new(path: impl Into<String>, problem: &'static str) -> Self {
        Self {
            path: path.into(),
            problem,
        }
    }
}
impl std::fmt::Display for WireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.path, self.problem)
    }
}
impl std::error::Error for WireError {}
pub type WireResult<T = ()> = Result<T, WireError>;

/// Parse only bounded JSON. Duplicate object keys are rejected rather than overwritten.
pub fn parse_json(bytes: &[u8]) -> WireResult<Value> {
    if bytes.len() > MAX_WIRE_BYTES {
        return Err(WireError::new("/", "wire byte limit exceeded"));
    }
    let mut de = serde_json::Deserializer::from_slice(bytes);
    let value = <super::manifest::UniqueJson as serde::Deserialize>::deserialize(&mut de)
        .map_err(|_| WireError::new("/", "invalid JSON or duplicate object key"))?
        .0;
    de.end()
        .map_err(|_| WireError::new("/", "trailing JSON data"))?;
    measure(&value)?;
    Ok(value)
}
const NODE_BYTES: usize = 8;

fn string_bytes(length: usize) -> usize {
    length.saturating_mul(6)
}

/// Apply the same conservative bound as a string Value without copying its data.
pub(crate) fn measure_string(value: &str) -> WireResult {
    if NODE_BYTES.saturating_add(string_bytes(value.len())) > MAX_WIRE_BYTES {
        return Err(WireError::new("/", "wire byte limit exceeded"));
    }
    Ok(())
}

/// No serialization allocation is needed to bound a caller-provided Value.
pub(crate) fn measure(value: &Value) -> WireResult<usize> {
    fn walk(v: &Value, depth: usize, nodes: &mut usize, bytes: &mut usize) -> WireResult {
        *nodes += 1;
        *bytes = bytes.saturating_add(NODE_BYTES);
        if depth > MAX_WIRE_DEPTH || *nodes > MAX_WIRE_NODES || *bytes > MAX_WIRE_BYTES {
            return Err(WireError::new(
                "/",
                "wire depth, node or byte limit exceeded",
            ));
        }
        match v {
            Value::String(s) => *bytes = bytes.saturating_add(string_bytes(s.len())),
            Value::Array(a) => {
                for v in a {
                    walk(v, depth + 1, nodes, bytes)?;
                }
            }
            Value::Object(o) => {
                for (k, v) in o {
                    *bytes = bytes.saturating_add(string_bytes(k.len()));
                    walk(v, depth + 1, nodes, bytes)?;
                }
            }
            _ => {}
        }
        if *bytes > MAX_WIRE_BYTES {
            return Err(WireError::new("/", "wire byte limit exceeded"));
        }
        Ok(())
    }
    let mut nodes = 0;
    walk(value, 0, &mut nodes, &mut 0)?;
    Ok(nodes)
}

pub(crate) struct SchemaValidator {
    validator: jsonschema::Validator,
    cost: usize,
}
impl SchemaValidator {
    pub(crate) fn compile(schema: &Value) -> WireResult<Self> {
        // The embedded schemas have finite local reference closures. Refuse external
        // references and cycles before compilation; no untrusted schema is retrieved.
        fn cost(
            v: &Value,
            root: &Value,
            stack: &mut BTreeSet<String>,
            depth: usize,
        ) -> WireResult<usize> {
            if depth > 96 {
                return Err(WireError::new("/schema", "schema expansion limit"));
            }
            let mut total = 1usize;
            match v {
                Value::Object(o) => {
                    for key in ["$ref", "$dynamicRef", "$recursiveRef"] {
                        if let Some(reference) = o.get(key) {
                            let r = reference.as_str().ok_or_else(|| {
                                WireError::new("/schema", "invalid schema reference")
                            })?;
                            if !r.starts_with("#/") || !stack.insert(r.into()) {
                                return Err(WireError::new(
                                    "/schema",
                                    "external or cyclic schema reference",
                                ));
                            }
                            let target = root.pointer(&r[1..]).ok_or_else(|| {
                                WireError::new("/schema", "missing schema reference")
                            })?;
                            total = total.saturating_add(cost(target, root, stack, depth + 1)?);
                            stack.remove(r);
                        }
                    }
                    for (k, v) in o {
                        if !["$ref", "$dynamicRef", "$recursiveRef"].contains(&k.as_str()) {
                            total = total.saturating_add(cost(v, root, stack, depth + 1)?);
                        }
                    }
                }
                Value::Array(a) => {
                    for v in a {
                        total = total.saturating_add(cost(v, root, stack, depth + 1)?);
                    }
                }
                _ => {}
            }
            if total > MAX_VALIDATION_WORK {
                return Err(WireError::new("/schema", "schema expansion limit"));
            }
            Ok(total)
        }
        let cost = cost(schema, schema, &mut BTreeSet::new(), 0)?;
        let validator = jsonschema::options()
            .offline()
            .with_pattern_options(jsonschema::PatternOptions::fancy_regex().backtrack_limit(10_000))
            .build(schema)
            .map_err(|_| WireError::new("/schema", "invalid embedded schema"))?;
        Ok(Self { validator, cost })
    }
    pub(crate) fn validate(&self, value: &Value) -> WireResult {
        let nodes = measure(value)?;
        // Fixed finite closure multiplied by instance size, plus quadratic array
        // comparisons (uniqueItems), bounds work before entering the library.
        if self
            .cost
            .saturating_mul(nodes)
            .saturating_add(nodes.saturating_mul(nodes))
            > MAX_VALIDATION_WORK
        {
            return Err(WireError::new("/", "schema validation work limit exceeded"));
        }
        self.validator.validate(value).map_err(|e| {
            WireError::new(e.schema_path().to_string(), "wire schema constraint failed")
        })
    }
}

/// SDK cancellation is an owned control handle, not JSON supplied by a plugin.
#[derive(Clone, Default)]
pub struct CallbackSignal(std::sync::Arc<std::sync::atomic::AtomicBool>);
impl CallbackSignal {
    pub fn cancel(&self) {
        self.0.store(true, std::sync::atomic::Ordering::Release);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::Acquire)
    }
}

/// Actual callback signature. Input/result are validated at invocation, while
/// toolUseID undefined and AbortSignal retain their native meanings.
pub trait SdkHookCallback: Send + Sync {
    fn call<'a>(
        &'a self,
        input: Value,
        tool_use_id: Option<String>,
        signal: CallbackSignal,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = WireResult<Value>> + Send + 'a>>;
}
pub struct CallbackMatcher {
    pub matcher: Option<String>,
    pub hooks: Vec<std::sync::Arc<dyn SdkHookCallback>>,
    pub timeout: Option<f64>,
}

pub(crate) struct ClaudeGraph {
    types: serde_json::Map<String, Value>,
}
impl ClaudeGraph {
    pub(crate) fn new(graph: &Value) -> WireResult<Self> {
        let types = graph["types"]
            .as_object()
            .ok_or_else(|| WireError::new("/claude_wire/types", "missing graph types"))?
            .clone();
        // Validate every descriptor, including callback-only paths.
        fn inspect(v: &Value, types: &serde_json::Map<String, Value>) -> WireResult {
            let kind = v["kind"]
                .as_str()
                .ok_or_else(|| WireError::new("/claude_wire", "missing graph kind"))?;
            match kind {
                "object" => {
                    for p in v["properties"]
                        .as_object()
                        .ok_or_else(|| WireError::new("/claude_wire", "missing properties"))?
                        .values()
                    {
                        if !p["required"].is_boolean() {
                            return Err(WireError::new("/claude_wire", "invalid required flag"));
                        }
                        inspect(&p["type"], types)?;
                    }
                }
                "union" | "intersection" => {
                    let branches = v["branches"]
                        .as_object()
                        .filter(|b| !b.is_empty())
                        .ok_or_else(|| WireError::new("/claude_wire", "missing branches"))?;
                    for b in branches.values() {
                        inspect(b, types)?;
                    }
                }
                "reference" => {
                    let name = v["name"]
                        .as_str()
                        .ok_or_else(|| WireError::new("/claude_wire", "missing reference name"))?;
                    let args = v["arguments"].as_array().ok_or_else(|| {
                        WireError::new("/claude_wire", "missing reference arguments")
                    })?;
                    let arity = match name {
                        "Record" => 2,
                        "Promise" => 1,
                        "UUID" | "AbortSignal" => 0,
                        _ if types.contains_key(name) => 0,
                        _ => {
                            return Err(WireError::new(
                                "/claude_wire",
                                "unresolved graph reference",
                            ));
                        }
                    };
                    if args.len() != arity {
                        return Err(WireError::new("/claude_wire", "reference arity mismatch"));
                    }
                    for arg in args {
                        inspect(arg, types)?;
                    }
                }
                "array" => inspect(&v["element"], types)?,
                "function" => {
                    for p in v["parameters"].as_array().ok_or_else(|| {
                        WireError::new("/claude_wire", "missing callback parameters")
                    })? {
                        inspect(&p["type"], types)?;
                    }
                    inspect(&v["result"], types)?;
                }
                "literal" if v.get("value").is_some() => {}
                "unknown" | "undefined" | "string" | "number" | "boolean" => {}
                _ => return Err(WireError::new("/claude_wire", "unknown graph kind")),
            }
            Ok(())
        }
        for v in types.values() {
            inspect(v, &types)?;
        }
        for root in graph["roots"]
            .as_array()
            .ok_or_else(|| WireError::new("/claude_wire", "missing roots"))?
        {
            if !root.as_str().is_some_and(|r| types.contains_key(r)) {
                return Err(WireError::new("/claude_wire", "unresolved graph root"));
            }
        }
        validate_callback_contract(&types)?;
        Ok(Self { types })
    }
    pub(crate) fn validate(&self, name: &str, value: &Value) -> WireResult {
        measure(value)?;
        let node = self
            .types
            .get(name)
            .ok_or_else(|| WireError::new("/claude_wire/types", "unknown graph type"))?;
        let mut work = MAX_VALIDATION_WORK;
        self.closed(
            node,
            value,
            &format!("/claude_wire/types/{name}"),
            &mut work,
            0,
        )
    }
    fn closed(
        &self,
        node: &Value,
        value: &Value,
        path: &str,
        work: &mut usize,
        depth: usize,
    ) -> WireResult {
        let allowed = self.check(node, value, path, work, depth)?;
        if let (Some(allowed), Some(object)) = (allowed, value.as_object())
            && object.keys().any(|k| !allowed.contains(k))
        {
            return Err(WireError::new(path, "unknown JSON field"));
        }
        Ok(())
    }
    // None means bounded unknown/Record; Some contains all evaluated object keys.
    fn check(
        &self,
        node: &Value,
        value: &Value,
        path: &str,
        work: &mut usize,
        depth: usize,
    ) -> WireResult<Option<BTreeSet<String>>> {
        if *work == 0 || depth > 96 {
            return Err(WireError::new(path, "graph validation work limit exceeded"));
        }
        *work -= 1;
        let error = || WireError::new(path, "wire type constraint failed");
        match node["kind"].as_str().unwrap_or("") {
            "unknown" => Ok(None),
            "string" if value.is_string() => Ok(None),
            "number" if value.is_number() => Ok(None),
            "boolean" if value.is_boolean() => Ok(None),
            "literal" if node["value"] == *value => Ok(None),
            "object" => {
                let object = value.as_object().ok_or_else(error)?;
                let mut allowed = BTreeSet::new();
                for (key, prop) in node["properties"].as_object().ok_or_else(error)? {
                    allowed.insert(key.clone());
                    let field_path = format!("{path}/properties/{key}");
                    if let Some(v) = object.get(key) {
                        self.closed(&prop["type"], v, &field_path, work, depth + 1)?;
                    } else if prop["required"] == true {
                        return Err(WireError::new(field_path, "required field missing"));
                    }
                }
                Ok(Some(allowed))
            }
            "array" => {
                for v in value.as_array().ok_or_else(error)? {
                    self.closed(
                        &node["element"],
                        v,
                        &format!("{path}/element"),
                        work,
                        depth + 1,
                    )?;
                }
                Ok(None)
            }
            "intersection" => {
                let mut allowed = BTreeSet::new();
                let mut open = false;
                for (id, b) in node["branches"].as_object().ok_or_else(error)? {
                    match self.check(b, value, &format!("{path}/branches/{id}"), work, depth + 1)? {
                        Some(keys) => allowed.extend(keys),
                        None => open = true,
                    }
                }
                Ok(if open { None } else { Some(allowed) })
            }
            "union" => {
                for (id, b) in node["branches"].as_object().ok_or_else(error)? {
                    if let Ok(allowed) =
                        self.check(b, value, &format!("{path}/branches/{id}"), work, depth + 1)
                        && value
                            .as_object()
                            .zip(allowed.as_ref())
                            .is_none_or(|(o, a)| o.keys().all(|k| a.contains(k)))
                    {
                        return Ok(allowed);
                    }
                }
                Err(error())
            }
            "reference" => {
                let name = node["name"].as_str().ok_or_else(error)?;
                match name {
                    "UUID" if value.is_string() => Ok(None),
                    "Promise" => self.check(&node["arguments"][0], value, path, work, depth + 1),
                    "Record" => {
                        for (k, v) in value.as_object().ok_or_else(error)? {
                            self.closed(
                                &node["arguments"][0],
                                &Value::String(k.clone()),
                                &format!("{path}/arguments/0"),
                                work,
                                depth + 1,
                            )?;
                            self.closed(
                                &node["arguments"][1],
                                v,
                                &format!("{path}/arguments/1"),
                                work,
                                depth + 1,
                            )?;
                        }
                        Ok(None)
                    }
                    "AbortSignal" => Err(WireError::new(path, "SDK control handle is not JSON")),
                    _ => self.check(
                        self.types.get(name).ok_or_else(error)?,
                        value,
                        &format!("/claude_wire/types/{name}"),
                        work,
                        depth + 1,
                    ),
                }
            }
            "function" => Err(WireError::new(path, "SDK callback is not JSON")),
            "undefined" => Err(WireError::new(path, "undefined is not JSON null")),
            _ => Err(error()),
        }
    }
}

// This native signature is checked against the frozen descriptors at profile
// construction. A changed parameter/control property cannot silently disappear
// merely because it is outside JSON serialization.
fn validate_callback_contract(types: &serde_json::Map<String, Value>) -> WireResult {
    use serde_json::json;
    let reference = |name: &str| json!({"kind":"reference","name":name,"arguments":[]});
    let callback = types.get("HookCallback").ok_or_else(|| {
        WireError::new("/claude_wire/types/HookCallback", "missing callback type")
    })?;
    let parameters = callback["parameters"].as_array().ok_or_else(|| {
        WireError::new(
            "/claude_wire/types/HookCallback",
            "missing callback signature",
        )
    })?;
    let mismatch = || {
        WireError::new(
            "/claude_wire/types/HookCallback",
            "SDK callback signature differs from native binding",
        )
    };
    if callback["kind"] != "function" || parameters.len() != 3 {
        return Err(mismatch());
    }
    for (parameter, name) in parameters.iter().zip(["input", "toolUseID", "options"]) {
        if parameter["name"] != name || parameter["required"] != true || parameter["rest"] != false
        {
            return Err(mismatch());
        }
    }
    if parameters[0]["type"] != reference("HookInput") {
        return Err(mismatch());
    }
    let id = &parameters[1]["type"];
    let alternatives = id["branches"].as_object().ok_or_else(mismatch)?;
    if id["kind"] != "union"
        || alternatives.len() != 2
        || !["string", "undefined"]
            .iter()
            .all(|kind| alternatives.values().any(|v| v == &json!({"kind":kind})))
    {
        return Err(mismatch());
    }
    if parameters[2]["type"]
        != json!({"kind":"object","properties":{"signal":{"required":true,"type":reference("AbortSignal")}}})
    {
        return Err(mismatch());
    }
    if callback["result"]
        != json!({"kind":"reference","name":"Promise","arguments":[reference("HookJSONOutput")]})
    {
        return Err(mismatch());
    }
    let expected = json!({"kind":"object","properties":{
        "matcher":{"required":false,"type":{"kind":"string"}},
        "hooks":{"required":true,"type":{"kind":"array","element":reference("HookCallback")}},
        "timeout":{"required":false,"type":{"kind":"number"}}
    }});
    if types.get("HookCallbackMatcher") != Some(&expected) {
        return Err(WireError::new(
            "/claude_wire/types/HookCallbackMatcher",
            "SDK callback matcher differs from native binding",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn graph() -> Value {
        let profile: Value = serde_json::from_str(include_str!(
            "../../docs/spec/compatibility/plugin-profile-v1.json"
        ))
        .unwrap();
        profile["claude_wire"].clone()
    }
    fn root_fixtures(graph: &Value) -> Vec<Value> {
        let fixtures: Vec<Value> = serde_json::from_str(include_str!(
            "../../tests/fixtures/plugin-wire/claude-graph.json"
        ))
        .unwrap();
        fixtures
            .into_iter()
            .filter(|f| graph["roots"].as_array().unwrap().contains(&f["type"]))
            .collect()
    }
    fn killed_by_roots(graph: &Value, fixtures: &[Value]) -> bool {
        match ClaudeGraph::new(graph) {
            Err(_) => true, // Actual native callback signature binding rejects drift.
            Ok(validator) => fixtures.iter().any(|f| {
                validator
                    .validate(f["type"].as_str().unwrap(), &f["value"])
                    .is_err()
            }),
        }
    }
    #[test]
    fn borrowed_string_check_keeps_value_accounting_at_the_boundary() {
        let boundary = (MAX_WIRE_BYTES - 8) / 6;
        let text = "s".repeat(boundary + 1);
        for value in ["", &text[..boundary], text.as_str()] {
            assert_eq!(
                measure_string(value),
                measure(&Value::String(value.into())).map(|_| ())
            );
        }
        assert!(measure_string(&text[..boundary]).is_ok());
        let error = measure_string(&text).unwrap_err();
        assert_eq!(error, WireError::new("/", "wire byte limit exceeded"));
        assert_eq!(string_bytes(usize::MAX), usize::MAX);
    }

    #[test]
    fn every_field_constraint_is_enforced_through_an_event_or_control_root() {
        let baseline = graph();
        let fixtures = root_fixtures(&baseline);
        let validator = ClaudeGraph::new(&baseline).unwrap();
        for fixture in &fixtures {
            validator
                .validate(fixture["type"].as_str().unwrap(), &fixture["value"])
                .unwrap();
        }
        for field in baseline["field_paths"].as_array().unwrap() {
            let path = field["path"].as_str().unwrap().strip_prefix('#').unwrap();
            let (parent, key) = path.rsplit_once('/').unwrap();
            let mut mutated = baseline.clone();
            mutated
                .pointer_mut(parent)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .remove(key)
                .unwrap();
            if !killed_by_roots(&mutated, &fixtures) {
                // A duplicate inherited property has no observable deletion
                // effect. A conflicting constraint proves that occurrence is
                // evaluated without inventing a hash/count runtime contract.
                let mut conflicting = baseline.clone();
                conflicting.pointer_mut(path).unwrap()["type"] =
                    serde_json::json!({"kind":"literal","value":"conflicting-declaration-probe"});
                assert!(
                    killed_by_roots(&conflicting, &fixtures),
                    "field constraint ignored through roots: {path}"
                );
            }
        }
    }
    #[test]
    fn every_union_branch_deletion_is_detected_through_event_or_control_roots() {
        fn branches(value: &Value, path: &str, result: &mut Vec<String>) {
            match value {
                Value::Object(o) => {
                    if value["kind"] == "union" {
                        for key in value["branches"].as_object().unwrap().keys() {
                            result.push(format!("{path}/branches/{key}"));
                        }
                    }
                    for (key, value) in o {
                        branches(value, &format!("{path}/{key}"), result);
                    }
                }
                Value::Array(a) => {
                    for (index, value) in a.iter().enumerate() {
                        branches(value, &format!("{path}/{index}"), result);
                    }
                }
                _ => {}
            }
        }
        let baseline = graph();
        let fixtures = root_fixtures(&baseline);
        let mut paths = Vec::new();
        branches(&baseline["types"], "/types", &mut paths);
        for path in paths {
            let (parent, key) = path.rsplit_once('/').unwrap();
            let mut mutated = baseline.clone();
            mutated
                .pointer_mut(parent)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .remove(key)
                .unwrap();
            assert!(
                killed_by_roots(&mutated, &fixtures),
                "union branch deletion survived event/control roots: {path}"
            );
        }
    }
    #[test]
    fn every_reachable_json_field_has_an_invalid_shape_or_bound_probe() {
        type Locations = std::collections::BTreeMap<String, (usize, String)>;
        fn locate(
            node: &Value,
            value: &Value,
            path: &str,
            instance: &str,
            index: usize,
            context: (&Value, &ClaudeGraph),
            locations: &mut Locations,
        ) {
            let (graph, validator) = context;
            match node["kind"].as_str().unwrap() {
                "reference" => {
                    let name = node["name"].as_str().unwrap();
                    if let Some(target) = graph["types"].get(name) {
                        locate(
                            target,
                            value,
                            &format!("/types/{name}"),
                            instance,
                            index,
                            context,
                            locations,
                        );
                    } else if name == "Record" {
                        if let Some(object) = value.as_object() {
                            for (key, value) in object {
                                locate(
                                    &node["arguments"][1],
                                    value,
                                    &format!("{path}/arguments/1"),
                                    &format!("{instance}/{key}"),
                                    index,
                                    context,
                                    locations,
                                );
                            }
                        }
                    } else if name == "Promise" {
                        locate(
                            &node["arguments"][0],
                            value,
                            &format!("{path}/arguments/0"),
                            instance,
                            index,
                            context,
                            locations,
                        );
                    }
                }
                "object" => {
                    for (key, property) in node["properties"].as_object().unwrap() {
                        if let Some(value) = value.get(key) {
                            let field = format!("{path}/properties/{key}");
                            let target = format!("{instance}/{key}");
                            locations
                                .entry(field.clone())
                                .or_insert((index, target.clone()));
                            locate(
                                &property["type"],
                                value,
                                &format!("{field}/type"),
                                &target,
                                index,
                                context,
                                locations,
                            );
                        }
                    }
                }
                "union" | "intersection" => {
                    for (key, branch) in node["branches"].as_object().unwrap() {
                        let mut work = MAX_VALIDATION_WORK;
                        if node["kind"] == "intersection"
                            || validator.closed(branch, value, path, &mut work, 0).is_ok()
                        {
                            locate(
                                branch,
                                value,
                                &format!("{path}/branches/{key}"),
                                instance,
                                index,
                                context,
                                locations,
                            );
                        }
                    }
                }
                "array" => {
                    if let Some(array) = value.as_array() {
                        for (i, value) in array.iter().enumerate() {
                            locate(
                                &node["element"],
                                value,
                                &format!("{path}/element"),
                                &format!("{instance}/{i}"),
                                index,
                                context,
                                locations,
                            );
                        }
                    }
                }
                _ => {}
            }
        }
        let graph = graph();
        let validator = ClaudeGraph::new(&graph).unwrap();
        let fixtures = root_fixtures(&graph);
        let mut locations = Locations::new();
        for (i, fixture) in fixtures.iter().enumerate() {
            let name = fixture["type"].as_str().unwrap();
            locate(
                &graph["types"][name],
                &fixture["value"],
                &format!("/types/{name}"),
                "",
                i,
                (&graph, &validator),
                &mut locations,
            );
        }
        for field in graph["field_paths"].as_array().unwrap() {
            let path = field["path"].as_str().unwrap().strip_prefix('#').unwrap();
            if path == "/types/HookCallback/parameters/2/type/properties/signal" {
                // The native binding validates the actual non-JSON control slot.
                let mut changed = graph.clone();
                changed.pointer_mut(path).unwrap()["type"] = serde_json::json!({"kind":"string"});
                assert!(ClaudeGraph::new(&changed).is_err());
                continue;
            }
            let (index, instance) = locations
                .get(path)
                .unwrap_or_else(|| panic!("field not reachable through root fixture: {path}"));
            let fixture = &fixtures[*index];
            let name = fixture["type"].as_str().unwrap();
            let mut rejected = false;
            for bad in [
                Value::Null,
                Value::Bool(true),
                serde_json::json!(7),
                serde_json::json!("invalid-shape-probe"),
                serde_json::json!({"unknown-probe":true}),
                serde_json::json!([]),
            ] {
                let mut value = fixture["value"].clone();
                *value.pointer_mut(instance).unwrap() = bad;
                if validator.validate(name, &value).is_err() {
                    rejected = true;
                    break;
                }
            }
            if !rejected {
                assert_eq!(
                    graph.pointer(path).unwrap()["type"]["kind"],
                    "unknown",
                    "field type constraint was not enforced: {path}"
                );
                let mut value = fixture["value"].clone();
                *value.pointer_mut(instance).unwrap() =
                    Value::String("x".repeat(MAX_WIRE_BYTES + 1));
                assert!(
                    validator.validate(name, &value).is_err(),
                    "unknown field is not bounded: {path}"
                );
            }
            if field["required"] == true {
                let mut value = fixture["value"].clone();
                let (parent, key) = instance.rsplit_once('/').unwrap();
                value
                    .pointer_mut(parent)
                    .unwrap()
                    .as_object_mut()
                    .unwrap()
                    .remove(key);
                assert!(
                    validator.validate(name, &value).is_err(),
                    "required nested field accepted deletion: {path}"
                );
            }
        }
    }
}
