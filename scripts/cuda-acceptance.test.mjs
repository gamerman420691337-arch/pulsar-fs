// Authored accepted-evidence consistency fixtures; no native inference or admission.
import test from 'node:test';import assert from 'node:assert/strict';
import fs from 'node:fs';import path from 'node:path';import os from 'node:os';import crypto from 'node:crypto';
import {spawn,spawnSync} from 'node:child_process';import {setTimeout as delay} from 'node:timers/promises';
const hash=p=>{const h=crypto.createHash('sha256'),fd=fs.openSync(p,'r'),b=Buffer.alloc(1048576);try{for(;;){const n=fs.readSync(fd,b);if(!n)break;h.update(b.subarray(0,n));}}finally{fs.closeSync(fd);}return h.digest('hex');};
test('accepted CUDA membership survives restart and fences old and corrupted readers before WAL mutation',
 {skip:!process.env.PULSAR_CUDA_INTENT_FIXTURE||!process.env.PULSAR_CUDA_SCHEMA15_READER||!process.env.PULSAR_STASH_TEST_BINARY,timeout:90000},async t=>{
 const binary=fs.realpathSync(process.env.PULSAR_STASH_TEST_BINARY),fixture=fs.realpathSync(process.env.PULSAR_CUDA_INTENT_FIXTURE),old=fs.realpathSync(process.env.PULSAR_CUDA_SCHEMA15_READER);
 const subjects=[binary,fixture,old],before=subjects.map(hash);
 const root=fs.mkdtempSync(path.join(os.tmpdir(),'pulsar-cuda-acceptance-'));fs.chmodSync(root,0o700);
 const state=path.join(root,'state'),env={PATH:'/usr/bin:/bin',PULSAR_STATE_DIR:state},db=path.join(state,'projects.sqlite3');
 const run=(program,args,e=env)=>{const r=spawnSync(program,args,{env:e,encoding:'utf8',timeout:15000,maxBuffer:2*1024*1024});assert.ifError(r.error);return r;};
 const ok=(program,args)=>{const r=run(program,args);assert.equal(r.status,0,r.stderr);return JSON.parse(r.stdout);};
 const bytes=database=>Object.fromEntries(['','-wal'].map(s=>[s,fs.existsSync(database+s)?hash(database+s):null]));
 let engine,ended,stderr='';const start=async()=>{engine=spawn(binary,['engine'],{env,stdio:['ignore','ignore','pipe']});stderr='';engine.stderr.on('data',b=>stderr=(stderr+b).slice(-16384));ended=new Promise(resolve=>engine.once('exit',resolve));const end=Date.now()+15000;
  while(true){assert.equal(engine.exitCode,null,stderr);assert.ok(Date.now()<end,'engine ready deadline');if(fs.existsSync(path.join(state,'engine.sock'))&&run(binary,['capabilities']).status===0)return;await delay(25);}};
 const stop=async()=>{if(engine?.exitCode===null&&engine.signalCode===null){engine.kill('SIGTERM');const timer=setTimeout(()=>engine.kill('SIGKILL'),2000);try{await ended;}finally{clearTimeout(timer);}}};
 const hold=async database=>{
  const child=spawn(fixture,['hold',database],{env,stdio:['pipe','pipe','pipe']});let err='';child.stderr.on('data',b=>err=(err+b).slice(-16384));const exit=new Promise(resolve=>child.once('exit',(code,signal)=>resolve({code,signal})));let timer;
  try{await new Promise((resolve,reject)=>{let text='';timer=setTimeout(()=>reject(new Error('WAL ready deadline')),5000);child.once('error',reject);child.once('exit',()=>reject(new Error(err)));child.stdout.on('data',b=>{text+=b;if(text.includes('\n')){try{assert.equal(JSON.parse(text.trim()).wal_fixture_ready,true);resolve();}catch(e){reject(e);}}});});}
  catch(e){child.kill('SIGKILL');await exit;throw e;}finally{clearTimeout(timer);}
  assert.ok(fs.statSync(database+'-wal').size>0);
  return async()=>{child.stdin.end(Buffer.from([1]));const timer=setTimeout(()=>child.kill('SIGKILL'),2000);try{const r=await exit;assert.equal(r.code,0,err);assert.equal(r.signal,null);}finally{clearTimeout(timer);}};
 };
 const readbacks=[];
 try{
  await start();const project=ok(binary,['project','create','Authored CUDA membership']).value.project_id;await stop();
  ok(fixture,['seed',db]);const accepted=ok(fixture,['accept',db]);assert.equal(accepted.schema,16);assert.equal(accepted.job_state,'completed');assert.equal(accepted.attempt_state,'completed');
  let release=await hold(db);try{const frozen=bytes(db);const r=run(old,['engine']);assert.notEqual(r.status,0);assert.match(r.stderr,/explicit recoverable migration/);assert.deepEqual(bytes(db),frozen);readbacks.push({case:'schema15-reader',before:frozen,after:bytes(db),exit_code:r.status,stderr:r.stderr});}finally{await release();}
  await start();ok(binary,['project','open',project]);assert.equal(ok(fixture,['check',db]).job_state,'completed');
  ok(binary,['model','pack-deactivate','--expected-selection-revision','0']);assert.equal(ok(fixture,['check',db]).schema,16);await stop();
  await start();assert.equal(ok(fixture,['check',db]).schema,16);await stop();
  for(const mode of ['downgrade','missing-table','membership','digest','lineage','candidate','state','oversized','noncanonical','layout']){
   const broken=path.join(root,'broken-'+mode);fs.mkdirSync(broken,{mode:0o700});const copy=path.join(broken,'projects.sqlite3');fs.copyFileSync(db,copy);
   ok(fixture,['tamper-acceptance',copy,mode]);release=await hold(copy);try{const frozen=bytes(copy);const r=run(binary,['engine'],{...env,PULSAR_STATE_DIR:broken});assert.notEqual(r.status,0,mode);assert.match(r.stderr,/CUDA/,mode);assert.deepEqual(bytes(copy),frozen,mode);readbacks.push({case:mode,before:frozen,after:bytes(copy),exit_code:r.status,stderr:r.stderr});}finally{await release();}
  }
  assert.deepEqual(subjects.map(hash),before);
  const report={scope:'authored accepted evidence consistency only',native_execution:false,production_gpu_admission:false,accepted,reader_barrier:16,restart_preserved_completed_candidate:true,readbacks,executables:subjects.map((p,i)=>({path:p,sha256:before[i]}))};
  if(process.env.PULSAR_CUDA_ACCEPTANCE_EVIDENCE){fs.mkdirSync(process.env.PULSAR_CUDA_ACCEPTANCE_EVIDENCE,{mode:0o700});fs.writeFileSync(path.join(process.env.PULSAR_CUDA_ACCEPTANCE_EVIDENCE,'checked.json'),JSON.stringify(report,null,2)+'\n',{flag:'wx'});}
  t.diagnostic(JSON.stringify({root,scope:report.scope,reader_refusals:readbacks.length,native_execution:false,reader_barrier:16}));
 }finally{await stop();}
});
