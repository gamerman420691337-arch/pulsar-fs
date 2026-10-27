// Explicit candidate inputs. Integrity/recovery wiring; no edition/model qualification.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import crypto from 'node:crypto';
import {spawn,spawnSync} from 'node:child_process';
import {once} from 'node:events';
const binary=process.env.PULSAR_TEST_INSTALLER;
const source=process.env.PULSAR_TEST_INSTALL_SOURCE;
const enabled=binary&&source;
const hash=f=>{const fd=fs.openSync(f,'r');try{const h=crypto.createHash('sha256'),b=Buffer.alloc(65536);for(;;){const n=fs.readSync(fd,b,0,b.length,null);if(!n)break;h.update(b.subarray(0,n));}return h.digest('hex');}finally{fs.closeSync(fd);}};
test('actual first-party installer survives interruption, publishes exact bytes under restrictive umask and preserves existing destination',{skip:!enabled,timeout:120000},async t=>{
 const root=fs.mkdtempSync(path.join(os.tmpdir(),'runtime-install-'));fs.chmodSync(root,0o700);
 const destination=path.join(root,'installed package');const env={PATH:'/usr/bin:/bin'};
 let child,stopped;
 try {
  const start=Date.now();child=spawn(binary,['runtime-install',source,destination],{env,stdio:'ignore'});stopped=once(child,'exit');
  let stage;
  while(true){
   assert.equal(child.exitCode,null,'installer completed before interruption observation');assert.ok(Date.now()-start<10000,'stage observation deadline');
   const candidates=fs.readdirSync(root).filter(n=>n.startsWith('.pulsar-runtime-stage-'));
   if(candidates.length){const p=path.join(root,candidates[0]);const first=path.join(p,'pulsar');if(fs.existsSync(first)&&fs.statSync(first).size>0){stage=p;break;}}
   await new Promise(r=>setTimeout(r,2));
  }
  const cli=args=>{const r=spawnSync(binary,args,{env,encoding:'utf8',timeout:30000,maxBuffer:1<<20});assert.ifError(r.error);return r;};
  const locked=cli(['runtime-stage-inspect',stage]);assert.equal(locked.status,0,locked.stderr);assert.equal(JSON.parse(locked.stdout).state,'locked');
  const manifestHash=hash(path.join(source,'runtime-candidate.json'));
  const busy=cli(['runtime-stage-discard',stage,'--expected-manifest-sha256',manifestHash,'--approve-discard']);assert.notEqual(busy.status,0);assert.equal(fs.existsSync(stage),true);
  assert.equal(fs.existsSync(destination),false);assert.equal(child.kill('SIGKILL'),true);const [code,signal]=await stopped;assert.equal(code,null);assert.equal(signal,'SIGKILL');child=undefined;
  assert.equal(fs.existsSync(destination),false);assert.equal(fs.existsSync(stage),true);assert.equal(fs.existsSync(path.join(stage,'runtime-candidate.json')),false);
  const inspected=cli(['runtime-stage-inspect',stage]);assert.equal(inspected.status,0,inspected.stderr);const observation=JSON.parse(inspected.stdout);assert.equal(observation.state,'discardable');assert.equal(observation.manifest_sha256,manifestHash);assert.ok(observation.present_files>0);
  const partialHash=hash(path.join(stage,'pulsar'));
  const stale=cli(['runtime-stage-discard',stage,'--expected-manifest-sha256','0'.repeat(64),'--approve-discard']);assert.notEqual(stale.status,0);assert.equal(fs.existsSync(stage),true);
  const unknown=path.join(stage,'owner-file');fs.writeFileSync(unknown,'keep');const unsafe=cli(['runtime-stage-discard',stage,'--expected-manifest-sha256',manifestHash,'--approve-discard']);assert.notEqual(unsafe.status,0);assert.equal(fs.readFileSync(unknown,'utf8'),'keep');assert.ok(fs.existsSync(path.join(stage,'pulsar')));assert.equal(hash(path.join(stage,'pulsar')),partialHash);fs.unlinkSync(unknown);
  const discarded=cli(['runtime-stage-discard',stage,'--expected-manifest-sha256',manifestHash,'--approve-discard']);assert.equal(discarded.status,0,discarded.stderr);assert.equal(fs.existsSync(stage),false);
  t.diagnostic(JSON.stringify({active_stage:'locked; discard refused',abandoned_stage:observation,stale_identity:'refused',unknown_entry:'refused before deleting known payload',explicit_discard:'PASS'}));

  const result=spawnSync('/usr/bin/bash',['-c','umask 0777; exec "$@"','installer-test',binary,'runtime-install',source,destination],{env,encoding:'utf8',timeout:60000,maxBuffer:1<<20});assert.ifError(result.error);assert.equal(result.status,0,result.stderr);t.diagnostic(JSON.stringify({status:result.status,stdout:result.stdout,stderr:result.stderr}));
  assert.equal(fs.existsSync(stage),false,'explicitly discarded stage must stay absent');
  const manifest=JSON.parse(fs.readFileSync(path.join(source,'runtime-candidate.json')));
  assert.equal(hash(path.join(source,'runtime-candidate.json')),hash(path.join(destination,'runtime-candidate.json')));
  for(const record of manifest.files){const file=path.join(destination,record.path);assert.equal(hash(file),record.sha256);assert.equal(fs.statSync(file).size,record.byte_len);const executable=['pulsar','pulsar-worker','runtimes/ffmpeg','runtimes/ffprobe','assistant/llama-completion'].includes(record.path);assert.equal(fs.statSync(file).mode&0o777,executable?0o755:0o644);}
  const owner=path.join(destination,'owner-file');fs.writeFileSync(owner,'preserve');const refused=spawnSync(binary,['runtime-install',source,destination],{env,encoding:'utf8',timeout:30000});assert.ifError(refused.error);assert.notEqual(refused.status,0);assert.equal(fs.readFileSync(owner,'utf8'),'preserve');assert.equal(hash(path.join(source,'runtime-candidate.json')),hash(path.join(destination,'runtime-candidate.json')));
  assert.equal(fs.existsSync(path.join(destination,'assistant-policy.json')),false);
  t.diagnostic(JSON.stringify({interrupted_signal:signal,unpublished_stage_inspected_and_explicitly_discarded:true,retry:'PASS',files:manifest.files.length,hashes:'PASS',existing_destination:'preserved',umask:'0777; output modes verified',engine_started:false,source_manifest_sha256:hash(path.join(source,'runtime-candidate.json'))}));
 } finally {
  if(child){if(child.exitCode===null&&child.signalCode===null)child.kill('SIGKILL');await stopped;}
  fs.rmSync(root,{recursive:true,force:true});
 }
});
