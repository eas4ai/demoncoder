#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo test --locked --lib
cargo build --locked
python3 tests/terminal_sweep.py
python3 tests/chat_presentation.py
python3 tests/scrollback.py
printf 'cairn: SWEEP-002: pass\ncairn: SWEEP-003: pass\ncairn: SWEEP-004: pass\n'
