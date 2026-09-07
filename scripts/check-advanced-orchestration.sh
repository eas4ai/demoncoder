#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --locked
cargo test --locked --lib --test subagent_state --test subagent_worktrees --test worktree_access --test tool_extensions
failed=0
for requirement in ORCH-001 ORCH-002 ORCH-003 ORCH-004 ORCH-005 ORCH-006 ORCH-007; do
    if python3 tests/advanced_orchestration.py --requirement "$requirement"; then
        printf 'cairn: %s: pass\n' "$requirement"
    else
        printf 'cairn: %s: fail\n' "$requirement"
        failed=1
    fi
done
exit "$failed"
