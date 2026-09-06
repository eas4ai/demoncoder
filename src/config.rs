use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use clap::Parser;
use serde::Deserialize;

#[derive(Parser)]
#[command(version, about)]
pub struct Args {
    /// Workspace authorized for this session.
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
    /// Reasoning effort accepted by the selected connection and model.
    #[arg(long)]
    pub effort: Option<String>,
    /// Save session events to a new file; may contain prompts and model output.
    #[arg(long)]
    pub event_log: Option<PathBuf>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Connection {
    pub adapter: String,
    pub model: Option<String>,
    /// Full HTTP endpoint; HTTPS or a literal loopback address for local servers.
    pub endpoint: Option<String>,
    /// Executable for an external backend. This is trusted configuration.
    pub binary: Option<PathBuf>,
    pub effort: Option<String>,
    /// Trusted API credential. Never include this structure in events or diagnostics.
    pub api_key: Option<String>,
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

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    default_connection: Option<String>,
    #[serde(default)]
    connections: BTreeMap<String, Connection>,
}

fn read_config(path: &Path) -> Result<Config> {
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
    pub fn selection(&self) -> Result<Selection> {
        let home_path = std::env::var_os("HOME")
            .filter(|home| !home.is_empty())
            .map(|home| PathBuf::from(home).join(".demoncoder/settings.toml"));
        let config = match self.config.as_deref().or(home_path.as_deref()) {
            Some(path)
                if self.config.is_some()
                    || path.try_exists().context("inspect home settings")? =>
            {
                read_config(path)?
            }
            _ => Config::default(),
        };
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
                    api_key: None,
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
        connection.validate()?;
        if connection
            .model
            .as_ref()
            .is_some_and(|model| model.trim().is_empty())
        {
            bail!("model must not be empty");
        }
        let workspace = self.workspace.canonicalize().context("resolve workspace")?;
        if !workspace.is_dir() {
            bail!("workspace must be a directory");
        }
        Ok(Selection {
            name,
            connection,
            workspace,
        })
    }
}
