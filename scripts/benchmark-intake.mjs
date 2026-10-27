#!/usr/bin/env node
/** Private, hash-bound input staging for production benchmark campaigns. */
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { performance } from 'node:perf_hooks';
import { pathToFileURL } from 'node:url';
import { validateAnnotation, validateCorpus } from './benchmark-good.mjs';

const JSON_LIMIT = 8 * 1024 * 1024;
const CHUNK = 1024 * 1024;
const digest = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
function requireValue(ok, message) { if (!ok) throw new Error(message); }
function fields(value, allowed, label) {
  requireValue(value && typeof value === 'object' && !Array.isArray(value), `${label}: expected object`);
  requireValue(Object.keys(value).length === allowed.length && allowed.every(k => Object.hasOwn(value, k)), `${label}: fields differ from schema`);
}
function positive(value, maximum, label) {
  requireValue(Number.isSafeInteger(value) && value > 0 && value <= maximum, `${label}: invalid budget`);
}
function readJson(filename) {
  const fd = fs.openSync(filename, fs.constants.O_RDONLY | fs.constants.O_NONBLOCK);
  try {
    const stat = fs.fstatSync(fd);
    requireValue(stat.isFile() && stat.size <= JSON_LIMIT, 'JSON type/size budget exceeded');
    const buffer = Buffer.alloc(JSON_LIMIT + 1);
    let used = 0;
    while (used < buffer.length) {
      const n = fs.readSync(fd, buffer, used, buffer.length - used, null);
      if (!n) break;
      used += n;
    }
    requireValue(used <= JSON_LIMIT, 'JSON grew beyond byte budget');
    const bytes = buffer.subarray(0, used);
    return { value: JSON.parse(bytes.toString('utf8')), bytes, sha256: digest(bytes) };
  } finally { fs.closeSync(fd); }
}
function containedFile(root, relative) {
  requireValue(typeof relative === 'string' && relative.length > 0 && relative.length <= 4096 && !path.isAbsolute(relative), 'artifact path must be relative');
  const resolved = fs.realpathSync(path.resolve(root, relative));
  const remainder = path.relative(root, resolved);
  requireValue(remainder !== '..' && !remainder.startsWith('..' + path.sep) && !path.isAbsolute(remainder), 'artifact escapes input root');
  return resolved;
}
function copyBounded(source, destination, expected, budget, deadline) {
  const input = fs.openSync(source, fs.constants.O_RDONLY | fs.constants.O_NOFOLLOW | fs.constants.O_NONBLOCK);
  let output;
  try {
    const stat = fs.fstatSync(input);
    requireValue(stat.isFile(), 'source must be a regular file');
    requireValue(stat.size <= budget.perFile && stat.size <= budget.remaining, 'source byte budget exceeded');
    output = fs.openSync(destination, 'wx', 0o600);
    const buffer = Buffer.alloc(CHUNK), hash = crypto.createHash('sha256');
    let total = 0;
    for (;;) {
      requireValue(performance.now() <= deadline, 'intake deadline exceeded');
      const count = fs.readSync(input, buffer, 0, buffer.length, null);
      if (!count) break;
      total += count;
      requireValue(total <= budget.perFile && total <= budget.remaining, 'growing source exceeded byte budget');
      hash.update(buffer.subarray(0, count));
      for (let written = 0; written < count;) {
        const n = fs.writeSync(output, buffer, written, count - written);
        requireValue(n > 0, 'snapshot write made no progress');
        written += n;
      }
    }
    requireValue(hash.digest('hex') === expected, 'source digest mismatch');
    fs.fsyncSync(output);
    fs.fchmodSync(output, 0o400);
    budget.remaining -= total;
    return { path: path.basename(destination), sha256: expected, byte_len: total };
  } finally {
    fs.closeSync(input);
    if (output !== undefined) fs.closeSync(output);
  }
}

export function stageBenchmarkInputs({ corpus, sources, outputRoot, split = 'development', maxSourceBytes, maxTotalBytes, timeoutMs = 60000 }) {
  requireValue(['development', 'heldout'].includes(split), 'training is not an evaluation split');
  positive(maxSourceBytes, 64 * 1024 ** 3, 'maxSourceBytes');
  positive(maxTotalBytes, 256 * 1024 ** 3, 'maxTotalBytes');
  positive(timeoutMs, 86400000, 'timeoutMs');
  const deadline = performance.now() + timeoutMs;
  const corpusPath = fs.realpathSync(corpus), sourcePath = fs.realpathSync(sources);
  const parsed = readJson(corpusPath), mapping = readJson(sourcePath);
  const manifest = validateCorpus(parsed.value);
  fields(mapping.value, ['schema_version', 'entries'], 'source map');
  requireValue(mapping.value.schema_version === 1 && Array.isArray(mapping.value.entries) && mapping.value.entries.length <= 256, 'invalid source map version/entry count');
  const selected = manifest.entries.filter(e => e.split === split);
  requireValue(selected.length > 0, 'selected evaluation split is empty');
  const selectedIds = new Set(selected.map(e => e.id)), files = new Map();
  for (const entry of mapping.value.entries) {
    fields(entry, ['entry_id', 'path'], 'source map entry');
    requireValue(selectedIds.has(entry.entry_id) && !files.has(entry.entry_id), 'unknown or duplicate selected source identity');
    files.set(entry.entry_id, containedFile(path.dirname(sourcePath), entry.path));
  }
  requireValue(files.size === selected.length, 'source map must cover selected split exactly');
  let referenceBytes = 0;
  const references = selected.map(entry => {
    fields(entry.reference, ['path', 'sha256'], 'reference');
    const ref = readJson(containedFile(path.dirname(corpusPath), entry.reference.path));
    referenceBytes += ref.bytes.length;
    requireValue(referenceBytes <= 64 * 1024 * 1024, 'aggregate reference byte budget exceeded');
    requireValue(ref.sha256 === entry.reference.sha256, 'reference digest mismatch');
    requireValue(validateAnnotation(ref.value, 'reference').subject === entry.id, 'reference subject identity mismatch');
    return ref;
  });
  const run = fs.mkdtempSync(path.join(fs.realpathSync(outputRoot), 'pulsar-intake-'));
  fs.chmodSync(run, 0o700);
  try {
    const budget = { perFile: maxSourceBytes, remaining: maxTotalBytes };
    const entries = selected.map((entry, index) => {
      const source = copyBounded(files.get(entry.id), path.join(run, `${index}-source`), entry.source_sha256, budget, deadline);
      const referenceName = `${index}-reference.json`;
      fs.writeFileSync(path.join(run, referenceName), references[index].bytes, { flag: 'wx', mode: 0o400 });
      return { declaration: entry, source, reference: { path: referenceName, sha256: references[index].sha256, byte_len: references[index].bytes.length } };
    });
    requireValue(performance.now() <= deadline, 'intake deadline exceeded');
    const receipt = {
      schema_version: 1, kind: 'benchmark-input-snapshot', split,
      corpus_sha256: parsed.sha256, source_map_sha256: mapping.sha256,
      budgets: { max_source_bytes: maxSourceBytes, max_total_source_bytes: maxTotalBytes, timeout_ms: timeoutMs },
      source_bytes: maxTotalBytes - budget.remaining, entries,
      execution: null, release_qualified: false,
    };
    fs.writeFileSync(path.join(run, 'intake.json'), JSON.stringify(receipt, null, 2) + '\n', { flag: 'wx', mode: 0o400 });
    return { directory: run, receipt };
  } catch (error) {
    // Only this invocation's freshly allocated private directory is removed.
    fs.rmSync(run, { recursive: true, force: true });
    throw error;
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  try {
    const args = process.argv.slice(2), options = {};
    const names = new Map([['--corpus', 'corpus'], ['--sources', 'sources'], ['--output-root', 'outputRoot'], ['--split', 'split'], ['--max-source-bytes', 'maxSourceBytes'], ['--max-total-bytes', 'maxTotalBytes'], ['--timeout-ms', 'timeoutMs']]);
    requireValue(args.length % 2 === 0, 'expected option/value pairs');
    for (let i = 0; i < args.length; i += 2) {
      const key = names.get(args[i]);
      requireValue(key && !Object.hasOwn(options, key), 'unknown or duplicate option');
      options[key] = ['maxSourceBytes', 'maxTotalBytes', 'timeoutMs'].includes(key) ? Number(args[i + 1]) : args[i + 1];
    }
    console.log(JSON.stringify(stageBenchmarkInputs(options)));
  } catch (error) { console.error(error.message); process.exitCode = 1; }
}
