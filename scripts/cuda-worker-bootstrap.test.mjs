// Explicit authored CUDA experiment; no engine GPU admission or device actuation.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import crypto from 'node:crypto';
import {spawn,spawnSync} from 'node:child_process';
import {setTimeout as delay} from 'node:timers/promises';
const hash=p=>{const fd=fs.openSync(p,'r'),digest=crypto.createHash('sha256'),buffer=Buffer.alloc(1048576);try{for(;;){const n=fs.readSync(fd,buffer);if(!n)break;digest.update(buffer.subarray(0,n));}}finally{fs.closeSync(fd);}return digest.digest('hex');};

test('authored CUDA worker generates through verified bootstrap and actual parent artifact egress',
 {skip:!process.env.PULSAR_CUDA_BOOTSTRAP_WORKER||!process.env.PULSAR_CUDA_BOOTSTRAP_INSPECTOR||!process.env.PULSAR_NATIVE_LIBRARY_SPEC||!process.env.PULSAR_CUDA_DEVICE_EXPECTATION,timeout:120000},async t=>{
  const worker=fs.realpathSync(process.env.PULSAR_CUDA_BOOTSTRAP_WORKER),helper=fs.realpathSync(process.env.PULSAR_CUDA_BOOTSTRAP_INSPECTOR);
  const spec=fs.realpathSync(process.env.PULSAR_NATIVE_LIBRARY_SPEC),device=fs.realpathSync(process.env.PULSAR_CUDA_DEVICE_EXPECTATION);
  const model=path.resolve('docs/evidence/roadmap-2026-10-01/cuda-result-egress/native-egress/authored.onnx');
  const subjects=[worker,helper,spec,device,model],before=subjects.map(hash);
  const root=fs.mkdtempSync(path.join(os.tmpdir(),'pulsar-cuda-bootstrap-'));fs.chmodSync(root,0o700);
  const args=[worker,spec,device,model,path.join(root,'native')];let stdout='',stderr='',timedOut=false;
  const child=spawn(helper,args,{stdio:['ignore','pipe','pipe']});
  const timer=setTimeout(()=>{timedOut=true;child.kill('SIGKILL');},110000);
  for(const [stream,key]of [[child.stdout,'stdout'],[child.stderr,'stderr']])stream.on('data',b=>{if(key==='stdout')stdout+=b.toString();else stderr+=b.toString();if(Buffer.byteLength(stdout)+Buffer.byteLength(stderr)>1048576)child.kill('SIGKILL');});
  let result;try{result=await new Promise((resolve,reject)=>{child.once('error',reject);child.once('exit',(code,signal)=>resolve({code,signal}));});}finally{clearTimeout(timer);}
  assert.equal(timedOut,false);assert.equal(result.code,0,stderr);assert.equal(result.signal,null);
  const report=JSON.parse(stdout);assert.equal(report.state,'passed');assert.equal(report.native_execution,true);assert.equal(report.production_gpu_admission,false);assert.equal(report.capture_removed,true);
  assert.deepEqual(report.cases.map(c=>c.case),['baseline','wrong-request','wrong-alias-inode','wrong-device-inode']);
  for(const [i,c]of report.cases.entries()){assert.equal(c.tree_empty,true);assert.equal(c.exit_code,i?1:0);if(i)assert.equal(c.published_artifacts,false);}
  const baseline=report.cases[0];assert.ok(Math.abs(baseline.position-(0.25+0.25*128/255))<1e-5);
  const artifacts=[];
  for(const name of ['program','receipt']){const a=baseline[name];assert.equal(hash(a.path),a.identity.sha256);artifacts.push(a);}
  for(const a of Object.values(baseline.cuda_artifacts)){assert.equal(hash(a.path),a.identity.sha256);artifacts.push(a);}
  const receipt=JSON.parse(fs.readFileSync(baseline.cuda_artifacts.receipt.path));assert.equal(receipt.provider,'cuda');assert.equal(receipt.successful_runs,1);
  assert.equal(receipt.provider_evidence,'strict-cuda-node-trace');
  assert.match(report.cases[1].stderr,/CUDA mount request identity differs/);
  assert.match(report.cases[2].stderr,/mounted native alias differs/);assert.match(report.cases[3].stderr,/mounted CUDA node differs/);
  assert.deepEqual(subjects.map(hash),before);
  if(process.env.PULSAR_CUDA_BOOTSTRAP_EVIDENCE){const out=process.env.PULSAR_CUDA_BOOTSTRAP_EVIDENCE;fs.mkdirSync(out,{mode:0o700});
    for(const a of artifacts)fs.copyFileSync(a.path,path.join(out,path.basename(a.path)),fs.constants.COPYFILE_EXCL);
    fs.copyFileSync(path.join(root,'native','baseline-request.json'),path.join(out,'worker-request.json'),fs.constants.COPYFILE_EXCL);
    fs.writeFileSync(path.join(out,'checked.json'),JSON.stringify({...report,subjects:subjects.map((p,i)=>({path:p,sha256:before[i]})),command:{argv:[helper,...args]}},null,2)+'\n',{flag:'wx',mode:0o600});}
  t.diagnostic(JSON.stringify({root,native_execution:true,production_gpu_admission:false,cases:report.cases.length,actual_parent_egress:true}));
});

test('CUDA bootstrap erases inherited descriptors before waiting for input and default build refuses substitution',
 {skip:!process.env.PULSAR_CUDA_BOOTSTRAP_WORKER||!process.env.PULSAR_STASH_TEST_BINARY,timeout:10000},async t=>{
  const root=fs.mkdtempSync(path.join(os.tmpdir(),'pulsar-cuda-fd-'));const canary=path.join(root,'canary');fs.writeFileSync(canary,'authored canary',{mode:0o600});const fd=fs.openSync(canary,'r+');
  const worker=process.env.PULSAR_CUDA_BOOTSTRAP_WORKER;const env={PATH:'/usr/bin:/bin',PULSAR_WORKER_CONFINED:'1'};
  const child=spawn(worker,['cuda-worker'],{env,stdio:['pipe','pipe','pipe',fd]});let stdout='',stderr='';child.stdout.on('data',b=>stdout+=b);child.stderr.on('data',b=>stderr+=b);
  const ended=new Promise(resolve=>child.once('exit',(code,signal)=>resolve({code,signal})));let finished=false;
  try{
    await delay(150);assert.equal(child.exitCode,null,stderr);assert.equal(child.signalCode,null);
    assert.deepEqual(fs.readdirSync(`/proc/${child.pid}/fd`).map(Number).sort((a,b)=>a-b),[0,1,2]);
    const body=Buffer.from('{}'),header=Buffer.alloc(4);header.writeUInt32BE(body.length);child.stdin.end(Buffer.concat([header,body]));
    const timer=setTimeout(()=>child.kill('SIGKILL'),2000);let exit;try{exit=await ended;finished=true;}finally{clearTimeout(timer);}
    assert.equal(exit.code,1);assert.equal(exit.signal,null);assert.equal(stdout,'');assert.match(stderr,/missing field/);
    assert.equal(fs.readFileSync(canary,'utf8'),'authored canary');assert.equal(fs.fstatSync(fd).size,Buffer.byteLength('authored canary'));
    const defaultResult=spawnSync(process.env.PULSAR_STASH_TEST_BINARY,['cuda-worker'],{env,input:'',encoding:'utf8',timeout:2000,maxBuffer:16384});assert.ifError(defaultResult.error);
    assert.equal(defaultResult.status,1);assert.equal(defaultResult.stdout,'');assert.match(defaultResult.stderr,/CUDA worker is not compiled; no automatic substitution/);
    t.diagnostic(JSON.stringify({scope:'standalone descriptor erasure only; marker supplied without namespace',input_wait_observed:true,extra_descriptor_closed:true,parent_canary_unchanged:true,default_cuda_worker_refused:true}));
  }finally{if(!finished && child.exitCode===null&&child.signalCode===null){child.kill('SIGKILL');await ended;}fs.closeSync(fd);}
});

test('second-stage CUDA role refuses a missing or unsealed relay before native execution',
 {skip:!process.env.PULSAR_CUDA_BOOTSTRAP_WORKER,timeout:10000},()=>{
  const worker=process.env.PULSAR_CUDA_BOOTSTRAP_WORKER;
  const invoke=(env,stdio)=>spawnSync(worker,['cuda-worker-exec'],{env,input:'',stdio,encoding:'utf8',timeout:2000,maxBuffer:16384});
  const missing=invoke({PATH:'/usr/bin:/bin',PULSAR_WORKER_CONFINED:'1'},['pipe','pipe','pipe']);
  assert.ifError(missing.error);assert.equal(missing.status,1);assert.equal(missing.stdout,'');
  assert.match(missing.stderr,/requires its private sealed relay/);
  const root=fs.mkdtempSync(path.join(os.tmpdir(),'pulsar-cuda-relay-'));
  const file=path.join(root,'unsealed');fs.writeFileSync(file,'{}',{mode:0o600});const fd=fs.openSync(file,'r');
  try{
    const unsealed=invoke({PATH:'/usr/bin:/bin',PULSAR_WORKER_CONFINED:'1',PULSAR_CUDA_RELAY_FD:'3'},['pipe','pipe','pipe',fd]);
    assert.ifError(unsealed.error);assert.equal(unsealed.status,1);assert.equal(unsealed.stdout,'');
    assert.match(unsealed.stderr,/relay is not fully sealed/);
    assert.equal(fs.readFileSync(file,'utf8'),'{}');assert.equal(fs.fstatSync(fd).size,2);
  }finally{fs.closeSync(fd);}
});
