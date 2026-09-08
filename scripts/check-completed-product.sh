#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
export PYTHONDONTWRITEBYTECODE=1
export DEMONCODER_TEST_BINARY="$PWD/target/debug/demoncoder"

cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets

# Keep the completed commitments together. The two local-only runners still
# exercise installed backends; only paid live-provider records remain separate.
for gate in \
    startup coding-session connections developer-usability output-limits \
    usage-display chat-presentation sweep-investigation sweep-interaction \
    sweep-status sweep-docs clipboard-shortcuts reliability-sockets \
    reliability-queues reliability-streams verification-review-recovery \
    assignable-subagents advanced-orchestration status-decision-remediation \
    evidence-based-improvement provider-agent-settings; do
    printf 'Completed-product gate: %s\n' "$gate"
    case "$gate" in
        coding-session|connections) bash "scripts/check-$gate.sh" --local-only ;;
        *) bash "scripts/check-$gate.sh" ;;
    esac
done

# Verify the installed artifact itself at the corrected security, workflow and
# provider boundaries. Synthetic loopback traffic does not establish paid access.
cargo install --path . --locked --root "$HOME/.cargo"
export DEMONCODER_TEST_BINARY="$HOME/.cargo/bin/demoncoder"
python3 tests/installed_backends.py
python3 tests/installed_backends.py --results
python3 tests/audit_remediation.py
python3 tests/generated_outputs.py
python3 tests/bounded_review.py
for requirement in VERIFY-001 VERIFY-002 VERIFY-003 VERIFY-004 VERIFY-005 VERIFY-006; do
    python3 tests/verification_workflow.py --requirement "$requirement"
done
printf 'cairn: AUD-006: pass\n'
