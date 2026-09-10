// Retain the complete authoritative source schemas, rather than reconstructing constraints from field lists.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
const [codexRoot, portableDirectory, mode = '--check'] = process.argv.slice(2);
assert(codexRoot && portableDirectory && ['--check', '--write'].includes(mode),
  'Pass the pinned Codex root, portable schema directory and --check or --write');
const profileUrl = new URL('./plugin-profile-v1.json', import.meta.url);
const profile = JSON.parse(readFileSync(profileUrl));
function load(path, digest) {
  const bytes = readFileSync(path);
  assert.equal(createHash('sha256').update(bytes).digest('hex'), digest, path);
  return JSON.parse(bytes);
}
for (const item of profile.codex_wire) {
  const document = load(join(codexRoot, item.path), item.sha256);
  let schema = document;
  if (item.definition) {
    // Preserve all and only the reference closure for this definition.
    schema = { $schema: document.$schema, $ref: `#/definitions/${item.definition}`, definitions: {} };
    function visit(node) {
      if (!node || typeof node !== 'object') return;
      if (node.$ref) {
        assert(node.$ref.startsWith('#/definitions/'), `Unresolved external schema reference: ${node.$ref}`);
        const name = node.$ref.slice('#/definitions/'.length);
        if (!(name in schema.definitions)) {
          assert(document.definitions[name], `Missing definition: ${name}`);
          schema.definitions[name] = document.definitions[name];
          visit(document.definitions[name]);
        }
      }
      Object.values(node).forEach(visit);
    }
    visit({ $ref: schema.$ref });
  }
  if (mode === '--write') item.schema = schema;
  else assert.deepEqual(item.schema, schema, `Schema drift: ${item.path} ${item.definition ?? ''}`);
}
for (const item of profile.source_revisions.portable) {
  const schema = load(join(portableDirectory, `${item.name}-portable-schema.json`), item.sha256);
  if (mode === '--write') item.schema = schema;
  else assert.deepEqual(item.schema, schema, `Portable schema drift: ${item.name}`);
}
if (mode === '--write') writeFileSync(profileUrl, JSON.stringify(profile, null, 2) + '\n');
console.log(`${mode}: complete Codex and portable schemas match pinned source bytes`);
