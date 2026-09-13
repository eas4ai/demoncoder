use crate::settings::{Action, Editor, Handle, SaveJob};
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;

pub(super) struct Panel {
    handle: Handle,
    draft: crate::settings::Draft,
    editor: Editor,
    saving: Option<SaveJob>,
    close_after_save: bool,
}

impl Panel {
    pub fn open(handle: Handle) -> Result<Self> {
        let draft = handle.draft()?;
        let mut editor = Editor::new(draft.config.clone(), false)?;
        editor.notice = "Saved defaults apply to subsequent work; explicit launch/assignment overrides still take precedence. Ctrl+C cancels running work.".into();
        Ok(Self {
            handle,
            draft,
            editor,
            saving: None,
            close_after_save: false,
        })
    }

    pub async fn poll(&mut self) -> bool {
        self.editor.poll();
        if self.saving.as_ref().is_some_and(|job| job.is_finished()) {
            let (result, cleanup) = self.saving.take().expect("save job").finish().await;
            let cleanup = cleanup.err();
            match (result, cleanup) {
                (Ok(crate::settings::SaveStatus::Applied), None) => return true,
                (Ok(crate::settings::SaveStatus::Applied), Some(cleanup)) => {
                    self.editor.notice =
                        format!("Applied, but Settings cleanup is incomplete: {cleanup}")
                }
                (Ok(crate::settings::SaveStatus::AppliedUncertain(reason)), cleanup) => {
                    self.editor.notice = uncertain_notice(&reason, cleanup.as_ref())
                }
                (Ok(crate::settings::SaveStatus::PublicationUncertain(reason)), cleanup) => {
                    self.editor.notice = publication_uncertain_notice(&reason, cleanup.as_ref())
                }
                (Err(_), None) if self.close_after_save => return true,
                (Err(error), cleanup) => {
                    self.editor.notice = format!("Not applied: {error}");
                    append_cleanup(&mut self.editor.notice, cleanup.as_ref());
                }
            }
            self.close_after_save = false;
        }
        false
    }

    pub fn key(&mut self, key: KeyEvent) -> Result<bool> {
        if let Some(job) = &self.saving {
            if key.code == KeyCode::Esc {
                job.cancel();
                self.close_after_save = true;
                self.editor.notice = "Cancelling Settings save…".into();
            }
            return Ok(false);
        }
        match self.editor.key(key)? {
            Action::Stay => {}
            Action::Cancel => return Ok(true),
            Action::Save => {
                self.draft.config = self.editor.config.clone();
                self.editor.notice = "Saving private settings…".into();
                self.saving = Some(self.handle.start_save(self.draft.clone()));
            }
        }
        Ok(false)
    }

    pub fn draw(&self, frame: &mut Frame) {
        self.editor.draw(frame, frame.area());
    }

    pub fn paste(&mut self, text: &str) {
        if self.saving.is_none() {
            self.editor.paste(text);
        }
    }

    pub fn cancel_for_exit(&mut self) {
        if let Some(job) = &self.saving {
            job.cancel();
        }
    }
}

impl Drop for Panel {
    fn drop(&mut self) {
        self.cancel_for_exit();
    }
}

fn uncertain_notice(reason: &str, cleanup: Option<&anyhow::Error>) -> String {
    let mut notice = format!("Applied, but verification is required: {reason}");
    append_cleanup(&mut notice, cleanup);
    notice
}

fn publication_uncertain_notice(reason: &str, cleanup: Option<&anyhow::Error>) -> String {
    let mut notice = format!("Settings publication is uncertain: {reason}");
    append_cleanup(&mut notice, cleanup);
    notice
}

fn append_cleanup(notice: &mut String, cleanup: Option<&anyhow::Error>) {
    if let Some(error) = cleanup {
        notice.push_str(&format!(" Settings cleanup is also incomplete: {error}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use crossterm::event::KeyModifiers;
    use std::{os::unix::fs::PermissionsExt, sync::Arc, time::Duration};

    struct KnownPauseGate {
        entered: Arc<tokio::sync::Semaphore>,
        release: Arc<tokio::sync::Semaphore>,
    }

    #[async_trait::async_trait]
    impl crate::plugins::dispatch::HookRunner for KnownPauseGate {
        fn side_effect_free(&self) -> bool {
            true
        }

        async fn run(
            &self,
            _: &crate::plugins::dispatch::HookInvocation,
        ) -> anyhow::Result<crate::plugins::receipts::RawOutcome> {
            self.entered.add_permits(1);
            self.release.acquire().await.unwrap().forget();
            Ok(crate::plugins::receipts::RawOutcome::Command {
                exit_code: Some(0),
                stdout: br#"{"decision":"approve"}"#.to_vec(),
                stderr: vec![],
            })
        }
    }

    async fn catalog_peer() -> (String, tokio::task::JoinHandle<()>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/v1/responses", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buffer = [0_u8; 1024];
            while !request.windows(4).any(|part| part == b"\r\n\r\n") {
                let count = stream.read(&mut buffer).await.unwrap();
                assert_ne!(count, 0);
                request.extend_from_slice(&buffer[..count]);
            }
            assert!(String::from_utf8_lossy(&request).starts_with("GET /v1/models "));
            let body = r#"{"data":[{"id":"a-model"}]}"#;
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
        });
        (endpoint, task)
    }

    fn fixture(root: &std::path::Path) -> (Handle, std::path::PathBuf) {
        std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = root.join("settings.toml");
        let config: crate::config::Config = serde_json::from_value(serde_json::json!({
            "default_connection": "a", "onboarding_complete": true,
            "connections": {
                "a": {"adapter":"openai-api", "model":"a-model", "api_key":"synthetic-settings-key"}
            }
        }))
        .unwrap();
        crate::startup::save(&path, &config).unwrap();
        let args = crate::config::Args::parse_from([
            "demoncoder",
            "--config",
            path.to_str().unwrap(),
            "--workspace",
            root.to_str().unwrap(),
            "--trust-workspace",
        ]);
        (Handle::open(&args).unwrap(), path)
    }

    fn changed_draft(handle: &Handle, model: &str) -> crate::settings::Draft {
        let mut draft = handle.draft().unwrap();
        let mut assignments = crate::settings::Assignments::from_config(&draft.config);
        assignments.creator = Some(crate::settings::Assignment {
            connection: "a".into(),
            model: Some(model.into()),
            effort: None,
        });
        draft.config.settings = Some(assignments);
        draft
    }

    async fn cancel_panel(panel: &mut Panel) {
        panel
            .key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .unwrap();
        assert!(
            !tokio::time::timeout(Duration::from_millis(50), panel.poll())
                .await
                .expect("Panel poll blocked while owned cleanup continued"),
            "Panel closed before its pending operation drained"
        );
        tokio::time::timeout(Duration::from_secs(3), async {
            while !panel.poll().await {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("Panel Escape did not join its Settings operation");
    }

    async fn await_panel_completion(panel: &mut Panel) -> bool {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let closed = panel.poll().await;
                if panel.saving.is_none() {
                    break closed;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("Settings Panel did not receive its save result")
    }

    #[test]
    fn published_uncertainty_is_never_described_as_unapplied() {
        let notice = super::uncertain_notice("directory synchronization failed", None);
        assert!(notice.starts_with("Applied,"));
        assert!(!notice.contains("Not applied"));
        assert!(notice.contains("verification is required"));
    }

    #[tokio::test]
    async fn ungated_post_rename_worker_panic_stays_visible_after_escape() {
        let root = tempfile::tempdir().unwrap();
        let (handle, path) = fixture(root.path());
        let original = std::fs::read(&path).unwrap();
        let (file_published, release_publication) = handle.pause_next_after_file_publication();
        let mut panel = Panel::open(handle.clone()).unwrap();
        panel.saving =
            Some(handle.start_save(changed_draft(&handle, "ungated-post-rename-panic-model")));

        file_published
            .await
            .expect("real Settings replacement did not complete");
        assert_ne!(std::fs::read(&path).unwrap(), original);
        panel
            .key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .unwrap();
        drop(release_publication);

        assert!(
            !await_panel_completion(&mut panel).await,
            "Escape silently closed after a published Settings worker panic"
        );
        assert!(panel.editor.notice.contains("verification is required"));
        assert!(!panel.editor.notice.contains("Not applied"));
        assert!(handle.current().is_err(), "stale retry remained available");
        assert!(
            String::from_utf8(std::fs::read(&path).unwrap())
                .unwrap()
                .contains("ungated-post-rename-panic-model")
        );
        let reopened = Handle::open(handle.args()).unwrap();
        assert_eq!(
            reopened
                .role(crate::settings::Role::Creator, None)
                .unwrap()
                .model
                .as_deref(),
            Some("ungated-post-rename-panic-model")
        );
    }

    #[tokio::test]
    async fn escape_revokes_ungated_save_queued_before_publication_admission() {
        let root = tempfile::tempdir().unwrap();
        let (handle, path) = fixture(root.path());
        let original = std::fs::read(&path).unwrap();
        let (admission_waiting, release_admission) = handle.pause_next_before_final_admission();
        let mut panel = Panel::open(handle.clone()).unwrap();
        panel.saving =
            Some(handle.start_save(changed_draft(&handle, "queued-ungated-cancellation-model")));

        admission_waiting
            .await
            .expect("ungated worker reached serialized publication boundary");
        assert_eq!(std::fs::read(&path).unwrap(), original);
        panel
            .key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .unwrap();
        release_admission.send(()).unwrap();

        assert!(
            await_panel_completion(&mut panel).await,
            "known pre-admission cancellation should close Settings after joining"
        );
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert_eq!(
            handle
                .role(crate::settings::Role::Creator, None)
                .unwrap()
                .model
                .as_deref(),
            Some("a-model")
        );
    }

    #[tokio::test]
    async fn exit_and_drop_revoke_ungated_save_queued_before_publication_admission() {
        let root = tempfile::tempdir().unwrap();
        let (handle, path) = fixture(root.path());
        let original = std::fs::read(&path).unwrap();
        let (admission_waiting, release_admission) = handle.pause_next_before_final_admission();
        let mut panel = Panel::open(handle.clone()).unwrap();
        panel.saving = Some(handle.start_save(changed_draft(&handle, "queued-ungated-exit-model")));

        admission_waiting
            .await
            .expect("ungated worker reached serialized publication boundary");
        panel.cancel_for_exit();
        drop(panel);
        release_admission.send(()).unwrap();
        handle.wait_for_publication_transaction().await;

        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert_eq!(
            handle
                .role(crate::settings::Role::Creator, None)
                .unwrap()
                .model
                .as_deref(),
            Some("a-model")
        );
    }

    #[tokio::test]
    async fn gated_post_rename_worker_panic_keeps_publication_and_cleanup_truth() {
        let root = tempfile::tempdir().unwrap();
        let (handle, path) = fixture(root.path());
        let original = std::fs::read(&path).unwrap();
        let running = crate::config_change_test_support::start_control(
            &handle,
            root.path(),
            crate::config_change_test_support::command_registration(
                crate::plugins::hook_types::HookDialect::Native,
                true,
            ),
        )
        .await;
        let directory = running.runtime.directory().unwrap();
        let (file_published, release_publication) = handle.pause_next_after_file_publication();
        let mut panel = Panel::open(handle.clone()).unwrap();
        panel.saving =
            Some(handle.start_save(changed_draft(&handle, "gated-post-rename-panic-model")));

        file_published
            .await
            .expect("real gated Settings replacement did not complete");
        assert_ne!(std::fs::read(&path).unwrap(), original);
        drop(release_publication);

        assert!(!await_panel_completion(&mut panel).await);
        assert!(panel.editor.notice.contains("verification is required"));
        assert!(panel.editor.notice.contains("cleanup"));
        assert!(!panel.editor.notice.contains("Not applied"));
        assert!(handle.current().is_err(), "stale retry remained available");
        let retry = handle.start_save(changed_draft(&handle, "must-not-replay-after-panic"));
        let (retry_outcome, retry_cleanup) = retry.finish().await;
        assert!(retry_outcome.is_err(), "stale save unexpectedly retried");
        retry_cleanup.expect("refused retry created no operation to clean up");
        let envelope: serde_json::Value =
            serde_json::from_slice(&std::fs::read(directory.join("state.json")).unwrap()).unwrap();
        let record: crate::workflow::runtime::Record =
            serde_json::from_value(envelope["payload"].clone()).unwrap();
        let config_changes = record
            .operations
            .iter()
            .filter_map(|operation| {
                operation
                    .non_tool_receipt()
                    .map(|receipt| (operation, receipt))
            })
            .filter(|(_, receipt)| {
                receipt.facts.subject.occurrence.event()
                    == crate::plugins::hook_types::HookEvent::ConfigChange
            })
            .collect::<Vec<_>>();
        assert_eq!(config_changes.len(), 1, "stale retry replayed the gate");
        let (operation, receipt) = config_changes[0];
        assert!(operation.complete && receipt.settled);
        assert_eq!(receipt.publication, None);
        assert_eq!(receipt.hooks.len(), 1);
        assert!(receipt.hooks[0].outcome.is_some());
        assert!(
            String::from_utf8(std::fs::read(&path).unwrap())
                .unwrap()
                .contains("gated-post-rename-panic-model")
        );
        let reopened = Handle::open(handle.args()).unwrap();
        assert_eq!(
            reopened
                .role(crate::settings::Role::Creator, None)
                .unwrap()
                .model
                .as_deref(),
            Some("gated-post-rename-panic-model")
        );
        let shutdown = running.shutdown_application(false).await;
        assert!(matches!(shutdown, Err(_) | Ok(Err(_))));
    }

    #[tokio::test]
    async fn escape_waits_for_actual_settings_command_descendants() {
        let root = tempfile::tempdir().unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::create_dir(root.path().join("effects")).unwrap();
        let path = root.path().join("settings.toml");
        let (endpoint, catalog) = catalog_peer().await;
        let config: crate::config::Config = serde_json::from_value(serde_json::json!({
            "default_connection": "a", "onboarding_complete": true,
            "connections": {
                "a": {"adapter":"openai-api", "model":"a-model", "api_key":"synthetic-settings-key", "endpoint":endpoint}
            }
        }))
        .unwrap();
        crate::startup::save(&path, &config).unwrap();
        let original = std::fs::read(&path).unwrap();
        let args = crate::config::Args::parse_from([
            "demoncoder",
            "--config",
            path.to_str().unwrap(),
            "--workspace",
            root.path().to_str().unwrap(),
            "--trust-workspace",
        ]);
        let handle = Handle::open(&args).unwrap();
        let token = format!("settings-panel-command-{}", std::process::id());
        let running = crate::config_change_test_support::start_control(
            &handle,
            root.path(),
            crate::config_change_test_support::pending_command_registration(&token),
        )
        .await;
        running.start_task("preserve unrelated active work").await;
        let before = running.runtime.record().unwrap();
        let unrelated_task = before.task.as_ref().map(|task| {
            (
                task.id,
                task.objective.clone(),
                task.stopped,
                task.accepted.clone(),
                task.corrections,
                task.verification_generation,
            )
        });
        let unrelated_allocation = before.allocation.as_ref().map(|allocation| {
            (
                allocation.started_ms,
                allocation.deadline_ms,
                allocation.model_calls,
                allocation.tool_calls,
            )
        });
        let mut panel = Panel::open(handle.clone()).unwrap();
        panel
            .key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE))
            .unwrap();
        panel
            .key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .unwrap();
        catalog.await.unwrap();
        for _ in 0..20 {
            panel.poll().await;
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        panel
            .key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE))
            .unwrap();
        panel
            .key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .unwrap();
        panel
            .key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE))
            .unwrap();
        panel
            .key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .unwrap();
        assert!(
            panel.saving.is_some(),
            "actual Settings save key did not start a save: {}",
            panel.editor.notice
        );
        tokio::time::timeout(Duration::from_secs(3), async {
            while crate::config_change_test_support::processes(&token).len() < 2 {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("actual Settings command descendants did not start");
        panel
            .key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .unwrap();

        tokio::time::timeout(Duration::from_secs(3), async {
            while !panel.poll().await {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("Panel Escape did not join its Settings command");
        assert!(crate::config_change_test_support::processes(&token).is_empty());
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert_eq!(
            handle
                .role(crate::settings::Role::Creator, None)
                .unwrap()
                .model
                .as_deref(),
            Some("a-model")
        );
        let after = running.runtime.record().unwrap();
        assert_eq!(
            after.task.as_ref().map(|task| (
                task.id,
                task.objective.clone(),
                task.stopped,
                task.accepted.clone(),
                task.corrections,
                task.verification_generation,
            )),
            unrelated_task
        );
        assert_eq!(
            after.allocation.as_ref().map(|allocation| (
                allocation.started_ms,
                allocation.deadline_ms,
                allocation.model_calls,
                allocation.tool_calls,
            )),
            unrelated_allocation
        );
        assert!(after.operations.iter().any(|operation| {
            operation.non_tool_receipt().is_some_and(|receipt| {
                receipt.facts.subject.occurrence.event()
                    == crate::plugins::hook_types::HookEvent::ConfigChange
                    && receipt.publication.is_none()
            })
        }));
        running.shutdown().await;
    }

    #[tokio::test]
    async fn escape_revokes_only_settings_while_actual_provider_task_remains_in_flight() {
        let root = tempfile::tempdir().unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::create_dir(root.path().join("effects")).unwrap();
        let peer = crate::config_change_test_support::ActualOwnerPeer::start(
            root.path(),
            "openai-api",
            true,
        )
        .await
        .unwrap();
        let path = root.path().join("settings.toml");
        let mut connections = std::collections::BTreeMap::new();
        connections.insert("a".to_owned(), peer.connection.clone());
        let config = crate::config::Config {
            onboarding_complete: true,
            default_connection: Some("a".into()),
            connections,
            settings: Some(crate::settings::Assignments {
                providers: vec!["a".into()],
                creator: Some(crate::settings::Assignment {
                    connection: "a".into(),
                    model: peer.connection.model.clone(),
                    effort: peer.connection.effort.clone(),
                }),
                overrides: Default::default(),
            }),
            ..Default::default()
        };
        crate::startup::save(&path, &config).unwrap();
        let original = std::fs::read(&path).unwrap();
        let args = crate::config::Args::parse_from([
            "demoncoder",
            "--config",
            path.to_str().unwrap(),
            "--workspace",
            root.path().to_str().unwrap(),
            "--trust-workspace",
        ]);
        let handle = Handle::open(&args).unwrap();
        let token = format!("settings-panel-provider-task-{}", std::process::id());
        let policy = Arc::new(
            crate::plugins::non_tool::NonToolPlan::new(
                crate::plugins::hook_types::HookEvent::ConfigChange,
                vec![crate::config_change_test_support::pending_command_registration(&token)],
            )
            .unwrap(),
        );
        let mut running = crate::config_change_test_support::start_actual_control(
            &handle,
            root.path(),
            peer.connection.clone(),
            policy,
            crate::workflow::allocation::Limits {
                seconds: 60,
                model_calls: 4,
                tool_calls: 4,
            },
        )
        .await;
        running.submit("/task preserve actual in-flight work").await;
        peer.wait_held().await;
        assert_eq!(peer.request_count(), 1);
        let before = running.runtime.record().unwrap();
        let task_before = before.task.as_ref().map(|task| {
            (
                task.id,
                task.objective.clone(),
                task.stopped,
                task.accepted.clone(),
                task.corrections,
                task.verification_generation,
            )
        });
        assert!(task_before.as_ref().is_some_and(|task| !task.2));
        let allocation_before = before.allocation.as_ref().map(|allocation| {
            (
                allocation.started_ms,
                allocation.deadline_ms,
                allocation.model_calls,
                allocation.tool_calls,
            )
        });

        let mut panel = Panel::open(handle.clone()).unwrap();
        panel.saving = Some(handle.start_save(changed_draft(
            &handle,
            "must-not-publish-during-provider-turn",
        )));
        tokio::time::timeout(Duration::from_secs(3), async {
            while crate::config_change_test_support::processes(&token).len() < 2 {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("actual Settings runner did not start during the provider request");

        cancel_panel(&mut panel).await;

        assert!(crate::config_change_test_support::processes(&token).is_empty());
        assert_eq!(peer.request_count(), 1);
        assert_eq!(std::fs::read(&path).unwrap(), original);
        let after_cancel = running.runtime.record().unwrap();
        assert_eq!(
            after_cancel.task.as_ref().map(|task| (
                task.id,
                task.objective.clone(),
                task.stopped,
                task.accepted.clone(),
                task.corrections,
                task.verification_generation,
            )),
            task_before
        );
        assert_eq!(
            after_cancel.allocation.as_ref().map(|allocation| (
                allocation.started_ms,
                allocation.deadline_ms,
                allocation.model_calls,
                allocation.tool_calls,
            )),
            allocation_before
        );
        assert!(after_cancel.operations.iter().any(|operation| {
            operation.non_tool_receipt().is_some_and(|receipt| {
                receipt.facts.subject.occurrence.event()
                    == crate::plugins::hook_types::HookEvent::ConfigChange
                    && receipt.facts.task.is_none()
                    && receipt.facts.child_owner.is_none()
                    && receipt.publication.is_none()
            })
        }));

        peer.release();
        running.wait_turn_finished().await;
        let completed = running.runtime.record().unwrap();
        assert!(completed.task.as_ref().is_some_and(|task| {
            task.id == task_before.as_ref().unwrap().0
                && task.objective == task_before.as_ref().unwrap().1
                && task.stopped
        }));
        assert_eq!(
            completed.allocation.as_ref().map(|allocation| (
                allocation.started_ms,
                allocation.deadline_ms,
                allocation.model_calls,
                allocation.tool_calls,
            )),
            allocation_before
        );
        running.shutdown().await;
    }

    #[tokio::test]
    async fn known_settings_cancellation_during_held_task_allows_later_normal_work() {
        let root = tempfile::tempdir().unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let peer = crate::config_change_test_support::ActualOwnerPeer::start(
            root.path(),
            "openai-api",
            true,
        )
        .await
        .unwrap();
        let path = root.path().join("settings.toml");
        let mut connections = std::collections::BTreeMap::new();
        connections.insert("a".to_owned(), peer.connection.clone());
        let config = crate::config::Config {
            onboarding_complete: true,
            default_connection: Some("a".into()),
            connections,
            settings: Some(crate::settings::Assignments {
                providers: vec!["a".into()],
                creator: Some(crate::settings::Assignment {
                    connection: "a".into(),
                    model: peer.connection.model.clone(),
                    effort: peer.connection.effort.clone(),
                }),
                overrides: Default::default(),
            }),
            ..Default::default()
        };
        crate::startup::save(&path, &config).unwrap();
        let original = std::fs::read(&path).unwrap();
        let args = crate::config::Args::parse_from([
            "demoncoder",
            "--config",
            path.to_str().unwrap(),
            "--workspace",
            root.path().to_str().unwrap(),
            "--trust-workspace",
        ]);
        let handle = Handle::open(&args).unwrap();
        let entered = Arc::new(tokio::sync::Semaphore::new(0));
        let release = Arc::new(tokio::sync::Semaphore::new(0));
        let gate = Arc::new(KnownPauseGate {
            entered: entered.clone(),
            release: release.clone(),
        });
        let policy = Arc::new(
            crate::plugins::non_tool::NonToolPlan::new(
                crate::plugins::hook_types::HookEvent::ConfigChange,
                vec![crate::plugins::dispatch::Registration {
                    declaration: crate::config_change_test_support::declaration(
                        "known-settings-cancel",
                        crate::plugins::hook_types::HookDialect::Native,
                        crate::plugins::hook_types::HandlerKind::Command,
                    ),
                    runner: gate,
                    revalidation: None,
                }],
            )
            .unwrap(),
        );
        let mut running = crate::config_change_test_support::start_actual_control(
            &handle,
            root.path(),
            peer.connection.clone(),
            policy,
            crate::workflow::allocation::Limits {
                seconds: 60,
                model_calls: 4,
                tool_calls: 0,
            },
        )
        .await;
        running.submit("/task preserve held provider work").await;
        peer.wait_held().await;

        let mut panel = Panel::open(handle.clone()).unwrap();
        panel.saving =
            Some(handle.start_save(changed_draft(&handle, "known-cancel-must-not-publish")));
        tokio::time::timeout(Duration::from_secs(3), entered.acquire())
            .await
            .expect("known Settings gate did not start")
            .unwrap()
            .forget();
        let release_after_cancel = release.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            release_after_cancel.add_permits(1);
        });
        cancel_panel(&mut panel).await;
        assert_eq!(peer.request_count(), 1);
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert!(!running.runtime.record().unwrap().recovery_pending);

        peer.release();
        running.wait_turn_finished().await;
        assert!(!running.runtime.record().unwrap().recovery_pending);
        running
            .submit("later normal work after Settings Escape")
            .await;
        running.wait_turn_finished().await;
        assert_eq!(peer.request_count(), 2);
        assert!(!running.runtime.record().unwrap().recovery_pending);
        running.shutdown().await;
    }

    #[tokio::test]
    async fn attempt_deadline_finishes_only_after_actual_mcp_cleanup() {
        let root = tempfile::tempdir().unwrap();
        let (handle, path) = fixture(root.path());
        let original = std::fs::read(&path).unwrap();
        handle.set_control_timeout(Duration::from_secs(2));
        let (registration, peer, service) =
            crate::config_change_test_support::pending_http_mcp_registration_delayed(
                root.path(),
                Duration::from_secs(5),
                10_000,
            )
            .await;
        let running =
            crate::config_change_test_support::start_control(&handle, root.path(), registration)
                .await;
        let mut panel = Panel::open(handle.clone()).unwrap();
        let started = tokio::time::Instant::now();
        panel.saving =
            Some(handle.start_save(changed_draft(&handle, "must-not-publish-after-timeout")));
        tokio::time::timeout(Duration::from_secs(2), async {
            while peer.method_count("tools/call") != 1 {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("actual Settings MCP call did not start before the attempt deadline");

        tokio::time::timeout(Duration::from_secs(8), async {
            loop {
                let closed = tokio::time::timeout(Duration::from_millis(50), panel.poll())
                    .await
                    .expect("Panel poll blocked while timeout cleanup continued");
                assert!(!closed, "a failed timed-out save must leave Settings open");
                if panel.saving.is_none() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("timed-out Settings gate did not complete its bounded cleanup");

        assert!(
            started.elapsed() >= Duration::from_millis(1_800),
            "Settings attempt finished before its original execution deadline"
        );
        assert!(
            panel.editor.notice.starts_with("Not applied:")
                && panel.editor.notice.contains("deadline expired"),
            "Panel reported completion before successful cleanup: {}",
            panel.editor.notice
        );
        assert!(matches!(
            service.state(),
            crate::plugins::services::ServiceState::Stopped
                | crate::plugins::services::ServiceState::Failed
        ));
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert!(
            running
                .runtime
                .record()
                .unwrap()
                .operations
                .iter()
                .any(|operation| {
                    operation.non_tool_receipt().is_some_and(|receipt| {
                        receipt.facts.subject.occurrence.event()
                            == crate::plugins::hook_types::HookEvent::ConfigChange
                            && receipt.publication.is_none()
                    })
                })
        );
        service.stop().await.unwrap();
        running.shutdown().await;
    }

    #[tokio::test]
    async fn terminal_exit_revokes_promptly_and_app_shutdown_joins_actual_command() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("effects")).unwrap();
        let (handle, path) = fixture(root.path());
        let original = std::fs::read(&path).unwrap();
        let token = format!("settings-terminal-exit-command-{}", std::process::id());
        let running = crate::config_change_test_support::start_control(
            &handle,
            root.path(),
            crate::config_change_test_support::pending_command_registration(&token),
        )
        .await;
        let runtime = running.runtime.clone();
        let mut panel = Panel::open(handle.clone()).unwrap();
        panel.saving = Some(handle.start_save(changed_draft(
            &handle,
            "must-not-publish-after-terminal-exit",
        )));
        tokio::time::timeout(Duration::from_secs(3), async {
            while crate::config_change_test_support::processes(&token).len() < 2 {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("actual Settings command descendants did not start");

        let exit_started = tokio::time::Instant::now();
        panel.cancel_for_exit();
        drop(panel);
        assert!(
            exit_started.elapsed() < Duration::from_millis(50),
            "terminal exit waited for Settings cleanup before app shutdown"
        );
        let shutdown_started = tokio::time::Instant::now();
        running.shutdown_application(false).await.unwrap().unwrap();
        assert!(
            shutdown_started.elapsed() < Duration::from_secs(3),
            "external app shutdown exceeded its existing whole-operation bound"
        );

        assert!(crate::config_change_test_support::processes(&token).is_empty());
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert!(
            runtime
                .record()
                .unwrap()
                .operations
                .iter()
                .any(|operation| {
                    operation.non_tool_receipt().is_some_and(|receipt| {
                        receipt.facts.subject.occurrence.event()
                            == crate::plugins::hook_types::HookEvent::ConfigChange
                            && receipt.publication.is_none()
                    })
                })
        );
    }

    #[tokio::test]
    async fn escape_joins_pending_stdio_mcp_call_and_its_descendant() {
        let root = tempfile::tempdir().unwrap();
        let (handle, path) = fixture(root.path());
        let original = std::fs::read(&path).unwrap();
        let token = format!("settings-panel-mcp-stdio-{}", std::process::id());
        let (registration, service) =
            crate::config_change_test_support::pending_stdio_mcp_registration(root.path(), &token);
        let running =
            crate::config_change_test_support::start_control(&handle, root.path(), registration)
                .await;
        let mut panel = Panel::open(handle.clone()).unwrap();
        panel.saving = Some(handle.start_save(changed_draft(&handle, "must-not-publish-stdio")));
        tokio::time::timeout(Duration::from_secs(3), async {
            while crate::config_change_test_support::processes(&token).len() < 2 {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("pending stdio MCP call and descendant did not start");

        cancel_panel(&mut panel).await;

        assert!(crate::config_change_test_support::processes(&token).is_empty());
        assert!(matches!(
            service.state(),
            crate::plugins::services::ServiceState::Stopped
                | crate::plugins::services::ServiceState::Failed
        ));
        assert_eq!(std::fs::read(&path).unwrap(), original);
        service.stop().await.unwrap();
        running.shutdown().await;
    }

    #[tokio::test]
    async fn escape_withholds_late_http_mcp_reply_until_actual_call_cleanup() {
        let root = tempfile::tempdir().unwrap();
        let (handle, path) = fixture(root.path());
        let original = std::fs::read(&path).unwrap();
        let (registration, peer, service) =
            crate::config_change_test_support::pending_http_mcp_registration(root.path()).await;
        let running =
            crate::config_change_test_support::start_control(&handle, root.path(), registration)
                .await;
        let mut panel = Panel::open(handle.clone()).unwrap();
        panel.saving = Some(handle.start_save(changed_draft(&handle, "must-not-publish-http")));
        tokio::time::timeout(Duration::from_secs(3), async {
            while peer.method_count("tools/call") != 1 {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("pending HTTP MCP call did not start");

        cancel_panel(&mut panel).await;
        tokio::time::sleep(Duration::from_millis(800)).await;

        assert_eq!(peer.method_count("tools/call"), 1);
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert!(matches!(
            service.state(),
            crate::plugins::services::ServiceState::Stopped
                | crate::plugins::services::ServiceState::Failed
        ));
        service.stop().await.unwrap();
        running.shutdown().await;
    }
}
