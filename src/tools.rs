//! The shared tool boundary. Provider payloads become typed requests only here.
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, PermissionsExt},
    },
    path::{Component, Path, PathBuf},
    process::Stdio,
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{Context, Result, bail, ensure};
use rustix::fs::{Mode, OFlags, ResolveFlags, openat2};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
};

use crate::events::{Event, EventSink};

const MAX_BYTES: usize = 1024 * 1024;
const RESOLVE: ResolveFlags = ResolveFlags::BENEATH.union(ResolveFlags::NO_SYMLINKS);

#[derive(Clone)]
pub struct AccessPolicy {
    pub unrestricted: bool,
    /// Runtime-only child boundary; cannot be expanded by Oracle approval.
    pub strict_worktree: bool,
    pub tools_enabled: bool,
    pub oracle: Option<Box<crate::config::Connection>>,
    pub credential_paths: Vec<PathBuf>,
    /// Trusted runtime executable that supervises host Bash through a lifetime pipe.
    pub supervisor: Option<PathBuf>,
    /// Trusted parent-only operations, attached by the session owner.
    pub extension: Option<Arc<dyn ToolExtension>>,
    /// Explicit session configuration; repositories and models cannot enable servers.
    pub language_servers: crate::language_services::LanguageServers,
    /// Host-selected lifecycle owner; packages cannot configure a backend relay.
    pub lifecycle: Option<Arc<crate::plugins::bridge::Lifecycle>>,
    /// Host-only model-hook evidence capability; never deserialized with Connection.
    pub snapshot: Option<Arc<crate::plugins::runners::SnapshotInspection>>,
}

impl Default for AccessPolicy {
    fn default() -> Self {
        Self {
            unrestricted: false,
            strict_worktree: false,
            tools_enabled: true,
            oracle: None,
            credential_paths: Vec::new(),
            supervisor: None,
            extension: None,
            language_servers: crate::language_services::LanguageServers::default(),
            lifecycle: None,
            snapshot: None,
        }
    }
}

impl AccessPolicy {
    pub fn worktree_only(credential_paths: Vec<PathBuf>) -> Self {
        Self {
            strict_worktree: true,
            credential_paths,
            ..Self::default()
        }
    }

    pub fn review_only() -> Self {
        Self {
            tools_enabled: false,
            ..Self::default()
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct ToolResult {
    pub call_id: String,
    pub tool: String,
    pub success: bool,
    pub output: String,
    pub exit_code: Option<i32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadArgs {
    path: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WriteArgs {
    path: String,
    content: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EditArgs {
    path: String,
    old_text: String,
    new_text: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BashArgs {
    command: String,
}

/// Hooks can transform requests, but cannot admit or execute them.
pub trait ToolHook: Send + Sync {
    fn before(&self, call: &mut ToolCall) -> Result<()>;
    fn present(&self, result: &ToolResult) -> Result<String> {
        Ok(result.output.clone())
    }
}

/// Extensions share admission, hooks and receipts with the four coding tools.
#[async_trait::async_trait]
pub trait ToolExtension: Send + Sync {
    fn definitions(&self) -> Vec<Value>;
    async fn execute(&self, call: &ToolCall, events: &EventSink) -> Result<String>;
}

struct ToolEffect<'a> {
    events: &'a EventSink,
    started: bool,
    admission: Option<crate::plugins::admission::AdmittedCandidate>,
    boundary: Option<Arc<tokio::sync::Mutex<()>>>,
    guard: Option<Arc<tokio::sync::OwnedMutexGuard<()>>>,
}

/// The same descriptor resolver used by file admission pins the target inspected
/// by path matchers. A missing leaf pins its existing parent instead.
pub(crate) struct PluginTarget {
    anchor: File,
    path: PathBuf,
    leaf: Option<std::ffi::OsString>,
}

impl PluginTarget {
    pub(crate) fn verify(&self) -> Result<()> {
        ensure!(
            descriptor_path(&self.anchor)? == self.path,
            "plugin target moved after path matching; retry"
        );
        Ok(())
    }

    fn bind(&self, file: &File, parent: bool) -> Result<()> {
        let expected = self.anchor.metadata()?;
        let actual = file.metadata()?;
        ensure!(
            parent == self.leaf.is_some()
                && (expected.dev(), expected.ino()) == (actual.dev(), actual.ino())
                && descriptor_path(file)? == self.path,
            "plugin target changed after path matching; retry"
        );
        self.verify()
    }
}

impl ToolEffect<'_> {
    fn bind_target(&self, file: &File, parent: bool) -> Result<()> {
        if let Some(target) = self.admission.as_ref().and_then(|a| a.target.as_ref()) {
            target.bind(file, parent)?;
        }
        Ok(())
    }

    async fn lock(&mut self) {
        if self.guard.is_none()
            && let Some(boundary) = &self.boundary
        {
            self.guard = Some(Arc::new(boundary.clone().lock_owned().await));
        }
    }
    async fn start(&mut self) -> Result<()> {
        self.lock().await;
        if !self.started
            && let Some(admission) = &self.admission
        {
            admission.validate(self.guard.clone()).await?;
        }
        self.events.tool_effect()?;
        self.started = true;
        Ok(())
    }
}

pub struct ToolExecutor {
    root: Arc<File>,
    workspace: PathBuf,
    scratch: Option<PathBuf>,
    access: AccessPolicy,
    developer: Option<Arc<crate::developer_access::DeveloperAccess>>,
    worktree: Option<Arc<crate::worktree_access::WorktreeAccess>>,
    intent: Mutex<String>,
    hooks: Vec<Box<dyn ToolHook>>,
    plugin_plan: Option<Arc<crate::plugins::dispatch::PreToolPlan>>,
    gate_workspace: Arc<crate::plugins::gate_snapshot::GateWorkspace>,
    // Execution is sequential. Keep the current receipt across cancellation
    // during event delivery or a presentation error; never retain a full copy
    // of the session history here.
    completed: Mutex<Option<ToolResult>>,
    language_services: Option<crate::language_services::Manager>,
}

impl ToolExecutor {
    pub(crate) fn hook_host(&self) -> crate::plugins::runners::HookHost {
        crate::plugins::runners::HookHost::new(
            self.root.clone(),
            self.workspace.clone(),
            self.gate_workspace.frozen_credentials(),
            self.access.supervisor.clone(),
        )
    }

    pub fn new(workspace: &Path) -> Result<Self> {
        Self::with_policy(workspace, &AccessPolicy::default())
    }

    pub fn with_policy(workspace: &Path, access: &AccessPolicy) -> Result<Self> {
        ensure!(
            access.snapshot.is_none()
                || (!access.unrestricted
                    && !access.strict_worktree
                    && access.oracle.is_none()
                    && access.extension.is_none()
                    && !access.language_servers.enabled()
                    && access.lifecycle.is_none()),
            "snapshot hook policy cannot inherit live tools, extensions, language services or lifecycle dispatch"
        );
        access.language_servers.validate()?;
        ensure!(
            !access.language_servers.enabled() || (access.tools_enabled && !access.strict_worktree),
            "language servers are unavailable for child and review-only policies"
        );
        ensure!(cfg!(target_os = "linux"), "coding tools require Linux");
        ensure!(
            !(access.unrestricted && access.strict_worktree),
            "worktree-only policy cannot enable host tools"
        );
        ensure!(
            access.extension.is_none() || (access.tools_enabled && !access.strict_worktree),
            "child and reviewer policies cannot expose parent tool extensions"
        );
        if let Some(extension) = &access.extension {
            let mut names = std::collections::BTreeSet::from([
                "read".to_owned(),
                "write".to_owned(),
                "edit".to_owned(),
                "bash".to_owned(),
                "lsp".to_owned(),
            ]);
            let definitions = extension.definitions();
            ensure!(definitions.len() <= 16, "too many parent tool extensions");
            for definition in definitions {
                let name = definition["name"]
                    .as_str()
                    .context("extension tool requires a name")?;
                ensure!(
                    !name.is_empty()
                        && name.len() <= 64
                        && name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_'),
                    "invalid extension tool name"
                );
                ensure!(names.insert(name.to_owned()), "duplicate tool name: {name}");
                ensure!(
                    definition["input_schema"].is_object(),
                    "extension tool requires an input schema"
                );
            }
        }
        let root = File::open(workspace).context("open authorized workspace")?;
        ensure!(root.metadata()?.is_dir(), "workspace must be a directory");
        // Probe the required primitive up front. There is no path-based fallback.
        openat2(
            &root,
            ".",
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
            RESOLVE,
        )
        .context("workspace requires Linux openat2")?;
        let root = Arc::new(root);
        let workspace = workspace.canonicalize().context("resolve tool workspace")?;
        let developer = if !access.unrestricted
            && !access.strict_worktree
            && access.tools_enabled
            && access.snapshot.is_none()
        {
            Some(Arc::new(crate::developer_access::DeveloperAccess::new(
                &workspace,
                &access.credential_paths,
            )?))
        } else {
            None
        };
        let language_services = if access.language_servers.enabled() {
            let confined = match &developer {
                Some(access) => access.clone(),
                None => Arc::new(crate::developer_access::DeveloperAccess::new(
                    &workspace,
                    &access.credential_paths,
                )?),
            };
            Some(crate::language_services::Manager::new(
                access.language_servers.clone(),
                workspace.clone(),
                root.clone(),
                confined,
            ))
        } else {
            None
        };
        Ok(Self {
            root,
            language_services,
            workspace: workspace.clone(),
            scratch: if access.unrestricted && access.tools_enabled {
                // Host tools may put valuable data here. Do not recursively delete
                // it on session close; ordinary OS temporary-file policy applies.
                Some(
                    tempfile::Builder::new()
                        .prefix("demoncoder-")
                        .permissions(std::fs::Permissions::from_mode(0o700))
                        .tempdir_in("/tmp")?
                        .keep(),
                )
            } else {
                None
            },
            access: access.clone(),
            developer,
            worktree: if access.strict_worktree && access.tools_enabled {
                Some(Arc::new(crate::worktree_access::WorktreeAccess::new(
                    &workspace.canonicalize()?,
                    &access.credential_paths,
                )?))
            } else {
                None
            },
            intent: Mutex::new(String::new()),
            hooks: Vec::new(),
            plugin_plan: None,
            gate_workspace: Arc::new(
                crate::plugins::gate_snapshot::GateWorkspace::open_with_credentials(
                    &workspace,
                    &access.credential_paths,
                )?,
            ),
            completed: Mutex::new(None),
        })
    }

    pub fn definitions(&self) -> Vec<Value> {
        if !self.access.tools_enabled {
            return Vec::new();
        }
        if let Some(snapshot) = &self.access.snapshot {
            return snapshot.definitions();
        }
        let mut tools = definitions();
        if self.access.strict_worktree {
            for tool in &mut tools {
                tool["description"] = Value::String(if tool["name"] == "bash" {
                    "Run Bash with system executables and libraries in the child worktree. Only the worktree is writable. Home, other repositories, credentials, Git administration, and networking are unavailable. Hard links prevent launch. Limit 120 seconds and 1 MiB output.".into()
                } else {
                    "Access a UTF-8 file up to 1 MiB using a relative path inside the child worktree. Parent traversal, symlinks, hard links, credentials, and Git administration are forbidden. Edit requires exactly one old_text match; write requires an existing parent directory.".into()
                });
            }
        } else if self.access.unrestricted {
            for tool in &mut tools {
                let detail = if tool["name"] == "bash" {
                    format!(
                        "Run Bash directly on the host in {}. No sandbox. TMPDIR={} is approved session scratch. The Oracle reviews possible outside access before execution. Limit 120 seconds and 1 MiB output.",
                        self.workspace.display(),
                        self.scratch.as_ref().expect("host scratch").display()
                    )
                } else {
                    format!(
                        "{} Host paths may be absolute or relative to {}. Files outside the project or TMPDIR require Oracle approval.",
                        tool["description"].as_str().unwrap_or(""),
                        self.workspace.display()
                    )
                };
                tool["description"] = Value::String(detail);
            }
        }
        if let Some(extension) = &self.access.extension {
            tools.extend(extension.definitions());
        }
        if self.language_services.is_some() {
            tools.push(crate::language_services::definition());
        }
        tools
    }

    pub fn set_intent(&self, text: &str) {
        *self.intent.lock().expect("tool intent lock poisoned") = text.to_owned();
    }

    pub fn unrestricted(&self) -> bool {
        self.access.unrestricted
    }

    pub fn tools_enabled(&self) -> bool {
        self.access.tools_enabled
    }

    pub(crate) async fn stop_language_services(&mut self) -> Result<()> {
        if let Some(manager) = &mut self.language_services {
            manager.stop().await?;
        }
        Ok(())
    }

    /// Host-selected immutable registration; package discovery/activation is separate.
    pub fn register_pre_tool_plan(
        &mut self,
        plan: Arc<crate::plugins::dispatch::PreToolPlan>,
    ) -> Result<()> {
        ensure!(
            self.plugin_plan.is_none(),
            "pre-tool plan is already frozen"
        );
        self.plugin_plan = Some(plan);
        Ok(())
    }

    pub fn add_hook(&mut self, hook: Box<dyn ToolHook>) {
        self.hooks.push(hook);
    }

    pub(crate) fn take_completed(&self) -> Option<ToolResult> {
        self.completed
            .lock()
            .expect("tool receipt lock poisoned")
            .take()
    }

    pub async fn execute(&self, mut call: ToolCall, events: &EventSink) -> Result<ToolResult> {
        ensure!(self.hooks.len() <= 32, "too many tool hooks");
        let (scoped_events, replay) = events.begin_tool(&call)?;
        if let Some(result) = replay {
            return Ok(result);
        }
        let events = &scoped_events;
        self.take_completed();
        let identity = (call.id.clone(), call.name.clone());
        let root_metadata = self.root.metadata()?;
        let workspace_identity = (root_metadata.dev(), root_metadata.ino());
        let mut effect = ToolEffect {
            events,
            started: false,
            admission: None,
            boundary: if matches!(call.name.as_str(), "write" | "edit" | "bash") {
                events.mutation_boundary(workspace_identity)?
            } else {
                None
            },
            guard: None,
        };
        let execution = async {
            ensure!(self.access.tools_enabled, "the Oracle cannot execute tools");
            for hook in &self.hooks {
                hook.before(&mut call)?;
            }
            ensure!(
                call.id == identity.0 && call.name == identity.1,
                "hooks cannot change tool identity"
            );
            ensure!(
                !call.id.is_empty() && call.id.len() <= 256,
                "invalid tool call identity"
            );
            ensure!(
                serde_json::to_vec(&call.arguments)?.len() <= MAX_BYTES,
                "tool arguments exceed 1 MiB"
            );
            if let Some(plan) = &self.plugin_plan {
                effect.admission = Some(plan.admit(&mut call, events, self.gate_workspace.clone(), workspace_identity, self).await?);
            }
            self.validate_final_call(&call)?;
            events.admit_tool(&call)?;
            events
                .emit(Event::ToolStarted { call: call.clone() })
                .await?;
            if let Some(snapshot) = &self.access.snapshot {
                effect.start().await?;
                return Ok((snapshot.execute(&call)?, None));
            }
            match call.name.as_str() {
                "read" => {
                    let args: ReadArgs = serde_json::from_value(call.arguments.clone())?;
                    let mut file = self
                        .admitted_file(&call, &args.path, OFlags::RDONLY, false, &mut effect)
                        .await?;
                    effect.start().await?;
                    Ok((read_text(&mut file)?, None))
                }
                "write" => {
                    let args: WriteArgs = serde_json::from_value(call.arguments.clone())?;
                    let mut file = self
                        .admitted_file(&call, &args.path, OFlags::WRONLY, true, &mut effect)
                        .await?;
                    effect.start().await?;
                    write_text(&mut file, &args.content)?;
                    Ok((
                        format!("Wrote {} bytes to {}", args.content.len(), args.path),
                        None,
                    ))
                }
                "edit" => {
                    let args: EditArgs = serde_json::from_value(call.arguments.clone())?;
                    ensure!(!args.old_text.is_empty(), "old_text must not be empty");
                    let mut file = self
                        .admitted_file(&call, &args.path, OFlags::RDWR, false, &mut effect)
                        .await?;
                    let old = read_text(&mut file)?;
                    ensure!(
                        old.matches(&args.old_text).count() == 1,
                        "edit requires exactly one matching old_text"
                    );
                    let new = old.replacen(&args.old_text, &args.new_text, 1);
                    effect.start().await?;
                    write_text(&mut file, &new)?;
                    Ok((format!("Edited {}", args.path), None))
                }
                "bash" => {
                    let args: BashArgs = serde_json::from_value(call.arguments.clone())?;
                    ensure!(
                        !args.command.is_empty() && args.command.len() <= 65536,
                        "invalid Bash command size"
                    );
                    if self.access.unrestricted {
                        self.review(&call, None, None, events).await?;
                    }
                    self.bash(&call.id, &args.command, &mut effect).await
                }
                "lsp" => {
                    let args = serde_json::from_value(call.arguments.clone())?;
                    Ok((self.language_query(args, Some(&mut effect)).await?, None))
                }
                _ => {
                    let extension = self
                        .access
                        .extension
                        .as_ref()
                        .context("tool is not authorized")?;
                    ensure!(
                        extension
                            .definitions()
                            .iter()
                            .any(|definition| definition["name"] == call.name),
                        "tool is not authorized: {}",
                        call.name
                    );
                    effect.start().await?;
                    let output = extension.execute(&call, events).await?;
                    ensure!(output.len() <= MAX_BYTES, "parent tool result exceeds 1 MiB; inspect the retained agent record with /agent ID");
                    Ok((output, None))
                }
            }
        }
        .await;
        effect.guard.take();
        let mut result = match execution {
            Ok((output, exit_code)) => ToolResult {
                call_id: identity.0,
                tool: identity.1,
                success: exit_code.is_none_or(|v| v == 0),
                output,
                exit_code,
            },
            Err(error) => ToolResult {
                call_id: identity.0,
                tool: identity.1,
                success: false,
                output: format!("{error:#}"),
                exit_code: None,
            },
        };
        // No await separates completion from this receipt. The session can
        // recover it even if event delivery is cancelled or presentation fails.
        *self.completed.lock().expect("tool receipt lock poisoned") = Some(result.clone());
        events.original_tool_result(&result)?;
        if result.success
            && matches!(result.tool.as_str(), "write" | "edit")
            && let Some(path) = call.arguments["path"].as_str()
            && self
                .language_services
                .as_ref()
                .is_some_and(|manager| manager.watches(path))
        {
            // The completed mutation is recoverable before any diagnostic await.
            let feedback = self
                .language_query(crate::language_services::Args::diagnostics(path), None)
                .await;
            result.output.push_str("\nLanguage diagnostics: ");
            match feedback {
                Ok(output) => result.output.push_str(&output),
                Err(error) => result.output.push_str(&format!(
                    "unavailable or pending: {error:#}; verification not run"
                )),
            }
        }
        events.model_tool_result(&result)?;
        *self.completed.lock().expect("tool receipt lock poisoned") = Some(result.clone());
        if self.hooks.is_empty() || !effect.started {
            events.settle_tool()?;
        }
        // Presentation never replaces the original evidence.
        events
            .emit(Event::ToolFinished {
                result: result.clone(),
            })
            .await?;
        if !effect.started {
            return Ok(result);
        }
        for (index, hook) in self.hooks.iter().enumerate() {
            events.tool_observer(index, None)?;
            let text = match hook.present(&result) {
                Ok(text) => {
                    events.tool_observer(index, Some(Ok(&text)))?;
                    text
                }
                Err(error) => {
                    events.tool_observer(index, Some(Err(&format!("{error:#}"))))?;
                    return Err(error);
                }
            };
            if index + 1 == self.hooks.len() {
                events.settle_tool()?;
            }
            events
                .emit(Event::ToolPresentation {
                    call_id: result.call_id.clone(),
                    text,
                })
                .await?;
        }
        Ok(result)
    }

    fn validate_final_call(&self, call: &ToolCall) -> Result<()> {
        ensure!(
            serde_json::to_vec(&call.arguments)?.len() <= MAX_BYTES,
            "tool arguments exceed 1 MiB"
        );
        match call.name.as_str() {
            "read" => {
                let _: ReadArgs = serde_json::from_value(call.arguments.clone())?;
            }
            "write" => {
                let args: WriteArgs = serde_json::from_value(call.arguments.clone())?;
                ensure!(
                    args.content.len() <= MAX_BYTES,
                    "file content exceeds 1 MiB"
                );
            }
            "edit" => {
                let args: EditArgs = serde_json::from_value(call.arguments.clone())?;
                ensure!(!args.old_text.is_empty(), "old_text must not be empty");
            }
            "bash" => {
                let args: BashArgs = serde_json::from_value(call.arguments.clone())?;
                ensure!(
                    !args.command.is_empty() && args.command.len() <= 65536,
                    "invalid Bash command size"
                );
            }
            _ => ensure!(
                self.plugin_plan.is_none(),
                "plugin admission for extension and language-service tools requires their effect owner integration"
            ),
        }
        Ok(())
    }

    /// Normalize only the file tools whose effects this executor owns. O_PATH
    /// inspects identity without reading contents or creating/truncating a file;
    /// ordinary access checks and Oracle review still run in admitted_file.
    pub(crate) fn plugin_candidate(&self, call: &mut ToolCall) -> Result<Option<PluginTarget>> {
        if !matches!(call.name.as_str(), "read" | "write" | "edit") {
            return Ok(None);
        }
        let path = call.arguments["path"]
            .as_str()
            .context("file tool requires a path")?;
        ensure!(!path.is_empty() && path.len() <= 4096, "invalid tool path");
        let resolve = if self.access.strict_worktree {
            validate_path(path)?;
            RESOLVE | ResolveFlags::NO_XDEV
        } else if self.access.unrestricted {
            ResolveFlags::empty()
        } else if call.name == "read" {
            ResolveFlags::NO_MAGICLINKS
        } else {
            validate_path(path)?;
            RESOLVE
        };
        let opened = openat2(
            &*self.root,
            path,
            OFlags::PATH | OFlags::CLOEXEC,
            Mode::empty(),
            resolve,
        );
        let (anchor, leaf) = match opened {
            Ok(fd) => (File::from(fd), None),
            Err(rustix::io::Errno::NOENT) if call.name == "write" => {
                let path = Path::new(path);
                let name = path
                    .file_name()
                    .context("new file needs a name")?
                    .to_owned();
                let parent = path
                    .parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .unwrap_or(Path::new("."));
                let directory = openat2(
                    &*self.root,
                    parent,
                    OFlags::PATH | OFlags::DIRECTORY | OFlags::CLOEXEC,
                    Mode::empty(),
                    resolve,
                )
                .context("open existing parent directory for plugin path matching")?;
                (File::from(directory), Some(name))
            }
            Err(error) => {
                return Err(error).context("resolve plugin candidate with tool access policy");
            }
        };
        let target = PluginTarget {
            path: descriptor_path(&anchor)?,
            anchor,
            leaf,
        };
        let absolute = target
            .leaf
            .as_ref()
            .map_or_else(|| target.path.clone(), |leaf| target.path.join(leaf));
        let normalized = absolute.strip_prefix(&self.workspace).unwrap_or(&absolute);
        call.arguments["path"] = json!(normalized.to_str().context("tool target is not UTF-8")?);
        Ok(Some(target))
    }

    async fn language_query(
        &self,
        args: crate::language_services::Args,
        effect: Option<&mut ToolEffect<'_>>,
    ) -> Result<String> {
        let manager = self
            .language_services
            .as_ref()
            .context("language services are disabled for this session")?;
        let source = if let Some(path) = &args.path {
            Some(self.language_source(path).await?)
        } else {
            ensure!(args.is_status(), "language query requires a source path");
            None
        };
        let path = args.path.clone();
        let original = source.clone();
        if let Some(effect) = effect {
            effect.start().await?;
        }
        let output = manager.execute(args, source).await?;
        if let (Some(path), Some(original)) = (path, original) {
            ensure!(
                self.language_source(&path).await? == original,
                "language result is stale: source changed during the request; query again"
            );
        }
        Ok(output)
    }

    async fn language_source(&self, path: &str) -> Result<String> {
        validate_path(path)?;
        ensure!(
            !crate::export_policy::private_path(Path::new(path)),
            "language services cannot read protected source"
        );
        let file = openat2(
            &*self.root,
            path,
            OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NONBLOCK,
            Mode::empty(),
            RESOLVE,
        )?;
        let mut file = File::from(file);
        ensure!(
            file.metadata()?.nlink() == 1,
            "language source cannot be a hard link"
        );
        let resolved = descriptor_path(&file)?;
        for protected in crate::export_policy::private_roots(
            &self.workspace,
            &self.access.credential_paths,
            true,
        ) {
            ensure!(
                !resolved.starts_with(protected),
                "language source is protected"
            );
        }
        read_text(&mut file)
    }

    async fn open(
        &self,
        path: &str,
        flags: OFlags,
        create: bool,
        effect: &mut ToolEffect<'_>,
    ) -> Result<File> {
        validate_path(path)?;
        if let Some(worktree) = &self.worktree {
            worktree.check_path(&self.workspace.join(path))?;
        } else {
            self.developer
                .as_ref()
                .context("developer tools are disabled")?
                .check_mutation(&self.workspace.join(path))?;
        }
        // BENEATH alone permits bind mounts that alias files outside the tree.
        let resolve = if self.access.strict_worktree {
            RESOLVE | ResolveFlags::NO_XDEV
        } else {
            RESOLVE
        };
        let flags = flags | OFlags::CLOEXEC | OFlags::NONBLOCK;
        let fd = openat2(&*self.root, path, flags, Mode::empty(), resolve);
        let file = match fd {
            Ok(fd) => File::from(fd),
            Err(rustix::io::Errno::NOENT) if create => {
                effect.start().await?;
                // With a plugin plan, create relative to the parent that the
                // matchers inspected, not a second traversal of the request.
                let target = effect.admission.as_ref().and_then(|a| a.target.as_ref());
                let (root, path) = if let Some(target) = target {
                    (
                        &target.anchor,
                        Path::new(target.leaf.as_ref().context("plugin target disappeared")?),
                    )
                } else {
                    (&*self.root, Path::new(path))
                };
                File::from(
                    openat2(
                        root,
                        path,
                        flags | OFlags::CREATE | OFlags::EXCL,
                        Mode::RUSR | Mode::WUSR,
                        resolve,
                    )
                    .context("create file beneath workspace; parent directory must exist")?,
                )
            }
            Err(error) => {
                return Err(error).context("open file beneath workspace without symlinks");
            }
        };
        let meta = file.metadata()?;
        ensure!(
            meta.is_file() && meta.nlink() == 1,
            "tools require regular files without hard links"
        );
        ensure!(meta.len() <= MAX_BYTES as u64, "file exceeds 1 MiB");
        if !effect.started {
            effect.bind_target(&file, false)?;
        }
        Ok(file)
    }

    async fn admitted_file(
        &self,
        call: &ToolCall,
        path: &str,
        flags: OFlags,
        create: bool,
        effect: &mut ToolEffect<'_>,
    ) -> Result<File> {
        let events = effect.events;
        if self.access.strict_worktree {
            effect.lock().await;
            return self.open(path, flags, create, effect).await;
        }
        if !self.access.unrestricted {
            if flags == OFlags::RDONLY {
                let file = self
                    .developer
                    .as_ref()
                    .context("developer tools are disabled")?
                    .read(self.root.clone(), path.to_owned())
                    .await?;
                effect.bind_target(&file, false)?;
                return Ok(file);
            }
            effect.lock().await;
            return self.open(path, flags, create, effect).await;
        }
        ensure!(!path.is_empty() && path.len() <= 4096, "invalid tool path");
        let flags = flags | OFlags::CLOEXEC | OFlags::NONBLOCK;
        // Opening without CREATE or TRUNC obtains the real target before any
        // read/write effect. A symlink cannot change this descriptor afterward.
        match openat2(
            &*self.root,
            path,
            flags,
            Mode::empty(),
            ResolveFlags::empty(),
        ) {
            Ok(fd) => {
                let file = File::from(fd);
                let meta = file.metadata()?;
                ensure!(
                    meta.is_file() && meta.nlink() > 0 && meta.len() <= MAX_BYTES as u64,
                    "tools require a linked regular file up to 1 MiB"
                );
                let target = descriptor_path(&file)?;
                if !self.approved_path(&target) || meta.nlink() > 1 {
                    self.review(call, Some(&target), Some(meta.nlink()), events)
                        .await?;
                }
                ensure!(
                    descriptor_path(&file)? == target,
                    "tool target moved during admission; retry with its current path"
                );
                effect.bind_target(&file, false)?;
                effect.lock().await;
                Ok(file)
            }
            Err(rustix::io::Errno::NOENT) if create => {
                let path = Path::new(path);
                let name = path.file_name().context("new file needs a name")?;
                let parent = path
                    .parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .unwrap_or(Path::new("."));
                let directory = File::from(
                    openat2(
                        &*self.root,
                        parent,
                        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
                        Mode::empty(),
                        ResolveFlags::empty(),
                    )
                    .context("open existing parent directory")?,
                );
                let parent_path = descriptor_path(&directory)?;
                let target = parent_path.join(name);
                if !self.approved_path(&target) {
                    self.review(call, Some(&target), None, events).await?;
                }
                ensure!(
                    descriptor_path(&directory)? == parent_path,
                    "parent directory moved during admission; retry"
                );
                effect.bind_target(&directory, true)?;
                // No file is created before review. A concurrently created leaf
                // or symlink fails rather than changing the admitted target.
                effect.start().await?;
                Ok(File::from(
                    openat2(
                        &directory,
                        name,
                        flags | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW,
                        Mode::RUSR | Mode::WUSR,
                        RESOLVE,
                    )
                    .context("create the reviewed file without replacing an existing leaf")?,
                ))
            }
            Err(error) => Err(error).context("open host file"),
        }
    }

    fn approved_path(&self, path: &Path) -> bool {
        path.starts_with(&self.workspace)
            || self
                .scratch
                .as_ref()
                .is_some_and(|root| path.starts_with(root))
    }

    async fn review(
        &self,
        call: &ToolCall,
        target: Option<&Path>,
        links: Option<u64>,
        events: &EventSink,
    ) -> Result<()> {
        let config = self
            .access
            .oracle
            .as_ref()
            .context("outside access requires a configured Oracle")?;
        let reviewer = format!(
            "{} / {}",
            config.adapter,
            config.model.as_deref().unwrap_or("backend-default")
        );
        events
            .emit(Event::ToolReview {
                call_id: call.id.clone(),
                reviewer: reviewer.clone(),
                decision: "reviewing",
                reason: "Checking possible outside-project effects.".into(),
            })
            .await?;
        let intent = self
            .intent
            .lock()
            .expect("tool intent lock poisoned")
            .clone();
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let request = crate::oracle::ReviewRequest {
            developer_task: &intent,
            workspace: &self.workspace,
            scratch: self.scratch.as_deref(),
            home: home.as_deref(),
            proposed_tool: call,
            resolved_target: target,
            hard_link_count: links,
        };
        match crate::oracle::review(config, &request, events).await {
            Ok(decision) => {
                let allowed = decision.decision == crate::oracle::Verdict::Allow;
                events
                    .emit(Event::ToolReview {
                        call_id: call.id.clone(),
                        reviewer,
                        decision: if allowed { "allowed" } else { "blocked" },
                        reason: decision.reason.clone(),
                    })
                    .await?;
                ensure!(allowed, "Oracle blocked this request: {}", decision.reason);
                Ok(())
            }
            Err(error) => {
                let reason = format!("Oracle review unavailable: {error}");
                events
                    .emit(Event::ToolReview {
                        call_id: call.id.clone(),
                        reviewer,
                        decision: "blocked",
                        reason: reason.clone(),
                    })
                    .await?;
                bail!("{reason}")
            }
        }
    }

    async fn confined_command(&self, script: &str) -> Result<Command> {
        if let Some(worktree) = &self.worktree {
            return worktree
                .command(self.root.clone(), self.workspace.clone(), script.to_owned())
                .await;
        }
        self.developer
            .as_ref()
            .context("developer tools are disabled")?
            .command(self.root.clone(), self.workspace.clone(), script.to_owned())
            .await
    }

    async fn bash(
        &self,
        id: &str,
        script: &str,
        effect: &mut ToolEffect<'_>,
    ) -> Result<(String, Option<i32>)> {
        let events = effect.events;
        // Both modes start in the pinned project. Only explicit host access
        // takes this branch; a confined launch never falls back to it.
        let root_path = format!("/proc/{}/fd/{}", std::process::id(), self.root.as_raw_fd());
        let mut command = if self.access.unrestricted {
            let mut command = Command::new(
                self.access
                    .supervisor
                    .as_ref()
                    .context("host Bash requires a configured runtime supervisor")?,
            );
            command
                .args(["--supervise-bash", script])
                .current_dir(&root_path)
                .env_clear();
            for variable in [
                "PATH",
                "HOME",
                "USER",
                "LANG",
                "LC_ALL",
                "TZ",
                "XDG_CONFIG_HOME",
                "XDG_DATA_HOME",
                "SSH_AUTH_SOCK",
            ] {
                if let Some(value) = std::env::var_os(variable) {
                    command.env(variable, value);
                }
            }
            command.env("TMPDIR", self.scratch.as_ref().expect("host scratch"));
            command
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .process_group(0)
                .kill_on_drop(false);
            command
        } else {
            let mut command = self.confined_command(script).await?;
            command
                .env_clear()
                .stdin(Stdio::from(self.root.try_clone()?))
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true);
            command
        };
        effect.start().await?;
        let mut child = command.spawn().with_context(|| {
            if self.access.unrestricted {
                "start host Bash"
            } else {
                "Bash requires /usr/bin/bwrap; no host fallback"
            }
        })?;
        // Only this future owns the pipe writer. Cancellation or runtime death
        // closes it, allowing the independent supervisor to kill Bash and its descendants.
        let mut lifetime = child.stdin.take();
        if self.access.unrestricted {
            lifetime
                .as_mut()
                .context("missing host lifetime pipe")?
                .write_all(b"1")
                .await?;
        }
        let mut stdout = child.stdout.take().context("missing Bash stdout")?;
        let mut stderr = child.stderr.take().context("missing Bash stderr")?;
        let collect = async {
            let (mut out_open, mut err_open) = (true, true);
            let (mut out_buf, mut err_buf) = ([0u8; 4096], [0u8; 4096]);
            let mut output = String::new();
            let mut received_bytes = 0;
            let (mut out_decoder, mut err_decoder) =
                (Utf8Decoder::default(), Utf8Decoder::default());
            while out_open || err_open {
                let (stream, bytes) = tokio::select! {
                    n = stdout.read(&mut out_buf), if out_open => {
                        let n = n?; out_open = n != 0; ("stdout", &out_buf[..n])
                    },
                    n = stderr.read(&mut err_buf), if err_open => {
                        let n = n?; err_open = n != 0; ("stderr", &err_buf[..n])
                    },
                };
                ensure!(
                    bytes.len() <= MAX_BYTES - received_bytes,
                    "Bash output exceeds 1 MiB"
                );
                received_bytes += bytes.len();
                let decoder = if stream == "stdout" {
                    &mut out_decoder
                } else {
                    &mut err_decoder
                };
                let text = decoder.decode(bytes, bytes.is_empty());
                output.push_str(&text);
                if !text.is_empty() {
                    events
                        .emit(Event::ToolOutput {
                            call_id: id.into(),
                            stream,
                            text,
                        })
                        .await?;
                }
            }
            let status = child.wait().await?;
            Ok((output, Some(status.code().unwrap_or(-1))))
        };
        match tokio::time::timeout(Duration::from_secs(120), collect).await {
            Ok(result) => result,
            Err(_) => bail!("Bash exceeded its 120 second limit"),
        }
    }
}

fn descriptor_path(file: &File) -> Result<PathBuf> {
    std::fs::read_link(format!(
        "/proc/{}/fd/{}",
        std::process::id(),
        file.as_raw_fd()
    ))
    .context("resolve opened tool target")
}

fn validate_path(path: &str) -> Result<()> {
    ensure!(!path.is_empty() && path.len() <= 4096, "invalid tool path");
    for component in Path::new(path).components() {
        match component {
            Component::Normal(part) => ensure!(
                part != ".git" && part != ".demoncoder",
                "protected workspace path"
            ),
            _ => bail!("tool paths must be relative, without dot or parent components"),
        }
    }
    Ok(())
}

fn read_text(file: &mut File) -> Result<String> {
    let mut bytes = Vec::new();
    file.take((MAX_BYTES + 1) as u64).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= MAX_BYTES, "file exceeds 1 MiB");
    String::from_utf8(bytes).context("tool requires UTF-8 text")
}

fn write_text(file: &mut File, content: &str) -> Result<()> {
    ensure!(content.len() <= MAX_BYTES, "file content exceeds 1 MiB");
    file.seek(SeekFrom::Start(0))?;
    file.write_all(content.as_bytes())?;
    file.set_len(content.len() as u64)?;
    file.sync_data()?;
    Ok(())
}

pub fn definitions() -> Vec<Value> {
    [
        ("read", "Read a UTF-8 source or documentation file (up to 1 MiB). Absolute and relative paths are accepted, including outside the project. Private credentials and process state are protected.", json!({"path":{"type":"string"}}), vec!["path"]),
        ("write", "Create or replace a UTF-8 workspace file. Parent directory must exist.", json!({"path":{"type":"string"},"content":{"type":"string"}}), vec!["path","content"]),
        ("edit", "Replace exactly one occurrence of old_text in a workspace file.", json!({"path":{"type":"string"},"old_text":{"type":"string"},"new_text":{"type":"string"}}), vec!["path","old_text","new_text"]),
        ("bash", "Run Bash in the selected project's real path with normal network access and installed developer tools. Read source, documentation, Git state, and verification evidence. File writes are limited to the project, session TMPDIR, and approved build caches. Private credentials are protected and provider secrets are removed from the environment. Limit 120 seconds and 1 MiB output.", json!({"command":{"type":"string"}}), vec!["command"]),
    ].into_iter().map(|(name, description, properties, required)| json!({
        "name":name,"description":description,"input_schema":{"type":"object","properties":properties,"required":required,"additionalProperties":false}
    })).collect()
}

/// Keep only an incomplete code point between reads, separately for each pipe.
#[derive(Default)]
struct Utf8Decoder {
    pending: Vec<u8>,
}

impl Utf8Decoder {
    fn decode(&mut self, bytes: &[u8], eof: bool) -> String {
        self.pending.extend_from_slice(bytes);
        let mut text = String::new();
        let mut consumed = 0;
        while consumed < self.pending.len() {
            match std::str::from_utf8(&self.pending[consumed..]) {
                Ok(valid) => {
                    text.push_str(valid);
                    consumed = self.pending.len();
                }
                Err(error) => {
                    let valid_end = consumed + error.valid_up_to();
                    text.push_str(
                        std::str::from_utf8(&self.pending[consumed..valid_end])
                            .expect("validated UTF-8 prefix"),
                    );
                    consumed = valid_end;
                    match error.error_len() {
                        Some(length) => {
                            text.push('\u{fffd}');
                            consumed += length;
                        }
                        None => break,
                    }
                }
            }
        }
        self.pending.drain(..consumed);
        if eof && !self.pending.is_empty() {
            text.push('\u{fffd}');
            self.pending.clear();
        }
        text
    }
}

#[cfg(test)]
mod utf8_tests {
    use super::Utf8Decoder;

    #[test]
    fn every_partition_matches_whole_stream_lossy_decoding() {
        for bytes in [
            "Aé€🙂Z".as_bytes(),
            &[0xff, b'a', 0xe2, b'b', 0xf0, 0x9f],
            &[0xe0, 0x80, 0x80, 0xed, 0xa0, 0x80],
            &[0xf4, 0x90, 0x80, 0x80, 0xc3],
        ] {
            for boundaries in 0..(1 << (bytes.len() - 1)) {
                let mut decoder = Utf8Decoder::default();
                let mut decoded = String::new();
                let mut start = 0;
                for end in 1..=bytes.len() {
                    if end == bytes.len() || boundaries & (1 << (end - 1)) != 0 {
                        decoded.push_str(&decoder.decode(&bytes[start..end], false));
                        assert!(decoder.pending.len() <= 3);
                        start = end;
                    }
                }
                decoded.push_str(&decoder.decode(&[], true));
                assert_eq!(decoded, String::from_utf8_lossy(bytes));
                assert!(decoder.pending.is_empty());
                assert!(decoder.decode(&[], true).is_empty());
            }
        }
    }
}

#[cfg(test)]
mod plugin_target_tests;
