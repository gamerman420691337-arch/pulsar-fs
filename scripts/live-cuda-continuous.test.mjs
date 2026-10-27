// Authored paced ingress exposes startup/handoff gaps; not capture-to-command qualification.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import {spawn, spawnSync} from 'node:child_process';
import {performance} from 'node:perf_hooks';

const worker=process.env.PULSAR_CUDA_CLI_WORKER, runtime=process.env.PULSAR_CUDA_CLI_RUNTIME;
const policy=process.env.PULSAR_CUDA_CLI_POLICY, profileFile=process.env.PULSAR_CUDA_CLI_PROFILE;
const model=process.env.PULSAR_CUDA_CLI_MODEL;
const delay=ms=>new Promise(resolve=>setTimeout(resolve,ms));
const digest=bytes=>crypto.createHash('sha256').update(bytes).digest('hex');

test('paced public CUDA ingress continues through startup and explicit/derived epoch handoffs',
  {skip:!worker||!runtime||!policy||!profileFile||!model||process.platform!=='linux',timeout:150000},async t=>{
    const root=fs.mkdtempSync(path.join(os.tmpdir(),'pulsar-live-cuda-continuous-'));fs.chmodSync(root,0o700);
    t.diagnostic(`retained authored continuous CUDA ingress: ${root}`);
    const state=path.join(root,'state'), ownerFile=path.join(root,'policy.json');
    fs.copyFileSync(policy,ownerFile);fs.chmodSync(ownerFile,0o600);
    const profile=JSON.parse(fs.readFileSync(profileFile));
    assert.equal(profile.provider,'cuda');assert.equal(digest(fs.readFileSync(model)),profile.model.sha256);
    assert.equal(digest(fs.readFileSync(runtime)),profile.runtime.sha256);
    const env={...process.env,PULSAR_STATE_DIR:state,PULSAR_ORT_DYLIB:runtime,PULSAR_CUDA_OWNER_POLICY:ownerFile};
    const log=fs.openSync(path.join(root,'engine.log'),'wx',0o600);
    const engine=spawn(worker,['engine'],{env,stdio:['ignore','ignore',log]});fs.closeSync(log);
    let exit;const closed=new Promise(resolve=>engine.once('exit',(code,signal)=>{exit={code,signal};resolve(exit);}));
    const commands=[],epochs=[];let project,activeEpoch;
    const cli=(args,allowFailure=false)=>{
      const begin=performance.now();const r=spawnSync(worker,args,{env,encoding:'utf8',timeout:15000,maxBuffer:4<<20});
      commands.push({args,status:r.status,elapsed_ms:performance.now()-begin});assert.ifError(r.error);
      if(allowFailure)return r;assert.equal(r.status,0,r.stderr);return JSON.parse(r.stdout);
    };
    const until=async(observe,message,ms=10000)=>{
      const end=performance.now()+ms;
      for(;;){assert.equal(exit,undefined,'owned Engine exited');const v=observe();if(v)return v;
        assert.ok(performance.now()<end,message);await delay(5);}
    };
    const fenceEmpty=()=>{const f=JSON.parse(fs.readFileSync(path.join(state,'worker-fence.json')));
      return ['groups','retired','staging'].every(k=>Object.keys(f[k]).length===0)&&Object.keys(f.creation.records).length===0;};
    t.after(async()=>{
      try{
        if(!exit&&project&&activeEpoch)cli(['live-capture','stop',project,'0',activeEpoch],true);
        if(!exit&&fs.existsSync(path.join(state,'worker-fence.json')))await until(fenceEmpty,'custody retirement');
      }finally{
        if(!exit)engine.kill('SIGTERM');await closed;
        fs.writeFileSync(path.join(root,'commands.json'),JSON.stringify(commands,null,2));
        fs.writeFileSync(path.join(root,'epochs.json'),JSON.stringify(epochs,null,2));
      }
    });
    await until(()=>fs.existsSync(path.join(state,'engine.sock'))&&cli(['capabilities'],true).status===0,'Engine startup');
    project=cli(['project','create','authored-continuous-ingress']).value.project_id;
    const status=epoch=>cli(['live-capture','status',project,'0',epoch]).value;
    const options={policy:{maximum_analysis_age_ns:200000000,added_buffer_ns:0},source_anchor:{numerator:0,denominator:1},
      project_anchor:0,presentation_anchor:0,lease_ms:30000,inference:{model_path:model,profile}};
    const pixels=path.join(root,'pixels.rgb'),header=path.join(root,'frame.json');
    let prior,lastObservedWall,sourceFrame=0;const producerBegin=performance.now(),gaps=[];
    for(let epochIndex=0;epochIndex<3;epochIndex++){
      const epochBegin=performance.now();let initial,retiredPrior=null;
      if(!prior){const p=path.join(root,'start.json');fs.writeFileSync(p,JSON.stringify(options),{mode:0o600});
        initial=cli(['live-capture','start',project,'0',p]).value;
      }else if(epochIndex===1){const p=path.join(root,'continue.json');
        fs.writeFileSync(p,JSON.stringify({...options,source_anchor:{numerator:sourceFrame,denominator:30},project_anchor:sourceFrame*1e9/30}),{mode:0o600});
        initial=cli(['live-capture','continue',project,'0',prior.identity.epoch,p]).value;
      }else{const p=path.join(root,'anchor.json');fs.writeFileSync(p,JSON.stringify({numerator:sourceFrame,denominator:30}),{mode:0o600});
        initial=cli(['live-capture','continue-at',project,'0',prior.identity.epoch,p]).value;}
      activeEpoch=initial.identity.epoch;
      if(prior){retiredPrior=status(prior.identity.epoch);assert.equal(retiredPrior.active,false);
        assert.equal(retiredPrior.retirement,'superseded');assert.equal(initial.identity.source,prior.identity.source);}
      const observations=[],uploads=[],base=sourceFrame;let lastSequence=0,ready_ms=null;
      const observe=report=>{
        assert.equal(report.active,true,JSON.stringify(report.inference));assert.notEqual(report.inference.state,'failed',JSON.stringify(report.inference));
        if(report.inference.state==='running'&&ready_ms===null)ready_ms=performance.now()-epochBegin;
        if(report.queue){assert.ok(report.queue.pending_frames<=1);assert.ok(report.queue.in_flight_frames<=1);}
        const estimate=report.inference.latest;if(!estimate)return;
        assert.equal(estimate.result.status,'cuda_provisional');assert.equal(estimate.qualified,false);
        const v=estimate.result.payload;if(v.input.sequence===lastSequence)return;
        assert.ok(v.input.sequence>lastSequence);lastSequence=v.input.sequence;
        const previous=v.input.pair.previous_frame,current=v.input.pair.current_frame;
        const expected=.25+.5*(((previous+base)%64)*3+((current+base)%64)*3)/510;
        assert.ok(Math.abs(expected-v.position)<1e-4,JSON.stringify({expected,position:v.position}));
        const observed_wall_ms=performance.now()-producerBegin;
        if(lastObservedWall!==undefined)gaps.push({epoch:epochIndex,sequence:v.input.sequence,gap_ms:observed_wall_ms-lastObservedWall});
        lastObservedWall=observed_wall_ms;
        observations.push({sequence:v.input.sequence,previous,current,position:v.position,expected,observed_wall_ms,
          capture_tick:estimate.capture_tick,accepted_at:estimate.accepted_at,capture_to_analysis_ns:estimate.capture_to_analysis_ns,
          pair_analysis_elapsed_ns:v.pair_analysis_elapsed_ns,fresh:estimate.fresh});
      };
      const pacedBegin=performance.now();
      for(let frame=0;frame<600;frame++){
        await delay(Math.max(0,pacedBegin+frame*1000/30-performance.now()));
        const report=status(activeEpoch);observe(report);
        const bytes=Buffer.alloc(12288,(sourceFrame%64)*3);fs.writeFileSync(pixels,bytes,{mode:0o600});
        const value={frame,source_time:{numerator:sourceFrame,denominator:30},capture_tick:report.server_elapsed,sha256:digest(bytes)};
        fs.writeFileSync(header,JSON.stringify(value),{mode:0o600});
        cli(['live-capture','push',project,'0',activeEpoch,header,pixels]);
        uploads.push({...value,sourceFrame,observed_wall_ms:performance.now()-producerBegin,inference_state:report.inference.state});sourceFrame++;
      }
      const drained=await until(()=>{const report=status(activeEpoch);observe(report);
        return report.inference.state==='running'&&report.queue.pending_frames===0&&report.queue.in_flight_frames===0?report:false;
      },'paced epoch drain',2000);
      assert.equal(drained.received_frames,600);assert.equal(drained.inference.completed,drained.inference.accepted+drained.inference.stale);
      assert.ok(drained.inference.completed>0);assert.ok(drained.inference.completed<=599);
      epochs.push({epochIndex,initial,retiredPrior,ready_ms,observations,uploads,drained,paced_elapsed_ms:performance.now()-pacedBegin});prior=drained;
    }
    cli(['live-capture','finish',project,'0',activeEpoch]);
    const terminal=await until(()=>{const v=status(activeEpoch);return v.active?false:v;},'Finish');
    assert.equal(terminal.retirement,'completed');assert.equal(terminal.inference.cuda_evidence.summary.successful_runs,prior.inference.completed);
    await until(fenceEmpty,'confirmed custody retirement');activeEpoch=undefined;
    fs.writeFileSync(path.join(root,'result.json'),JSON.stringify({status:'PASS',qualified:false,
      scope:'60 seconds of paced authored RGB ingress including startup and explicit/derived handoff; observations only',
      binary:{path:worker,sha256:digest(fs.readFileSync(worker))},epochs,terminal,evidence:terminal.inference.cuda_evidence,
      observed_gaps:gaps,maximum_observed_gap_ms:Math.max(...gaps.map(v=>v.gap_ms)),first_observation_wall_ms:epochs[0].observations[0].observed_wall_ms,
      capture_to_command_qualified:false,physical_dispatch:false,real_media:false,
      limits:'Client-observed gaps include polling; no camera/encoded decoding/six-axis/device dispatch or statistical qualification'},null,2));
  });
