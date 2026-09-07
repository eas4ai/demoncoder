#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --locked
cargo test --locked --test verification_workflow --test workflow_workspace --test workflow_store --test host_guard
cargo test --locked --lib workflow
failed=0
for requirement in VERIFY-001 VERIFY-002 VERIFY-003 VERIFY-004 VERIFY-005 VERIFY-006; do
    if python3 tests/verification_workflow.py --requirement "$requirement"; then
        printf 'cairn: %s: pass\n' "$requirement"
    else
        printf 'cairn: %s: fail\n' "$requirement"
        failed=1
    fi
done
exit "$failed"
