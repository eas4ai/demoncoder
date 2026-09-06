#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo test --locked --test output_limits
cargo build --locked
python3 tests/output_limits.py
printf 'cairn: OUTPUT-001: pass\ncairn: OUTPUT-002: pass\ncairn: OUTPUT-003: pass\n'
