#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --locked
python3 tests/terminal_session.py --tools
python3 tests/live_evidence.py
python3 tests/live_connections.py
