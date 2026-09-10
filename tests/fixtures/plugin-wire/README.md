# Wire validation fixtures

These frozen, synthetic JSON payloads come from agreed profile v1 revision 2.
They contain no real session or credential values. They establish shape validation
and model-outcome interpretation, not runner execution, permission effects,
rewritten arguments, persistence, admission, or user-interface behavior.

## Claude graph coverage

`claude-graph.json` contains 951 serializable examples. Each type has complete
object fields and one-at-a-time variations of nested fields and union alternatives.
The recorded event/control root fixtures include their reference closures.
Callback functions and AbortSignal use the actual Rust callback binding, not JSON
stand-ins; an empty callback array remains a valid serializable shape.

Production graph tests validate all examples. Root-only mutation tests remove each
distinct field and all 218 union branches. Identical inherited declarations use a
conflicting type because deleting one copy is observationally equivalent. Separate
root-level probes replace each reachable JSON field with an invalid value and
remove every required field at its actual nested payload location. Unknown-valued
fields admit bounded JSON and receive limit probes. These checks cover 316 graph
field declarations, including the separately bound callback control property.

## Full JSON Schema coverage

`full-schemas.json` contains 289 positive examples spanning all 29 portable/Codex
schemas. Codex configuration definitions have separate keys despite sharing a path.
These positives alone do not establish negative constraint coverage.

`full-schema-probes.json` adds 1,042 named probes keyed by the exact schema identity
and reachable schema JSON pointer. A probe references an unchanged positive fixture
and records explicit data/schema patches; it does not regenerate expected results
from the mutated runtime schema. Its 649 negatives include invalid field types,
132 required-member deletions, array item errors, forbidden properties, enum/const
violations, string/numeric bounds, and forbidden property names. It also covers all
82 enum alternatives, 24 nullable-type alternatives, all reference closures, and
oneOf/allOf branches. Boundary constraints have exact-boundary positives; each
oneOf receives a duplicate-matching-branch probe to verify exclusivity.

The production test independently traverses the retained schema and its references
to enumerate required probe identities. Missing schemas, fields, alternatives,
constraints, or unrecognized vocabulary fail coverage. It then validates frozen
positive/negative payloads through `CompatibilityProfile::validate_schema` and
compiles each controlled mutation with the production `SchemaValidator`:

- 591 erasures admit an unchanged negative, demonstrating that those constraints
  are necessary to reject the violating payload.
- 443 mutations reject an unchanged positive, covering fields, alternative removal,
  oneOf exclusivity, inverted constraints, and conflicting constraints.
- Eight `format` probes retain their annotation-only behavior (`uint`/`uint64`);
  type/minimum constraints are tested separately.

Deletion is not distinguishable where another constraint implies the same rule:
49 such probes retain the invalid payload and use a conflicting constraint to prove
that occurrence is evaluated. Two `additionalProperties: true` probes likewise use
false to distinguish the explicit open schema; one `not` probe records that removal
of an inner constraint tightens rather than loosens acceptance. Eight unconstrained
source fields use bounded-JSON negatives rather than invented type restrictions.
Every probe records its mutation effect and any redundancy explanation.

The optional `generate_full_schema_probes.py` generator uses an independent,
pinned Python JSON Schema implementation and rejects external references. It uses
each schema's declared draft. Normal Rust tests consume only the frozen JSON and do
not require Python packages or network access. Regenerate from the repository root:

```sh
python3 -m venv /tmp/demoncoder-schema-probes
/tmp/demoncoder-schema-probes/bin/pip install -r tests/fixtures/plugin-wire/schema-probe-generator-requirements.txt
/tmp/demoncoder-schema-probes/bin/python tests/fixtures/plugin-wire/generate_full_schema_probes.py
```

The generator was run with Python 3.14 and python-jsonschema 4.26.0. Both Python and
Rust accepted/rejected the retained probes consistently. Do not weaken the schemas
or replace a negative with an unrelated violation to reconcile a future disagreement.
