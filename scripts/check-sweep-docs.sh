#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cairn_bin="$(readlink -f "$(command -v cairn)")"
node "$(dirname "$cairn_bin")/../scripts/spec-lint.mjs" docs/spec
printf 'cairn: SWEEP-008: pass\n'
