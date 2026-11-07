// Real CPU launch and host-local worker binding; authored media, no device use.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import crypto from 'node:crypto';
import {spawn,spawnSync} from 'node:child_process';
import {setTimeout as delay} from 'node:timers/promises';
const hash=p=>{const h=crypto.createHash('sha256'),fd=fs.openSync(p,'r'),b=Buffer.alloc(1048576);try{for(;;){const n=fs.readSync(fd,b);if(!n)break;h.update(b.subarray(0,n));}}finally{fs.closeSync(fd);}return h.digest('hex');};
test('engine CPU launches bind schema 20 and old or altered startup preserves committed DB/WAL',{
 skip:!process.env.PULSAR_STASH_TEST_BINARY||!process.env.PULSAR_SCHEMA16_WORKER_FENCE_READER||!process.env.PULSAR_CUDA_INTENT_FIXTURE,
 timeout:120000,
},async t=>{
 const binary=fs.realpathSync(process.env.PULSAR_STASH_TEST_BINARY),old=fs.realpathSync(process.env.PULSAR_SCHEMA16_WORKER_FENCE_READER),fixture=fs.realpathSync(process.env.PULSAR_CUDA_INTENT_FIXTURE);
 const subjects=[binary,old,fixture],initial=subjects.map(hash);
 const root=fs.mkdtempSync(path.join(os.tmpdir(),'pulsar-engine-worker-fence-'));fs.chmodSync(root,0o700);
 t.diagnostic(`retained worker-fence fixture: ${root}`);
 const state=path.join(root,'state'),db=path.join(state,'projects.sqlite3'),env={PATH:'/usr/bin:/bin',PULSAR_STATE_DIR:state};
 const run=(program,args,options={})=>{const r=spawnSync(program,args,{env,encoding:'utf8',timeout:15000,maxBuffer:2*1024*1024,...options});assert.ifError(r.error);return r;};
 const ok=(program,args)=>{const r=run(program,args);assert.equal(r.status,0,r.stderr);return JSON.parse(r.stdout);};
 const sql=(statement,params=[])=>{const r=run('/usr/bin/python3',['-c','import sqlite3,json,sys;v=json.loads(sys.stdin.read());d=sqlite3.connect(sys.argv[1]);rows=d.execute(v["sql"],v["params"]).fetchall();d.commit();print(json.dumps(rows))',db],{input:JSON.stringify({sql:statement,params})});assert.equal(r.status,0,r.stderr);return JSON.parse(r.stdout);};
 const bytes=()=>Object.fromEntries(['','-wal'].map(s=>[s,fs.existsSync(db+s)?hash(db+s):null]));
 let engine,ended,stderr='',startup=0;
 const start=async program=>{
  const index=startup++;engine=spawn(program,['engine'],{env,stdio:['ignore','ignore','pipe']});stderr='';engine.stderr.on('data',b=>{stderr=(stderr+b).slice(-16384);fs.writeFileSync(path.join(root,`engine-${index}.stderr`),stderr,{mode:0o600});});ended=new Promise(resolve=>engine.once('exit',resolve));const deadline=Date.now()+15000;
  for(;;){assert.equal(engine.exitCode,null,stderr);assert.ok(Date.now()<deadline,'engine ready deadline');if(fs.existsSync(path.join(state,'engine.sock'))){const r=run(program,['capabilities']);fs.writeFileSync(path.join(root,`ready-${index}.json`),JSON.stringify({program,status:r.status,stderr:r.stderr},null,2));if(r.status===0)return;}await delay(25);}
 };
 const stop=async()=>{if(engine?.exitCode===null&&engine.signalCode===null){engine.kill('SIGTERM');const timer=setTimeout(()=>engine.kill('SIGKILL'),2000);try{await ended;}finally{clearTimeout(timer);}}};
 const hold=async()=>{
  const child=spawn(fixture,['hold',db],{env,stdio:['pipe','pipe','pipe']});let err='';child.stderr.on('data',b=>err=(err+b).slice(-16384));const exit=new Promise(resolve=>child.once('exit',(code,signal)=>resolve({code,signal})));let timer;
  try{await new Promise((resolve,reject)=>{let text='';timer=setTimeout(()=>reject(new Error('WAL ready deadline')),5000);child.once('error',reject);child.once('exit',()=>reject(new Error(err)));child.stdout.on('data',b=>{text+=b;if(text.includes('\n')){try{assert.equal(JSON.parse(text.trim()).wal_fixture_ready,true);resolve();}catch(e){reject(e);}}});});}
  catch(e){child.kill('SIGKILL');await exit;throw e;}finally{clearTimeout(timer);}
  assert.ok(fs.statSync(db+'-wal').size>0);
  return async()=>{child.stdin.end(Buffer.from([1]));const timer=setTimeout(()=>child.kill('SIGKILL'),2000);try{const r=await exit;assert.equal(r.code,0,err);assert.equal(r.signal,null);}finally{clearTimeout(timer);}};
 };
 const readbacks=[];
 const refuse=async(program,mode)=>{const release=await hold();try{const before=bytes();const r=run(program,['engine']);assert.notEqual(r.status,0,mode);assert.match(r.stderr,/worker fence|schema.*migration/i,mode);assert.deepEqual(bytes(),before,mode);readbacks.push({mode,before,after:bytes(),exit_code:r.status,stderr:r.stderr});}finally{await release();}};
 try{
  await start(old);const project=ok(old,['project','create','schema-16-positive-control']).value.project_id;await stop();
  ok(fixture,['seed',db]);ok(fixture,['accept',db]);assert.equal(sql('PRAGMA user_version')[0][0],16);
  await start(old);ok(old,['project','open',project]);assert.equal(sql('PRAGMA user_version')[0][0],16);await stop();
  await start(binary);assert.equal(sql('PRAGMA user_version')[0][0],16);
  const output=path.join(root,'synthetic.funscript');const generated=run(binary,['synthesize','pattern=triangle axis=stroke duration=1s frequency=1hz amplitude=0.1 offset=0.5','--output',output]);assert.equal(generated.status,0,generated.stderr);assert.ok(JSON.parse(fs.readFileSync(output)).actions.length>0);
  assert.equal(sql('PRAGMA user_version')[0][0],20);
  const ledgerPath=path.join(state,'worker-fence.json'),leasePath=path.join(state,'worker-fence.lock');const ledgerBytes=fs.readFileSync(ledgerPath),ledger=JSON.parse(ledgerBytes);
  assert.equal(sql('SELECT scope FROM worker_fence_binding')[0][0],ledger.scope);assert.deepEqual(ledger.groups,{});
  assert.ok(sql("SELECT COUNT(*) FROM jobs WHERE state='completed' AND json_type(manifest,'$.cuda_execution') IS NULL")[0][0]>=1);
  await stop();await refuse(old,'schema16-reader');
  await start(binary);assert.equal(sql('PRAGMA user_version')[0][0],20);assert.deepEqual(JSON.parse(fs.readFileSync(ledgerPath)).groups,{});await stop();
  for(const mode of ['scope','missing-binding','downgrade','noncanonical-ledger','missing-ledger','missing-both','replaced-lease']){
   let restore;
   if(mode==='scope'){sql('UPDATE worker_fence_binding SET scope=?',[crypto.randomUUID()]);restore=()=>sql('UPDATE worker_fence_binding SET scope=?',[ledger.scope]);}
   else if(mode==='missing-binding'){sql('DROP TABLE worker_fence_binding');restore=()=>{sql('CREATE TABLE worker_fence_binding(scope TEXT PRIMARY KEY NOT NULL,minimum_ledger_version INTEGER NOT NULL CHECK(minimum_ledger_version=4))');sql('INSERT INTO worker_fence_binding VALUES(?,4)',[ledger.scope]);};}
   else if(mode==='downgrade'){sql('PRAGMA user_version=16');restore=()=>sql('PRAGMA user_version=20');}
   else if(mode==='noncanonical-ledger'){fs.writeFileSync(ledgerPath,Buffer.concat([ledgerBytes,Buffer.from(' ')]));restore=()=>fs.writeFileSync(ledgerPath,ledgerBytes);}
   else{
    const paths=mode==='missing-both'?[ledgerPath,leasePath]:mode==='replaced-lease'?[leasePath]:[ledgerPath];for(const p of paths)fs.renameSync(p,p+'.retained');
    if(mode==='replaced-lease')fs.writeFileSync(leasePath,'',{mode:0o600,flag:'wx'});
    restore=()=>{if(mode==='replaced-lease')fs.unlinkSync(leasePath);for(const p of paths)fs.renameSync(p+'.retained',p);};
   }
   try{
    const invalidLedger=fs.existsSync(ledgerPath)?hash(ledgerPath):null;
    await refuse(binary,mode);
    assert.equal(fs.existsSync(ledgerPath)?hash(ledgerPath):null,invalidLedger,mode);
    if(mode==='missing-both')assert.equal(fs.existsSync(leasePath),false);
   }finally{restore();}
  }
  await start(binary);assert.equal(sql('PRAGMA user_version')[0][0],20);assert.equal(sql('SELECT scope FROM worker_fence_binding')[0][0],ledger.scope);await stop();
  assert.deepEqual(subjects.map(hash),initial);
  const report={scope:'actual confined CPU launch, schema binding and startup refusal only',production_cuda:false,physical_device:false,preceding_schema16_positive_open:true,reader_barrier:20,real_cpu_launch:true,retirement_roster_empty:true,restart_same_scope:true,readbacks,executables:subjects.map((p,i)=>({path:p,sha256:initial[i]})),limitations:['No power-loss/exhaustive crash proof or independent host IO deadline.','No model/media/hardware/platform/human/release qualification.']};
  if(process.env.PULSAR_WORKER_FENCE_EVIDENCE){fs.mkdirSync(process.env.PULSAR_WORKER_FENCE_EVIDENCE,{mode:0o700});fs.writeFileSync(path.join(process.env.PULSAR_WORKER_FENCE_EVIDENCE,'checked.json'),JSON.stringify(report,null,2)+'\n',{flag:'wx'});}
  t.diagnostic(JSON.stringify({root,reader_refusals:readbacks.length,reader_barrier:20,real_cpu_launch:true,physical_device:false}));
 }finally{await stop();}
});
