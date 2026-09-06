#!/usr/bin/env bash
set -euo pipefail
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
