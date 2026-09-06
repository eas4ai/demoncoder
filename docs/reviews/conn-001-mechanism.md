# CONN-001 mechanism review

The connection mechanism builds the application and runs the four controlled
tool cycles, then validates retained live records from the committed inputs.
Fixtures do not replace live evidence. Missing records or credentials cannot
produce a CONN-001 pass.

The live runner drives the production executable through a pseudo-terminal,
with its built-in connection selection and default provider endpoints.
It never substitutes a fixture executable or copies subscription secrets.
It creates a temporary repository and an unpredictable seed. The first
prompt requires read, write, edit, and Bash to create and verify a function.
The second prompt extends that same function without repeating its name.
Each turn must complete, retain successful required tool operations and an
actual Python assertion command, and produce the required source.

The runner parses source with Python's AST library. It requires exactly one
zero-argument function with one integer-literal return, checks its exact
value against the seed, and checks the second turn preserves its name.
It does not execute model-written code on the host. These deliberately small
tasks establish the live tool/session path, not arbitrary coding quality.

Records include original tool and session events, source after each turn,
connection, authentication category, model selection, installed backend
version, and committed input digest. The checker rejects a stale digest,
wrong transport or authentication category, unrelated result identity,
incorrect or renamed function, failed turn, or missing Python check. The
record is evidence subject to developer review, not cryptographic proof that
an external service was used. Cairn cannot establish honesty by itself.

Safe failure demonstrations: `python3 tests/live_evidence.py` accepts its
corrected in-memory record and rejects eight controlled violations covering
those boundaries. These synthetic examples are never saved as live records.
Live executions and their unresolved prerequisites are recorded separately;
no live pass is claimed merely because the validator tests pass.

Live runs on 2026-09-06: Codex 0.153.4 with gpt-6-astra and Claude Code
2.1.263 with sonnet completed both turns and passed the source/tool checks.
The current records are retained under `.cairn/evidence/live/`. An earlier
Codex run read the seed but refused edits because its advertised permission
policy was read-only. Changing that policy to workspace-write, while keeping
the empty environment selection and all confined tool routing, passed the
installed-backend boundary checks and then the live coding task.

The first API attempts stopped because environment keys were absent. The
developer then directed normal home settings and environment credentials.
The older home configuration contained both API keys; each provider accepted
its key for a model-list request. The new private settings file preserves
those credentials without copying them into repository fixtures or events.
The live runner now admits the same normal home configuration path, rejects
custom endpoints or executables for live evidence, and redacts saved keys
as well as environment credentials. New live task records remain required
against this changed implementation.

Configuration checks cover both APIs' saved credentials and environment
precedence, saved model/effort and CLI overrides for all four transports,
backend login-directory environment variables, and credential absence in
events and subscription subprocess environments. Seven invalid-file/settings
cases, an explicitly blank environment key, and a FIFO reject before terminal
startup without quoting the synthetic secret. The FIFO check also establishes
that opening a special file cannot hang configuration loading.

Failure demonstration: temporarily sending OpenAI effort under an ignored
field made the home-settings tool-cycle case fail at the provider's actual
request assertion. Restoring the production field passed all four cases.
The complete CODE-001 through CODE-008 driver passed after the configuration
changes. The configuration suite and both home/override four-adapter cases
passed. These editing checks are separate from the committed Cairn receipts.
