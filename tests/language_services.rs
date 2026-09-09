use demoncoder::{
    events::EventSink,
    language_services::LanguageServers,
    tools::{AccessPolicy, ToolCall, ToolExecutor, ToolResult},
};
use serde_json::{Value, json};
use std::{
    io::BufRead,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::Arc,
    time::Duration,
};

async fn tool(executor: &ToolExecutor, name: &str, arguments: Value) -> ToolResult {
    let (tx, mut rx) = tokio::sync::mpsc::channel(256);
    let sink = EventSink::new("language-services".into(), tx, None).unwrap();
    let drain = tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let result = executor
        .execute(
            ToolCall {
                id: "lsp-test".into(),
                name: name.into(),
                arguments,
            },
            &sink,
        )
        .await
        .unwrap();
    drop(sink);
    drain.await.unwrap();
    result
}

struct Fixture {
    root: tempfile::TempDir,
    binary: PathBuf,
    controller: Child,
}
impl Fixture {
    fn new(mode: &str) -> Self {
        let root = tempfile::tempdir().unwrap();
        let binary = root.path().join("fixture-server");
        std::fs::copy(
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/lsp_fixture.py"),
            &binary,
        )
        .unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
        // Keep test coordination outside the server's filesystem access. The
        // controller serves only synthetic barriers, never protected canaries.
        let mut controller = Command::new("/usr/bin/python3")
            .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/lsp_control.py"))
            .arg(root.path())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut address = String::new();
        std::io::BufReader::new(controller.stdout.take().unwrap())
            .read_line(&mut address)
            .unwrap();
        assert!(address.starts_with("http://127.0.0.1:"), "{address}");
        std::fs::write(root.path().join(".fixture-control"), address).unwrap();
        std::fs::write(root.path().join(".gitignore"),
            ".fixture-mode\n.fixture-canaries\nrelease-*\nrequest-started\ndescendant-escaped\nserver-exiting\nadversarial-results\nsave-receipts\nsynchronized-version\n").unwrap();
        let fixture = Self {
            root,
            binary,
            controller,
        };
        fixture.mode(mode);
        std::fs::write(fixture.root.path().join("main.rs"), "😀 target\n").unwrap();
        fixture
    }
    fn mode(&self, mode: &str) {
        std::fs::write(self.root.path().join(".fixture-mode"), mode).unwrap();
    }
    fn policy(&self) -> AccessPolicy {
        AccessPolicy {
            language_servers: LanguageServers {
                rust: Some(self.binary.clone()),
                typescript: Some(self.binary.clone()),
                ..Default::default()
            },
            ..Default::default()
        }
    }
    fn executor(&self) -> ToolExecutor {
        ToolExecutor::with_policy(self.root.path(), &self.policy()).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.controller.kill();
        let _ = self.controller.wait();
    }
}
fn query(operation: &str) -> Value {
    json!({"operation":operation,"path":"main.rs","line":0,"character":3})
}
async fn diagnostics(executor: &ToolExecutor) -> Value {
    let result = tool(
        executor,
        "lsp",
        json!({"operation":"diagnostics","path":"main.rs"}),
    )
    .await;
    assert!(result.success, "{}", result.output);
    serde_json::from_str(&result.output).unwrap()
}
async fn wait_for(path: PathBuf) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !path.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn disabled_missing_failed_and_unsupported_servers_are_explicit() {
    let fixture = Fixture::new("normal");
    let disabled = ToolExecutor::new(fixture.root.path()).unwrap();
    assert!(
        !disabled
            .definitions()
            .iter()
            .any(|definition| definition["name"] == "lsp")
    );
    assert!(!tool(&disabled, "lsp", query("hover")).await.success);
    let mut policy = fixture.policy();
    policy.language_servers.rust = Some("/nonexistent/demoncoder-lsp".into());
    let missing = ToolExecutor::with_policy(fixture.root.path(), &policy).unwrap();
    let result = tool(
        &missing,
        "lsp",
        json!({"operation":"status","language":"rust"}),
    )
    .await;
    assert!(
        !result.success && result.output.contains("unavailable"),
        "{}",
        result.output
    );
    assert!(
        tool(
            &missing,
            "write",
            json!({"path":"notes","content":"ordinary coding works"})
        )
        .await
        .success
    );
    fixture.mode("init-failure");
    let result = tool(
        &fixture.executor(),
        "lsp",
        json!({"operation":"status","language":"rust"}),
    )
    .await;
    assert!(
        !result.success && result.output.contains("initialize"),
        "{}",
        result.output
    );
    fixture.mode("unsupported");
    let executor = fixture.executor();
    let status = tool(
        &executor,
        "lsp",
        json!({"operation":"status","language":"rust"}),
    )
    .await;
    assert!(status.success, "{}", status.output);
    let status: Value = serde_json::from_str(&status.output).unwrap();
    assert_eq!(status["capabilities"]["hover"], false);
    let result = tool(&executor, "lsp", query("hover")).await;
    assert!(
        !result.success && result.output.contains("support"),
        "{}",
        result.output
    );
}

#[tokio::test]
async fn navigation_retains_unicode_positions_and_empty_results() {
    let fixture = Fixture::new("unicode");
    let executor = fixture.executor();
    for operation in ["definition", "references", "hover"] {
        let result = tool(&executor, "lsp", query(operation)).await;
        assert!(result.success, "{operation}: {}", result.output);
        let value: Value = serde_json::from_str(&result.output).unwrap();
        assert_eq!(value["source"]["path"], "main.rs");
        assert_eq!(value["source"]["version"], 1);
        assert_eq!(value["source"]["sha256"].as_str().unwrap().len(), 64);
        assert_eq!(value["truncated"], false);
        if operation != "hover" {
            assert_eq!(value["data"]["result"][0]["range"]["start"]["character"], 3);
        }
    }
    let mut invalid = query("hover");
    invalid["character"] = json!(1);
    let result = tool(&executor, "lsp", invalid).await;
    assert!(
        !result.success && result.output.contains("surrogate"),
        "{}",
        result.output
    );
    fixture.mode("empty");
    let result = tool(&executor, "lsp", query("references")).await;
    assert!(result.success, "{}", result.output);
    let value: Value = serde_json::from_str(&result.output).unwrap();
    assert_eq!(value["data"]["result"], json!([]));
}

#[tokio::test]
async fn recoverable_content_modified_replies_do_not_restart_indexing() {
    let fixture = Fixture::new("content-modified");
    let executor = fixture.executor();
    // The peer returns two recoverable errors per process. Restarting on either
    // error cannot reach its eventual answer; this must complete in one call.
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        tool(&executor, "lsp", query("hover")),
    )
    .await
    .expect("indexing retry was not bounded");
    assert!(result.success, "{}", result.output);
    let value: Value = serde_json::from_str(&result.output).unwrap();
    assert_eq!(value["data"]["state"], "available");
    assert!(
        value["data"]["result"]["contents"]["value"]
            .as_str()
            .unwrap()
            .contains("fixture")
    );
}

#[tokio::test]
async fn exhausted_content_modified_budget_preserves_the_healthy_server() {
    let fixture = Fixture::new("normal");
    let executor = fixture.executor();
    let first = tool(&executor, "lsp", query("hover")).await;
    assert!(first.success, "{}", first.output);
    let first: Value = serde_json::from_str(&first.output).unwrap();
    fixture.mode("content-modified-forever");
    let pending = tokio::time::timeout(
        Duration::from_secs(5),
        tool(&executor, "lsp", query("hover")),
    )
    .await
    .expect("recoverable retry budget was not enforced");
    assert!(
        !pending.success && pending.output.contains("pending"),
        "{}",
        pending.output
    );
    fixture.mode("normal");
    let next = tool(&executor, "lsp", query("hover")).await;
    assert!(next.success, "{}", next.output);
    let next: Value = serde_json::from_str(&next.output).unwrap();
    assert_eq!(
        next["data"]["result"]["fixture_instance"], first["data"]["result"]["fixture_instance"],
        "healthy peer was restarted"
    );
}

#[tokio::test]
async fn server_cancelled_diagnostics_retrigger_without_restarting_indexing() {
    let fixture = Fixture::new("pull-retrigger");
    let executor = fixture.executor();
    let first = tool(&executor, "lsp", query("hover")).await;
    assert!(first.success, "{}", first.output);
    let first: Value = serde_json::from_str(&first.output).unwrap();
    let report = diagnostics(&executor).await;
    assert_eq!(report["data"]["state"], "current");
    assert_eq!(
        report["data"]["items"][0]["message"],
        "pulled fixture error"
    );
    fixture.mode("pull-retrigger-forever");
    let pending = tokio::time::timeout(
        Duration::from_secs(5),
        tool(
            &executor,
            "lsp",
            json!({"operation":"diagnostics","path":"main.rs"}),
        ),
    )
    .await
    .unwrap();
    assert!(
        !pending.success && pending.output.contains("pending"),
        "{}",
        pending.output
    );
    fixture.mode("pull");
    let next = tool(&executor, "lsp", query("hover")).await;
    assert!(next.success, "{}", next.output);
    let next: Value = serde_json::from_str(&next.output).unwrap();
    assert_eq!(
        next["data"]["result"]["fixture_instance"],
        first["data"]["result"]["fixture_instance"]
    );
}

#[tokio::test]
async fn empty_pull_reports_preserve_pushed_errors_and_their_freshness() {
    for (mode, state) in [
        ("pull-mixed", "current"),
        ("pull-mixed-unversioned", "freshness_unknown"),
    ] {
        let fixture = Fixture::new(mode);
        std::fs::write(fixture.root.path().join("main.rs"), "BROKEN").unwrap();
        let executor = fixture.executor();
        let report = diagnostics(&executor).await;
        assert_eq!(report["data"]["state"], state, "{report}");
        assert_eq!(report["data"]["items"][0]["severity"], 1, "{report}");
        assert_eq!(report["data"]["items"][0]["source"], "fixture");
        let written = tool(
            &executor,
            "write",
            json!({"path":"main.rs","content":"corrected"}),
        )
        .await;
        assert!(written.success, "{}", written.output);
        let report = diagnostics(&executor).await;
        assert_eq!(report["data"]["state"], state, "{report}");
        assert_eq!(report["data"]["items"], json!([]), "{report}");
    }
}

#[tokio::test]
async fn revisions_reject_old_and_future_diagnostics_and_preserve_repeated_errors() {
    let fixture = Fixture::new("reorder");
    let executor = fixture.executor();
    for (index, content) in ["BROKEN first", "BROKEN second", "corrected"]
        .iter()
        .enumerate()
    {
        let result = tool(
            &executor,
            "write",
            json!({"path":"main.rs","content":content}),
        )
        .await;
        assert!(result.success, "{}", result.output);
        assert!(
            result.output.contains("Language diagnostics"),
            "{}",
            result.output
        );
        let value = diagnostics(&executor).await;
        assert_eq!(value["source"]["version"], index + 1);
        assert_eq!(value["data"]["state"], "current");
        assert_eq!(value["data"]["verification"], "not run");
        assert_eq!(
            value["data"]["items"].as_array().unwrap().len(),
            usize::from(index < 2)
        );
        if index < 2 {
            assert!(
                value["data"]["items"][0]["message"]
                    .as_str()
                    .unwrap()
                    .contains("persistent")
            );
        }
    }
}

#[tokio::test]
async fn unversioned_and_absent_diagnostics_never_claim_current_clean() {
    let fixture = Fixture::new("unversioned");
    assert_eq!(
        diagnostics(&fixture.executor()).await["data"]["state"],
        "freshness_unknown"
    );
    fixture.mode("pending");
    let result = tool(
        &fixture.executor(),
        "lsp",
        json!({"operation":"diagnostics","path":"main.rs"}),
    )
    .await;
    assert!(
        !result.success && result.output.contains("pending"),
        "{}",
        result.output
    );
}

#[tokio::test]
async fn initial_pending_diagnostics_preserve_background_indexing() {
    let fixture = Fixture::new("late-first");
    std::fs::write(fixture.root.path().join("main.rs"), "BROKEN").unwrap();
    let executor = fixture.executor();
    let first = tool(
        &executor,
        "lsp",
        json!({"operation":"diagnostics","path":"main.rs"}),
    )
    .await;
    assert!(
        !first.success && first.output.contains("pending"),
        "{}",
        first.output
    );
    tokio::time::sleep(Duration::from_millis(1200)).await;
    let next = diagnostics(&executor).await;
    assert_eq!(next["data"]["state"], "current");
    assert_eq!(next["data"]["items"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn reported_loading_or_failed_project_never_looks_current_clean() {
    for (mode, expected) in [
        ("health-loading", "pending"),
        ("health-error", "unavailable"),
        ("health-warning", "limited"),
    ] {
        let fixture = Fixture::new(mode);
        let value = diagnostics(&fixture.executor()).await;
        assert_eq!(value["data"]["state"], expected, "{mode}: {value}");
        assert_eq!(value["data"]["items"], json!([]));
        assert_eq!(value["data"]["verification"], "not run");
    }
}

#[tokio::test]
async fn pull_diagnostics_require_a_full_report_and_notification_floods_fail_boundedly() {
    let fixture = Fixture::new("pull");
    let value = diagnostics(&fixture.executor()).await;
    assert_eq!(value["data"]["state"], "current");
    assert_eq!(value["data"]["items"][0]["message"], "pulled fixture error");
    assert_eq!(value["data"]["verification"], "not run");
    fixture.mode("pull-unchanged");
    let result = tool(
        &fixture.executor(),
        "lsp",
        json!({"operation":"diagnostics","path":"main.rs"}),
    )
    .await;
    assert!(
        !result.success && result.output.contains("full"),
        "{}",
        result.output
    );
    fixture.mode("notification-flood");
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        tool(&fixture.executor(), "lsp", query("hover")),
    )
    .await
    .expect("notification flood was not bounded");
    assert!(
        !result.success && result.output.contains("message bound"),
        "{}",
        result.output
    );
}

#[tokio::test]
async fn newer_notification_for_same_revision_replaces_cached_empty_result() {
    let fixture = Fixture::new("late-current");
    std::fs::write(fixture.root.path().join("main.rs"), "BROKEN").unwrap();
    let executor = fixture.executor();
    let _ = diagnostics(&executor).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let value = diagnostics(&executor).await;
    assert_eq!(
        value["data"]["items"].as_array().unwrap().len(),
        1,
        "{value}"
    );
}

#[tokio::test]
async fn unversioned_updates_remain_visible_and_retain_last_versioned_items() {
    for (mode, latest_errors, versioned_errors) in [
        ("mixed-version-empty-first", 1, 0),
        ("mixed-version-error-first", 0, 1),
    ] {
        let fixture = Fixture::new(mode);
        let executor = fixture.executor();
        let initial = diagnostics(&executor).await;
        assert_eq!(initial["data"]["state"], "current", "{mode}: {initial}");
        assert_eq!(
            initial["data"]["items"].as_array().unwrap().len(),
            versioned_errors,
            "{mode}: {initial}"
        );
        std::fs::write(fixture.root.path().join("release-unversioned"), "").unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        let latest = diagnostics(&executor).await;
        assert_eq!(
            latest["data"]["state"], "freshness_unknown",
            "{mode}: {latest}"
        );
        assert_eq!(
            latest["data"]["items"].as_array().unwrap().len(),
            latest_errors,
            "{mode}: {latest}"
        );
        assert_eq!(
            latest["data"]["last_versioned_items"]
                .as_array()
                .unwrap()
                .len(),
            versioned_errors,
            "{mode}: {latest}"
        );
        assert_eq!(latest["data"]["verification"], "not run");
    }
}

#[tokio::test]
async fn failed_post_edit_diagnostics_preserve_the_mutation() {
    let fixture = Fixture::new("exit");
    let executor = fixture.executor();
    for (name, arguments, expected) in [
        (
            "write",
            json!({"path":"main.rs","content":"before"}),
            "before",
        ),
        (
            "edit",
            json!({"path":"main.rs","old_text":"before","new_text":"after"}),
            "after",
        ),
    ] {
        let result = tool(&executor, name, arguments).await;
        assert!(result.success, "{}", result.output);
        assert_eq!(
            std::fs::read_to_string(fixture.root.path().join("main.rs")).unwrap(),
            expected
        );
        assert!(
            result.output.contains("Language diagnostics") && result.output.contains("unavailable"),
            "{}",
            result.output
        );
    }
}

#[tokio::test]
async fn only_successful_mutations_save_current_synchronized_text() {
    let fixture = Fixture::new("save-required");
    let executor = fixture.executor();
    let result = tool(&executor, "lsp", query("hover")).await;
    assert!(result.success, "{}", result.output);
    assert!(!fixture.root.path().join("save-receipts").exists());
    let mut forged_save = query("hover");
    forged_save["saved"] = json!(true);
    let result = tool(&executor, "lsp", forged_save).await;
    assert!(
        !result.success,
        "a model supplied save flag was accepted: {}",
        result.output
    );
    assert!(!fixture.root.path().join("save-receipts").exists());
    for (name, arguments, expected, errors) in [
        (
            "write",
            json!({"path":"main.rs","content":"BROKEN saved"}),
            "BROKEN saved",
            1,
        ),
        (
            "edit",
            json!({"path":"main.rs","old_text":"BROKEN saved","new_text":"corrected saved"}),
            "corrected saved",
            0,
        ),
    ] {
        let result = tool(&executor, name, arguments).await;
        assert!(result.success, "{}", result.output);
        let feedback = result
            .output
            .split("Language diagnostics: ")
            .nth(1)
            .unwrap();
        let feedback: Value = serde_json::from_str(feedback).unwrap();
        assert_eq!(feedback["data"]["state"], "current");
        assert_eq!(feedback["data"]["items"].as_array().unwrap().len(), errors);
        let receipts = std::fs::read_to_string(fixture.root.path().join("save-receipts")).unwrap();
        let last: Value = serde_json::from_str(receipts.lines().last().unwrap()).unwrap();
        assert_eq!(last["text"], expected);
        let _ = diagnostics(&executor).await;
        assert_eq!(
            std::fs::read_to_string(fixture.root.path().join("save-receipts")).unwrap(),
            receipts,
            "a diagnostic query emitted didSave"
        );
    }
    let receipts = std::fs::read_to_string(fixture.root.path().join("save-receipts")).unwrap();
    assert_eq!(receipts.lines().count(), 2);
    let result = tool(
        &executor,
        "edit",
        json!({"path":"main.rs","old_text":"absent text","new_text":"BROKEN"}),
    )
    .await;
    assert!(!result.success);
    assert_eq!(
        std::fs::read_to_string(fixture.root.path().join("save-receipts")).unwrap(),
        receipts,
        "a failed mutation emitted didSave"
    );
}

#[tokio::test]
async fn external_change_during_navigation_invalidates_the_answer() {
    let fixture = Fixture::new("external-change");
    let executor = Arc::new(fixture.executor());
    let task_executor = executor.clone();
    let task = tokio::spawn(async move { tool(&task_executor, "lsp", query("hover")).await });
    wait_for(fixture.root.path().join("request-started")).await;
    std::fs::write(fixture.root.path().join("main.rs"), "changed externally").unwrap();
    std::fs::write(fixture.root.path().join("release-request"), "").unwrap();
    let result = task.await.unwrap();
    assert!(
        !result.success && (result.output.contains("changed") || result.output.contains("stale")),
        "{}",
        result.output
    );
}

#[tokio::test]
async fn new_public_dependency_during_navigation_invalidates_the_answer() {
    for external_dependency in [false, true] {
        let fixture = Fixture::new("external-change");
        let dependency = tempfile::tempdir().unwrap();
        let mut policy = fixture.policy();
        if external_dependency {
            policy
                .language_servers
                .read_roots
                .push(dependency.path().to_owned());
        }
        let executor = Arc::new(ToolExecutor::with_policy(fixture.root.path(), &policy).unwrap());
        let task_executor = executor.clone();
        let task = tokio::spawn(async move { tool(&task_executor, "lsp", query("hover")).await });
        wait_for(fixture.root.path().join("request-started")).await;
        let changed_root = if external_dependency {
            dependency.path()
        } else {
            fixture.root.path()
        };
        std::fs::write(
            changed_root.join("new_dependency.rs"),
            "pub fn added() {}\n",
        )
        .unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        std::fs::write(fixture.root.path().join("release-request"), "").unwrap();
        let result = task.await.unwrap();
        assert!(
            !result.success
                && (result.output.contains("changed") || result.output.contains("stale")),
            "external_dependency={external_dependency}: {}",
            result.output
        );
    }
}

#[tokio::test]
async fn cancellation_stops_owned_server_descendants_and_allows_restart() {
    let fixture = Fixture::new("stall");
    let executor = Arc::new(fixture.executor());
    let task_executor = executor.clone();
    let task = tokio::spawn(async move { tool(&task_executor, "lsp", query("hover")).await });
    wait_for(fixture.root.path().join("request-started")).await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    tokio::time::sleep(Duration::from_millis(1300)).await;
    assert!(!fixture.root.path().join("descendant-escaped").exists());
    fixture.mode("normal");
    let result = tool(&executor, "lsp", query("hover")).await;
    assert!(result.success, "{}", result.output);
}

#[tokio::test]
async fn server_cannot_read_protected_files_write_outside_or_request_actions() {
    let fixture = Fixture::new("adversarial");
    let outside = tempfile::tempdir().unwrap();
    let protected = outside.path().join("credential");
    let canary = outside.path().join("canary");
    std::fs::write(&protected, "PROTECTED-CANARY").unwrap();
    std::fs::write(&canary, "OUTSIDE-CANARY").unwrap();
    std::fs::write(
        fixture.root.path().join(".fixture-canaries"),
        json!({"protected":protected,"outside":canary}).to_string(),
    )
    .unwrap();
    let mut policy = fixture.policy();
    policy.credential_paths.push(protected.clone());
    let executor = ToolExecutor::with_policy(fixture.root.path(), &policy).unwrap();
    let result = tool(&executor, "lsp", query("hover")).await;
    assert!(result.success, "{}", result.output);
    let effects: Value = serde_json::from_str(
        &std::fs::read_to_string(fixture.root.path().join("adversarial-results")).unwrap(),
    )
    .unwrap();
    assert_eq!(effects["protected_read"], false);
    assert_eq!(effects["outside_write"], false);
    assert_eq!(effects["edit"]["result"]["applied"], false);
    assert_eq!(effects["command"]["error"]["code"], -32601);
    assert_eq!(effects["folders"]["error"]["code"], -32601);
    assert_eq!(std::fs::read_to_string(canary).unwrap(), "OUTSIDE-CANARY");
    assert_eq!(
        std::fs::read_to_string(protected).unwrap(),
        "PROTECTED-CANARY"
    );
    assert_eq!(
        std::fs::read_to_string(fixture.root.path().join("main.rs")).unwrap(),
        "😀 target\n"
    );
    assert!(!fixture.root.path().join("unsolicited-command").exists());
    fixture.mode("outside-uri");
    let result = tool(&executor, "lsp", query("definition")).await;
    assert!(
        !result.success && result.output.contains("outside"),
        "{}",
        result.output
    );
}

#[tokio::test]
async fn running_server_cannot_read_new_nested_private_directory() {
    let fixture = Fixture::new("normal");
    let executor = fixture.executor();
    let result = tool(
        &executor,
        "lsp",
        json!({"operation":"status","language":"rust"}),
    )
    .await;
    assert!(result.success, "{}", result.output);
    assert_eq!(
        serde_json::from_str::<Value>(&result.output).unwrap()["state"],
        "ready"
    );

    // Create private state on the host after the server's sandbox is running.
    let protected = fixture.root.path().join("nested/.demoncoder/canary");
    std::fs::create_dir_all(protected.parent().unwrap()).unwrap();
    std::fs::write(&protected, "LATE-PRIVATE-CANARY").unwrap();
    let outside = tempfile::tempdir().unwrap();
    let canary = outside.path().join("canary");
    std::fs::write(&canary, "OUTSIDE-CANARY").unwrap();
    std::fs::write(
        fixture.root.path().join(".fixture-canaries"),
        json!({"protected":protected,"outside":canary}).to_string(),
    )
    .unwrap();
    fixture.mode("adversarial");
    let result = tool(&executor, "lsp", query("definition")).await;
    assert!(result.success, "{}", result.output);
    let effects: Value = serde_json::from_str(
        &std::fs::read_to_string(fixture.root.path().join("adversarial-results")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        effects["protected_read"], false,
        "running server read a private file created after initialization"
    );
}

#[tokio::test]
async fn idle_server_cannot_observe_late_private_workspace_or_dependency_files() {
    let fixture = Fixture::new("background-probe");
    let dependency = tempfile::tempdir().unwrap();
    std::fs::create_dir(fixture.root.path().join("nested")).unwrap();
    std::fs::write(
        dependency.path().join("public.ts"),
        "export const value = 1;",
    )
    .unwrap();
    let private = fixture.root.path().join("nested/.demoncoder/canary");
    let env = fixture.root.path().join(".env.production");
    let dependency_private = dependency.path().join(".env.production");
    let alias = fixture.root.path().join("innocent.rs");
    std::fs::write(
        fixture.root.path().join(".fixture-canaries"),
        json!({
            "read_paths": {
                "workspace_private": private, "workspace_env": env,
                "dependency_private": dependency_private, "alias": alias,
                "permitted_dependency": dependency.path().join("public.ts"),
            }
        })
        .to_string(),
    )
    .unwrap();
    let mut policy = fixture.policy();
    policy
        .language_servers
        .read_roots
        .push(dependency.path().to_owned());
    let executor = ToolExecutor::with_policy(fixture.root.path(), &policy).unwrap();
    let result = tool(
        &executor,
        "lsp",
        json!({"operation":"status","language":"rust"}),
    )
    .await;
    assert!(result.success, "{}", result.output);
    wait_for(fixture.root.path().join("request-started")).await;
    std::fs::create_dir_all(private.parent().unwrap()).unwrap();
    std::fs::write(&private, "LATE-PRIVATE-CANARY").unwrap();
    std::fs::write(&env, "LATE-ENV-CANARY").unwrap();
    std::fs::write(&dependency_private, "LATE-DEPENDENCY-CANARY").unwrap();
    std::os::unix::fs::symlink(&env, &alias).unwrap();
    // No new LSP request: the existing process probes independently, before a
    // request-time restart could conceal the lifetime exposure.
    std::fs::write(fixture.root.path().join("release-probe"), "").unwrap();
    wait_for(fixture.root.path().join("adversarial-results")).await;
    let effects: Value = serde_json::from_str(
        &std::fs::read_to_string(fixture.root.path().join("adversarial-results")).unwrap(),
    )
    .unwrap();
    assert_eq!(effects["permitted_dependency"], true, "{effects}");
    for denied in [
        "workspace_private",
        "workspace_env",
        "dependency_private",
        "alias",
    ] {
        assert_eq!(
            effects[denied], false,
            "idle server read {denied}: {effects}"
        );
    }
}

#[tokio::test]
async fn initial_private_files_and_aliases_never_enter_the_server_view() {
    let fixture = Fixture::new("adversarial");
    let outside = tempfile::tempdir().unwrap();
    let private = fixture.root.path().join(".env.production");
    std::fs::write(&private, "INITIAL-PRIVATE-CANARY").unwrap();
    let alias = fixture.root.path().join("alias.rs");
    let hardlink = fixture.root.path().join("hardlink.rs");
    std::os::unix::fs::symlink(&private, &alias).unwrap();
    std::fs::hard_link(&private, &hardlink).unwrap();
    std::fs::write(
        fixture.root.path().join(".fixture-canaries"),
        json!({
            "protected": private, "outside": outside.path().join("must-not-exist"),
            "read_paths": {"alias":alias, "hardlink":hardlink,
                "permitted":fixture.root.path().join("main.rs")}
        })
        .to_string(),
    )
    .unwrap();
    let result = tool(&fixture.executor(), "lsp", query("hover")).await;
    assert!(result.success, "{}", result.output);
    let effects: Value = serde_json::from_str(
        &std::fs::read_to_string(fixture.root.path().join("adversarial-results")).unwrap(),
    )
    .unwrap();
    assert_eq!(effects["protected_read"], false, "{effects}");
    assert_eq!(effects["reads"]["alias"], false, "{effects}");
    assert_eq!(effects["reads"]["hardlink"], false, "{effects}");
    assert_eq!(effects["reads"]["permitted"], true, "{effects}");
    assert!(!outside.path().join("must-not-exist").exists());
}

#[test]
fn child_and_review_policies_cannot_enable_servers() {
    let fixture = Fixture::new("normal");
    for mut policy in [
        AccessPolicy::worktree_only(vec![]),
        AccessPolicy::review_only(),
    ] {
        policy.language_servers = fixture.policy().language_servers;
        assert!(ToolExecutor::with_policy(fixture.root.path(), &policy).is_err());
    }
}

#[test]
fn language_configuration_can_be_constructed_without_an_async_runtime() {
    let fixture = Fixture::new("normal");
    let executor = fixture.executor();
    assert!(
        executor
            .definitions()
            .iter()
            .any(|tool| tool["name"] == "lsp")
    );
    assert!(!fixture.root.path().join("request-started").exists());
}

#[tokio::test]
async fn declared_credentials_cannot_overlap_language_system_runtime() {
    let fixture = Fixture::new("owned-child");
    let mut policy = fixture.policy();
    policy
        .credential_paths
        .push("/usr/lib/demoncoder-synthetic-credential".into());
    let executor = ToolExecutor::with_policy(fixture.root.path(), &policy).unwrap();
    let result = tool(
        &executor,
        "lsp",
        json!({"operation":"status","language":"rust"}),
    )
    .await;
    assert!(
        !result.success && result.output.contains("runtime"),
        "{}",
        result.output
    );
    assert!(!fixture.root.path().join("request-started").exists());
}

#[tokio::test]
async fn status_detects_server_exit_after_successful_initialization() {
    let fixture = Fixture::new("exit-after-init");
    let executor = fixture.executor();
    let status = json!({"operation":"status","language":"rust"});
    let result = tool(&executor, "lsp", status.clone()).await;
    assert!(result.success, "{}", result.output);
    assert_eq!(
        serde_json::from_str::<Value>(&result.output).unwrap()["state"],
        "ready"
    );
    std::fs::write(fixture.root.path().join("release-exit"), "").unwrap();
    wait_for(fixture.root.path().join("server-exiting")).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let result = tool(&executor, "lsp", status).await;
    assert!(
        !result.success,
        "exited server reported ready: {}",
        result.output
    );
    assert!(
        result.output.contains("exited") || result.output.contains("closed"),
        "{}",
        result.output
    );
}

#[tokio::test]
async fn unsupported_synchronization_never_advertises_usable_navigation() {
    for mode in ["no-sync", "no-open-sync"] {
        let fixture = Fixture::new(mode);
        let executor = fixture.executor();
        let result = tool(
            &executor,
            "lsp",
            json!({"operation":"status","language":"rust"}),
        )
        .await;
        assert!(result.success, "{mode}: {}", result.output);
        let status: Value = serde_json::from_str(&result.output).unwrap();
        for operation in ["definition", "references", "hover"] {
            assert_eq!(status["capabilities"][operation], false, "{mode}: {status}");
            let result = tool(&executor, "lsp", query(operation)).await;
            assert!(
                !result.success && result.output.contains("sync"),
                "{mode} {operation}: {}",
                result.output
            );
        }
    }
}

#[tokio::test]
async fn oversized_result_is_explicit_and_shutdown_stops_idle_descendants() {
    let fixture = Fixture::new("oversized");
    let result = tool(&fixture.executor(), "lsp", query("references")).await;
    assert!(
        !result.success,
        "oversized result silently succeeded: {}",
        result.output
    );
    assert!(
        result.output.contains("bound")
            || result.output.contains("MiB")
            || result.output.contains("body length")
            || result.output.contains("large"),
        "{}",
        result.output
    );
    fixture.mode("owned-child");
    let executor = fixture.executor();
    let result = tool(
        &executor,
        "lsp",
        json!({"operation":"status","language":"rust"}),
    )
    .await;
    assert!(result.success, "{}", result.output);
    assert!(fixture.root.path().join("request-started").exists());
    drop(executor);
    tokio::time::sleep(Duration::from_millis(1300)).await;
    assert!(!fixture.root.path().join("descendant-escaped").exists());
}

async fn installed_smoke(language: &str, variable: &str) {
    let binary = PathBuf::from(
        std::env::var_os(variable)
            .unwrap_or_else(|| panic!("set {variable} to the installed server executable")),
    );
    assert!(
        binary.is_absolute() && binary.is_file(),
        "{variable} must name an absolute executable path"
    );
    let root = tempfile::tempdir().unwrap();
    let (path, broken, corrected, line, character) = if language == "rust" {
        std::fs::create_dir(root.path().join("src")).unwrap();
        std::fs::write(
            root.path().join("Cargo.toml"),
            "[package]\nname = \"lsp_smoke\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        (
            "src/main.rs",
            "fn target() -> i32 { let = 1; 42 }\nfn main() { let _: i32 = target(); }\n",
            "fn target() -> i32 { 42 }\nfn main() { let _: i32 = target(); }\n",
            1,
            26,
        )
    } else {
        std::fs::write(
            root.path().join("tsconfig.json"),
            "{\"compilerOptions\":{\"strict\":true,\"noEmit\":true},\"include\":[\"main.ts\"]}",
        )
        .unwrap();
        (
            "main.ts",
            "function target(): number { return 42; }\nconst value: string = target();\n",
            "function target(): number { return 42; }\nconst value: number = target();\n",
            1,
            23,
        )
    };
    std::fs::write(root.path().join(path), broken).unwrap();
    let servers = if language == "rust" {
        LanguageServers {
            rust: Some(binary),
            typescript: None,
            read_roots: installed_read_roots(variable),
        }
    } else {
        LanguageServers {
            rust: None,
            typescript: Some(binary),
            read_roots: installed_read_roots(variable),
        }
    };
    let executor = ToolExecutor::with_policy(
        root.path(),
        &AccessPolicy {
            language_servers: servers,
            ..Default::default()
        },
    )
    .unwrap();
    for operation in ["definition", "hover", "references"] {
        // Servers may acknowledge initialize before project indexing completes.
        let mut result = tool(
            &executor,
            "lsp",
            json!({"operation":operation,"path":path,"line":line,"character":character}),
        )
        .await;
        for _ in 0..40 {
            if result.success {
                let value: Value = serde_json::from_str(&result.output).unwrap();
                let answer = &value["data"]["result"];
                if !answer.is_null() && !answer.as_array().is_some_and(Vec::is_empty) {
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
            result = tool(
                &executor,
                "lsp",
                json!({"operation":operation,"path":path,"line":line,"character":character}),
            )
            .await;
        }
        assert!(
            result.success,
            "real {language} {operation}: {}",
            result.output
        );
        let value: Value = serde_json::from_str(&result.output).unwrap();
        let answer = &value["data"]["result"];
        assert!(
            !answer.is_null(),
            "real {language} {operation} returned null: {value}"
        );
        if operation == "hover" {
            assert!(answer["contents"].to_string().contains("target"), "{value}");
        } else {
            assert!(
                !answer.as_array().unwrap().is_empty(),
                "real {language} {operation} returned no locations"
            );
        }
    }
    for expected_errors in [true, false] {
        if !expected_errors {
            let result = tool(&executor, "write", json!({"path":path,"content":corrected})).await;
            assert!(result.success, "{}", result.output);
        }
        let mut observed = None;
        let mut last_output = String::new();
        for _ in 0..40 {
            let result = tool(
                &executor,
                "lsp",
                json!({"operation":"diagnostics","path":path}),
            )
            .await;
            last_output = result.output.clone();
            if result.success {
                let value: Value = serde_json::from_str(&result.output).unwrap();
                let has_errors = value["data"]["items"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|item| item["severity"] == 1);
                assert_eq!(value["data"]["verification"], "not run");
                let usable = matches!(
                    value["data"]["state"].as_str(),
                    Some("current" | "freshness_unknown")
                );
                if usable && has_errors == expected_errors {
                    observed = Some(value);
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        assert!(
            observed.is_some(),
            "real {language} diagnostics did not report expected error presence {expected_errors}: {last_output}"
        );
    }
}

fn installed_read_roots(variable: &str) -> Vec<PathBuf> {
    std::env::split_paths(
        &std::env::var_os(format!("{variable}_READ_ROOTS"))
            .unwrap_or_else(|| panic!("set {variable}_READ_ROOTS to admitted runtime paths")),
    )
    .collect()
}

#[tokio::test]
#[ignore = "requires DEMONCODER_LSP_RUST installed executable"]
async fn installed_rust_navigation_and_diagnostics() {
    installed_smoke("rust", "DEMONCODER_LSP_RUST").await;
}

#[tokio::test]
#[ignore = "requires DEMONCODER_LSP_TYPESCRIPT installed executable"]
async fn installed_typescript_navigation_and_diagnostics() {
    installed_smoke("typescript", "DEMONCODER_LSP_TYPESCRIPT").await;
}

#[tokio::test]
#[ignore = "requires DEMONCODER_LSP_RUST installed executable and Rust toolchain"]
async fn installed_rust_save_triggers_compiler_diagnostics() {
    let binary =
        PathBuf::from(std::env::var_os("DEMONCODER_LSP_RUST").expect("set DEMONCODER_LSP_RUST"));
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("src")).unwrap();
    std::fs::write(
        root.path().join("Cargo.toml"),
        "[package]\nname = \"lsp_save_smoke\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    let corrected = "fn main() { let _value: i32 = 42; }\n";
    std::fs::write(root.path().join("src/main.rs"), corrected).unwrap();
    let executor = ToolExecutor::with_policy(
        root.path(),
        &AccessPolicy {
            language_servers: LanguageServers {
                rust: Some(binary),
                typescript: None,
                read_roots: installed_read_roots("DEMONCODER_LSP_RUST"),
            },
            ..Default::default()
        },
    )
    .unwrap();
    // Let the project load before requesting Cargo checks through actual saves.
    let result = tool(
        &executor,
        "lsp",
        json!({"operation":"hover","path":"src/main.rs","line":0,"character":23}),
    )
    .await;
    assert!(result.success, "{}", result.output);
    tokio::time::sleep(Duration::from_secs(2)).await;
    for (content, expected_errors) in [
        (
            "fn main() { let value = String::new(); drop(value); drop(value); }\n",
            true,
        ),
        (corrected, false),
    ] {
        let result = tool(
            &executor,
            "write",
            json!({"path":"src/main.rs","content":content}),
        )
        .await;
        assert!(result.success, "{}", result.output);
        let mut matched = false;
        let mut last_output = result.output;
        for _ in 0..40 {
            let result = tool(
                &executor,
                "lsp",
                json!({"operation":"diagnostics","path":"src/main.rs"}),
            )
            .await;
            last_output = result.output.clone();
            if result.success {
                let value: Value = serde_json::from_str(&result.output).unwrap();
                let errors: Vec<_> = value["data"]["items"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|item| item["severity"] == 1)
                    .collect();
                let has_errors = !errors.is_empty();
                let usable = matches!(
                    value["data"]["state"].as_str(),
                    Some("current" | "freshness_unknown")
                );
                let compiler_error = errors.iter().any(|item| item["source"] == "rustc");
                if usable && has_errors == expected_errors && (!expected_errors || compiler_error) {
                    if expected_errors {
                        assert!(
                            errors.iter().any(|item| item["code"] == "E0382"
                                || item["message"].as_str().unwrap().contains("moved value")),
                            "{value}"
                        );
                    }
                    assert_eq!(value["data"]["verification"], "not run");
                    matched = true;
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        assert!(
            matched,
            "Rust compiler diagnostic error presence {expected_errors} was not observed: {last_output}"
        );
    }
}
