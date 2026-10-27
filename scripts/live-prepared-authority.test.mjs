// Authored native capture authority proof; local IPC/CUDA, no device dispatch.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import net from 'node:net';
import {spawn,spawnSync} from 'node:child_process';
const worker=process.env.PULSAR_CUDA_CLI_WORKER,runtime=process.env.PULSAR_CUDA_CLI_RUNTIME;
const model=process.env.PULSAR_CUDA_CLI_MODEL,profileFile=process.env.PULSAR_CUDA_CLI_PROFILE;
const ownerFile=process.env.PULSAR_CUDA_CLI_POLICY;
const provider=process.env.PULSAR_LIVE_AUTHORITY_PROVIDER??'cuda';
const hash=b=>crypto.createHash('sha256').update(b).digest('hex');
// One bounded JSON reply per connection; credentials never enter retained logs.
function exchange(endpoint,value) {
  return new Promise((resolve,reject)=>{
    const bytes=Buffer.from(JSON.stringify(value)),header=Buffer.alloc(4);header.writeUInt32BE(bytes.length);
    const socket=net.createConnection(endpoint);let buffer=Buffer.alloc(0),settled=false;
    const done=(error,value)=>{if(settled)return;settled=true;socket.destroy();error?reject(error):resolve(value);};
    socket.setTimeout(15000,()=>done(new Error('authority IPC deadline')));
    socket.once('error',error=>done(error));socket.once('close',()=>done(new Error('authority IPC closed before reply')));
    socket.once('connect',()=>socket.write(Buffer.concat([header,bytes])));
    socket.on('data',bytes=>{buffer=Buffer.concat([buffer,bytes]);
      if(buffer.length>1048580)return done(new Error('authority reply byte limit'));
      if(buffer.length<4)return;const n=buffer.readUInt32BE();
      if(n>1048576)return done(new Error('authority reply frame limit'));
      if(buffer.length>=n+4){try{done(null,JSON.parse(buffer.subarray(4,n+4)));}catch(error){done(error);}}
    });
  });
}
test(`native ${provider} prepared captures remain actor-owned and revoked authority cannot rearm`,
  {skip:!worker||!runtime||!model||(provider==='cuda'&&(!profileFile||!ownerFile))||process.platform!=='linux',timeout:120000},async t=>{
  const root=fs.mkdtempSync(path.join(os.tmpdir(),'pa-'));fs.chmodSync(root,0o700);
  t.diagnostic(`authored native authority evidence: ${root}`);
  assert.ok(['cpu','cuda'].includes(provider));
  const state=path.join(root,'s'),ownerBytes=ownerFile?fs.readFileSync(ownerFile):null;
  const policy=ownerBytes?JSON.parse(ownerBytes):null;
  if(provider==='cuda'){assert.equal(policy.accounting.maximum_reserved_bytes,128*2**20);policy.accounting.maximum_reserved_bytes=256*2**20;}
  const owner=path.join(root,'owner.json');if(provider==='cuda')fs.writeFileSync(owner,JSON.stringify(policy),{mode:0o600});
  const env={...process.env,PULSAR_STATE_DIR:state,PULSAR_ORT_DYLIB:runtime};delete env.PULSAR_CUDA_OWNER_POLICY;
  if(provider==='cuda')env.PULSAR_CUDA_OWNER_POLICY=owner;
  const fd=fs.openSync(path.join(root,'engine.log'),'wx',0o600),engine=spawn(worker,['engine'],{env,stdio:['ignore','ignore',fd]});fs.closeSync(fd);
  let exited,sequence=0;const commands=[],captures=[],observations=[];
  const closed=new Promise(resolve=>{engine.once('error',error=>{exited={error:error.message};resolve();});engine.once('exit',(code,signal)=>{exited={code,signal};resolve();});});
  const cli=(args,success=true)=>{const r=spawnSync(worker,args,{env,encoding:'utf8',timeout:15000,maxBuffer:4<<20});
    commands.push({transport:'cli',args,status:r.status,stderr:r.stderr,stdout:r.stdout,error:r.error?.message});assert.ifError(r.error);
    if(!success)return r;assert.equal(r.status,0,r.stderr);const parsed=JSON.parse(r.stdout);return args[0]==='model'&&args[1]==='profile'?parsed:parsed.value;};
  const until=async(observe,message,ms=15000)=>{const end=Date.now()+ms;for(;;){assert.equal(exited,undefined,'owned Engine exited');
    const value=await observe();if(value)return value;assert.ok(Date.now()<end,message);await new Promise(resolve=>setTimeout(resolve,250));}};
  const empty=()=>{const f=JSON.parse(fs.readFileSync(path.join(state,'worker-fence.json')));
    return ['groups','retired','staging'].every(k=>Object.keys(f[k]).length===0)&&Object.keys(f.creation.records).length===0;};
  t.after(async()=>{try{if(!exited){for(const capture of captures)cli(['live-capture','stop',capture.identity.project,'0',capture.identity.epoch],false);
    if(fs.existsSync(path.join(state,'worker-fence.json')))await until(empty,'owned authority worker retirement');}}
    finally{if(!exited)engine.kill('SIGTERM');
      let timer;await Promise.race([closed,new Promise(resolve=>{timer=setTimeout(resolve,5000);})]);clearTimeout(timer);
      if(!exited){engine.kill('SIGKILL');await closed;}
      fs.writeFileSync(path.join(root,'commands.json'),JSON.stringify(commands,null,2),{mode:0o600});
      fs.writeFileSync(path.join(root,'exit.json'),JSON.stringify({pid:engine.pid,...exited}),{mode:0o600});}});
  await until(()=>fs.existsSync(path.join(state,'engine.sock'))&&cli(['capabilities'],false).status===0,'startup',45000);
  const profile=provider==='cuda'?JSON.parse(fs.readFileSync(profileFile)):cli(['model','profile',model,'--width','64','--height','64','--class-count','1','--temporal-script-position']);
  assert.equal(profile.provider,provider);assert.equal(hash(fs.readFileSync(model)),profile.model.sha256);assert.equal(hash(fs.readFileSync(runtime)),profile.runtime.sha256);
  const ownerCredentials=JSON.parse(fs.readFileSync(path.join(state,'first-party.session.json')));
  const rpc=async(credentials,operation,args,project=null)=>{
    const response=await exchange(path.join(state,'engine.sock'),{version:2,request_id:`authority-${++sequence}`,...credentials,
      project,expected_revision:project===null?null:0,command:{operation,...(args===undefined?{}:{arguments:args})}});
    commands.push({transport:'control',actor:credentials?.session??'pairing',operation,project,
      result:operation==='pair'?{paired:!!response.result.Ok}:response.result});return response.result;};
  const ok=async(...args)=>{const r=await rpc(...args);assert.ok(r.Ok,JSON.stringify(r));return r.Ok;};
  const denied=async(...args)=>{const r=await rpc(...args);assert.equal(r.Err?.code,'forbidden',JSON.stringify(r));};
  const pair=async label=>{const r=await ok({},'pair',{client_name:label,pairing_token:fs.readFileSync(path.join(state,'pairing.token'),'utf8').trim()});return r.value;};
  const foreign=await pair('authored foreign capture client'),admin=await pair('authored grant administrator');
  const scopes=['read','generate','import_source'];
  const grant=(credentials,actor,project,scopes)=>ok(credentials,'grant',{session:actor.session,project_id:project,scopes});
  const options={policy:{maximum_analysis_age_ns:1000000000,added_buffer_ns:0},source_anchor:{numerator:0,denominator:1},
    project_anchor:0,presentation_anchor:0,lease_ms:30000,inference:{model_path:model,profile}};
  const prepared={...options,continuation_mode:'when_ready',source_anchor:{numerator:1,denominator:1},project_anchor:1000000000};
  const startFile=path.join(root,'start.json'),nextFile=path.join(root,'prepared.json');
  fs.writeFileSync(startFile,JSON.stringify(options),{mode:0o600});fs.writeFileSync(nextFile,JSON.stringify(prepared),{mode:0o600});
  const status=c=>cli(['live-capture','status',c.identity.project,'0',c.identity.epoch]);
  const start=project=>{const c=cli(['live-capture','start',project,'0',startFile]);captures.push(c);return c;};
  const running=async c=>until(()=>{const r=status(c);assert.equal(r.active,true,JSON.stringify(r));return r.inference.state==='running'?r:false;},'native capture Ready',29000);
  const prepare=prior=>{const c=cli(['live-capture','continue',prior.identity.project,'0',prior.identity.epoch,nextFile]);captures.push(c);assert.equal(c.inference.state,'starting');return c;};
  const provePair=async c=>{
    for(let frame=0;frame<2;frame++){
      const bytes=Buffer.alloc(12288,frame*64),pixels=path.join(root,'pixels.rgb'),header=path.join(root,'frame.json');
      fs.writeFileSync(pixels,bytes,{mode:0o600});fs.writeFileSync(header,JSON.stringify({frame,source_time:{numerator:30+frame,denominator:30},capture_tick:status(c).server_elapsed,sha256:hash(bytes)}),{mode:0o600});
      cli(['live-capture','push',c.identity.project,'0',c.identity.epoch,header,pixels]);}
    const r=await until(()=>{const r=status(c);return r.inference.completed===1?r:false;},'authored native output');
    assert.equal(r.inference.stale,0);assert.ok(Math.abs(r.inference.latest.result.payload.position-(.25+.5*64/510))<1e-4);return r;
  };
  const project=cli(['project','create','authored-native-actor']).project_id;
  await grant(ownerCredentials,foreign,project,scopes);await ok(foreign,'live_capture_limits',undefined,project);
  const prior=start(project);await running(prior);const next=prepare(prior);
  const other=cli(['project','create','authored-other-project']).project_id;
  await denied(ownerCredentials,'continue_live_capture',{engine_epoch:prior.engine_epoch,predecessor:prior.identity.epoch,options:prepared},other);
  await denied(foreign,'continue_live_capture',{engine_epoch:prior.engine_epoch,predecessor:prior.identity.epoch,options:prepared},project);
  for(const c of [prior,next]){
    for(const operation of ['live_capture_status','stop_live_capture','finish_live_capture'])await denied(foreign,operation,{epoch:c.identity.epoch},project);
    await denied(foreign,'continue_live_capture_at',{engine_epoch:c.engine_epoch,predecessor:c.identity.epoch,source_anchor:{numerator:2,denominator:1},lease_ms:30000},project);
    const bulk=await exchange(path.join(state,'bulk.sock'),{version:2,channel:'live_capture',...foreign,engine_epoch:c.engine_epoch,identity:c.identity});
    assert.equal(bulk.status,'error');assert.equal(bulk.payload.code,'forbidden');commands.push({transport:'bulk',actor:foreign.session,epoch:c.identity.epoch,result:bulk});
  }
  observations.push({phase:'foreign-refusals',prior:status(prior),next:status(next)});
  await running(next);assert.equal(status(prior).retirement,'superseded');const output=await provePair(next);
  if(provider==='cpu'){const refusal=cli(['live-capture','finish',project,'0',next.identity.epoch],false);
    assert.equal(refusal.status,1);assert.match(refusal.stderr,/Unsupported: Finish requires CUDA inference/);
    cli(['live-capture','stop',project,'0',next.identity.epoch]);}
  else cli(['live-capture','finish',project,'0',next.identity.epoch]);
  const finished=await until(()=>{const r=status(next);return r.active?false:r;},'owned Finish');
  assert.equal(finished.retirement,provider==='cuda'?'completed':'stopped');assert.equal(finished.inference.completed,1);
  if(provider==='cuda')assert.equal(finished.inference.cuda_evidence.summary.successful_runs,1);
  await until(empty,'first scenario retirement');
  const revokedProject=cli(['project','create','authored-native-revocation']).project_id;
  await grant(ownerCredentials,admin,revokedProject,[...scopes,'manage_grants']);
  const revokedPrior=start(revokedProject);await running(revokedPrior);const revokedNext=prepare(revokedPrior);
  const pending=status(revokedNext);assert.equal(pending.inference.state,'starting');assert.equal(status(revokedPrior).active,true);
  await ok(admin,'revoke',{session:ownerCredentials.session,project_id:revokedProject});
  await grant(admin,ownerCredentials,revokedProject,scopes);
  await until(empty,'revoked native workers retirement');
  const revoked=[status(revokedPrior),status(revokedNext)];for(const r of revoked)assert.equal(r.active,false);
  assert.equal(revoked[1].inference.completed,0);observations.push({phase:'restored-after-revocation',pending,revoked});
  if(ownerBytes)assert.equal(hash(fs.readFileSync(ownerFile)),hash(ownerBytes));
  fs.writeFileSync(path.join(root,'result.json'),JSON.stringify({status:'PASS',qualified:false,provider,profile,binary:{path:worker,sha256:hash(fs.readFileSync(worker))},
    prior,next,output,finished,revokedPrior,revokedNext,observations,physical_dispatch:false,real_media:false},null,2),{mode:0o600});
});
