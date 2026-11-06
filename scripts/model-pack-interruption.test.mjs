// Real isolated engine crashes; authored graphs and inert runtime bytes only.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import {spawn,spawnSync} from 'node:child_process';
import {once} from 'node:events';
import {authoredOnnxFixture} from './benchmark-onnx-fixture.mjs';

const binary=process.env.PULSAR_BENCHMARK_WORKER;
const sha=bytes=>crypto.createHash('sha256').update(bytes).digest('hex');
const delay=ms=>new Promise(resolve=>setTimeout(resolve,ms));
function hashFile(file) {
  const fd=fs.openSync(file,'r'),hash=crypto.createHash('sha256'),buffer=Buffer.alloc(65536);
  try {while(true){const count=fs.readSync(fd,buffer);if(!count)break;hash.update(buffer.subarray(0,count));}} finally {fs.closeSync(fd);}
  return hash.digest('hex');
}
function snapshot(directory) {
  return fs.readdirSync(directory).sort().map(name=>{
    const file=path.join(directory,name),stat=fs.lstatSync(file);assert.ok(stat.isFile());
    return {name,byte_len:stat.size,sha256:hashFile(file)};
  });
}

test('inert pack import survives actual pre-publication and completed-publication engine crashes',
  {skip:!binary || process.platform!=='linux',timeout:90000},async t=>{
  const root=fs.mkdtempSync(path.join(os.tmpdir(),'pulsar-pack-crash-'));
  t.diagnostic(`retained authored pack interruption evidence: ${root}`);
  const state=path.join(root,'state'),env={...process.env,PULSAR_STATE_DIR:state};
  const children=[];let engine,engineExit,index=0;
  const save=(name,value)=>fs.writeFileSync(path.join(root,name),JSON.stringify(value,null,2)+'\n',{mode:0o600});
  const cli=args=>{
    const result=spawnSync(binary,args,{env,encoding:'utf8',timeout:30000,maxBuffer:1048576});assert.equal(result.error,undefined);
    save(`cli-${index++}.json`,{args,status:result.status,signal:result.signal,stdout:result.stdout,stderr:result.stderr});return result;
  };
  const success=args=>{const result=cli(args);assert.equal(result.status,0,result.stderr);const body=JSON.parse(result.stdout).value;
    for(const flag of ['native_execution_approved','rights_verified','compatible','qualified','active'])assert.equal(body[flag],false);
    return result;
  };
  const staging=()=>{const result=cli(['model','pack-staging-status']);assert.equal(result.status,0,result.stderr);const report=JSON.parse(result.stdout).value;assert.equal(report.observation_only,true);return report;};
  const start=async()=>{
    const log=fs.openSync(path.join(root,`engine-${crypto.randomUUID()}.log`),'wx',0o600);
    engine=spawn(binary,['engine'],{env,stdio:['ignore','ignore',log]});fs.closeSync(log);children.push(engine);engineExit=once(engine,'exit');
    const deadline=Date.now()+15000;
    while(true){assert.equal(engine.exitCode,null);assert.equal(engine.signalCode,null);assert.ok(Date.now()<deadline,'engine startup deadline');
      if(fs.existsSync(path.join(state,'engine.sock'))){const result=spawnSync(binary,['capabilities'],{env,encoding:'utf8',timeout:1000,maxBuffer:1048576});if(result.status===0)break;}
      await delay(10);
    }
  };
  const kill=async()=>{assert.equal(engine.exitCode,null);assert.equal(engine.signalCode,null);assert.equal(engine.kill('SIGKILL'),true);const [code,signal]=await engineExit;assert.equal(code,null);assert.equal(signal,'SIGKILL');return {code,signal};};
  t.after(async()=>{for(const child of children){if(child.exitCode===null && child.signalCode===null){const ended=once(child,'exit');child.kill('SIGKILL');await ended;}}});
  const makePack=(name,large=false)=>{
    const source=path.join(root,name);fs.mkdirSync(source);
    const model=authoredOnnxFixture(),runtime=Buffer.from('Authored inert runtime, never executed'),license=Buffer.from('Authored test fixture only');
    const identity=(bytes,id)=>({artifact_id:id,sha256:sha(bytes),byte_len:bytes.length});
    const profile={schema_version:1,model:identity(model,'authored-model'),runtime:identity(runtime,'authored-runtime'),preprocessing:{input:{width:64,height:64,channels:3,layout:'nchw',class_count:1,decoder:'yolo_channel_major'},resize:'stretch',color:'rgb',scale:[1/255,1/255,1/255],bias:[0,0,0],padding_byte:0},provider:'cpu',minimum_memory_bytes:4096,minimum_output_bytes:1024,cpu_threads:1};
    const files=[['model.bin','model',model],['runtime.bin','runtime',runtime],['profile.json','profile',Buffer.from(JSON.stringify(profile))],['LICENSE','license',license]];
    const assets=files.map(([name,role,bytes])=>{fs.writeFileSync(path.join(source,name),bytes);return {path:name,role,byte_len:bytes.length,sha256:sha(bytes)};});
    if(large){
      const size=256*1024*1024,fd=fs.openSync(path.join(source,'payload.bin'),'wx',0o600);try{fs.ftruncateSync(fd,size);}finally{fs.closeSync(fd);}
      const hash=crypto.createHash('sha256'),zero=Buffer.alloc(1048576);for(let i=0;i<256;i++)hash.update(zero);
      // First asset creates a wide, observable partial-copy interval without hooks.
      assets.unshift({path:'payload.bin',role:'fixture',byte_len:size,sha256:hash.digest('hex')});
    }
    const manifest={schema_version:1,pack_id:name,target_os:'linux',target_arch:'x86_64',ort_api_version:24,labels:['fixture'],assets,license_path:'LICENSE',source_description:'Authored inert crash test; no activation or external assets'};
    const bytes=Buffer.from(JSON.stringify(manifest));fs.writeFileSync(path.join(source,'manifest.json'),bytes);
    return {source,hash:sha(bytes),manifest};
  };
  const prior=makePack('prior-pack'),next=makePack('successor-pack',true);await start();
  assert.deepEqual(staging().stages,[]);assert.equal(fs.existsSync(path.join(state,'model-packs')),false);
  success(['model','pack-import',prior.source,'--request-id','prior-import']);
  const store=path.join(state,'model-packs'),priorInstalled=path.join(store,prior.hash),priorBefore=snapshot(priorInstalled);
  const args=['model','pack-import',next.source,'--request-id','interrupted-import'];
  const client=spawn(binary,args,{env,stdio:['ignore','pipe','pipe']});children.push(client);
  const clientExit=once(client,'exit');let stdout='',stderr='';
  for(const [stream,key]of [[client.stdout,'stdout'],[client.stderr,'stderr']])stream.on('data',bytes=>{if(key==='stdout')stdout+=bytes.toString();else stderr+=bytes.toString();if(Buffer.byteLength(stdout)+Buffer.byteLength(stderr)>1048576)client.kill('SIGKILL');});
  const deadline=Date.now()+15000;let stage;
  while(true){assert.equal(engine.exitCode,null);assert.equal(engine.signalCode,null);assert.equal(client.exitCode,null);assert.ok(Date.now()<deadline,'partial-copy observation deadline');
    const stages=fs.readdirSync(store).filter(name=>name.startsWith('.pack-stage-'));
    assert.ok(stages.length<=1,'unexpected overlapping staging');
    if(stages.length){const candidate=path.join(store,stages[0]),payload=path.join(candidate,'payload.bin');
      if(fs.existsSync(payload)){const size=fs.statSync(payload).size;if(size>0 && size<256*1024*1024){stage=candidate;assert.equal(engine.kill('SIGSTOP'),true);break;}}
    }
    await delay(1);
  }
  const pauseDeadline=Date.now()+5000;while(!/^State:\s+T/m.test(fs.readFileSync(`/proc/${engine.pid}/status`,'utf8'))){assert.ok(Date.now()<pauseDeadline);await delay(1);}
  // These facts are checked after the owned engine is actually stopped.
  assert.equal(fs.existsSync(path.join(store,next.hash)),false);
  const partial=snapshot(stage);assert.ok(partial.some(f=>f.name==='payload.bin' && f.byte_len>0 && f.byte_len<256*1024*1024));
  assert.deepEqual(snapshot(priorInstalled),priorBefore);
  const beforePublication={engine_pid:engine.pid,observed_process_state:'T',stage_basename:path.basename(stage),files:partial,successor_published:false,prior_manifest_sha256:prior.hash,prior_files:priorBefore};
  const preCrash=await kill();const [clientCode,clientSignal]=await clientExit;assert.notEqual(clientCode,0);
  save(`cli-${index++}.json`,{args,status:clientCode,signal:clientSignal,stdout,stderr});
  await start();success(['model','pack-status',prior.hash]);
  assert.equal(fs.existsSync(path.join(store,next.hash)),false);assert.deepEqual(snapshot(stage),partial);
  const recognized=staging();assert.deepEqual(recognized.stages,[{basename:path.basename(stage),state:'unlocked_recognized',manifest_sha256:next.hash}]);
  // A separately owned process holds the same persistent descriptor lock.
  const marker=path.join(stage,'.pulsar-stage.json');
  const holder=spawn('/usr/bin/python3',['-c','import fcntl,sys; f=open(sys.argv[1],"rb"); fcntl.flock(f,fcntl.LOCK_EX); print("locked",flush=True); sys.stdin.buffer.read(1)',marker],{stdio:['pipe','pipe','pipe']});children.push(holder);
  const holderExit=once(holder,'exit'),timer=setTimeout(()=>holder.kill('SIGKILL'),5000);
  try {const [message]=await once(holder.stdout,'data');assert.equal(message.toString().trim(),'locked');
    assert.deepEqual(staging().stages,[{basename:path.basename(stage),state:'locked',manifest_sha256:null}]);
    assert.notEqual(cli(['model','pack-discard-stage',path.basename(stage),next.hash,'--request-id','discard-locked']).status,0);assert.deepEqual(snapshot(stage),partial);
  } finally {holder.stdin.end();clearTimeout(timer);}
  const [holderCode,holderSignal]=await holderExit;assert.equal(holderCode,0);assert.equal(holderSignal,null);
  const legacy=path.join(store,'.pack-stage-legacy'),alias=path.join(store,'.pack-stage-alias');fs.mkdirSync(legacy,{mode:0o700});fs.symlinkSync(priorInstalled,alias);
  const unrecognized=staging();assert.equal(unrecognized.stages.find(s=>s.basename==='.pack-stage-legacy').state,'unrecognized');assert.equal(unrecognized.stages.find(s=>s.basename==='.pack-stage-alias').state,'unrecognized');
  const markerBytes=fs.readFileSync(marker);fs.writeFileSync(marker,JSON.stringify({schema_version:1,manifest_sha256:'0'.repeat(64)}));
  assert.equal(staging().stages.find(s=>s.basename===path.basename(stage)).state,'unrecognized');fs.writeFileSync(marker,markerBytes);
  assert.deepEqual(snapshot(stage),partial);assert.deepEqual(snapshot(priorInstalled),priorBefore);assert.ok(fs.lstatSync(alias).isSymbolicLink());
  assert.notEqual(cli(['model','pack-status',next.hash]).status,0);
  const retried=success(args),successorInstalled=path.join(store,next.hash);
  const successorFiles=snapshot(successorInstalled);
  for(const asset of next.manifest.assets){const item=successorFiles.find(f=>f.name===asset.path);assert.equal(item.byte_len,asset.byte_len);assert.equal(item.sha256,asset.sha256);}
  assert.deepEqual(snapshot(priorInstalled),priorBefore);assert.deepEqual(snapshot(stage),partial);
  const postCrash=await kill();await start();success(['model','pack-status',prior.hash]);success(['model','pack-status',next.hash]);
  assert.deepEqual(snapshot(priorInstalled),priorBefore);assert.deepEqual(snapshot(successorInstalled),successorFiles);assert.deepEqual(snapshot(stage),partial);
  const fd=fs.openSync(path.join(next.source,'payload.bin'),'r+');try{fs.writeSync(fd,Buffer.from([1]),0,1,0);}finally{fs.closeSync(fd);}
  const replay=success(args);assert.deepEqual(JSON.parse(replay.stdout),JSON.parse(retried.stdout));
  assert.deepEqual(snapshot(successorInstalled),successorFiles);
  const discardArgs=['model','pack-discard-stage',path.basename(stage),next.hash,'--request-id','discard-partial'];
  const rejectDiscard=(id,basename=path.basename(stage),hash=next.hash)=>{assert.notEqual(cli(['model','pack-discard-stage',basename,hash,'--request-id',id]).status,0);assert.deepEqual(snapshot(stage),partial);};
  rejectDiscard('discard-wrong-hash',path.basename(stage),'0'.repeat(64));
  rejectDiscard('discard-published',prior.hash,prior.hash);
  rejectDiscard('discard-legacy','.pack-stage-legacy');rejectDiscard('discard-alias','.pack-stage-alias');
  fs.writeFileSync(path.join(stage,'unlisted'),'preserve me',{mode:0o600});
  assert.notEqual(cli(['model','pack-discard-stage',path.basename(stage),next.hash,'--request-id','discard-unlisted']).status,0);
  assert.equal(fs.readFileSync(path.join(stage,'unlisted'),'utf8'),'preserve me');fs.unlinkSync(path.join(stage,'unlisted'));
  fs.symlinkSync(path.join(priorInstalled,'model.bin'),path.join(stage,'model.bin'));
  assert.notEqual(cli(['model','pack-discard-stage',path.basename(stage),next.hash,'--request-id','discard-symlink']).status,0);assert.ok(fs.lstatSync(path.join(stage,'model.bin')).isSymbolicLink());fs.unlinkSync(path.join(stage,'model.bin'));
  fs.linkSync(path.join(priorInstalled,'model.bin'),path.join(stage,'model.bin'));
  assert.notEqual(cli(['model','pack-discard-stage',path.basename(stage),next.hash,'--request-id','discard-hardlink']).status,0);fs.unlinkSync(path.join(stage,'model.bin'));
  const removed=cli(discardArgs);assert.equal(removed.status,0,removed.stderr);const discard=JSON.parse(removed.stdout).value;
  assert.equal(discard.outcome,'removed');assert.equal(discard.native_execution_or_activation,false);assert.equal(discard.planned_file_count,partial.length);assert.equal(discard.planned_bytes,partial.reduce((sum,f)=>sum+f.byte_len,0));
  assert.equal(fs.existsSync(stage),false);assert.deepEqual(snapshot(priorInstalled),priorBefore);assert.deepEqual(snapshot(successorInstalled),successorFiles);assert.ok(fs.lstatSync(alias).isSymbolicLink());assert.ok(fs.statSync(legacy).isDirectory());
  const removedReplay=cli(discardArgs);assert.equal(removedReplay.status,0);assert.deepEqual(JSON.parse(removedReplay.stdout),JSON.parse(removed.stdout));
  assert.notEqual(cli(['model','pack-discard-stage','.pack-stage-legacy',next.hash,'--request-id','discard-partial']).status,0);
  await kill();await start();const restartedDiscard=cli(discardArgs);assert.equal(restartedDiscard.status,0,restartedDiscard.stderr);assert.deepEqual(JSON.parse(restartedDiscard.stdout),JSON.parse(removed.stdout));
  assert.deepEqual(snapshot(priorInstalled),priorBefore);assert.deepEqual(snapshot(successorInstalled),successorFiles);
  save('discard.json',{schema_version:1,state:'passed',receipt:discard,replay_and_restart_identical:true,published_assets_preserved:true,legacy_and_alias_preserved:true,refusals:['wrong hash','published basename','legacy','alias','unlisted file','symlink asset','hardlinked asset'],native_pack_execution:false,qualification:'unqualified'});
  save('interruption.json',{schema_version:1,state:'passed',prepublication:{...beforePublication,termination:preCrash,client_exit_code:clientCode,client_exit_signal:clientSignal},completed_publication:{termination:postCrash,manifest_sha256:next.hash,files:successorFiles},orphan_preserved_until_explicit_discard:true,retry_same_intent_succeeded:true,replay_after_supplier_corruption_identical:true,prior_bytes_unchanged:true,native_pack_execution:false,activation:false,qualification:'unqualified',limitations:['Observed two concrete import process-crash points; not exhaustive discard/power-loss/disk-full/receipt-window proof.','Only an explicit hash-bound recognized stage was discarded; legacy and unsafe entries remain.','No active-selection update/rollback, publisher trust, rights, runtime/model/device/platform/release qualification.']});
});
