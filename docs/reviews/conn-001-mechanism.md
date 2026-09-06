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
