#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
cargo build --locked
python3 tests/status_sweep.py
python3 tests/usage.py
python3 tests/continuation.py
python3 tests/continuation.py --ownership
python3 tests/registry.py
bash scripts/check-startup.sh
printf 'cairn: SWEEP-005: pass\ncairn: SWEEP-006: pass\ncairn: SWEEP-007: pass\n'
