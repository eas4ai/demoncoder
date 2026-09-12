# Investigate intermittent private-settings mount setup failures

Surfaced from: PRUN-001
Captured: 2026-09-12T18:41:15.725Z

The native async prerequisite full run all-targets-publication-final-20260912T183616110421Z failed two developer_access tests during bwrap bind-mount setup for /home/shawn/.claude.json, before a shell started. The unchanged targeted rerun developer-access-mount-rerun-20260912T183900145678Z passed 15 tests with one ignored; frozen source and configuration inputs were unchanged. The unchanged DeveloperAccess builder checks live private-path metadata and later supplies pathname destinations to bwrap. The exact external mutation or cause during the failed calls was not captured. Investigate and reproduce this intermittent setup dependency before selecting a fix; retain fail-closed masking and do not weaken the assertions. This is a captured follow-up, not authorization to expand the native async implementation.
