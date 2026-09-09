# Admit only owned child source inside the session container

Level: Judged
Decided by: Codex
Supersedes: reject-private-root-overlap-before-capture-and-retire-unsafe-snapshots
Cause: an unforeseen condition occurred
Rests on: AUD-001 AUD-002 AUD-003 SUB-003 SUB-005
Would be wrong if: Ordinary capture exports private roots, a dynamic credential declaration is ignored for a child, or validated child source cannot be checked and integrated
History: Fixed relative names missed relocated credential roots. Complete root rejection repaired that leak but also blocked application-owned child worktrees inside the private session container. Preserve the complete guard for ordinary sources and distinguish only the validated child owner at capture.

## Decision

Keep the shared complete private-root refusal for ordinary workspaces. The worktree owner may capture only its newly materialized or administratively validated child with an internal owned-worktree marker. For that path only, omit the HOME-based DemonCoder session container from the root prohibition; retain all other HOME roots, every explicit environment credential declaration, fixed private-name exclusions, and unsafe historical-version refusal. The exception is internal, is not a CLI scope, and cannot change child tool permissions.

## Realized by

- fc7869f85a2a096ee6fdf2d6fefacb1b82f44e1a Preserve owned child capture without admitting private workspaces

CaptureRoot distinguishes ordinary workspaces from the worktree owner. The owner exception omits only the HOME session store. The generated-output terminal regression and isolated-process positive/negative checks pass.
