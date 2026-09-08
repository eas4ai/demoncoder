#!/usr/bin/env bash
set -euo pipefail
local_only=0
if [[ $# == 1 && $1 == --local-only ]]; then
    local_only=1
elif [[ $# != 0 ]]; then
    printf 'Usage: %s [--local-only]\n' "$0" >&2
    exit 2
fi
cd "$(dirname "$0")/.."
cargo build --locked
python3 tests/terminal_session.py --tools
python3 tests/configuration.py
python3 tests/terminal_session.py --tools --settings home
python3 tests/terminal_session.py --tools --settings override
python3 tests/live_evidence.py
python3 tests/registry.py
python3 tests/authentication.py
python3 tests/continuation.py --ownership
cargo test --locked --test capabilities
python3 tests/capability_rejection.py
python3 tests/usage.py

# Report independent checks before validating historical live-provider records.
# A stale live record still fails this runner and never becomes a CONN-001 pass.
if [[ $local_only == 1 ]]; then
    printf 'cairn: CONN-001: unverified\n'
    printf 'Paid live provider availability is outside this local run.\n'
else
    python3 tests/live_connections.py
fi
