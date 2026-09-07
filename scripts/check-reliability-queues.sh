#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --locked
cargo test --locked --test queue_runtime
cargo test --locked --lib session::tests
cargo test --locked --lib native::tests::cancellation_during_result_publication_preserves_completed_receipt
python3 tests/reliability_queues.py
python3 tests/registry.py
python3 tests/steering.py
python3 tests/cancellation.py
printf 'cairn: REL-002: pass\n'
