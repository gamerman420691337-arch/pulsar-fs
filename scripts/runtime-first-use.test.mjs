// Actual first-use and AppImage process custody; authored packaging only.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import crypto from 'node:crypto';
import {spawnSync} from 'node:child_process';
import {authoredOnnxFixture} from './benchmark-onnx-fixture.mjs';
const binary=process.env.PULSAR_TEST_INSTALLER,source=process.env.PULSAR_TEST_INSTALL_SOURCE;
const image=process.env.PULSAR_TEST_APPIMAGE;
const hash=p=>{const fd=fs.openSync(p,'r');try{const h=crypto.createHash('sha256'),b=Buffer.alloc(65536);for(;;){const n=fs.readSync(fd,b,0,b.length,null);if(!n)break;h.update(b.subarray(0,n));}return h.digest('hex');}finally{fs.closeSync(fd);}};
const cacheEngines=cache=>{
 const found=[];
 for(const pid of fs.readdirSync('/proc').filter(n=>/^\d+$/.test(n))) {
  try{const executable=fs.readlinkSync(`/proc/${pid}/exe`);if(executable.startsWith(`${cache}/`)&&executable.endsWith('/pulsar')&&fs.readFileSync(`/proc/${pid}/cmdline`).toString().split('\0').includes('engine'))found.push(Number(pid));}
  catch(e){if(e.code!=='ENOENT'&&e.code!=='EACCES'&&e.code!=='EPERM')throw e;}
 }
 return found;
};
test('graphical setup without a display refuses before installation or Engine startup',{skip:!(binary&&source),timeout:30000},()=>{
 const root=fs.mkdtempSync(path.join(os.tmpdir(),'first-use-no-display-'));
 fs.chmodSync(root,0o700);
 const cache=path.join(root,'cache'),state=path.join(root,'state');
 try {
  const result=spawnSync(binary,['runtime-launch',source,cache,'--'],{
   env:{PATH:'/usr/bin:/bin',PULSAR_STATE_DIR:state},encoding:'utf8',timeout:20000,maxBuffer:1<<20,
  });
  assert.ifError(result.error);
  assert.equal(result.status,1,result.stderr);
  assert.match(result.stderr,/Setup window could not open/);
  assert.equal(fs.existsSync(cache),false,'window failure must not prepare a cache');
  assert.equal(fs.existsSync(state),false,'window failure must not start an Engine');
  if(image) {
   const transient=path.join(root,'transient');fs.mkdirSync(transient,{mode:0o700});
   const packaged=spawnSync(image,[],{
    env:{PATH:'/usr/bin:/bin',PULSAR_STATE_DIR:state,PULSAR_RUNTIME_CACHE_ROOT:cache,
     APPIMAGE_EXTRACT_AND_RUN:'1',TMPDIR:transient},encoding:'utf8',timeout:20000,maxBuffer:1<<20,
   });
   assert.ifError(packaged.error);assert.equal(packaged.status,1,packaged.stderr);
   assert.match(packaged.stderr,/Setup window could not open/);
   assert.equal(fs.existsSync(cache),false);assert.equal(fs.existsSync(state),false);
   assert.deepEqual(fs.readdirSync(transient),[],'failed graphical launch must clean its extraction');
  }
 } finally {fs.rmSync(root,{recursive:true,force:true});}
});
test('persistent first use installs under hostile umask, verifies reuse and preserves damaged or unknown cache entries',{skip:!(binary&&source),timeout:120000},t=>{
 const root=fs.mkdtempSync(path.join(os.tmpdir(),'first-use-'));fs.chmodSync(root,0o700);
 const cache=path.join(root,'nested/cache'),env={PATH:'/usr/bin:/bin'};
 const launch=()=>spawnSync('/usr/bin/bash',['-c','umask 0777; exec "$@"','first-use',binary,'runtime-launch',source,cache,'--','--version'],{env,encoding:'utf8',timeout:60000,maxBuffer:1<<20});
 try {
  let r=launch();assert.ifError(r.error);assert.equal(r.status,0,r.stderr);assert.match(r.stdout,/0\.8\.0/);
  const manifest=JSON.parse(fs.readFileSync(path.join(source,'runtime-candidate.json'))),id=hash(path.join(source,'runtime-candidate.json')),installed=path.join(cache,id);
  for(const record of manifest.files)assert.equal(hash(path.join(installed,record.path)),record.sha256);
  assert.equal(fs.statSync(cache).mode&0o777,0o700);assert.equal(fs.statSync(path.join(cache,'.pulsar-runtime-cache.lock')).mode&0o777,0o600);
  const before=fs.statSync(path.join(installed,'pulsar'),{bigint:true});r=launch();assert.ifError(r.error);assert.equal(r.status,0,r.stderr);const after=fs.statSync(path.join(installed,'pulsar'),{bigint:true});assert.equal(after.ino,before.ino);assert.equal(after.mtimeNs,before.mtimeNs);
  const owner=path.join(installed,'owner-file');fs.writeFileSync(owner,'preserve');r=launch();assert.ifError(r.error);assert.notEqual(r.status,0);assert.equal(fs.readFileSync(owner,'utf8'),'preserve');assert.match(r.stderr,/inventory differs/);fs.unlinkSync(owner);
  const worker=path.join(installed,'pulsar-worker'),fd=fs.openSync(worker,'r+'),b=Buffer.alloc(1);fs.readSync(fd,b,0,1,0);b[0]^=1;fs.writeSync(fd,b,0,1,0);fs.closeSync(fd);const damaged=hash(worker);r=launch();assert.ifError(r.error);assert.notEqual(r.status,0);assert.match(r.stderr,/integrity differs/);assert.equal(hash(worker),damaged);
  t.diagnostic(JSON.stringify({manifest_sha256:id,files:manifest.files.length,first_use:'PASS',reuse:'verified; inode/mtime unchanged',umask:'0777; cache0700 and lock0600',unknown_and_corrupt:'refused and preserved',engine_started:false}));
 }finally{fs.rmSync(root,{recursive:true,force:true});}
});
test('actual AppImage client exits and automatic transient cleanup leaves Engine and later workers usable',{skip:!image,timeout:120000},async t=>{
 const root=fs.mkdtempSync(path.join(os.tmpdir(),'appimage-custody-'));fs.chmodSync(root,0o700);
 const cache=path.join(root,'xdg/pulsar/runtime'),state=path.join(root,'state'),transient=path.join(root,'transient');fs.mkdirSync(transient,{mode:0o700});
 const env={PATH:'/usr/bin:/bin',APPIMAGE_EXTRACT_AND_RUN:'1',TMPDIR:transient,XDG_CACHE_HOME:path.join(root,'xdg'),PULSAR_STATE_DIR:state};
 let enginePid;
 try {
  const run=args=>{const r=spawnSync(image,args,{env,encoding:'utf8',timeout:60000,maxBuffer:1<<20});assert.ifError(r.error);assert.equal(r.status,0,r.stderr);return r;};
  const teardown=()=>{assert.deepEqual(fs.readdirSync(transient),[],'AppImage must remove its extracted payload automatically');};
  const initial=run(['--version']);assert.match(initial.stdout,/0\.8\.0/);teardown();
  const names=fs.readdirSync(cache).filter(n=>/^[0-9a-f]{64}$/.test(n));assert.equal(names.length,1);const installed=path.join(cache,names[0]);
  run(['model','status']);teardown();
  const engines=cacheEngines(cache);assert.equal(engines.length,1,'exactly one test-owned persistent Engine');enginePid=engines[0];
  assert.ok(enginePid,'Engine executable is from persistent cache');
  const manifest=JSON.parse(fs.readFileSync(path.join(installed,'runtime-candidate.json')));
  for(const record of manifest.files)assert.equal(hash(path.join(installed,record.path)),record.sha256);
  assert.ok(fs.existsSync(path.join(installed,'pulsar-worker')));
  const later=spawnSync(path.join(installed,'pulsar'),['model','status'],{env:{PATH:'/usr/bin:/bin',PULSAR_STATE_DIR:state},encoding:'utf8',timeout:15000,maxBuffer:1<<20});assert.ifError(later.error);assert.equal(later.status,0,later.stderr);
  assert.equal(fs.readlinkSync(`/proc/${enginePid}/exe`),path.join(installed,'pulsar'));
  const cli=args=>{const r=spawnSync(path.join(installed,'pulsar'),args,{env:{PATH:'/usr/bin:/bin',PULSAR_STATE_DIR:state},encoding:'utf8',timeout:30000,maxBuffer:4<<20});assert.ifError(r.error);assert.equal(r.status,0,r.stderr);return r;};
  const catalog=JSON.parse(cli(['model','status']).stdout),runtime=manifest.files.find(r=>r.path==='runtimes/libonnxruntime.so.1');assert.ok(runtime);assert.equal(catalog.runtime.sha256,runtime.sha256);
  const model=path.join(root,'authored.onnx');fs.writeFileSync(model,authoredOnnxFixture());
  const profile=cli(['model','profile',model,'--width','64','--height','64','--class-count','1']),profilePath=path.join(root,'profile.json');fs.writeFileSync(profilePath,profile.stdout);
  const media=path.join(root,'source.y4m');fs.writeFileSync(media,Buffer.concat([Buffer.from('YUV4MPEG2 W64 H64 F8:1 Ip A1:1 Cmono\n'),...Array.from({length:16},(_,frame)=>Buffer.concat([Buffer.from('FRAME\n'),Buffer.from(Array.from({length:4096},(_,i)=>(i%64*31+((Math.floor(i/64)+64-frame)%64)*17)%256))]))]));
  const output=path.join(root,'output.funscript');cli(['generate',media,'--model',model,'--inference-profile',profilePath,'--width','64','--height','64','--output',output]);assert.ok(JSON.parse(fs.readFileSync(output)).actions.length>0);
  const fence=JSON.parse(fs.readFileSync(path.join(state,'worker-fence.json')));assert.deepEqual(fence.groups,{});assert.deepEqual(fence.creation.records,{});
  t.diagnostic(JSON.stringify({appimage_sha256:hash(image),manifest_sha256:names[0],files:manifest.files.length,engine_pid:enginePid,transient_extraction:'automatically removed after AppImage parent exit; no fixture teardown',engine_executable:'persistent cache',later_client:'PASS; same Engine',later_confined_worker_generation:'PASS; authored model/media; nonempty export and empty final custody',all_payloads:'PASS',model_quality:false,platform_qualified:false}));
 }finally{
  for(const enginePid of cacheEngines(cache)){process.kill(enginePid,'SIGTERM');const until=Date.now()+5000;while(fs.existsSync(`/proc/${enginePid}`)&&Date.now()<until){let stateText;try{stateText=fs.readFileSync(`/proc/${enginePid}/stat`,'utf8');}catch(e){if(e.code==='ENOENT')break;throw e;}if(stateText.slice(stateText.lastIndexOf(')')+2).startsWith('Z'))break;await new Promise(resolve=>setTimeout(resolve,40));}try{const stat=fs.readFileSync(`/proc/${enginePid}/stat`,'utf8');assert.ok(stat.slice(stat.lastIndexOf(')')+2).startsWith('Z'),'owned Engine must be terminal before cleanup');}catch(e){if(e.code!=='ENOENT')throw e;}}
  fs.rmSync(root,{recursive:true,force:true});
 }
});
