#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --lib
cargo build --locked
python3 tests/terminal_sweep.py
python3 tests/cancellation.py
bash scripts/check-sweep-docs.sh
printf 'cairn: CLIP-001: pass\ncairn: CLIP-002: pass\n'
