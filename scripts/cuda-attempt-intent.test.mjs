// Authored private database states; no media inference or GPU/device effects.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import crypto from 'node:crypto';
import {spawn,spawnSync} from 'node:child_process';
import {setTimeout as delay} from 'node:timers/promises';

test('immutable CUDA attempt intent fences old writers and restart without GPU replay',
 {skip:!process.env.PULSAR_CUDA_INTENT_FIXTURE||!process.env.PULSAR_CUDA_SCHEMA14_READER||!process.env.PULSAR_STASH_TEST_BINARY,timeout:90000},async t=>{
  const binary=fs.realpathSync(process.env.PULSAR_STASH_TEST_BINARY),fixture=fs.realpathSync(process.env.PULSAR_CUDA_INTENT_FIXTURE),old=fs.realpathSync(process.env.PULSAR_CUDA_SCHEMA14_READER);
  const root=fs.mkdtempSync(path.join(os.tmpdir(),'pulsar-cuda-intent-'));fs.chmodSync(root,0o700);const state=path.join(root,'state'),env={PATH:'/usr/bin:/bin',PULSAR_STATE_DIR:state};
  const run=(program,args,environment=env)=>{const result=spawnSync(program,args,{env:environment,encoding:'utf8',timeout:15000,maxBuffer:2*1024*1024});assert.ifError(result.error);return result;};
  const ok=(program,args)=>{const result=run(program,args);assert.equal(result.status,0,result.stderr);return JSON.parse(result.stdout);};
  const digest=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
  const before=[binary,fixture,old].map(digest);const db=path.join(state,'projects.sqlite3');
  const readbacks=[];
  const databaseBytes=database=>Object.fromEntries(['','-wal'].map(suffix=>{const p=database+suffix;return [suffix,fs.existsSync(p)?digest(p):null];}));
  const holdWal=async database=>{
    const child=spawn(fixture,['hold',database],{env,stdio:['pipe','pipe','pipe']});let diagnostics='';child.stderr.on('data',b=>diagnostics=(diagnostics+b).slice(-16384));const ended=new Promise(resolve=>child.once('exit',(code,signal)=>resolve({code,signal})));
    let timer;try {await new Promise((resolve,reject)=>{let text='';timer=setTimeout(()=>reject(new Error('WAL hold ready deadline')),5000);child.once('error',reject);child.once('exit',code=>reject(new Error(`WAL hold exited ${code}: ${diagnostics}`)));child.stdout.on('data',b=>{text+=b;if(text.includes('\n')){try{assert.equal(JSON.parse(text.trim()).wal_fixture_ready,true);resolve();}catch(error){reject(error);}}});});}
    catch(error){child.kill('SIGKILL');await ended;throw error;}finally{clearTimeout(timer);}
    assert.ok(fs.statSync(database+'-wal').size>0,'committed nonempty WAL required');
    return async()=>{child.stdin.end(Buffer.from([1]));const timer=setTimeout(()=>child.kill('SIGKILL'),2000);try{const result=await ended;assert.equal(result.code,0,diagnostics);assert.equal(result.signal,null);}finally{clearTimeout(timer);}};
  };
  let engine,exit,stderr='';
  const start=async()=>{engine=spawn(binary,['engine'],{env,stdio:['ignore','ignore','pipe']});stderr='';engine.stderr.on('data',b=>stderr=(stderr+b).slice(-16384));exit=new Promise(resolve=>engine.once('exit',resolve));const deadline=Date.now()+15000;while(true){assert.equal(engine.exitCode,null,stderr);assert.ok(Date.now()<deadline,'engine ready deadline');if(fs.existsSync(path.join(state,'engine.sock'))&&run(binary,['capabilities']).status===0)return;await delay(25);}};
  const stop=async()=>{if(engine?.exitCode===null&&engine.signalCode===null){engine.kill('SIGTERM');const timer=setTimeout(()=>engine.kill('SIGKILL'),2000);try{await exit;}finally{clearTimeout(timer);}}};
  try {
    await start();const project=ok(binary,['project','create','Authored GPU reader barrier']).value.project_id;
    const script=path.join(root,'authored.funscript');fs.writeFileSync(script,JSON.stringify({actions:[{at:0,pos:20},{at:1000,pos:60},{at:2000,pos:30}]}));
    const candidate=ok(binary,['import-script',script,'--project',project]).value.candidate_id;ok(binary,['commit-candidate',project,candidate,'--revision','0']);
    const projectBefore=ok(binary,['project','open',project]).value;await stop();
    const seeded=ok(fixture,['seed',db]);assert.equal(seeded.schema,15);assert.equal(seeded.job_state,'queued');assert.equal(seeded.attempt_directory_exists,false);
    const release=await holdWal(db);try {const original=databaseBytes(db);const refusal=run(old,['engine']);assert.notEqual(refusal.status,0);assert.match(refusal.stderr,/project database schema.*original was not modified/);const after=databaseBytes(db);assert.deepEqual(after,original);readbacks.push({case:'preceding-schema14-reader',before:original,after,nonempty_wal:true,exit_code:refusal.status,stderr:refusal.stderr});} finally {await release();}
    await start();assert.deepEqual(ok(binary,['project','open',project]).value,projectBefore);
    const interrupted=ok(fixture,['check',db]);assert.equal(interrupted.schema,15);assert.equal(interrupted.job_state,'interrupted');assert.equal(interrupted.attempt_state,'interrupted');assert.equal(interrupted.engine_epoch,seeded.engine_epoch);assert.equal(interrupted.request_sha256,seeded.request_sha256);assert.equal(interrupted.attempt_directory_exists,false);
    ok(binary,['model','pack-deactivate','--expected-selection-revision','0']);assert.equal(ok(fixture,['check',db]).schema,15);await stop();
    await start();assert.deepEqual(ok(binary,['project','open',project]).value,projectBefore);assert.equal(ok(fixture,['check',db]).schema,15);await stop();
    const corruptions=[];
    for(const mode of ['downgrade','missing-table','digest','membership','oversized','oversized-text','attempt','job','scope','epoch','oversized-epoch','layout']) {
      const broken=path.join(root,mode);fs.mkdirSync(broken,{mode:0o700});const copy=path.join(broken,'projects.sqlite3');fs.copyFileSync(db,copy);
      ok(fixture,['tamper',copy,mode]);const release=await holdWal(copy);try{const frozen=databaseBytes(copy);const result=run(binary,['engine'],{...env,PULSAR_STATE_DIR:broken});assert.notEqual(result.status,0,mode);assert.match(result.stderr,/CUDA|invalid CUDA|missing field/,mode);if(mode==='oversized-text')assert.match(result.stderr,/capacity exceeded/);const after=databaseBytes(copy);assert.deepEqual(after,frozen,mode);readbacks.push({case:mode,before:frozen,after,nonempty_wal:true,exit_code:result.status,stderr:result.stderr});corruptions.push(mode);}finally{await release();}
    }
    assert.deepEqual([binary,fixture,old].map(digest),before);
    if(process.env.PULSAR_CUDA_INTENT_EVIDENCE) {fs.mkdirSync(process.env.PULSAR_CUDA_INTENT_EVIDENCE,{mode:0o700});fs.writeFileSync(path.join(process.env.PULSAR_CUDA_INTENT_EVIDENCE,'checked.json'),JSON.stringify({scope:'authored intent only',native_execution:false,seeded,interrupted,project_motion_preserved:true,readbacks,executables:[binary,fixture,old].map((p,i)=>({path:p,sha256:before[i]}))},null,2)+'\n',{flag:'wx'});}
    t.diagnostic(JSON.stringify({root,scope:'authored GPU intent only',native_execution:false,old_reader_refused_without_database_change:true,project_motion_preserved:true,restart_without_gpu_replay:true,corruptions,reader_barrier:15}));
  } finally {await stop();}
});
