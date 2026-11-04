// Isolated real-engine configuration and CLI intent refusals. No GPU execution.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
import {spawnSync,spawn} from 'node:child_process';
import {once} from 'node:events';
import {authoredOnnxFixture} from '../../../scripts/benchmark-onnx-fixture.mjs';
const [inspectorArg,workerArg,specArg,kernelArg,runtimeArg,rootArg]=process.argv.slice(2);
assert.equal(process.argv.length,8,'INSPECTOR WORKER SPEC KERNEL EXPECTED-RUNTIME NEW-ROOT');
const inspector=fs.realpathSync(inspectorArg),worker=fs.realpathSync(workerArg),runtime=fs.realpathSync(runtimeArg);
const spec=JSON.parse(fs.readFileSync(specArg)),device=JSON.parse(fs.readFileSync(kernelArg));
fs.mkdirSync(rootArg,{mode:0o700});const root=fs.realpathSync(rootArg);
const accounting={device_uuid:device.device_uuid,device_ordinal:0,maximum_reserved_bytes:2*2**30,interactive_reserved_bytes:2**30,control_reserve_bytes:256*2**20};
const policy={schema_version:1,device,accounting,native_dependencies:spec};
const policyPath=path.join(root,'owner.json');fs.writeFileSync(policyPath,JSON.stringify(policy,null,2)+'\n',{mode:0o600,flag:'wx'});
const hash=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const before=Object.fromEntries([inspector,worker,runtime,policyPath].map(p=>[p,hash(p)]));
const env={...process.env,PULSAR_ORT_DYLIB:runtime};delete env.PULSAR_CUDA_OWNER_POLICY;
const cases=[];
function inspect(name,input,pass,pattern) {
  const state=path.join(root,`inspect-${name}`);
  const result=spawnSync(inspector,[input,state,worker],{env,encoding:'utf8',timeout:30_000,maxBuffer:1048576});
  fs.writeFileSync(path.join(root,`${name}.stdout`),result.stdout??'');fs.writeFileSync(path.join(root,`${name}.stderr`),result.stderr??'');
  assert.ifError(result.error);assert.equal(result.signal,null);
  if(pass) {assert.equal(result.status,0,result.stderr);const status=JSON.parse(result.stdout);
    assert.equal(status.source_identity.sha256,before[policyPath]);assert.equal(status.execution_enabled,false);
    assert.deepEqual(status.pool_usage,{reserved_bytes:0,offline_reserved_bytes:0});assert.deepEqual(status.accounting,accounting);
    assert.equal(status.native_declared_bytes,spec.libraries.reduce((n,l)=>n+l.byte_len,0));
  } else {assert.notEqual(result.status,0);assert.equal(result.stdout,'');assert.match(result.stderr,pattern);assert(!fs.existsSync(state),'refusal created engine state');}
  cases.push({name,exit_code:result.status,expected:pass?'configured_inert_engine':'refusal_before_state'});
}
inspect('private-policy',policyPath,true);
const wrongRuntime=structuredClone(policy);wrongRuntime.native_dependencies.libraries.find(l=>l.names.includes('libonnxruntime.so.1')).sha256='f'.repeat(64);
for(const [name,payload,pattern] of [
  ['runtime-mismatch',wrongRuntime,/differs from the engine's pinned runtime/],
  ['unknown-field',{...policy,enable_gpu:true},/unknown field/],
  ['missing-role',{...policy,native_dependencies:{...spec,libraries:spec.libraries.filter(l=>!l.names.includes('libcuda.so.1'))}},/lacks required alias/],
  ['wrong-accounting-device',{...policy,accounting:{...accounting,device_uuid:'GPU-00000000-0000-0000-0000-000000000000'}},/UUID differ/]
]) {const file=path.join(root,`${name}.json`);fs.writeFileSync(file,JSON.stringify(payload)+'\n',{mode:0o600,flag:'wx'});inspect(name,file,false,pattern);}
const exposed=path.join(root,'exposed.json');fs.copyFileSync(policyPath,exposed);fs.chmodSync(exposed,0o640);inspect('group-readable',exposed,false,/private current-owner storage/);
const symlink=path.join(root,'symlink.json');fs.symlinkSync(policyPath,symlink);inspect('symlink',symlink,false,/Too many levels|symbolic links/);

const state=path.join(root,'cli-state');const cliEnv={...env,PULSAR_STATE_DIR:state,PULSAR_CUDA_OWNER_POLICY:policyPath};
const log=fs.openSync(path.join(root,'engine.stderr'),'wx',0o600);
const engine=spawn(worker,['engine'],{env:cliEnv,stdio:['ignore','ignore',log]});fs.closeSync(log);
const stopped=once(engine,'exit');let index=0;
function cli(args) {const result=spawnSync(worker,args,{env:cliEnv,encoding:'utf8',timeout:30_000,maxBuffer:1048576});
  assert.ifError(result.error);fs.writeFileSync(path.join(root,`cli-${index++}.json`),JSON.stringify({args,exit_code:result.status,stdout:result.stdout,stderr:result.stderr},null,2)+'\n');return result;}
function counts() {const result=spawnSync('python3',['-c','import sqlite3,json,sys;c=sqlite3.connect("file:"+sys.argv[1]+"?mode=ro",uri=True);print(json.dumps([c.execute("select count(*) from jobs").fetchone()[0],c.execute("select count(*) from attempts").fetchone()[0]]))',path.join(state,'projects.sqlite3')],{encoding:'utf8',timeout:10000});assert.equal(result.status,0,result.stderr);return JSON.parse(result.stdout);}
try {
  const deadline=Date.now()+15000;while(!fs.existsSync(path.join(state,'engine.sock'))) {assert.equal(engine.exitCode,null);assert(Date.now()<deadline);await new Promise(r=>setTimeout(r,25));}
  const catalogResult=cli(['model','status']);assert.equal(catalogResult.status,0,catalogResult.stderr);const catalog=JSON.parse(catalogResult.stdout);
  assert.deepEqual(catalog.providers,['cpu']);assert.match(catalog.limitation,/Owner CUDA accounting policy is configured; GPU job admission remains disabled/);
  const model=path.join(root,'authored.onnx');fs.writeFileSync(model,authoredOnnxFixture(),{flag:'wx'});
  const profileResult=cli(['model','profile',model,'--width','64','--height','64','--class-count','1']);assert.equal(profileResult.status,0,profileResult.stderr);
  const cuda={...JSON.parse(profileResult.stdout),schema_version:2,provider:'cuda',cuda_resources:{device_uuid:device.device_uuid,device_ordinal:0,address_space_bytes:16*2**30,device_reservation_bytes:2**30,arena_limit_bytes:64*2**20}};
  const video=path.join(root,'authored.y4m');fs.writeFileSync(video,Buffer.concat([Buffer.from('YUV4MPEG2 W64 H64 F8:1 Ip A1:1 Cmono\n'),...Array.from({length:8},()=>Buffer.concat([Buffer.from('FRAME\n'),Buffer.alloc(64*64,128)]))]),{flag:'wx'});
  const initial=counts();assert.deepEqual(initial,[0,0]);
  for(const [name,payload,pattern] of [
    ['matching-intent',cuda,/unsupported pinned provider/],
    ['foreign-device',{...cuda,cuda_resources:{...cuda.cuda_resources,device_ordinal:1}},/device differs from owner policy/],
    ['foreign-runtime',{...cuda,runtime:{...cuda.runtime,sha256:'f'.repeat(64)}},/runtime differs from owner policy/],
    ['offline-ceiling',{...cuda,cuda_resources:{...cuda.cuda_resources,device_reservation_bytes:2**30+1}},/exceeds owner priority ceiling/]
  ]) {const input=path.join(root,`${name}.json`),output=path.join(root,`${name}.funscript`);fs.writeFileSync(input,JSON.stringify(payload)+'\n',{flag:'wx'});
    const result=cli(['generate',video,'--model',model,'--inference-profile',input,'--width','64','--height','64','--output',output]);assert.notEqual(result.status,0);assert.match(result.stderr,pattern);assert(!fs.existsSync(output));assert.deepEqual(counts(),initial);
    assert.deepEqual(fs.readdirSync(path.join(state,'attempts')),[],'unsupported intent created an attempt directory');
    cases.push({name,exit_code:result.status,expected:'refusal_before_jobs_and_attempts',attempt_directories:0});
  }
} finally {if(engine.exitCode===null&&engine.signalCode===null) engine.kill('SIGTERM');await stopped;}
const after=Object.fromEntries(Object.keys(before).map(p=>[p,hash(p)]));assert.deepEqual(after,before);
fs.writeFileSync(path.join(root,'checked.json'),JSON.stringify({scope:'real owner-configured engine accounting and inert CUDA intent; no GPU execution',before,after,cases},null,2)+'\n',{flag:'wx'});
