#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --locked
cargo test --locked --lib settings::
python3 tests/provider_agent_settings.py
printf 'cairn: SET-001: pass\ncairn: SET-002: pass\n'
python3 tests/role_settings.py
printf 'cairn: SET-003: pass\ncairn: SET-004: pass\ncairn: SET-006: pass\n'
python3 tests/live_settings.py
printf 'cairn: SET-005: pass\ncairn: SET-007: pass\n'

# Exercise the installed release, including onboarding and live work admission.
cargo install --path . --locked --root "$HOME/.cargo"
DEMONCODER_TEST_BINARY="$HOME/.cargo/bin/demoncoder" python3 tests/provider_agent_settings.py
DEMONCODER_TEST_BINARY="$HOME/.cargo/bin/demoncoder" python3 tests/live_settings.py
DEMONCODER_TEST_BINARY="$HOME/.cargo/bin/demoncoder" python3 tests/role_settings.py
printf 'cairn: SET-008: pass\n'
