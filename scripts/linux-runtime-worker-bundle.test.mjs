// Explicit actual build/media inputs; packaging checks are not edition qualification.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import crypto from 'node:crypto';
import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
const assembler = process.env.PULSAR_TEST_BUNDLE_ASSEMBLER
  ?? fileURLToPath(new URL('./assemble-linux-runtime-candidate.mjs', import.meta.url));
const binary = process.env.PULSAR_TEST_BUNDLE_BINARY;
const media = process.env.PULSAR_TEST_BUNDLE_MEDIA;
const enabled = binary && media;
function run(app, inputs, output) {
  return spawnSync(process.execPath, [assembler, app, inputs, output],
    {encoding:'utf8', timeout:60000, maxBuffer:1<<20});
}
function hash(file) {
  return crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
}
test('actual runtime assembly includes the exact dedicated worker and preserves existing output',
  {skip:!enabled}, () => {
    assert.ok(path.isAbsolute(binary) && path.isAbsolute(media));
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'worker-bundle-'));
    try {
      const output = path.join(root,'candidate');
      const result = run(binary, media, output);
      assert.ifError(result.error);
      assert.equal(result.status,0,result.stderr);
      const manifest = JSON.parse(fs.readFileSync(path.join(output,'runtime-candidate.json')));
      assert.equal(manifest.qualified,false);
      const worker = manifest.files.find(f=>f.path==='pulsar-worker');
      assert.ok(worker,'mandatory dedicated worker absent from manifest');
      const companion = path.join(path.dirname(fs.realpathSync(binary)),'pulsar-worker');
      assert.equal(worker.sha256,hash(companion));
      assert.equal(worker.byte_len,fs.statSync(companion).size);
      for (const record of manifest.files) {
        assert.equal(hash(path.join(output,record.path)),record.sha256);
        assert.equal(fs.statSync(path.join(output,record.path)).size,record.byte_len);
      }
      fs.accessSync(path.join(output,'pulsar-worker'),fs.constants.X_OK);
      const before = hash(path.join(output,'runtime-candidate.json'));
      assert.notEqual(run(binary,media,output).status,0);
      assert.equal(hash(path.join(output,'runtime-candidate.json')),before);
      console.log(JSON.stringify({worker,files:manifest.files.length,qualified:manifest.qualified}));
    } finally { fs.rmSync(root,{recursive:true,force:true}); }
  });
test('missing companion and dynamically dependent media refuse before output creation',
  {skip:!enabled}, () => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(),'worker-bundle-refusal-'));
    try {
      const app = path.join(root,'pulsar');
      fs.copyFileSync('/usr/bin/true',app);fs.chmodSync(app,0o755);
      let output = path.join(root,'missing-worker');
      assert.notEqual(run(app,media,output).status,0);
      assert.equal(fs.existsSync(output),false);
      fs.copyFileSync('/usr/bin/true',path.join(root,'pulsar-worker'));
      const inputs = path.join(root,'media');fs.mkdirSync(inputs);
      for (const name of ['ffprobe','COPYING.LGPLv2.1','LICENSE.md','ffmpeg-9.0.2.tar.xz','configure.log'])
        fs.symlinkSync(path.join(media,name),path.join(inputs,name));
      fs.symlinkSync('/usr/bin/true',path.join(inputs,'ffmpeg'));
      output = path.join(root,'dynamic-media');
      const result = run(app,inputs,output);
      assert.notEqual(result.status,0);
      assert.match(result.stderr,/dynamic runtime dependencies/);
      assert.equal(fs.existsSync(output),false);
    } finally {fs.rmSync(root,{recursive:true,force:true});}
  });
