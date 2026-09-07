#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo test --locked --test developer_access
cargo test --locked --lib developer_access::tests
cargo test --locked --lib socket_filter::tests
cargo test --locked --test host_guard reliability_explicit_host_mode_retains_unix_sockets
printf 'cairn: REL-001: pass\n'
