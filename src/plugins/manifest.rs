//! Manifest composition consumes only admitted bytes. Runner validation follows separately.
use super::{
    snapshot::{Snapshot, canonical_name},
    types::*,
};
use anyhow::{Context, Result, ensure};
use serde_json::{Map, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
/// Maximum distinct component reference paths in one manifest field.
pub const MAX_COMPONENT_PATHS: usize = 4096;
const PORTABLE_SCHEMA: &str = "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json";
const METADATA: &[&str] = &[
    "$schema",
    "name",
    "version",
    "description",
    "author",
    "homepage",
    "repository",
    "license",
    "keywords",
    "displayName",
    "metadata",
    "interface",
];
fn json(snapshot: &Snapshot, path: &str) -> Result<Value> {
    let file = snapshot
        .files
        .get(path)
        .with_context(|| format!("declaration is absent from snapshot: {path}"))?;
    let value: Value = serde_json::from_slice::<UniqueJson>(file.bytes())
        .with_context(|| format!("invalid JSON in {path}"))?
        .0;
    ensure!(value.is_object(), "{path} must contain an object");
    Ok(value)
}
pub(super) fn inspect(mut snapshot: Snapshot, options: &ImportOptions) -> Result<Package> {
    let root = snapshot.files.contains_key("plugin.json");
    let claude = snapshot.files.contains_key(".claude-plugin/plugin.json");
    let codex = snapshot.files.contains_key(".codex-plugin/plugin.json");
    ensure!(
        root || !(claude && codex) || options.dialect.is_some(),
        "independent Claude and Codex manifests require dialect selection"
    );
    let dialect = if root {
        Dialect::Portable
    } else {
        options.dialect.unwrap_or(if codex {
            Dialect::Codex
        } else {
            Dialect::Claude
        })
    };
    if root {
        ensure!(
            options.dialect.is_none_or(|d| d == Dialect::Portable),
            "portable root is authoritative; legacy dialect cannot replace it"
        );
    }
    let path = match dialect {
        Dialect::Portable => "plugin.json",
        Dialect::Claude => ".claude-plugin/plugin.json",
        Dialect::Codex => ".codex-plugin/plugin.json",
    };
    let metadata = if snapshot.files.contains_key(path) {
        json(&snapshot, path)?
    } else {
        ensure!(dialect == Dialect::Claude, "selected manifest missing");
        serde_json::json!({"name":snapshot.source.canonical_root.file_name().and_then(|s|s.to_str()).unwrap_or("plugin")})
    };
    if dialect == Dialect::Portable {
        ensure!(
            metadata.get("$schema").and_then(Value::as_str) == Some(PORTABLE_SCHEMA),
            "root plugin.json must declare the supported portable schema"
        );
    }
    let name = metadata
        .get("name")
        .and_then(Value::as_str)
        .context("manifest name must be a string")?
        .to_owned();
    ensure!(
        !name.is_empty()
            && name.len() <= 64
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.' || b == b'_'),
        "invalid package name"
    );
    if dialect == Dialect::Portable {
        ensure!(
            name.bytes().all(|b| !b.is_ascii_uppercase() && b != b'_')
                && !name.contains("--")
                && !name.contains("..")
                && name.as_bytes()[0].is_ascii_alphanumeric()
                && name.as_bytes()[name.len() - 1].is_ascii_alphanumeric(),
            "invalid portable package name"
        );
    }
    validate_metadata(&metadata)?;
    if claude && dialect != Dialect::Claude {
        snapshot.report.exclusions.push(
            ".claude-plugin/plugin.json: foreign metadata retained; declarations not selected"
                .into(),
        );
    }
    if codex && dialect == Dialect::Claude {
        snapshot
            .report
            .exclusions
            .push(".codex-plugin/plugin.json: independent Codex declarations not selected".into());
    }
    let mut builder = Builder {
        snapshot,
        name: name.clone(),
        components: Vec::new(),
        declarations: BTreeSet::new(),
        identities: BTreeMap::new(),
        repeated_declarations: 0,
    };
    if dialect == Dialect::Portable {
        builder.paths(ComponentKind::Skill, "skills", None, false)?;
        builder.service(ComponentKind::Mcp, "mcpServers", None, Some("mcp.json"))?;
        let extension = match metadata.get("extensions") {
            None => None,
            Some(v) => {
                let extensions = v.as_object().context("extensions must be an object")?;
                for (key, value) in extensions {
                    ensure!(value.is_object(), "extension {key} must be an object");
                }
                extensions.get("com.openai").cloned()
            }
        };
        let settings = if let Some(value) = extension {
            builder
                .snapshot
                .report
                .composition
                .push("Inline extensions.com.openai replaces the complete Codex overlay".into());
            Some(value)
        } else if codex {
            builder.snapshot.report.composition.push(
                "Portable identity, skills and MCP retained; Codex overlay adds settings".into(),
            );
            Some(json(&builder.snapshot, ".codex-plugin/plugin.json")?)
        } else {
            None
        };
        if let Some(settings) = settings {
            builder.manifest(&settings, Dialect::Codex, true)?;
        }
        builder.unknown_fields(&metadata, &["extensions"])?;
    } else {
        builder.manifest(&metadata, dialect, false)?;
    }
    if builder.repeated_declarations > 0 {
        builder.snapshot.report.composition.push(format!("{} repeated component references or additional invalid paths summarized; each declaration inspected once", builder.repeated_declarations));
    }
    Ok(Package {
        source: builder.snapshot.source,
        digest: builder.snapshot.digest,
        files: builder.snapshot.files,
        directories: builder.snapshot.directories,
        dialect,
        name,
        metadata,
        components: builder.components,
        report: builder.snapshot.report,
    })
}
fn validate_metadata(value: &Value) -> Result<()> {
    for key in [
        "name",
        "version",
        "description",
        "homepage",
        "repository",
        "license",
        "displayName",
    ] {
        if let Some(value) = value.get(key) {
            ensure!(value.is_string(), "manifest {key} must be a string");
        }
    }
    if let Some(author) = value.get("author") {
        let author = author
            .as_object()
            .context("manifest author must be an object")?;
        for (key, value) in author {
            ensure!(value.is_string(), "author.{key} must be a string");
        }
    }
    if let Some(keywords) = value.get("keywords") {
        ensure!(
            keywords
                .as_array()
                .is_some_and(|a| a.iter().all(Value::is_string)),
            "keywords must be an array of strings"
        );
    }
    Ok(())
}
struct Builder {
    snapshot: Snapshot,
    name: String,
    components: Vec<Component>,
    declarations: BTreeSet<(ComponentKind, String)>,
    identities: BTreeMap<String, String>,
    repeated_declarations: usize,
}
impl Builder {
    fn diagnostic(&mut self, identity: Option<String>, field: &str, message: impl Into<String>) {
        self.snapshot.report.diagnostics.push(Diagnostic {
            component: identity,
            field: field.into(),
            message: message.into(),
        });
    }
    fn add(
        &mut self,
        kind: ComponentKind,
        name: &str,
        declaration: &str,
        value: Value,
        validity: SourceValidity,
    ) -> Result<()> {
        if !self.declarations.insert((kind.clone(), declaration.into())) {
            self.repeated_declarations += 1;
            return Ok(());
        }
        let identity = format!("{}:{name}", self.name);
        if let Some(previous) = self.identities.insert(identity.clone(), declaration.into()) {
            ensure!(
                previous == declaration,
                "qualified component identity collision {identity}: {previous} and {declaration}"
            );
        }
        let readiness = if validity == SourceValidity::Invalid {
            ExecutableReadiness::Invalid
        } else {
            ExecutableReadiness::Unavailable
        };
        self.diagnostic(
            Some(identity.clone()),
            declaration,
            if validity == SourceValidity::Invalid {
                "Invalid declaration; activation held"
            } else {
                "Runner and semantic validation unavailable; parsing is not executable support"
            },
        );
        self.components.push(Component {
            identity,
            kind,
            declaration: declaration.into(),
            required: true,
            validity,
            readiness,
            metadata: value,
        });
        Ok(())
    }
    fn invalid(
        &mut self,
        kind: ComponentKind,
        field: &str,
        value: &Value,
        error: impl std::fmt::Display,
    ) -> Result<()> {
        if self
            .declarations
            .contains(&(kind.clone(), format!("manifest#{field}")))
        {
            self.repeated_declarations += 1;
            return Ok(());
        }
        self.diagnostic(None, field, error.to_string());
        self.add(
            kind,
            &format!("invalid:{field}"),
            &format!("manifest#{field}"),
            value.clone(),
            SourceValidity::Invalid,
        )
    }
    fn unknown_fields(&mut self, value: &Value, known: &[&str]) -> Result<()> {
        for (field, value) in value.as_object().context("manifest must be object")? {
            if !METADATA.contains(&field.as_str()) && !known.contains(&field.as_str()) {
                self.diagnostic(
                    None,
                    field,
                    "Unknown field retained without executable interpretation",
                );
                self.add(
                    ComponentKind::Unknown,
                    &format!("unknown:{field}"),
                    &format!("manifest#{field}"),
                    value.clone(),
                    SourceValidity::Unvalidated,
                )?;
            }
        }
        Ok(())
    }
    fn manifest(&mut self, value: &Value, dialect: Dialect, overlay: bool) -> Result<()> {
        ensure!(value.is_object(), "manifest settings must be object");
        if overlay {
            for key in ["name", "skills", "mcpServers"] {
                if value.get(key).is_some() {
                    self.snapshot.report.composition.push(format!(
                        "Overlay {key} does not replace portable root declaration"
                    ));
                }
            }
        } else {
            self.paths(
                ComponentKind::Skill,
                "skills",
                value.get("skills"),
                dialect == Dialect::Claude,
            )?;
            self.service(
                ComponentKind::Mcp,
                "mcpServers",
                value.get("mcpServers"),
                Some(".mcp.json"),
            )?;
        }
        self.service(
            ComponentKind::Hooks,
            "hooks",
            value.get("hooks"),
            Some("hooks/hooks.json"),
        )?;
        if dialect == Dialect::Claude {
            for (kind, field, default) in [
                (ComponentKind::Command, "commands", "commands"),
                (ComponentKind::Agent, "agents", "agents"),
                (ComponentKind::Workflow, "workflows", "workflows"),
                (ComponentKind::OutputStyle, "outputStyles", "output-styles"),
                (ComponentKind::Theme, "themes", "themes"),
                (ComponentKind::Monitor, "monitors", "monitors"),
            ] {
                let declaration = value
                    .get(field)
                    .or_else(|| value.get("experimental").and_then(|v| v.get(field)));
                self.paths(kind, default, declaration, false)?;
            }
            self.service(
                ComponentKind::Lsp,
                "lspServers",
                value.get("lspServers"),
                Some(".lsp.json"),
            )?;
        }
        let configuration: &[&str] = if dialect == Dialect::Claude {
            &["userConfig", "channels", "dependencies", "defaultEnabled"]
        } else {
            &["apps"]
        };
        for &field in configuration {
            if let Some(v) = value.get(field) {
                let valid = if field == "defaultEnabled" {
                    v.is_boolean()
                } else if field == "dependencies" {
                    v.is_object() || v.is_array()
                } else {
                    v.is_object()
                };
                if valid {
                    self.add(
                        if field == "apps" {
                            ComponentKind::App
                        } else {
                            ComponentKind::Configuration
                        },
                        field,
                        &format!("manifest#{field}"),
                        v.clone(),
                        SourceValidity::Unvalidated,
                    )?;
                } else {
                    self.invalid(
                        ComponentKind::Configuration,
                        field,
                        v,
                        "invalid configuration shape",
                    )?;
                }
            }
        }
        if dialect == Dialect::Claude
            && let Some(v) = value.get("experimental")
        {
            if !v.is_object() {
                self.invalid(
                    ComponentKind::Configuration,
                    "experimental",
                    v,
                    "experimental must be an object",
                )?;
            } else {
                for (field, v) in v.as_object().unwrap() {
                    if !["themes", "monitors"].contains(&field.as_str()) {
                        self.invalid(
                            ComponentKind::Unknown,
                            &format!("experimental.{field}"),
                            v,
                            "unknown experimental field",
                        )?;
                    }
                }
            }
        }
        let mut recognized = vec!["skills", "mcpServers", "hooks"];
        recognized.extend_from_slice(configuration);
        if dialect == Dialect::Claude {
            recognized.extend_from_slice(&[
                "commands",
                "agents",
                "workflows",
                "outputStyles",
                "themes",
                "monitors",
                "lspServers",
                "experimental",
            ]);
        }
        self.unknown_fields(value, &recognized)
    }

    fn paths(
        &mut self,
        kind: ComponentKind,
        default: &str,
        declared: Option<&Value>,
        additive: bool,
    ) -> Result<()> {
        let mut roots = Vec::new();
        let mut seen = BTreeSet::new();
        if declared.is_none() || additive {
            seen.insert(default.to_owned());
            roots.push(default.to_owned());
        }
        if let Some(value) = declared {
            match paths(value) {
                Ok(custom) => {
                    self.snapshot.report.composition.push(format!(
                        "{default}: custom paths {} defaults",
                        if additive { "add to" } else { "replace" }
                    ));
                    for path in custom {
                        match canonical_name(path) {
                            Ok(path) => {
                                if seen.insert(path.clone()) {
                                    roots.push(path);
                                } else {
                                    self.repeated_declarations += 1;
                                }
                                ensure!(
                                    roots.len() <= MAX_COMPONENT_PATHS,
                                    "component field {default} exceeds {MAX_COMPONENT_PATHS} distinct paths"
                                );
                            }
                            Err(error) => self.invalid(
                                kind.clone(),
                                default,
                                &Value::String(path.to_owned()),
                                error,
                            )?,
                        }
                    }
                }
                Err(error) => return self.invalid(kind, default, value, error),
            }
        }
        let mut found = false;
        for root in roots {
            let prefix = format!("{root}/");
            let matches: Vec<String> = self
                .snapshot
                .files
                .get_key_value(&root)
                .into_iter()
                .chain(
                    self.snapshot
                        .files
                        .range(prefix.clone()..)
                        .take_while(|(path, _)| path.starts_with(&prefix)),
                )
                .map(|(path, _)| path)
                .filter(|path| {
                    if kind == ComponentKind::Skill {
                        Path::new(path)
                            .file_name()
                            .is_some_and(|name| name == "SKILL.md")
                    } else {
                        path.ends_with(".md") || path.ends_with(".json")
                    }
                })
                .cloned()
                .collect();
            if matches.is_empty() && declared.is_some() && (!additive || root != default) {
                self.invalid(
                    kind.clone(),
                    default,
                    &Value::String(root.clone()),
                    format!("component path has no admitted declarations: {root}"),
                )?;
            }
            for path in matches {
                found = true;
                let name = if kind == ComponentKind::Skill {
                    Path::new(&path)
                        .parent()
                        .and_then(Path::file_name)
                        .and_then(|n| n.to_str())
                        .unwrap_or("skill")
                        .to_owned()
                } else {
                    Path::new(&path)
                        .file_stem()
                        .and_then(|n| n.to_str())
                        .unwrap_or("component")
                        .to_owned()
                };
                let name = if kind == ComponentKind::Skill || kind == ComponentKind::Command {
                    name
                } else {
                    format!("{kind:?}:{name}")
                };
                let file = self.snapshot.files.get(&path).unwrap();
                let valid = std::str::from_utf8(file.bytes()).is_ok();
                self.add(
                    kind.clone(),
                    &name,
                    &path,
                    Value::Null,
                    if valid {
                        SourceValidity::Unvalidated
                    } else {
                        SourceValidity::Invalid
                    },
                )?;
            }
        }
        if kind == ComponentKind::Skill
            && additive
            && !found
            && declared.is_none()
            && self.snapshot.files.contains_key("SKILL.md")
            && !self.snapshot.directories.contains_key("skills")
        {
            self.add(
                kind,
                "skill",
                "SKILL.md",
                Value::Null,
                SourceValidity::Unvalidated,
            )?;
        }
        Ok(())
    }
    fn service(
        &mut self,
        kind: ComponentKind,
        field: &str,
        declared: Option<&Value>,
        default: Option<&str>,
    ) -> Result<()> {
        let mut inputs = Vec::new();
        let mut seen = BTreeSet::new();
        if let Some(default) = default
            && self.snapshot.files.contains_key(default)
        {
            seen.insert(default.to_owned());
            inputs.push((default.to_owned(), None));
        }
        if let Some(value) = declared {
            if value.is_object() {
                inputs.push((format!("manifest#{field}"), Some(value.clone())));
            } else {
                match paths(value) {
                    Ok(paths) => {
                        for path in paths {
                            match canonical_name(path) {
                                Ok(path) => {
                                    if seen.insert(path.clone()) {
                                        inputs.push((path, None));
                                    } else {
                                        self.repeated_declarations += 1;
                                    }
                                    ensure!(
                                        inputs.len() <= MAX_COMPONENT_PATHS,
                                        "component field {field} exceeds {MAX_COMPONENT_PATHS} distinct paths"
                                    );
                                }
                                Err(e) => {
                                    self.invalid(
                                        kind.clone(),
                                        field,
                                        &Value::String(path.to_owned()),
                                        e,
                                    )?;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        self.invalid(kind.clone(), field, value, e)?;
                    }
                }
            }
        }
        for (path, inline) in inputs {
            let value = match inline {
                Some(v) => v,
                None => match json(&self.snapshot, &path) {
                    Ok(v) => v,
                    Err(e) => {
                        self.invalid(kind.clone(), field, &Value::String(path), e)?;
                        continue;
                    }
                },
            };
            if kind == ComponentKind::Hooks {
                let errors = validate_hooks(&value);
                for (field, message) in &errors {
                    self.diagnostic(None, field, message.clone());
                }
                self.add(
                    kind.clone(),
                    &format!("hooks:{path}"),
                    &path,
                    value,
                    if errors.is_empty() {
                        SourceValidity::Unvalidated
                    } else {
                        SourceValidity::Invalid
                    },
                )?;
            } else {
                let entries = value.get(field).unwrap_or(&value);
                let Some(entries) = entries.as_object() else {
                    self.invalid(
                        kind.clone(),
                        field,
                        &value,
                        "server declarations must be an object",
                    )?;
                    continue;
                };
                for (server, definition) in entries {
                    let identity = format!("{kind:?}:{server}");
                    let declaration = format!("{path}#{server}");
                    if let Some(previous) = self
                        .components
                        .iter()
                        .find(|c| c.identity == format!("{}:{identity}", self.name))
                    {
                        ensure!(
                            previous.metadata == *definition,
                            "conflicting {field} server {server} requires explicit selection"
                        );
                        self.repeated_declarations += 1;
                        continue;
                    }
                    let valid = validate_server(definition, kind == ComponentKind::Lsp);
                    if let Err(ref e) = valid {
                        self.diagnostic(None, &format!("{field}.{server}"), e.to_string());
                    }
                    self.add(
                        kind.clone(),
                        &identity,
                        &declaration,
                        definition.clone(),
                        if valid.is_ok() {
                            SourceValidity::Unvalidated
                        } else {
                            SourceValidity::Invalid
                        },
                    )?;
                }
            }
        }
        Ok(())
    }
}
fn paths(value: &Value) -> Result<Vec<&str>> {
    if let Some(s) = value.as_str() {
        Ok(vec![s])
    } else {
        value
            .as_array()
            .context("component paths must be a string or array of strings")?
            .iter()
            .map(|v| v.as_str().context("component path must be a string"))
            .collect()
    }
}
fn validate_server(value: &Value, lsp: bool) -> Result<()> {
    let map = value.as_object().context("server must be an object")?;
    for field in map.keys() {
        let known = [
            "type",
            "command",
            "args",
            "env",
            "url",
            "headers",
            "headersHelper",
            "oauth",
            "cwd",
        ];
        let language = [
            "extensionToLanguage",
            "transport",
            "initializationOptions",
            "settings",
            "workspaceFolder",
            "startupTimeout",
            "shutdownTimeout",
            "restartOnCrash",
            "maxRestarts",
            "diagnostics",
        ];
        ensure!(
            known.contains(&field.as_str()) || (lsp && language.contains(&field.as_str())),
            "unknown executable server field {field}"
        );
    }
    ensure!(
        map.get("command").is_some_and(Value::is_string)
            || (!lsp && map.get("url").is_some_and(Value::is_string)),
        "server requires command or URL"
    );
    for field in [
        "command",
        "url",
        "type",
        "cwd",
        "transport",
        "headersHelper",
    ] {
        if let Some(v) = map.get(field) {
            ensure!(v.is_string(), "{field} must be a string");
        }
    }
    if let Some(v) = map.get("args") {
        ensure!(
            v.as_array().is_some_and(|a| a.iter().all(Value::is_string)),
            "args must be strings"
        );
    }
    for field in ["env", "headers", "extensionToLanguage"] {
        if let Some(v) = map.get(field) {
            ensure!(
                v.as_object()
                    .is_some_and(|m| m.values().all(Value::is_string)),
                "{field} must map strings to strings"
            );
        }
    }
    Ok(())
}
fn validate_hooks(value: &Value) -> Vec<(String, String)> {
    let mut errors = Vec::new();
    let Some(events) = value.get("hooks").and_then(Value::as_object) else {
        return vec![("hooks".into(), "hook file requires a hooks object".into())];
    };
    for field in value.as_object().into_iter().flat_map(|m| m.keys()) {
        if field != "hooks" && field != "description" {
            errors.push((field.clone(), "unknown hook declaration field".into()));
        }
    }
    for (event, groups) in events {
        const EVENTS: &[&str] = &[
            "SessionStart",
            "UserPromptSubmit",
            "UserPromptExpansion",
            "InstructionsLoaded",
            "PreToolUse",
            "PermissionRequest",
            "PermissionDenied",
            "PostToolUse",
            "PostToolUseFailure",
            "PostToolBatch",
            "Stop",
            "StopFailure",
            "Interrupt",
            "SessionEnd",
            "TaskCreated",
            "TaskCompleted",
            "SubagentStart",
            "SubagentStop",
            "TeammateIdle",
            "WorktreeCreate",
            "WorktreeRemove",
            "PreCompact",
            "PostCompact",
            "PreModelSwitch",
            "PostModelSwitch",
            "ConfigChange",
            "Setup",
            "Notification",
            "FileChanged",
            "CwdChanged",
            "DirectoryAdded",
            "MessageDisplay",
            "Elicitation",
            "ElicitationResult",
        ];
        if !EVENTS.contains(&event.as_str()) {
            errors.push((format!("hooks.{event}"), "unknown hook event".into()));
        }
        let Some(groups) = groups.as_array() else {
            errors.push((
                format!("hooks.{event}"),
                "event groups must be an array".into(),
            ));
            continue;
        };
        for (index, group) in groups.iter().enumerate() {
            let field = format!("hooks.{event}[{index}]");
            let Some(group) = group.as_object() else {
                errors.push((field, "hook group must be object".into()));
                continue;
            };
            for (key, v) in group {
                if key == "matcher" {
                    if !v.is_string() {
                        errors.push((
                            format!("{field}.matcher"),
                            "matcher must be a string".into(),
                        ));
                    }
                } else if key != "hooks" {
                    errors.push((
                        format!("{field}.{key}"),
                        "unvalidated hook group field".into(),
                    ));
                }
            }
            let Some(handlers) = group.get("hooks").and_then(Value::as_array) else {
                errors.push((field, "group requires hooks array".into()));
                continue;
            };
            for (index, handler) in handlers.iter().enumerate() {
                let field = format!("{field}.hooks[{index}]");
                let Some(handler) = handler.as_object() else {
                    errors.push((field, "handler must be object".into()));
                    continue;
                };
                validate_handler(handler, &field, &mut errors);
            }
        }
    }
    errors
}
fn validate_handler(handler: &Map<String, Value>, field: &str, errors: &mut Vec<(String, String)>) {
    let kind = handler.get("type").and_then(Value::as_str).unwrap_or("");
    let required: &[&str] = match kind {
        "command" => &["command"],
        "http" => &["url"],
        "mcp_tool" => &["server", "tool"],
        "prompt" | "agent" => &["prompt"],
        _ => {
            errors.push((
                format!("{field}.type"),
                "unknown or missing hook type".into(),
            ));
            &[]
        }
    };
    for key in required {
        if handler
            .get(*key)
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        {
            errors.push((
                format!("{field}.{key}"),
                "required nonempty string missing".into(),
            ));
        }
    }
    for (key, value) in handler {
        let valid = match key.as_str() {
            "type" | "command" | "url" | "server" | "tool" | "prompt" | "model" | "if"
            | "statusMessage" => value.is_string(),
            "timeout" | "asyncTimeout" => value.as_f64().is_some_and(|v| v.is_finite() && v > 0.0),
            "once" | "async" | "asyncRewake" | "continueOnBlock" => value.is_boolean(),
            "args" | "allowedEnvVars" => value
                .as_array()
                .is_some_and(|a| a.iter().all(Value::is_string)),
            "headers" => value
                .as_object()
                .is_some_and(|m| m.values().all(Value::is_string)),
            "input" => value.is_object(),
            _ => false,
        };
        if !valid {
            errors.push((
                format!("{field}.{key}"),
                "unknown or malformed executable field".into(),
            ));
        }
    }
}

// serde_json::Value normally overwrites duplicate keys. A recursive visitor makes
// ambiguity an import error, including inside executable settings.
pub(super) struct UniqueJson(pub(super) Value);
impl<'de> serde::Deserialize<'de> for UniqueJson {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = UniqueJson;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("JSON without duplicate object keys")
            }
            fn visit_bool<E: serde::de::Error>(
                self,
                v: bool,
            ) -> std::result::Result<UniqueJson, E> {
                Ok(UniqueJson(Value::Bool(v)))
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> std::result::Result<UniqueJson, E> {
                Ok(UniqueJson(v.into()))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> std::result::Result<UniqueJson, E> {
                Ok(UniqueJson(v.into()))
            }
            fn visit_f64<E: serde::de::Error>(self, v: f64) -> std::result::Result<UniqueJson, E> {
                serde_json::Number::from_f64(v)
                    .map(|n| UniqueJson(Value::Number(n)))
                    .ok_or_else(|| E::custom("nonfinite JSON number"))
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> std::result::Result<UniqueJson, E> {
                Ok(UniqueJson(Value::String(v.into())))
            }
            fn visit_string<E: serde::de::Error>(
                self,
                v: String,
            ) -> std::result::Result<UniqueJson, E> {
                Ok(UniqueJson(Value::String(v)))
            }
            fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<UniqueJson, E> {
                Ok(UniqueJson(Value::Null))
            }
            fn visit_none<E: serde::de::Error>(self) -> std::result::Result<UniqueJson, E> {
                Ok(UniqueJson(Value::Null))
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> std::result::Result<UniqueJson, A::Error> {
                let mut values = Vec::new();
                while let Some(UniqueJson(value)) = seq.next_element()? {
                    values.push(value);
                }
                Ok(UniqueJson(Value::Array(values)))
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> std::result::Result<UniqueJson, A::Error> {
                let mut values = Map::new();
                while let Some(key) = map.next_key::<String>()? {
                    if values.contains_key(&key) {
                        return Err(serde::de::Error::custom("duplicate JSON object key"));
                    }
                    let UniqueJson(value) = map.next_value()?;
                    values.insert(key, value);
                }
                Ok(UniqueJson(Value::Object(values)))
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}
