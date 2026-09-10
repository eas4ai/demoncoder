// Compare independently compiled SDK examples with types reconstructed from the inventory.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { resolve } from 'node:path';
const [compilerPath, sdkPath] = process.argv.slice(2);
assert(compilerPath && sdkPath, 'Pass the pinned TypeScript compiler and sdk.d.ts paths');
const ts = createRequire(import.meta.url)(compilerPath);
assert.equal(ts.version, '5.9.2');
const graph = JSON.parse(readFileSync(new URL('./plugin-profile-v1.json', import.meta.url))).claude_wire;
function emit(node) {
  switch (node.kind) {
    case 'reference': return node.name + (node.arguments.length ? `<${node.arguments.map(emit).join(',')}>` : '');
    case 'literal': return JSON.stringify(node.value);
    case 'array': return `Array<${emit(node.element)}>`;
    case 'union': return `(${Object.values(node.branches).map(emit).join(' | ')})`;
    case 'intersection': return `(${Object.values(node.branches).map(emit).join(' & ')})`;
    case 'object': return '{' + Object.entries(node.properties).map(([name, property]) =>
      `${JSON.stringify(name)}${property.required ? '' : '?'}: ${emit(property.type)};`).join('') + '}';
    case 'function': return '(' + node.parameters.map(parameter =>
      `${parameter.rest ? '...' : ''}${parameter.name}${parameter.required ? '' : '?'}: ${emit(parameter.type)}`).join(',') + `) => ${emit(node.result)}`;
    default:
      assert(['string', 'number', 'boolean', 'unknown', 'undefined'].includes(node.kind));
      return node.kind;
  }
}
const base = { session_id: 's', transcript_path: '/transcript', cwd: '/workspace' };
const cases = [
  ['base-no-effort', 'BaseHookInput', base, true],
  ['nested-effort', 'BaseHookInput', { ...base, effort: { level: 'high' } }, true],
  ['missing-nested-level', 'BaseHookInput', { ...base, effort: {} }, false],
  ['flat-is-not-nested', 'BaseHookInput', { ...base, effort: {}, level: 'high' }, false],
  ['allow', 'PermissionRequestHookSpecificOutput', { hookEventName: 'PermissionRequest', decision: { behavior: 'allow', updatedInput: { command: 'check' } } }, true],
  ['deny', 'PermissionRequestHookSpecificOutput', { hookEventName: 'PermissionRequest', decision: { behavior: 'deny', interrupt: true } }, true],
  ['invalid-discriminator', 'PermissionRequestHookSpecificOutput', { hookEventName: 'PermissionRequest', decision: { behavior: 'other' } }, false],
  ['wrong-branch-field', 'PermissionRequestHookSpecificOutput', { hookEventName: 'PermissionRequest', decision: { behavior: 'deny', updatedInput: {} } }, false],
  ['inherited-session-end', 'HookInput', { ...base, hook_event_name: 'SessionEnd', reason: 'other' }, true],
  ['missing-inherited-input', 'HookInput', { hook_event_name: 'SessionEnd', reason: 'other' }, false],
];
function results(types, sourceSdk) {
  const directory = resolve('/tmp/demoncoder-wire-type-probes');
  const files = new Map();
  const definition = `${directory}/graph.d.ts`;
  if (!sourceSdk) files.set(definition, 'type UUID = string;\n' + Object.entries(types)
    .map(([name, node]) => `export type ${name} = ${emit(node)};`).join('\n'));
  for (const [name, type, value] of cases) {
    const imported = (sourceSdk ? resolve(sdkPath) : definition).replace(/\.d\.ts$/, '');
    files.set(`${directory}/${name}.ts`, `import type { ${type} } from ${JSON.stringify(imported)};\nconst probe: ${type} = ${JSON.stringify(value)};\n`);
  }
  const options = { strict: true, noEmit: true, skipLibCheck: true,
    target: ts.ScriptTarget.ES2022, moduleResolution: ts.ModuleResolutionKind.Node10 };
  const host = ts.createCompilerHost(options);
  const read = host.readFile.bind(host), exists = host.fileExists.bind(host);
  host.readFile = path => files.get(path) ?? read(path);
  host.fileExists = path => files.has(path) || exists(path);
  const directoryExists = host.directoryExists.bind(host);
  host.directoryExists = path => path === directory || directoryExists(path);
  const program = ts.createProgram([...files.keys()], options, host);
  assert(program.getSourceFile(sourceSdk ? resolve(sdkPath) : definition), 'Probe import was not loaded');
  const diagnostics = ts.getPreEmitDiagnostics(program);
  const failures = new Set(diagnostics.filter(item => item.file?.fileName.startsWith(directory))
    .map(item => item.file.fileName));
  return cases.map(([name]) => !failures.has(`${directory}/${name}.ts`));
}
const expected = cases.map(item => item[3]);
assert.deepEqual(results(graph.types, true), expected, 'Pinned SDK oracle disagrees with the probes');
assert.deepEqual(results(graph.types, false), expected, 'Inventory loses source type semantics');
const missingLevel = structuredClone(graph.types);
delete missingLevel.BaseHookInput.properties.effort.type.properties.level;
assert.notDeepEqual(results(missingLevel, false), expected, 'Nested-field deletion escaped the probes');
const missingAllow = structuredClone(graph.types);
const branches = missingAllow.PermissionRequestHookSpecificOutput.properties.decision.type.branches;
delete branches[Object.keys(branches).find(key => branches[key].properties.behavior.type.value === 'allow')];
assert.notDeepEqual(results(missingAllow, false), expected, 'Permission-branch deletion escaped the probes');
console.log(`PASS: ${cases.length} SDK/inventory assignability probes; nested-field and union-branch deletion both detected`);
