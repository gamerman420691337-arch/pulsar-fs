import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import {spawnSync} from 'node:child_process';
import {admitCampaign,runCampaign} from './reference-worker-campaign.mjs';
import {authoredOnnxFixture} from './benchmark-onnx-fixture.mjs';
const sha=b=>crypto.createHash('sha256').update(b).digest('hex');
const write=(p,v)=>fs.writeFileSync(p,JSON.stringify(v),{mode:0o600});

test('development campaign binds subjects, model, window and immutable case bytes',()=>{
  const root=fs.mkdtempSync(path.join(os.tmpdir(),'pulsar-development-admission-'));fs.chmodSync(root,0o700);
  const id='development-clip-00',input={subject:id,model:{identity:{sha256:'a'.repeat(64)}},window_start_ns:'0',window_end_ns:'1000000000'};
  const reference={schema_version:1,subject_id:id,window:{start_ns:'0',end_ns:'1000000000'},strokes:[],material_sections:[]};
  write(path.join(root,'input.json'),input);write(path.join(root,'reference.json'),reference);
  const entry={id,input:'input.json',input_sha256:sha(fs.readFileSync(path.join(root,'input.json'))),reference:'reference.json',reference_sha256:sha(fs.readFileSync(path.join(root,'reference.json'))),expected_runs:2};
  const manifest={schema_version:1,qualification:'unqualified',training_authorized:true,sterile_inputs_used:false,model_retrained:false,model_sha256:'a'.repeat(64),cases:[entry]};
  assert.equal(admitCampaign(root,manifest).length,1);
  for(const change of [{sterile_inputs_used:true},{model_retrained:true},{training_authorized:false},{model_sha256:'b'.repeat(64)},{cases:[entry,entry]}])assert.throws(()=>admitCampaign(root,{...manifest,...change}));
  assert.throws(()=>admitCampaign(root,{...manifest,cases:[{...entry,input:'../input.json'}]}),/contained/);
  write(path.join(root,'input.json'),{...input,window_end_ns:'2000000000'});assert.throws(()=>admitCampaign(root,manifest),/bytes changed/);
});

test('authored frozen temporal model traverses confined worker and independent development scoring',
  {skip:!process.env.PULSAR_BENCHMARK_WORKER||!process.env.PULSAR_BENCHMARK_RUNNER||!process.env.PULSAR_BENCHMARK_ORT,timeout:30000},async t=>{
    const root=fs.mkdtempSync(path.join(os.tmpdir(),'pulsar-development-worker-'));fs.chmodSync(root,0o700);t.diagnostic(`retained authored development worker fixture: ${root}`);
    const worker=fs.realpathSync(process.env.PULSAR_BENCHMARK_WORKER),runner=fs.realpathSync(process.env.PULSAR_BENCHMARK_RUNNER),runtime=fs.realpathSync(process.env.PULSAR_BENCHMARK_ORT);
    const identity=p=>({artifact_id:sha(fs.readFileSync(p)),sha256:sha(fs.readFileSync(p)),byte_len:fs.statSync(p).size});
    const model=path.join(root,'model.onnx');fs.writeFileSync(model,authoredOnnxFixture({temporal:true}),{mode:0o400});
    const id='development-clip-00',directory=path.join(root,id);fs.mkdirSync(directory,{mode:0o700});
    const raw=path.join(directory,'frames.rgb');fs.writeFileSync(raw,Buffer.alloc(4*12288,80),{mode:0o400});const source=path.join(directory,'source.nut');
    const encoded=spawnSync('/usr/bin/ffmpeg',['-v','error','-nostdin','-f','rawvideo','-pixel_format','rgb24','-video_size','64x64','-framerate','8','-i',raw,'-vf','setpts=PTS+1/TB','-c:v','rawvideo','-threads','1','-f','nut',source],{timeout:10000,encoding:'utf8'});assert.equal(encoded.status,0,encoded.stderr);
    const artifacts={worker:identity(worker),runner:identity(runner),runtime:identity(runtime),model:identity(model),ffmpeg:identity('/usr/bin/ffmpeg'),ffprobe:identity('/usr/bin/ffprobe')};
    const abi={width:64,height:64,channels:6,layout:'nchw',class_count:1,decoder:'temporal_script_position'};
    const profile={schema_version:1,model:artifacts.model,runtime:artifacts.runtime,preprocessing:{input:abi,resize:'nearest_stretch',color:'rgb_frame_pair',scale:Array(6).fill(1/255),bias:Array(6).fill(0),padding_byte:0},provider:'cpu',minimum_memory_bytes:134217728,minimum_output_bytes:1048576,cpu_threads:2};
    const input={worker_path:worker,worker_identity:artifacts.worker,source:{source_version:id,path:source,identity:identity(source)},tools:{ffmpeg:{path:'/usr/bin/ffmpeg',identity:artifacts.ffmpeg},ffprobe:{path:'/usr/bin/ffprobe',identity:artifacts.ffprobe},onnx_runtime:{path:runtime,identity:artifacts.runtime}},
      model:{source_version:'authored-model',path:model,identity:artifacts.model},settings:{analysis_width:64,analysis_height:64,source_sample_stride:1,axis:'stroke',vr_mode:false,model_input:abi,inference_profile:profile},profile,preset:'fast',budget:{memory_bytes:1610612736,output_bytes:16777216,wall_time_ms:10000,cpu_threads:2},subject:id,target:'declared-script',window_start_ns:'0',window_end_ns:'375000000',evaluation_window_policy:'recorded_actions'};
    const reference={schema_version:1,subject_id:id,window:{start_ns:'0',end_ns:'375000000'},strokes:[{id:'authored-edge',target_id:'declared-script',axis:'stroke',start_ns:'125000000',end_ns:'375000000',start_position_ppm:100000,end_position_ppm:900000}],material_sections:[]};
    write(path.join(directory,'input.json'),input);write(path.join(directory,'reference.json'),reference);
    const manifest={schema_version:1,scope:'authored model wiring; no perception or media qualification',qualification:'unqualified',training_authorized:true,sterile_inputs_used:false,model_retrained:false,model_sha256:artifacts.model.sha256,runner:{path:runner,identity:artifacts.runner},artifacts,limitations:['Authored wiring only.'],cases:[{id,declared_group:'authored',input:`${id}/input.json`,input_sha256:sha(fs.readFileSync(path.join(directory,'input.json'))),reference:`${id}/reference.json`,reference_sha256:sha(fs.readFileSync(path.join(directory,'reference.json'))),expected_runs:3}]};write(path.join(root,'campaign.json'),manifest);
    const report=await runCampaign(root);assert.equal(report.state,'measured');assert.equal(report.results[0].worker_runs,3);assert.equal(report.results[0].score.axes.stroke.true_positive_strokes,0);
    assert.equal(report.results[0].source_origin_pts>0,true);assert.equal(report.production_activation,false);
    const retained=fs.readFileSync(path.join(root,'report.json'));await assert.rejects(runCampaign(root),/EEXIST/);assert.deepEqual(fs.readFileSync(path.join(root,'report.json')),retained);
  });
