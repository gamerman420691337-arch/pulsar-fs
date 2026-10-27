// Actual first-party pack/extract commands. Authored package wiring, no release qualification.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import crypto from 'node:crypto';
import {spawnSync} from 'node:child_process';
const binary=process.env.PULSAR_TEST_INSTALLER,source=process.env.PULSAR_TEST_INSTALL_SOURCE;
const hash=file=>{const fd=fs.openSync(file,'r');try{const h=crypto.createHash('sha256'),b=Buffer.alloc(65536);for(;;){const n=fs.readSync(fd,b,0,b.length,null);if(!n)break;h.update(b.subarray(0,n));}return h.digest('hex');}finally{fs.closeSync(fd);}};
test('single-file offline runtime creation and extraction preserve identities and refuse damage or overwrite',{skip:!(binary&&source),timeout:120000},t=>{
 const root=fs.mkdtempSync(path.join(os.tmpdir(),'runtime-archive-'));fs.chmodSync(root,0o700);
 const env={PATH:'/usr/bin:/bin'};const archive=path.join(root,'runtime.pulsar-runtime'),installed=path.join(root,'installed package');
 const cli=args=>{const r=spawnSync(binary,args,{env,encoding:'utf8',timeout:60000,maxBuffer:1<<20});assert.ifError(r.error);t.diagnostic(JSON.stringify({args,status:r.status,stdout:r.stdout,stderr:r.stderr}));return r;};
 try{
  const packed=spawnSync('/usr/bin/bash',['-c','umask 0777; exec "$@"','archive-test',binary,'runtime-pack',source,archive],{env,encoding:'utf8',timeout:60000,maxBuffer:1<<20});assert.ifError(packed.error);assert.equal(packed.status,0,packed.stderr);assert.equal(fs.statSync(archive).mode&0o777,0o600);
  const manifest=JSON.parse(fs.readFileSync(path.join(source,'runtime-candidate.json')));const header=Buffer.alloc(16),fd=fs.openSync(archive,'r');fs.readSync(fd,header,0,16,0);fs.closeSync(fd);assert.equal(header.subarray(0,12).toString(),'PULSARRT0001');assert.equal(header.readUInt32LE(12),fs.statSync(path.join(source,'runtime-candidate.json')).size);assert.equal(fs.statSync(archive).size,16+header.readUInt32LE(12)+manifest.files.reduce((n,r)=>n+r.byte_len,0));
  const archiveSha=hash(archive);assert.notEqual(cli(['runtime-pack',source,archive]).status,0);assert.equal(hash(archive),archiveSha);
  const extracted=cli(['runtime-install',archive,installed]);assert.equal(extracted.status,0,extracted.stderr);assert.equal(hash(path.join(source,'runtime-candidate.json')),hash(path.join(installed,'runtime-candidate.json')));
  for(const r of manifest.files){assert.equal(hash(path.join(installed,r.path)),r.sha256);assert.equal(fs.statSync(path.join(installed,r.path)).size,r.byte_len);}
  const marker=JSON.parse(fs.readFileSync(path.join(installed,'.pulsar-runtime-stage.json')));assert.equal(marker.manifest_sha256,hash(path.join(installed,'runtime-candidate.json')));assert.equal(marker.manifest_json,fs.readFileSync(path.join(source,'runtime-candidate.json'),'utf8'));
  fs.writeFileSync(path.join(installed,'owner-file'),'preserve');assert.notEqual(cli(['runtime-install',archive,installed]).status,0);assert.equal(fs.readFileSync(path.join(installed,'owner-file'),'utf8'),'preserve');
  const corrupt=path.join(root,'corrupt.pulsar-runtime');fs.copyFileSync(archive,corrupt);const edit=fs.openSync(corrupt,'r+');const last=Buffer.alloc(1),end=fs.statSync(corrupt).size-1;fs.readSync(edit,last,0,1,end);last[0]^=1;fs.writeSync(edit,last,0,1,end);fs.closeSync(edit);const refused=path.join(root,'damaged-install');assert.notEqual(cli(['runtime-install',corrupt,refused]).status,0);assert.equal(fs.existsSync(refused),false);assert.equal(fs.readdirSync(root).some(n=>n.startsWith('.pulsar-runtime-stage-')),false);
  t.diagnostic(JSON.stringify({archive_sha256:archiveSha,archive_bytes:fs.statSync(archive).size,manifest_sha256:hash(path.join(source,'runtime-candidate.json')),files:manifest.files.length,all_payloads:'PASS',original_manifest_receipt:'PASS',late_corruption:'refused; no published destination',overwrite:'refused; unchanged',archive_umask:'0777; private0600',engine_started:false}));
 }finally{fs.rmSync(root,{recursive:true,force:true});}
});
