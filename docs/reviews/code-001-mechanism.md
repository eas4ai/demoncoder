# CODE-001 mechanism build review

The requirement concerns a prompt entered through the actual terminal and
the selected runtime starting that turn. The check launches the production
binary through a Linux pseudo-terminal. For each initial connection it
submits a fresh unpredictable prompt, observes the same value at the HTTP
or subprocess peer, and waits for the peer's response in terminal output.
The check also compares the selected connection in retained events.

The native fixtures speak OpenAI Responses SSE and Anthropic Messages SSE.
The external fixtures speak Codex app-server JSONL and Claude stream-json.
The application uses its normal configuration, registry, session, and
renderer; no production test mode supplies the response.

## Safe failure demonstration

`python3 tests/terminal_session.py --fault-drop-prompt` ran on 2026-09-06.
The Codex peer refused to start the submitted turn. Its expected response
never appeared in the terminal, the checkpoint timed out, and the command
reported `cairn: CODE-001: fail` with exit 1. The other three connection
cases passed. The normal command exercised the corrected peer and reported
all four cases passing with exit 0.

This demonstrates that a broken connection prevents the aggregate pass.
It does not establish live authentication or equivalence between a fixture
and every behavior of an installed backend. It does not establish tools,
streaming while a peer is paused, steering, continued context after
cancellation, confinement, or tool evidence. Those requirements remain
unreported by this mechanism until their behavioral cases exist.

The complete commitment implementation review remains pending.
