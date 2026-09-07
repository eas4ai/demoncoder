# Terminal usability sweep

Status: Agreed 2026-09-07
Requirements: SWEEP-001, SWEEP-002, SWEEP-003, SWEEP-004, SWEEP-005, SWEEP-006, SWEEP-007, SWEEP-008

The developer selected the remainder of the recovered reliability and usability
sweep with "proceed to sweep the rest" after the current chat commitment reached
Done. Creator identity is complete and is not reopened.

## Deliverable and work sequence

1. Investigate the four reported test/access symptoms through the production
   executor; retain exact commands and the distinction between reproduction,
   expected test behavior and unavailable original evidence.
2. Add timer-driven activity, scrollbar dragging and bounded stable visible-text
   selection/copy to the existing terminal. Start with failing behavior tests.
3. Add model/context/Git/child-count/token status; collect Git asynchronously,
   distinguish context from billing, and omit unknown cost consistently.
4. Correct the eight recorded lint findings without changing prior substantive
   obligations; update the controls and limits documentation.
5. Run focused checks and the Cargo/chat/startup/continuation regressions against
   committed inputs, review uncovered risks, and install the verified binary.

## Ownership and checks

- `src/terminal.rs`, `src/chat.rs`, a focused selection module if needed: view,
  mouse interaction and clipboard gesture; `tests/terminal_sweep.py` plus library
  rendered-cell tests and the existing chat/scrollback drivers.
- `src/status.rs`, `src/events.rs`, `src/main.rs`, `src/config.rs` and adapters:
  selected model/workspace, bounded Git and attributed context; status unit tests
  and provider/backend PTY fixtures in `tests/usage.py` and its peers.
- `tests/developer_access.rs`, existing native presentation test, investigation
  driver/record: temporary storage, PTY, nested namespaces and harmless skill
  reads while retaining outside-write and credential canaries.
- `docs/spec/`, `README.md`, `docs/recon.md`, `handoff.md`: contract and delivery
  records. Mechanisms declare all source/test/script dependencies.

`.cairn/reviews/terminal-usability-sweep.md` records mechanism reviews, safe
violating examples and final review. `.cairn/evidence/` retains receipts/output.

Done when all eight requirements have current passing evidence, the final review
has no open finding in this scope, the verified binary is installed, and Cairn
reports Done. No paid live-provider refresh or remote publication is part of this
commitment; subscription/API behavior is tested with local protocol fixtures.
