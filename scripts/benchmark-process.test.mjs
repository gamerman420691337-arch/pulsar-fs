import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import { snapshotRunner, runAttempt } from './benchmark-process.mjs';

function fixture(t, body) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'pulsar-supervision-test-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const source = path.join(root, 'source');
  const bytes = Buffer.from(`#!/bin/sh\n${body}\n`);
  fs.writeFileSync(source, bytes, { mode: 0o700 });
  const snapshot = snapshotRunner(source, crypto.createHash('sha256').update(bytes).digest('hex'));
  t.after(snapshot.dispose);
  return { root, source, snapshot,
    options: { timeout: 1000, environment: { PATH: '/usr/bin:/bin' }, logPrefix: path.join(root, 'raw') } };
}

async function assertRetired(pid) {
  for (let count = 0; count < 30; count++) {
    try {
      const stat = fs.readFileSync(`/proc/${pid}/stat`, 'utf8');
      if (/\) [ZX] /.test(stat)) return; // Exited, awaiting the host's reaper.
    } catch (error) { if (error.code === 'ENOENT' || error.code === 'ESRCH') return; throw error; }
    await new Promise(resolve => setTimeout(resolve, 10));
  }
  assert.fail(`campaign descendant ${pid} still runs`);
}

test('supported native runner capture avoids rewriting executable bytes', { skip: process.platform !== 'linux' }, t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'pulsar-reflink-test-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const source = path.join(root, 'source'), bytes = Buffer.from('#!/bin/sh\necho captured\n');
  fs.writeFileSync(source, bytes, { mode: 0o700 });
  try {
    fs.copyFileSync(source, path.join(root, 'probe'), fs.constants.COPYFILE_EXCL | fs.constants.COPYFILE_FICLONE_FORCE);
  } catch (error) {
    if (['ENOTSUP', 'EOPNOTSUPP', 'EINVAL', 'EXDEV', 'ENOSYS'].includes(error.code)) return t.skip('filesystem lacks native reflink support');
    throw error;
  }
  t.mock.method(fs, 'writeSync', () => { throw new Error('runner capture rewrote executable bytes'); });
  const snapshot = snapshotRunner(source, crypto.createHash('sha256').update(bytes).digest('hex'));
  t.after(snapshot.dispose);
  assert.deepEqual(fs.readFileSync(snapshot.path), bytes);
  assert.equal(fs.statSync(snapshot.path).mode & 0o777, 0o500);
});

test('native capture verifies captured bytes after source replacement and mutation', { skip: process.platform !== 'linux' }, t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'pulsar-reflink-binding-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const source = path.join(root, 'source'), bytes = Buffer.from('#!/bin/sh\necho original\n');
  fs.writeFileSync(source, bytes, { mode: 0o700 });
  const copy = fs.copyFileSync;
  try {
    copy(source, path.join(root, 'probe'), fs.constants.COPYFILE_EXCL | fs.constants.COPYFILE_FICLONE_FORCE);
  } catch (error) {
    if (['ENOTSUP', 'EOPNOTSUPP', 'EINVAL', 'EXDEV', 'ENOSYS'].includes(error.code)) return t.skip('filesystem lacks native reflink support');
    throw error;
  }
  const replacement = '#!/bin/sh\necho replacement\n';
  t.mock.method(fs, 'copyFileSync', (heldSource, destination, flags) => {
    fs.renameSync(source, path.join(root, 'held-source'));
    fs.writeFileSync(source, replacement);
    copy(heldSource, destination, flags);
    fs.writeFileSync(path.join(root, 'held-source'), '#!/bin/sh\necho changed-after-capture\n');
  });
  const snapshot = snapshotRunner(source, crypto.createHash('sha256').update(bytes).digest('hex'));
  t.after(snapshot.dispose);
  assert.equal(fs.readFileSync(source, 'utf8'), replacement);
  assert.deepEqual(fs.readFileSync(snapshot.path), bytes);
});

test('unsupported native capture uses verified copying', { skip: process.platform !== 'linux' }, t => {
  t.mock.method(fs, 'copyFileSync', () => { throw Object.assign(new Error('unsupported clone'), { code: 'ENOTSUP' }); });
  const f = fixture(t, 'echo fallback');
  assert.equal(fs.readFileSync(f.snapshot.path, 'utf8'), '#!/bin/sh\necho fallback\n');
  assert.throws(() => snapshotRunner(f.source, '0'.repeat(64)), /identity mismatch/);
});

test('native capture IO errors refuse without a second copy attempt', { skip: process.platform !== 'linux' }, t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'pulsar-reflink-refusal-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const source = path.join(root, 'source'), bytes = Buffer.from('#!/bin/sh\nexit 0\n');
  fs.writeFileSync(source, bytes);
  t.mock.method(fs, 'copyFileSync', () => { throw Object.assign(new Error('capture IO failure'), { code: 'EIO' }); });
  t.mock.method(fs, 'writeSync', () => { assert.fail('refused native capture must not retry through copying'); });
  assert.throws(() => snapshotRunner(source, crypto.createHash('sha256').update(bytes).digest('hex')), { code: 'EIO' });
});

test('verified snapshot executes original bytes after caller path changes', async t => {
  const f = fixture(t, 'echo original');
  fs.writeFileSync(f.source, '#!/bin/sh\necho replaced\n');
  const result = await runAttempt(f.snapshot.path, [], f.options);
  assert.equal(result.status, 0);
  assert.equal(result.error, null);
  assert.equal(result.stdout, 'original\n');
  assert.equal(fs.readFileSync(`${f.options.logPrefix}.stdout`, 'utf8'), result.stdout);
  assert.throws(() => snapshotRunner(f.source, '0'.repeat(64)), /identity mismatch/);
});

for (const mode of ['timeout', 'cancel', 'exit']) {
  test(`${mode} retires descendants retaining output pipes`, async t => {
    const f = fixture(t, `sleep 60 &\necho $!\n${mode === 'exit' ? 'exit 0' : 'wait'}`);
    const controller = new AbortController();
    const timer = mode === 'cancel' ? setTimeout(() => controller.abort(), 100) : null;
    t.after(() => clearTimeout(timer));
    const result = await runAttempt(f.snapshot.path, [], { ...f.options, timeout: 300, signal: controller.signal });
    if (mode === 'exit') assert.equal(result.status, 0);
    else assert.match(result.error, mode === 'cancel' ? /cancelled/ : /deadline/);
    const pid = Number(result.stdout.trim());
    assert.ok(Number.isSafeInteger(pid) && pid > 1);
    await assertRetired(pid);
  });
}

test('unbounded runner output is stopped and raw retention remains bounded', async t => {
  const f = fixture(t, 'yes output');
  const result = await runAttempt(f.snapshot.path, [], f.options);
  assert.match(result.error, /output exceeded/);
  assert.equal(fs.statSync(`${f.options.logPrefix}.stdout`).size, 1024 * 1024);
});

test('pre-cancelled execution never launches', async t => {
  const f = fixture(t, 'touch "$1"');
  const marker = path.join(f.root, 'launched');
  const result = await runAttempt(f.snapshot.path, [marker], { ...f.options, signal: AbortSignal.abort() });
  assert.match(result.error, /cancelled/);
  assert.equal(fs.existsSync(marker), false);
});
