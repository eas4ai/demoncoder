// Documentary type inventory, not the application's runtime validator.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';

const [compilerPath, sdkPath, mode = '--check'] = process.argv.slice(2);
assert(compilerPath && sdkPath && ['--check', '--write'].includes(mode),
  'Usage: node build-claude-wire.mjs /path/to/typescript.js /path/to/sdk.d.ts [--check|--write]');
const ts = createRequire(import.meta.url)(compilerPath);
assert.equal(ts.version, '5.9.2', 'Use the recorded parser version');
const sha = value => createHash('sha256').update(value).digest('hex');
const bytes = readFileSync(sdkPath);
const sourceDigest = '5d243d3837ac5cf211470f11bcb51eb77cb5a0c8554074935d5c2c4c159d0cf3';
assert.equal(sha(bytes), sourceDigest, 'SDK source must match the pinned artifact');
const source = ts.createSourceFile('sdk.d.ts', bytes.toString(), ts.ScriptTarget.Latest, true);
assert.equal(source.parseDiagnostics.length, 0, 'SDK must parse without recovery');
const declarations = new Map(source.statements
  .filter(node => ts.isTypeAliasDeclaration(node) || ts.isInterfaceDeclaration(node))
  .map(node => [node.name.text, node]));
const roots = ['HookInput', 'HookJSONOutput', 'HookEvent', 'HookCallbackMatcher',
  'SDKHookStartedMessage', 'SDKHookProgressMessage', 'SDKHookResponseMessage'];
const selected = new Set();
const external = new Set();
function select(name) {
  if (selected.has(name)) return;
  const declaration = declarations.get(name);
  if (!declaration) { external.add(name); return; }
  selected.add(name);
  function visit(node) {
    if (ts.isTypeReferenceNode(node)) select(node.typeName.getText(source));
    ts.forEachChild(node, visit);
  }
  visit(declaration);
}
roots.forEach(select);
assert.deepEqual([...external].sort(), ['AbortSignal', 'Promise', 'Record', 'UUID']);

function object(members) {
  const properties = {};
  for (const member of members) {
    assert(ts.isPropertySignature(member), `Unmapped member: ${member.getText(source)}`);
    const name = member.name.text ?? member.name.getText(source);
    assert(!(name in properties), `Duplicate property: ${name}`);
    properties[name] = { required: !member.questionToken, type: type(member.type) };
  }
  return { kind: 'object', properties };
}
function type(node) {
  assert(node, 'Every type must be explicit');
  if (ts.isParenthesizedTypeNode(node)) return type(node.type);
  if (ts.isTypeReferenceNode(node)) {
    return { kind: 'reference', name: node.typeName.getText(source),
      arguments: (node.typeArguments ?? []).map(type) };
  }
  if (ts.isTypeLiteralNode(node)) return object(node.members);
  if (ts.isUnionTypeNode(node) || ts.isIntersectionTypeNode(node)) {
    // Content-based branch identities survive reordering. No flattening across branches.
    const branches = {};
    for (const child of node.types) {
      const value = type(child);
      const id = sha(JSON.stringify(value));
      assert(!(id in branches), 'Duplicate branch requires explicit normalization');
      branches[id] = value;
    }
    return { kind: ts.isUnionTypeNode(node) ? 'union' : 'intersection', branches };
  }
  if (ts.isArrayTypeNode(node)) return { kind: 'array', element: type(node.elementType) };
  if (ts.isLiteralTypeNode(node)) {
    const literal = node.literal;
    if (ts.isStringLiteral(literal)) return { kind: 'literal', value: literal.text };
    if (ts.isNumericLiteral(literal)) return { kind: 'literal', value: Number(literal.text) };
    const values = new Map([[ts.SyntaxKind.TrueKeyword, true],
      [ts.SyntaxKind.FalseKeyword, false], [ts.SyntaxKind.NullKeyword, null]]);
    assert(values.has(literal.kind), `Unmapped literal: ${literal.getText(source)}`);
    return { kind: 'literal', value: values.get(literal.kind) };
  }
  if (ts.isFunctionTypeNode(node)) {
    assert(!node.typeParameters?.length, 'Generic callback needs an explicit mapping');
    return { kind: 'function', parameters: node.parameters.map(parameter => ({
      name: parameter.name.getText(source), required: !parameter.questionToken,
      rest: !!parameter.dotDotDotToken, type: type(parameter.type),
    })), result: type(node.type) };
  }
  const primitives = new Map([[ts.SyntaxKind.StringKeyword, 'string'],
    [ts.SyntaxKind.NumberKeyword, 'number'], [ts.SyntaxKind.BooleanKeyword, 'boolean'],
    [ts.SyntaxKind.UnknownKeyword, 'unknown'], [ts.SyntaxKind.UndefinedKeyword, 'undefined']]);
  assert(primitives.has(node.kind), `Unmapped type: ${ts.SyntaxKind[node.kind]}`);
  return { kind: primitives.get(node.kind) };
}

const types = {};
for (const name of [...selected].sort()) {
  const declaration = declarations.get(name);
  assert(!declaration.typeParameters?.length && !declaration.heritageClauses?.length,
    `Generic or inherited interface needs an explicit mapping: ${name}`);
  types[name] = ts.isInterfaceDeclaration(declaration)
    ? object(declaration.members) : type(declaration.type);
}
const fieldPaths = [];
function fields(node, path) {
  if (node.kind === 'object') {
    for (const [name, property] of Object.entries(node.properties)) {
      const escaped = name.replaceAll('~', '~0').replaceAll('/', '~1');
      const at = `${path}/properties/${escaped}`;
      fieldPaths.push({ path: at, required: property.required });
      fields(property.type, `${at}/type`);
    }
  } else if (node.branches) {
    for (const [id, branch] of Object.entries(node.branches)) fields(branch, `${path}/branches/${id}`);
  } else if (node.kind === 'array') fields(node.element, `${path}/element`);
  else if (node.kind === 'function') {
    node.parameters.forEach((parameter, index) => fields(parameter.type, `${path}/parameters/${index}/type`));
    fields(node.result, `${path}/result`);
  } else if (node.kind === 'reference') {
    node.arguments.forEach((argument, index) => fields(argument, `${path}/arguments/${index}`));
  }
}
for (const [name, node] of Object.entries(types)) fields(node, `#/types/${name}`);
const graph = { format: 'typescript-type-graph-v1', parser: 'typescript@5.9.2',
  source: { file: 'sdk.d.ts', sha256: sourceDigest }, roots,
  external_types: {
    AbortSignal: 'SDK callback control object; not a JSON payload or imported feature',
    Promise: 'SDK callback asynchronous return wrapper; validate its resolved type',
    Record: 'String-keyed dictionary; validate keys and values against its arguments',
    UUID: 'String alias imported by the SDK; preserve the source identifier value',
  },
  events: Object.values(types.HookEvent.branches).map(node => node.value), types,
  field_paths: fieldPaths,
  coverage: 'Traverse references and every branch from each root; field paths identify declaration sites, not flattened instance fields',
};
const profilePath = fileURLToPath(new URL('./plugin-profile-v1.json', import.meta.url));
const profile = JSON.parse(readFileSync(profilePath, 'utf8'));
if (mode === '--write') {
  profile.claude_wire = graph;
  writeFileSync(profilePath, JSON.stringify(profile, null, 2) + '\n');
} else {
  assert.deepEqual(profile.claude_wire, graph, 'Frozen graph differs from its pinned SDK source');
}
console.log(`${mode}: ${selected.size} types, ${graph.events.length} events, ${fieldPaths.length} scoped fields; all source references resolved`);
