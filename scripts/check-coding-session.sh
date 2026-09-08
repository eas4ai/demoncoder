#!/usr/bin/env bash
set -euo pipefail
local_only=0
if [[ $# == 1 && $1 == --local-only ]]; then
    local_only=1
elif [[ $# != 0 ]]; then
    printf 'Usage: %s [--local-only]\n' "$0" >&2
    exit 2
fi
cd "$(dirname "$0")/.."
cargo build --locked
python3 tests/terminal_session.py

cargo test --locked --test tools
python3 tests/terminal_session.py --tools
python3 tests/responsiveness.py

cargo test --locked --lib interrupted_receive_retains_partial_json
python3 tests/steering.py

cargo test --locked --lib group_cleanup_covers_drop_and_exited_leader
python3 tests/cancellation.py

python3 tests/continuation.py

python3 tests/installed_backends.py

cargo test --locked --lib native::tests
python3 tests/installed_backends.py --results

python3 tests/terminal_screen.py
python3 tests/onboarding.py

cargo test --locked --test host_guard
python3 tests/host_access.py
if [[ $local_only == 1 ]]; then
    printf 'cairn: CODE-010: unverified\n'
    printf 'Paid live Oracle availability is outside this local run.\n'
else
    python3 tests/live_oracle.py
    printf 'cairn: CODE-010: pass\n'
fi
