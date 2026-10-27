// Host SDK experiment, not confined inference or production GPU admission.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
const [binaryArg, specArg, expectationArg, outputArg] = process.argv.slice(2);
assert.equal(process.argv.length, 6, 'BINARY LIBRARY-SPEC EXPECTATION NEW-OUTPUT');
const binary = fs.realpathSync(binaryArg);
const spec = JSON.parse(fs.readFileSync(specArg));
const expected = JSON.parse(fs.readFileSync(expectationArg));
const selected = spec.libraries.filter(l => l.names.includes('libcuda.so.1'));
assert.equal(selected.length, 1);
const library = selected[0];
assert(library.byte_len > 0 && library.byte_len <= 2 ** 31);
fs.mkdirSync(outputArg, { mode: 0o700 });
const output = fs.realpathSync(outputArg);
const capture = fs.mkdtempSync(path.join(output, 'driver-'));
const captured = path.join(capture, 'libcuda.so.1');
const binaryHash = () => crypto.createHash('sha256').update(fs.readFileSync(binary)).digest('hex');
const before = binaryHash();
let descriptor;
try {
  descriptor = fs.openSync(library.path, fs.constants.O_RDONLY | fs.constants.O_NOFOLLOW | fs.constants.O_NONBLOCK);
  const metadata = fs.fstatSync(descriptor);
  assert(metadata.isFile()); assert.equal(metadata.size, library.byte_len);
  // Bounded experiment copy: exactly the declared length plus growth detection.
  const bytes = Buffer.alloc(library.byte_len);
  let offset = 0;
  while (offset < bytes.length) {
    const count = fs.readSync(descriptor, bytes, offset, bytes.length - offset, null);
    assert(count > 0, 'driver shrank'); offset += count;
  }
  assert.equal(fs.readSync(descriptor, Buffer.alloc(1), 0, 1, null), 0, 'driver grew');
  assert.equal(crypto.createHash('sha256').update(bytes).digest('hex'), library.sha256);
  fs.writeFileSync(captured, bytes, { flag: 'wx', mode: 0o400 });
} finally { if (descriptor !== undefined) fs.closeSync(descriptor); }
const cases = [];
function run(name, expectation, driver, passes, pattern) {
  const input = path.join(output, `${name}.json`);
  fs.writeFileSync(input, JSON.stringify(expectation, null, 2) + '\n', { flag: 'wx' });
  const result = spawnSync(binary, [driver, input], { encoding: 'utf8', env: {}, timeout: 30_000, maxBuffer: 131072 });
  fs.writeFileSync(path.join(output, `${name}.stdout`), result.stdout ?? '');
  fs.writeFileSync(path.join(output, `${name}.stderr`), result.stderr ?? '');
  assert.ifError(result.error); assert.equal(result.signal, null);
  if (passes) {
    assert.equal(result.status, 0, result.stderr);
    const observation = JSON.parse(result.stdout);
    assert.deepEqual(observation.initial, observation.rechecked);
    assert.equal(observation.initial.device_uuid, expected.device_uuid.toLowerCase().replace('gpu-', 'GPU-'));
    assert.equal(observation.initial.pci_bus_address, expected.pci_bus_address);
    assert.equal(observation.initial.device_ordinal, expected.device_ordinal);
    assert(observation.initial.total_bytes > 0);
  } else {
    assert.notEqual(result.status, 0); assert.equal(result.stdout, '');
    assert.match(result.stderr, pattern);
  }
  cases.push({ name, exit_code: result.status, expected: passes ? 'success' : 'refusal' });
}
try {
  run('matching', expected, captured, true);
  run('domain-padding', { ...expected, pci_bus_address: '00000000' + expected.pci_bus_address.slice(4) }, captured, true);
  run('uuid', { ...expected, device_uuid: 'GPU-00000000-0000-0000-0000-000000000000' }, captured, false, /differs from requested device/);
  run('pci', { ...expected, pci_bus_address: '0000:ff:00.0' }, captured, false, /differs from requested device/);
  run('ordinal', { ...expected, device_ordinal: 63 }, captured, false, /outside the visible SDK inventory/);
  run('invalid-ordinal', { ...expected, device_ordinal: 64 }, '/does-not-exist', false, /identity declaration/);
  run('unknown-field', { ...expected, extra: 1 }, captured, false, /unknown field/);
  const symlink = path.join(capture, 'link.so'); fs.symlinkSync(captured, symlink);
  run('symlink', expected, symlink, false, /symbolic links|symlink|Too many levels|open regular/);
  run('relative-driver', expected, 'libcuda.so.1', false, /explicit absolute path/);
  assert.equal(binaryHash(), before, 'binary changed during experiment');
  assert.equal(crypto.createHash('sha256').update(fs.readFileSync(captured)).digest('hex'), library.sha256);
} finally { fs.rmSync(capture, { recursive: true }); }
assert(!fs.existsSync(capture));
fs.writeFileSync(path.join(output, 'checked.json'), JSON.stringify({
  scope: 'host SDK identity only; no confinement, CUDA context, inference, reservation or job authority',
  binary, binary_sha256: before, driver: library, cases, explicit_cleanup_verified: true
}, null, 2) + '\n', { flag: 'wx' });
