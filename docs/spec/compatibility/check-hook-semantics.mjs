// Validate the finite specification tables, not a runtime hook implementation.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
const [ajvPath] = process.argv.slice(2);
assert(ajvPath, 'Pass the Ajv package path');
const Ajv = createRequire(import.meta.url)(ajvPath);
const ajv = new Ajv({ strict: false, allErrors: true });
const profile = JSON.parse(readFileSync(new URL('./plugin-profile-v1.json', import.meta.url)));
const table = profile.hook_applicability;
const events = [...profile.claude_wire.events, ...profile.native_additional_events];
const handlers = profile.required_handler_types;
function coverage(candidate) {
  assert.deepEqual(Object.keys(candidate.dialects).sort(), ['claude', 'codex', 'native']);
  let count = 0;
  for (const dialect of Object.values(candidate.dialects)) {
    assert.deepEqual(Object.keys(dialect).sort(), [...events].sort());
    for (const group of Object.values(dialect)) {
      assert(candidate.groups[group], `Missing group ${group}`);
      assert.deepEqual(Object.keys(candidate.groups[group]).sort(), [...handlers].sort());
      for (const status of Object.values(candidate.groups[group])) {
        assert(candidate.statuses[status], `Unspecified status ${status}`);
        count += 1;
      }
    }
  }
  return count;
}
assert.equal(coverage(table), 510);
const missing = structuredClone(table);
delete missing.dialects.claude.PermissionRequest;
assert.throws(() => coverage(missing), 'Deleting one event must fail coverage');
const status = (dialect, event, handler) => table.groups[table.dialects[dialect][event]][handler];
assert.equal(status('claude', 'SessionStart', 'http'), 'no-source-handler');
assert.equal(status('claude', 'Setup', 'mcp_tool'), 'run');
assert.equal(status('claude', 'PreCompact', 'agent'), 'no-source-handler');
assert.equal(status('codex', 'Stop', 'prompt'), 'source-nonexecuting');
assert.equal(status('codex', 'SessionEnd', 'mcp_tool'), 'source-nonexecuting');
assert.equal(status('codex', 'SessionEnd', 'command'), 'run');
assert.equal(status('codex', 'SessionStart', 'mcp_tool'), 'run');
assert.equal(status('native', 'SessionEnd', 'mcp_tool'), 'run');
assert.equal(status('native', 'SessionStart', 'http'), 'run');
const rules = profile.model_result_rules;
function decision(node, conditions = {}) {
  if (typeof node === 'string') {
    assert(rules.outcomes[node], `Unspecified outcome: ${node}`);
    return node;
  }
  assert(rules.conditions[node.when], `Unspecified condition: ${node.when}`);
  return decision(conditions[node.when] ? node.then : node.else, conditions);
}
for (const event of events) {
  for (const kind of ['prompt', 'agent']) {
    if (status('claude', event, kind) === 'run') {
      const rule = rules.claude[event]?.ok_false[kind];
      assert(rule, `Missing model rule: ${event}/${kind}`);
      for (const mask of Array.from({ length: 8 }, (_, index) => index)) {
        decision(rule, { continueOnBlock: !!(mask & 1), impossible: !!(mask & 2), 'teammate-stop': !!(mask & 4) });
      }
    }
    assert(rules.native[event]);
    decision(rules.native[event].ok_false);
  }
}
const result = (event, kind, conditions) => decision(rules.claude[event].ok_false[kind], conditions);
assert.equal(result('PermissionRequest', 'prompt'), 'no-source-decision');
assert.equal(result('PermissionDenied', 'agent'), 'no-source-decision');
assert.equal(result('Stop', 'prompt', { impossible: true }), 'stop-unmet');
assert.equal(result('Stop', 'prompt'), 'bounded-correction');
assert.equal(result('Stop', 'agent'), 'bounded-correction');
assert.equal(result('PreToolUse', 'prompt'), 'deny-tool-and-end-turn');
assert.equal(result('PreToolUse', 'prompt', { continueOnBlock: true }), 'deny-tool-and-continue');
assert.equal(result('PreToolUse', 'agent'), 'deny-tool-and-continue');
assert.equal(result('PostToolUse', 'prompt'), 'end-turn-unmet');
assert.equal(result('PostToolUse', 'prompt', { continueOnBlock: true }), 'continue-after-result');
assert.equal(result('PostToolUse', 'agent'), 'continue-after-result');
assert.equal(result('TaskCompleted', 'prompt', { 'teammate-stop': true }), 'stop-unmet');
assert.equal(result('TaskCompleted', 'agent', { 'teammate-stop': true }), 'keep-working');
assert.equal(result('TaskCompleted', 'prompt'), 'reject-and-continue');
for (const [name, schema] of Object.entries(profile.model_response_schemas)) {
  const validate = ajv.compile(schema);
  assert(validate({ ok: true }), name);
  assert(validate({ ok: false, reason: 'Unmet condition' }), name);
  assert(!validate({ ok: false }), name);
  assert(!validate({ ok: 'false', reason: 'Invalid type' }), name);
  assert.equal(validate({ ok: false, reason: 'Cannot satisfy', impossible: true }), name.endsWith('prompt'), name);
}
console.log('PASS: 510 applicability cells; missing-cell mutation rejected; all model branches resolved; 14 outcome cases; four model schemas validated');
