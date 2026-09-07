#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo test --locked --test reliability_streams
printf 'cairn: REL-003: pass\n'
cargo test --locked --test reliability_utf8
printf 'cairn: REL-004: pass\n'
