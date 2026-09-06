use std::{collections::BTreeMap, path::PathBuf};

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
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    default_connection: Option<String>,
    #[serde(default)]
    connections: BTreeMap<String, Connection>,
}

pub struct Selection {
    pub name: String,
    pub connection: Connection,
    pub workspace: PathBuf,
}

impl Args {
    pub fn selection(&self) -> Result<Selection> {
        let config: Config = match &self.config {
            Some(path) => {
                let bytes = std::fs::read(path).context("read connection configuration")?;
                if bytes.len() > 64 * 1024 {
                    bail!("connection configuration exceeds 64 KiB");
                }
                // Parser errors can quote input. Configuration can contain accidental secrets.
                toml::from_str(std::str::from_utf8(&bytes).context("configuration must be UTF-8")?)
                    .map_err(|_| anyhow::anyhow!("invalid connection configuration"))?
            }
            None => Config::default(),
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
                }
            }
            None => bail!("connection is not registered in the configuration"),
        };
        if let Some(model) = &self.model {
            connection.model = Some(model.clone());
        }
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
