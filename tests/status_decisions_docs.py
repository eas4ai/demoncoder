#!/usr/bin/env python3
"""Documentation contract for the selected inspection remediation."""
from pathlib import Path

readme = Path("README.md").read_text()
assert "No cumulative token, spend, or wall-clock budget in this release." not in readme
for required in (
    "F2 opens a read-only inspection view",
    "Left/Right change evidence pages",
    "Ctrl-C cancels work even while inspection is open",
    "files not rechecked",
    "Hard cumulative token and monetary caps are unsupported",
):
    assert required in readme, f"documentation missing: {required}"
print("REM-005: controls, freshness and allocation documentation passed")

