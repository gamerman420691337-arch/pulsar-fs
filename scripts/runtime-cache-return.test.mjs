// Actual retained-runtime selection. No learned quality or database rollback claim.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import crypto from 'node:crypto';
import {spawnSync} from 'node:child_process';

const binary=process.env.PULSAR_TEST_INSTALLER;
const image=process.env.PULSAR_TEST_APPIMAGE;
const sources=[process.env.PULSAR_TEST_OLD_RUNTIME_SOURCE,process.env.PULSAR_TEST_INSTALL_SOURCE];
const hash=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const engines=cache=>fs.readdirSync('/proc').filter(p=>/^\d+$/.test(p)).flatMap(pid=>{
 try {
  const exe=fs.readlinkSync(`/proc/${pid}/exe`),argv=fs.readFileSync(`/proc/${pid}/cmdline`).toString().split('\0');
  return exe.startsWith(`${cache}/`)&&exe.endsWith('/pulsar')&&argv.includes('engine')?[Number(pid)]:[];
 } catch(e) {if(!['ENOENT','EACCES','EPERM'].includes(e.code))throw e;return [];}
});
const pause=()=>Atomics.wait(new Int32Array(new SharedArrayBuffer(4)),0,0,25);
test('explicit retained selection survives damaged update and unavailable suppliers',{
 skip:!(binary&&sources.every(Boolean)),timeout:120000,
},t=>{
 const root=fs.mkdtempSync(path.join(os.tmpdir(),'runtime-return-'));fs.chmodSync(root,0o700);
 const cache=path.join(root,'cache'),state=path.join(root,'state');
 const env={PATH:'/usr/bin:/bin',PULSAR_STATE_DIR:state};
 const run=args=>spawnSync(binary,args,{env,encoding:'utf8',timeout:30000,maxBuffer:1<<20});
 const success=r=>{assert.ifError(r.error);assert.equal(r.status,0,r.stderr);};
 let ownedEngine;
 try {
  const ids=sources.map(s=>hash(path.join(s,'runtime-candidate.json')));
  assert.notEqual(ids[0],ids[1],'campaign requires distinct actual runtime subjects');
  for(const source of sources)success(run(['runtime-launch',source,cache,'--','--version']));
  const old=path.join(cache,ids[0]),updated=path.join(cache,ids[1]);
  const before=fs.statSync(path.join(old,'pulsar'),{bigint:true});
  for(const id of ids) {
   const inspected=run(['runtime-cache-inspect',cache,id]);success(inspected);assert.match(inspected.stdout,/Verified retained runtime/);
   success(run(['runtime-cache-launch',cache,id,'--','--version']));
  }
  if(image) {
   const transient=path.join(root,'transient');fs.mkdirSync(transient,{mode:0o700});
   for(const args of [
    ['runtime-cache-inspect',cache,ids[0]],
    ['runtime-cache-launch',cache,ids[0],'--','--version'],
   ]) {
    success(spawnSync(image,args,{env:{...env,APPIMAGE_EXTRACT_AND_RUN:'1',
     PULSAR_RUNTIME_CACHE_ROOT:cache,TMPDIR:transient},encoding:'utf8',timeout:30000,maxBuffer:1<<20}));
    assert.deepEqual(fs.readdirSync(transient),[],'packaged retained selection must clean extraction');
   }
  }
  // Hide both supplier directories inside a nested owned mount namespace.
  // The retained cache is the only runtime source still visible to selection.
  const selector=path.join(root,'selector');fs.copyFileSync(binary,selector);fs.chmodSync(selector,0o700);
  const hidden=spawnSync('/usr/bin/bwrap',['--die-with-parent','--unshare-net',
   '--ro-bind','/','/','--bind',root,root,'--dev-bind','/dev','/dev','--proc','/proc',
   ...sources.flatMap(source=>['--tmpfs',source]),selector,'runtime-cache-launch',cache,ids[0],'--','--version'],
   {env,encoding:'utf8',timeout:30000,maxBuffer:1<<20});
  success(hidden);assert.match(hidden.stdout,/0\.8\.0/);
  assert.equal(fs.existsSync(state),false,'inspection and version launch must not start Engine');
  success(run(['runtime-cache-launch',cache,ids[1],'--','model','status']));
  const running=engines(cache);assert.equal(running.length,1);ownedEngine=running[0];
  assert.equal(fs.readlinkSync(`/proc/${ownedEngine}/exe`),path.join(updated,'pulsar'));
  success(run(['runtime-cache-launch',cache,ids[0],'--','model','status']));
  assert.deepEqual(engines(cache),[ownedEngine],'selecting an older client must preserve the existing Engine');
  const damaged=path.join(updated,'pulsar-worker');fs.writeFileSync(damaged,'owned damaged update');
  const refused=run(['runtime-cache-launch',cache,ids[1],'--','--version']);assert.ifError(refused.error);assert.equal(refused.status,1);
  assert.equal(fs.readFileSync(damaged,'utf8'),'owned damaged update');
  success(run(['runtime-cache-launch',cache,ids[0],'--','--version']));
  const after=fs.statSync(path.join(old,'pulsar'),{bigint:true});assert.equal(after.ino,before.ino);assert.equal(after.mtimeNs,before.mtimeNs);
  fs.writeFileSync(path.join(old,'owner-file'),'preserve');
  const unknown=run(['runtime-cache-inspect',cache,ids[0]]);assert.ifError(unknown.error);assert.equal(unknown.status,1);
  assert.equal(fs.readFileSync(path.join(old,'owner-file'),'utf8'),'preserve');
  const missing=path.join(root,'missing/cache');
  for(const id of ['../escape','A'.repeat(64),'a'.repeat(64)]) {
   const invalid=run(['runtime-cache-inspect',missing,id]);assert.ifError(invalid.error);assert.equal(invalid.status,1);
   assert.equal(fs.existsSync(path.join(root,'missing')),false);
  }
  t.diagnostic(JSON.stringify({old_manifest:ids[0],updated_manifest:ids[1],suppliers:'hidden; retained launch PASS',damaged_update:'refused and preserved',explicit_return:'PASS; inode and mtime unchanged',unknown:'refused and preserved',missing:'no creation',engine_pid:ownedEngine,existing_engine:'same PID and executable; authenticated older client status PASS',packaged_selection:image?'PASS; automatic extraction cleanup':'not supplied',database_rollback:false}));
 } finally {
  // Recover a test-owned Engine even when the first status acknowledgement fails.
  const owned=engines(cache);
  for(const pid of owned) {
   try {process.kill(pid,'SIGTERM');}catch(e){if(e.code!=='ESRCH')throw e;}
   const deadline=Date.now()+10000;let stopped=false;
   while(Date.now()<deadline) {
    try {if(fs.readFileSync(`/proc/${pid}/stat`,'utf8').split(') ')[1].startsWith('Z ')){stopped=true;break;}}
    catch(e){if(e.code==='ENOENT'){stopped=true;break;}throw e;}
    pause();
   }
   assert.ok(stopped,`owned Engine ${pid} must stop before fixture cleanup`);
  }
  fs.rmSync(root,{recursive:true,force:true});
 }
});
