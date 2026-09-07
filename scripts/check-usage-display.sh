#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --locked
python3 tests/usage.py
python3 tests/scrollback.py
printf 'cairn: DISPLAY-001: pass\n'
