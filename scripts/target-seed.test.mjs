// Authored two-box model verifies application wiring, not perception quality.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import {spawn,spawnSync} from 'node:child_process';
import {setTimeout as delay} from 'node:timers/promises';
import {authoredOnnxFixture} from './benchmark-onnx-fixture.mjs';

test('manual source-frame target seed reaches confined native generation and refuses absent or unsupported selection',
  {skip:!process.env.PULSAR_BENCHMARK_WORKER || !process.env.PULSAR_BENCHMARK_ORT,timeout:90000},async t=>{
    const root=fs.mkdtempSync(path.join(os.tmpdir(),'pulsar-target-seed-'));fs.chmodSync(root,0o700);
    t.diagnostic(`retained authored target-selection fixture: ${root}`);
    const binary=fs.realpathSync(process.env.PULSAR_BENCHMARK_WORKER),state=path.join(root,'state');
    const env={PATH:'/usr/bin:/bin',PULSAR_STATE_DIR:state,PULSAR_ORT_DYLIB:process.env.PULSAR_BENCHMARK_ORT};
    let engine,exit,stderr='';
    const start=()=>{engine=spawn(binary,['engine'],{env,stdio:['ignore','ignore','pipe']});engine.stderr.on('data',b=>{stderr=(stderr+b).slice(-65536);});exit=new Promise(resolve=>engine.once('exit',resolve));};
    const cli=args=>{const result=spawnSync(binary,args,{env,encoding:'utf8',timeout:30000,maxBuffer:1024*1024});assert.equal(result.error,undefined);return result;};
    const ok=args=>{const r=cli(args);assert.equal(r.status,0,r.stderr+r.stdout);return JSON.parse(r.stdout);};
    const ready=async()=>{const deadline=Date.now()+15000;while(true){assert.equal(engine.exitCode,null,stderr);assert.ok(Date.now()<deadline,'engine startup deadline');if(fs.existsSync(path.join(state,'engine.sock'))&&cli(['capabilities']).status===0)return;await delay(25);}};
    const stop=async()=>{if(engine?.exitCode===null && engine.signalCode===null){engine.kill('SIGTERM');const timer=setTimeout(()=>engine.kill('SIGKILL'),2000);try{await exit;}finally{clearTimeout(timer);}}};
    const attempts=()=>fs.existsSync(path.join(state,'attempts'))?fs.readdirSync(path.join(state,'attempts')):[];
    const hash=file=>crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
    try {
      start();await ready();
      const model=path.join(root,'two-boxes.onnx');fs.writeFileSync(model,authoredOnnxFixture({twoTargets:true}));
      const profile=path.join(root,'profile.json');fs.writeFileSync(profile,JSON.stringify(ok(['model','profile',model,'--width','64','--height','64','--class-count','1'])));
      const video=path.join(root,'stationary.y4m'),texture=Buffer.from(Array.from({length:4096},(_,i)=>(i%64*31+Math.floor(i/64)*17)%256));
      fs.writeFileSync(video,Buffer.concat([Buffer.from('YUV4MPEG2 W64 H64 F8:1 Ip A1:1 Cmono\n'),...Array.from({length:8},()=>Buffer.concat([Buffer.from('FRAME\n'),texture]))]));
      const sourceHash=hash(video),modelHash=hash(model),seedPath=path.join(root,'target.json');
      const args=(name,withSeed=true)=>['generate',video,'--model',model,'--inference-profile',profile,'--width','64','--height','64','--output',path.join(root,`${name}.funscript`),...(withSeed?['--target-seed',seedPath]:[])];
      const run=(name,withSeed=true)=>{const before=attempts();const result=cli(args(name,withSeed));const added=attempts().filter(id=>!before.includes(id));assert.equal(added.length,1,result.stderr);const folder=path.join(state,'attempts',added[0]);return {result,folder,checkpoint:JSON.parse(fs.readFileSync(path.join(folder,'initial-checkpoint.json'))),receipt:fs.existsSync(path.join(folder,'receipt.json'))?JSON.parse(fs.readFileSync(path.join(folder,'receipt.json'))):null};};
      const baseline=run('automatic',false);assert.equal(baseline.result.status,0,baseline.result.stderr);assert.equal(baseline.receipt.samples[0].selected_track.split('.').at(-1),'0');
      const seed={source_frame_index:0,x:0.3,y:0.5,class_id:0};fs.writeFileSync(seedPath,JSON.stringify(seed));
      const chosen=run('manual');assert.equal(chosen.result.status,0,chosen.result.stderr);
      assert.deepEqual(chosen.checkpoint.operation.arguments.settings.target_seed,seed);assert.deepEqual(chosen.receipt.manual_target_seed,seed);
      assert.equal(chosen.receipt.samples[0].selected_track.split('.').at(-1),'1');assert.equal(chosen.receipt.samples[0].selected_class,0);assert.equal(chosen.receipt.samples[0].observed_boxes,2);
      assert.ok(chosen.receipt.samples.every(s=>s.selected_track===chosen.receipt.samples[0].selected_track));assert.ok(chosen.receipt.review.some(r=>r.reason==='manual_target_seed_selected'));assert.ok(!chosen.receipt.review.some(r=>r.reason==='automatic_target_selection_unqualified'));
      assert.equal(chosen.receipt.qualification,'unqualified');assert.deepEqual(JSON.parse(fs.readFileSync(path.join(root,'manual.funscript'))).actions,JSON.parse(fs.readFileSync(path.join(root,'automatic.funscript'))).actions);
      fs.writeFileSync(seedPath,JSON.stringify({...seed,x:0.5}));const missing=run('missing');assert.notEqual(missing.result.status,0);assert.equal(fs.existsSync(path.join(root,'missing.funscript')),false);assert.ok(missing.receipt);assert.ok(missing.receipt.samples.every(s=>s.selected_track===null && s.evidence==='unavailable'));assert.ok(missing.receipt.review.some(r=>r.reason==='manual_target_seed_missing'));assert.ok(missing.receipt.unresolved_gaps.length>0);
      fs.writeFileSync(seedPath,JSON.stringify({...seed,source_frame_index:99}));const outside=run('outside');assert.notEqual(outside.result.status,0);assert.equal(outside.receipt,null);assert.match(outside.result.stderr,/outside the selected decoded source support/);assert.equal(fs.existsSync(path.join(root,'outside.funscript')),false);
      const beforeInvalid=attempts();for(const bad of [{...seed,x:1},{...seed,class_id:1},{...seed,extra:'guess'}]){fs.writeFileSync(seedPath,JSON.stringify(bad));assert.notEqual(cli(args('invalid')).status,0);}assert.deepEqual(attempts(),beforeInvalid);
      fs.writeFileSync(seedPath,JSON.stringify(seed));const noModel=cli(['generate',video,'--target-seed',seedPath,'--output',path.join(root,'no-model.funscript')]);assert.notEqual(noModel.status,0);assert.deepEqual(attempts(),beforeInvalid);
      const project=chosen.receipt.project_id,before=ok(['project','open',project]);await stop();start();await ready();assert.deepEqual(ok(['project','open',project]),before);assert.deepEqual(JSON.parse(fs.readFileSync(path.join(chosen.folder,'receipt.json'))),chosen.receipt);assert.equal(hash(video),sourceHash);assert.equal(hash(model),modelHash);
      if(process.env.PULSAR_TARGET_SEED_EVIDENCE){fs.mkdirSync(process.env.PULSAR_TARGET_SEED_EVIDENCE,{recursive:true});fs.writeFileSync(path.join(process.env.PULSAR_TARGET_SEED_EVIDENCE,'authored-cli.json'),JSON.stringify({seed,source_sha256:sourceHash,model_sha256:modelHash,baseline:baseline.receipt,chosen:chosen.receipt,missing:missing.receipt,outside_error:outside.result.stderr,restart_master_unchanged:true},null,2)+'\n',{flag:'wx'});}
    } finally {await stop();}
  });
