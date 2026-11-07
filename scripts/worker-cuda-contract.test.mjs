// Authored metadata only: no runtime loading, device access or project state.
import test from 'node:test';
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import fs from 'node:fs';
import crypto from 'node:crypto';

const current=process.env.PULSAR_BENCHMARK_WORKER;
const preceding=process.env.PULSAR_OWNER_POLICY_READER;
const evidence=process.env.PULSAR_WORKER_CONTRACT_EVIDENCE;
let invocation=0;
const identity=n=>({artifact_id:`artifact-${n}`,sha256:n.toString(16).padStart(64,'0'),byte_len:1});
function fixture() {
  const input={width:64,height:64,channels:3,layout:'nchw',class_count:1,decoder:'yolo_channel_major'};
  const profile={schema_version:1,model:identity(9),runtime:identity(2),preprocessing:{input:structuredClone(input),
    resize:'stretch',color:'rgb',scale:[1/255,1/255,1/255],bias:[0,0,0],padding_byte:0},
    provider:'cpu',minimum_memory_bytes:1,minimum_output_bytes:1,cpu_threads:2};
  const tool={identity:identity(10),path:'/absent-contract-tool'};
  return {version:2,attempt_id:'attempt',job_id:'job',project_id:'project',base_revision:0,
    source:{source_version:'source',path:'/absent-contract-source',identity:identity(11)},
    output_dir:'/absent-pulsar-cuda-contract-output',operation:{operation:'generate',arguments:{preset:'fast',
      settings:{analysis_width:64,analysis_height:64,source_sample_stride:1,axis:'stroke',vr_mode:false,
        model_input:input,inference_profile:profile},model:{source_version:'model',path:'/absent-contract-model',identity:identity(9)}}},
    budget:{memory_bytes:1,output_bytes:1,wall_time_ms:1000,cpu_threads:2},
    dependencies:[identity(9),identity(2)],tools:{ffmpeg:tool,ffprobe:tool,onnx_runtime:{identity:identity(2),path:'/absent-contract-runtime'}}};
}
function cuda(request) {
  const uuid='GPU-1790118a-a6d7-4eaf-fcac-dcacac5f4351';
  const profile=request.operation.arguments.settings.inference_profile;
  Object.assign(profile,{schema_version:2,provider:'cuda',cuda_resources:{device_uuid:uuid,device_ordinal:0,
    address_space_bytes:1024,device_reservation_bytes:100,arena_limit_bytes:50}});
  request.cuda_execution={schema_version:1,owner_policy:identity(8),device_uuid:uuid,pci_bus_address:'0000:01:00.0',
    device_minor:0,device_ordinal:0,library_directory:'/absent-contract-capture',
    libraries:['ld-linux-x86-64.so.2','libcuda.so.1','libonnxruntime.so.1','libonnxruntime_providers_cuda.so',
      'libonnxruntime_providers_shared.so','libcudnn.so.9'].map((name,n)=>({identity:identity(n),names:[name]}))};
  request.dependencies.push(identity(8),...request.cuda_execution.libraries.map(lib=>lib.identity));
  return request;
}
function worker(binary,request) {
  const body=Buffer.from(JSON.stringify(request)),header=Buffer.alloc(4);header.writeUInt32BE(body.length);
  const result=spawnSync(binary,['worker'],{input:Buffer.concat([header,body]),timeout:10000,maxBuffer:1024*1024});
  assert.equal(result.error,undefined);
  if(evidence) {
    const prefix=`${evidence}/${String(invocation++).padStart(2,'0')}`;
    fs.writeFileSync(`${prefix}.request.json`,body,{flag:'wx',mode:0o600});
    fs.writeFileSync(`${prefix}.stdout.bin`,result.stdout,{flag:'wx',mode:0o600});
    fs.writeFileSync(`${prefix}.stderr.log`,result.stderr,{flag:'wx',mode:0o600});
    fs.writeFileSync(`${prefix}.result.json`,JSON.stringify({binary,status:result.status,
      request_sha256:crypto.createHash('sha256').update(body).digest('hex')},null,2),{flag:'wx',mode:0o600});
  }
  if(result.status!==0)return {status:result.status,stderr:result.stderr.toString(),raw:result.stdout};
  assert.ok(result.stdout.length>=4);
  assert.equal(result.stdout.readUInt32BE(),result.stdout.length-4);
  return {status:0,raw:result.stdout,response:JSON.parse(result.stdout.subarray(4))};
}
test('shipped workers preserve CPU responses and refuse unsupported CUDA transport correctly',
  {skip:!current||!preceding},()=>{
    if(evidence)fs.mkdirSync(evidence,{mode:0o700});
    assert.equal(fs.existsSync('/absent-pulsar-cuda-contract-output'),false);
    const before=[current,preceding].map(p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex'));
    const cpu=fixture(),old=worker(preceding,cpu),now=worker(current,cpu);
    assert.equal(old.status,0,old.stderr);assert.equal(now.status,0,now.stderr);
    assert.deepEqual(now.raw,old.raw);
    assert.match(now.response.result.Err.message,/approved attempt directory is absent/);
    const request=cuda(fixture()),legacy=worker(preceding,request),valid=worker(current,request);
    assert.notEqual(legacy.status,0);assert.match(legacy.stderr,/unknown field `cuda_execution`/);
    assert.equal(valid.status,0,valid.stderr);assert.match(valid.response.result.Err.message,/unsupported pinned provider/);
    for(const mutate of [r=>delete r.cuda_execution,r=>r.cuda_execution.device_ordinal=1,
      r=>r.dependencies=r.dependencies.filter(a=>a.artifact_id!=='artifact-8'),
      r=>r.cuda_execution.libraries[0].names.push('../escape'),
      r=>r.tools.onnx_runtime.identity=identity(12),
      r=>r.operation.arguments.settings.model_input.class_count=2]) {
      const bad=structuredClone(request);mutate(bad);const result=worker(current,bad);
      assert.equal(result.status,0,result.stderr);assert.equal(result.response.result.Err.code,'invalid_request');
    }
    assert.equal(fs.existsSync('/absent-pulsar-cuda-contract-output'),false);
    assert.deepEqual([current,preceding].map(p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex')),before);
  });
