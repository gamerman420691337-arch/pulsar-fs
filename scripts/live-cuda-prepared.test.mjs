// Authored public worker-overlap proof; excludes camera, six-axis and device dispatch.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import {spawn,spawnSync} from 'node:child_process';
import {performance} from 'node:perf_hooks';
const worker=process.env.PULSAR_CUDA_CLI_WORKER,runtime=process.env.PULSAR_CUDA_CLI_RUNTIME;
const model=process.env.PULSAR_CUDA_CLI_MODEL,profileFile=process.env.PULSAR_CUDA_CLI_PROFILE;
const owner=process.env.PULSAR_CUDA_CLI_POLICY;
const digest=b=>crypto.createHash('sha256').update(b).digest('hex');
const delay=ms=>new Promise(r=>setTimeout(r,ms));
for(const overlap of [true,false]) test(`public prepared CUDA ${overlap?'overlap switches at Ready':'budget refusal preserves predecessor'}`,
  {skip:!worker||!runtime||!model||!profileFile||!owner||process.platform!=='linux',timeout:90000},async t=>{
    const root=fs.mkdtempSync(path.join(os.tmpdir(),'pulsar-live-cuda-prepared-'));fs.chmodSync(root,0o700);
    t.diagnostic(`authored prepared CUDA evidence: ${root}`);
    const state=path.join(root,'state'),policyFile=path.join(root,'policy.json');
    const original=fs.readFileSync(owner),policy=JSON.parse(original),profile=JSON.parse(fs.readFileSync(profileFile));
    assert.equal(policy.accounting.maximum_reserved_bytes,128*2**20);
    if(overlap)policy.accounting.maximum_reserved_bytes=256*2**20;
    fs.writeFileSync(policyFile,JSON.stringify(policy),{mode:0o600});
    assert.equal(profile.provider,'cuda');assert.equal(digest(fs.readFileSync(model)),profile.model.sha256);
    assert.equal(digest(fs.readFileSync(runtime)),profile.runtime.sha256);
    const env={...process.env,PULSAR_STATE_DIR:state,PULSAR_ORT_DYLIB:runtime,PULSAR_CUDA_OWNER_POLICY:policyFile};
    const fd=fs.openSync(path.join(root,'engine.log'),'wx',0o600);
    const engine=spawn(worker,['engine'],{env,stdio:['ignore','ignore',fd]});fs.closeSync(fd);
    let exit;const closed=new Promise(resolve=>engine.once('exit',(code,signal)=>{exit={code,signal};resolve(exit);}));
    const commands=[],observations=[],uploads=[];let project,prior,next;
    const begin=performance.now(),lastSequence=new Map();let lastObserved,gapMax=0;
    const cli=(args,allowFailure=false)=>{
      const start=performance.now(),r=spawnSync(worker,args,{env,encoding:'utf8',timeout:15000,maxBuffer:4<<20});
      commands.push({args,status:r.status,elapsed_ms:performance.now()-start});assert.ifError(r.error);
      if(allowFailure)return r;assert.equal(r.status,0,r.stderr);return JSON.parse(r.stdout).value;
    };
    const until=async(fn,message,ms=10000)=>{
      const end=performance.now()+ms;
      for(;;){assert.equal(exit,undefined,'owned Engine exited');const v=fn();if(v)return v;
        assert.ok(performance.now()<end,message);await delay(5);}
    };
    const empty=()=>{const f=JSON.parse(fs.readFileSync(path.join(state,'worker-fence.json')));
      return ['groups','retired','staging'].every(k=>Object.keys(f[k]).length===0)&&Object.keys(f.creation.records).length===0;};
    t.after(async()=>{
      try{if(!exit&&project)for(const epoch of [prior?.identity.epoch,next?.identity.epoch].filter(Boolean))
        cli(['live-capture','stop',project,'0',epoch],true);
        if(!exit&&fs.existsSync(path.join(state,'worker-fence.json')))await until(empty,'owned custody retirement');
      }finally{if(!exit)engine.kill('SIGTERM');await closed;
        fs.writeFileSync(path.join(root,'commands.json'),JSON.stringify(commands,null,2));
        fs.writeFileSync(path.join(root,'observations.json'),JSON.stringify(observations,null,2));
        fs.writeFileSync(path.join(root,'uploads.json'),JSON.stringify(uploads,null,2));
        fs.writeFileSync(path.join(root,'exit.json'),JSON.stringify(exit));}
    });
    await until(()=>fs.existsSync(path.join(state,'engine.sock'))&&cli(['capabilities'],true).status===0,'startup');
    project=cli(['project','create','authored-prepared-overlap']).project_id;
    const status=epoch=>cli(['live-capture','status',project,'0',epoch]);
    const options={policy:{maximum_analysis_age_ns:200000000,added_buffer_ns:0},source_anchor:{numerator:0,denominator:1},
      project_anchor:0,presentation_anchor:0,lease_ms:30000,inference:{model_path:model,profile}};
    const startFile=path.join(root,'start.json');fs.writeFileSync(startFile,JSON.stringify(options),{mode:0o600});
    prior=cli(['live-capture','start',project,'0',startFile]);
    const bases=new Map([[prior.identity.epoch,0]]);let globalFrame=0,priorFrame=0,nextFrame=0;
    const observe=r=>{
      if(r.queue){assert.ok(r.queue.pending_frames<=1);assert.ok(r.queue.in_flight_frames<=1);}
      const e=r.inference.latest;if(!e)return;
      const v=e.result.payload,key=r.identity.epoch;if(v.input.sequence===(lastSequence.get(key)||0))return;
      assert.ok(v.input.sequence>(lastSequence.get(key)||0));lastSequence.set(key,v.input.sequence);
      assert.equal(e.result.status,'cuda_provisional');assert.equal(e.qualified,false);
      const base=bases.get(key),expected=.25+.5*(((base+v.input.pair.previous_frame)%64)*3+((base+v.input.pair.current_frame)%64)*3)/510;
      assert.ok(Math.abs(v.position-expected)<1e-4);
      const wall=performance.now()-begin;if(lastObserved!==undefined)gapMax=Math.max(gapMax,wall-lastObserved);lastObserved=wall;
      observations.push({epoch:key,sequence:v.input.sequence,wall_ms:wall,position:v.position,expected,
        capture_to_analysis_ns:e.capture_to_analysis_ns,capture_tick:e.capture_tick,accepted_at:e.accepted_at});
    };
    const pixels=path.join(root,'pixels.rgb'),header=path.join(root,'frame.json');
    const push=(r,frame)=>{
      const bytes=Buffer.alloc(12288,(globalFrame%64)*3);fs.writeFileSync(pixels,bytes,{mode:0o600});
      const value={frame,source_time:{numerator:globalFrame,denominator:30},capture_tick:r.server_elapsed,sha256:digest(bytes)};
      fs.writeFileSync(header,JSON.stringify(value),{mode:0o600});
      const result=cli(['live-capture','push',project,'0',r.identity.epoch,header,pixels],true);
      if(result.status!==0){const retired=status(r.identity.epoch);assert.equal(retired.active,false);
        assert.ok(['superseded','worker_failed'].includes(retired.retirement),JSON.stringify(retired));}
      uploads.push({epoch:r.identity.epoch,globalFrame,...value,status:result.status});
    };
    const pace=async()=>{await delay(Math.max(0,begin+globalFrame*1000/30-performance.now()));};
    const startupEnd=performance.now()+16000;
    for(;;){await pace();const r=status(prior.identity.epoch);assert.equal(r.active,true,JSON.stringify(r));observe(r);
      push(r,priorFrame++);globalFrame++;if(r.inference.state==='running'&&r.inference.accepted>5&&globalFrame%30===0)break;
      assert.ok(performance.now()<startupEnd,'first worker Ready');}
    const before=status(prior.identity.epoch),prepareBegin=performance.now(),prepareFile=path.join(root,'prepare.json');
    fs.writeFileSync(prepareFile,JSON.stringify({...options,continuation_mode:'when_ready',
      source_anchor:{numerator:globalFrame,denominator:30},project_anchor:globalFrame*1e9/30}),{mode:0o600});
    next=cli(['live-capture','continue',project,'0',prior.identity.epoch,prepareFile]);bases.set(next.identity.epoch,globalFrame);
    assert.equal(next.inference.state,'starting');assert.equal(status(prior.identity.epoch).active,true);
    assert.equal(next.identity.source,prior.identity.source);
    let outcome,oldAtOutcome,outcomeElapsedMs,pendingOldOutputs=0,oldAccepted=before.inference.accepted;
    for(;;){await pace();let newer=status(next.identity.epoch);const older=status(prior.identity.epoch);
      if(!older.active&&older.retirement==='superseded'&&newer.inference.state==='starting')newer=status(next.identity.epoch);
      observe(older);observe(newer);
      if(newer.inference.state==='running'||!newer.active){outcome=newer;oldAtOutcome=older;outcomeElapsedMs=performance.now()-prepareBegin;break;}
      assert.equal(older.active,true,JSON.stringify(older));
      pendingOldOutputs+=Math.max(0,older.inference.accepted-oldAccepted);oldAccepted=older.inference.accepted;
      push(older,priorFrame++);push(newer,nextFrame++);globalFrame++;
      assert.ok(performance.now()-prepareBegin<16000,'preparation terminal observation');}
    assert.ok(pendingOldOutputs>20,'predecessor continued real CUDA output during preparation');
    let active;
    if(overlap){assert.equal(outcome.inference.state,'running');assert.equal(oldAtOutcome.active,false);
      assert.equal(oldAtOutcome.retirement,'superseded');active=next;
      assert.equal(cli(['live-capture','push',project,'0',prior.identity.epoch,header,pixels],true).status,1);
    }else{assert.equal(outcome.retirement,'worker_failed');assert.match(outcome.inference.error.message,/aggregate GPU admission budget exhausted/);
      assert.equal(oldAtOutcome.active,true);assert.equal(oldAtOutcome.inference.state,'running');active=prior;}
    for(let i=0;i<60;i++){await pace();const r=status(active.identity.epoch);assert.equal(r.active,true);observe(r);
      push(r,overlap?nextFrame++:priorFrame++);globalFrame++;}
    const drained=await until(()=>{const r=status(active.identity.epoch);observe(r);
      return r.queue.pending_frames===0&&r.queue.in_flight_frames===0?r:false;},'drain',2000);
    cli(['live-capture','finish',project,'0',active.identity.epoch]);
    const terminal=await until(()=>{const r=status(active.identity.epoch);return r.active?false:r;},'Finish');
    assert.equal(terminal.retirement,'completed');assert.equal(terminal.inference.cuda_evidence.summary.successful_runs,drained.inference.completed);
    await until(empty,'final custody');assert.equal(digest(fs.readFileSync(owner)),digest(original));
    fs.writeFileSync(path.join(root,'result.json'),JSON.stringify({status:'PASS',qualified:false,overlap,
      maximum_reserved_bytes:policy.accounting.maximum_reserved_bytes,binary:{path:worker,sha256:digest(fs.readFileSync(worker))},
      prior,next,before,outcome,oldAtOutcome,pendingOldOutputs,preparation_ms:outcomeElapsedMs,
      drained,terminal,observations,uploads,maximum_observed_gap_ms:gapMax,
      capture_to_command_qualified:false,physical_dispatch:false,real_media:false,
      scope:'two authored scalar epochs; actual CUDA predecessor output during preparation and strict budget refusal'},null,2));
  });
