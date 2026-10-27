import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import { runBenchmarkCampaign } from './benchmark-campaign.mjs';
import { authoredOnnxFixture } from './benchmark-onnx-fixture.mjs';

const hash = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
const configured = process.env.PULSAR_BENCHMARK_RUNNER && process.env.PULSAR_BENCHMARK_WORKER;
test('authored stationary media traverses intake, production worker, adapter and scoring', {skip: !configured}, async t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(),'pulsar-campaign-evidence-'));
  t.diagnostic(`retained authored campaign evidence: ${root}`);
  const pin = filename => {
    const absolute = fs.realpathSync(filename), bytes = fs.readFileSync(absolute), digest = hash(bytes);
    return {path:absolute,identity:{artifact_id:digest,sha256:digest,byte_len:bytes.length}};
  };
  const worker = pin(process.env.PULSAR_BENCHMARK_WORKER);
  const runner = pin(process.env.PULSAR_BENCHMARK_RUNNER);
  const media = Buffer.concat([Buffer.from('YUV4MPEG2 W64 H64 F8:1 Ip A1:1 Cmono\n'),
    ...Array.from({length:8},()=>Buffer.concat([Buffer.from('FRAME\n'),Buffer.alloc(64*64,128)]))]);
  fs.writeFileSync(path.join(root,'stationary.y4m'),media);
  const reference = {schema_version:1,subject_id:'stationary',
    window:{start_ns:'0',end_ns:'1000000000'},strokes:[],material_sections:[]};
  const referenceBytes = Buffer.from(JSON.stringify(reference));
  fs.writeFileSync(path.join(root,'reference.json'),referenceBytes);
  const corpus = {schema_version:1,policy_version:'B1.1',entries:[{
    id:'stationary',category:'authored-stationary',split:'development',source_group_id:'authored-stationary',
    evidence_class:'synthetic',source_sha256:hash(media),
    rights:{status:'permitted',evidence_id:'agent-authored-constant-pixels',permitted_uses:['evaluation'],redistribution:'forbidden'},
    annotation_provenance:{reference_author:'constant-pixel-fixture',independent:true,
      method:'constant frames imply no displacement; reference written before execution',review_record:'synthetic-only'},
    reference:{path:'reference.json',sha256:hash(referenceBytes)},
    candidate:{path:'unused-supplied-candidate.json',sha256:'0'.repeat(64)},
  }]};
  fs.writeFileSync(path.join(root,'corpus.json'),JSON.stringify(corpus));
  fs.writeFileSync(path.join(root,'sources.json'),JSON.stringify({schema_version:1,entries:[{entry_id:'stationary',path:'stationary.y4m'}]}));
  const config = {
    runner:runner.path,runner_sha256:runner.identity.sha256,attempt_timeout_ms:35000,
    intake:{corpus:path.join(root,'corpus.json'),sources:path.join(root,'sources.json'),outputRoot:root,
      split:'development',maxSourceBytes:1024*1024,maxTotalBytes:1024*1024},
    targets:{stationary:'target'},
    generation:{worker_path:worker.path,worker_identity:worker.identity,
      tools:{ffmpeg:pin('/usr/bin/ffmpeg'),ffprobe:pin('/usr/bin/ffprobe'),onnx_runtime:null},
      model:null,preset:'fast',
      settings:{analysis_width:64,analysis_height:64,source_sample_stride:1,axis:'stroke',vr_mode:false,model_input:null},
      // Match the production engine's address-space admission. This host's
      // FFmpeg cannot start its output thread under the older 512 MiB fixture.
      budget:{memory_bytes:1536*1024*1024,output_bytes:16*1024*1024,wall_time_ms:30000,cpu_threads:1}},
  };
  const result = await runBenchmarkCampaign(config);
  assert.equal(result.receipt.results.length,1);
  const attempt = result.receipt.results[0];
  assert.equal(attempt.exit_code,0,attempt.stderr);
  assert.equal(attempt.error,null);
  assert.notEqual(attempt.score,null);
  assert.equal(attempt.adapter_receipt.adapter,'neutral-monotone-plateau-v1');
  assert.ok(Array.isArray(attempt.adapter_receipt.worker_review));
  assert.equal(attempt.adapter_receipt.review_evidence,'native-global-and-stroke-flags-mapped-v1');
  assert.equal(result.receipt.human_effort,null);
  assert.equal(result.receipt.release_qualified,false);
  const cancelled = await runBenchmarkCampaign(config, { signal: AbortSignal.abort() });
  assert.match(cancelled.receipt.results[0].error, /cancelled/);
  assert.equal(cancelled.receipt.results[0].score, null);
  assert.equal(fs.existsSync(path.join(cancelled.directory,'attempt-0')), false);
  const workerDrift = structuredClone(config);
  workerDrift.generation.worker_identity.sha256 = 'e'.repeat(64);
  const rejectedWorker = await runBenchmarkCampaign(workerDrift);
  assert.notEqual(rejectedWorker.receipt.results[0].exit_code, 0);
  assert.match(rejectedWorker.receipt.results[0].stderr, /worker bytes do not match/);
  assert.equal(rejectedWorker.receipt.results[0].score, null);
  t.diagnostic(`campaign receipt: ${path.join(result.directory,'campaign.json')}`);
  const effortBytes = Buffer.from('Synthetic effort-binding regression fixture. Not real human measurements.');
  const effortPath = path.join(root,'synthetic-effort.txt');
  fs.writeFileSync(effortPath,effortBytes);
  const withEffort = structuredClone(config);
  withEffort.human_effort = {stationary:{evidence_path:effortPath,record:{
    schema_version:1,subject_id:'stationary',source_sha256:hash(media),
    candidate_sha256:attempt.adapter_receipt.raw_sha256,
    actor:'synthetic-regression-fixture',session_id:'synthetic-session',active_edit_ms:0,wall_elapsed_ms:1,
    method:'instrumented-editor',evidence_sha256:hash(effortBytes),
  }}};
  const effortCampaign = await runBenchmarkCampaign(withEffort);
  assert.equal(effortCampaign.receipt.results[0].error,null);
  assert.equal(effortCampaign.receipt.human_effort.length,1);
  assert.deepEqual(effortCampaign.receipt.human_effort[0],effortCampaign.receipt.results[0].human_effort);
  assert.equal(effortCampaign.receipt.human_effort[0].status,'supplied-human-claim-not-independently-authenticated');
  assert.equal(hash(fs.readFileSync(path.join(effortCampaign.directory,effortCampaign.receipt.human_effort[0].evidence.path))),hash(effortBytes));
  const badEffort = structuredClone(withEffort);
  badEffort.human_effort.stationary.record.candidate_sha256 = 'f'.repeat(64);
  const rejectedEffort = await runBenchmarkCampaign(badEffort);
  assert.match(rejectedEffort.receipt.results[0].error, /candidate\/scoring failure/);
  assert.equal(rejectedEffort.receipt.results[0].score, null);
  assert.equal(rejectedEffort.receipt.human_effort, null);
  const rejectedConfig = structuredClone(config);
  rejectedConfig.generation.preset = 'unsupported-campaign-preset';
  const rejected = await runBenchmarkCampaign(rejectedConfig);
  assert.notEqual(rejected.receipt.results[0].exit_code,0);
  assert.equal(rejected.receipt.results[0].score,null);
  const failure = JSON.parse(fs.readFileSync(path.join(rejected.directory,'attempt-0','execution.json'),'utf8'));
  assert.ok(failure.result.Err,'worker failure must survive in execution receipt');
  assert.equal(failure.release_qualified,false);
  t.diagnostic(`failed-worker receipt retained: ${path.join(rejected.directory,'campaign.json')}`);
  const deadlineConfig=structuredClone(config);
  deadlineConfig.generation.budget.wall_time_ms=1;
  const deadlineCampaign=await runBenchmarkCampaign(deadlineConfig);
  assert.notEqual(deadlineCampaign.receipt.results[0].exit_code,0);
  assert.equal(deadlineCampaign.receipt.results[0].score,null);
  const deadlineReceipt=JSON.parse(fs.readFileSync(path.join(deadlineCampaign.directory,'attempt-0','execution.json'),'utf8'));
  assert.match(deadlineReceipt.result.Err.message,/deadline/);
  t.diagnostic(`deadline failure receipt: ${path.join(deadlineCampaign.directory,'campaign.json')}`);
  if (process.env.PULSAR_BENCHMARK_ORT) {
    const modelPath=path.join(root,'authored-diagnostic.onnx');
    fs.writeFileSync(modelPath,authoredOnnxFixture());
    const model=pin(modelPath),runtime=pin(process.env.PULSAR_BENCHMARK_ORT);
    const withModel=structuredClone(config);
    withModel.generation.model={source_version:'authored-model-v1',...model};
    withModel.generation.tools.onnx_runtime=runtime;
    const input={width:64,height:64,channels:3,layout:'nchw',class_count:1,decoder:'yolo_channel_major'};
    withModel.generation.settings.model_input=input;
    withModel.generation.profile={schema_version:1,model:model.identity,runtime:runtime.identity,
      preprocessing:{input,resize:'stretch',color:'rgb',scale:[1/255,1/255,1/255],bias:[0,0,0],padding_byte:0},
      provider:'cpu',minimum_memory_bytes:128*1024*1024,minimum_output_bytes:1024*1024,cpu_threads:1};
    const modelCampaign=await runBenchmarkCampaign(withModel);
    assert.equal(modelCampaign.receipt.results[0].exit_code,0,modelCampaign.receipt.results[0].stderr);
    assert.equal(modelCampaign.receipt.results[0].error,null);
    const execution=JSON.parse(fs.readFileSync(path.join(modelCampaign.directory,'attempt-0','execution.json'),'utf8'));
    assert.equal(execution.declared_profile.profile.provider,'cpu');
    assert.equal(execution.declared_profile.sha256.length,64);
    assert.ok(execution.worker_peak_memory_bytes>0);
    assert.equal(modelCampaign.receipt.results[0].adapter_receipt.worker_peak_memory_bytes,execution.worker_peak_memory_bytes);
    assert.equal(modelCampaign.receipt.results[0].adapter_receipt.memory_scope,'whole-confined-worker-cgroup');
    const nativeReceipt=JSON.parse(fs.readFileSync(path.join(modelCampaign.directory,'attempt-0','artifacts','receipt.json'),'utf8'));
    assert.equal(nativeReceipt.inference_execution.provider,'cpu');
    assert.ok(nativeReceipt.inference_execution.successful_runs>0);
    assert.ok(BigInt(nativeReceipt.inference_execution.inference_elapsed_ns)>0n);
    assert.equal(nativeReceipt.inference_execution.peak_memory_bytes,null);
    const nested = structuredClone(withModel);
    nested.generation.settings.inference_profile = nested.generation.profile;
    delete nested.generation.profile;
    const nestedCampaign = await runBenchmarkCampaign(nested);
    assert.equal(nestedCampaign.receipt.results[0].exit_code, 0, nestedCampaign.receipt.results[0].stderr);
    const nestedExecution = JSON.parse(fs.readFileSync(path.join(nestedCampaign.directory,'attempt-0','execution.json'),'utf8'));
    assert.equal(nestedExecution.declared_profile.sha256, execution.declared_profile.sha256);
    nested.generation.profile = structuredClone(nested.generation.settings.inference_profile);
    nested.generation.profile.provider = 'cuda';
    const conflict = await runBenchmarkCampaign(nested);
    assert.notEqual(conflict.receipt.results[0].exit_code, 0);
    assert.equal(conflict.receipt.results[0].score, null);
    assert.match(conflict.receipt.results[0].stderr, /conflicting campaign profiles/);
    const strict = structuredClone(withModel);
    strict.generation.profile.provider = 'cuda';
    const rejectedPin = await runBenchmarkCampaign(strict);
    assert.notEqual(rejectedPin.receipt.results[0].exit_code, 0);
    assert.match(rejectedPin.receipt.results[0].stderr, /unsupported pinned provider/);
    assert.equal(rejectedPin.receipt.results[0].score, null);
    // Preserve the original fixture and declare its identity against changed bytes.
    const corruptPath = path.join(root, 'corrupt-model.onnx');
    fs.writeFileSync(corruptPath, Buffer.alloc(model.identity.byte_len));
    const corrupt = structuredClone(withModel);
    corrupt.generation.model.path = corruptPath;
    const corruptCampaign = await runBenchmarkCampaign(corrupt);
    assert.notEqual(corruptCampaign.receipt.results[0].exit_code, 0);
    assert.equal(corruptCampaign.receipt.results[0].score, null);
    const corruptExecution = JSON.parse(fs.readFileSync(path.join(corruptCampaign.directory,'attempt-0','execution.json'),'utf8'));
    assert.equal(corruptExecution.result.Err.code, 'dependency_mismatch');
    t.diagnostic(`authored ONNX campaign receipt: ${path.join(modelCampaign.directory,'campaign.json')}`);
  }
});
