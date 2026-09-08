use crate::config::{Args, Config};
use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone)]
pub struct Handle {
    args: Arc<Args>,
    state: Arc<Mutex<Config>>,
    uncertain: Arc<AtomicBool>,
}

#[derive(Clone)]
pub(crate) struct Draft {
    pub config: Config,
    base: Option<[u8; 32]>,
}

impl Handle {
    pub fn open(args: &Args) -> Result<Self> {
        Ok(Self {
            args: Arc::new(args.clone()),
            state: Arc::new(Mutex::new(args.load_config()?)),
            uncertain: Arc::new(AtomicBool::new(false)),
        })
    }

    pub(crate) fn draft(&self) -> Result<Draft> {
        let (config, base) = self.args.load_config_snapshot()?;
        Ok(Draft { base, config })
    }

    pub(crate) fn current(&self) -> Result<Config> {
        let state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("settings state unavailable"))?;
        ensure!(
            !self.uncertain.load(Ordering::SeqCst),
            "settings publication is uncertain; reopen Settings and save a verified revision before new work"
        );
        Ok(state.clone())
    }

    pub(crate) fn args(&self) -> &Args {
        &self.args
    }

    pub(crate) fn creator(
        &self,
        previous: &crate::config::Connection,
    ) -> Result<crate::config::Selection> {
        let mut selection = self.args.selection_from(&self.current()?)?;
        let oracle = selection.connection.access.oracle.take();
        // Assignment edits cannot change trust, extension tools or launch authority.
        selection.connection.access = previous.access.clone();
        selection.connection.access.oracle = oracle;
        Ok(selection)
    }

    pub(crate) fn role(
        &self,
        role: super::Role,
        explicit: Option<&str>,
    ) -> Result<crate::config::Connection> {
        let config = self.current()?;
        let mut connection = if let Some(name) = explicit {
            Args::role_connection(&config, role, name)?
        } else {
            super::Assignments::from_config(&config).resolve(&config, role)?
        };
        connection.access = crate::tools::AccessPolicy::review_only();
        connection.validate()?;
        Ok(connection)
    }

    pub(crate) fn save(&self, draft: &mut Draft) -> Result<()> {
        let path = self
            .args
            .config_path()
            .context("set HOME or select --config")?;
        let _lock = crate::startup::settings_lock(&path, self.args.config.is_none())?;
        let (_, current) = self.args.load_config_snapshot()?;
        ensure!(
            current == draft.base,
            "settings changed in another editor; close and reopen Settings before saving"
        );
        if let Some(settings) = &draft.config.settings {
            settings.validate(&draft.config)?;
        }
        for connection in draft.config.connections.values() {
            connection.validate()?;
        }
        // Acquire the state lock before publication, so failure cannot publish a file
        // that this handle then refuses to make available to subsequent work.
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("settings state unavailable"))?;
        if let Err(error) = crate::startup::save(&path, &draft.config) {
            if error
                .downcast_ref::<crate::startup::PublicationUncertain>()
                .is_some()
            {
                self.uncertain.store(true, Ordering::SeqCst);
            }
            return Err(error);
        }
        draft.base = Some(Sha256::digest(toml::to_string_pretty(&draft.config)?.as_bytes()).into());
        self.uncertain.store(false, Ordering::SeqCst);
        *state = draft.config.clone();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{Assignment, Assignments, Role};
    use clap::Parser;
    use std::os::unix::fs::PermissionsExt;

    fn fixture(root: &std::path::Path) -> (Handle, std::path::PathBuf) {
        std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = root.join("settings.toml");
        let config: Config = serde_json::from_value(serde_json::json!({
            "default_connection": "a", "onboarding_complete": true,
            "connections": {
                "a": {"adapter":"openai-api", "model":"a-model", "api_key":"synthetic-settings-key"},
                "b": {"adapter":"anthropic-api", "model":"b-model", "api_key":"synthetic-other-key"}
            }
        })).unwrap();
        crate::startup::save(&path, &config).unwrap();
        let args = Args::parse_from([
            "demoncoder",
            "--config",
            path.to_str().unwrap(),
            "--workspace",
            root.to_str().unwrap(),
            "--trust-workspace",
        ]);
        (Handle::open(&args).unwrap(), path)
    }

    fn configure(draft: &mut Draft, connection: &str, model: &str) {
        let mut assignments = Assignments::from_config(&draft.config);
        assignments.creator = Some(Assignment {
            connection: connection.into(),
            model: Some(model.into()),
            effort: None,
        });
        draft.config.settings = Some(assignments);
    }

    #[test]
    fn inherited_roles_follow_creator_while_overrides_and_authority_remain_distinct() {
        let root = tempfile::tempdir().unwrap();
        let (handle, _) = fixture(root.path());
        let mut draft = handle.draft().unwrap();
        configure(&mut draft, "a", "a-model");
        draft.config.settings.as_mut().unwrap().overrides.insert(
            Role::Judge,
            Assignment {
                connection: "a".into(),
                model: Some("judge-model".into()),
                effort: Some("high".into()),
            },
        );
        handle.save(&mut draft).unwrap();
        configure(&mut draft, "b", "b-model");
        handle.save(&mut draft).unwrap();
        for role in [Role::Worker, Role::Oracle, Role::Reviewer, Role::Advisor] {
            let connection = handle.role(role, None).unwrap();
            assert_eq!(connection.model.as_deref(), Some("b-model"));
            assert!(!connection.access.tools_enabled);
            assert!(!connection.access.unrestricted);
            assert!(connection.access.extension.is_none());
        }
        assert_eq!(
            handle.role(Role::Judge, None).unwrap().model.as_deref(),
            Some("judge-model")
        );
        assert!(handle.args().agent_settings().unwrap().is_none());
        assert!(
            handle
                .args()
                .workflow_settings()
                .unwrap()
                .reviewer
                .is_none()
        );
        assert_eq!(
            handle
                .role(Role::Reviewer, Some("a"))
                .unwrap()
                .model
                .as_deref(),
            Some("a-model")
        );
        draft
            .config
            .settings
            .as_mut()
            .unwrap()
            .overrides
            .remove(&Role::Judge);
        handle.save(&mut draft).unwrap();
        assert_eq!(
            handle.role(Role::Judge, None).unwrap().model.as_deref(),
            Some("b-model")
        );
    }

    #[test]
    fn private_publication_rejects_competing_edits_and_preserves_active_revision() {
        let root = tempfile::tempdir().unwrap();
        let (handle, path) = fixture(root.path());
        let mut first = handle.draft().unwrap();
        let mut second = handle.draft().unwrap();
        configure(&mut first, "a", "first-model");
        configure(&mut second, "b", "second-model");
        handle.save(&mut first).unwrap();
        let saved = std::fs::read(&path).unwrap();
        let error = handle.save(&mut second).unwrap_err();
        assert!(error.to_string().contains("another editor"), "{error:#}");
        assert_eq!(std::fs::read(&path).unwrap(), saved);
        assert_eq!(
            handle.role(Role::Creator, None).unwrap().model.as_deref(),
            Some("first-model")
        );
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let mut stale = handle.draft().unwrap();
        let mut changed = saved;
        changed.extend_from_slice(b"\n# independent edit\n");
        std::fs::write(&path, &changed).unwrap();
        assert!(
            handle.save(&mut stale).is_err(),
            "even a concurrent comment edit must not be lost"
        );
        assert_eq!(std::fs::read(&path).unwrap(), changed);
    }

    #[test]
    fn failed_or_invalid_save_does_not_apply_and_deselection_never_redirects() {
        let root = tempfile::tempdir().unwrap();
        let (handle, path) = fixture(root.path());
        let mut draft = handle.draft().unwrap();
        configure(&mut draft, "a", "a-model");
        handle.save(&mut draft).unwrap();
        let saved = std::fs::read(&path).unwrap();
        draft
            .config
            .settings
            .as_mut()
            .unwrap()
            .creator
            .as_mut()
            .unwrap()
            .model = Some("bad\nmodel".into());
        let error = handle.save(&mut draft).unwrap_err();
        assert!(!error.to_string().contains("synthetic-settings-key"));
        assert_eq!(std::fs::read(&path).unwrap(), saved);
        configure(&mut draft, "b", "b-model");
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o777)).unwrap();
        assert!(handle.save(&mut draft).is_err());
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(
            handle.role(Role::Creator, None).unwrap().model.as_deref(),
            Some("a-model")
        );
        configure(&mut draft, "a", "a-model");
        draft
            .config
            .settings
            .as_mut()
            .unwrap()
            .providers
            .retain(|p| p != "a");
        handle.save(&mut draft).unwrap();
        assert!(
            handle
                .role(Role::Creator, None)
                .err()
                .unwrap()
                .to_string()
                .contains("deselected")
        );
        assert_eq!(
            handle
                .current()
                .unwrap()
                .settings
                .unwrap()
                .creator
                .unwrap()
                .connection,
            "a"
        );
        let restarted = Handle::open(handle.args()).unwrap();
        assert!(restarted.role(Role::Creator, None).is_err());
    }

    #[test]
    fn migration_keeps_legacy_oracle_override() {
        let root = tempfile::tempdir().unwrap();
        let (handle, _) = fixture(root.path());
        let mut draft = handle.draft().unwrap();
        draft.config.oracle = Some(crate::config::OracleAssignment {
            connection: "b".into(),
            model: Some("legacy-oracle".into()),
            effort: Some("high".into()),
        });
        let mut settings = Assignments::from_config(&draft.config);
        assert_eq!(
            settings
                .overrides
                .get(&Role::Oracle)
                .unwrap()
                .model
                .as_deref(),
            Some("legacy-oracle")
        );
        settings.creator.as_mut().unwrap().model = Some("changed-creator".into());
        assert_eq!(
            settings
                .resolve(&draft.config, Role::Oracle)
                .unwrap()
                .model
                .as_deref(),
            Some("legacy-oracle")
        );
    }
}
