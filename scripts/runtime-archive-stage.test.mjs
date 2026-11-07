// Actual SIGSTOP/SIGKILL of owned packers; no power-loss or release qualification.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import crypto from 'node:crypto';
import {spawn,spawnSync} from 'node:child_process';
const binary=process.env.PULSAR_TEST_INSTALLER,base=process.env.PULSAR_TEST_INSTALL_SOURCE,updated=process.env.PULSAR_TEST_UPDATE_SOURCE;
const prefix='.pulsar-runtime-archive-stage-';
const delay=ms=>new Promise(resolve=>setTimeout(resolve,ms));
const hash=file=>{const h=crypto.createHash('sha256'),b=Buffer.alloc(65536),fd=fs.openSync(file,'r');try{for(;;){const n=fs.readSync(fd,b,0,b.length,null);if(!n)break;h.update(b.subarray(0,n));}return h.digest('hex');}finally{fs.closeSync(fd);}};
test('full and changed-payload archive packers refuse live discard and recover after actual process death',{skip:!(binary&&base&&updated),timeout:180000},async t=>{
 const root=fs.mkdtempSync(path.join(os.tmpdir(),'archive-stage-'));fs.chmodSync(root,0o700);
 const cli=args=>{const r=spawnSync(binary,args,{env:{PATH:'/usr/bin:/bin'},encoding:'utf8',timeout:60000,maxBuffer:1<<20});assert.ifError(r.error);t.diagnostic(JSON.stringify({args,status:r.status,stdout:r.stdout,stderr:r.stderr}));return r;};
 const pin=hash(path.join(base,'runtime-candidate.json'));let child=null,closed=null;
 try {
  for(const mode of ['full','delta']) {
   const parent=path.join(root,mode);fs.mkdirSync(parent,{mode:0o700});const output=path.join(parent,'new archive');const args=mode==='full'?['runtime-pack',base,output]:['runtime-update-pack',updated,base,output,'--expected-base-manifest-sha256',pin];
   child=spawn(binary,args,{env:{PATH:'/usr/bin:/bin'},stdio:['ignore','pipe','pipe']});assert.ok(child.pid);let stdout='',stderr='';child.stdout.on('data',b=>{stdout+=b.toString();});child.stderr.on('data',b=>{stderr+=b.toString();});closed=new Promise((resolve,reject)=>{child.once('error',reject);child.once('close',(code,signal)=>resolve({code,signal}));});
   let stage=null;const deadline=Date.now()+30000;
   while(Date.now()<deadline) {
    assert.equal(child.exitCode,null,`packer exited before interruption: ${stdout} ${stderr}`);assert.equal(child.signalCode,null);
    const entries=fs.readdirSync(parent).filter(n=>n.startsWith(prefix));
    if(entries.length===1) {const candidate=path.join(parent,entries[0]);try{if(fs.statSync(path.join(candidate,'archive.part')).size>=1024*1024){stage=candidate;assert.ok(child.kill('SIGSTOP'));break;}}catch(e){if(e.code!=='ENOENT')throw e;}}
    await delay(2);
   }
   assert.ok(stage,'owned packer did not create an observable partial stage');
   let stopped=false;for(let n=0;n<100;n++){const stat=fs.readFileSync(`/proc/${child.pid}/stat`,'utf8');if(/\)\s+T\s/.test(stat)){stopped=true;break;}await delay(2);}assert.ok(stopped,'owned packer not confirmed stopped');
   const locked=cli(['runtime-archive-stage-inspect',stage]);assert.equal(locked.status,0);assert.equal(JSON.parse(locked.stdout).state,'locked');assert.notEqual(cli(['runtime-archive-stage-discard',stage,'--expected-stage-sha256','0'.repeat(64),'--approve-discard']).status,0);
   assert.ok(child.kill('SIGKILL'));const killed=await closed;assert.equal(killed.signal,'SIGKILL');child=null;closed=null;assert.ok(!fs.existsSync(output));
   const report=cli(['runtime-archive-stage-inspect',stage]);assert.equal(report.status,0);const receipt=JSON.parse(report.stdout);assert.equal(receipt.state,'discardable');assert.equal(receipt.qualified,false);assert.ok(receipt.present_bytes>=1024*1024&&receipt.present_bytes<=receipt.expected_bytes);
   const part=path.join(stage,'archive.part'),before=hash(part);
   assert.notEqual(cli(['runtime-archive-stage-discard',stage,'--expected-stage-sha256','0'.repeat(64),'--approve-discard']).status,0);assert.equal(hash(part),before);
   fs.writeFileSync(path.join(stage,'owner-note'),'preserve');assert.notEqual(cli(['runtime-archive-stage-discard',stage,'--expected-stage-sha256',receipt.stage_sha256,'--approve-discard']).status,0);assert.equal(hash(part),before);assert.equal(fs.readFileSync(path.join(stage,'owner-note'),'utf8'),'preserve');fs.unlinkSync(path.join(stage,'owner-note'));
   const edit=fs.openSync(part,'r+'),byte=Buffer.alloc(1);fs.readSync(edit,byte,0,1,0);byte[0]^=1;fs.writeSync(edit,byte,0,1,0);fs.closeSync(edit);
   assert.notEqual(cli(['runtime-archive-stage-discard',stage,'--expected-stage-sha256',receipt.stage_sha256,'--approve-discard']).status,0);assert.ok(fs.existsSync(part));const fresh=JSON.parse(cli(['runtime-archive-stage-inspect',stage]).stdout);assert.notEqual(fresh.stage_sha256,receipt.stage_sha256);
   assert.equal(cli(['runtime-archive-stage-discard',stage,'--expected-stage-sha256',fresh.stage_sha256,'--approve-discard']).status,0);assert.ok(!fs.existsSync(stage));
   assert.equal(cli(args).status,0);assert.ok(fs.existsSync(output));assert.equal(fs.statSync(output).mode&0o777,0o600);assert.equal(fs.readdirSync(parent).filter(n=>n.startsWith(prefix)).length,0);
   t.diagnostic(JSON.stringify({mode,signal:'SIGKILL',active_lock:'refused discard',inactive_receipt:receipt,stale_wrong_identity_unknown_entry_changed_payload:'refused; payload preserved',exact_fresh_discard:'PASS',retry:'PASS',archive_sha256:hash(output),qualified:false}));
  }
 } finally {
  if(child&&child.exitCode===null&&child.signalCode===null){child.kill('SIGKILL');await closed;}
  fs.rmSync(root,{recursive:true,force:true});
 }
});
