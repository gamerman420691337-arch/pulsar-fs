// Public CLI measurement of authored RGB ingress, not capture-to-command qualification.
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

test('public CUDA RGB measurement retains frame ages, bounded queue and continuation startup',
  {skip:!worker||!runtime||!policy||!profileFile||!model||process.platform!=='linux',timeout:180000},async t=>{
    const root=fs.mkdtempSync(path.join(os.tmpdir(),'pulsar-live-cuda-measurement-'));fs.chmodSync(root,0o700);
    t.diagnostic(`retained authored CUDA measurement: ${root}`);
    const state=path.join(root,'state'), ownerFile=path.join(root,'policy.json');
    fs.copyFileSync(policy,ownerFile);fs.chmodSync(ownerFile,0o600);
    const profile=JSON.parse(fs.readFileSync(profileFile));
    assert.equal(profile.provider,'cuda');assert.equal(digest(fs.readFileSync(model)),profile.model.sha256);
    assert.equal(digest(fs.readFileSync(runtime)),profile.runtime.sha256);
    const env={...process.env,PULSAR_STATE_DIR:state,PULSAR_ORT_DYLIB:runtime,PULSAR_CUDA_OWNER_POLICY:ownerFile};
    const log=fs.openSync(path.join(root,'engine.log'),'wx',0o600);
    const engine=spawn(worker,['engine'],{env,stdio:['ignore','ignore',log]});fs.closeSync(log);
    let exit;const closed=new Promise(resolve=>engine.once('exit',(code,signal)=>{exit={code,signal};resolve(exit);}));
    const commands=[], epochs=[];let project, activeEpoch;
    const cli=(args,allowFailure=false)=>{
      const begin=performance.now();const result=spawnSync(worker,args,{env,encoding:'utf8',timeout:15000,maxBuffer:4<<20});
      const elapsed_ms=performance.now()-begin;
      commands.push({args,status:result.status,elapsed_ms});assert.ifError(result.error);
      if(allowFailure)return result;
      assert.equal(result.status,0,result.stderr);return JSON.parse(result.stdout);
    };
    t.after(async()=>{
      if(!exit&&project&&activeEpoch)cli(['live-capture','stop',project,'0',activeEpoch],true);
      if(!exit&&fs.existsSync(path.join(state,'worker-fence.json'))){
        const end=performance.now()+10000;
        for(;;){const fence=JSON.parse(fs.readFileSync(path.join(state,'worker-fence.json')));
          if(['groups','retired','staging'].every(k=>Object.keys(fence[k]).length===0)
            &&Object.keys(fence.creation.records).length===0)break;
          if(performance.now()>=end){engine.kill('SIGTERM');await closed;assert.fail('unconfirmed custody retirement');}
          await delay(10);
        }
      }
      if(!exit)engine.kill('SIGTERM');await closed;
      fs.writeFileSync(path.join(root,'commands.json'),JSON.stringify(commands,null,2));
      fs.writeFileSync(path.join(root,'epochs.json'),JSON.stringify(epochs,null,2));
    });
    const until=async(observe,message,ms=10000)=>{
      const end=performance.now()+ms;
      for(;;){assert.equal(exit,undefined,'owned Engine exited');const value=observe();if(value)return value;
        assert.ok(performance.now()<end,message);await delay(5);}
    };
    await until(()=>fs.existsSync(path.join(state,'engine.sock'))&&cli(['capabilities'],true).status===0,'Engine startup');
    project=cli(['project','create','authored-live-measurement']).value.project_id;
    const status=epoch=>cli(['live-capture','status',project,'0',epoch]).value;
    const options={policy:{maximum_analysis_age_ns:200000000,added_buffer_ns:0},
      source_anchor:{numerator:0,denominator:1},project_anchor:0,presentation_anchor:0,
      lease_ms:30000,inference:{model_path:model,profile}};
    const optionsPath=path.join(root,'start.json');fs.writeFileSync(optionsPath,JSON.stringify(options),{mode:0o600});
    let prior, sourceFrame=0;
    for(let epochIndex=0;epochIndex<2;epochIndex++){
      const begin=performance.now();let initial;
      if(!prior)initial=cli(['live-capture','start',project,'0',optionsPath]).value;
      else {
        const next=path.join(root,'continue.json');
        fs.writeFileSync(next,JSON.stringify({...options,source_anchor:{numerator:sourceFrame,denominator:30},
          project_anchor:Math.floor(sourceFrame*1e9/30)}),{mode:0o600});
        initial=cli(['live-capture','continue',project,'0',prior.identity.epoch,next]).value;
      }
      activeEpoch=initial.identity.epoch;
      const running=await until(()=>{const report=status(activeEpoch);assert.equal(report.active,true,JSON.stringify(report));
        assert.notEqual(report.inference.state,'failed',JSON.stringify(report.inference));
        return report.inference.state==='running'?report:false;},'CUDA Ready',30000);
      const startup_ms=performance.now()-begin;
      const observations=[], uploads=[], sourceFrameBase=sourceFrame;let lastSequence=0;
      const observe=report=>{
        if(report.queue){assert.ok(report.queue.pending_frames<=1);assert.ok(report.queue.in_flight_frames<=1);}
        const estimate=report.inference.latest;if(!estimate)return;
        assert.equal(estimate.result.status,'cuda_provisional');assert.equal(estimate.qualified,false);
        const value=estimate.result.payload,sequence=value.input.sequence;
        if(sequence===lastSequence)return;assert.ok(sequence>lastSequence);lastSequence=sequence;
        const previous=value.input.pair.previous_frame,current=value.input.pair.current_frame;
        const expected=0.25+0.5*(((previous+sourceFrameBase)%64)*3+((current+sourceFrameBase)%64)*3)/510;
        assert.ok(Math.abs(value.position-expected)<1e-4,
          JSON.stringify({previous,current,expected,position:value.position}));
        assert.ok(estimate.capture_to_analysis_ns>=value.pair_analysis_elapsed_ns);
        observations.push({sequence,previous,current,position:value.position,expected,
          capture_tick:estimate.capture_tick,accepted_at:estimate.accepted_at,
          capture_to_analysis_ns:estimate.capture_to_analysis_ns,pair_analysis_elapsed_ns:value.pair_analysis_elapsed_ns,
          fresh:estimate.fresh,completed:report.inference.completed,accepted:report.inference.accepted,
          stale:report.inference.stale,queue:report.queue});
      };
      const pixels=path.join(root,'pixels.rgb'),header=path.join(root,'frame.json');
      const send=frame=>{
        const report=status(activeEpoch);assert.equal(report.active,true);observe(report);
        const bytes=Buffer.alloc(12288,(sourceFrame%64)*3);fs.writeFileSync(pixels,bytes,{mode:0o600});
        // A real Engine clock observation precedes this authored RGB event.
        // It is a conservative probe-to-analysis interval, not a camera timestamp.
        const value={frame,source_time:{numerator:sourceFrame,denominator:30},
          capture_tick:report.server_elapsed,sha256:digest(bytes)};
        fs.writeFileSync(header,JSON.stringify(value),{mode:0o600});
        const begin=performance.now();cli(['live-capture','push',project,'0',activeEpoch,header,pixels]);
        uploads.push({...value,sourceFrame,cli_upload_ms:performance.now()-begin});sourceFrame++;
      };
      // Use epoch-local frame ordinals; source time and pixel values continue globally.
      const pacedBegin=performance.now();
      for(let frame=0;frame<60;frame++){
        await delay(Math.max(0,pacedBegin+frame*1000/30-performance.now()));send(frame);
        if(frame>0)await until(()=>{const report=status(activeEpoch);observe(report);
          return report.inference.completed>=frame?report:false;},'paced inference completion');
      }
      const pacedEnd=status(activeEpoch);observe(pacedEnd);const paced_elapsed_ms=performance.now()-pacedBegin;
      const burstBegin=performance.now();
      for(let frame=60;frame<180;frame++)send(frame);
      const burst_ms=performance.now()-burstBegin;
      const drained=await until(()=>{const report=status(activeEpoch);observe(report);return report.queue.pending_frames===0
        &&report.queue.in_flight_frames===0?report:false;},'bounded queue drain');
      assert.equal(drained.received_frames,180);assert.equal(drained.inference.completed,
        drained.inference.accepted+drained.inference.stale);
      assert.ok(drained.inference.completed>=59);assert.ok(drained.inference.completed<=179);
      const record={epochIndex,initial,running,startup_ms,observations,uploads,pacedEnd,drained,burst_ms,
        paced_elapsed_ms};epochs.push(record);
      prior=drained;
    }
    const beforeFinish=status(activeEpoch);assert.ok(beforeFinish.inference.completed>0);
    cli(['live-capture','finish',project,'0',activeEpoch]);
    const terminal=await until(()=>{const value=status(activeEpoch);return value.active?false:value;},'consuming Finish');
    assert.equal(terminal.retirement,'completed');assert.equal(terminal.inference.cuda_evidence.qualified,false);
    assert.equal(terminal.inference.cuda_evidence.summary.successful_runs,beforeFinish.inference.completed);
    const evidence=terminal.inference.cuda_evidence;
    await until(()=>{const fence=JSON.parse(fs.readFileSync(path.join(state,'worker-fence.json')));
      return ['groups','retired','staging'].every(k=>Object.keys(fence[k]).length===0)&&Object.keys(fence.creation.records).length===0;
    },'worker custody retirement');activeEpoch=undefined;
    fs.writeFileSync(path.join(root,'result.json'),JSON.stringify({status:'PASS',qualified:false,
      scope:'Authored RGB probe-to-inference observations; no encoded decoding, six-axis reconstruction or dispatch',
      binary:{path:worker,sha256:digest(fs.readFileSync(worker))},epochs,terminal,evidence,
      capture_to_command_qualified:false,physical_dispatch:false,real_media:false,
      statistical_latency_acceptance:'Not established; no percentile agreement inferred'},null,2));
  });
