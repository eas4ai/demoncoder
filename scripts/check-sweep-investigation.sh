#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo test --locked --test developer_access sweep_ -- --nocapture
cargo test --locked --lib presentation_error_preserves_actual_failure_for_the_next_prompt
printf 'cairn: SWEEP-001: pass\n'
