import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import { stageBenchmarkInputs } from './benchmark-intake.mjs';

const hash = value => crypto.createHash('sha256').update(value).digest('hex');
function fixture(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'pulsar-intake-test-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const out = path.join(root, 'out'); fs.mkdirSync(out);
  const media = Buffer.from('authored source bytes');
  fs.writeFileSync(path.join(root, 'source'), media);
  const reference = { schema_version: 1, subject_id: 'subject', window: { start_ns: '0', end_ns: '1000000000' }, strokes: [], material_sections: [] };
  const bytes = JSON.stringify(reference);
  fs.writeFileSync(path.join(root, 'reference.json'), bytes);
  const manifest = { schema_version: 1, policy_version: 'B1.1', entries: [{
    id: 'subject', category: 'synthetic-kat', split: 'development', source_group_id: 'source-family', evidence_class: 'synthetic', source_sha256: hash(media),
    rights: { status: 'permitted', evidence_id: 'authored-fixture', permitted_uses: ['evaluation'], redistribution: 'forbidden' },
    annotation_provenance: { reference_author: 'fixture', independent: false, method: 'analytic', review_record: 'fixture-only' },
    reference: { path: 'reference.json', sha256: hash(bytes) }, candidate: { path: 'future-candidate.json', sha256: '0'.repeat(64) },
  }] };
  const mapping = { schema_version: 1, entries: [{ entry_id: 'subject', path: 'source' }] };
  const options = { corpus: path.join(root, 'corpus.json'), sources: path.join(root, 'sources.json'), outputRoot: out, maxSourceBytes: 1024, maxTotalBytes: 2048 };
  const save = () => { fs.writeFileSync(options.corpus, JSON.stringify(manifest)); fs.writeFileSync(options.sources, JSON.stringify(mapping)); };
  save();
  return { root, out, media, manifest, mapping, options, save };
}

test('snapshot binds copied bytes and reference; later source changes do not alter it', t => {
  const f = fixture(t), result = stageBenchmarkInputs(f.options);
  fs.writeFileSync(path.join(f.root, 'source'), 'changed');
  assert.deepEqual(fs.readFileSync(path.join(result.directory, result.receipt.entries[0].source.path)), f.media);
  assert.equal(result.receipt.source_bytes, f.media.length);
  assert.equal(result.receipt.entries[0].source.sha256, hash(f.media));
  assert.equal(result.receipt.entries[0].reference.sha256, f.manifest.entries[0].reference.sha256);
  assert.equal(result.receipt.execution, null);
  assert.equal(result.receipt.release_qualified, false);
  if (process.platform !== 'win32') assert.equal(fs.statSync(result.directory).mode & 0o777, 0o700);
});

for (const [name, change, expected] of [
  ['wrong media digest', f => { f.manifest.entries[0].source_sha256 = 'a'.repeat(64); }, /source digest/],
  ['per-file budget', f => { f.options.maxSourceBytes = 1; }, /byte budget/],
  ['aggregate budget', f => { f.options.maxTotalBytes = 1; }, /byte budget/],
  ['wrong reference digest', f => { f.manifest.entries[0].reference.sha256 = 'a'.repeat(64); }, /reference digest/],
  ['missing evaluation permission', f => { f.manifest.entries[0].rights.permitted_uses = []; }, /not permitted/],
  ['duplicate source identity', f => { f.mapping.entries.push({ ...f.mapping.entries[0] }); }, /duplicate/],
  ['missing source', f => { f.mapping.entries = []; }, /cover selected/],
  ['unfrozen heldout', f => { f.manifest.entries[0].split = 'heldout'; f.options.split = 'heldout'; }, /heldout lock/],
  ['reference subject mismatch', f => { const ref = { schema_version: 1, subject_id: 'other', window: { start_ns: '0', end_ns: '1' }, strokes: [] }; const bytes = JSON.stringify(ref); fs.writeFileSync(path.join(f.root, 'reference.json'), bytes); f.manifest.entries[0].reference.sha256 = hash(bytes); }, /subject identity/],
  ['content split leakage', f => { const entry = structuredClone(f.manifest.entries[0]); entry.id = 'training'; entry.split = 'training'; entry.rights.permitted_uses = ['training']; f.manifest.entries.push(entry); }, /leakage/],
  ['source directory', f => { f.mapping.entries[0].path = 'out'; }, /regular file/],
]) test(`rejects ${name} without retaining a partial intake`, t => {
  const f = fixture(t); change(f); f.save();
  assert.throws(() => stageBenchmarkInputs(f.options), expected);
  assert.deepEqual(fs.readdirSync(f.out), []);
  assert.equal(fs.readFileSync(path.join(f.root, 'source'), 'utf8'), f.media.toString());
});

test('source symlink cannot escape declared input root', t => {
  const f = fixture(t);
  fs.symlinkSync(os.tmpdir(), path.join(f.root, 'escape'));
  f.mapping.entries[0].path = 'escape'; f.save();
  assert.throws(() => stageBenchmarkInputs(f.options), /escapes input root/);
  assert.deepEqual(fs.readdirSync(f.out), []);
});

test('previous successful snapshot survives a later failed intake', t => {
  const f = fixture(t), first = stageBenchmarkInputs(f.options);
  f.manifest.entries[0].source_sha256 = 'a'.repeat(64); f.save();
  assert.throws(() => stageBenchmarkInputs(f.options), /source digest/);
  assert.deepEqual(fs.readdirSync(f.out), [path.basename(first.directory)]);
  assert.deepEqual(fs.readFileSync(path.join(first.directory, '0-source')), f.media);
});
