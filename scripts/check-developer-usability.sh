#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --locked
cargo test --locked --lib transcript::tests
python3 tests/scrollback.py
printf 'cairn: USABLE-001: pass\n'
cargo test --locked --test developer_access
cargo test --locked --lib developer_access::tests
printf 'cairn: USABLE-002: pass\n'
printf 'cairn: USABLE-003: pass\n'
python3 tests/usability_contract.py
printf 'cairn: USABLE-004: pass\n'
