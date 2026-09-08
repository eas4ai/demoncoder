//! Saved defaults are separate from connections captured by admitted work.
mod editor;
pub(crate) mod probe;
mod store;

use std::collections::BTreeMap;

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use crate::config::{Config, Connection};
pub(crate) use editor::{Action, Editor, onboarding};
pub(crate) use store::Draft;
pub use store::Handle;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Role {
    Creator,
    Worker,
    Oracle,
    Reviewer,
    Advisor,
    Judge,
}

impl Role {
    pub const ALL: [Self; 6] = [
        Self::Creator,
        Self::Worker,
        Self::Oracle,
        Self::Reviewer,
        Self::Advisor,
        Self::Judge,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Creator => "Creator",
            Self::Worker => "Worker",
            Self::Oracle => "Oracle",
            Self::Reviewer => "Reviewer",
            Self::Advisor => "Advisor",
            Self::Judge => "Judge",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Creator => "Your coding conversation",
            Self::Worker => "Assigned work in an isolated worktree",
            Self::Oracle => "Outside-access decisions",
            Self::Reviewer => "Review of the actual patch",
            Self::Advisor => "Advice on delegated work",
            Self::Judge => "Decision on disputed advice",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Assignment {
    pub connection: String,
    pub model: Option<String>,
    pub effort: Option<String>,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Assignments {
    pub providers: Vec<String>,
    pub creator: Option<Assignment>,
    #[serde(default)]
    pub overrides: BTreeMap<Role, Assignment>,
}

impl Assignments {
    pub fn from_config(config: &Config) -> Self {
        if let Some(settings) = &config.settings {
            return settings.clone();
        }
        let creator = config.default_connection.as_ref().and_then(|name| {
            config.connections.get(name).map(|c| Assignment {
                connection: name.clone(),
                model: c.model.clone(),
                effort: c.effort.clone(),
            })
        });
        let mut overrides = BTreeMap::new();
        if let Some(oracle) = &config.oracle {
            let connection = config.connections.get(&oracle.connection);
            overrides.insert(
                Role::Oracle,
                Assignment {
                    connection: oracle.connection.clone(),
                    model: oracle
                        .model
                        .clone()
                        .or_else(|| connection.and_then(|c| c.model.clone())),
                    effort: oracle
                        .effort
                        .clone()
                        .or_else(|| connection.and_then(|c| c.effort.clone())),
                },
            );
        }
        Self {
            providers: config.connections.keys().cloned().collect(),
            creator,
            overrides,
        }
    }

    pub fn assignment(&self, role: Role) -> Option<&Assignment> {
        if role == Role::Creator {
            self.creator.as_ref()
        } else {
            self.overrides.get(&role).or(self.creator.as_ref())
        }
    }

    pub fn resolve(&self, config: &Config, role: Role) -> Result<Connection> {
        let assignment = self
            .assignment(role)
            .context("choose a Creator model in Settings")?;
        ensure!(
            self.providers.contains(&assignment.connection),
            "{} provider is deselected; repair its assignment in Settings",
            role.name()
        );
        let mut connection = config
            .connections
            .get(&assignment.connection)
            .cloned()
            .context("assigned connection is missing; repair it in Settings")?;
        connection.model = assignment.model.clone();
        connection.effort = assignment.effort.clone();
        connection.validate()?;
        Ok(connection)
    }

    pub fn validate(&self, config: &Config) -> Result<()> {
        ensure!(
            self.providers.len() <= 64,
            "at most 64 providers may be selected"
        );
        let unique: std::collections::BTreeSet<_> = self.providers.iter().collect();
        ensure!(
            unique.len() == self.providers.len(),
            "duplicate selected provider"
        );
        ensure!(
            !self.overrides.contains_key(&Role::Creator),
            "Creator cannot inherit itself"
        );
        for name in &self.providers {
            ensure!(
                config.connections.contains_key(name),
                "selected provider is not configured"
            );
        }
        for assignment in self.creator.iter().chain(self.overrides.values()) {
            ensure!(
                assignment.connection.len() <= 64 && !assignment.connection.is_empty(),
                "invalid assignment connection"
            );
            ensure!(
                assignment.model.as_ref().is_none_or(|s| !s.is_empty()
                    && s.len() <= 256
                    && !s.chars().any(char::is_control)),
                "invalid assignment model"
            );
            let mut c = config
                .connections
                .get(&assignment.connection)
                .cloned()
                .context("assigned connection is not configured")?;
            c.model = assignment.model.clone();
            c.effort = assignment.effort.clone();
            c.validate()?;
        }
        // Deselected assignments intentionally survive; resolve blocks their new work.
        Ok(())
    }
}

pub(crate) fn provider_label(adapter: &str) -> &str {
    match adapter {
        "codex" => "Codex subscription",
        "claude" => "Claude subscription",
        "openai-api" => "OpenAI API",
        "anthropic-api" => "Anthropic API",
        _ => adapter,
    }
}
