// Shared shipped CLI/local IPC owner-death harness; only explicitly supplied authored inputs.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import net from 'node:net';
import {spawn, spawnSync} from 'node:child_process';
import {authoredOnnxFixture} from './benchmark-onnx-fixture.mjs';
import {finishCancellation} from './live-capture-finish-cancellation-fixture.mjs';
import {cpuPreparedOverlap} from './live-cpu-prepared-overlap-fixture.mjs';
async function waitFor(observe,message,ms=10000) {
  const end=Date.now()+ms;
  for (;;) {const value=await observe();if(value)return value;assert.ok(Date.now()<end,message);await new Promise(resolve=>setTimeout(resolve,10));}
}
function accepts(endpoint) {
  return new Promise(resolve=>{
    const socket=net.createConnection(endpoint);let settled=false;
    const done=value=>{if(!settled){settled=true;socket.destroy();resolve(value);}};
    socket.once('connect',()=>done(true));socket.once('error',()=>done(false));socket.setTimeout(250,()=>done(false));
  });
}
export async function captureReplay(t,{worker,runtime,provider='cpu',ownerPolicy,declaredProfile,authoredModel,scenario='restart',prepared=false,overlapMaximumReservedBytes}) {
  assert.ok(['restart','blocked_finish','cpu_overlap'].includes(scenario),'unknown authored capture scenario');
  if(scenario==='cpu_overlap'){assert.equal(provider,'cpu');assert.equal(prepared,false);}
  if(scenario==='blocked_finish'){assert.equal(provider,'cuda');assert.equal(prepared,false);}
  if(overlapMaximumReservedBytes!==undefined)assert.ok(prepared&&provider==='cuda'&&Number.isSafeInteger(overlapMaximumReservedBytes)&&overlapMaximumReservedBytes>0);
  const root=fs.mkdtempSync(path.join(os.tmpdir(),provider==='cuda'?'pulsar-cuda-capture-replay-':'pulsar-capture-replay-'));fs.chmodSync(root,0o700);
  t.diagnostic(`retained authored CLI replay evidence: ${root}`);
  const state=path.join(root,'state');const env={...process.env,PULSAR_STATE_DIR:state,PULSAR_ORT_DYLIB:runtime};delete env.PULSAR_CUDA_OWNER_POLICY;
  if(provider==='cuda') {
    assert.ok(ownerPolicy&&declaredProfile&&authoredModel,'CUDA inputs must be explicitly pinned');
    const policy=path.join(root,'cuda-owner.json');
    if(overlapMaximumReservedBytes===undefined){fs.copyFileSync(ownerPolicy,policy);fs.chmodSync(policy,0o600);}
    else{const declared=JSON.parse(fs.readFileSync(ownerPolicy));declared.accounting.maximum_reserved_bytes=overlapMaximumReservedBytes;
      fs.writeFileSync(policy,JSON.stringify(declared),{mode:0o600});}
    env.PULSAR_CUDA_OWNER_POLICY=policy;
  }
  const owned=[];let sequence=0;
  function start(label) {
    const stderr=path.join(root,`${label}.stderr`);const fd=fs.openSync(stderr,'wx',0o600);
    const child=spawn(worker,['engine'],{env,stdio:['ignore','ignore',fd]});fs.closeSync(fd);
    const entry={child,stderr,result:null};entry.closed=new Promise(resolve=>{
      child.once('error',error=>{entry.result={error:error.message};resolve(entry.result);});
      child.once('exit',(code,signal)=>{entry.result={code,signal};resolve(entry.result);});
    });owned.push(entry);return entry;
  }
  async function stop(entry,signal) {if(!entry.result)assert.equal(entry.child.kill(signal),true);return entry.closed;}
  async function ready(entry) {await waitFor(async()=>{
    assert.equal(entry.result,null,fs.readFileSync(entry.stderr,'utf8'));
    return accepts(path.join(state,'engine.sock'));
  },'owned Engine did not accept local connections');}
  t.after(async()=>{for(const entry of owned)if(!entry.result)await stop(entry,'SIGKILL');
    fs.writeFileSync(path.join(root,'owned-exits.json'),JSON.stringify(owned.map(e=>({pid:e.child.pid,result:e.result})),null,2),{mode:0o600});});
  const cli=(args,success=true)=>{
    const result=spawnSync(worker,args,{env,encoding:'utf8',timeout:15000,maxBuffer:1048576});
    fs.writeFileSync(path.join(root,`cli-${sequence++}.json`),JSON.stringify({args,status:result.status,error:result.error?.message,stdout:result.stdout,stderr:result.stderr},null,2),{mode:0o600});
    assert.equal(result.error,undefined);
    if(success){assert.equal(result.status,0,result.stderr);return JSON.parse(result.stdout);}
    assert.notEqual(result.status,0);return result;
  };
  const first=start('owner');await ready(first);
  const project=cli(['project','create','authored-restart-replay']).value.project_id;
  const model=path.join(root,'authored-temporal.onnx');
  if(authoredModel){fs.copyFileSync(authoredModel,model);fs.chmodSync(model,0o600);}else{fs.writeFileSync(model,authoredOnnxFixture({temporal:true}),{mode:0o600});}
  const profile=declaredProfile?JSON.parse(fs.readFileSync(declaredProfile,'utf8')):cli(['model','profile',model,'--width','64','--height','64','--class-count','1','--temporal-script-position']);
  assert.equal(profile.provider,provider);
  const options={policy:{maximum_analysis_age_ns:1000000000,added_buffer_ns:0},source_anchor:{numerator:0,denominator:1},project_anchor:0,presentation_anchor:0,lease_ms:30000,inference:{model_path:model,profile}};
  const optionsPath=path.join(root,'start.json');fs.writeFileSync(optionsPath,JSON.stringify(options),{mode:0o600});
  const startArgs=['live-capture','start',project,'0',optionsPath,'--request-id','replay-start'];const initial=cli(startArgs).value;
  const running=async epoch=>waitFor(()=>{const report=cli(['live-capture','status',project,'0',epoch]).value;assert.equal(report.active,true,JSON.stringify(report.inference));return report.inference.state==='running'?report:false;},'capture did not reach Running',provider==='cuda'?30000:10000);
  await running(initial.identity.epoch);
  const initialGroups=prepared?Object.keys(JSON.parse(fs.readFileSync(path.join(state,'worker-fence.json'))).groups):[];
  assert.ok(!prepared||initialGroups.length>0);
  if(scenario==='cpu_overlap')return cpuPreparedOverlap({cli,waitFor,running,root,state,project,initial,options});
  if(scenario==='blocked_finish')return finishCancellation(t,{root,state,cli,waitFor,running,initial,startArgs,optionsPath,project,ownerPid:first.child.pid});
  const nextOptions=path.join(root,'continue.json');fs.writeFileSync(nextOptions,JSON.stringify({...options,...(prepared?{continuation_mode:'when_ready'}:{}),source_anchor:{numerator:1,denominator:1},project_anchor:1000000000}),{mode:0o600});
  const continueArgs=['live-capture','continue',project,'0',initial.identity.epoch,nextOptions,'--request-id','replay-continue'];const next=cli(continueArgs).value;
  const preparedObservations=[];
  if(prepared){const replay=cli(continueArgs).value;assert.deepEqual(replay.identity,next.identity);assert.equal(replay.inference.attempt_id,next.inference.attempt_id);
    const predecessor=cli(['live-capture','status',project,'0',initial.identity.epoch]).value;
    const successor=cli(['live-capture','status',project,'0',next.identity.epoch]).value;
    if(!predecessor.active){assert.equal(predecessor.retirement,'superseded');assert.equal(successor.inference.state,'running');}
    preparedObservations.push({replay,predecessor,successor});
    const conflict=path.join(root,'prepared-conflict.json');const altered=JSON.parse(fs.readFileSync(nextOptions));altered.lease_ms=20000;
    fs.writeFileSync(conflict,JSON.stringify(altered),{mode:0o600});
    const args=[...continueArgs];args[5]=conflict;cli(args,false);
  }
  await running(next.identity.epoch);
  if(prepared){const retired=cli(['live-capture','status',project,'0',initial.identity.epoch]).value;
    assert.equal(retired.active,false);assert.equal(retired.retirement,'superseded');
    // A Superseded capture is not proof its worker claim has retired. Observe exact original groups.
    await waitFor(()=>{const f=JSON.parse(fs.readFileSync(path.join(state,'worker-fence.json')));
      return initialGroups.every(id=>!f.groups[id]&&!f.retired[id]);},'superseded worker custody still owns overlap capacity');
  }

  const anchor=path.join(root,'anchor.json');fs.writeFileSync(anchor,JSON.stringify({numerator:2,denominator:1}),{mode:0o600});
  const derivedArgs=['live-capture','continue-at',project,'0',next.identity.epoch,anchor,'--request-id','replay-derived'];const derived=cli(derivedArgs).value;
  if(prepared){assert.equal(derived.options.continuation_mode,'when_ready');const replay=cli(derivedArgs).value;
    assert.deepEqual(replay.identity,derived.identity);assert.equal(replay.inference.attempt_id,derived.inference.attempt_id);}
  await running(derived.identity.epoch);
  if(prepared){const retired=cli(['live-capture','status',project,'0',next.identity.epoch]).value;
    assert.equal(retired.active,false);assert.equal(retired.retirement,'superseded');}

  const pixels=path.join(root,'pixels.rgb'),header=path.join(root,'frame.json');
  for(let frame=0;frame<2;frame++) {
    const bytes=Buffer.alloc(12288,frame*64);fs.writeFileSync(pixels,bytes,{mode:0o600});
    const current=cli(['live-capture','status',project,'0',derived.identity.epoch]).value;
    fs.writeFileSync(header,JSON.stringify({frame,source_time:{numerator:20+frame,denominator:10},capture_tick:current.server_elapsed,sha256:crypto.createHash('sha256').update(bytes).digest('hex')}),{mode:0o600});
    cli(['live-capture','push',project,'0',derived.identity.epoch,header,pixels]);
  }
  const before=await waitFor(()=>{const report=cli(['live-capture','status',project,'0',derived.identity.epoch]).value;assert.equal(report.active,true);return report.inference.completed===1?report:false;},'real CPU inference did not complete');
  const ledgerPath=path.join(state,'worker-fence.json');const ledger=JSON.parse(fs.readFileSync(ledgerPath));
  assert.ok(Object.keys(ledger.groups).length>0);assert.ok(Object.keys(ledger.staging).length>0);
  assert.ok(Object.values(ledger.groups).some(group=>/^populated 1$/m.test(fs.readFileSync(path.join(group.path,'cgroup.events'),'utf8'))));
  const killed=await stop(first,'SIGKILL');assert.equal(killed.signal,'SIGKILL');
  // Observe the exact recorded groups; do not signal or delete them manually.
  await waitFor(()=>Object.values(ledger.groups).every(group=>/^populated 0$/m.test(fs.readFileSync(path.join(group.path,'cgroup.events'),'utf8'))),'owned workers survived owner death');
  const recovered=start('recovered');await ready(recovered);
  const limits=cli(['live-capture','limits',project,'0']).value;assert.notEqual(limits.engine_epoch,initial.engine_epoch);
  const roster=()=>JSON.parse(fs.readFileSync(ledgerPath));
  for(const key of ['groups','retired','staging'])assert.deepEqual(roster()[key],{});assert.deepEqual(roster().creation.records,{});
  const replays=[];
  for(const [args,original] of [[startArgs,initial],[continueArgs,next],[derivedArgs,derived]]) {
    const replay=cli([...args,'--engine-epoch',original.engine_epoch]).value;
    assert.deepEqual(replay.identity,original.identity);assert.equal(replay.engine_epoch,original.engine_epoch);
    assert.equal(replay.active,false);assert.equal(replay.retirement,'engine_restarted');assert.equal(replay.admitted_pixel_bytes,0);assert.equal(replay.server_elapsed,null);
    assert.equal(replay.inference.state,'retired');assert.equal(replay.inference.latest,null);replays.push(replay);
  }
  cli(startArgs,false); // The implicit current boot changes the saved fingerprint.
  cli(['live-capture','start',project,'0',optionsPath,'--request-id','unknown-old-boot','--engine-epoch',initial.engine_epoch],false);
  const changed=path.join(root,'changed.json');fs.writeFileSync(changed,JSON.stringify({...options,lease_ms:20000}),{mode:0o600});
  cli(['live-capture','start',project,'0',changed,'--request-id','replay-start','--engine-epoch',initial.engine_epoch],false);
  cli(['live-capture','start',project,'0',optionsPath,'--engine-epoch',initial.engine_epoch],false);
  cli(['live-capture','status',project,'0',derived.identity.epoch],false);
  cli(['live-capture','push',project,'0',derived.identity.epoch,header,pixels],false);
  for(const key of ['groups','retired','staging'])assert.deepEqual(roster()[key],{});
  const fresh=cli(['live-capture','start',project,'0',optionsPath,'--request-id','fresh-after-replay']).value;
  assert.equal(fresh.engine_epoch,limits.engine_epoch);assert.notEqual(fresh.identity.epoch,derived.identity.epoch);assert.notEqual(fresh.identity.source,derived.identity.source);
  await running(fresh.identity.epoch);
  let terminal;
  if(provider==='cuda') {
    for(let frame=0;frame<2;frame++) {
      const bytes=Buffer.alloc(12288,frame*64);fs.writeFileSync(pixels,bytes,{mode:0o600});
      const current=cli(['live-capture','status',project,'0',fresh.identity.epoch]).value;
      fs.writeFileSync(header,JSON.stringify({frame,source_time:{numerator:frame,denominator:10},capture_tick:current.server_elapsed,sha256:crypto.createHash('sha256').update(bytes).digest('hex')}),{mode:0o600});
      cli(['live-capture','push',project,'0',fresh.identity.epoch,header,pixels]);
    }
    await waitFor(()=>{const value=cli(['live-capture','status',project,'0',fresh.identity.epoch]).value;assert.equal(value.active,true,JSON.stringify(value.inference));return value.inference.completed===1;},'fresh CUDA inference did not complete');
    cli(['live-capture','finish',project,'0',fresh.identity.epoch,'--request-id','fresh-finish']);
    terminal=await waitFor(()=>{const value=cli(['live-capture','status',project,'0',fresh.identity.epoch]).value;return value.active?false:value;},'fresh CUDA Finish did not retire');
    assert.equal(terminal.retirement,'completed',JSON.stringify(terminal.inference));assert.equal(terminal.inference.cuda_evidence.summary.successful_runs,1);assert.equal(terminal.inference.cuda_evidence.qualified,false);
  }else {
    terminal=cli(['live-capture','stop',project,'0',fresh.identity.epoch,'--request-id','fresh-stop']).value;assert.equal(terminal.retirement,'stopped');
  }
  await waitFor(()=>['groups','retired','staging'].every(key=>Object.keys(roster()[key]).length===0),'fresh worker custody did not retire');
  fs.writeFileSync(path.join(root,'replay-receipt.json'),JSON.stringify({signal:killed.signal,before,initial,next,derived,replays,fresh,terminal,original_request_ids_preserved:true,all_three_command_forms:true,recorded_groups_observed_empty_before_recovery:true,no_manual_group_cleanup:true,final_ledger:roster(),provider,prepared,preparedObservations,initialGroups,superseded_custody_waited:prepared,overlapMaximumReservedBytes,authored_model:true,real_media:false,physical_dispatch:false,qualified:false},null,2),{mode:0o600});
}
