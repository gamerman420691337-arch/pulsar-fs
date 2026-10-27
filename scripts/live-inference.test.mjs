// Actual persistent confined worker with an authored input-dependent ONNX graph.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import {spawnSync} from 'node:child_process';
import {authoredOnnxFixture} from './benchmark-onnx-fixture.mjs';
const worker=process.env.PULSAR_BENCHMARK_WORKER;
const runtime=process.env.PULSAR_BENCHMARK_ORT;
const runner=process.env.PULSAR_LIVE_INFERENCE_RUNNER||(worker?path.join(path.dirname(worker),'examples/live-inference-fixture'):undefined);
const pin=filename=>{
  const absolute=fs.realpathSync(filename);const hash=crypto.createHash('sha256');const file=fs.openSync(absolute,'r');let count=0;const buffer=Buffer.alloc(65536);
  try{for(;;){const n=fs.readSync(file,buffer,0,buffer.length,null);if(n===0)break;count+=n;hash.update(buffer.subarray(0,n));}}finally{fs.closeSync(file);}
  const sha256=hash.digest('hex');return {path:absolute,identity:{artifact_id:sha256,sha256,byte_len:count}};
};
test('encoded source role rejects accidental unconfined invocation before reading input',{skip:!worker},()=>{
  const env={...process.env};delete env.PULSAR_WORKER_CONFINED;
  const result=spawnSync(worker,['capture-worker'],{env,input:Buffer.alloc(0),encoding:'utf8',timeout:3000,maxBuffer:65536});
  assert.equal(result.error,undefined);assert.notEqual(result.status,0);assert.match(result.stderr,/parent-owned confinement|unsupported on this platform/);assert.equal(result.stdout,'');
});
test('authored unconfined source role preserves framed manifest and encoded body boundary',{skip:!worker||process.platform!=='linux'},t=>{
  const root=fs.mkdtempSync(path.join(os.tmpdir(),'pulsar-source-role-'));t.diagnostic(`retained authored unconfined source role: ${root}`);
  const ffmpeg=pin('/usr/bin/ffmpeg'),binary=pin(worker);const identity={epoch:'authored-source-capture',project:'authored-project',revision:0,source:'authored-pipe-source'};
  const manifest={version:2,attempt_id:'authored-decode-attempt',identity,ffmpeg,worker:binary.identity,format:'yuv4mpeg_pipe',maximum_input_bytes:1048576,maximum_frames:10,budget:{memory_bytes:1073741824,output_bytes:1048576,wall_time_ms:3000,cpu_threads:1}};
  const source=Buffer.concat([Buffer.from('YUV4MPEG2 W64 H64 F4:1 Ip A1:1 Cmono\n'),...Array.from([0,128,255],v=>Buffer.concat([Buffer.from('FRAME\n'),Buffer.alloc(4096,v)]))]);
  const json=Buffer.from(JSON.stringify(manifest)),size=Buffer.alloc(4);size.writeUInt32BE(json.length);const request=Buffer.concat([size,json,source]);
  fs.writeFileSync(path.join(root,'request.bin'),request);fs.writeFileSync(path.join(root,'manifest.json'),JSON.stringify(manifest,null,2));fs.writeFileSync(path.join(root,'source.y4m'),source);
  // The marker exercises the role's descriptor boundary. This fixture is
  // deliberately unconfined and cannot qualify the future engine launcher.
  const result=spawnSync(binary.path,['capture-worker'],{env:{...process.env,PULSAR_WORKER_CONFINED:'1'},input:request,timeout:5000,maxBuffer:1048576});
  fs.writeFileSync(path.join(root,'reply.bin'),result.stdout);fs.writeFileSync(path.join(root,'stderr'),result.stderr);assert.equal(result.error,undefined);assert.equal(result.status,0,result.stderr.toString());
  let offset=0;const header=()=>{const n=result.stdout.readUInt32BE(offset);offset+=4;assert.ok(n>0&&n<=65536);const v=JSON.parse(result.stdout.subarray(offset,offset+n).toString());offset+=n;assert.equal(v.payload.attempt_id,manifest.attempt_id);assert.deepEqual(v.payload.identity,identity);return v;};
  const ready=header();assert.equal(ready.status,'ready');assert.equal(ready.payload.format,manifest.format);
  for(let n=0;n<3;n++){const frame=header();assert.equal(frame.status,'frame');assert.equal(frame.payload.frame,n);assert.deepEqual(frame.payload.source_time,{numerator:n,denominator:4});assert.ok(frame.payload.delivered_elapsed_ns>=ready.payload.worker_elapsed_ns);const pixels=result.stdout.subarray(offset,offset+12288);offset+=12288;assert.equal(pixels.length,12288);assert.equal(frame.payload.pixels.sha256,crypto.createHash('sha256').update(pixels).digest('hex'));}
  const end=header();assert.equal(end.status,'end');assert.equal(end.payload.frames,3);assert.equal(end.payload.input_bytes,source.length);assert.equal(end.payload.input_sha256,crypto.createHash('sha256').update(source).digest('hex'));assert.equal(offset,result.stdout.length);
  fs.writeFileSync(path.join(root,'validation.json'),JSON.stringify({worker:binary,ffmpeg,manifest_body_boundary:true,frames:3,source_sha256:end.payload.input_sha256,source_bytes:end.payload.input_bytes,confined:false,lease_integration:false,physical_dispatch:false},null,2));
});
test('parent confined encoded source binds delivery clocks and existing lease expiry and validates pixels intake and independent reap',{skip:!worker||process.platform!=='linux'},t=>{
  const sourceRunner=path.join(path.dirname(worker),'examples/live-source-fixture');assert.ok(fs.existsSync(sourceRunner),'native source fixture must be built');
  const root=fs.mkdtempSync(path.join(os.tmpdir(),'pulsar-parent-source-'));t.diagnostic(`retained parent confined source evidence: ${root}`);
  const source=Buffer.concat([Buffer.from('YUV4MPEG2 W64 H64 F4:1 Ip A1:1 Cmono\n'),...Array.from([0,128,255],v=>Buffer.concat([Buffer.from('FRAME\n'),Buffer.alloc(4096,v)]))]);
  const sourcePath=path.join(root,'source.y4m');fs.writeFileSync(sourcePath,source);
  const ffmpeg=pin('/usr/bin/ffmpeg'),binary=pin(worker);
  const input={worker_path:binary.path,source_path:sourcePath,wait_for_first_frame:true,manifest:{version:2,attempt_id:'placeholder',identity:{epoch:'authored-capture',project:'authored-project',revision:0,source:'authored-source'},ffmpeg,worker:binary.identity,format:'yuv4mpeg_pipe',maximum_input_bytes:1048576,maximum_frames:10,budget:{memory_bytes:1073741824,output_bytes:1048576,wall_time_ms:30000,cpu_threads:1}}};
  const run=(name,change=()=>{})=>{const value=structuredClone(input);value.manifest.attempt_id=crypto.randomUUID();change(value);const file=path.join(root,`${name}.json`);fs.writeFileSync(file,JSON.stringify(value));const result=spawnSync(sourceRunner,[file],{env:process.env,encoding:'utf8',timeout:45000,maxBuffer:1048576});fs.writeFileSync(path.join(root,`${name}.result.json`),JSON.stringify({status:result.status,error:result.error?.message,stdout:result.stdout,stderr:result.stderr},null,2));assert.equal(result.error,undefined);return result;};
  const normal=run('normal');assert.equal(normal.status,0,normal.stderr);const report=JSON.parse(normal.stdout);assert.equal(report.completed,true);assert.equal(report.frames.length,3);assert.equal(report.producer_result.Ok,null);assert.equal(report.local_bounds_refused,true);assert.equal(report.confined,true);assert.equal(report.process_reaped,true);assert.ok(report.worker_peak_memory_bytes>0);assert.equal(report.lease_integration,false);assert.equal(report.physical_dispatch,false);
  const checkDelivery=v=>{
    assert.equal(v.clock_probe.parent_end,30000000000);assert.ok(v.clock_probe.parent_receive>=v.clock_probe.parent_send);assert.ok(v.clock_probe.parent_receive-v.clock_probe.parent_send<=1000000000);
    for(const frame of v.frames){const d=frame.delivery;assert.deepEqual(d.probe,v.clock_probe);assert.equal(d.age_basis,'decoded_stdout_local_monotonic_equal_rate');assert.deepEqual(d.identity,input.manifest.identity);assert.deepEqual(d.worker,binary.identity);assert.deepEqual(d.ffmpeg,ffmpeg.identity);assert.deepEqual(d.source_time,frame.source_time);assert.equal(d.frame,frame.frame);assert.equal(d.pixels.sha256,frame.pixel_sha256);assert.equal(d.worker_delivered_elapsed_ns,frame.delivered_elapsed_ns);const delta=d.worker_delivered_elapsed_ns-d.probe.worker_ready_elapsed_ns;assert.ok(delta>=0);assert.equal(d.bounds.earliest,d.probe.parent_send+delta);assert.equal(d.bounds.latest,d.probe.parent_receive+delta);assert.equal(d.bounds.uncertainty_ns,d.probe.parent_receive-d.probe.parent_send);assert.ok(d.bounds.earliest<=d.parent_receipt&&d.parent_receipt<d.probe.parent_end&&d.bounds.latest<d.probe.parent_end);assert.ok(frame.conservative_delivery_age_ns>=d.parent_receipt-d.bounds.earliest);}
  };checkDelivery(report);
  for(const [n,frame] of report.frames.entries()){assert.equal(frame.frame,n);assert.deepEqual(frame.source_time,{numerator:n,denominator:4});assert.equal(frame.pixel_bytes,12288);assert.match(frame.pixel_sha256,/^[0-9a-f]{64}$/);}
  const chunkedPath=path.join(root,'chunked.y4m');fs.writeFileSync(chunkedPath,Buffer.concat([Buffer.from('YUV4MPEG2 W64 H64 F4:1 Ip A1:1 Cmono\n'),...Array.from({length:18},(_,n)=>Buffer.concat([Buffer.from('FRAME\n'),Buffer.alloc(4096,n*13)]))]));
  const chunked=run('chunked-intake',v=>{v.source_path=chunkedPath;v.manifest.maximum_frames=20;});assert.equal(chunked.status,0,chunked.stderr);const chunkedReport=JSON.parse(chunked.stdout);assert.equal(chunkedReport.completed,true);assert.equal(chunkedReport.frames.length,18);assert.equal(chunkedReport.process_reaped,true);assert.ok(fs.statSync(chunkedPath).size>65536);
  checkDelivery(chunkedReport);
  const aged=run('existing-lease',v=>{v.lease_elapsed_ms=1000;});assert.equal(aged.status,0,aged.stderr);const agedReport=JSON.parse(aged.stdout);checkDelivery(agedReport);assert.equal(agedReport.completed,true);assert.ok(agedReport.clock_probe.parent_send>=1000000000);assert.equal(agedReport.lease_elapsed_ms,1000);
  const delayed=run('delayed-consumer',v=>{v.idle_ms=80;});assert.equal(delayed.status,0,delayed.stderr);const delayedReport=JSON.parse(delayed.stdout);assert.equal(delayedReport.completed,true);checkDelivery(delayedReport);
  for(const [name,change] of [['cancelled',v=>{v.cancel_after_first=true;v.producer_hold_ms=500;}],['dropped-input',v=>{v.drop_input=true;}],['idle-expired',v=>{v.manifest.budget.wall_time_ms=2000;v.lease_elapsed_ms=500;v.idle_ms=2500;v.producer_hold_ms=3000;}],['frame-cap',v=>{v.manifest.maximum_frames=2;}],['input-cap',v=>{v.manifest.maximum_input_bytes=100;v.wait_for_first_frame=false;}]]){
    const result=run(name,change);assert.equal(result.status,0,result.stderr);const v=JSON.parse(result.stdout);assert.equal(v.completed,false);assert.ok(v.refused);assert.equal(v.process_reaped,true);if(name==='idle-expired')assert.equal(v.idle_process_reaped,true);
  }
  const wrong=run('wrong-tool-bytes',v=>{v.manifest.ffmpeg.identity.sha256='a'.repeat(64);});assert.notEqual(wrong.status,0);assert.match(wrong.stderr,/snapshot differs/);
  const wrongFormat=run('wrong-declared-format',v=>{v.manifest.format='nut';v.wait_for_first_frame=false;});assert.equal(wrongFormat.status,0,wrongFormat.stderr);const wrongFormatReport=JSON.parse(wrongFormat.stdout);assert.equal(wrongFormatReport.completed,false);assert.ok(wrongFormatReport.refused);assert.equal(wrongFormatReport.process_reaped,true);
  const expiredBeforeLaunch=run('expired-before-launch',v=>{v.manifest.budget.wall_time_ms=1000;v.lease_elapsed_ms=1000;});assert.notEqual(expiredBeforeLaunch.status,0);assert.match(expiredBeforeLaunch.stderr,/already expired/);
});
test('persistent confined CPU worker reuses model process and fences pixels, cancellation and idle expiry',{skip:!worker||!runtime},t=>{
  assert.ok(fs.existsSync(runner),'native live inference runner must be built');
  const root=fs.mkdtempSync(path.join(os.tmpdir(),'pulsar-live-inference-'));t.diagnostic(`retained authored persistent inference evidence: ${root}`);
  const modelPath=path.join(root,'model.onnx');fs.writeFileSync(modelPath,authoredOnnxFixture({temporal:true}));
  const model=pin(modelPath),ort=pin(runtime),binary=pin(worker);
  const input={worker_path:binary.path,manifest:{version:2,attempt_id:'placeholder',identity:{epoch:'authored-capture',project:'authored-project',revision:0,source:'authored-live-source'},clock:{epoch:'authored-capture',source_anchor:{numerator:0,denominator:1},project_anchor:0,presentation_anchor:0,added_buffer_ns:0},model,runtime:ort,worker:binary.identity,profile:{schema_version:1,model:model.identity,runtime:ort.identity,preprocessing:{input:{width:64,height:64,channels:6,class_count:1,layout:'nchw',decoder:'temporal_script_position'},resize:'nearest_stretch',color:'rgb_frame_pair',scale:Array(6).fill(Math.fround(1/255)),bias:Array(6).fill(0),padding_byte:0},provider:'cpu',minimum_memory_bytes:134217728,minimum_output_bytes:1048576,cpu_threads:2},budget:{memory_bytes:1610612736,output_bytes:1048576,wall_time_ms:30000,cpu_threads:2}}};
  // Protocol version is part of the private worker manifest, not guessed from model metadata.
  const run=(name,change=()=>{})=>{
    const value=structuredClone(input);value.manifest.attempt_id=crypto.randomUUID();change(value);
    const file=path.join(root,`${name}.json`);fs.writeFileSync(file,JSON.stringify(value));
    const result=spawnSync(runner,[file],{env:process.env,encoding:'utf8',timeout:45000,maxBuffer:1024*1024});
    fs.writeFileSync(path.join(root,`${name}.result.json`),JSON.stringify({status:result.status,error:result.error?.message,stdout:result.stdout,stderr:result.stderr},null,2));assert.equal(result.error,undefined);return result;
  };
  const normal=run('normal');assert.equal(normal.status,0,normal.stderr);const report=JSON.parse(normal.stdout);assert.equal(report.results.length,3);assert.equal(report.one_process_reused,true);assert.equal(report.process_reaped,true);assert.equal(report.physical_dispatch,false);assert.equal(report.authenticated_capture_integration,false);assert.ok(report.worker_peak_memory_bytes>0);
  // ReduceMean accumulates 24,576 float32 values; permit bounded float32 reduction error.
  for(const [i,result] of report.results.entries()) {assert.equal(result.status,'estimated');const value=result.payload;assert.equal(value.input.sequence,i+1);assert.equal(value.input.pair.current_frame,i+1);assert.deepEqual(value.identity,input.manifest.identity);assert.ok(Math.abs(value.position-(0.25+0.25*((i*64+(i+1)*64)/255)))<1e-5);assert.equal(value.input.previous.byte_len,12288);assert.equal(value.input.current.byte_len,12288);}
  const cancelled=run('cancelled',v=>{v.cancel_after_first=true;});assert.equal(cancelled.status,0,cancelled.stderr);const cancelReport=JSON.parse(cancelled.stdout);assert.equal(cancelReport.results.length,1);assert.ok(cancelReport.refused);assert.equal(cancelReport.process_reaped,true);
  const aged=run('existing-lease',v=>{v.lease_elapsed_ms=1000;});assert.equal(aged.status,0,aged.stderr);const agedReport=JSON.parse(aged.stdout);assert.equal(agedReport.results.length,3);assert.equal(agedReport.lease_elapsed_ms,1000);assert.equal(agedReport.process_reaped,true);
  // Reach readiness before testing idle expiry; cold snapshot preparation has its own deadline gate.
  const expired=run('idle-expired',v=>{v.manifest.budget.wall_time_ms=5000;v.lease_elapsed_ms=500;v.idle_ms=5000;});assert.equal(expired.status,0,expired.stderr);const expiryReport=JSON.parse(expired.stdout);assert.equal(expiryReport.results.length,1);assert.match(expiryReport.refused.message,/expired|closed/);assert.equal(expiryReport.process_reaped,true);assert.equal(expiryReport.idle_process_reaped,true);
  const expiredBeforeLaunch=run('expired-before-launch',v=>{v.manifest.budget.wall_time_ms=1000;v.lease_elapsed_ms=1000;});assert.notEqual(expiredBeforeLaunch.status,0);assert.match(expiredBeforeLaunch.stderr,/already expired/);
  const wrong=run('wrong-model-bytes',v=>{v.manifest.model.identity.sha256='a'.repeat(64);v.manifest.profile.model.sha256='a'.repeat(64);});assert.notEqual(wrong.status,0);assert.match(wrong.stderr,/snapshot differs/);
  const unsupported=run('unsupported-provider',v=>{v.manifest.profile.provider='cuda';});assert.notEqual(unsupported.status,0);assert.match(unsupported.stderr,/profile/);
});
