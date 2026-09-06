//! The shared tool boundary. Provider payloads become typed requests only here.
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    os::{fd::AsRawFd, unix::fs::MetadataExt},
    path::{Component, Path},
    process::Stdio,
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result, bail, ensure};
use rustix::fs::{Mode, OFlags, ResolveFlags, openat2};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::{io::AsyncReadExt, process::Command};

use crate::events::{Event, EventSink};

const MAX_BYTES: usize = 1024 * 1024;
const RESOLVE: ResolveFlags = ResolveFlags::BENEATH.union(ResolveFlags::NO_SYMLINKS);

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

pub struct ToolExecutor {
    root: Arc<File>,
    hooks: Vec<Box<dyn ToolHook>>,
}

impl ToolExecutor {
    pub fn new(workspace: &Path) -> Result<Self> {
        ensure!(cfg!(target_os = "linux"), "coding tools require Linux");
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
            hooks: Vec::new(),
        })
    }

    pub fn add_hook(&mut self, hook: Box<dyn ToolHook>) {
        self.hooks.push(hook);
    }

    pub async fn execute(&self, mut call: ToolCall, events: &EventSink) -> Result<ToolResult> {
        let identity = (call.id.clone(), call.name.clone());
        let execution = async {
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
                    Ok((self.read(&args.path)?, None))
                }
                "write" => {
                    let args: WriteArgs = serde_json::from_value(call.arguments.clone())?;
                    self.write(&args.path, &args.content)?;
                    Ok((
                        format!("Wrote {} bytes to {}", args.content.len(), args.path),
                        None,
                    ))
                }
                "edit" => {
                    let args: EditArgs = serde_json::from_value(call.arguments.clone())?;
                    ensure!(!args.old_text.is_empty(), "old_text must not be empty");
                    let mut file = self.open(&args.path, OFlags::RDWR, false)?;
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
                    self.bash(&call.id, &args.command, events).await
                }
                _ => bail!("tool is not authorized: {}", call.name),
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
                output: error.to_string(),
                exit_code: None,
            },
        };
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
        let flags = flags | OFlags::CLOEXEC | OFlags::NONBLOCK;
        let fd = openat2(&*self.root, path, flags, Mode::empty(), RESOLVE);
        let file = match fd {
            Ok(fd) => File::from(fd),
            Err(rustix::io::Errno::NOENT) if create => File::from(
                openat2(
                    &*self.root,
                    path,
                    flags | OFlags::CREATE | OFlags::EXCL,
                    Mode::RUSR | Mode::WUSR,
                    RESOLVE,
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

    fn read(&self, path: &str) -> Result<String> {
        read_text(&mut self.open(path, OFlags::RDONLY, false)?)
    }
    fn write(&self, path: &str, content: &str) -> Result<()> {
        write_text(&mut self.open(path, OFlags::WRONLY, true)?, content)
    }

    async fn bash(
        &self,
        id: &str,
        script: &str,
        events: &EventSink,
    ) -> Result<(String, Option<i32>)> {
        // Pin the mount to the authorized descriptor, not a path that can be swapped.
        let root_path = format!("/proc/{}/fd/{}", std::process::id(), self.root.as_raw_fd());
        check_tree(Path::new(&root_path), 0, &mut 0)?;
        let mut command = Command::new("/usr/bin/bwrap");
        command.args([
            "--unshare-all",
            "--die-with-parent",
            "--new-session",
            "--ro-bind",
            "/usr",
            "/usr",
            "--symlink",
            "usr/bin",
            "/bin",
            "--symlink",
            "usr/lib",
            "/lib",
            "--symlink",
            "usr/lib64",
            "/lib64",
            "--proc",
            "/proc",
            "--dev",
            "/dev",
            "--tmpfs",
            "/tmp",
            "--tmpfs",
            "/home",
            "--bind-fd",
            "0",
            "/workspace",
        ]);
        if Path::new(&root_path).join(".git").exists() {
            command.args(["--ro-bind", "/proc/self/fd/0/.git", "/workspace/.git"]);
        }
        command.args([
            "--chdir",
            "/workspace",
            "--clearenv",
            "--setenv",
            "PATH",
            "/usr/bin:/bin",
            "--setenv",
            "HOME",
            "/home",
            "--setenv",
            "LANG",
            "C.UTF-8",
            "/bin/bash",
            "--noprofile",
            "--norc",
            "-c",
            script,
        ]);
        command
            .env_clear()
            .stdin(Stdio::from(self.root.try_clone()?))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = command
            .spawn()
            .context("Bash requires /usr/bin/bwrap; no host fallback")?;
        let mut stdout = child.stdout.take().context("missing Bash stdout")?;
        let mut stderr = child.stderr.take().context("missing Bash stderr")?;
        let collect = async {
            let (mut out_open, mut err_open) = (true, true);
            let (mut out_buf, mut err_buf) = ([0u8; 4096], [0u8; 4096]);
            let mut output = Vec::new();
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
                    output.len() + bytes.len() <= MAX_BYTES,
                    "Bash output exceeds 1 MiB"
                );
                output.extend_from_slice(bytes);
                if !bytes.is_empty() {
                    events
                        .emit(Event::ToolOutput {
                            call_id: id.into(),
                            stream,
                            text: String::from_utf8_lossy(bytes).into_owned(),
                        })
                        .await?;
                }
            }
            let status = child.wait().await?;
            Ok((
                String::from_utf8_lossy(&output).into_owned(),
                Some(status.code().unwrap_or(-1)),
            ))
        };
        match tokio::time::timeout(Duration::from_secs(120), collect).await {
            Ok(result) => result,
            Err(_) => bail!("Bash exceeded its 120 second limit"),
        }
    }
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

fn check_tree(path: &Path, depth: usize, count: &mut usize) -> Result<()> {
    ensure!(
        depth < 64 && *count < 100_000,
        "workspace exceeds Bash inspection limit"
    );
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        *count += 1;
        let meta = entry.path().symlink_metadata()?;
        ensure!(
            !meta.is_symlink(),
            "Bash workspace must not contain symlinks"
        );
        if meta.is_dir() {
            check_tree(&entry.path(), depth + 1, count)?;
        } else {
            ensure!(
                meta.is_file() && meta.nlink() == 1,
                "Bash workspace must contain regular files without hard links"
            );
        }
    }
    Ok(())
}

pub fn definitions() -> Vec<Value> {
    [
        ("read", "Read a UTF-8 workspace file (up to 1 MiB).", json!({"path":{"type":"string"}}), vec!["path"]),
        ("write", "Create or replace a UTF-8 workspace file. Parent directory must exist.", json!({"path":{"type":"string"},"content":{"type":"string"}}), vec!["path","content"]),
        ("edit", "Replace exactly one occurrence of old_text in a workspace file.", json!({"path":{"type":"string"},"old_text":{"type":"string"},"new_text":{"type":"string"}}), vec!["path","old_text","new_text"]),
        ("bash", "Run Bash in the workspace with isolated filesystem, network and environment. Limit 120 seconds and 1 MiB output.", json!({"command":{"type":"string"}}), vec!["command"]),
    ].into_iter().map(|(name, description, properties, required)| json!({
        "name":name,"description":description,"input_schema":{"type":"object","properties":properties,"required":required,"additionalProperties":false}
    })).collect()
}
