#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo test --locked --test developer_access
cargo test --locked --lib developer_access::tests
printf 'cairn: REL-001: pass\n'
