// Private wire compatibility only; no native library or GPU execution.
import test from 'node:test';import assert from 'node:assert/strict';import fs from 'node:fs';import path from 'node:path';import {spawnSync} from 'node:child_process';
const configured=process.env.PULSAR_PRECEDING_LIVE_READER&&process.env.PULSAR_CURRENT_LIVE_READER;
test('preceding and current ordinary readers preserve CPU bytes and refuse the private CUDA live envelope',{skip:!configured},()=>{
 const cpu=JSON.parse(fs.readFileSync('crates/pulsar-protocol/tests/fixtures/live-cpu-manifest.json'));
 const worker=JSON.parse(fs.readFileSync('crates/pulsar-engine/tests/fixtures/cuda-intent-request.json'));
 const cuda={schema_version:1,stream:{...cpu,model:{path:worker.operation.arguments.model.path,identity:worker.operation.arguments.model.identity},runtime:{...worker.tools.onnx_runtime,path:path.join(worker.cuda_execution.library_directory,'libonnxruntime.so.1')},profile:worker.operation.arguments.settings.inference_profile,budget:{...worker.budget,wall_time_ms:30000}},execution:worker.cuda_execution,dependencies:[...worker.dependencies,cpu.worker],output_dir:'/work'};
 if(!cuda.execution.libraries.some(lib=>lib.names.includes('libnvidia-ml.so.1'))){const nvml={artifact_id:'e'.repeat(64),sha256:'e'.repeat(64),byte_len:1};cuda.execution.libraries.push({identity:nvml,names:['libnvidia-ml.so.1']});cuda.dependencies.push(nvml);}
 const accepted=spawnSync(process.env.PULSAR_CURRENT_LIVE_READER,['--cuda-contract'],{input:JSON.stringify(cuda),encoding:'utf8',timeout:10000});assert.ifError(accepted.error);assert.equal(accepted.status,0,accepted.stderr);assert.match(accepted.stdout,/^[a-f0-9]{64}$/);
 const out=[];for(const binary of [process.env.PULSAR_PRECEDING_LIVE_READER,process.env.PULSAR_CURRENT_LIVE_READER]){
  const result=spawnSync(binary,[],{input:JSON.stringify(cpu),encoding:'utf8',timeout:10000});assert.ifError(result.error);assert.equal(result.status,0,result.stderr);out.push(result.stdout);
  for(const [name,input]of [['envelope',cuda],['bare-cuda-stream',cuda.stream]]){const refused=spawnSync(binary,[],{input:JSON.stringify(input),encoding:'utf8',timeout:10000});assert.ifError(refused.error);assert.notEqual(refused.status,0,name);assert.equal(refused.stdout,'');}
 }
 assert.equal(out[0],out[1]);assert.equal(out[0],fs.readFileSync(process.env.PULSAR_PRECEDING_CPU_BYTES,'utf8'));assert.equal(out[0].includes('cuda'),false);
 if(process.env.PULSAR_LIVE_CONTRACT_EVIDENCE){const dest=path.resolve(process.env.PULSAR_LIVE_CONTRACT_EVIDENCE);assert.equal(fs.existsSync(dest),false);fs.mkdirSync(dest,{recursive:true});fs.writeFileSync(path.join(dest,'cpu.json'),out[0]);fs.writeFileSync(path.join(dest,'cuda-envelope.json'),JSON.stringify(cuda,null,2));fs.writeFileSync(path.join(dest,'checked.json'),JSON.stringify({cpu_canonical_bytes_equal:true,ordinary_readers_refuse_cuda:true,private_cuda_metadata_valid:true,manifest_sha256:accepted.stdout,native_execution:false},null,2));}
});
