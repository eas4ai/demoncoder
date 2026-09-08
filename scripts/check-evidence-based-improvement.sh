#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo build --locked
cargo test --locked --lib learning
cargo test --locked --test workflow_store
cairn_bin="$(readlink -f "$(command -v cairn)")"
node "$(dirname "$cairn_bin")/../scripts/spec-lint.mjs" docs/spec
failed=0
for requirement in LEARN-001 LEARN-002 LEARN-003 LEARN-004 LEARN-005 LEARN-006 LEARN-007 LEARN-008; do
    if python3 tests/evidence_based_improvement.py --requirement "$requirement"; then
        printf 'cairn: %s: pass\n' "$requirement"
    else
        printf 'cairn: %s: fail\n' "$requirement"
        failed=1
    fi
done
exit "$failed"
