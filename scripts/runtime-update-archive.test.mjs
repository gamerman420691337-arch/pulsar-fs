// Actual offline changed-payload delivery; no edition or release qualification.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import crypto from 'node:crypto';
import {spawnSync} from 'node:child_process';
const binary=process.env.PULSAR_TEST_INSTALLER,base=process.env.PULSAR_TEST_INSTALL_SOURCE,worker=process.env.PULSAR_TEST_WORKER;
const retained=process.env.PULSAR_TEST_UPDATE_ARCHIVE;
const hash=file=>{const fd=fs.openSync(file,'r');try{const h=crypto.createHash('sha256'),b=Buffer.alloc(65536);for(;;){const n=fs.readSync(fd,b,0,b.length,null);if(!n)break;h.update(b.subarray(0,n));}return h.digest('hex');}finally{fs.closeSync(fd);}};
test('actual component archive verifies changed and reused payloads, preserves base and refuses damaged or unbound delivery',{skip:!(binary&&base&&worker),timeout:180000},t=>{
 const root=fs.mkdtempSync(path.join(os.tmpdir(),'runtime-update-archive-'));fs.chmodSync(root,0o700);
 const archive=retained??path.join(root,'changed.pulsar-update'),desired=path.join(root,'desired'),installed=path.join(root,'installed');
 const cli=args=>{const r=spawnSync(binary,args,{env:{PATH:'/usr/bin:/bin'},encoding:'utf8',timeout:60000,maxBuffer:1<<20});assert.ifError(r.error);t.diagnostic(JSON.stringify({args,status:r.status,stdout:r.stdout,stderr:r.stderr}));return r;};
 try {
  const rawBase=fs.readFileSync(path.join(base,'runtime-candidate.json')),baseHash=crypto.createHash('sha256').update(rawBase).digest('hex'),manifest=JSON.parse(rawBase);
  const preserved=new Map(manifest.files.map(r=>[r.path,hash(path.join(base,r.path))]));
  assert.equal(cli(['runtime-install',base,desired]).status,0);
  for(const [name,source] of [['pulsar',binary],['pulsar-worker',worker]]){fs.copyFileSync(source,path.join(desired,name));const record=manifest.files.find(r=>r.path===name);record.byte_len=fs.statSync(source).size;record.sha256=hash(source);}
  const desiredBytes=Buffer.from(JSON.stringify(manifest,null,2)+'\n');fs.writeFileSync(path.join(desired,'runtime-candidate.json'),desiredBytes);
  const pin=['--expected-base-manifest-sha256',baseHash];
  const packed=cli(['runtime-update-pack',desired,base,archive,...pin]);assert.equal(packed.status,0);const report=JSON.parse(packed.stdout);assert.equal(report.provided_files,2);assert.equal(report.reused_files,manifest.files.length-2);assert.equal(report.qualified,false);
  const header=Buffer.alloc(16),fd=fs.openSync(archive,'r');fs.readSync(fd,header,0,16,0);const length=header.readUInt32LE(12),encoded=Buffer.alloc(length);fs.readSync(fd,encoded,0,length,16);fs.closeSync(fd);
  assert.equal(header.subarray(0,12).toString(),'PULSARUP0001');const descriptor=JSON.parse(encoded);assert.equal(descriptor.base_manifest_sha256,baseHash);assert.deepEqual(descriptor.provided_paths,manifest.files.filter(r=>['pulsar','pulsar-worker'].includes(r.path)).map(r=>r.path));assert.deepEqual(Buffer.from(descriptor.desired_manifest),desiredBytes);assert.equal(fs.statSync(archive).size,16+length+report.provided_bytes);assert.equal(fs.statSync(archive).mode&0o777,0o600);
  const updated=cli(['runtime-update',archive,base,installed,...pin]);assert.equal(updated.status,0);assert.deepEqual(JSON.parse(updated.stdout),report);
  assert.deepEqual(fs.readFileSync(path.join(installed,'runtime-candidate.json')),desiredBytes);
  for(const r of manifest.files){const file=path.join(installed,r.path);assert.equal(fs.statSync(file).size,r.byte_len);assert.equal(hash(file),r.sha256);const a=fs.statSync(file),b=fs.statSync(path.join(base,r.path));assert.ok(a.dev!==b.dev||a.ino!==b.ino);}
  const archiveHash=hash(archive);assert.notEqual(cli(['runtime-update-pack',desired,base,archive,...pin]).status,0);assert.equal(hash(archive),archiveHash);assert.notEqual(cli(['runtime-update',archive,base,installed,...pin]).status,0);
  const wrong=path.join(root,'wrong');assert.notEqual(cli(['runtime-update',archive,base,wrong,'--expected-base-manifest-sha256','0'.repeat(64)]).status,0);assert.ok(!fs.existsSync(wrong));
  assert.notEqual(cli(['runtime-install',archive,path.join(root,'unbound')]).status,0);assert.ok(!fs.existsSync(path.join(root,'unbound')));
  const corrupt=path.join(root,'damaged');fs.copyFileSync(archive,corrupt);const edit=fs.openSync(corrupt,'r+'),last=fs.statSync(corrupt).size-1,byte=Buffer.alloc(1);fs.readSync(edit,byte,0,1,last);byte[0]^=1;fs.writeSync(edit,byte,0,1,last);fs.closeSync(edit);
  const refused=path.join(root,'refused');assert.notEqual(cli(['runtime-update',corrupt,base,refused,...pin]).status,0);assert.ok(!fs.existsSync(refused));fs.unlinkSync(corrupt);
  fs.copyFileSync(archive,corrupt);fs.appendFileSync(corrupt,Buffer.from([0]));assert.notEqual(cli(['runtime-update',corrupt,base,refused,...pin]).status,0);assert.ok(!fs.existsSync(refused));fs.unlinkSync(corrupt);
  fs.symlinkSync(archive,corrupt);assert.notEqual(cli(['runtime-update',corrupt,base,refused,...pin]).status,0);assert.ok(!fs.existsSync(refused));fs.unlinkSync(corrupt);
  fs.copyFileSync(archive,corrupt);const wrongHeader=Buffer.from(encoded.toString().replace(baseHash,'0'.repeat(64)));assert.equal(wrongHeader.length,length);const headerEdit=fs.openSync(corrupt,'r+');fs.writeSync(headerEdit,wrongHeader,0,length,16);fs.closeSync(headerEdit);assert.notEqual(cli(['runtime-update',corrupt,base,refused,...pin]).status,0);assert.ok(!fs.existsSync(refused));fs.unlinkSync(corrupt);
  assert.equal(cli(['runtime-launch',installed,path.join(root,'cache'),'--','--version']).status,0);
  fs.rmSync(path.join(root,'cache'),{recursive:true,force:true});
  const noop=path.join(root,'unchanged.pulsar-update'),same=path.join(root,'unchanged');const noChange=cli(['runtime-update-pack',base,base,noop,...pin]);assert.equal(noChange.status,0);const noChangeReport=JSON.parse(noChange.stdout);assert.equal(noChangeReport.provided_files,0);assert.equal(noChangeReport.provided_bytes,0);assert.equal(noChangeReport.reused_files,manifest.files.length);assert.equal(cli(['runtime-update',noop,base,same,...pin]).status,0);assert.deepEqual(fs.readFileSync(path.join(same,'runtime-candidate.json')),rawBase);

  for(const [name,before] of preserved)assert.equal(hash(path.join(base,name)),before);
  assert.deepEqual(fs.readFileSync(path.join(base,'runtime-candidate.json')),rawBase);
  t.diagnostic(JSON.stringify({qualified:false,archive,archive_sha256:archiveHash,archive_bytes:fs.statSync(archive).size,report,all_payloads_verified:manifest.files.length,independent_copies:true,base_preserved:true,explicit_version_launch:'PASS',damaged_trailing_symlink_wrong_base_embedded_pin_unbound_overwrite:'refused',zero_changed_payload_update:'PASS'}));
 }finally{fs.rmSync(root,{recursive:true,force:true});}
});
