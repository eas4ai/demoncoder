use demoncoder::plugins::{self, Dialect, ImportOptions};
use std::{fs, path::Path};
fn put(root: &Path, name: &str, body: &str) {
    let path = root.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}
#[test]
fn imports_three_ecosystems_without_activation() {
    for (path, body, dialect) in [
        (
            ".claude-plugin/plugin.json",
            r#"{"name":"demo"}"#,
            Dialect::Claude,
        ),
        (
            ".codex-plugin/plugin.json",
            r#"{"name":"demo"}"#,
            Dialect::Codex,
        ),
        (
            "plugin.json",
            r#"{"$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json","name":"demo","version":"1.0.0"}"#,
            Dialect::Portable,
        ),
    ] {
        let root = tempfile::tempdir().unwrap();
        put(root.path(), path, body);
        put(
            root.path(),
            "skills/hello/SKILL.md",
            "---\nname: hello\ndescription: Hello\n---\nHello",
        );
        let package = plugins::inspect(root.path(), &ImportOptions::default()).unwrap();
        assert_eq!(package.dialect(), dialect);
        assert_eq!(package.name(), "demo");
        assert!(!package.is_active());
        assert!(!package.report().executable_ready());
        assert_eq!(package.components().len(), 1);
    }
}

#[test]
fn portable_inline_replaces_overlay_without_changing_root_components() {
    let root = tempfile::tempdir().unwrap();
    put(
        root.path(),
        "plugin.json",
        r#"{"$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json","name":"root","version":"1.0.0","extensions":{"com.openai":{"hooks":"./inline.json","skills":"./wrong"}}}"#,
    );
    put(
        root.path(),
        ".codex-plugin/plugin.json",
        r#"{"name":"overlay","hooks":"./overlay.json"}"#,
    );
    put(
        root.path(),
        "inline.json",
        r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"true"}]}]}}"#,
    );
    put(root.path(), "overlay.json", "{}");
    put(root.path(), "skills/a/SKILL.md", "body");
    let p = plugins::inspect(root.path(), &ImportOptions::default()).unwrap();
    assert_eq!(p.name(), "root");
    assert!(
        p.components()
            .iter()
            .any(|c| c.declaration == "inline.json")
    );
    assert!(
        !p.components()
            .iter()
            .any(|c| c.declaration == "overlay.json")
    );
    assert!(
        p.components()
            .iter()
            .any(|c| c.declaration == "skills/a/SKILL.md")
    );
    assert!(!p.report().composition.is_empty());
}
#[test]
fn wrong_typed_inline_is_not_overlay_fallback() {
    let root = tempfile::tempdir().unwrap();
    put(
        root.path(),
        "plugin.json",
        r#"{"$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json","name":"demo","extensions":{"com.openai":false}}"#,
    );
    assert!(plugins::inspect(root.path(), &ImportOptions::default()).is_err());
}
#[test]
fn legacy_ambiguity_requires_selection_and_reports_exclusion() {
    let root = tempfile::tempdir().unwrap();
    put(
        root.path(),
        ".claude-plugin/plugin.json",
        r#"{"name":"claude"}"#,
    );
    put(
        root.path(),
        ".codex-plugin/plugin.json",
        r#"{"name":"codex"}"#,
    );
    assert!(plugins::inspect(root.path(), &ImportOptions::default()).is_err());
    let p = plugins::inspect(
        root.path(),
        &ImportOptions {
            dialect: Some(Dialect::Claude),
        },
    )
    .unwrap();
    assert_eq!(p.name(), "claude");
    assert!(!p.report().exclusions.is_empty());
}
#[test]
fn claude_paths_replace_commands_add_skills_and_deduplicate() {
    let root = tempfile::tempdir().unwrap();
    put(
        root.path(),
        ".claude-plugin/plugin.json",
        r#"{"name":"demo","commands":"./custom","skills":["./extra","extra/","./skills"]}"#,
    );
    for path in [
        "commands/ignored.md",
        "custom/yes.md",
        "skills/a/SKILL.md",
        "extra/b/SKILL.md",
    ] {
        put(root.path(), path, "body");
    }
    let p = plugins::inspect(root.path(), &ImportOptions::default()).unwrap();
    assert_eq!(p.components().len(), 3);
    assert!(
        !p.components()
            .iter()
            .any(|c| c.declaration == "commands/ignored.md")
    );
}
#[test]
fn different_files_with_same_qualified_identity_conflict() {
    let root = tempfile::tempdir().unwrap();
    put(
        root.path(),
        ".claude-plugin/plugin.json",
        r#"{"name":"demo","skills":"./extra"}"#,
    );
    put(root.path(), "skills/a/SKILL.md", "body");
    put(root.path(), "extra/a/SKILL.md", "other");
    assert!(plugins::inspect(root.path(), &ImportOptions::default()).is_err());
}
#[test]
fn manifest_references_cannot_escape_snapshot() {
    for path in ["../outside", "/tmp/outside", "./skills/../../outside"] {
        let root = tempfile::tempdir().unwrap();
        put(
            root.path(),
            ".claude-plugin/plugin.json",
            &serde_json::json!({"name":"demo","skills":path}).to_string(),
        );
        let p = plugins::inspect(root.path(), &ImportOptions::default()).unwrap();
        assert_eq!(p.source_validity(), plugins::SourceValidity::Invalid);
    }
}
#[test]
fn unknown_and_malformed_components_are_diagnosed_and_not_ready() {
    let root = tempfile::tempdir().unwrap();
    put(
        root.path(),
        ".claude-plugin/plugin.json",
        r#"{"name":"demo","hooks":42,"mysteryExecutable":{"command":"touch canary"}}"#,
    );
    let p = plugins::inspect(root.path(), &ImportOptions::default()).unwrap();
    assert_eq!(p.source_validity(), plugins::SourceValidity::Invalid);
    assert!(p.report().diagnostics.iter().any(|d| d.field == "hooks"));
    assert!(
        p.report()
            .diagnostics
            .iter()
            .any(|d| d.field == "mysteryExecutable")
    );
    assert!(!p.report().executable_ready());
}
#[test]
fn imported_bytes_are_immutable_and_do_not_execute() {
    let root = tempfile::tempdir().unwrap();
    put(
        root.path(),
        ".claude-plugin/plugin.json",
        r#"{"name":"demo","hooks":{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"touch canary"}]}]}}}"#,
    );
    put(root.path(), "skills/a/SKILL.md", "original");
    let p = plugins::inspect(root.path(), &ImportOptions::default()).unwrap();
    let digest = p.digest().to_owned();
    put(root.path(), "skills/a/SKILL.md", "changed");
    assert_eq!(p.files()["skills/a/SKILL.md"].bytes(), b"original");
    assert_eq!(p.digest(), digest);
    assert!(!root.path().join("canary").exists());
    assert_ne!(
        plugins::inspect(root.path(), &ImportOptions::default())
            .unwrap()
            .digest(),
        digest
    );
}
#[test]
fn unsafe_links_and_special_files_are_rejected() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    put(outside.path(), "secret", "canary");
    std::os::unix::fs::symlink(outside.path().join("secret"), root.path().join("link")).unwrap();
    assert!(plugins::inspect(root.path(), &ImportOptions::default()).is_err());
    fs::remove_file(root.path().join("link")).unwrap();
    let _socket = std::os::unix::net::UnixListener::bind(root.path().join("socket")).unwrap();
    assert!(plugins::inspect(root.path(), &ImportOptions::default()).is_err());
}
#[test]
fn private_paths_are_excluded_before_capture() {
    let root = tempfile::tempdir().unwrap();
    put(root.path(), ".env", "secret");
    put(root.path(), ".ssh/key", "secret");
    put(root.path(), ".git/config", "secret");
    put(root.path(), "skills/a/SKILL.md", "body");
    let p = plugins::inspect(root.path(), &ImportOptions::default()).unwrap();
    assert_eq!(p.files().len(), 1);
    assert_eq!(p.report().exclusions.len(), 3);
}
#[test]
fn oversize_file_is_rejected() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("large"),
        vec![0; plugins::MAX_FILE_BYTES + 1],
    )
    .unwrap();
    assert!(plugins::inspect(root.path(), &ImportOptions::default()).is_err());
}

#[test]
fn duplicate_json_fields_cannot_replace_executable_declarations() {
    let root = tempfile::tempdir().unwrap();
    put(
        root.path(),
        ".claude-plugin/plugin.json",
        r#"{"name":"demo","hooks":42,"hooks":{}}"#,
    );
    assert!(plugins::inspect(root.path(), &ImportOptions::default()).is_err());
}
#[test]
fn missing_explicit_default_component_is_invalid() {
    let root = tempfile::tempdir().unwrap();
    put(
        root.path(),
        ".claude-plugin/plugin.json",
        r#"{"name":"demo","commands":"./commands"}"#,
    );
    assert_eq!(
        plugins::inspect(root.path(), &ImportOptions::default())
            .unwrap()
            .source_validity(),
        plugins::SourceValidity::Invalid
    );
}
#[test]
fn server_conflicts_and_bad_fields_are_visible() {
    let root = tempfile::tempdir().unwrap();
    put(
        root.path(),
        ".claude-plugin/plugin.json",
        r#"{"name":"demo","mcpServers":{"a":{"command":"second"}}}"#,
    );
    put(
        root.path(),
        ".mcp.json",
        r#"{"mcpServers":{"a":{"command":"first"}}}"#,
    );
    assert!(plugins::inspect(root.path(), &ImportOptions::default()).is_err());
    put(
        root.path(),
        ".mcp.json",
        r#"{"mcpServers":{"a":{"command":"second"}}}"#,
    );
    assert_eq!(
        plugins::inspect(root.path(), &ImportOptions::default())
            .unwrap()
            .components()
            .len(),
        1
    );
    put(
        root.path(),
        ".claude-plugin/plugin.json",
        r#"{"name":"demo","mcpServers":{"b":{"command":"tool","mystery":true}}}"#,
    );
    let p = plugins::inspect(root.path(), &ImportOptions::default()).unwrap();
    assert!(
        p.report()
            .diagnostics
            .iter()
            .any(|d| d.message.contains("mystery"))
    );
}
#[test]
fn portable_root_requires_schema_and_preserves_mcp_with_overlay() {
    let root = tempfile::tempdir().unwrap();
    put(root.path(), "plugin.json", r#"{"name":"demo"}"#);
    assert!(plugins::inspect(root.path(), &ImportOptions::default()).is_err());
    put(
        root.path(),
        "plugin.json",
        r#"{"$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json","name":"demo"}"#,
    );
    put(
        root.path(),
        "mcp.json",
        r#"{"$schema":"https://agent-plugins.org/schemas/1.0.0/mcp.schema.json","mcpServers":{"root":{"type":"stdio","command":"tool"}}}"#,
    );
    put(
        root.path(),
        ".codex-plugin/plugin.json",
        r#"{"name":"ignored","mcpServers":{"wrong":{"command":"bad"}},"hooks":"./hooks.json"}"#,
    );
    put(root.path(), "hooks.json", r#"{"hooks":{}}"#);
    let p = plugins::inspect(root.path(), &ImportOptions::default()).unwrap();
    assert_eq!(p.components().len(), 2);
    assert!(p.components().iter().any(|c| c.identity == "demo:Mcp:root"));
}
#[test]
fn unknown_hook_fields_and_types_fail_source_validation() {
    let root = tempfile::tempdir().unwrap();
    put(
        root.path(),
        ".claude-plugin/plugin.json",
        r#"{"name":"demo","hooks":{"hooks":{"Stop":[{"hooks":[{"type":"mystery","command":"true","newField":true}]}]}}}"#,
    );
    let p = plugins::inspect(root.path(), &ImportOptions::default()).unwrap();
    assert_eq!(p.source_validity(), plugins::SourceValidity::Invalid);
    assert!(
        p.report()
            .diagnostics
            .iter()
            .any(|d| d.field.ends_with(".newField"))
    );
}

#[test]
fn explicit_private_root_is_not_imported() {
    let root = tempfile::tempdir().unwrap();
    put(root.path(), ".ssh/key", "secret");
    assert!(plugins::inspect(&root.path().join(".ssh"), &ImportOptions::default()).is_err());
}
#[test]
fn count_and_total_capture_limits_are_enforced() {
    let root = tempfile::tempdir().unwrap();
    for i in 0..=plugins::MAX_ENTRIES {
        fs::write(root.path().join(format!("entry-{i}")), []).unwrap();
    }
    assert!(
        plugins::inspect(root.path(), &ImportOptions::default())
            .unwrap_err()
            .to_string()
            .contains("entries")
    );
    let root = tempfile::tempdir().unwrap();
    let bytes = vec![0; plugins::MAX_FILE_BYTES];
    for i in 0..=plugins::MAX_TOTAL_BYTES / plugins::MAX_FILE_BYTES {
        fs::write(root.path().join(format!("file-{i}")), &bytes).unwrap();
    }
    assert!(
        plugins::inspect(root.path(), &ImportOptions::default())
            .unwrap_err()
            .to_string()
            .contains("total bytes")
    );
}
#[test]
fn snapshot_digest_includes_file_mode() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    put(root.path(), "script.sh", "true");
    let first = plugins::inspect(root.path(), &ImportOptions::default()).unwrap();
    fs::set_permissions(
        root.path().join("script.sh"),
        fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    let second = plugins::inspect(root.path(), &ImportOptions::default()).unwrap();
    assert_ne!(first.digest(), second.digest());
    assert_eq!(second.files()["script.sh"].mode(), 0o700);
}

#[test]
fn only_claude_uses_root_skill_fallback() {
    let root = tempfile::tempdir().unwrap();
    put(root.path(), "SKILL.md", "body");
    assert_eq!(
        plugins::inspect(root.path(), &ImportOptions::default())
            .unwrap()
            .components()
            .len(),
        1
    );
    put(
        root.path(),
        "plugin.json",
        r#"{"$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json","name":"demo"}"#,
    );
    assert!(
        plugins::inspect(root.path(), &ImportOptions::default())
            .unwrap()
            .components()
            .is_empty()
    );
}
#[test]
fn unknown_hook_events_and_top_level_fields_are_diagnosed() {
    let root = tempfile::tempdir().unwrap();
    put(
        root.path(),
        ".claude-plugin/plugin.json",
        r#"{"name":"demo","hooks":{"hooks":{"InventedEvent":[]},"runNow":"true"}}"#,
    );
    let p = plugins::inspect(root.path(), &ImportOptions::default()).unwrap();
    assert_eq!(p.source_validity(), plugins::SourceValidity::Invalid);
    assert!(
        p.report()
            .diagnostics
            .iter()
            .any(|d| d.field.contains("InventedEvent"))
    );
    assert!(p.report().diagnostics.iter().any(|d| d.field == "runNow"));
}
#[test]
fn safe_selected_root_alias_is_pinned_and_internal_links_are_refused() {
    let root = tempfile::tempdir().unwrap();
    let parent = tempfile::tempdir().unwrap();
    put(root.path(), "public", "body");
    std::os::unix::fs::symlink(root.path(), parent.path().join("alias")).unwrap();
    let p = plugins::inspect(&parent.path().join("alias"), &ImportOptions::default()).unwrap();
    assert_eq!(
        p.source().canonical_root,
        root.path().canonicalize().unwrap()
    );
    std::os::unix::fs::symlink("public", root.path().join("internal")).unwrap();
    assert!(plugins::inspect(root.path(), &ImportOptions::default()).is_err());
}
#[test]
fn excessive_depth_and_hard_links_are_rejected() {
    let root = tempfile::tempdir().unwrap();
    let deep = std::iter::repeat_n("a", plugins::MAX_PATH_DEPTH + 1)
        .collect::<Vec<_>>()
        .join("/");
    put(root.path(), &deep, "body");
    assert!(plugins::inspect(root.path(), &ImportOptions::default()).is_err());
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    put(outside.path(), "secret", "canary");
    fs::hard_link(outside.path().join("secret"), root.path().join("link")).unwrap();
    assert!(plugins::inspect(root.path(), &ImportOptions::default()).is_err());
}

#[test]
fn installed_plugin_subtree_does_not_expose_sibling_credentials() {
    let root = tempfile::tempdir().unwrap();
    put(root.path(), ".codex/auth.json", "secret");
    put(root.path(), ".codex/plugins/demo/skills/a/SKILL.md", "body");
    let p = plugins::inspect(
        &root.path().join(".codex/plugins/demo"),
        &ImportOptions::default(),
    )
    .unwrap();
    assert_eq!(p.files().len(), 1);
    assert!(plugins::inspect(&root.path().join(".codex"), &ImportOptions::default()).is_err());
}
#[test]
fn fifo_files_and_fifo_source_are_rejected_without_waiting_for_writers() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("fifo");
    rustix::fs::mkfifoat(
        rustix::fs::CWD,
        &path,
        rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
    )
    .unwrap();
    assert!(plugins::inspect(&path, &ImportOptions::default()).is_err());
    assert!(plugins::inspect(root.path(), &ImportOptions::default()).is_err());
}

#[test]
fn empty_skills_directory_suppresses_root_fallback() {
    let root = tempfile::tempdir().unwrap();
    put(root.path(), "SKILL.md", "body");
    assert_eq!(
        plugins::inspect(root.path(), &ImportOptions::default())
            .unwrap()
            .components()
            .len(),
        1
    );
    fs::create_dir(root.path().join("skills")).unwrap();
    let package = plugins::inspect(root.path(), &ImportOptions::default()).unwrap();
    assert!(package.components().is_empty());
    assert!(package.directories().contains_key("skills"));
}
#[test]
fn directory_presence_and_mode_change_snapshot_digest() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    put(root.path(), "readme", "body");
    let initial = plugins::inspect(root.path(), &ImportOptions::default()).unwrap();
    fs::create_dir(root.path().join("empty")).unwrap();
    fs::set_permissions(root.path().join("empty"), fs::Permissions::from_mode(0o700)).unwrap();
    let created = plugins::inspect(root.path(), &ImportOptions::default()).unwrap();
    assert_ne!(initial.digest(), created.digest());
    fs::set_permissions(root.path().join("empty"), fs::Permissions::from_mode(0o755)).unwrap();
    let changed = plugins::inspect(root.path(), &ImportOptions::default()).unwrap();
    assert_ne!(created.digest(), changed.digest());
    assert_eq!(created.directories()["empty"].mode(), 0o700);
    assert_eq!(changed.directories()["empty"].mode(), 0o755);
    fs::remove_dir(root.path().join("empty")).unwrap();
    assert_eq!(
        plugins::inspect(root.path(), &ImportOptions::default())
            .unwrap()
            .digest(),
        initial.digest()
    );
    fs::write(root.path().join("empty"), []).unwrap();
    let file = plugins::inspect(root.path(), &ImportOptions::default()).unwrap();
    assert_ne!(file.digest(), created.digest());
    assert!(!file.directories().contains_key("empty"));
}
#[test]
fn foreign_executable_fields_never_disappear_under_codex_profile() {
    for field in [
        "commands",
        "agents",
        "workflows",
        "outputStyles",
        "themes",
        "monitors",
        "lspServers",
        "experimental",
        "channels",
        "userConfig",
        "dependencies",
        "defaultEnabled",
    ] {
        let root = tempfile::tempdir().unwrap();
        put(
            root.path(),
            ".codex-plugin/plugin.json",
            &serde_json::json!({"name":"demo",field:42}).to_string(),
        );
        let package = plugins::inspect(root.path(), &ImportOptions::default()).unwrap();
        assert_ne!(
            package.source_validity(),
            plugins::SourceValidity::Valid,
            "{field}"
        );
        assert!(
            package
                .report()
                .diagnostics
                .iter()
                .any(|d| d.field == field),
            "{field}"
        );
        assert!(!package.report().executable_ready());
    }
}

#[test]
fn repeated_canonical_roots_produce_a_bounded_summary() {
    let root = tempfile::tempdir().unwrap();
    let paths: Vec<_> = (0..2000)
        .map(|i| if i % 2 == 0 { "./skills" } else { "skills/" })
        .collect();
    put(
        root.path(),
        ".claude-plugin/plugin.json",
        &serde_json::json!({"name":"demo","skills":paths}).to_string(),
    );
    for i in 0..50 {
        put(root.path(), &format!("skills/s{i}/SKILL.md"), "body");
    }
    let package = plugins::inspect(root.path(), &ImportOptions::default()).unwrap();
    assert_eq!(package.components().len(), 50);
    assert!(
        package.report().composition.len() <= 3,
        "{} composition rows",
        package.report().composition.len()
    );
    assert_eq!(package.report().diagnostics.len(), 50);
}
#[test]
fn many_invalid_paths_do_not_multiply_metadata_and_diagnostics() {
    let root = tempfile::tempdir().unwrap();
    let paths: Vec<_> = (0..500).map(|i| format!("../outside-{i}")).collect();
    put(
        root.path(),
        ".claude-plugin/plugin.json",
        &serde_json::json!({"name":"demo","skills":paths,"hooks":paths}).to_string(),
    );
    let package = plugins::inspect(root.path(), &ImportOptions::default()).unwrap();
    assert_eq!(package.source_validity(), plugins::SourceValidity::Invalid);
    assert!(
        package.report().diagnostics.len() <= 4,
        "{} diagnostics",
        package.report().diagnostics.len()
    );
    assert!(package.report().composition.len() <= 4);
    assert!(package.components().iter().all(|c| !c.metadata.is_array()));
    assert_eq!(package.metadata()["skills"].as_array().unwrap().len(), 500);
}
#[test]
fn excessive_distinct_component_roots_are_rejected() {
    let root = tempfile::tempdir().unwrap();
    let paths: Vec<_> = (0..4097).map(|i| format!("./missing-{i}")).collect();
    put(
        root.path(),
        ".claude-plugin/plugin.json",
        &serde_json::json!({"name":"demo","skills":paths}).to_string(),
    );
    assert!(plugins::inspect(root.path(), &ImportOptions::default()).is_err());
}

#[test]
fn deduplicated_hook_paths_keep_first_declaration_order() {
    let root = tempfile::tempdir().unwrap();
    put(
        root.path(),
        ".claude-plugin/plugin.json",
        r#"{"name":"demo","hooks":["./z.json","./a.json","z.json"]}"#,
    );
    put(root.path(), "z.json", r#"{"hooks":{}}"#);
    put(root.path(), "a.json", r#"{"hooks":{}}"#);
    let package = plugins::inspect(root.path(), &ImportOptions::default()).unwrap();
    assert_eq!(
        package
            .components()
            .iter()
            .map(|c| c.declaration.as_str())
            .collect::<Vec<_>>(),
        ["z.json", "a.json"]
    );
}
#[test]
fn prefix_discovery_does_not_admit_similarly_named_skill_files() {
    let root = tempfile::tempdir().unwrap();
    put(root.path(), "skills/a/NOTSKILL.md", "body");
    assert!(
        plugins::inspect(root.path(), &ImportOptions::default())
            .unwrap()
            .components()
            .is_empty()
    );
}
