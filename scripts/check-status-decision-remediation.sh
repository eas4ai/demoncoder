#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --locked
cargo test --locked --lib
failed=0
for requirement in REM-001 REM-002 REM-003 REM-004; do
    if python3 tests/status_decisions.py --requirement "$requirement"; then
        printf 'cairn: %s: pass\n' "$requirement"
    else
        printf 'cairn: %s: fail\n' "$requirement"
        failed=1
    fi
done
cairn_bin="$(readlink -f "$(command -v cairn)")"
if node "$(dirname "$cairn_bin")/../scripts/spec-lint.mjs" docs/spec \
    && python3 tests/status_decisions_docs.py; then
    printf 'cairn: REM-005: pass\n'
else
    printf 'cairn: REM-005: fail\n'
    failed=1
fi
exit "$failed"

