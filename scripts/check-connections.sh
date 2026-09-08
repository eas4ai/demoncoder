#!/usr/bin/env bash
set -euo pipefail
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
python3 tests/live_connections.py
