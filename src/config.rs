use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use clap::Parser;
use serde::{Deserialize, Serialize};

#[derive(Parser)]
#[command(version, about, disable_version_flag = true)]
pub struct Args {
    /// Internal host-tool process lifetime protocol.
    #[arg(long, hide = true, allow_hyphen_values = true)]
    pub supervise_bash: Option<String>,
    /// Print the application version.
    #[arg(short = 'v', long = "version", visible_short_alias = 'V', action = clap::ArgAction::Version)]
    pub version: Option<bool>,
    /// Workspace where coding tools may change files.
    #[arg(long, default_value = ".")]
    pub workspace: PathBuf,
    /// A trusted connection configuration file. Repository files are not loaded automatically.
    #[arg(long)]
    pub config: Option<PathBuf>,
    /// Connection name from the configuration, or a built-in adapter name.
    #[arg(long)]
    pub connection: Option<String>,
    /// Model identifier accepted by the selected connection.
    #[arg(long)]
    pub model: Option<String>,
    /// Known context capacity for display; does not change provider limits.
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
    pub context_window: Option<u64>,
    /// Reasoning effort accepted by the selected connection and model.
    #[arg(long)]
    pub effort: Option<String>,
    /// Maximum response tokens for native API connections; omission uses model/provider defaults.
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    pub max_output_tokens: Option<u32>,
    /// Reopen guided provider, model, Oracle, and project setup.
    #[arg(long)]
    pub setup: bool,
    /// Run tools on the host without sandboxing or routine prompts. Outside access requires the Oracle.
    #[arg(long)]
    pub yolo: bool,
    /// Explicitly authorize this invocation's workspace, without saving permanent trust.
    #[arg(long)]
    pub trust_workspace: bool,
    /// Save session events to a new file; may contain prompts and model output.
    #[arg(long)]
    pub event_log: Option<PathBuf>,
    /// Verification command selected for explicit /task work; repeat for several checks.
    #[arg(long = "check")]
    pub checks: Vec<String>,
    /// Configured connection that reviews the actual patch without tools.
    #[arg(long)]
    pub reviewer: Option<String>,
    /// Maximum correction rounds for each explicit task.
    #[arg(long, default_value_t = 2, value_parser = clap::value_parser!(u32).range(0..=20))]
    pub correction_rounds: u32,
    /// Resume a private session directory printed by an earlier invocation.
    #[arg(long)]
    pub resume: Option<PathBuf>,
    /// Cumulative deadline for explicit task work, including review and correction.
    #[arg(long, default_value_t = 900, value_parser = clap::value_parser!(u64).range(1..=86400))]
    pub task_seconds: u64,
    /// Total model calls across worker, Oracle, reviewer and correction.
    #[arg(long, default_value_t = 64, value_parser = clap::value_parser!(u64).range(1..=4096))]
    pub task_model_calls: u64,
    /// Total tool admissions across worker and verification.
    #[arg(long, default_value_t = 128, value_parser = clap::value_parser!(u64).range(1..=4096))]
    pub task_tool_calls: u64,
    /// Make a named connection available for confined worktree subagents; repeat as needed.
    #[arg(long = "agent-connection")]
    pub agent_connections: Vec<String>,
    /// Maximum active child assignments; does not increase the shared task allowance.
    #[arg(long, default_value_t = 2, value_parser = clap::value_parser!(u32).range(1..=8))]
    pub agent_limit: u32,
    /// Cumulative external backend invocations; internal model calls remain unavailable.
    #[arg(long, default_value_t = 64, value_parser = clap::value_parser!(u64).range(1..=4096))]
    pub agent_backend_turns: u64,
    /// Requested hard total token cap; rejected when the adapter cannot enforce it.
    #[arg(long)]
    pub task_token_limit: Option<u64>,
    /// Requested hard monetary cap; rejected when pricing or enforcement is unavailable.
    #[arg(long)]
    pub task_cost_limit: Option<f64>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Connection {
    pub adapter: String,
    pub model: Option<String>,
    /// Full HTTP endpoint; HTTPS or a literal loopback address for local servers.
    pub endpoint: Option<String>,
    /// Executable for an external backend. This is trusted configuration.
    pub binary: Option<PathBuf>,
    pub effort: Option<String>,
    /// Explicit native API output limit. None preserves model/provider defaults.
    pub max_output_tokens: Option<u32>,
    /// Trusted API credential. Never include this structure in events or diagnostics.
    pub api_key: Option<String>,
    /// Resolved by the host. Repository/provider payloads cannot set access policy.
    #[serde(skip)]
    pub access: crate::tools::AccessPolicy,
}

impl Connection {
    pub fn api_key(&self, variable: &str) -> Result<String> {
        match std::env::var(variable) {
            Ok(value) if !value.trim().is_empty() => Ok(value.trim().to_owned()),
            Ok(_) => bail!("{variable} is empty; set it or unset it to use the saved API key"),
            Err(std::env::VarError::NotUnicode(_)) => bail!("{variable} must be UTF-8"),
            Err(std::env::VarError::NotPresent) => self
                .api_key
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .map(|value| value.trim().to_owned())
                .with_context(|| {
                    format!("set {variable} or api_key in the private connection settings")
                }),
        }
    }

    pub fn validate(&self) -> Result<()> {
        if let Some(limit) = self.max_output_tokens {
            anyhow::ensure!(limit > 0, "max_output_tokens must be positive");
            anyhow::ensure!(
                matches!(self.adapter.as_str(), "anthropic-api" | "openai-api"),
                "max_output_tokens is supported only by native API connections; the selected backend owns its output limits"
            );
        }
        anyhow::ensure!(
            self.model
                .as_ref()
                .is_none_or(|model| !model.trim().is_empty()),
            "model must not be empty"
        );
        if matches!(self.adapter.as_str(), "codex" | "claude") && self.api_key.is_some() {
            bail!(
                "subscription connections do not accept api_key; use the backend subscription login"
            );
        }
        if let Some(effort) = &self.effort {
            let supported: &[&str] = match self.adapter.as_str() {
                "openai-api" => &[
                    "none",
                    "minimal",
                    "low",
                    "medium",
                    "high",
                    "xhigh",
                    "max",
                    "ultra",
                    "persistent",
                ],
                "codex" => &[
                    "none",
                    "minimal",
                    "low",
                    "medium",
                    "high",
                    "xhigh",
                    "max",
                    "ultra",
                    "persistent",
                ],
                "anthropic-api" | "claude" => &["low", "medium", "high", "xhigh", "max"],
                _ => bail!("reasoning effort is not declared for this adapter"),
            };
            anyhow::ensure!(
                supported.contains(&effort.as_str()),
                "unsupported reasoning effort for the selected adapter"
            );
        }
        Ok(())
    }
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Config {
    pub default_connection: Option<String>,
    #[serde(default)]
    pub connections: BTreeMap<String, Connection>,
    #[serde(default)]
    pub onboarding_complete: bool,
    #[serde(default)]
    pub trusted_workspaces: Vec<PathBuf>,
    pub oracle: Option<OracleAssignment>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OracleAssignment {
    pub connection: String,
    pub model: Option<String>,
    pub effort: Option<String>,
}

pub(crate) fn read_config(path: &Path) -> Result<Config> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(
            (rustix::fs::OFlags::NONBLOCK | rustix::fs::OFlags::NOFOLLOW).bits() as i32,
        );
    }
    let file = options
        .open(path)
        .context("open connection configuration")?;
    let metadata = file
        .metadata()
        .context("inspect connection configuration")?;
    anyhow::ensure!(
        metadata.is_file(),
        "connection configuration must be a regular file"
    );
    let mut bytes = Vec::new();
    file.take(64 * 1024 + 1)
        .read_to_end(&mut bytes)
        .context("read connection configuration")?;
    anyhow::ensure!(
        bytes.len() <= 64 * 1024,
        "connection configuration exceeds 64 KiB"
    );
    // TOML errors quote input, which may contain credentials.
    let config: Config =
        toml::from_str(std::str::from_utf8(&bytes).context("configuration must be UTF-8")?)
            .map_err(|_| anyhow::anyhow!("invalid connection configuration"))?;
    if config
        .connections
        .values()
        .any(|connection| connection.api_key.is_some())
    {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            anyhow::ensure!(
                metadata.uid() == rustix::process::getuid().as_raw()
                    && metadata.mode() & 0o077 == 0,
                "configuration containing API keys must belong to the current user and have owner-only permissions (chmod 600)"
            );
        }
        #[cfg(not(unix))]
        bail!(
            "private API-key file permissions cannot be verified on this platform; use environment credentials"
        );
    }
    Ok(config)
}

pub struct Selection {
    pub name: String,
    pub connection: Connection,
    pub workspace: PathBuf,
}

impl Args {
    pub fn workflow_settings(&self) -> Result<crate::workflow::Settings> {
        anyhow::ensure!(
            self.task_token_limit.is_none() && self.task_cost_limit.is_none(),
            "hard cumulative token and monetary caps cannot be enforced by these adapters; use --task-seconds, --task-model-calls and --task-tool-calls"
        );
        let config = self.load_config()?;
        let reviewer = self
            .reviewer
            .as_ref()
            .map(|name| {
                let mut connection = config
                    .connections
                    .get(name)
                    .cloned()
                    .context("reviewer connection is not configured")?;
                connection.validate()?;
                connection.access = crate::tools::AccessPolicy::review_only();
                Ok::<_, anyhow::Error>(connection)
            })
            .transpose()?;
        Ok(crate::workflow::Settings {
            checks: self.checks.clone(),
            reviewer,
            correction_limit: self.correction_rounds,
            limits: crate::workflow::allocation::Limits {
                seconds: self.task_seconds,
                model_calls: self.task_model_calls,
                tool_calls: self.task_tool_calls,
            },
        })
    }

    pub fn agent_settings(&self) -> Result<Option<crate::subagents::Settings>> {
        if self.agent_connections.is_empty() {
            return Ok(None);
        }
        anyhow::ensure!(
            self.agent_connections.len() <= 16,
            "at most sixteen agent connections may be enabled"
        );
        let config = self.load_config()?;
        let mut connections = BTreeMap::new();
        for name in &self.agent_connections {
            let mut connection = match config.connections.get(name) {
                Some(connection) => connection.clone(),
                None if ["openai-api", "anthropic-api", "codex", "claude"]
                    .contains(&name.as_str()) =>
                {
                    serde_json::from_value(serde_json::json!({"adapter":name}))?
                }
                None => bail!("agent connection is not configured: {name}"),
            };
            connection.validate()?;
            connection.access =
                crate::tools::AccessPolicy::worktree_only(self.config_path().into_iter().collect());
            connections.insert(name.clone(), connection);
        }
        let workflow = self.workflow_settings()?;
        Ok(Some(crate::subagents::Settings {
            connections,
            reviewer: workflow.reviewer,
            checks: workflow.checks,
            limits: workflow.limits,
            max_active: self.agent_limit,
            backend_limit: self.agent_backend_turns,
        }))
    }

    pub(crate) fn config_path(&self) -> Option<PathBuf> {
        self.config.clone().or_else(|| {
            std::env::var_os("HOME")
                .filter(|home| !home.is_empty())
                .map(|home| PathBuf::from(home).join(".demoncoder/settings.toml"))
        })
    }

    pub(crate) fn load_config(&self) -> Result<Config> {
        match self.config_path() {
            Some(path)
                if self.config.is_some()
                    || path.try_exists().context("inspect home settings")? =>
            {
                read_config(&path)
            }
            _ => Ok(Config::default()),
        }
    }

    pub fn selection(&self) -> Result<Selection> {
        let config = self.load_config()?;
        let name = self
            .connection
            .clone()
            .or(config.default_connection)
            .context("select --connection openai-api, anthropic-api, codex, or claude")?;
        let mut connection = match config.connections.get(&name) {
            Some(connection) => connection.clone(),
            None if ["openai-api", "anthropic-api", "codex", "claude"].contains(&name.as_str()) => {
                Connection {
                    adapter: name.clone(),
                    model: None,
                    endpoint: None,
                    binary: None,
                    effort: None,
                    max_output_tokens: None,
                    api_key: None,
                    access: crate::tools::AccessPolicy::default(),
                }
            }
            None => bail!("connection is not registered in the configuration"),
        };
        if let Some(model) = &self.model {
            connection.model = Some(model.clone());
        }
        if let Some(effort) = &self.effort {
            connection.effort = Some(effort.clone());
        }
        if let Some(limit) = self.max_output_tokens {
            connection.max_output_tokens = Some(limit);
        }
        connection.validate()?;
        let workspace = self.workspace.canonicalize().context("resolve workspace")?;
        if !workspace.is_dir() {
            bail!("workspace must be a directory");
        }
        anyhow::ensure!(
            self.yolo
                || self.trust_workspace
                || config
                    .trusted_workspaces
                    .iter()
                    .any(|root| workspace.starts_with(root)),
            "project is not trusted; use guided setup or explicit --trust-workspace authorization"
        );
        connection.access.unrestricted = self.yolo;
        if self.yolo {
            connection.access.supervisor =
                Some(std::env::current_exe().context("resolve host tool supervisor executable")?);
        }
        if self.config.is_none()
            || config
                .connections
                .values()
                .any(|value| value.api_key.is_some())
        {
            connection.access.credential_paths = self.config_path().into_iter().collect();
        }
        if self.yolo {
            let assignment = config
                .oracle
                .context("--yolo requires an Oracle assignment; run --setup")?;
            let mut oracle = config
                .connections
                .get(&assignment.connection)
                .cloned()
                .context("Oracle connection is not configured")?;
            if assignment.model.is_some() {
                oracle.model = assignment.model;
            }
            if assignment.effort.is_some() {
                oracle.effort = assignment.effort;
            }
            oracle.validate()?;
            oracle.access = crate::tools::AccessPolicy::review_only();
            connection.access.oracle = Some(Box::new(oracle));
        }
        Ok(Selection {
            name,
            connection,
            workspace,
        })
    }
}
