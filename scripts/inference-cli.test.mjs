// Shipped CLI -> authenticated engine -> isolated worker. Synthetic wiring only.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import { spawn, spawnSync } from 'node:child_process';
import { once } from 'node:events';
import { authoredOnnxFixture } from './benchmark-onnx-fixture.mjs';

const configured = process.env.PULSAR_BENCHMARK_WORKER && process.env.PULSAR_BENCHMARK_ORT;
test('CLI catalog, profile export, generation and strict rejection share the engine contract', { skip: !configured }, async t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(),'pulsar-profile-cli-'));
  t.diagnostic(`retained authored CLI evidence: ${root}`);
  const state = path.join(root,'state');
  const env = { ...process.env, PULSAR_STATE_DIR: state, PULSAR_ORT_DYLIB: process.env.PULSAR_BENCHMARK_ORT };
  const engineLog = fs.openSync(path.join(root,'engine.stderr'),'wx',0o600);
  const engine = spawn(process.env.PULSAR_BENCHMARK_WORKER,['engine'],{ env, stdio:['ignore','ignore',engineLog] });
  fs.closeSync(engineLog);
  const stopped = once(engine,'exit');
  t.after(async () => {
    if (engine.exitCode === null && engine.signalCode === null) engine.kill('SIGTERM');
    await stopped;
  });
  const deadline = Date.now() + 15000;
  while (!fs.existsSync(path.join(state,'engine.sock'))) {
    assert.equal(engine.exitCode,null,fs.readFileSync(path.join(root,'engine.stderr'),'utf8'));
    assert.ok(Date.now() < deadline,'engine startup deadline');
    await new Promise(resolve=>setTimeout(resolve,25));
  }
  let index = 0;
  const cli = args => {
    const result = spawnSync(process.env.PULSAR_BENCHMARK_WORKER,args,{env,timeout:30000,encoding:'utf8',maxBuffer:1024*1024});
    fs.writeFileSync(path.join(root,`cli-${index++}.json`),JSON.stringify({args,status:result.status,error:result.error?.message,
      stdout:result.stdout,stderr:result.stderr},null,2),{mode:0o600});
    assert.equal(result.error,undefined);
    return result;
  };
  const custodyChecks=[];
  const custodyRoster=()=>JSON.parse(fs.readFileSync(path.join(state,'worker-fence.json')));
  const observedGroups=new Map();
  const checkOwned=attempt=>{
    const ledger=custodyRoster();assert.equal(ledger.version,4);assert.deepEqual(ledger.creation.records,{});
    // Each controlled scenario has exactly one worker. Observe its recorded
    // identity rather than assuming a resource name encodes the attempt ID.
    const entries=Object.entries(ledger.groups);assert.equal(entries.length,1,`live attempt ${attempt} lacks a unique registered group`);
    const [name,group]=entries[0],prefix=`pulsar-worker-${ledger.scope}-`;
    assert.ok(name.startsWith(prefix));assert.match(name.slice(prefix.length),/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
    assert.equal(path.basename(group.path),name);const held=fs.statSync(group.path);
    assert.equal(held.dev,group.device);assert.equal(held.ino,group.inode);
    assert.match(fs.readFileSync(path.join(group.path,'cgroup.events'),'utf8'),/^populated 1$/m);
    observedGroups.set(attempt,{name,...group});custodyChecks.push({attempt,registered:true,scope:ledger.scope,group:{name,...group}});
  };
  const awaitRetired=async attempt=>{
    const group=observedGroups.get(attempt);assert.ok(group,`attempt ${attempt} had no independently observed group`);
    const deadline=Date.now()+3000;while(custodyRoster().groups[group.name]||fs.existsSync(group.path)){
      assert.ok(Date.now()<deadline,`live attempt ${attempt} not retired`);await new Promise(resolve=>setTimeout(resolve,10));
    }
    assert.deepEqual(custodyRoster().creation.records,{});custodyChecks.push({attempt,durably_cleared:true,group});
  };
  const status = cli(['model','status']);
  assert.equal(status.status,0,status.stderr);
  const catalog = JSON.parse(status.stdout);
  assert.deepEqual(catalog.providers,['cpu']);
  assert.deepEqual(catalog.qualified_profiles,[]);
  assert.equal(catalog.qualified_fallback_available,false);
  const captureProjectResult=cli(['project','create','authored-capture-lifecycle']);assert.equal(captureProjectResult.status,0,captureProjectResult.stderr);const captureProject=JSON.parse(captureProjectResult.stdout).value.project_id;
  const captureLimits=cli(['live-capture','limits',captureProject,'0']);assert.equal(captureLimits.status,0,captureLimits.stderr);const limits=JSON.parse(captureLimits.stdout).value;assert.equal(limits.maximum_active,8);assert.equal(limits.maximum_lease_ms,30000);assert.equal(limits.native_acquisition,false);assert.equal(limits.continuous_inference,true);assert.equal(limits.physical_dispatch,false);
  const captureOptions=path.join(root,'capture-options.json');const options={policy:{maximum_analysis_age_ns:200000000,added_buffer_ns:0},source_anchor:{numerator:0,denominator:1},project_anchor:0,presentation_anchor:0,lease_ms:30000};fs.writeFileSync(captureOptions,JSON.stringify(options));
  const captureStartArgs=['live-capture','start',captureProject,'0',captureOptions,'--request-id','authored-capture-start'];const captureStart=cli(captureStartArgs);assert.equal(captureStart.status,0,captureStart.stderr);const lease=JSON.parse(captureStart.stdout).value;assert.equal(lease.active,true);assert.equal(lease.received_frames,0);assert.equal(lease.admitted_pixel_bytes,61440);
  const captureStatus=cli(['live-capture','status',captureProject,'0',lease.identity.epoch]);assert.equal(captureStatus.status,0,captureStatus.stderr);assert.equal(JSON.parse(captureStatus.stdout).value.active,true);
  const captureClock=cli(['live-capture','clock',captureProject,'0',lease.identity.epoch]);assert.equal(captureClock.status,0,captureClock.stderr);const clockFacts=JSON.parse(captureClock.stdout);assert.deepEqual(clockFacts.identity,lease.identity);assert.equal(clockFacts.engine_epoch,lease.engine_epoch);assert.equal(clockFacts.basis,'local_monotonic_equal_rate');assert.equal(clockFacts.probe.producer_send_ns,0);assert.ok(clockFacts.probe.producer_receive_ns>=0&&clockFacts.probe.producer_receive_ns<=clockFacts.probe.maximum_uncertainty_ns);assert.equal(clockFacts.probe.lease_end,30000000000);assert.ok(clockFacts.probe.server_sample>=JSON.parse(captureStatus.stdout).value.server_elapsed);
  const captureReplay=cli(captureStartArgs);assert.equal(captureReplay.status,0,captureReplay.stderr);assert.equal(JSON.parse(captureReplay.stdout).value.identity.epoch,lease.identity.epoch);
  const pixels=path.join(root,'live-frame.rgb');fs.writeFileSync(pixels,Buffer.alloc(12288,7));
  const frameHeader=path.join(root,'live-frame.json');const header={frame:0,source_time:{numerator:0,denominator:1},capture_tick:0,sha256:crypto.createHash('sha256').update(fs.readFileSync(pixels)).digest('hex')};fs.writeFileSync(frameHeader,JSON.stringify(header));
  const pushArgs=['live-capture','push',captureProject,'0',lease.identity.epoch,frameHeader,pixels];
  const pushed=cli(pushArgs);assert.equal(pushed.status,0,pushed.stderr);assert.equal(JSON.parse(pushed.stdout).value.received_frames,1);assert.deepEqual(JSON.parse(pushed.stdout).value.last_frame,header);
  const retried=cli(pushArgs);assert.equal(retried.status,0,retried.stderr);assert.equal(JSON.parse(retried.stdout).value.received_frames,1);
  fs.writeFileSync(pixels,Buffer.alloc(12288,8));const nextHeader={...header,frame:1,source_time:{numerator:1,denominator:1000},capture_tick:1,sha256:crypto.createHash('sha256').update(fs.readFileSync(pixels)).digest('hex')};fs.writeFileSync(frameHeader,JSON.stringify(nextHeader));
  const secondFrame=cli(pushArgs);assert.equal(secondFrame.status,0,secondFrame.stderr);assert.equal(JSON.parse(secondFrame.stdout).value.received_frames,2);
  fs.writeFileSync(frameHeader,JSON.stringify({...nextHeader,frame:0}));const oldFrame=cli(pushArgs);assert.notEqual(oldFrame.status,0);assert.match(oldFrame.stderr,/latest live frame/);
  fs.writeFileSync(frameHeader,JSON.stringify({...nextHeader,source_time:{numerator:2,denominator:1000}}));const conflictingFrame=cli(pushArgs);assert.notEqual(conflictingFrame.status,0);assert.match(conflictingFrame.stderr,/reused with different metadata/);
  fs.writeFileSync(frameHeader,JSON.stringify(nextHeader));fs.writeFileSync(pixels,Buffer.alloc(12289,8));assert.notEqual(cli(pushArgs).status,0);fs.writeFileSync(pixels,Buffer.alloc(12288,8));
  const captureStopArgs=['live-capture','stop',captureProject,'0',lease.identity.epoch,'--request-id','authored-capture-stop'];const captureStop=cli(captureStopArgs);assert.equal(captureStop.status,0,captureStop.stderr);assert.equal(JSON.parse(captureStop.stdout).value.retirement,'stopped');assert.equal(JSON.parse(captureStop.stdout).value.admitted_pixel_bytes,0);assert.equal(cli(captureStopArgs).status,0);
  // Live-only admission arms ownership before the first ordinary generation.
  const freshEncoded=path.join(root,'fresh-encoded-options.json');fs.writeFileSync(freshEncoded,JSON.stringify({...options,encoded_source:{format:'yuv4mpeg_pipe',maximum_input_bytes:65536,maximum_frames:8}}));
  const freshLive=cli(['live-capture','start',captureProject,'0',freshEncoded,'--request-id','fresh-live-fence']);assert.equal(freshLive.status,0,freshLive.stderr);
  const freshBinding=spawnSync('/usr/bin/python3',['-c',"import sqlite3,json,sys;d=sqlite3.connect('file:'+sys.argv[1]+'?mode=ro',uri=True);print(json.dumps([d.execute('PRAGMA user_version').fetchone()[0],d.execute('SELECT scope FROM worker_fence_binding').fetchone()[0],d.execute('SELECT COUNT(*) FROM jobs').fetchone()[0]]))",path.join(state,'projects.sqlite3')],{encoding:'utf8',timeout:5000});assert.equal(freshBinding.status,0,freshBinding.stderr);const [freshSchema,freshScope,freshJobs]=JSON.parse(freshBinding.stdout);assert.equal(freshSchema,20);assert.equal(freshJobs,0);assert.equal(freshScope,custodyRoster().scope);
  const freshStop=cli(['live-capture','stop',captureProject,'0',JSON.parse(freshLive.stdout).value.identity.epoch,'--request-id','fresh-live-fence-stop']);assert.equal(freshStop.status,0,freshStop.stderr);
  const stoppedPush=cli(pushArgs);assert.notEqual(stoppedPush.status,0);assert.match(stoppedPush.stderr,/retired/);
  const stoppedStartReplay=cli(captureStartArgs);assert.equal(stoppedStartReplay.status,0,stoppedStartReplay.stderr);assert.equal(JSON.parse(stoppedStartReplay.stdout).value.active,false);
  fs.writeFileSync(captureOptions,JSON.stringify({...options,lease_ms:20000}));const changedCapture=cli(captureStartArgs);assert.notEqual(changedCapture.status,0);assert.match(changedCapture.stderr,/request identity reused/);
  fs.writeFileSync(captureOptions,JSON.stringify({...options,lease_ms:30001}));assert.notEqual(cli(['live-capture','start',captureProject,'0',captureOptions]).status,0);fs.writeFileSync(captureOptions,JSON.stringify(options));
  const model = path.join(root,'authored.onnx');
  fs.writeFileSync(model,authoredOnnxFixture());
  const declaration = cli(['model','profile',model,'--width','64','--height','64','--class-count','1']);
  assert.equal(declaration.status,0,declaration.stderr);
  const profile = JSON.parse(declaration.stdout);
  assert.equal(Object.hasOwn(profile,'cuda_resources'),false);
  if(process.env.PULSAR_SCHEMA14_RESOURCE_TEST_BINARY) {
    const historical=spawnSync(process.env.PULSAR_SCHEMA14_RESOURCE_TEST_BINARY,
      ['model','profile',model,'--width','64','--height','64','--class-count','1'],{env,timeout:30000,encoding:'utf8'});
    fs.writeFileSync(path.join(root,'historical-cpu-profile.stdout'),historical.stdout??'');
    fs.writeFileSync(path.join(root,'historical-cpu-profile.stderr'),historical.stderr??'');
    assert.equal(historical.status,0,historical.stderr);assert.equal(historical.stdout,declaration.stdout);
  }
  assert.deepEqual(profile.runtime,catalog.runtime);
  assert.equal(profile.model.sha256,crypto.createHash('sha256').update(fs.readFileSync(model)).digest('hex'));
  const profilePath = path.join(root,'profile.json');
  fs.writeFileSync(profilePath,declaration.stdout);
  const video = path.join(root,'stationary.y4m');
  const texture = Buffer.from(Array.from({length:64*64},(_,index)=>{
    const x = index % 64, y = Math.floor(index / 64);
    return (x*31+y*17+Math.floor(x/7)*Math.floor(y/5)*13)%256;
  }));
  fs.writeFileSync(video,Buffer.concat([Buffer.from('YUV4MPEG2 W64 H64 F8:1 Ip A1:1 Cmono\n'),
    ...Array.from({length:8},()=>Buffer.concat([Buffer.from('FRAME\n'),texture]))]));
  const output = path.join(root,'output.funscript');
  const args = ['generate',video,'--model',model,'--inference-profile',profilePath,'--width','64','--height','64','--output',output];
  const generated = cli(args);
  assert.equal(generated.status,0,generated.stderr);
  assert.ok(Array.isArray(JSON.parse(fs.readFileSync(output,'utf8')).actions));
  const attempts = fs.readdirSync(path.join(state,'attempts'));
  assert.equal(attempts.length,1);
  const checkpoint = JSON.parse(fs.readFileSync(path.join(state,'attempts',attempts[0],'initial-checkpoint.json'),'utf8'));
  assert.equal(checkpoint.operation.arguments.settings.inference_profile.model.sha256,profile.model.sha256);
  assert.equal(checkpoint.operation.arguments.settings.inference_profile.provider,'cpu');
  const viewPath=path.join(root,'view.json');
  const view={layout:'mono',eye:'left',projection:{kind:'half_equirectangular180',viewport:{yaw_degrees:0,pitch_degrees:0,roll_degrees:0,horizontal_fov_degrees:90,vertical_fov_degrees:90}}};
  fs.writeFileSync(viewPath,JSON.stringify(view));
  const viewOutput=path.join(root,'projected.funscript');const projected=cli([...args.slice(0,-1),viewOutput,'--video-view',viewPath]);
  assert.equal(projected.status,0,projected.stderr);assert.ok(JSON.parse(fs.readFileSync(viewOutput,'utf8')).actions.length>0);
  const newAttempts=fs.readdirSync(path.join(state,'attempts')).filter(name=>!attempts.includes(name));assert.equal(newAttempts.length,1);
  const viewCheckpoint=JSON.parse(fs.readFileSync(path.join(state,'attempts',newAttempts[0],'initial-checkpoint.json'),'utf8'));
  assert.deepEqual(viewCheckpoint.operation.arguments.settings.video_view,view);
  const viewReceipt=JSON.parse(fs.readFileSync(path.join(state,'attempts',newAttempts[0],'receipt.json'),'utf8'));
  assert.match(viewReceipt.coordinate_projection,/^video-view-v1\./);assert.equal(viewReceipt.qualification,'unqualified');
  assert.equal(viewReceipt.samples[3].project_time_ns,375000000);
  // Conflicting container audio traverses the real scoped engine and worker.
  // This authored model is a wiring fixture, not trained perception evidence.
  const pcm=Buffer.alloc(16000*4);for(let i=0;i<16000;i++)pcm.writeFloatLE(0.4,i*4);
  const audioPath=path.join(root,'mixed.f32');fs.writeFileSync(audioPath,pcm);
  const mixedVideo=path.join(root,'mixed.mkv');const mux=spawnSync('/usr/bin/ffmpeg',['-v','error','-nostdin','-i',video,'-f','f32le','-ar','16000','-ac','1','-i',audioPath,'-map','0:v:0','-map','1:a:0','-c:v','ffv1','-c:a','pcm_f32le','-threads','1',mixedVideo],{timeout:15000,encoding:'utf8'});assert.equal(mux.status,0,mux.stderr);
  const mixedPolicy=path.join(root,'mixed-policy.json');const mapping={axis:'stroke',amplitude:0.2,offset:0.4};
  fs.writeFileSync(mixedPolicy,JSON.stringify({intent:'faithful',mapping}));
  const faithfulOutput=path.join(root,'faithful.funscript');const mixedArgs=['generate',mixedVideo,'--model',model,'--inference-profile',profilePath,'--width','64','--height','64','--output',faithfulOutput,'--mixed-audio',mixedPolicy];
  const faithful=cli(mixedArgs);assert.equal(faithful.status,0,faithful.stderr);assert.deepEqual(JSON.parse(fs.readFileSync(faithfulOutput)).actions,JSON.parse(fs.readFileSync(output)).actions);
  const beforeCreative=fs.readdirSync(path.join(state,'attempts'));fs.writeFileSync(mixedPolicy,JSON.stringify({intent:'creative_audio',mapping}));mixedArgs[mixedArgs.indexOf(faithfulOutput)]=path.join(root,'creative.funscript');const creative=cli(mixedArgs);assert.equal(creative.status,0,creative.stderr);
  const creativeActions=JSON.parse(fs.readFileSync(path.join(root,'creative.funscript'))).actions;assert.ok(creativeActions.length>0);assert.ok(creativeActions.every(a=>a.pos===48));assert.deepEqual(creativeActions.map(a=>a.at),JSON.parse(fs.readFileSync(faithfulOutput)).actions.map(a=>a.at));
  const creativeAttempt=fs.readdirSync(path.join(state,'attempts')).find(name=>!beforeCreative.includes(name));const creativeReceipt=JSON.parse(fs.readFileSync(path.join(state,'attempts',creativeAttempt,'receipt.json')));assert.equal(creativeReceipt.configuration.mixed_audio.intent,'creative_audio');assert.equal(creativeReceipt.mixed_audio.audio_timeline.first_pts,0);assert.ok(creativeReceipt.samples.every(s=>s.mixed_audio.contribution==='audio' && s.evidence==='synthesized'));assert.ok(creativeReceipt.review.some(r=>r.reason==='mixed_audio_synthesized_uncalibrated'));
  fs.writeFileSync(mixedPolicy,JSON.stringify({mapping}));const unspecified=cli(mixedArgs);assert.notEqual(unspecified.status,0);assert.match(unspecified.stderr,/missing field.*intent/);
  const temporalModel=path.join(root,'temporal.onnx');fs.writeFileSync(temporalModel,authoredOnnxFixture({temporal:true}));
  const temporalDeclaration=cli(['model','profile',temporalModel,'--width','64','--height','64','--class-count','1','--temporal-script-position']);assert.equal(temporalDeclaration.status,0,temporalDeclaration.stderr);const temporalProfile=JSON.parse(temporalDeclaration.stdout);assert.equal(temporalProfile.preprocessing.input.channels,6);assert.equal(temporalProfile.preprocessing.color,'rgb_frame_pair');assert.equal(temporalProfile.preprocessing.resize,'nearest_stretch');
  const temporalProfilePath=path.join(root,'temporal-profile.json');fs.writeFileSync(temporalProfilePath,temporalDeclaration.stdout);
  const beforeTemporal=fs.readdirSync(path.join(state,'attempts'));const temporalOutput=path.join(root,'temporal.funscript');const temporalArgs=['generate',video,'--model',temporalModel,'--inference-profile',temporalProfilePath,'--output',temporalOutput];const temporal=cli(temporalArgs);assert.equal(temporal.status,0,temporal.stderr);
  const temporalAttempt=fs.readdirSync(path.join(state,'attempts')).find(name=>!beforeTemporal.includes(name));const temporalReceipt=JSON.parse(fs.readFileSync(path.join(state,'attempts',temporalAttempt,'receipt.json')));assert.equal(temporalReceipt.recipe,'temporal-reference-script-position-v1');assert.equal(temporalReceipt.analysis_dimensions[0],64);assert.equal(temporalReceipt.inference_execution.successful_runs,7);assert.equal(temporalReceipt.samples[0].evidence,'synthesized');assert.ok(temporalReceipt.samples.slice(1).every(s=>s.evidence==='inferred' && s.observed_boxes===0));assert.ok(temporalReceipt.review.some(r=>r.reason==='learned_reference_script_position_uncalibrated'));
  const badDimensions=cli([...temporalArgs,'--width','320']);assert.notEqual(badDimensions.status,0);assert.match(badDimensions.stderr,/64x64 original packed-source/);
  const wrongTemporal=cli(['model','profile',temporalModel,'--width','64','--height','64','--class-count','2','--temporal-script-position']);assert.notEqual(wrongTemporal.status,0);assert.match(wrongTemporal.stderr,/legacy class_count must be 1/);
  const rawBytes=path.join(root,'captured.rgb');fs.writeFileSync(rawBytes,Buffer.concat([Buffer.alloc(64*64*3),Buffer.alloc(64*64*3,128)]));
  const pairPath=path.join(root,'captured.json');const pair={source_epoch:'authored-epoch',previous_frame:10,current_frame:11,previous_time:{numerator:10,denominator:3},current_time:{numerator:11,denominator:3}};fs.writeFileSync(pairPath,JSON.stringify(pair));
  const rawOutput=path.join(root,'captured.funscript');const rawArgs=['generate',rawBytes,'--model',temporalModel,'--inference-profile',temporalProfilePath,'--raw-frame-pair',pairPath,'--output',rawOutput];
  const beforeRaw=fs.readdirSync(path.join(state,'attempts'));const rawResult=cli(rawArgs);assert.equal(rawResult.status,0,rawResult.stderr);
  assert.deepEqual(JSON.parse(fs.readFileSync(rawOutput)).actions,[{at:0,pos:50},{at:333,pos:38}]);
  const rawAttempt=fs.readdirSync(path.join(state,'attempts')).find(name=>!beforeRaw.includes(name));const rawReceipt=JSON.parse(fs.readFileSync(path.join(state,'attempts',rawAttempt,'receipt.json')));
  assert.equal(rawReceipt.recipe,'captured-rgb-pair-script-position-v1');assert.deepEqual(rawReceipt.source_pair,pair);assert.equal(rawReceipt.current_quantization.time,333333333);assert.equal(rawReceipt.inference_execution.successful_runs,1);assert.equal(rawReceipt.physical_dispatch,false);assert.equal(rawReceipt.continuous_live_session,false);assert.equal(rawReceipt.qualification,'unqualified');assert.ok(Math.abs(rawReceipt.position-(0.25+0.25*128/255))<1e-6);
  const presentationClock={epoch:pair.source_epoch,source_anchor:pair.previous_time,project_anchor:500000000,presentation_anchor:100000000,added_buffer_ns:1000000000};
  const presentedPair={...pair,presentation_clock:presentationClock};fs.writeFileSync(pairPath,JSON.stringify(presentedPair));
  const presentedOutput=path.join(root,'presented.funscript');const presentedArgs=[...rawArgs];presentedArgs[presentedArgs.indexOf(rawOutput)]=presentedOutput;
  const beforePresented=fs.readdirSync(path.join(state,'attempts'));const presentedResult=cli(presentedArgs);assert.equal(presentedResult.status,0,presentedResult.stderr);
  assert.deepEqual(JSON.parse(fs.readFileSync(presentedOutput)).actions,[{at:500,pos:50},{at:833,pos:38}]);
  const presentedAttempt=fs.readdirSync(path.join(state,'attempts')).find(name=>!beforePresented.includes(name));const presentedReceipt=JSON.parse(fs.readFileSync(path.join(state,'attempts',presentedAttempt,'receipt.json')));
  assert.deepEqual(presentedReceipt.source_pair,presentedPair);assert.equal(presentedReceipt.presentation_mapping[0].project.time,500000000);assert.equal(presentedReceipt.presentation_mapping[1].project.time,833333333);assert.equal(presentedReceipt.presentation_mapping[1].presentation_tick,1433333333);assert.equal(presentedReceipt.presentation_mapping[1].presentation_remainder_numerator,'1');assert.equal(presentedReceipt.presentation_mapping[1].presentation_remainder_denominator,'3');assert.equal(presentedReceipt.presentation_buffer_executed,false);
  fs.writeFileSync(pairPath,JSON.stringify({...presentedPair,presentation_clock:{...presentationClock,added_buffer_ns:0}}));const unbufferedOutput=path.join(root,'unbuffered.funscript');presentedArgs[presentedArgs.indexOf(presentedOutput)]=unbufferedOutput;assert.equal(cli(presentedArgs).status,0);assert.deepEqual(JSON.parse(fs.readFileSync(unbufferedOutput)).actions,JSON.parse(fs.readFileSync(presentedOutput)).actions);
  const clockRejected=path.join(root,'clock-rejected.funscript');presentedArgs[presentedArgs.indexOf(unbufferedOutput)]=clockRejected;
  for(const clock of [{...presentationClock,epoch:'retired-epoch'},{...presentationClock,added_buffer_ns:1000000001},{...presentationClock,project_anchor:-1}]){fs.writeFileSync(pairPath,JSON.stringify({...presentedPair,presentation_clock:clock}));assert.notEqual(cli(presentedArgs).status,0);assert.equal(fs.existsSync(clockRejected),false);}
  const rawRejected=path.join(root,'raw-rejected.funscript');rawArgs[rawArgs.indexOf(rawOutput)]=rawRejected;
  fs.writeFileSync(pairPath,JSON.stringify({...pair,current_frame:10}));assert.notEqual(cli(rawArgs).status,0);assert.equal(fs.existsSync(rawRejected),false);
  fs.writeFileSync(pairPath,JSON.stringify({...pair,current_time:{numerator:4,denominator:1}}));assert.notEqual(cli(rawArgs).status,0);assert.equal(fs.existsSync(rawRejected),false);
  fs.writeFileSync(pairPath,JSON.stringify(pair));fs.writeFileSync(rawBytes,Buffer.alloc(24));const shortPair=cli(rawArgs);assert.notEqual(shortPair.status,0);assert.match(shortPair.stderr,/raw frame-pair bytes or budget/);assert.equal(fs.existsSync(rawRejected),false);
  // Declaring the temporal contract cannot make a detector graph a regressor.
  // Authenticated lease -> actual bulk pixels -> automatic persistent inference.
  const inferOptionsPath=path.join(root,'live-inference-options.json');const inferOptions={...options,policy:{maximum_analysis_age_ns:1000000000,added_buffer_ns:0},inference:{model_path:temporalModel,profile:temporalProfile}};fs.writeFileSync(inferOptionsPath,JSON.stringify(inferOptions));
  const inferStartArgs=['live-capture','start',captureProject,'0',inferOptionsPath,'--request-id','authored-live-inference-start'];const inferStart=cli(inferStartArgs);assert.equal(inferStart.status,0,inferStart.stderr);const inferenceLease=JSON.parse(inferStart.stdout).value;assert.equal(inferenceLease.inference.state,'starting');
  const liveStatus=()=>{const result=cli(['live-capture','status',captureProject,'0',inferenceLease.identity.epoch]);assert.equal(result.status,0,result.stderr);return JSON.parse(result.stdout).value;};
  const waitLive=async predicate=>{const until=Date.now()+10000;for(;;){const report=liveStatus();assert.equal(report.active,true,JSON.stringify(report.inference));if(predicate(report))return report;assert.ok(Date.now()<until,'live inference deadline');await new Promise(resolve=>setTimeout(resolve,10));}};
  let live=await waitLive(report=>report.inference.state==='running');const attempt=live.inference.attempt_id;checkOwned(attempt);
  for(let frame=0;frame<4;frame++) {
    live=liveStatus();fs.writeFileSync(pixels,Buffer.alloc(12288,frame*64));const metadata={frame,source_time:{numerator:frame,denominator:10},capture_tick:live.server_elapsed,sha256:crypto.createHash('sha256').update(fs.readFileSync(pixels)).digest('hex')};fs.writeFileSync(frameHeader,JSON.stringify(metadata));
    const push=cli(['live-capture','push',captureProject,'0',inferenceLease.identity.epoch,frameHeader,pixels]);assert.equal(push.status,0,push.stderr);
    if(frame>0) {live=await waitLive(report=>report.inference.latest?.result.payload.input.pair.current_frame===frame);const result=live.inference.latest.result.payload;assert.equal(result.attempt_id,attempt);assert.deepEqual(result.identity,inferenceLease.identity);assert.equal(live.inference.latest.fresh,true);assert.equal(live.inference.latest.evidence,'inferred');assert.equal(live.inference.latest.qualified,false);assert.ok(live.inference.latest.capture_to_analysis_ns<=inferOptions.policy.maximum_analysis_age_ns);assert.ok(Math.abs(result.position-(0.25+0.25*((frame-1)*64+frame*64)/255))<1e-5);assert.equal(live.queue.in_flight_frames,0);}
  }
  assert.equal(live.inference.accepted,3);assert.equal(live.inference.completed,3);assert.equal(live.received_frames,4);
  const inferenceReplay=cli(inferStartArgs);assert.equal(inferenceReplay.status,0,inferenceReplay.stderr);assert.equal(JSON.parse(inferenceReplay.stdout).value.inference.attempt_id,attempt);
  await new Promise(resolve=>setTimeout(resolve,1100));const aged=liveStatus();assert.equal(aged.inference.latest.fresh,false);assert.equal(aged.inference.accepted,3);
  const inferStop=cli(['live-capture','stop',captureProject,'0',inferenceLease.identity.epoch]);assert.equal(inferStop.status,0,inferStop.stderr);const stoppedInference=JSON.parse(inferStop.stdout).value;assert.equal(stoppedInference.inference.state,'retired');assert.equal(stoppedInference.inference.latest,null);assert.equal(stoppedInference.queue,undefined);assert.equal(JSON.parse(cli(inferStartArgs).stdout).value.active,false);
  await awaitRetired(attempt);
  const badInference={...inferOptions,inference:{...inferOptions.inference,profile:structuredClone(temporalProfile)}};badInference.inference.profile.model.sha256='a'.repeat(64);fs.writeFileSync(inferOptionsPath,JSON.stringify(badInference));const badStart=cli(['live-capture','start',captureProject,'0',inferOptionsPath]);assert.equal(badStart.status,0,badStart.stderr);const badLease=JSON.parse(badStart.stdout).value;const failDeadline=Date.now()+10000;let failed;
  for(;;){const response=cli(['live-capture','status',captureProject,'0',badLease.identity.epoch]);assert.equal(response.status,0,response.stderr);failed=JSON.parse(response.stdout).value;if(!failed.active)break;assert.ok(Date.now()<failDeadline);await new Promise(resolve=>setTimeout(resolve,10));}
  assert.equal(failed.retirement,'worker_failed');assert.equal(failed.inference.state,'failed');assert.equal(failed.inference.latest,null);assert.match(failed.inference.error.message,/snapshot differs/);assert.equal(failed.admitted_pixel_bytes,0);
  assert.equal(limits.encoded_source_ingress,true);
  const encodedPath=path.join(root,'encoded-source.y4m');const encodedBytes=Buffer.concat([Buffer.from('YUV4MPEG2 W64 H64 F4:1 Ip A1:1 Cmono\n'),...Array.from([0,128,255],v=>Buffer.concat([Buffer.from('FRAME\n'),Buffer.alloc(4096,v)]))]);fs.writeFileSync(encodedPath,encodedBytes);
  const encodedOptionsPath=path.join(root,'encoded-options.json');
  const encodedOptions={...options,policy:{maximum_analysis_age_ns:1000000000,added_buffer_ns:0},encoded_source:{format:'yuv4mpeg_pipe',maximum_input_bytes:1048576,maximum_frames:20}};
  const startEncoded=(name,changes={})=>{fs.writeFileSync(encodedOptionsPath,JSON.stringify({...encodedOptions,...changes}));const args=['live-capture','start',captureProject,'0',encodedOptionsPath,'--request-id',`authored-encoded-${name}`];const result=cli(args);assert.equal(result.status,0,result.stderr);const report=JSON.parse(result.stdout).value;assert.equal(report.age_basis,'decoded_stdout_local_monotonic_equal_rate');assert.equal(report.encoded_source.state,'pending');return {report,args};};
  const sourceStatus=epoch=>{const r=cli(['live-capture','status',captureProject,'0',epoch]);assert.equal(r.status,0,r.stderr);return JSON.parse(r.stdout).value;};
  const sourceStop=epoch=>{const r=cli(['live-capture','stop',captureProject,'0',epoch]);assert.equal(r.status,0,r.stderr);return JSON.parse(r.stdout).value;};
  const nativeSourceRunner=path.join(path.dirname(process.env.PULSAR_BENCHMARK_WORKER),'examples/encoded-capture-fixture');assert.ok(fs.existsSync(nativeSourceRunner));
  for(const mode of ['normal','drop','stop','expire','continue']){
    const {report,args}=startEncoded(mode,mode==='expire'?{lease_ms:2000}:{});
    const input=path.join(root,`encoded-${mode}.json`);fs.writeFileSync(input,JSON.stringify({state,project:captureProject,revision:0,epoch:report.identity.epoch,source:encodedPath,mode}));
    const r=spawnSync(nativeSourceRunner,[input],{env,encoding:'utf8',timeout:15000,maxBuffer:1048576});fs.writeFileSync(path.join(root,`encoded-${mode}.result.json`),JSON.stringify({status:r.status,error:r.error?.message,stdout:r.stdout,stderr:r.stderr},null,2));assert.equal(r.error,undefined);assert.equal(r.status,0,r.stderr);const receipt=JSON.parse(r.stdout);
    assert.equal(receipt.before_eof.received_frames,3);assert.equal(receipt.before_eof.encoded_source.state,'running');assert.equal(receipt.before_eof.encoded_source.completion,null);assert.equal(receipt.before_eof.age_basis,'decoded_stdout_local_monotonic_equal_rate');assert.equal(receipt.physical_dispatch,false);
    const final=receipt.final;assert.equal(final.received_frames,3);assert.equal(final.last_frame.capture_tick,final.encoded_source.latest_delivery.bounds.earliest);assert.equal(final.last_frame.sha256,final.encoded_source.latest_delivery.pixels.sha256);assert.equal(final.encoded_source.latest_delivery.probe.parent_end,mode==='expire'?2000000000:30000000000);
    if(mode==='normal'){
      assert.equal(final.active,true);assert.equal(final.encoded_source.state,'finished');assert.equal(final.encoded_source.completion.input_bytes,encodedBytes.length);assert.equal(final.encoded_source.completion.input_sha256,crypto.createHash('sha256').update(encodedBytes).digest('hex'));assert.deepEqual(final.encoded_source.completion.identity,report.identity);
      const duplicate=cli(['live-capture','encoded',captureProject,'0',report.identity.epoch,encodedPath]);assert.notEqual(duplicate.status,0);assert.match(duplicate.stderr,/already attached|no rearm/);assert.equal(sourceStatus(report.identity.epoch).encoded_source.state,'finished');
      assert.notEqual(cli(['live-capture','clock',captureProject,'0',report.identity.epoch]).status,0);assert.equal(sourceStatus(report.identity.epoch).active,true);sourceStop(report.identity.epoch);
    }else if(mode==='continue'){assert.equal(final.active,false);assert.equal(final.retirement,'superseded');assert.equal(receipt.successor.active,true);assert.equal(receipt.successor.identity.source,report.identity.source);assert.equal(receipt.successor.received_frames,0);assert.equal(receipt.successor.encoded_source.state,'pending');
    }else{assert.equal(final.active,false);assert.equal(final.admitted_pixel_bytes,0);assert.equal(final.retirement,mode==='drop'?'worker_failed':mode==='stop'?'stopped':'expired');assert.equal(final.encoded_source.state,mode==='drop'?'failed':'retired');assert.equal(final.encoded_source.completion,null);}
    const replay=cli(args);assert.equal(replay.status,0,replay.stderr);assert.equal(JSON.parse(replay.stdout).value.active,false);
  }
  for(const mode of ['raw-auth','raw-boot','raw-source','raw-revision','raw-hash','raw-sequence','raw-oversize','raw-duplicate']){
    const {report}=startEncoded(mode);const file=path.join(root,`encoded-${mode}.json`);fs.writeFileSync(file,JSON.stringify({state,project:captureProject,revision:0,epoch:report.identity.epoch,source:encodedPath,mode}));
    const r=spawnSync(nativeSourceRunner,[file],{env,encoding:'utf8',timeout:15000,maxBuffer:1048576});fs.writeFileSync(path.join(root,`encoded-${mode}.result.json`),JSON.stringify({status:r.status,error:r.error?.message,stdout:r.stdout,stderr:r.stderr},null,2));assert.equal(r.error,undefined);assert.equal(r.status,0,r.stderr);const receipt=JSON.parse(r.stdout);assert.equal(receipt.reply.status,'error');assert.equal(receipt.final.received_frames,0);assert.equal(receipt.final.encoded_source.input_bytes,0);assert.equal(receipt.final.encoded_source.completion,null);
    if(['raw-auth','raw-boot','raw-source','raw-revision'].includes(mode)){assert.equal(receipt.final.active,true);assert.equal(receipt.final.encoded_source.state,'pending');assert.equal(receipt.final.encoded_source.manifest,null);}else{assert.equal(receipt.final.active,false);assert.equal(receipt.final.encoded_source.state,'failed');}
    assert.equal(receipt.refused_before_body,mode!=='raw-hash');
  }
  // Actual shipped stdin streaming publishes while the producer remains open.
  for(const mode of ['finish','stop-idle','expire-idle','empty','oversize','malformed']){
    const changes=mode==='expire-idle'?{lease_ms:2000}:mode==='oversize'?{encoded_source:{...encodedOptions.encoded_source,maximum_input_bytes:100}}:{};
    const {report}=startEncoded(`stdin-${mode}`,changes);
    const streamArgs=['live-capture','stream',captureProject,'0',report.identity.epoch];
    // Node's built-in 'pipe' uses a Unix socket pair. Exercise a real FIFO
    // descriptor for the supported pipe path without granting socket input.
    const fifo=path.join(root,`stream-${mode}.fifo`);const mkfifo=spawnSync('/usr/bin/mkfifo',[fifo],{encoding:'utf8'});assert.equal(mkfifo.status,0,mkfifo.stderr);
    const reader=fs.openSync(fifo,fs.constants.O_RDONLY|fs.constants.O_NONBLOCK);let writer=fs.openSync(fifo,fs.constants.O_WRONLY);
    const input={writableEnded:false,write(bytes){fs.writeSync(writer,bytes);},end(bytes){if(bytes)this.write(bytes);fs.closeSync(writer);writer=null;this.writableEnded=true;},destroy(){if(writer!==null){fs.closeSync(writer);writer=null;}this.writableEnded=true;}};
    const child=spawn(process.env.PULSAR_BENCHMARK_WORKER,streamArgs,{env,stdio:[reader,'pipe','pipe']});fs.closeSync(reader);
    let stdout='',stderr='';child.stdout.setEncoding('utf8');child.stderr.setEncoding('utf8');child.stdout.on('data',s=>{stdout+=s;});child.stderr.on('data',s=>{stderr+=s;});
    const terminal=once(child,'exit');let watchdog=setTimeout(()=>child.kill('SIGTERM'),10000);
    try{
      if(['finish','stop-idle'].includes(mode)){
        input.write(encodedBytes);const deadline=Date.now()+5000;let observed;
        for(;;){observed=sourceStatus(report.identity.epoch);assert.equal(observed.active,true);if(observed.received_frames===3)break;assert.ok(Date.now()<deadline,stderr);await new Promise(resolve=>setTimeout(resolve,10));}
        assert.equal(child.exitCode,null);assert.equal(input.writableEnded,false);assert.equal(observed.encoded_source.completion,null);checkOwned(observed.encoded_source.manifest.attempt_id);fs.writeFileSync(path.join(root,`stream-${mode}-before-eof.json`),JSON.stringify(observed,null,2));
        if(mode==='finish')input.end();else sourceStop(report.identity.epoch);
      }else if(mode==='expire-idle'){
        // No data or EOF arrives. The CLI must retire on its bounded lifetime.
      }else input.end(mode==='empty'?undefined:mode==='oversize'?Buffer.alloc(101):Buffer.from('authored malformed encoded input'));
      const [code,signal]=await terminal;assert.equal(signal,null,stderr);
      if(['finish','stop-idle'].includes(mode)){const completedSource=sourceStatus(report.identity.epoch);await awaitRetired(completedSource.encoded_source.manifest.attempt_id);}
      if(mode==='finish'){assert.equal(code,0,stderr);const completed=JSON.parse(stdout).value;assert.equal(completed.received_frames,3);assert.equal(completed.encoded_source.completion.input_sha256,crypto.createHash('sha256').update(encodedBytes).digest('hex'));assert.equal(completed.encoded_source.completion.input_bytes,encodedBytes.length);sourceStop(report.identity.epoch);}
      else {assert.notEqual(code,0,stdout);if(mode==='stop-idle')assert.match(stderr,/retired/);if(mode==='expire-idle')assert.match(stderr,/expired|deadline|timed out/i);if(mode==='empty')assert.match(stderr,/empty/);if(mode==='oversize')assert.match(stderr,/exceeds admitted byte/);}
      const deadline=Date.now()+5000;let final;
      for(;;){final=sourceStatus(report.identity.epoch);if(!final.active)break;assert.ok(Date.now()<deadline);await new Promise(resolve=>setTimeout(resolve,10));}
      assert.equal(final.admitted_pixel_bytes,0);assert.equal(final.inference,undefined);if(mode==='oversize')assert.equal(final.encoded_source.input_bytes,0);
      fs.writeFileSync(path.join(root,`stream-${mode}.result.json`),JSON.stringify({args:streamArgs,status:code,signal,stdout,stderr,final,physical_dispatch:false},null,2));
    }finally{clearTimeout(watchdog);input.destroy();if(child.exitCode===null&&child.signalCode===null){child.kill('SIGTERM');await terminal;}}
  }
  const {report:refusedStdin}=startEncoded('stdin-device-refusal');
  const refusedInput=spawnSync(process.env.PULSAR_BENCHMARK_WORKER,['live-capture','stream',captureProject,'0',refusedStdin.identity.epoch],{env,stdio:['ignore','pipe','pipe'],encoding:'utf8',timeout:5000});assert.notEqual(refusedInput.status,0);assert.match(refusedInput.stderr,/pipe or regular file/);assert.equal(sourceStatus(refusedStdin.identity.epoch).encoded_source.state,'pending');sourceStop(refusedStdin.identity.epoch);
  fs.writeFileSync(path.join(root,'stream-device-refusal.result.json'),JSON.stringify({status:refusedInput.status,stdout:refusedInput.stdout,stderr:refusedInput.stderr,source_claimed:false,physical_dispatch:false},null,2));
  const {report:encodedInference}=startEncoded('inference',{inference:inferOptions.inference});
  const readyDeadline=Date.now()+10000;while(sourceStatus(encodedInference.identity.epoch).inference.state!=='running'){assert.ok(Date.now()<readyDeadline);await new Promise(resolve=>setTimeout(resolve,10));}
  const captured=cli(['live-capture','encoded',captureProject,'0',encodedInference.identity.epoch,encodedPath]);assert.equal(captured.status,0,captured.stderr);const capturedSource=JSON.parse(captured.stdout).value;assert.equal(capturedSource.encoded_source.state,'finished');assert.equal(capturedSource.received_frames,3);
  const analysisDeadline=Date.now()+10000;let analysed;
  for(;;){analysed=sourceStatus(encodedInference.identity.epoch);if(analysed.inference.latest?.result.payload.input.pair.current_frame===2)break;assert.ok(Date.now()<analysisDeadline,JSON.stringify(analysed));await new Promise(resolve=>setTimeout(resolve,10));}
  assert.equal(analysed.inference.latest.age_basis,'decoded_stdout_local_monotonic_equal_rate');assert.equal(analysed.inference.latest.qualified,false);assert.equal(analysed.inference.latest.evidence,'inferred');assert.equal(analysed.inference.latest.capture_tick,analysed.encoded_source.latest_delivery.bounds.earliest);assert.equal(analysed.inference.latest.result.payload.input.current.sha256,analysed.last_frame.sha256);sourceStop(encodedInference.identity.epoch);
  fs.writeFileSync(path.join(root,'live-custody-checks.json'),JSON.stringify({fresh_live_only_schema:20,fresh_live_only_jobs:0,checks:custodyChecks},null,2));
  // Fractional-rate handoff preserves exact project phase under fresh native attempts.
  const fractionalSource=path.join(root,'fractional-source.y4m');fs.writeFileSync(fractionalSource,Buffer.concat([Buffer.from('YUV4MPEG2 W64 H64 F30:1 Ip A1:1 Cmono\n'),...Array.from([0,128,255],v=>Buffer.concat([Buffer.from('FRAME\n'),Buffer.alloc(4096,v)]))]));
  const nutSegments=[path.join(root,'continuation-first.nut'),path.join(root,'continuation-second.nut')];
  for(let i=0;i<2;i++){
    const r=spawnSync('/usr/bin/ffmpeg',['-v','error','-nostdin','-i',fractionalSource,'-copyts','-an','-vf',`setpts=PTS+${i*4}`,'-fps_mode','passthrough','-enc_time_base','demux','-c:v','rawvideo','-pix_fmt','rgb24','-threads','1','-f','nut',nutSegments[i]],{encoding:'utf8',timeout:15000});assert.equal(r.status,0,r.stderr);
  }
  const continuationChanges={encoded_source:{...encodedOptions.encoded_source,format:'nut'},inference:inferOptions.inference};
  const {report:continuationPrior,args:continuationPriorArgs}=startEncoded('continuation',continuationChanges);
  const awaitRunning=async epoch=>{const deadline=Date.now()+10000;for(;;){const report=sourceStatus(epoch);assert.equal(report.active,true);if(report.inference.state==='running')return report;assert.ok(Date.now()<deadline,JSON.stringify(report));await new Promise(resolve=>setTimeout(resolve,10));}};
  const awaitAnalysed=async epoch=>{const deadline=Date.now()+10000;for(;;){const report=sourceStatus(epoch);assert.equal(report.active,true);if(report.inference.latest?.result.payload.input.pair.current_frame===2)return report;assert.ok(Date.now()<deadline,JSON.stringify(report));await new Promise(resolve=>setTimeout(resolve,10));}};
  await awaitRunning(continuationPrior.identity.epoch);
  const firstSegment=cli(['live-capture','encoded',captureProject,'0',continuationPrior.identity.epoch,nutSegments[0]]);assert.equal(firstSegment.status,0,firstSegment.stderr);
  const beforeHandoff=await awaitAnalysed(continuationPrior.identity.epoch);assert.equal(beforeHandoff.last_frame.source_time.numerator/beforeHandoff.last_frame.source_time.denominator,2/30);
  const continuationAnchorPath=path.join(root,'continuation-anchor.json');fs.writeFileSync(continuationAnchorPath,JSON.stringify({numerator:4,denominator:30}));
  const continueArgs=['live-capture','continue-at',captureProject,'0',continuationPrior.identity.epoch,continuationAnchorPath,'--request-id','authored-epoch-continuation'];
  const handoff=cli(continueArgs);assert.equal(handoff.status,0,handoff.stderr);const successor=JSON.parse(handoff.stdout).value;assert.deepEqual(successor.clock.exact_project_anchor,{numerator:'2',denominator:'15'});assert.equal(successor.clock.project_anchor,133333333);assert.equal(successor.identity.source,continuationPrior.identity.source);assert.notEqual(successor.identity.epoch,continuationPrior.identity.epoch);assert.deepEqual(successor.continuation.predecessor,continuationPrior.identity);assert.equal(successor.received_frames,0);assert.equal(successor.encoded_source.state,'pending');assert.equal(successor.inference.latest,null);
  const retiredPrior=sourceStatus(continuationPrior.identity.epoch);assert.equal(retiredPrior.retirement,'superseded');assert.equal(retiredPrior.active,false);assert.equal(retiredPrior.inference.latest,null);assert.equal(retiredPrior.admitted_pixel_bytes,0);
  assert.notEqual(cli(['live-capture','encoded',captureProject,'0',continuationPrior.identity.epoch,nutSegments[1]]).status,0);
  await awaitRunning(successor.identity.epoch);
  const secondSegment=cli(['live-capture','encoded',captureProject,'0',successor.identity.epoch,nutSegments[1]]);assert.equal(secondSegment.status,0,secondSegment.stderr);
  const afterHandoff=await awaitAnalysed(successor.identity.epoch);assert.equal(afterHandoff.last_frame.source_time.numerator/afterHandoff.last_frame.source_time.denominator,6/30);assert.equal(afterHandoff.inference.latest.result.payload.identity.epoch,successor.identity.epoch);assert.equal(afterHandoff.inference.latest.result.payload.input.pair.previous_time.numerator/afterHandoff.inference.latest.result.payload.input.pair.previous_time.denominator,5/30);assert.equal(afterHandoff.inference.latest.result.payload.input.pair.presentation_clock.project_anchor,133333333);assert.deepEqual(afterHandoff.inference.latest.result.payload.input.pair.presentation_clock.exact_project_anchor,{numerator:'2',denominator:'15'});assert.equal(afterHandoff.encoded_source.completion.identity.epoch,successor.identity.epoch);
  const handoffReplay=cli(continueArgs);assert.equal(handoffReplay.status,0,handoffReplay.stderr);assert.equal(JSON.parse(handoffReplay.stdout).value.received_frames,3);assert.equal(JSON.parse(handoffReplay.stdout).value.identity.epoch,successor.identity.epoch);
  assert.notEqual(cli([...continueArgs,'--lease-ms','20000']).status,0);assert.equal(sourceStatus(successor.identity.epoch).active,true);
  // The same exact anchor also traverses finite captured-pair generation and export.
  const fractionalPairPath=path.join(root,'fractional-pair.json');fs.writeFileSync(fractionalPairPath,JSON.stringify(afterHandoff.inference.latest.result.payload.input.pair));
  const fractionalPixelsPath=path.join(root,'fractional-pair.rgb');fs.writeFileSync(fractionalPixelsPath,Buffer.concat([Buffer.alloc(12288,128),Buffer.alloc(12288,255)]));
  const fractionalOutput=path.join(root,'fractional.funscript');const fractionalBefore=fs.readdirSync(path.join(state,'attempts'));
  const fractionalGenerated=cli(['generate',fractionalPixelsPath,'--model',temporalModel,'--inference-profile',temporalProfilePath,'--raw-frame-pair',fractionalPairPath,'--output',fractionalOutput]);assert.equal(fractionalGenerated.status,0,fractionalGenerated.stderr);assert.deepEqual(JSON.parse(fs.readFileSync(fractionalOutput)).actions.map(a=>a.at),[167,200]);
  const fractionalAttempt=fs.readdirSync(path.join(state,'attempts')).find(name=>!fractionalBefore.includes(name));const fractionalReceipt=JSON.parse(fs.readFileSync(path.join(state,'attempts',fractionalAttempt,'receipt.json')));fs.writeFileSync(path.join(root,'fractional-generation-receipt.json'),JSON.stringify(fractionalReceipt,null,2));assert.deepEqual(fractionalReceipt.project_action_times_ns,[166666666,200000000]);assert.equal(fractionalReceipt.presentation_mapping[1].project.remainder_numerator,'0');
  sourceStop(successor.identity.epoch);assert.equal(JSON.parse(cli(continueArgs).stdout).value.active,false);assert.equal(JSON.parse(cli(continuationPriorArgs).stdout).value.retirement,'superseded');
  assert.notEqual(cli([...continueArgs.slice(0,-2)]).status,0);
  const {report:wrongFormatSource}=startEncoded('wrong-format',{encoded_source:{...encodedOptions.encoded_source,format:'nut'}});assert.notEqual(cli(['live-capture','encoded',captureProject,'0',wrongFormatSource.identity.epoch,encodedPath]).status,0);const broken=sourceStatus(wrongFormatSource.identity.epoch);assert.equal(broken.active,false);assert.equal(broken.encoded_source.state,'failed');assert.equal(broken.encoded_source.completion,null);
  const wrongGraphProfile=cli(['model','profile',model,'--width','64','--height','64','--class-count','1','--temporal-script-position']);assert.equal(wrongGraphProfile.status,0,wrongGraphProfile.stderr);fs.writeFileSync(temporalProfilePath,wrongGraphProfile.stdout);const wrongGraph=cli(['generate',video,'--model',model,'--inference-profile',temporalProfilePath,'--output',path.join(root,'wrong-temporal.funscript')]);assert.notEqual(wrongGraph.status,0);assert.equal(fs.existsSync(path.join(root,'wrong-temporal.funscript')),false);
  profile.provider = 'cuda';
  fs.writeFileSync(profilePath,JSON.stringify(profile));
  args[args.length-1] = path.join(root,'rejected.funscript');
  const rejected = cli(args);
  assert.notEqual(rejected.status,0);
  assert.match(rejected.stderr,/unsupported pinned provider/);
  assert.equal(fs.existsSync(args[args.length-1]),false);
  const cudaV2=structuredClone(profile);cudaV2.schema_version=2;
  cudaV2.cuda_resources={device_ordinal:0,device_uuid:'GPU-1790118a-a6d7-4eaf-fcac-dcacac5f4351',
    address_space_bytes:17179869184,device_reservation_bytes:1073741824,arena_limit_bytes:67108864};
  fs.writeFileSync(profilePath,JSON.stringify(cudaV2));
  const jobCount=()=>{
    const r=spawnSync('/usr/bin/python3',['-c','import sqlite3,sys; print(sqlite3.connect("file:"+sys.argv[1]+"?mode=ro",uri=True).execute("SELECT COUNT(*) FROM jobs").fetchone()[0])',path.join(state,'projects.sqlite3')],{timeout:10000,encoding:'utf8'});
    assert.equal(r.status,0,r.stderr);return Number(r.stdout.trim());
  };
  const beforeCuda=jobCount();const newCudaRejected=cli(args);
  assert.notEqual(newCudaRejected.status,0);assert.match(newCudaRejected.stderr,/unsupported pinned provider/);
  assert.equal(fs.existsSync(args[args.length-1]),false);assert.equal(jobCount(),beforeCuda);
  if(process.env.PULSAR_SCHEMA14_RESOURCE_TEST_BINARY) {
    const historical=spawnSync(process.env.PULSAR_SCHEMA14_RESOURCE_TEST_BINARY,args,{env,timeout:30000,encoding:'utf8'});
    fs.writeFileSync(path.join(root,'historical-cuda-profile-refusal.stdout'),historical.stdout??'');
    fs.writeFileSync(path.join(root,'historical-cuda-profile-refusal.stderr'),historical.stderr??'');
    assert.notEqual(historical.status,0);assert.match(historical.stderr,/cuda_resources|unknown field/);
    assert.equal(jobCount(),beforeCuda);
  }
});
