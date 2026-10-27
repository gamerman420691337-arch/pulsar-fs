// Host-local fence version migration and actual CPU directory reclamation.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import crypto from 'node:crypto';
import {spawn,spawnSync} from 'node:child_process';
import {setTimeout as delay} from 'node:timers/promises';
const hash=p=>{const h=crypto.createHash('sha256'),fd=fs.openSync(p,'r'),b=Buffer.alloc(1048576);try{for(;;){const n=fs.readSync(fd,b);if(!n)break;h.update(b.subarray(0,n));}}finally{fs.closeSync(fd);}return h.digest('hex');};
test('fence-v1 reader opens its bound state then refuses current creation admission without DB/WAL mutation',{
 skip:!process.env.PULSAR_STASH_TEST_BINARY||!process.env.PULSAR_FENCE_V1_READER||!process.env.PULSAR_CUDA_INTENT_FIXTURE,
 timeout:120000,
},async t=>{
 const binary=fs.realpathSync(process.env.PULSAR_STASH_TEST_BINARY),old=fs.realpathSync(process.env.PULSAR_FENCE_V1_READER),fixture=fs.realpathSync(process.env.PULSAR_CUDA_INTENT_FIXTURE),subjects=[binary,old,fixture],initial=subjects.map(hash);
 const root=fs.mkdtempSync(path.join(os.tmpdir(),'pulsar-cgroup-reclamation-'));fs.chmodSync(root,0o700);t.diagnostic(`retained cgroup-reclamation fixture: ${root}`);
 const state=path.join(root,'state'),db=path.join(state,'projects.sqlite3'),ledgerPath=path.join(state,'worker-fence.json'),env={PATH:'/usr/bin:/bin',PULSAR_STATE_DIR:state};
 const run=(program,args,options={})=>{const r=spawnSync(program,args,{env,encoding:'utf8',timeout:15000,maxBuffer:2*1024*1024,...options});assert.ifError(r.error);return r;};
 const sql=statement=>{const r=run('/usr/bin/python3',['-c','import sqlite3,json,sys;d=sqlite3.connect("file:"+sys.argv[1]+"?mode=ro",uri=True);print(json.dumps(d.execute(sys.argv[2]).fetchall()))',db,statement]);assert.equal(r.status,0,r.stderr);return JSON.parse(r.stdout);};
 let engine,ended,stderr='';
 const start=async program=>{engine=spawn(program,['engine'],{env,stdio:['ignore','ignore','pipe']});stderr='';engine.stderr.on('data',b=>stderr=(stderr+b).slice(-16384));ended=new Promise(resolve=>engine.once('exit',resolve));const deadline=Date.now()+15000;
  for(;;){assert.equal(engine.exitCode,null,stderr);assert.equal(engine.signalCode,null,stderr);assert.ok(Date.now()<deadline,'engine ready deadline');if(fs.existsSync(path.join(state,'engine.sock'))&&run(program,['capabilities']).status===0)return;await delay(25);}
 };
 const stop=async()=>{if(engine?.exitCode===null&&engine.signalCode===null){engine.kill('SIGTERM');const timer=setTimeout(()=>engine.kill('SIGKILL'),2000);try{await ended;}finally{clearTimeout(timer);}}};
 const synth=(program,label)=>{const output=path.join(root,`${label}.funscript`),r=run(program,['synthesize','pattern=triangle axis=stroke duration=1s frequency=1hz amplitude=0.1 offset=0.5','--output',output]);assert.equal(r.status,0,r.stderr);assert.ok(JSON.parse(fs.readFileSync(output)).actions.length>0);};
 try{
  await start(old);synth(old,'v1-positive');assert.equal(sql('PRAGMA user_version')[0][0],17);const prior=JSON.parse(fs.readFileSync(ledgerPath));assert.equal(prior.version,1);assert.deepEqual(prior.groups,{});assert.equal(sql('SELECT scope FROM worker_fence_binding')[0][0],prior.scope);await stop();
  await start(old);assert.equal(JSON.parse(fs.readFileSync(ledgerPath)).version,1);await stop();
  await start(binary);const current=JSON.parse(fs.readFileSync(ledgerPath));assert.equal(current.version,2);assert.equal(current.scope,prior.scope);assert.equal(current.lease_device,prior.lease_device);assert.equal(current.lease_inode,prior.lease_inode);assert.deepEqual(current.groups,{});assert.deepEqual(current.retired,{});synth(binary,'v2-positive');assert.deepEqual(JSON.parse(fs.readFileSync(ledgerPath)).retired,{});await stop();
  const holder=spawn(fixture,['hold',db],{env,stdio:['pipe','pipe','pipe']});let heldError='';holder.stderr.on('data',b=>heldError=(heldError+b).slice(-16384));const heldExit=new Promise(resolve=>holder.once('exit',(code,signal)=>resolve({code,signal})));let timer;
  try{
   await new Promise((resolve,reject)=>{let text='';timer=setTimeout(()=>reject(new Error('WAL ready deadline')),5000);holder.once('error',reject);holder.once('exit',()=>reject(new Error(heldError)));holder.stdout.on('data',b=>{text+=b;if(text.includes('\n')){try{assert.equal(JSON.parse(text.trim()).wal_fixture_ready,true);resolve();}catch(e){reject(e);}}});});clearTimeout(timer);assert.ok(fs.statSync(db+'-wal').size>0);
   const before={main:hash(db),wal:hash(db+'-wal'),ledger:hash(ledgerPath)},refused=run(old,['engine']);assert.notEqual(refused.status,0);assert.match(refused.stderr,/unknown field `retired`|worker fence|schema.*migration/i);assert.deepEqual({main:hash(db),wal:hash(db+'-wal'),ledger:hash(ledgerPath)},before);// No older reader became ready.
   const capabilities=run(old,['capabilities']);assert.notEqual(capabilities.status,0);
   const report={scope:'preceding version-1 reader and actual CPU retirement only',preceding_v1_positive_open:true,legacy_scope_and_lease_preserved:true,database_reader_barrier:20,host_fence_version:4,actual_cpu_synthesis:true,active_and_retired_records_cleared:true,older_reader_refused:true,before,after:{main:hash(db),wal:hash(db+'-wal'),ledger:hash(ledgerPath)},reader_error:refused.stderr,physical_device:false,production_cuda:false};
   if(process.env.PULSAR_RECLAMATION_EVIDENCE){fs.mkdirSync(process.env.PULSAR_RECLAMATION_EVIDENCE,{mode:0o700});fs.writeFileSync(path.join(process.env.PULSAR_RECLAMATION_EVIDENCE,'checked.json'),JSON.stringify(report,null,2)+'\n',{flag:'wx'});}
  }finally{clearTimeout(timer);holder.stdin.end(Buffer.from([1]));const kill=setTimeout(()=>holder.kill('SIGKILL'),2000);try{const r=await heldExit;assert.equal(r.code,0,heldError);assert.equal(r.signal,null);}finally{clearTimeout(kill);}}
  await start(binary);assert.equal(sql('PRAGMA user_version')[0][0],20);assert.equal(JSON.parse(fs.readFileSync(ledgerPath)).scope,prior.scope);await stop();assert.deepEqual(subjects.map(hash),initial);
  t.diagnostic(JSON.stringify({root,legacy_v1_positive_open:true,fence_version:4,db_schema:20,actual_cpu_synthesis:true,physical_device:false}));
 }finally{await stop();}
});
