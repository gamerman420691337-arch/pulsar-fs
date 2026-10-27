// Authored public capture-capacity proof; real CUDA, no media or device dispatch.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import {spawn,spawnSync} from 'node:child_process';
const worker=process.env.PULSAR_CUDA_CLI_WORKER,runtime=process.env.PULSAR_CUDA_CLI_RUNTIME;
const model=process.env.PULSAR_CUDA_CLI_MODEL,profileFile=process.env.PULSAR_CUDA_CLI_PROFILE;
const owner=process.env.PULSAR_CUDA_CLI_POLICY;
const digest=b=>crypto.createHash('sha256').update(b).digest('hex');
test('prepared capture counts both leases at the public eight-slot boundary',
  {skip:!worker||!runtime||!model||!profileFile||!owner||process.platform!=='linux',timeout:90000},async t=>{
    const root=fs.mkdtempSync(path.join(os.tmpdir(),'pulsar-prepared-capacity-'));fs.chmodSync(root,0o700);
    t.diagnostic(`authored prepared capacity evidence: ${root}`);
    const state=path.join(root,'state'),policyFile=path.join(root,'owner.json');
    const original=fs.readFileSync(owner),policy=JSON.parse(original),profile=JSON.parse(fs.readFileSync(profileFile));
    assert.equal(policy.accounting.maximum_reserved_bytes,128*2**20);
    // Explicit private overlap campaign; production owner limits remain untouched.
    policy.accounting.maximum_reserved_bytes=256*2**20;
    fs.writeFileSync(policyFile,JSON.stringify(policy),{mode:0o600});
    assert.equal(profile.provider,'cuda');assert.equal(digest(fs.readFileSync(model)),profile.model.sha256);
    assert.equal(digest(fs.readFileSync(runtime)),profile.runtime.sha256);
    const env={...process.env,PULSAR_STATE_DIR:state,PULSAR_ORT_DYLIB:runtime,PULSAR_CUDA_OWNER_POLICY:policyFile};
    const fd=fs.openSync(path.join(root,'engine.log'),'wx',0o600);
    const engine=spawn(worker,['engine'],{env,stdio:['ignore','ignore',fd]});fs.closeSync(fd);
    let exit;const closed=new Promise(resolve=>{
      engine.once('error',error=>{exit={error:error.message};resolve(exit);});
      engine.once('exit',(code,signal)=>{exit={code,signal};resolve(exit);});
    });
    const commands=[],captures=[],observations=[];
    const cli=(args,success=true)=>{
      const r=spawnSync(worker,args,{env,encoding:'utf8',timeout:15000,maxBuffer:4<<20});
      commands.push({args,status:r.status,error:r.error?.message,stdout:r.stdout,stderr:r.stderr});assert.ifError(r.error);
      if(!success)return r;assert.equal(r.status,0,r.stderr);return JSON.parse(r.stdout).value;
    };
    const until=async(observe,message,ms=15000)=>{
      const end=Date.now()+ms;
      for(;;){assert.equal(exit,undefined,'owned Engine exited');const v=observe();if(v)return v;
        assert.ok(Date.now()<end,message);await new Promise(resolve=>setTimeout(resolve,5));}
    };
    const empty=()=>{const f=JSON.parse(fs.readFileSync(path.join(state,'worker-fence.json')));
      return ['groups','retired','staging'].every(k=>Object.keys(f[k]).length===0)&&Object.keys(f.creation.records).length===0;};
    t.after(async()=>{
      try{if(!exit){for(const capture of captures)cli(['live-capture','stop',capture.identity.project,'0',capture.identity.epoch],false);
        if(fs.existsSync(path.join(state,'worker-fence.json')))await until(empty,'owned worker cleanup');}
      }finally{if(!exit)engine.kill('SIGTERM');await closed;
        fs.writeFileSync(path.join(root,'commands.json'),JSON.stringify(commands,null,2),{mode:0o600});
        fs.writeFileSync(path.join(root,'exit.json'),JSON.stringify({pid:engine.pid,...exit}),{mode:0o600});}
    });
    await until(()=>fs.existsSync(path.join(state,'engine.sock'))&&cli(['capabilities'],false).status===0,'startup');
    const project=label=>cli(['project','create',label]).project_id;
    const main=project('authored-capacity-inference');
    const options={policy:{maximum_analysis_age_ns:1000000000,added_buffer_ns:0},source_anchor:{numerator:0,denominator:1},project_anchor:0,presentation_anchor:0,lease_ms:30000};
    const files={};for(const [key,value] of Object.entries({passive:options,
      immediate:{...options,source_anchor:{numerator:1,denominator:1},project_anchor:1000000000},start:{...options,inference:{model_path:model,profile}},
      prepared:{...options,inference:{model_path:model,profile},continuation_mode:'when_ready',source_anchor:{numerator:1,denominator:1},project_anchor:1000000000}})){
      files[key]=path.join(root,`${key}.json`);fs.writeFileSync(files[key],JSON.stringify(value),{mode:0o600});}
    const status=capture=>cli(['live-capture','status',capture.identity.project,'0',capture.identity.epoch]);
    const start=(project,file)=>{const r=cli(['live-capture','start',project,'0',file]);captures.push(r);return r;};
    const prior=start(main,files.start);
    await until(()=>{const r=status(prior);assert.equal(r.active,true,JSON.stringify(r.inference));return r.inference.state==='running';},'initial Ready');
    assert.equal(cli(['live-capture','limits',main,'0']).maximum_active,8);
    const passive=[];for(let i=0;i<7;i++)passive.push(start(project(`authored-capacity-passive-${i}`),files.passive));
    const observe=label=>{const reports=captures.map(status);observations.push({label,reports});return reports.filter(r=>r.active);};
    assert.equal(observe('initial-full').length,8);
    const exhausted=args=>{const r=cli(args,false);assert.equal(r.status,1);assert.match(r.stderr,/ResourceExhausted|resource_exhausted|capture lease capacity exhausted/);};
    exhausted(['live-capture','continue',main,'0',prior.identity.epoch,files.prepared,'--request-id','capacity-full-refusal']);
    const preserved=status(prior);assert.equal(preserved.active,true);assert.equal(preserved.inference.state,'running');
    cli(['live-capture','stop',passive[0].identity.project,'0',passive[0].identity.epoch]);
    assert.equal(observe('one-slot-free').length,7);
    const next=cli(['live-capture','continue',main,'0',prior.identity.epoch,files.prepared,'--request-id','capacity-prepared']);captures.push(next);
    assert.equal(next.inference.state,'starting');
    const pending=observe('prepared-pending-full');assert.equal(pending.length,8);
    assert.equal(pending.find(r=>r.identity.epoch===next.identity.epoch).inference.state,'starting');
    assert.equal(pending.find(r=>r.identity.epoch===prior.identity.epoch).inference.state,'running');
    const extra=project('authored-capacity-ninth');exhausted(['live-capture','start',extra,'0',files.passive]);
    const replay=cli(['live-capture','continue',main,'0',prior.identity.epoch,files.prepared,'--request-id','capacity-prepared']);
    assert.deepEqual(replay.identity,next.identity);assert.equal(replay.inference.attempt_id,next.inference.attempt_id);
    await until(()=>{const r=status(next);assert.equal(r.active,true,JSON.stringify(r.inference));return r.inference.state==='running';},'prepared Ready');
    const switched=observe('prepared-ready-slot-free');assert.equal(switched.length,7);
    assert.equal(status(prior).retirement,'superseded');
    const admitted=start(extra,files.passive);assert.equal(observe('refilled').length,8);
    const immediate=cli(['live-capture','continue',admitted.identity.project,'0',admitted.identity.epoch,files.immediate]);captures.push(immediate);
    assert.equal(status(admitted).retirement,'superseded');assert.equal(observe('immediate-full-replacement').length,8);
    exhausted(['live-capture','start',project('authored-capacity-still-ninth'),'0',files.passive]);
    const pixels=path.join(root,'pixels.rgb'),header=path.join(root,'frame.json');
    for(let frame=0;frame<2;frame++){
      const bytes=Buffer.alloc(12288,frame*64);fs.writeFileSync(pixels,bytes,{mode:0o600});
      fs.writeFileSync(header,JSON.stringify({frame,source_time:{numerator:30+frame,denominator:30},capture_tick:status(next).server_elapsed,sha256:digest(bytes)}),{mode:0o600});
      cli(['live-capture','push',main,'0',next.identity.epoch,header,pixels]);}
    const output=await until(()=>{const r=status(next);assert.equal(r.active,true);return r.inference.completed===1?r:false;},'successor actual CUDA output');
    assert.equal(output.inference.stale,0);assert.ok(Math.abs(output.inference.latest.result.payload.position-(.25+.5*64/510))<1e-4);
    cli(['live-capture','finish',main,'0',next.identity.epoch]);
    const terminal=await until(()=>{const r=status(next);return r.active?false:r;},'Finish');
    assert.equal(terminal.retirement,'completed');assert.equal(terminal.inference.cuda_evidence.summary.successful_runs,1);
    for(const c of captures)cli(['live-capture','stop',c.identity.project,'0',c.identity.epoch],false);
    await until(empty,'final custody');assert.equal(digest(fs.readFileSync(owner)),digest(original));
    fs.writeFileSync(path.join(root,'result.json'),JSON.stringify({status:'PASS',qualified:false,
      binary:{path:worker,sha256:digest(fs.readFileSync(worker))},prior,next,preserved,replay,output,terminal,observations,
      physical_dispatch:false,real_media:false,maximum_active:8,maximum_reserved_bytes:policy.accounting.maximum_reserved_bytes},null,2),{mode:0o600});
  });
