// Native application wiring with authored finite media/model; no quality or device claim.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import net from 'node:net';
import crypto from 'node:crypto';
import {spawn,spawnSync} from 'node:child_process';
import {setTimeout as delay} from 'node:timers/promises';
import {authoredOnnxFixture} from './benchmark-onnx-fixture.mjs';

test('native interval regeneration preserves outside/protected motion and binds target selection to original support',
 {skip:!process.env.PULSAR_BENCHMARK_WORKER || !process.env.PULSAR_BENCHMARK_ORT || !process.env.PULSAR_SCHEMA7_TEST_BINARY,timeout:90000},async t=>{
 const root=fs.mkdtempSync(path.join(os.tmpdir(),'pulsar-generation-interval-'));fs.chmodSync(root,0o700);t.diagnostic(`retained authored interval fixture: ${root}`);
 const binary=fs.realpathSync(process.env.PULSAR_BENCHMARK_WORKER),state=path.join(root,'state'),env={PATH:'/usr/bin:/bin',PULSAR_STATE_DIR:state,PULSAR_ORT_DYLIB:process.env.PULSAR_BENCHMARK_ORT};
 let engine,exit,stderr='';const start=()=>{engine=spawn(binary,['engine'],{env,stdio:['ignore','ignore','pipe']});engine.stderr.on('data',b=>{stderr=(stderr+b).slice(-65536);});exit=new Promise(resolve=>engine.once('exit',resolve));};
 const cli=args=>{const r=spawnSync(binary,args,{env,encoding:'utf8',timeout:30000,maxBuffer:4*1024*1024});assert.equal(r.error,undefined);return r;};
 const ok=args=>{const r=cli(args);assert.equal(r.status,0,r.stderr+r.stdout);return JSON.parse(r.stdout);};
 const ready=async()=>{const end=Date.now()+15000;while(true){assert.equal(engine.exitCode,null,stderr);assert.ok(Date.now()<end,'engine startup deadline');if(fs.existsSync(path.join(state,'engine.sock'))&&cli(['capabilities']).status===0)return;await delay(25);}};
 const stop=async()=>{if(engine?.exitCode===null && engine.signalCode===null){engine.kill('SIGTERM');const timer=setTimeout(()=>engine.kill('SIGKILL'),2000);try{await exit;}finally{clearTimeout(timer);}}};
 let sequence=0;const rpc=(project,revision,operation,args)=>new Promise((resolve,reject)=>{const credentials=JSON.parse(fs.readFileSync(path.join(state,'first-party.session.json')));const bytes=Buffer.from(JSON.stringify({version:2,request_id:`interval-fixture-${++sequence}`,...credentials,project,expected_revision:revision,command:{operation,...(args===undefined?{}:{arguments:args})}}));const header=Buffer.alloc(4);header.writeUInt32BE(bytes.length);const socket=net.createConnection(path.join(state,'engine.sock'));let received=Buffer.alloc(0);socket.setTimeout(10000,()=>socket.destroy(new Error('IPC deadline')));socket.once('error',reject);socket.once('close',()=>reject(new Error('IPC closed before response')));socket.once('connect',()=>socket.write(Buffer.concat([header,bytes])));socket.on('data',b=>{received=Buffer.concat([received,b]);if(received.length<4)return;const size=received.readUInt32BE();if(size>1024*1024){socket.destroy(new Error('response bound'));return;}if(received.length>=size+4){resolve(JSON.parse(received.subarray(4,size+4)).result);socket.destroy();}});});
 const value=result=>{assert.ok(result.Ok,JSON.stringify(result));return result.Ok.value;};
 const attempts=()=>fs.existsSync(path.join(state,'attempts'))?fs.readdirSync(path.join(state,'attempts')):[];
 const hash=file=>crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
 try {
  start();await ready();const p=ok(['project','create','Authored interval regeneration']).value.project_id;
  const script=path.join(root,'original.funscript');fs.writeFileSync(script,JSON.stringify({actions:Array.from({length:8},(_,i)=>({at:i*1000,pos:40}))}));const scriptHash=hash(script);
  const imported=ok(['import-script',script,'--project',p]).value.candidate_id;ok(['commit-candidate',p,imported,'--revision','0']);
  const model=path.join(root,'two-boxes.onnx');fs.writeFileSync(model,authoredOnnxFixture({twoTargets:true}));const modelHash=hash(model);
  const profile=ok(['model','profile',model,'--width','64','--height','64','--class-count','1']);
  const video=path.join(root,'stationary.y4m'),texture=Buffer.from(Array.from({length:4096},(_,i)=>(i%64*31+Math.floor(i/64)*17)%256));fs.writeFileSync(video,Buffer.concat([Buffer.from('YUV4MPEG2 W64 H64 F8:1 Ip A1:1 Cmono\n'),...Array.from({length:64},()=>Buffer.concat([Buffer.from('FRAME\n'),texture]))]));const videoHash=hash(video);
  const source=value(await rpc(p,1,'import_source',{path:video})).source_version;
  const interval={start:1000000000,end:6000000000},seed={source_frame_index:8,x:0.3,y:0.5,class_id:0};
  const settings={analysis_width:64,analysis_height:64,source_sample_stride:1,axis:'stroke',vr_mode:false,model_input:profile.preprocessing.input,inference_profile:profile,project_interval:interval,target_seed:seed};
  const settingsFile=path.join(root,'settings.json');fs.writeFileSync(settingsFile,JSON.stringify(settings));
  const regen=rev=>['regenerate',p,source,'--revision',String(rev),'--settings',settingsFile,'--model',model];
  if(process.env.PULSAR_GENERATION_INTERVAL_BASELINE==='1') {
   const before=ok(['project','open',p]).value,beforeAttempts=attempts();const rejected=cli(regen(before.revision));assert.notEqual(rejected.status,0);assert.match(rejected.stderr,/unknown field.*project_interval/);assert.deepEqual(attempts(),beforeAttempts);assert.deepEqual(ok(['project','open',p]).value,before);
   if(process.env.PULSAR_GENERATION_INTERVAL_EVIDENCE){fs.mkdirSync(process.env.PULSAR_GENERATION_INTERVAL_EVIDENCE,{recursive:true});fs.writeFileSync(path.join(process.env.PULSAR_GENERATION_INTERVAL_EVIDENCE,'baseline.json'),JSON.stringify({state:'reproduced_unsupported_interval_generation',binary_sha256:hash(binary),error:rejected.stderr,master_unchanged:true,no_attempt_started:true},null,2)+'\n',{flag:'wx'});}return;
  }
  const original=ok(['project','open',p]).value;const draft=path.join(root,'manual.json');fs.writeFileSync(draft,JSON.stringify({tracks:[...original.program.tracks.map(track=>({axis:track.axis,actions:track.actions.map(a=>({time:a.time,position:a.time===3000000000?0.9:a.position})),gaps:track.gaps})),{axis:'yaw',actions:Array.from({length:8},(_,i)=>({time:i*1000000000,position:0.6})),gaps:[]}]}));
  const edit=ok(['edit-values',p,draft,'--revision','1']).value.candidate_id;const before=ok(['commit-candidate',p,edit,'--revision','1']).value;const protection=ok(['protection','status',p]).value.protected_regions;
  let job=value(await rpc(p,before.revision,'generate',{source_version:source,preset:'fast',settings,model_path:model}));const end=Date.now()+15000;while(!['completed','failed','cancelled','interrupted'].includes(job.state)){assert.ok(Date.now()<end,'job deadline');await delay(25);job=value(await rpc(p,null,'job_status',{job_id:job.job_id}));}assert.equal(job.state,'completed',job.error);
  const staged=value(await rpc(p,null,'get_candidate',{candidate_id:job.candidate_id}));assert.deepEqual(staged.project_interval,interval);
  assert.deepEqual(ok(['project','open',p]).value,before);assert.deepEqual(ok(['protection','status',p]).value.protected_regions,protection);
  for(const [operation,args,code] of [['commit_candidate',{candidate_id:job.candidate_id},'forbidden'],['merge_candidate',{candidate_id:job.candidate_id,axes:['stroke'],range:null},'invalid_request'],['merge_candidate',{candidate_id:job.candidate_id,axes:['stroke'],range:{start:0,end:6000000000}},'invalid_request']]){const denied=await rpc(p,before.revision,operation,args);assert.equal(denied.Err?.code,code,JSON.stringify(denied));assert.deepEqual(ok(['project','open',p]).value,before);}
  const regenerated=ok(regen(before.revision)).value;
  const stroke=project=>project.program.tracks.find(track=>track.axis==='stroke');const pos=(project,time)=>stroke(project).actions.find(a=>a.time===time)?.position;
  for(const time of [0,1000000000,6000000000,7000000000]){assert.deepEqual(stroke(regenerated).actions.find(a=>a.time===time),stroke(before).actions.find(a=>a.time===time));}
  assert.equal(pos(regenerated,3000000000),0.9);assert.equal(pos(regenerated,5000000000),0.5);
  assert.deepEqual(regenerated.program.tracks.find(t=>t.axis==='yaw'),before.program.tracks.find(t=>t.axis==='yaw'));assert.deepEqual(ok(['protection','status',p]).value.protected_regions,protection);
  const receipts=attempts().map(id=>path.join(state,'attempts',id,'receipt.json')).filter(file=>fs.existsSync(file)).map(file=>JSON.parse(fs.readFileSync(file)));
  assert.equal(receipts.length,2);for(const r of receipts){assert.deepEqual(r.configuration.project_interval,interval);assert.equal(r.decoded_sample_count,64);assert.equal(r.analysed_sample_count,40);assert.deepEqual(r.samples.map(s=>s.source_frame_index),Array.from({length:40},(_,i)=>i+8));assert.ok(r.samples.every(s=>s.project_time_ns>=interval.start && s.project_time_ns<interval.end));assert.equal(r.samples[0].selected_track.split('.').at(-1),'1');assert.ok(r.review.some(flag=>flag.reason==='manual_target_seed_selected'));assert.equal(r.qualification,'unqualified');}
  const undone=value(await rpc(p,regenerated.revision,'undo'));assert.deepEqual(ok(['project','open',p]).value.program,before.program);const redone=value(await rpc(p,undone.revision,'redo'));assert.deepEqual(ok(['project','open',p]).value.program,regenerated.program);
  const rebased=value(await rpc(p,redone.revision,'rebase_candidate',{candidate_id:job.candidate_id}));assert.deepEqual(rebased.project_interval,interval);const denied=await rpc(p,redone.revision,'merge_candidate',{candidate_id:rebased.candidate_id,axes:['stroke'],range:null});assert.equal(denied.Err?.code,'invalid_request');
  const metadata=payload=>{const r=spawnSync('/usr/bin/python3',['-c',"import sqlite3,json,sys; d=sqlite3.connect(sys.argv[1]); old=d.execute('SELECT manifest FROM jobs WHERE id=?',(sys.argv[2],)).fetchone()[0]; p=json.load(sys.stdin); d.execute('UPDATE jobs SET manifest=? WHERE id=?',(p['manifest'],sys.argv[2])) if 'manifest' in p else None; d.commit(); print(json.dumps(old))",path.join(state,'projects.sqlite3'),job.job_id],{env,input:JSON.stringify(payload),encoding:'utf8',timeout:10000,maxBuffer:2*1024*1024});assert.equal(r.status,0,r.stderr);return JSON.parse(r.stdout);};
  const originalManifest=metadata({});
  try {
   const wrong=JSON.parse(originalManifest);wrong.project_id='another-owned-fixture-project';metadata({manifest:JSON.stringify(wrong)});
   assert.equal((await rpc(p,null,'get_candidate',{candidate_id:rebased.candidate_id})).Err?.code,'dependency_mismatch');
   metadata({manifest:'x'.repeat(524289)});assert.equal((await rpc(p,null,'get_candidate',{candidate_id:rebased.candidate_id})).Err?.code,'resource_exhausted');
   assert.equal((await rpc(p,redone.revision,'merge_candidate',{candidate_id:rebased.candidate_id,axes:['stroke'],range:interval})).Err?.code,'resource_exhausted');
  } finally {metadata({manifest:originalManifest});}
  assert.deepEqual(value(await rpc(p,null,'get_candidate',{candidate_id:rebased.candidate_id})).project_interval,interval);
  const stable=ok(['project','open',p]).value;fs.writeFileSync(settingsFile,JSON.stringify({...settings,target_seed:{...seed,source_frame_index:0}}));const missing=cli(regen(stable.revision));assert.notEqual(missing.status,0);assert.match(missing.stderr,/outside the selected decoded source support/);assert.deepEqual(ok(['project','open',p]).value,stable);
  fs.writeFileSync(settingsFile,JSON.stringify({...settings,project_interval:{start:-1,end:6000000000}}));const count=attempts();assert.notEqual(cli(regen(stable.revision)).status,0);assert.deepEqual(attempts(),count);
  await stop();
  const db=path.join(state,'projects.sqlite3'),dbFiles=[db,`${db}-wal`].filter(file=>fs.existsSync(file)),dbHashes=dbFiles.map(hash);
  const old=spawnSync(fs.realpathSync(process.env.PULSAR_SCHEMA7_TEST_BINARY),['engine'],{env,encoding:'utf8',timeout:10000,maxBuffer:1024*1024});assert.equal(old.error,undefined);assert.notEqual(old.status,0);assert.match(old.stderr,/schema requires an explicit recoverable migration/);assert.deepEqual(dbFiles.map(hash),dbHashes);
  start();await ready();assert.deepEqual(ok(['project','open',p]).value,stable);assert.deepEqual(value(await rpc(p,null,'get_candidate',{candidate_id:rebased.candidate_id})).project_interval,interval);assert.deepEqual(ok(['protection','status',p]).value.protected_regions,protection);assert.equal(hash(script),scriptHash);assert.equal(hash(video),videoHash);assert.equal(hash(model),modelHash);
  const oldBinary=fs.realpathSync(process.env.PULSAR_SCHEMA7_TEST_BINARY),legacyState=path.join(root,'legacy-state'),legacyEnv={...env,PULSAR_STATE_DIR:legacyState};
  const legacyCli=(executable,args)=>{const r=spawnSync(executable,args,{env:legacyEnv,encoding:'utf8',timeout:10000,maxBuffer:1024*1024});assert.equal(r.error,undefined);assert.equal(r.status,0,r.stderr+r.stdout);return JSON.parse(r.stdout);};
  let migratedBefore,migratedAfter,legacyProcess,legacyExit;
  const legacyStart=async executable=>{legacyProcess=spawn(executable,['engine'],{env:legacyEnv,stdio:['ignore','ignore','pipe']});legacyExit=new Promise(resolve=>legacyProcess.once('exit',resolve));let errors='';legacyProcess.stderr.on('data',b=>{errors=(errors+b).slice(-65536);});const deadline=Date.now()+10000;while(true){assert.equal(legacyProcess.exitCode,null,errors);assert.ok(Date.now()<deadline,'migration startup deadline');if(fs.existsSync(path.join(legacyState,'engine.sock'))){const probe=spawnSync(executable,['capabilities'],{env:legacyEnv,encoding:'utf8',timeout:1000});if(probe.status===0)return;}await delay(25);}};
  const legacyStop=async()=>{if(legacyProcess?.exitCode===null && legacyProcess.signalCode===null){legacyProcess.kill('SIGTERM');const timer=setTimeout(()=>legacyProcess.kill('SIGKILL'),2000);try{await legacyExit;}finally{clearTimeout(timer);}}};
  try {
   await legacyStart(oldBinary);const lp=legacyCli(oldBinary,['project','create','Authored schema-7 migration']).value.project_id;const lc=legacyCli(oldBinary,['import-script',script,'--project',lp]).value.candidate_id;legacyCli(oldBinary,['commit-candidate',lp,lc,'--revision','0']);migratedBefore=legacyCli(oldBinary,['project','open',lp]).value;
   await legacyStop();await legacyStart(binary);migratedAfter=legacyCli(binary,['project','open',lp]).value;assert.deepEqual(migratedAfter,migratedBefore);
   const version=spawnSync('/usr/bin/python3',['-c',"import sqlite3,sys; d=sqlite3.connect(sys.argv[1]); print(d.execute('PRAGMA user_version').fetchone()[0])",path.join(legacyState,'projects.sqlite3')],{env:legacyEnv,encoding:'utf8',timeout:10000});assert.equal(version.status,0,version.stderr);assert.equal(version.stdout.trim(),'9');assert.deepEqual(legacyCli(binary,['protection','status',lp]).value.protected_regions,[]);
  } finally {await legacyStop();}
  if(process.env.PULSAR_GENERATION_INTERVAL_EVIDENCE){fs.mkdirSync(process.env.PULSAR_GENERATION_INTERVAL_EVIDENCE,{recursive:true});fs.writeFileSync(path.join(process.env.PULSAR_GENERATION_INTERVAL_EVIDENCE,'authored-cli.json'),JSON.stringify({interval,seed,before,protection,regenerated,receipts,schema7_migration:{before:migratedBefore,after:migratedAfter,version:9},script_sha256:scriptHash,video_sha256:videoHash,model_sha256:modelHash,outside_motion_and_missing_axis_preserved:true,whole_and_unscoped_replacement_refused:true,oversized_and_mismatched_native_manifest_refused:true,historical_schema7_reader_refused_without_database_change:true,actual_restart:true},null,2)+'\n',{flag:'wx'});}
 } finally {await stop();}
});
