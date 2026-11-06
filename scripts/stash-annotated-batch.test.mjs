import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import http from 'node:http';
import crypto from 'node:crypto';
import {spawn} from 'node:child_process';
import {setTimeout as delay} from 'node:timers/promises';

const enabled=process.env.PULSAR_STASH_TEST_BINARY && process.env.PULSAR_ANNOTATED_MODEL_FIXTURE_DIR && process.env.PULSAR_ORT_DYLIB;
const cudaPolicy=process.env.PULSAR_ANNOTATED_BATCH_CUDA_POLICY;
const placed=process.env.PULSAR_ANNOTATED_BATCH_SOURCE_PLANS==='1';
const reportSchema=placed?6:5;
test(`annotated ${cudaPolicy?'CUDA':'CPU'} ${placed?'placed file-model':'file-model'} native Stash batch binds each source view through restart`,
  {skip:!enabled,timeout:240000},async t=>{
    const root=fs.mkdtempSync(path.join(os.tmpdir(),'pulsar-stash-annotated-batch-'));fs.chmodSync(root,0o700);
    t.diagnostic(`retained private annotated Stash fixture: ${root}`);
    const binary=fs.realpathSync(process.env.PULSAR_STASH_TEST_BINARY),fixture=fs.realpathSync(process.env.PULSAR_ANNOTATED_MODEL_FIXTURE_DIR);
    const hash=bytes=>crypto.createHash('sha256').update(bytes).digest('hex');
    const binaryIdentity={path:binary,sha256:hash(fs.readFileSync(binary)),byte_len:fs.statSync(binary).size};
    const expected=JSON.parse(fs.readFileSync(path.join(fixture,'expected.json'))),pairs=fs.readFileSync(path.join(fixture,'pairs.rgb'));
    assert.equal(hash(pairs),expected.rgb_sha256);
    const media=path.join(root,'media');fs.mkdirSync(media,{mode:0o700});
    const packed=[];for(let i=0;i<28;i++)for(let y=0;y<64;y++)packed.push(Buffer.alloc(96*3,17),Buffer.alloc(16*3,231),pairs.subarray((i*64+y)*64*3,(i*64+y+1)*64*3),Buffer.alloc(16*3,231));
    const raw=path.join(root,'stereo.rgb');fs.writeFileSync(raw,Buffer.concat(packed));const video=path.join(media,'stereo.mkv');
    const env={PATH:'/usr/bin:/bin',PULSAR_STATE_DIR:path.join(root,'engine'),PULSAR_ORT_DYLIB:fs.realpathSync(process.env.PULSAR_ORT_DYLIB)};
    if(cudaPolicy){
      const policy=JSON.parse(fs.readFileSync(cudaPolicy));
      for(const library of policy.native_dependencies.libraries){const bytes=fs.readFileSync(library.path);assert.equal(bytes.length,library.byte_len);assert.equal(hash(bytes),library.sha256);}
      const runtime=policy.native_dependencies.libraries.find(l=>l.names.includes('libonnxruntime.so.1'));
      assert.ok(runtime);assert.equal(env.PULSAR_ORT_DYLIB,fs.realpathSync(runtime.path));
      env.PULSAR_CUDA_OWNER_POLICY=path.join(root,'policy.json');fs.writeFileSync(env.PULSAR_CUDA_OWNER_POLICY,JSON.stringify(policy),{mode:0o600});
    }
    if(process.env.PULSAR_CGROUP_ROOT)env.PULSAR_CGROUP_ROOT=process.env.PULSAR_CGROUP_ROOT;
    const commands=[];
    const run=(executable,args)=>new Promise((resolve,reject)=>{
      const p=spawn(executable,args,{env,stdio:['ignore','pipe','pipe']});let out='',err='';
      p.stdout.on('data',b=>{out=(out+b).slice(-1024*1024);});p.stderr.on('data',b=>{err=(err+b).slice(-1024*1024);});p.once('error',reject);
      p.once('exit',code=>{commands.push({executable,args,code});resolve({code,out,err});});
    });
    const cli=args=>run(binary,args);
    const ok=async args=>{const r=await cli(args);assert.equal(r.code,0,r.err+r.out);return JSON.parse(r.out);};
    const encode=await run('/usr/bin/ffmpeg',['-v','error','-nostdin','-f','rawvideo','-pixel_format','rgb24','-video_size','192x64','-framerate','8','-i',raw,'-threads','1','-c:v','ffv1',video]);assert.equal(encode.code,0,encode.err);
    const secondRaw=path.join(root,'left.rgb'),secondVideo=path.join(media,'left.mkv'),left=[];
    for(let i=0;i<28;i++)for(let y=0;y<64;y++)left.push(Buffer.alloc(16*3,231),pairs.subarray((i*64+y)*64*3,(i*64+y+1)*64*3),Buffer.alloc(16*3,231),Buffer.alloc(96*3,37));
    fs.writeFileSync(secondRaw,Buffer.concat(left));const encoded=await run('/usr/bin/ffmpeg',['-v','error','-nostdin','-f','rawvideo','-pixel_format','rgb24','-video_size','192x64','-framerate','8','-i',secondRaw,'-threads','1','-c:v','ffv1',secondVideo]);assert.equal(encoded.code,0,encoded.err);
    const scenes=[{id:'s1',title:'Right target',interactive:false,files:[{id:'f1',path:'/stash/stereo.mkv'}]},{id:'s2',title:'Left target',interactive:false,files:[{id:'f2',path:'/stash/left.mkv'}]}],queries=[];let discoveryScenes=scenes;
    const server=http.createServer(async(req,res)=>{const chunks=[];for await(const b of req)chunks.push(b);const body=JSON.parse(Buffer.concat(chunks));queries.push(body);assert.ok(body.query.trim().startsWith('query'),'batch without scan/tag must remain query-only');const data=body.query.includes('findScene(id:')?{findScene:scenes.find(s=>s.id===body.variables.id)}:{findScenes:{count:discoveryScenes.length,scenes:discoveryScenes}};res.writeHead(200,{'Content-Type':'application/json',Connection:'close'});res.end(JSON.stringify({data}));});
    await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));const url=`http://127.0.0.1:${server.address().port}/graphql`;
    let engine,exited,engineError='';
    const start=()=>{engine=spawn(binary,['engine'],{env,stdio:['ignore','ignore','pipe']});engine.stderr.on('data',b=>{engineError=(engineError+b).slice(-65536);});exited=new Promise(resolve=>engine.once('exit',resolve));};
    const stop=async()=>{engine.kill('SIGTERM');await exited;};
    const ready=async()=>{const deadline=Date.now()+15000;while(true){assert.equal(engine.exitCode,null,engineError);assert.ok(Date.now()<deadline,'engine startup deadline');if(fs.existsSync(path.join(env.PULSAR_STATE_DIR,'engine.sock'))&&(await cli(['capabilities'])).code===0)return;await delay(25);}};
    try {
      start();await ready();const mappings=path.join(root,'mappings.json');fs.writeFileSync(mappings,JSON.stringify([{server_root:'/stash',local_root:media}]));
      const profile=await ok(['model','profile',path.join(fixture,'model.onnx'),'--annotated-temporal-position','--width','64','--height','64','--class-count','1']);if(cudaPolicy){
        const template=JSON.parse(fs.readFileSync('crates/pulsar-engine/tests/fixtures/cuda-intent-request.json')).operation.arguments.settings.inference_profile;
        profile.schema_version=2;profile.provider='cuda';profile.cuda_resources=template.cuda_resources;
      }
      const profileFile=path.join(root,'profile.json');fs.writeFileSync(profileFile,JSON.stringify(profile));
      const catalog=[video,secondVideo].map((file,i)=>({scene_id:scenes[i].id,file_id:scenes[i].files[0].id,view:{source_sha256:hash(fs.readFileSync(file)),declaration:{schema_version:1,source_dimensions:[192,64],analysis_dimensions:[96,64],target_region:[16,0,64,64],target_id:`target-${i}`,video_view:{layout:'side_by_side_left_right',eye:i===0?'right':'left',projection:{kind:'flat'}}}}}));assert.notEqual(catalog[0].view.source_sha256,catalog[1].view.source_sha256);
      const sourcePlan=(entry,rate=2)=>({scene_id:entry.scene_id,file_id:entry.file_id,source_sha256:entry.view.source_sha256,
        placement:{source_version:entry.view.source_sha256,source_start:{numerator:0,denominator:1},source_end:{numerator:3375000001,denominator:1000000000},
          source_anchor:{numerator:0,denominator:1},project_anchor:rate>0?2000000001:4000000001,rate_numerator:rate,rate_denominator:1},
        project_interval:rate>0?{start:2000000001,end:3687500002}:{start:2875000001,end:4000000002},source_time:true});
      const sourcePlans=placed?catalog.map((entry,i)=>sourcePlan(entry,i===0?2:-3)):[],sourcePlansFile=path.join(root,'source-plans.json');
      if(placed)fs.writeFileSync(sourcePlansFile,JSON.stringify(sourcePlans));
      const catalogFile=path.join(root,'views.json');fs.writeFileSync(catalogFile,JSON.stringify(catalog));
      const args=['stash','generate-missing','--url',url,'--mappings',mappings,'--model',path.join(fixture,'model.onnx'),'--inference-profile',profileFile,'--annotated-views',catalogFile,...(placed?['--source-plans',sourcePlansFile]:[]),'--width','64','--height','64','--limit','2','--detach','--request-id','annotated-two-source-batch'];
      const preflightRefusals=[];
      if(cudaPolicy){
        const savedPolicy=env.PULSAR_CUDA_OWNER_POLICY;
        await stop();delete env.PULSAR_CUDA_OWNER_POLICY;start();await ready();
        let refusal=await cli(args);assert.notEqual(refusal.code,0);assert.match(refusal.err,/owner-configured CUDA policy/);assert.equal(queries.length,0);preflightRefusals.push('missing_owner');
        await stop();env.PULSAR_CUDA_OWNER_POLICY=savedPolicy;start();await ready();
        const mismatch=structuredClone(profile);mismatch.runtime.artifact_id='a'.repeat(64);mismatch.runtime.sha256='a'.repeat(64);
        fs.writeFileSync(profileFile,JSON.stringify(mismatch));refusal=await cli(args);assert.notEqual(refusal.code,0);assert.match(refusal.err,/runtime differs from owner policy/);assert.equal(queries.length,0);preflightRefusals.push('runtime_mismatch');
        const over=structuredClone(profile),policy=JSON.parse(fs.readFileSync(savedPolicy));over.cuda_resources.device_reservation_bytes=policy.accounting.maximum_reserved_bytes-policy.accounting.interactive_reserved_bytes+1;
        fs.writeFileSync(profileFile,JSON.stringify(over));refusal=await cli(args);assert.notEqual(refusal.code,0);assert.match(refusal.err,/owner priority ceiling/);assert.equal(queries.length,0);preflightRefusals.push('offline_ceiling');
        fs.writeFileSync(profileFile,JSON.stringify(profile));
        const noLedger=await run('/usr/bin/python3',['-c',"import sqlite3,sys;d=sqlite3.connect('file:'+sys.argv[1]+'?mode=ro',uri=True);print(d.execute('SELECT count(*) FROM stash_batches').fetchone()[0]);print(d.execute('SELECT count(*) FROM jobs').fetchone()[0])",path.join(env.PULSAR_STATE_DIR,'projects.sqlite3')]);assert.equal(noLedger.code,0,noLedger.err);assert.equal(noLedger.out.trim(),'0\n0');
      }
      const missing=path.join(root,'missing.json');fs.writeFileSync(missing,JSON.stringify(catalog.slice(0,1)));const refused=await cli(args.map(a=>a===catalogFile?missing:a));assert.notEqual(refused.code,0);assert.match(refused.err,/matching annotated view|no explicit annotated view/);
      const initial=await ok(args);assert.equal(initial.schema_version,reportSchema);assert.equal(initial.item_count,2);assert.equal(initial.state,'running');
      let stopped=false,report;const deadline=Date.now()+120000;
      while(true){report=(await ok(['stash','batch-status',initial.batch_request_id])).value;assert.equal(report.schema_version,reportSchema);assert.ok(Date.now()<deadline,'batch deadline');
        if(!stopped && report.items.some(i=>i.requests.some(r=>r.phase==='generate'))){await stop();start();await ready();stopped=true;}
        if(report.state!=='running')break;await delay(25);
      }
      assert.equal(stopped,true,'restart after a durable generation request');assert.equal(report.state,'finished',JSON.stringify(report));assert.equal(report.release_qualified,false);assert.equal(report.items.length,2);
      for(let i=0;i<2;i++){const item=report.items[i];assert.equal(item.scene_id,catalog[i].scene_id);assert.equal(item.error,null);assert.equal(item.requests.filter(r=>r.phase==='generate').length,1);assert.ok(item.export);if(placed){assert.equal(item.export.timing_proof.mapped_actions,28);assert.equal(item.requests.filter(r=>r.phase==='place').length,1);}const actions=JSON.parse(fs.readFileSync(item.export.path)).actions;assert.equal(actions.length,28);assert.deepEqual(actions.map(a=>a.at),Array.from({length:28},(_,j)=>j*125));for(let j=0;j<14;j++)assert.equal(actions[j*2+1].pos,Math.round(expected.positions[j]*100));}
      const db=path.join(env.PULSAR_STATE_DIR,'projects.sqlite3');
      const inspect=await run('/usr/bin/python3',['-c',"import sqlite3,json,sys;d=sqlite3.connect('file:'+sys.argv[1]+'?mode=ro',uri=True);r=json.loads(d.execute('SELECT record FROM stash_batches WHERE request=?',(sys.argv[2],)).fetchone()[0]);jobs=[json.loads(d.execute('SELECT manifest FROM jobs WHERE id=?',(i['view']['job_id'],)).fetchone()[0]) for i in r['items']];print(json.dumps({'record':r,'jobs':jobs,'schema':d.execute('PRAGMA user_version').fetchone()[0]}))",db,initial.batch_request_id]);assert.equal(inspect.code,0,inspect.err);const retained=JSON.parse(inspect.out);assert.equal(retained.jobs.length,2);assert.deepEqual(retained.record.options.annotated_views,catalog);if(placed){assert.deepEqual(retained.record.options.source_plans,sourcePlans);for(let i=0;i<2;i++){const settings=retained.jobs[i].operation.arguments.settings;assert.deepEqual(settings.source_placement.spec,sourcePlans[i].placement);assert.deepEqual(settings.project_interval,sourcePlans[i].project_interval);}}for(let i=0;i<2;i++)assert.deepEqual(retained.jobs[i].operation.arguments.settings.annotated_view,catalog[i].view);
      await stop();start();await ready();const replay=await ok(args);assert.equal(replay.batch_request_id,initial.batch_request_id);const after=(await ok(['stash','batch-status',initial.batch_request_id])).value;assert.deepEqual(after.items,report.items);assert.equal(after.state,'finished');assert.equal(hash(fs.readFileSync(binary)),binaryIdentity.sha256);
      const thirdVideo=path.join(media,'repair.mkv');fs.copyFileSync(video,thirdVideo);const third={id:'s3',title:'Explicit repair',interactive:false,files:[{id:'f3',path:'/stash/repair.mkv'}]};scenes.push(third);discoveryScenes=[third];
      const correct={...catalog[0],scene_id:'s3',file_id:'f3'},bad={...correct,view:{...correct.view,source_sha256:'b'.repeat(64)}};
      const repairPlansFile=path.join(root,'repair-plans.json');if(placed)fs.writeFileSync(repairPlansFile,JSON.stringify([sourcePlan(bad)]));
      const badCatalog=path.join(root,'bad-source.json');fs.writeFileSync(badCatalog,JSON.stringify([bad]));const badArgs=args.map(a=>a===sourcePlansFile?repairPlansFile:a===catalogFile?badCatalog:a==='annotated-two-source-batch'?'annotated-repair-batch':a);const repairInitial=await ok(badArgs);let paused;const repairDeadline=Date.now()+120000;
      while(true){paused=(await ok(['stash','batch-status',repairInitial.batch_request_id])).value;if(paused.state!=='running')break;assert.ok(Date.now()<repairDeadline,'refusal deadline');await delay(25);}
      assert.equal(paused.state,'needs_attention');assert.equal(paused.items[0].job_id,null);assert.equal(paused.items[0].requests.filter(r=>r.phase==='generate').length,0);assert.match(paused.items[0].error,/differs from captured source content|source plan differs from captured Stash media/);assert.equal(fs.existsSync(path.join(media,'repair.funscript')),false);
      const replacement=path.join(root,'replacement.json');fs.writeFileSync(replacement,JSON.stringify({preset:'fast',settings:retained.record.options.settings,model_path:path.join(fixture,'model.onnx'),annotated_views:[correct],...(placed?{source_plans:[sourcePlan(correct)]}:{})}));
      const migrationArgs=['stash','migrate-batch',repairInitial.batch_request_id,'--expected-ledger-sha256',paused.ledger_sha256,'--generation',replacement,'--request-id','annotated-view-repair'];
      const stale=await cli(migrationArgs.map(a=>a===paused.ledger_sha256?'0'.repeat(64):a));assert.notEqual(stale.code,0);assert.match(stale.err,/differs from observed migration identity/);
      let migrationResult;const migrationDeadline=Date.now()+5000;while(true){const r=await cli(migrationArgs);if(r.code===0){migrationResult=JSON.parse(r.out).value;break;}assert.match(r.err,/scheduler is still active/);assert.ok(Date.now()<migrationDeadline,'scheduler retirement');await delay(25);}
      assert.equal(migrationResult.state,'needs_attention');assert.equal(migrationResult.schema_version,reportSchema);assert.equal(migrationResult.migrations.length,1);assert.equal(migrationResult.items[0].job_id,null);assert.deepEqual((await ok(migrationArgs)).value,migrationResult);
      await stop();start();await ready();const stillPaused=(await ok(['stash','batch-status',repairInitial.batch_request_id])).value;assert.equal(stillPaused.state,'needs_attention');assert.equal(stillPaused.items[0].job_id,null);
      await ok(['stash','resume-batch',repairInitial.batch_request_id]);let repaired;const repairedDeadline=Date.now()+120000;while(true){repaired=(await ok(['stash','batch-status',repairInitial.batch_request_id])).value;if(repaired.state!=='running')break;assert.ok(Date.now()<repairedDeadline,'repaired batch deadline');await delay(25);}
      assert.equal(repaired.state,'finished',JSON.stringify(repaired));assert.equal(repaired.items[0].requests.filter(r=>r.phase==='generate').length,1);if(placed)assert.equal(repaired.items[0].export.timing_proof.mapped_actions,28);const repairedActions=JSON.parse(fs.readFileSync(repaired.items[0].export.path)).actions;assert.equal(repairedActions.length,28);for(let j=0;j<14;j++)assert.equal(repairedActions[j*2+1].pos,Math.round(expected.positions[j]*100));
      const history=await run('/usr/bin/python3',['-c',"import sqlite3,json,sys;d=sqlite3.connect('file:'+sys.argv[1]+'?mode=ro',uri=True);print(d.execute('SELECT record FROM stash_batches WHERE request=?',(sys.argv[2],)).fetchone()[0])",db,repairInitial.batch_request_id]);assert.equal(history.code,0,history.err);const repairedRecord=JSON.parse(history.out);assert.deepEqual(repairedRecord.options.annotated_views,[correct]);assert.deepEqual(repairedRecord.migration_history[0].previous_options.annotated_views,[bad]);assert.equal(repairedRecord.report.schema_version,reportSchema);
      const jobIds=[...report.items,...repaired.items].map(item=>item.job_id);const completed=await run('/usr/bin/python3',['-c',`import sqlite3,json,sys;d=sqlite3.connect('file:'+sys.argv[1]+'?mode=ro',uri=True);print(json.dumps([d.execute("SELECT id FROM attempts WHERE job=? AND state='completed'",(job,)).fetchone()[0] for job in sys.argv[2:]]))`,db,...jobIds]);assert.equal(completed.code,0,completed.err);const attempts=JSON.parse(completed.out),programErrors=[],cudaEvidence=[];
      for(let i=0;i<attempts.length;i++){const dir=path.join(env.PULSAR_STATE_DIR,'attempts',attempts[i]),receipt=JSON.parse(fs.readFileSync(path.join(dir,'receipt.json'))),program=JSON.parse(fs.readFileSync(path.join(dir,'program.json')));if(cudaPolicy){
        const receiptBytes=fs.readFileSync(path.join(dir,'cuda-inference.json')),traceBytes=fs.readFileSync(path.join(dir,'cuda-trace.json'));
        const cudaReceipt=JSON.parse(receiptBytes),trace=JSON.parse(traceBytes),nodes=trace.filter(e=>e.cat==='Node');
        assert.equal(cudaReceipt.provider,'cuda');assert.equal(cudaReceipt.successful_runs,27);assert.ok(nodes.length>0);assert.ok(nodes.every(n=>n.args.provider==='CUDAExecutionProvider'));
        cudaEvidence.push({attempt:attempts[i],successful_runs:cudaReceipt.successful_runs,cuda_nodes:nodes.length,receipt_sha256:hash(receiptBytes),trace_sha256:hash(traceBytes)});
      }
      assert.equal(receipt.schema_version,3);assert.equal(receipt.decoded_sample_count,28);
      if(placed){const plan=i<2?sourcePlans[i]:sourcePlan(correct),rate=BigInt(plan.placement.rate_numerator),anchor=BigInt(plan.placement.project_anchor);
        for(const sample of receipt.samples){assert.equal(sample.source_pts,sample.source_frame_index*125);let numerator=anchor*rate+BigInt(sample.source_frame_index)*125000000n,denominator=rate;if(denominator<0n){numerator=-numerator;denominator=-denominator;}assert.equal(sample.project_time_ns,Number(numerator/denominator),'independent exact source/project mapping');assert.ok(sample.project_time_ns>=plan.project_interval.start&&sample.project_time_ns<plan.project_interval.end);}}
      assert.equal(receipt.qualification,'unqualified');assert.deepEqual(receipt.annotated_input.native_plan.declaration,(i<2?catalog[i]:correct).view.declaration);const actions=new Map(program.tracks[0].actions.map(a=>[a.time,a]));for(let j=0;j<14;j++){const sample=receipt.samples.find(s=>s.source_frame_index===j*2+1),action=actions.get(sample.project_time_ns);assert.equal(action.evidence,'inferred');programErrors.push(Math.abs(action.position-expected.positions[j]));}}
      assert.ok(Math.max(...programErrors)<=1e-6);
      if(cudaPolicy){
        const membership=await run('/usr/bin/python3',['-c',"import sqlite3,json,sys;d=sqlite3.connect('file:'+sys.argv[1]+'?mode=ro',uri=True);print(json.dumps({t:[r[0] for r in d.execute('SELECT attempt FROM '+t)] for t in ['annotated_generation_attempts','cuda_candidate_evidence']}))",db]);
        assert.equal(membership.code,0,membership.err);for(const members of Object.values(JSON.parse(membership.out)))assert.deepEqual(members.sort(),[...attempts].sort());
        await stop();start();await ready();
        for(const evidence of cudaEvidence){const dir=path.join(env.PULSAR_STATE_DIR,'attempts',evidence.attempt);assert.equal(hash(fs.readFileSync(path.join(dir,'cuda-inference.json'))),evidence.receipt_sha256);assert.equal(hash(fs.readFileSync(path.join(dir,'cuda-trace.json'))),evidence.trace_sha256);}
      }
      let oldReaderRefused=false;
      if(process.env.PULSAR_ANNOTATED_BATCH_OLD_READER){await stop();const previous=fs.realpathSync(process.env.PULSAR_ANNOTATED_BATCH_OLD_READER);const dbIdentity=()=>[db,db+'-wal'].map(p=>fs.existsSync(p)?hash(fs.readFileSync(p)):null),before=dbIdentity();const refusal=await run(previous,['engine']);assert.notEqual(refusal.code,0);assert.match(refusal.err,/unknown field `annotated_views`|annotated.*ledger|batch.*reader/i);assert.deepEqual(dbIdentity(),before,'preceding reader must preserve database/WAL bytes');oldReaderRefused=true;start();await ready();}
      fs.writeFileSync(path.join(root,'result.json'),JSON.stringify({status:'PASS',provider:cudaPolicy?'cuda':'cpu',placed,distinct_placements:placed?2:0,explicit_intervals:placed?2:0,source_time_proofs:placed?3:0,placed_samples_checked:placed?84:0,preflightRefusals,cudaEvidence,scope:'Authored native CLI/local IPC with own loopback Stash; no real Stash or physical playback',binary:binaryIdentity,distinct_sources:2,distinct_views:2,old_reader_refused:oldReaderRefused,missing_catalog_refused:true,restart_after_durable_generation:true,single_generation_request_per_source:true,python_rounded_positions:42,python_program_positions:42,maximum_program_difference:Math.max(...programErrors),query_only:true,source_hash_refused_before_job:true,stale_migration_refused:true,migration_paused_across_restart:true,explicit_resume_required:true,repairedRecord,repaired,retained,report,queries,commands},null,2));
    } finally {if(engine?.exitCode===null)await stop();await new Promise(resolve=>server.close(resolve));fs.writeFileSync(path.join(root,'commands.json'),JSON.stringify(commands,null,2));fs.writeFileSync(path.join(root,'engine.log'),engineError);}
  });
