#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo test --locked --lib
cargo build --locked
python3 tests/chat_presentation.py
python3 tests/scrollback.py
python3 tests/usage.py
printf 'cairn: CHAT-001: pass\ncairn: CHAT-002: pass\ncairn: CHAT-003: pass\ncairn: CHAT-004: pass\n'
