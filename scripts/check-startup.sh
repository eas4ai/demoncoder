#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --locked
python3 tests/startup.py
python3 tests/onboarding.py
python3 tests/configuration.py
