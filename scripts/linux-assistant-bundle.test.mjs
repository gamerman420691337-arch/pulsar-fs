// Explicit pinned local build inputs; this does not qualify Core/Complete.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import crypto from 'node:crypto';
import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
const assembler=fileURLToPath(new URL('./assemble-linux-runtime-candidate.mjs',import.meta.url));
const binary=process.env.PULSAR_TEST_BUNDLE_BINARY;
const media=process.env.PULSAR_TEST_BUNDLE_MEDIA;
const declaration=process.env.PULSAR_TEST_ASSISTANT_DECLARATION;
const enabled=binary&&media&&declaration;
const digest=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const run=(output,spec)=>spawnSync(process.execPath,[assembler,binary,media,output,spec],
  {encoding:'utf8',timeout:60000,maxBuffer:1<<20});
test('pinned assistant copies all identities and supports relocated offline host setup', {skip:!enabled},()=>{
 const root=fs.mkdtempSync(path.join(os.tmpdir(),'assistant-bundle-'));
 try {
  const original=path.join(root,'original');const result=run(original,declaration);
  assert.ifError(result.error);assert.equal(result.status,0,result.stderr);
  const spec=JSON.parse(fs.readFileSync(declaration));
  const manifest=JSON.parse(fs.readFileSync(path.join(original,'runtime-candidate.json')));
  assert.equal(manifest.qualified,false);assert.equal(manifest.files.length,12);
  for(const record of manifest.files){assert.equal(digest(path.join(original,record.path)),record.sha256);
    assert.equal(fs.statSync(path.join(original,record.path)).size,record.byte_len);}
  assert.equal(digest(path.join(original,manifest.assistant.runtime_path)),spec.runtime.sha256);
  assert.equal(digest(path.join(original,manifest.assistant.model_path)),spec.model.sha256);
  assert.equal(manifest.assistant.host_approval_required,true);
  assert.equal(fs.existsSync(path.join(original,'assistant-policy.json')),false);
  const moved=path.join(root,'moved installation');fs.renameSync(original,moved);
  const env={PATH:'/usr/bin:/bin',PULSAR_STATE_DIR:path.join(root,'engine-state')};
  const setup=spawnSync(path.join(moved,'pulsar'),['assistant-setup','--output',path.join(moved,'assistant-policy.json'),
    '--runtime',path.join(moved,manifest.assistant.runtime_path),'--model',path.join(moved,manifest.assistant.model_path),
    '--approve-host-inference'],{env,encoding:'utf8',timeout:30000,maxBuffer:1<<20});
  assert.ifError(setup.error);assert.equal(setup.status,0,setup.stderr);
  const policy=JSON.parse(fs.readFileSync(path.join(moved,'assistant-policy.json')));
  assert.equal(policy.schema_version,2);assert.equal(policy.runtime_path,manifest.assistant.runtime_path);
  assert.equal(policy.model_path,manifest.assistant.model_path);assert.equal(policy.runtime.sha256,spec.runtime.sha256);
  assert.equal(policy.model.sha256,spec.model.sha256);assert.equal(policy.worker_sha256,digest(path.join(moved,'pulsar-worker')));
  assert.equal(fs.existsSync(env.PULSAR_STATE_DIR),false);
  console.log(JSON.stringify({manifest,relocated_setup:'PASS; no Engine/inference',policy_mode:(fs.statSync(path.join(moved,'assistant-policy.json')).mode&0o777).toString(8)}));
 } finally{fs.rmSync(root,{recursive:true,force:true});}
});
test('changed pin and dynamic assistant refuse before candidate creation',{skip:!enabled},()=>{
 const root=fs.mkdtempSync(path.join(os.tmpdir(),'assistant-bundle-refusal-'));
 try {
  const spec=JSON.parse(fs.readFileSync(declaration));const bad=path.join(root,'bad.json');
  fs.writeFileSync(bad,' '.repeat(16385));const oversized=path.join(root,'oversized');
  assert.notEqual(run(oversized,bad).status,0);assert.equal(fs.existsSync(oversized),false);
  spec.model.sha256='0'.repeat(64);fs.writeFileSync(bad,JSON.stringify(spec));
  let output=path.join(root,'wrong-pin');assert.notEqual(run(output,bad).status,0);assert.equal(fs.existsSync(output),false);
  spec.model=JSON.parse(fs.readFileSync(declaration)).model;
  spec.runtime={path:'/usr/bin/true',byte_len:fs.statSync('/usr/bin/true').size,sha256:digest('/usr/bin/true')};
  fs.writeFileSync(bad,JSON.stringify(spec));output=path.join(root,'dynamic');
  const refused=run(output,bad);assert.notEqual(refused.status,0);assert.match(refused.stderr,/unpackaged dynamic dependencies/);
  assert.equal(fs.existsSync(output),false);
 } finally{fs.rmSync(root,{recursive:true,force:true});}
});
