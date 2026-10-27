// Real local CUDA image-lifetime proof; authored RGB only, no physical dispatch.
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
const ownerFile=process.env.PULSAR_CUDA_CLI_POLICY;
const digest=b=>crypto.createHash('sha256').update(b).digest('hex');
const delay=ms=>new Promise(r=>setTimeout(r,ms));
test('overlapping CUDA captures retain one frozen image until the last worker retires',
 {skip:!worker||!runtime||!model||!profileFile||!ownerFile||process.platform!=='linux',timeout:90000},async t=>{
  const root=fs.mkdtempSync(path.join(os.tmpdir(),'pulsar-nreuse-'));fs.chmodSync(root,0o700);
  t.diagnostic(`native image reuse evidence: ${root}`);
  const state=path.join(root,'state'),original=fs.readFileSync(ownerFile),policy=JSON.parse(original);
  const profile=JSON.parse(fs.readFileSync(profileFile)),captures=[],commands=[],observations=[];
  assert.equal(policy.accounting.maximum_reserved_bytes,128*2**20);
  policy.accounting.maximum_reserved_bytes=256*2**20;
  const policyPath=path.join(root,'policy.json');fs.writeFileSync(policyPath,JSON.stringify(policy),{mode:0o600});
  assert.equal(profile.provider,'cuda');assert.equal(digest(fs.readFileSync(model)),profile.model.sha256);
  assert.equal(digest(fs.readFileSync(runtime)),profile.runtime.sha256);
  const env={...process.env,PULSAR_STATE_DIR:state,PULSAR_ORT_DYLIB:runtime,PULSAR_CUDA_OWNER_POLICY:policyPath};
  const fd=fs.openSync(path.join(root,'engine.log'),'wx',0o600);
  const engine=spawn(worker,['engine'],{env,stdio:['ignore','ignore',fd]});fs.closeSync(fd);
  let exited;const closed=new Promise(resolve=>engine.once('exit',(code,signal)=>{exited={code,signal};resolve();}));
  const cli=(args,required=true)=>{const begin=performance.now(),r=spawnSync(worker,args,{env,encoding:'utf8',timeout:15000,maxBuffer:4<<20});
    commands.push({args,status:r.status,elapsed_ms:performance.now()-begin});assert.ifError(r.error);
    if(!required)return r;assert.equal(r.status,0,r.stderr);return JSON.parse(r.stdout).value;};
  const until=async(fn,label,ms=10000)=>{const end=performance.now()+ms;
    for(;;){assert.equal(exited,undefined,'owned Engine exited');const value=fn();if(value)return value;
      assert.ok(performance.now()<end,label);await delay(250);}};
  const fence=()=>JSON.parse(fs.readFileSync(path.join(state,'worker-fence.json')));
  const empty=()=>{const f=fence();return ['groups','retired','staging'].every(k=>Object.keys(f[k]).length===0)&&Object.keys(f.creation.records).length===0;};
  const status=c=>cli(['live-capture','status',c.identity.project,'0',c.identity.epoch]);
  // Only inspect owned state, bounded directory depth, never traverse symlinks.
  const images=()=>{const found=[];const walk=(dir,depth)=>{
    if(!fs.existsSync(dir))return;
    for(const item of fs.readdirSync(dir,{withFileTypes:true})){
      if(!item.isDirectory())continue;const child=path.join(dir,item.name);
      if(item.name.startsWith('native-dependencies-')){
        const libraries=path.join(child,'libraries');if(!fs.existsSync(libraries))continue;
        found.push({root:child,libraries:fs.readdirSync(libraries).sort().map(name=>{
          const stat=fs.lstatSync(path.join(libraries,name));assert.equal(stat.isFile(),true);
          return {name,device:stat.dev,inode:stat.ino,bytes:stat.size,mode:stat.mode};})});
      }else if(depth<3)walk(child,depth+1);
    }};walk(state,0);return found.sort((a,b)=>a.root.localeCompare(b.root));};
  let lastProcessSample=0;
  const processSample=()=>{
    const now=performance.now();if(now-lastProcessSample<1000)return null;lastProcessSample=now;
    const listing=spawnSync('ps',['-eo','pid=,ppid='],{encoding:'utf8',timeout:2000,maxBuffer:1<<20});
    if(listing.error||listing.status!==0)return {error:'process inventory unavailable'};
    const entries=listing.stdout.trim().split('\n').map(line=>line.trim().split(/\s+/).map(Number));
    const owned=new Set([engine.pid]);let changed=true;
    while(changed){changed=false;for(const [pid,parent] of entries)if(owned.has(parent)&&!owned.has(pid)){owned.add(pid);changed=true;}}
    const roles=new Set(['native-capture-worker','cuda-live-memory-observer','cuda-live-memory-observer-exec',
      'cuda-live-worker','cuda-live-worker-exec','cuda-live-mount-check','engine']);
    return [...owned].map(pid=>{try{
      const argv=fs.readFileSync(`/proc/${pid}/cmdline`).toString().split('\0');
      const role=roles.has(argv[1])?argv[1]:null;
      const io=Object.fromEntries(fs.readFileSync(`/proc/${pid}/io`,'utf8').trim().split('\n').map(line=>{
        const [key,value]=line.split(':');return [key,Number(value.trim())];}));
      const membership=fs.readFileSync(`/proc/${pid}/cgroup`,'utf8').trim();
      const relative=membership.split('\n').find(line=>line.startsWith('0::'))?.slice(3);
      const groups=[];
      if(relative!==undefined){let directory=path.resolve('/sys/fs/cgroup',`.${relative}`);
        for(let depth=0;depth<8&&directory.startsWith('/sys/fs/cgroup');depth++){
          const values={};for(const name of ['memory.max','memory.high','memory.current','memory.events','memory.pressure',
            'memory.stat','io.max','io.stat','io.pressure','cpu.max','cpu.stat']){
            try{values[name]=fs.readFileSync(path.join(directory,name),'utf8').slice(0,16384).trim();}catch{values[name]=null;}}
          groups.push({path:directory,values});if(directory==='/sys/fs/cgroup')break;directory=path.dirname(directory);
        }}
      return {pid,role,io,cgroups:groups,wchan:fs.readFileSync(`/proc/${pid}/wchan`,'utf8').trim()};
    }catch{return {pid,retired_during_observation:true};}});
  };
  t.after(async()=>{try{if(!exited){for(const c of captures)cli(['live-capture','stop',c.identity.project,'0',c.identity.epoch],false);
      if(fs.existsSync(path.join(state,'worker-fence.json')))await until(empty,'owned final custody');}}
    finally{if(!exited)engine.kill('SIGTERM');let timer;await Promise.race([closed,new Promise(resolve=>{timer=setTimeout(resolve,5000);})]);clearTimeout(timer);
      if(!exited){engine.kill('SIGKILL');await closed;}
      fs.writeFileSync(path.join(root,'commands.json'),JSON.stringify(commands,null,2),{mode:0o600});
      fs.writeFileSync(path.join(root,'observations.json'),JSON.stringify(observations,null,2),{mode:0o600});
      fs.writeFileSync(path.join(root,'exit.json'),JSON.stringify({pid:engine.pid,...exited}),{mode:0o600});}});
  await until(()=>fs.existsSync(path.join(state,'engine.sock'))&&cli(['capabilities'],false).status===0,'startup',45000);
  const project=cli(['project','create','authored-shared-native-image']).project_id;
  const options={policy:{maximum_analysis_age_ns:1000000000,added_buffer_ns:0},source_anchor:{numerator:0,denominator:1},
    project_anchor:0,presentation_anchor:0,lease_ms:30000,inference:{model_path:model,profile}};
  const startFile=path.join(root,'start.json'),nextFile=path.join(root,'prepared.json');
  fs.writeFileSync(startFile,JSON.stringify(options),{mode:0o600});
  fs.writeFileSync(nextFile,JSON.stringify({...options,continuation_mode:'when_ready',source_anchor:{numerator:1,denominator:1},project_anchor:1000000000}),{mode:0o600});
  const firstBegin=performance.now(),prior=cli(['live-capture','start',project,'0',startFile]);captures.push(prior);
  const running=async c=>until(()=>{const r=status(c);
    const observedImages=images();
    observations.push({phase:'Ready-poll',elapsed_ms:performance.now()-firstBegin,epoch:c.identity.epoch,state:r.inference.state,active:r.active,retirement:r.retirement,error:r.inference.error,
      images:observedImages.map(i=>{const unique=new Map(i.libraries.map(l=>[`${l.device}:${l.inode}`,l.bytes]));
        return {root:i.root,aliases:i.libraries.length,unique_libraries:unique.size,bytes:[...unique.values()].reduce((a,b)=>a+b,0)};}),
      processes:processSample()});
    assert.equal(r.active,true,JSON.stringify(r.inference));
    return r.inference.state==='running'?r:false;},'capture Ready',31000);
  await running(prior);const firstReadyMs=performance.now()-firstBegin,before=images();
  assert.equal(before.length,1,'one native image after first Ready');
  const names=policy.native_dependencies.libraries.flatMap(lib=>lib.names).sort();
  assert.deepEqual(before[0].libraries.map(l=>l.name),names);
  const prepareBegin=performance.now(),next=cli(['live-capture','continue',project,'0',prior.identity.epoch,nextFile]);captures.push(next);
  assert.equal(next.inference.state,'starting');assert.equal(status(prior).active,true);
  const ready=await running(next),preparationMs=performance.now()-prepareBegin;
  await until(()=>{const r=status(prior);return !r.active&&r.retirement==='superseded';},'prior retirement');
  // Confirm tree retirement while the shared image's staging obligation survives.
  await until(()=>{const f=fence();return Object.keys(f.groups).length===1&&Object.keys(f.retired).length===0;},'prior child tree retirement');
  const after=images();assert.deepEqual(after,before,'same captured inode set survives handoff');
  assert.ok(Object.keys(fence().creation.records).length>0,'shared staging obligation retained');
  const pixels=path.join(root,'pixels.rgb'),header=path.join(root,'frame.json'),uploads=[];
  for(let frame=0;frame<2;frame++){
    const bytes=Buffer.alloc(12288,frame*64);fs.writeFileSync(pixels,bytes,{mode:0o600});
    const value={frame,source_time:{numerator:30+frame,denominator:30},capture_tick:status(next).server_elapsed,sha256:digest(bytes)};
    fs.writeFileSync(header,JSON.stringify(value),{mode:0o600});cli(['live-capture','push',project,'0',next.identity.epoch,header,pixels]);uploads.push(value);}
  const output=await until(()=>{const r=status(next);return r.inference.completed===1?r:false;},'successor CUDA output');
  assert.equal(output.inference.stale,0);assert.equal(output.inference.latest.result.status,'cuda_provisional');
  const payload=output.inference.latest.result.payload;
  assert.deepEqual(payload.identity,next.identity);
  assert.equal(payload.input.sequence,1);
  assert.equal(payload.input.pair.previous_frame,0);assert.equal(payload.input.pair.current_frame,1);
  assert.deepEqual(payload.input.pair.previous_time,uploads[0].source_time);
  assert.deepEqual(payload.input.pair.current_time,uploads[1].source_time);
  assert.equal(payload.input.previous.sha256,uploads[0].sha256);
  assert.equal(payload.input.current.sha256,uploads[1].sha256);
  assert.equal(payload.input.previous.byte_len,12288);assert.equal(payload.input.current.byte_len,12288);
  assert.equal(payload.input.pair.presentation_clock.project_anchor,1000000000);
  assert.deepEqual(payload.input.pair.presentation_clock.source_anchor,{numerator:1,denominator:1});
  assert.ok(Math.abs(payload.position-(.25+.5*64/510))<1e-4);
  cli(['live-capture','finish',project,'0',next.identity.epoch]);
  const terminal=await until(()=>{const r=status(next);return r.active?false:r;},'Finish');
  assert.equal(terminal.retirement,'completed');assert.equal(terminal.inference.cuda_evidence.summary.successful_runs,1);
  await until(empty,'last-use staging cleanup');assert.deepEqual(images(),[]);
  assert.equal(digest(fs.readFileSync(ownerFile)),digest(original));
  fs.writeFileSync(path.join(root,'result.json'),JSON.stringify({status:'PASS',qualified:false,
    binary:{path:worker,sha256:digest(fs.readFileSync(worker))},prior,next,ready,output,terminal,uploads,before,after,
    first_ready_ms:firstReadyMs,preparation_ms:preparationMs,fence:fence(),
    scope:'actual CUDA shared image inode identity and last-use durable cleanup with authored scalar output',
    physical_dispatch:false,real_media:false,capture_to_command_qualified:false},null,2),{mode:0o600});
});
