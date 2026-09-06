#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --locked
python3 tests/terminal_session.py

cargo test --locked --test tools
python3 tests/terminal_session.py --tools
python3 tests/responsiveness.py
