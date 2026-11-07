// Linux research harness. No private model, production admission or device playback.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import assert from 'node:assert/strict';
import {authoredOnnxFixture} from '../../../scripts/benchmark-onnx-fixture.mjs';

const [destination,cudnnDirectory,...extra]=process.argv.slice(2);
assert.equal(process.platform,'linux');
assert.ok(destination&&cudnnDirectory&&!extra.length,'usage: node cuda-runtime-probe.mjs NEW-OUTPUT-DIRECTORY EXISTING-CUDNN-DIRECTORY');
const cudnn=fs.realpathSync(cudnnDirectory);
assert.ok(fs.statSync(path.join(cudnn,'libcudnn.so.9')).isFile());
fs.mkdirSync(destination,{mode:0o700});
const root=fs.realpathSync(destination);
const source=fileURLToPath(new URL('./cuda-runtime-probe.cpp',import.meta.url));
const binary=path.join(root,'cuda-runtime-probe'),model=path.join(root,'authored.onnx');
const records=[];
function run(name,program,args,env={}) {
  const result=spawnSync(program,args,{env:{PATH:'/usr/bin:/bin',...env},encoding:'utf8',timeout:30000,maxBuffer:8*1024**2});
  fs.writeFileSync(path.join(root,name+'.stdout'),result.stdout??'',{flag:'wx',mode:0o600});
  fs.writeFileSync(path.join(root,name+'.stderr'),result.stderr??'',{flag:'wx',mode:0o600});
  const record={name,program,args,environment:env,exit_code:result.status,signal:result.signal,error:result.error?.message??null};
  records.push(record);
  assert.ok(!result.error&&!result.signal,`${name}: harness timeout/output/process error`);
  return {record,result};
}
const identityCache=new Map();
function identity(filename) {
  const absolute=fs.realpathSync(filename);
  if(!identityCache.has(absolute)) {
    const fd=fs.openSync(absolute,'r');
    try {
      const before=fs.fstatSync(fd),hash=crypto.createHash('sha256'),buffer=Buffer.alloc(1024*1024);let length=0;
      assert.ok(before.isFile());
      for(;;){const count=fs.readSync(fd,buffer,0,buffer.length,null);if(!count)break;length+=count;hash.update(buffer.subarray(0,count));}
      const after=fs.fstatSync(fd);
      assert.equal(length,before.size);assert.equal(after.size,before.size);assert.equal(after.mtimeMs,before.mtimeMs);assert.equal(after.ctimeMs,before.ctimeMs);
      identityCache.set(absolute,{path:absolute,sha256:hash.digest('hex'),byte_len:length});
    } finally {fs.closeSync(fd);}
  }
  return identityCache.get(absolute);
}
try {
  fs.copyFileSync(source,path.join(root,'source.cpp'),fs.constants.COPYFILE_EXCL);
  fs.copyFileSync(fileURLToPath(import.meta.url),path.join(root,'harness.mjs'),fs.constants.COPYFILE_EXCL);
  fs.writeFileSync(model,authoredOnnxFixture({temporal:true}),{flag:'wx',mode:0o400});
  const pinned=['/usr/bin/g++','/usr/bin/bwrap','/usr/lib/libonnxruntime.so.1.29.0',
    '/usr/lib/libonnxruntime_providers_cuda.so','/usr/include/onnxruntime/onnxruntime_c_api.h',
    '/usr/include/onnxruntime/onnxruntime_cxx_api.h',path.join(cudnn,'libcudnn.so.9')].map(identity);
  assert.equal(run('compile','/usr/bin/g++',['-std=c++17','-O2','-Wall','-Wextra','-Werror',source,'-lonnxruntime','-o',binary]).record.exit_code,0);
  const binaryIdentity=identity(binary),modelIdentity=identity(model);
  const preload={LD_PRELOAD:path.join(cudnn,'libcudnn.so.9')};
  const namespace=['--unshare-all','--die-with-parent','--new-session','--clearenv'];
  for(const mount of ['/usr','/lib','/lib64','/bin','/etc/ld.so.cache','/opt/cuda/lib64'])namespace.push('--ro-bind',mount,mount);
  namespace.push('--ro-bind',cudnn,'/cudnn','--bind',root,'/probe','--proc','/proc','--dev','/dev','--tmpfs','/tmp',
    '--ro-bind','/usr/bin/false','/usr/bin/nvidia-modprobe','--cap-drop','ALL',
    '--setenv','LD_PRELOAD','/cudnn/libcudnn.so.9');
  const devices=['/dev/nvidiactl','/dev/nvidia0','/dev/nvidia-uvm'].flatMap(device=>['--dev-bind',device,device]);
  const sysfs=['/sys/module/nvidia/initstate','/sys/module/nvidia_uvm/initstate',
    '/sys/devices/system/memory/block_size_bytes','/sys/devices/system/node','/sys/devices/system/cpu']
    .flatMap(filename=>['--ro-bind',filename,filename]);
  const cases=[
    {name:'cpu',program:binary,args:['cpu',model,path.join(root,'cpu')],provider:'CPUExecutionProvider'},
    {name:'cuda-system-default',program:binary,args:['cuda',model,path.join(root,'cuda-default')],provider:'CUDAExecutionProvider',optional:true},
    {name:'cuda-host-preload',program:binary,args:['cuda',model,path.join(root,'cuda-host')],env:preload,provider:'CUDAExecutionProvider'},
    {name:'cuda-namespace-no-device',program:'/usr/bin/bwrap',args:[...namespace,'/probe/cuda-runtime-probe','cuda','/probe/authored.onnx','/probe/cuda-no-device'],expectedRefusal:100},
    {name:'cuda-namespace-no-sysfs',program:'/usr/bin/bwrap',args:[...namespace,...devices,
      '/probe/cuda-runtime-probe','cuda','/probe/authored.onnx','/probe/cuda-no-sysfs'],expectedRefusal:304},
    {name:'cuda-namespace-explicit-device',program:'/usr/bin/bwrap',args:[...namespace,
      ...devices,...sysfs,
      '/probe/cuda-runtime-probe','cuda','/probe/authored.onnx','/probe/cuda-device'],provider:'CUDAExecutionProvider'},
  ];
  const summaries=[];
  for(const item of cases) {
    const {record,result}=run(item.name,item.program,item.args,item.env);
    if(item.expectedRefusal){
      assert.equal(record.exit_code,1);assert.ok(result.stderr.includes(`CUDA failure ${item.expectedRefusal}:`));
      assert.ok(result.stderr.includes('host diagnostic refused:'));assert.equal(result.stdout,'');
      summaries.push({name:item.name,state:'expected_cuda_refusal',cuda_error:item.expectedRefusal,exit_code:record.exit_code});continue;
    }
    if(item.optional&&record.exit_code!==0){summaries.push({name:item.name,state:'refused',exit_code:record.exit_code});continue;}
    assert.equal(record.exit_code,0,result.stderr);
    const output=JSON.parse(result.stdout);assert.deepEqual(output.outputs,[.25,.375,.5,.75]);assert.equal(output.runs,4);
    const profileLine=result.stderr.split('\n').find(line=>line.startsWith('PROFILE_PATH='));assert.ok(profileLine);
    let profile=profileLine.slice('PROFILE_PATH='.length);
    if(profile.startsWith('/probe/'))profile=path.join(root,profile.slice('/probe/'.length));
    assert.equal(path.dirname(profile),root);
    const trace=JSON.parse(fs.readFileSync(profile,'utf8'));
    const computation=trace.filter(event=>event.cat==='Node'&&event.args?.provider&&!event.args.op_name?.startsWith('Memcpy'));
    assert.equal(computation.length,12);assert.deepEqual([...new Set(computation.map(event=>event.args.provider))],[item.provider]);
    assert.deepEqual([...new Set(computation.map(event=>event.args.op_name))].sort(),['Add','Mul','ReduceMean']);
    const libraries=[...new Set(result.stderr.split('\n').filter(line=>line.startsWith('LIBRARY_PATH=')).map(line=>line.slice('LIBRARY_PATH='.length)))];
    assert.ok(libraries.length);
    const mapped=libraries.map(filename=>identity(filename.startsWith('/cudnn/')?path.join(cudnn,filename.slice('/cudnn/'.length)):filename));
    summaries.push({name:item.name,state:'authored_oracle_and_node_trace_passed',output,computation_events:computation.length,
      executed_computation_providers:[item.provider],profile:path.basename(profile),loaded_library_identities:mapped});
  }
  // Read back the exact local sources and binary; this is not a full runtime lock.
  assert.deepEqual(fs.readFileSync(source),fs.readFileSync(path.join(root,'source.cpp')));
  assert.deepEqual(fs.readFileSync(fileURLToPath(import.meta.url)),fs.readFileSync(path.join(root,'harness.mjs')));
  identityCache.delete(fs.realpathSync(binary));
  assert.equal(identity(binary).sha256,binaryIdentity.sha256);
  for(const before of pinned){identityCache.delete(before.path);assert.deepEqual(identity(before.path),before);}
  fs.writeFileSync(path.join(root,'validation.json'),JSON.stringify({schema_version:1,state:'research_probe_completed',qualification:'unqualified',
    production_integrated:false,private_media_or_models_used:false,whole_worker_vram_quota_enforced:false,
    arena_limit_bytes:67108864,scope:'Linux host and explicit experimental namespaces; not production cgroup/limits/supervision',
    pinned,binary:binaryIdentity,model:modelIdentity,commands:records,cases:summaries,
    limitations:['Single tiny authored graph; no useful model, throughput or generalization evidence','CUDA arena setting is not a whole-worker VRAM quota',
      'Node traces cover observed computation only; no complete dependency closure guarantee','Explicit GPU nodes expand the native driver surface',
      'Production provider admission, dependency capture, device scheduling, confinement and release qualification remain required']},null,2)+'\n',{flag:'wx',mode:0o600});
  console.log(JSON.stringify({directory:root,state:'research_probe_completed',cases:summaries.map(({name,state})=>({name,state}))}));
} catch(error) {
  fs.writeFileSync(path.join(root,'failure.json'),JSON.stringify({state:'research_probe_failed',message:error.message,commands:records,qualification:'unqualified'},null,2)+'\n',{flag:'wx',mode:0o600});
  throw error;
}
