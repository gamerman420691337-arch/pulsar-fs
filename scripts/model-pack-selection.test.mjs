// Authored CPU graph and generated media: functional wiring, never model quality.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import net from 'node:net';
import http from 'node:http';
import {setTimeout as delay} from 'node:timers/promises';
import {spawn,spawnSync} from 'node:child_process';
import {once} from 'node:events';
import {authoredOnnxFixture} from './benchmark-onnx-fixture.mjs';
const sha=b=>crypto.createHash('sha256').update(b).digest('hex');
test('owner-selected pack feeds pinned CPU generation, retains evidence and refuses changed authority or bytes',
  {skip:!process.env.PULSAR_BENCHMARK_WORKER || !process.env.PULSAR_BENCHMARK_ORT,timeout:120000},async t=>{
  const binary=fs.realpathSync(process.env.PULSAR_BENCHMARK_WORKER);
  const root=fs.mkdtempSync(path.join(os.tmpdir(),'pulsar-pack-selection-'));fs.chmodSync(root,0o700);
  t.diagnostic(`retained authored selection fixture: ${root}`);
  const state=path.join(root,'state'),source=path.join(root,'supplier');fs.mkdirSync(source);
  const env={PATH:'/usr/bin:/bin',PULSAR_STATE_DIR:state,PULSAR_ORT_DYLIB:process.env.PULSAR_BENCHMARK_ORT};
  let engine,exited,index=0;
  const cli=(args,exe=binary)=>{const r=spawnSync(exe,args,{env,encoding:'utf8',timeout:30000,maxBuffer:4*1024*1024});assert.equal(r.error,undefined);fs.writeFileSync(path.join(root,`cli-${index++}.json`),JSON.stringify({args,status:r.status,stdout:r.stdout,stderr:r.stderr}),{mode:0o600});return r;};
  const ok=args=>{const r=cli(args);assert.equal(r.status,0,r.stderr);return JSON.parse(r.stdout);};
  const start=async()=>{const fd=fs.openSync(path.join(root,`engine-${index++}.log`),'wx',0o600);engine=spawn(binary,['engine'],{env,stdio:['ignore','ignore',fd]});fs.closeSync(fd);exited=once(engine,'exit');const end=Date.now()+15000;while(true){assert.equal(engine.exitCode,null);assert.ok(Date.now()<end,'engine startup');if(fs.existsSync(path.join(state,'engine.sock'))&&cli(['capabilities']).status===0)return;await new Promise(r=>setTimeout(r,25));}};
  const stop=async()=>{if(!engine)return;if(engine.exitCode===null&&engine.signalCode===null)engine.kill('SIGTERM');const timer=setTimeout(()=>engine.kill('SIGKILL'),2000);try{await exited;}finally{clearTimeout(timer);}};
  t.after(stop);await start();
  const catalog=ok(['model','status']);assert.deepEqual(catalog.providers,['cpu']);
  const model=authoredOnnxFixture(),runtime=fs.readFileSync(process.env.PULSAR_BENCHMARK_ORT);
  assert.equal(sha(runtime),catalog.runtime.sha256);
  const identity=b=>({artifact_id:sha(b),sha256:sha(b),byte_len:b.length});
  const profile={schema_version:1,model:identity(model),runtime:catalog.runtime,preprocessing:{input:{width:64,height:64,channels:3,layout:'nchw',class_count:1,decoder:'yolo_channel_major'},resize:'stretch',color:'rgb',scale:[1/255,1/255,1/255],bias:[0,0,0],padding_byte:0},provider:'cpu',minimum_memory_bytes:128*1024*1024,minimum_output_bytes:1024*1024,cpu_threads:2};
  const files=[['model.onnx','model',model],['runtime.bin','runtime',runtime],['profile.json','profile',Buffer.from(JSON.stringify(profile))],['LICENSE','license',Buffer.from('Authored fixture; no redistribution or model quality approval.')]];
  for(const [name,,bytes] of files)fs.writeFileSync(path.join(source,name),bytes);
  const manifest={schema_version:1,pack_id:'authored-selected-pack',target_os:process.platform,target_arch:process.arch==='x64'?'x86_64':process.arch,ort_api_version:24,labels:['fixture'],assets:files.map(([name,role,b])=>({path:name,role,byte_len:b.length,sha256:sha(b)})),license_path:'LICENSE',source_description:'Authored functional CPU fixture; unqualified'};
  const manifestBytes=Buffer.from(JSON.stringify(manifest)),hash=sha(manifestBytes);fs.writeFileSync(path.join(source,'manifest.json'),manifestBytes);
  const imported=ok(['model','pack-import',source,'--request-id','pack-install']).value;assert.equal(imported.active,false);
  assert.equal(ok(['model','pack-selection-status']).value.selection_revision,0);
  // Integrity intake permits unsupported declarations; explicit selection must refuse them.
  for(const kind of ['runtime','threads','target','native-dependency']){
    const variant=path.join(root,`supplier-${kind}`);fs.mkdirSync(variant);
    const p=structuredClone(profile);if(kind==='threads')p.cpu_threads=1;
    const r=kind==='runtime'?Buffer.from('authored unsupported runtime'):runtime;
    p.runtime=identity(r);
    const items=[['model.onnx','model',model],['runtime.bin','runtime',r],['profile.json','profile',Buffer.from(JSON.stringify(p))],['LICENSE','license',files[3][2]]];
    if(kind==='native-dependency')items.push(['extra.bin','native_dependency',Buffer.from('unapproved supplier native dependency')]);
    for(const [name,,bytes]of items)fs.writeFileSync(path.join(variant,name),bytes);
    const m={...manifest,pack_id:`authored-rejected-${kind}`,target_os:kind==='target'?'windows':manifest.target_os,assets:items.map(([name,role,b])=>({path:name,role,byte_len:b.length,sha256:sha(b)}))};
    const bytes=Buffer.from(JSON.stringify(m));fs.writeFileSync(path.join(variant,'manifest.json'),bytes);
    ok(['model','pack-import',variant]);const refused=cli(['model','pack-select',sha(bytes),'--expected-selection-revision','0','--approve-unqualified-native-execution']);assert.notEqual(refused.status,0,kind);
    assert.equal(ok(['model','pack-selection-status']).value.selection_revision,0);
  }
  const args=['model','pack-select',hash,'--expected-selection-revision','0','--approve-unqualified-native-execution','--request-id','pack-select'];
  const selected=ok(args).value;assert.equal(selected.selection_revision,1);assert.equal(selected.native_execution_approved,true);
  for(const field of ['rights_verified','compatible','qualified'])assert.equal(selected[field],false);
  assert.deepEqual(ok(args).value,selected);assert.equal(ok(['model','pack-status',hash]).value.active,false);
  const stale=cli(['model','pack-deactivate','--expected-selection-revision','0']);assert.notEqual(stale.status,0);assert.match(stale.stderr,/DependencyMismatch/);
  const ownerPath=path.join(state,'model-pack-owner.token'),proof=fs.readFileSync(ownerPath,'utf8').trim();
  let sequence=0;
  const rpc=(operation,args,project,revision,owner=true)=>new Promise((resolve,reject)=>{
    const credentials=JSON.parse(fs.readFileSync(path.join(state,'first-party.session.json')));
    const data=Buffer.from(JSON.stringify({version:2,request_id:`pack-rpc-${++sequence}`,...credentials,...(project?{project,expected_revision:revision}:{}),command:{operation,arguments:{...args,...(owner?{owner_proof:proof}:{})}}}));
    const header=Buffer.alloc(4);header.writeUInt32BE(data.length);const socket=net.createConnection(path.join(state,'engine.sock'));let received=Buffer.alloc(0);
    socket.setTimeout(10000,()=>socket.destroy(new Error('IPC timeout')));socket.on('error',reject);socket.on('close',()=>reject(new Error('IPC closed before response')));socket.on('connect',()=>socket.write(Buffer.concat([header,data])));socket.on('data',b=>{received=Buffer.concat([received,b]);if(received.length>=4&&received.length>=received.readUInt32BE(0)+4){resolve(JSON.parse(received.subarray(4)).result);socket.destroy();}});
  });
  const forbidden=await rpc('select_model_pack',{manifest_sha256:null,expected_selection_revision:1,approve_unqualified_native_execution:false,owner_proof:'wrong'},undefined,undefined,false);assert.equal(forbidden.Err?.code,'forbidden');
  const media=path.join(root,'source.y4m');fs.writeFileSync(media,Buffer.concat([Buffer.from('YUV4MPEG2 W64 H64 F8:1 Ip A1:1 Cmono\n'),...Array.from({length:16},(_,frame)=>Buffer.concat([Buffer.from('FRAME\n'),Buffer.from(Array.from({length:4096},(_,i)=>(i%64*31+((Math.floor(i/64)+64-frame)%64)*17)%256))]))]));
  const output=path.join(root,'selected.funscript');const generated=cli(['generate',media,'--model-pack',hash,'--width','64','--height','64','--output',output]);assert.equal(generated.status,0,generated.stderr);assert.ok(fs.existsSync(output));
  const db=path.join(state,'projects.sqlite3');
  const sql=code=>{const r=spawnSync('/usr/bin/python3',['-c',code,db],{env,encoding:'utf8',timeout:10000});assert.equal(r.status,0,r.stderr);return JSON.parse(r.stdout);};
  const job=sql("import sqlite3,json,sys;d=sqlite3.connect(sys.argv[1]); print(json.dumps(d.execute('SELECT id,project,source,manifest,state FROM jobs').fetchone()))");
  const request=JSON.parse(job[3]);assert.equal(job[4],'completed');assert.deepEqual(request.operation.arguments.model.identity,profile.model);
  const approval=Buffer.from(sql("import sqlite3,json,sys;d=sqlite3.connect(sys.argv[1]);print(json.dumps(d.execute('SELECT report FROM model_pack_selection WHERE singleton=1').fetchone()[0]))"));assert.deepEqual(JSON.parse(approval),selected);
  for(const bytes of [manifestBytes,files[2][2],approval]){
    const digest=sha(bytes);assert.ok(request.dependencies.some(d=>d.sha256===digest));assert.deepEqual(fs.readFileSync(path.join(state,'snapshots',digest)),bytes);
  }
  const project=job[1];let revision=ok(['project','open',project]).value.revision;
  const previewArgs=['project','preview-pack',project,'500000000','--model-pack',hash,'--source-version',job[2]];
  const preview=ok(previewArgs);assert.equal(preview.kind,'model_pack_preview');
  assert.deepEqual(preview.value.choice,selected.choice);assert.deepEqual(preview.value.model,profile.model);
  assert.equal(preview.value.qualified,false);assert.equal(preview.value.preview.source_frame_index,4);
  assert.equal(preview.value.preview.analysis.status,'available');assert.equal(preview.value.preview.observations.length,1);
  const profileDependency=request.dependencies.find(d=>d.sha256===preview.value.profile_sha256);assert.ok(profileDependency,'preview canonical profile retained by generation');
  assert.ok(profileDependency.byte_len>0);assert.deepEqual(request.operation.arguments.settings.inference_profile,selected.profile);
  const placedPreviews=[];
  for(const rate of [1,-1]) {
    const placed=await rpc('set_source_placement',{spec:{source_version:job[2],source_start:{numerator:0,denominator:1},source_end:{numerator:2,denominator:1},source_anchor:{numerator:rate===1?0:3,denominator:rate===1?1:2},project_anchor:0,rate_numerator:rate,rate_denominator:1},replace:null},project,revision,false);
    assert.ok(placed.Ok,JSON.stringify(placed.Err));revision=placed.Ok.value.revision;
    const placement=placed.Ok.value.placements[0].placement_id;
    const result=ok(['project','preview-pack',project,'750000000','--model-pack',hash,'--placement-id',placement]).value;
    assert.equal(result.preview.source_frame_index,6);assert.equal(result.preview.context.source_placement,placement);assert.equal(result.preview.observations.length,1);placedPreviews.push({rate,frame:result.preview.source_frame_index});
    const removed=await rpc('remove_source_placement',{placement_id:placement},project,revision,false);assert.ok(removed.Ok,JSON.stringify(removed.Err));revision=removed.Ok.value.revision;
  }
  const before=sql("import sqlite3,json,sys;d=sqlite3.connect(sys.argv[1]); print(json.dumps([d.execute('SELECT COUNT(*) FROM jobs').fetchone()[0],d.execute('SELECT COUNT(*) FROM attempts').fetchone()[0]]))");
  const genArgs={source_version:job[2],preset:'fast',settings:request.operation.arguments.settings,choice:{manifest_sha256:hash,selection_revision:1}};
  const modelPath=path.join(state,'model-packs',hash,'model.onnx');fs.chmodSync(modelPath,0o600);fs.writeFileSync(modelPath,'changed model');
  assert.ok((await rpc('generate_from_model_pack',genArgs,project,revision,false)).Err);
  assert.notEqual(cli(previewArgs).status,0,'preview must refuse changed model bytes');
  fs.writeFileSync(modelPath,model);fs.chmodSync(modelPath,0o400);
  const conflict=await rpc('generate_from_model_pack',{...genArgs,settings:{...genArgs.settings,model_input:{...profile.preprocessing.input,width:32}}},project,revision,false);assert.equal(conflict.Err?.code,'dependency_mismatch');
  const allowed=cli(['allow-packaging',project,'--acknowledge-unencrypted-private-data']);assert.equal(allowed.status,0,allowed.stderr);const archive=path.join(root,'pack-origin.pulsar');const exported=cli(['package-export',project,archive,'--acknowledge-unencrypted-private-data']);assert.equal(exported.status,0,exported.stderr);
  const clone=ok(['package-import',archive]).receipt.project_id;assert.notEqual(clone,project);
  assert.equal(sql("import sqlite3,json,sys;d=sqlite3.connect(sys.argv[1]);print(json.dumps(d.execute('PRAGMA user_version').fetchone()[0]))"),20);
  const deactivated=ok(['model','pack-deactivate','--expected-selection-revision','1','--request-id','pack-deactivate']).value;assert.equal(deactivated.selection_revision,2);assert.equal(deactivated.choice,null);
  assert.equal((await rpc('generate_from_model_pack',genArgs,project,revision,false)).Err?.code,'dependency_mismatch');
  const oldPreview={context:{source_version:job[2],source_placement:`primary:${job[2]}`,frame:0,transform:preview.value.preview.context.transform,seek_generation:0,request_generation:0},time:{kind:'source',source_time:{numerator:1,denominator:2},time_basis:'elapsed_from_first_frame'},video_view:null,choice:selected.choice};
  assert.equal((await rpc('preview_model_pack_frame',oldPreview,project,revision,false)).Err?.code,'dependency_mismatch');
  assert.deepEqual(sql("import sqlite3,json,sys;d=sqlite3.connect(sys.argv[1]); print(json.dumps([d.execute('SELECT COUNT(*) FROM jobs').fetchone()[0],d.execute('SELECT COUNT(*) FROM attempts').fetchone()[0]]))"),before);
  assert.deepEqual(ok(args).value,selected);assert.deepEqual(ok(['model','pack-selection-status']).value,deactivated);
  await stop();
  const old=process.env.PULSAR_SCHEMA11_PACK_TEST_BINARY;
  if(old){const databaseBytes=()=>Object.fromEntries([db,db+'-wal'].map(p=>[p,fs.existsSync(p)?sha(fs.readFileSync(p)):null]));const dbBefore=databaseBytes();const refused=cli(['engine'],old);assert.notEqual(refused.status,0);assert.match(refused.stderr,/schema.*migration/i);assert.deepEqual(databaseBytes(),dbBefore);}
  await start();assert.deepEqual(ok(['model','pack-selection-status']).value,deactivated);
  assert.equal(ok(['project','open',project]).value.revision,revision);assert.equal(ok(['project','open',clone]).value.project_id,clone);
  // Pack-bound batch admission, deactivation, restart and explicit migration.
  const liveChoice=ok(['model','pack-select',hash,'--expected-selection-revision','2','--approve-unqualified-native-execution']).value;
  const stashMedia=path.join(root,'stash-media');fs.mkdirSync(stashMedia);fs.copyFileSync(media,path.join(stashMedia,'source.mp4'));
  const mappings=path.join(root,'stash-mappings.json');fs.writeFileSync(mappings,JSON.stringify([{server_root:'/stash',local_root:stashMedia}]));
  let hold=true,release,arrive;const arrived=new Promise(resolve=>arrive=resolve);let queries=0,mutations=0;
  const server=http.createServer(async(req,res)=>{
    const chunks=[];for await(const b of req)chunks.push(b);const body=JSON.parse(Buffer.concat(chunks));queries++;
    if(body.query.includes('mutation'))mutations++;assert.ok(!body.query.includes('mutation'));
    const scene={id:'pack-scene',title:'Authored pack scene',interactive:false,files:[{id:'pack-file',path:'/stash/source.mp4'}]};
    if(body.query.includes('findScene(id:')&&hold){arrive();await new Promise(resolve=>release=resolve);}
    res.writeHead(200,{'Content-Type':'application/json',Connection:'close'});res.end(JSON.stringify({data:body.query.includes('findScene(id:')?{findScene:scene}:{findScenes:{count:1,scenes:[scene]}}}));
  });await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  t.after(async()=>{hold=false;release?.();server.closeAllConnections();await new Promise(resolve=>server.close(resolve));});
  const url=`http://127.0.0.1:${server.address().port}/graphql`;
  const asyncCli=args=>new Promise((resolve,reject)=>{
    const child=spawn(binary,args,{env,stdio:['ignore','pipe','pipe']});let stdout='',stderr='';const timer=setTimeout(()=>child.kill('SIGKILL'),30000);
    child.stdout.on('data',b=>stdout=(stdout+b).slice(-4*1024*1024));child.stderr.on('data',b=>stderr=(stderr+b).slice(-65536));
    child.on('error',e=>{clearTimeout(timer);reject(e);});child.on('exit',status=>{clearTimeout(timer);fs.writeFileSync(path.join(root,`cli-${index++}.json`),JSON.stringify({args,status,stdout,stderr}),{mode:0o600});resolve({status,stdout,stderr});});
  });
  const batchArgs=['stash','generate-missing','--url',url,'--mappings',mappings,'--model-pack',hash,'--width','64','--height','64','--detach','--request-id','authored-pack-batch'];
  const submitted=await asyncCli(batchArgs);assert.equal(submitted.status,0,submitted.stderr);const batch=JSON.parse(submitted.stdout);assert.equal(batch.schema_version,4);assert.deepEqual(batch.model_pack,liveChoice.choice);
  let holdTimer;try{await Promise.race([arrived,new Promise((_,reject)=>{holdTimer=setTimeout(()=>reject(new Error('pack capture hold deadline')),15000);})]);}finally{clearTimeout(holdTimer);}
  const pausedLedger=sql("import sqlite3,json,sys;d=sqlite3.connect(sys.argv[1]);print(d.execute('SELECT record FROM stash_batches').fetchone()[0])");
  assert.equal(pausedLedger.options.model_path,null);assert.deepEqual(pausedLedger.options.model_pack,liveChoice.choice);assert.deepEqual(pausedLedger.options.settings.inference_profile,liveChoice.profile);assert.equal(pausedLedger.items[0].cursor,1);
  // Selection shares the artifact gate with capture. Queue asynchronously, then
  // release the loopback response so deactivation commits before generation.
  const deactivation=asyncCli(['model','pack-deactivate','--expected-selection-revision','3']);
  hold=false;release();release=undefined;
  const deactivationResult=await deactivation;assert.equal(deactivationResult.status,0,deactivationResult.stderr);const deactivatedAgain=JSON.parse(deactivationResult.stdout).value;assert.equal(deactivatedAgain.selection_revision,4);
  const observe=async()=>{const end=Date.now()+30000;while(true){const r=await asyncCli(['stash','batch-status',batch.batch_request_id]);assert.equal(r.status,0,r.stderr);const value=JSON.parse(r.stdout).value;if(value.state!=='running')return value;assert.ok(Date.now()<end,'pack batch observation deadline');await delay(50);}};
  const refusedBatch=await observe();assert.equal(refusedBatch.state,'needs_attention');assert.match(refusedBatch.items[0].error,/current owner selection/);assert.equal(refusedBatch.items[0].job_id,null);
  assert.deepEqual(sql("import sqlite3,json,sys;d=sqlite3.connect(sys.argv[1]);print(json.dumps([d.execute('SELECT COUNT(*) FROM jobs').fetchone()[0],d.execute('SELECT COUNT(*) FROM attempts').fetchone()[0]]))"),before);
  const oldPhase=refusedBatch.items[0].requests.find(r=>r.phase==='generate').request_id;
  await stop();
  const schema12=process.env.PULSAR_SCHEMA12_PACK_BATCH_TEST_BINARY;
  if(schema12){const databaseBytes=()=>Object.fromEntries([db,db+'-wal'].map(p=>[p,fs.existsSync(p)?sha(fs.readFileSync(p)):null]));const retained=databaseBytes();const oldRefused=cli(['engine'],schema12);assert.notEqual(oldRefused.status,0);assert.match(oldRefused.stderr,/schema.*migration/i);assert.deepEqual(databaseBytes(),retained);}
  await start();assert.equal(ok(['model','pack-selection-status']).value.selection_revision,4);
  assert.equal(sql("import sqlite3,json,sys;d=sqlite3.connect(sys.argv[1]);print(json.dumps(d.execute('PRAGMA user_version').fetchone()[0]))"),20);
  const newSelection=ok(['model','pack-select',hash,'--expected-selection-revision','4','--approve-unqualified-native-execution']).value;
  const retry=await asyncCli(['stash','resume-batch',batch.batch_request_id]);assert.equal(retry.status,0,retry.stderr);const staleBatch=await observe();assert.equal(staleBatch.state,'needs_attention');assert.equal(staleBatch.items[0].requests.find(r=>r.phase==='generate').request_id,oldPhase);
  const migrationFile=path.join(root,'pack-migration.json');fs.writeFileSync(migrationFile,JSON.stringify({preset:'fast',settings:pausedLedger.options.settings,model_path:null,model_pack:newSelection.choice}));
  await delay(300);const migrationArgs=['stash','migrate-batch',batch.batch_request_id,'--expected-ledger-sha256',staleBatch.ledger_sha256,'--generation',migrationFile,'--request-id','authored-pack-migration'];
  const migration=ok(migrationArgs).value;assert.equal(migration.state,'needs_attention');assert.deepEqual(migration.model_pack,newSelection.choice);assert.deepEqual(migration.migrations[0].previous_model_pack,liveChoice.choice);assert.deepEqual(migration.migrations[0].current_model_pack,newSelection.choice);assert.deepEqual(migration.migrations[0].superseded_generation_requests,[oldPhase]);assert.deepEqual(ok(migrationArgs).value,migration);
  const resumedBatch=await asyncCli(['stash','resume-batch',batch.batch_request_id]);assert.equal(resumedBatch.status,0,resumedBatch.stderr);const finishedBatch=await observe();assert.equal(finishedBatch.state,'finished',JSON.stringify(finishedBatch));assert.ok(fs.existsSync(path.join(stashMedia,'source.funscript')));
  const batchManifest=sql("import sqlite3,json,sys;d=sqlite3.connect(sys.argv[1]);print(d.execute('SELECT manifest FROM jobs ORDER BY rowid DESC').fetchone()[0])");assert.deepEqual(batchManifest.operation.arguments.model.identity,profile.model);
  assert.ok(batchManifest.dependencies.some(d=>d.sha256===hash));assert.equal(finishedBatch.items[0].requests.filter(r=>r.phase==='generate').length,2);
  const batchApproval=Buffer.from(sql("import sqlite3,json,sys;d=sqlite3.connect(sys.argv[1]);print(json.dumps(d.execute('SELECT report FROM model_pack_selection').fetchone()[0]))"));assert.ok(batchManifest.dependencies.some(d=>d.sha256===sha(batchApproval)));assert.deepEqual(JSON.parse(batchApproval).choice,newSelection.choice);
  const finalSelection=ok(['model','pack-deactivate','--expected-selection-revision','5']).value;assert.equal(finalSelection.selection_revision,6);assert.equal(sql("import sqlite3,json,sys;d=sqlite3.connect(sys.argv[1]);print(json.dumps(d.execute('PRAGMA user_version').fetchone()[0]))"),20);
  await stop();await start();assert.deepEqual(ok(['model','pack-selection-status']).value,finalSelection);assert.equal((await observe()).state,'finished');
  t.diagnostic(JSON.stringify({state:'pack_bound_stash_batch',choice_revisions:[3,5],deactivation_revisions:[4,6],publication_unqualified:true,remote_mutations:mutations,queries,old_reader_schema12_refused:!!schema12,stale_phase_retained_until_explicit_migration:true,worker_fence_schema20_preserved:true}));
  t.diagnostic(JSON.stringify({state:'owner_selected_pack_cpu_generation',binary_sha256:sha(fs.readFileSync(binary)),manifest_sha256:hash,selection_revisions:[1,2],source_preview_frame:4,placed_previews:placedPreviews,preview_tampering_and_deactivation_refused:true,jobs:before[0],attempts:before[1],clone,portable_import_preserves_worker_fence_schema20:true,qualification:'unqualified',physical_dispatch:false}));
});
