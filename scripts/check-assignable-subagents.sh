#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --locked
cargo test --locked --lib --test subagent_state --test subagent_worktrees --test worktree_access --test tool_extensions
failed=0
for requirement in SUB-001 SUB-002 SUB-003 SUB-004 SUB-005 SUB-006 SUB-007; do
    if python3 tests/assignable_subagents.py --requirement "$requirement"; then
        printf 'cairn: %s: pass\n' "$requirement"
    else
        printf 'cairn: %s: fail\n' "$requirement"
        failed=1
    fi
done
exit "$failed"
