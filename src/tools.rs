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

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
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

pub struct ToolExecutor {
    root: Arc<File>,
    workspace: PathBuf,
    scratch: Option<PathBuf>,
    access: AccessPolicy,
    developer: Option<Arc<crate::developer_access::DeveloperAccess>>,
    worktree: Option<Arc<crate::worktree_access::WorktreeAccess>>,
    intent: Mutex<String>,
    hooks: Vec<Box<dyn ToolHook>>,
    // Execution is sequential. Keep the current receipt across cancellation
    // during event delivery or a presentation error; never retain a full copy
    // of the session history here.
    completed: Mutex<Option<ToolResult>>,
}

impl ToolExecutor {
    pub fn new(workspace: &Path) -> Result<Self> {
        Self::with_policy(workspace, &AccessPolicy::default())
    }

    pub fn with_policy(workspace: &Path, access: &AccessPolicy) -> Result<Self> {
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
        Ok(Self {
            root: Arc::new(root),
            workspace: workspace.canonicalize().context("resolve tool workspace")?,
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
            developer: if !access.unrestricted && !access.strict_worktree && access.tools_enabled {
                Some(Arc::new(crate::developer_access::DeveloperAccess::new(
                    &workspace
                        .canonicalize()
                        .context("resolve developer workspace")?,
                    &access.credential_paths,
                )?))
            } else {
                None
            },
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
            completed: Mutex::new(None),
        })
    }

    pub fn definitions(&self) -> Vec<Value> {
        if !self.access.tools_enabled {
            return Vec::new();
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
        self.take_completed();
        let identity = (call.id.clone(), call.name.clone());
        let execution = async {
            ensure!(self.access.tools_enabled, "the Oracle cannot execute tools");
            for hook in &self.hooks {
                hook.before(&mut call)?;
            }
            ensure!(
                call.id == identity.0,
                "hooks cannot change tool call identity"
            );
            ensure!(
                !call.id.is_empty() && call.id.len() <= 256,
                "invalid tool call identity"
            );
            ensure!(
                serde_json::to_vec(&call.arguments)?.len() <= MAX_BYTES,
                "tool arguments exceed 1 MiB"
            );
            events
                .emit(Event::ToolStarted { call: call.clone() })
                .await?;
            match call.name.as_str() {
                "read" => {
                    let args: ReadArgs = serde_json::from_value(call.arguments.clone())?;
                    let mut file = self
                        .admitted_file(&call, &args.path, OFlags::RDONLY, false, events)
                        .await?;
                    Ok((read_text(&mut file)?, None))
                }
                "write" => {
                    let args: WriteArgs = serde_json::from_value(call.arguments.clone())?;
                    let mut file = self
                        .admitted_file(&call, &args.path, OFlags::WRONLY, true, events)
                        .await?;
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
                        .admitted_file(&call, &args.path, OFlags::RDWR, false, events)
                        .await?;
                    let old = read_text(&mut file)?;
                    ensure!(
                        old.matches(&args.old_text).count() == 1,
                        "edit requires exactly one matching old_text"
                    );
                    let new = old.replacen(&args.old_text, &args.new_text, 1);
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
                    self.bash(&call.id, &args.command, events).await
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
                    let output = extension.execute(&call, events).await?;
                    ensure!(output.len() <= MAX_BYTES, "parent tool result exceeds 1 MiB; inspect the retained agent record with /agent ID");
                    Ok((output, None))
                }
            }
        }
        .await;
        let result = match execution {
            Ok((output, exit_code)) => ToolResult {
                call_id: identity.0,
                tool: call.name,
                success: exit_code.is_none_or(|v| v == 0),
                output,
                exit_code,
            },
            Err(error) => ToolResult {
                call_id: identity.0,
                tool: call.name,
                success: false,
                output: format!("{error:#}"),
                exit_code: None,
            },
        };
        // No await separates completion from this receipt. The session can
        // recover it even if event delivery is cancelled or presentation fails.
        *self.completed.lock().expect("tool receipt lock poisoned") = Some(result.clone());
        // The actual result is retained first. Presentation never replaces evidence.
        events
            .emit(Event::ToolFinished {
                result: result.clone(),
            })
            .await?;
        for hook in &self.hooks {
            events
                .emit(Event::ToolPresentation {
                    call_id: result.call_id.clone(),
                    text: hook.present(&result)?,
                })
                .await?;
        }
        Ok(result)
    }

    fn open(&self, path: &str, flags: OFlags, create: bool) -> Result<File> {
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
            Err(rustix::io::Errno::NOENT) if create => File::from(
                openat2(
                    &*self.root,
                    path,
                    flags | OFlags::CREATE | OFlags::EXCL,
                    Mode::RUSR | Mode::WUSR,
                    resolve,
                )
                .context("create file beneath workspace; parent directory must exist")?,
            ),
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
        Ok(file)
    }

    async fn admitted_file(
        &self,
        call: &ToolCall,
        path: &str,
        flags: OFlags,
        create: bool,
        events: &EventSink,
    ) -> Result<File> {
        if self.access.strict_worktree {
            return self.open(path, flags, create);
        }
        if !self.access.unrestricted {
            if flags == OFlags::RDONLY {
                return self
                    .developer
                    .as_ref()
                    .context("developer tools are disabled")?
                    .read(self.root.clone(), path.to_owned())
                    .await;
            }
            return self.open(path, flags, create);
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
                // No file is created before review. A concurrently created leaf
                // or symlink fails rather than changing the admitted target.
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
        events: &EventSink,
    ) -> Result<(String, Option<i32>)> {
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
