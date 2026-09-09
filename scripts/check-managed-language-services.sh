#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
export PYTHONDONTWRITEBYTECODE=1
export DEMONCODER_LSP_RUST="${DEMONCODER_LSP_RUST:-$(rustup which rust-analyzer)}"
export DEMONCODER_LSP_TYPESCRIPT="${DEMONCODER_LSP_TYPESCRIPT:-$(readlink -f "$(command -v typescript-language-server)")}"
language_toolchain="$(dirname "$(dirname "$DEMONCODER_LSP_RUST")")"
typescript_package="$(dirname "$(dirname "$DEMONCODER_LSP_TYPESCRIPT")")"
export DEMONCODER_LSP_RUST_READ_ROOTS="${DEMONCODER_LSP_RUST_READ_ROOTS:-$language_toolchain/bin:$language_toolchain/lib:$language_toolchain/libexec}"
export DEMONCODER_LSP_TYPESCRIPT_READ_ROOTS="${DEMONCODER_LSP_TYPESCRIPT_READ_ROOTS:-$typescript_package:$(dirname "$typescript_package")/typescript:$(command -v node)}"
test -x "$DEMONCODER_LSP_RUST"
test -x "$DEMONCODER_LSP_TYPESCRIPT"
"$DEMONCODER_LSP_RUST" --version
"$DEMONCODER_LSP_TYPESCRIPT" --version

# Cargo accepts a filter that matches no tests. Check the actual registered
# names before running the behavioral cases, so removed tests cannot pass.
python3 - <<'PY'
import subprocess
required = {
    "disabled_missing_failed_and_unsupported_servers_are_explicit",
    "status_detects_server_exit_after_successful_initialization",
    "unsupported_synchronization_never_advertises_usable_navigation",
    "navigation_retains_unicode_positions_and_empty_results",
    "recoverable_content_modified_replies_do_not_restart_indexing",
    "server_cancelled_diagnostics_retrigger_without_restarting_indexing",
    "empty_pull_reports_preserve_pushed_errors_and_their_freshness",
    "exhausted_content_modified_budget_preserves_the_healthy_server",
    "revisions_reject_old_and_future_diagnostics_and_preserve_repeated_errors",
    "newer_notification_for_same_revision_replaces_cached_empty_result",
    "unversioned_and_absent_diagnostics_never_claim_current_clean",
    "initial_pending_diagnostics_preserve_background_indexing",
    "reported_loading_or_failed_project_never_looks_current_clean",
    "failed_post_edit_diagnostics_preserve_the_mutation",
    "external_change_during_navigation_invalidates_the_answer",
    "new_public_dependency_during_navigation_invalidates_the_answer",
    "cancellation_stops_owned_server_descendants_and_allows_restart",
    "server_cannot_read_protected_files_write_outside_or_request_actions",
    "running_server_cannot_read_new_nested_private_directory",
    "idle_server_cannot_observe_late_private_workspace_or_dependency_files",
    "initial_private_files_and_aliases_never_enter_the_server_view",
    "child_and_review_policies_cannot_enable_servers",
    "language_configuration_can_be_constructed_without_an_async_runtime",
    "declared_credentials_cannot_overlap_language_system_runtime",
    "oversized_result_is_explicit_and_shutdown_stops_idle_descendants",
    "installed_rust_navigation_and_diagnostics",
    "installed_typescript_navigation_and_diagnostics",
    "installed_rust_save_triggers_compiler_diagnostics",
    "only_successful_mutations_save_current_synchronized_text",
    "pull_diagnostics_require_a_full_report_and_notification_floods_fail_boundedly",
    "unversioned_updates_remain_visible_and_retain_last_versioned_items",
}
listed = subprocess.check_output(["cargo", "test", "--locked", "--test", "language_services", "--", "--list"], text=True)
registered = {line.removesuffix(": test") for line in listed.splitlines() if line.endswith(": test")}
assert required <= registered, f"missing behavioral tests: {sorted(required - registered)}"
protocol = {
    "every_split_of_unicode_frame", "consecutive_frames_and_write_roundtrip",
    "rejects_bad_lengths_and_incomplete_frames", "validates_json_charset_and_header_fields",
    "enforces_header_limit_including_delimiter", "rejects_invalid_outbound_before_writing",
    "accepts_exact_body_limit_and_quoted_utf8_charset",
    "oversized_length_is_rejected_without_waiting_for_body_or_eof",
}
required = {"language_services::protocol::tests::" + name for name in protocol}
required.update({"language_services::admission_observer_tests::" + name for name in (
    "a_lone_event_flushes_without_another_event_or_query",
    "observer_recovers_after_a_failed_reconciliation_without_a_query",
    "revocation_stops_peers_before_a_blocked_scan_but_private_additions_do_not",
    "trailing_flush_coalesces_and_busy_events_have_a_maximum_delay",
)})
required.update({"language_services::view::tests::" + name for name in (
    "cancellation_and_system_private_overlap_fail_closed",
    "copies_are_independent_and_ignore_changes_reconsider_denials",
    "deny_cache_is_bounded_and_empty_cache_never_authorizes_private_content",
    "explicit_ignored_dependencies_still_exclude_private_data_and_aliases",
    "runtime_aliases_materialize_only_internal_public_targets",
    "nested_gitignore_negations_do_not_escape_ignored_directory_boundaries",
    "project_writes_live_only_in_the_disposable_layer",
    "input_validation_detects_new_policy_and_deletion_without_events",
    "input_validation_detects_public_additions_without_events",
)})
required.add("native::tests::cancellation_during_language_feedback_preserves_the_completed_edit")
listed = subprocess.check_output(["cargo", "test", "--locked", "--lib", "--", "--list"], text=True)
registered = {line.removesuffix(": test") for line in listed.splitlines() if line.endswith(": test")}
assert required <= registered, f"missing protocol/receipt tests: {sorted(required - registered)}"
PY

cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
cargo test --locked --test language_services -- --include-ignored
cargo build --locked
export DEMONCODER_TEST_BINARY="$PWD/target/debug/demoncoder"
python3 tests/lsp_adapters.py
for requirement in LSP-001 LSP-002 LSP-003 LSP-004 LSP-005 LSP-006; do
    printf 'cairn: %s: pass\n' "$requirement"
done
