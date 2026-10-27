import test from 'node:test';
import {cudaPolicy, configureCudaFixture, promoteCudaFixtureProfile} from './annotated-stash-cuda-fixture.mjs';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import net from 'node:net';
import crypto from 'node:crypto';
import {spawn} from 'node:child_process';
import {setTimeout as delay} from 'node:timers/promises';

const enabled=process.env.PULSAR_STASH_TEST_BINARY && process.env.PULSAR_ANNOTATED_MODEL_FIXTURE_DIR && process.env.PULSAR_ORT_DYLIB;
test(`annotated ${cudaPolicy?'CUDA':'CPU'} preview preserves target pixels, pair support and owner authority`,
  {skip:!enabled,timeout:900000},async t=>{
    const root=fs.mkdtempSync(path.join(os.tmpdir(),'pulsar-annotated-preview-'));fs.chmodSync(root,0o700);
    t.diagnostic(`retained private annotated preview fixture: ${root}`);
    const binary=fs.realpathSync(process.env.PULSAR_STASH_TEST_BINARY),fixture=fs.realpathSync(process.env.PULSAR_ANNOTATED_MODEL_FIXTURE_DIR);
    const hash=bytes=>crypto.createHash('sha256').update(bytes).digest('hex');
    const binaryIdentity={path:binary,sha256:hash(fs.readFileSync(binary)),byte_len:fs.statSync(binary).size};
    const expected=JSON.parse(fs.readFileSync(path.join(fixture,'expected.json'))),pairs=fs.readFileSync(path.join(fixture,'pairs.rgb'));
    assert.equal(hash(pairs),expected.rgb_sha256);
    const media=path.join(root,'media');fs.mkdirSync(media,{mode:0o700});
    const packed=[];for(let i=0;i<28;i++)for(let y=0;y<64;y++)packed.push(Buffer.alloc(96*3,17),Buffer.alloc(16*3,231),pairs.subarray((i*64+y)*64*3,(i*64+y+1)*64*3),Buffer.alloc(16*3,231));
    const raw=path.join(root,'stereo.rgb');fs.writeFileSync(raw,Buffer.concat(packed));const video=path.join(media,'stereo.mkv');
    const env={PATH:'/usr/bin:/bin',PULSAR_STATE_DIR:path.join(root,'engine'),PULSAR_ORT_DYLIB:fs.realpathSync(process.env.PULSAR_ORT_DYLIB)};
    configureCudaFixture(root,env);
    if(process.env.PULSAR_CGROUP_ROOT)env.PULSAR_CGROUP_ROOT=process.env.PULSAR_CGROUP_ROOT;
    const commands=[];
    const run=(executable,args,commandEnv=env)=>new Promise((resolve,reject)=>{
      const p=spawn(executable,args,{env:commandEnv,stdio:['ignore','pipe','pipe']});let out='',err='';
      p.stdout.on('data',b=>{out=(out+b).slice(-1024*1024);});p.stderr.on('data',b=>{err=(err+b).slice(-1024*1024);});p.once('error',reject);
      p.once('exit',code=>{commands.push({executable,args,code});resolve({code,out,err});});
    });
    const cli=args=>run(binary,args);
    const ok=async args=>{const r=await cli(args);assert.equal(r.code,0,r.err+r.out);return JSON.parse(r.out);};
    const encode=await run('/usr/bin/ffmpeg',['-v','error','-nostdin','-f','rawvideo','-pixel_format','rgb24','-video_size','192x64','-framerate','8','-i',raw,'-threads','1','-c:v','ffv1','-output_ts_offset','2',video]);assert.equal(encode.code,0,encode.err);
    let engine,exited,engineError='';
    const start=()=>{engine=spawn(binary,['engine'],{env,stdio:['ignore','ignore','pipe']});engine.stderr.on('data',b=>{engineError=(engineError+b).slice(-65536);});exited=new Promise(resolve=>engine.once('exit',resolve));};
    const stop=async()=>{engine.kill('SIGTERM');await exited;};
    const ready=async()=>{const deadline=Date.now()+15000;while(true){assert.equal(engine.exitCode,null,engineError);assert.ok(Date.now()<deadline,'engine startup deadline');if(fs.existsSync(path.join(env.PULSAR_STATE_DIR,'engine.sock'))&&(await cli(['capabilities'])).code===0)return;await delay(25);}};
    try {
      start();await ready();assert.ok((await ok(['capabilities'])).value.operations.includes('preview_annotated_frame'));const profile=await ok(['model','profile',path.join(fixture,'model.onnx'),'--annotated-temporal-position','--width','64','--height','64','--class-count','1']);
      promoteCudaFixtureProfile(profile);const model=fs.readFileSync(path.join(fixture,'model.onnx')),runtime=fs.readFileSync(env.PULSAR_ORT_DYLIB);assert.equal(hash(runtime),profile.runtime.sha256);
      const supplier=(name,bytes)=>{const dir=path.join(root,name);fs.mkdirSync(dir,{mode:0o700});const p=structuredClone(profile);p.model={artifact_id:hash(bytes),sha256:hash(bytes),byte_len:bytes.length};const files=[['model.onnx','model',bytes],['runtime.bin','runtime',runtime],['profile.json','profile',Buffer.from(JSON.stringify(p))],['LICENSE','license',Buffer.from('Authored verification fixture; no redistribution or quality qualification.')]];for(const [n,,b]of files)fs.writeFileSync(path.join(dir,n),b);const manifest={schema_version:1,pack_id:name,target_os:process.platform,target_arch:process.arch==='x64'?'x86_64':process.arch,ort_api_version:24,labels:['authored-position'],assets:files.map(([n,role,b])=>({path:n,role,byte_len:b.length,sha256:hash(b)})),license_path:'LICENSE',source_description:'Authored annotated CPU correspondence; unqualified'};const encoded=Buffer.from(JSON.stringify(manifest));fs.writeFileSync(path.join(dir,'manifest.json'),encoded);return {dir,manifest,manifestBytes:encoded,sha256:hash(encoded),profile:p,profileBytes:files[2][2]};};
      for(const [name,from,to]of [['bad-recipe','pulsar.temporal_recipe','qulsar.temporal_recipe'],['bad-input','target_frame_pair','targex_frame_pair']]){const bad=Buffer.from(model),offset=bad.indexOf(from);assert.ok(offset>=0);for(let at=offset;at>=0;at=bad.indexOf(from,at+to.length))bad.write(to,at);const candidate=supplier(name,bad);const refused=await cli(['model','pack-import',candidate.dir]);assert.notEqual(refused.code,0);assert.match(refused.err,/annotated|temporal|recipe|ONNX position output differs/);assert.equal((await ok(['model','pack-selection-status'])).value.selection_revision,0);}
      const pack=supplier('authored-annotated-pack',model);const imported=(await ok(['model','pack-import',pack.dir,'--request-id','annotated-pack-import'])).value;assert.equal(imported.active,false);assert.equal(imported.qualified,false);
      const selectArgs=['model','pack-select',pack.sha256,'--expected-selection-revision','0','--approve-unqualified-native-execution','--request-id','annotated-pack-select'];const selected=(await ok(selectArgs)).value;assert.deepEqual((await ok(selectArgs)).value,selected);assert.equal(selected.native_execution_approved,true);for(const key of ['rights_verified','compatible','qualified'])assert.equal(selected[key],false);
      const binding={source_sha256:hash(fs.readFileSync(video)),declaration:{schema_version:1,source_dimensions:[192,64],analysis_dimensions:[96,64],target_region:[16,0,64,64],target_id:'authored-pack-target',video_view:{layout:'side_by_side_left_right',eye:'right',projection:{kind:'flat'}}}};
      const viewFile=path.join(root,'view.json');fs.writeFileSync(viewFile,JSON.stringify(binding));const output=path.join(root,'selected.funscript');const args=['generate',video,'--model-pack',pack.sha256,'--annotated-view',viewFile,'--output',output];
      const absent=await cli(['generate',video,'--model-pack',pack.sha256,'--output',path.join(root,'missing.funscript')]);assert.notEqual(absent.code,0);assert.match(absent.err,/annotated view and explicit annotated profile/);assert.equal(fs.existsSync(path.join(root,'missing.funscript')),false);
      const wrongFile=path.join(root,'wrong.json');fs.writeFileSync(wrongFile,JSON.stringify({...binding,source_sha256:'b'.repeat(64)}));const wrong=await cli(args.map(a=>a===viewFile?wrongFile:a));assert.notEqual(wrong.code,0);assert.match(wrong.err,/not bound to the imported source/);assert.equal(fs.existsSync(path.join(env.PULSAR_STATE_DIR,'snapshots',profile.model.sha256)),false);assert.equal(fs.existsSync(output),false);
      const generated=await cli(args);assert.equal(generated.code,0,generated.err+generated.out);const actions=JSON.parse(fs.readFileSync(output)).actions;assert.equal(actions.length,28);for(let i=0;i<14;i++)assert.equal(actions[i*2+1].pos,Math.round(expected.positions[i]*100));
      const db=path.join(env.PULSAR_STATE_DIR,'projects.sqlite3');const inspect=await run('/usr/bin/python3',['-c',"import sqlite3,json,sys;d=sqlite3.connect('file:'+sys.argv[1]+'?mode=ro',uri=True);j=d.execute('SELECT id,project,source,manifest,state FROM jobs').fetchall();assert len(j)==1;a=d.execute(\"SELECT id FROM attempts WHERE job=? AND state='completed'\",(j[0][0],)).fetchone()[0];print(json.dumps({'job':j[0],'attempt':a,'selection':json.loads(d.execute('SELECT report FROM model_pack_selection WHERE singleton=1').fetchone()[0]),'selection_raw':d.execute('SELECT report FROM model_pack_selection WHERE singleton=1').fetchone()[0],'schema':d.execute('PRAGMA user_version').fetchone()[0]}))",db]);assert.equal(inspect.code,0,inspect.err);const retained=JSON.parse(inspect.out),request=JSON.parse(retained.job[3]);assert.equal(retained.job[4],'completed');assert.equal(retained.schema,23);assert.deepEqual(request.operation.arguments.settings.annotated_view,binding);assert.deepEqual(retained.selection,selected);
      for(const b of [pack.manifestBytes,pack.profileBytes,Buffer.from(retained.selection_raw)]){const digest=hash(b);assert.ok(request.dependencies.some(d=>d.sha256===digest),'missing exact pack/selection dependency');assert.deepEqual(fs.readFileSync(path.join(env.PULSAR_STATE_DIR,'snapshots',digest)),b);}
      const retainedDocuments=request.dependencies.map(d=>{const file=path.join(env.PULSAR_STATE_DIR,'snapshots',d.sha256);if(!fs.existsSync(file)||fs.statSync(file).size>1024*1024)return null;try{return {identity:d,value:JSON.parse(fs.readFileSync(file))};}catch{return null;}}).filter(Boolean);assert.ok(retainedDocuments.some(d=>d.value.choice?.manifest_sha256===pack.sha256&&d.value.selection_revision===selected.selection_revision));
      const attempt=path.join(env.PULSAR_STATE_DIR,'attempts',retained.attempt),receipt=JSON.parse(fs.readFileSync(path.join(attempt,'receipt.json'))),program=JSON.parse(fs.readFileSync(path.join(attempt,'program.json')));assert.equal(receipt.schema_version,3);assert.equal(receipt.qualification,'unqualified');assert.equal(receipt.decoded_sample_count,28);const map=new Map(program.tracks[0].actions.map(a=>[a.time,a])),errors=[];for(let i=0;i<14;i++){const sample=receipt.samples.find(s=>s.source_frame_index===i*2+1);errors.push(Math.abs(map.get(sample.project_time_ns).position-expected.positions[i]));}assert.ok(Math.max(...errors)<=1e-6);
      const project=retained.job[1],source=retained.job[2],before=(await ok(['project','open',project])).value;
      const profileFile=path.join(root,'explicit-profile.json');fs.writeFileSync(profileFile,JSON.stringify(profile));
      const previewArgs=(time,packMode=true)=>['project','preview-annotated',project,String(time),'--source-version',source,'--annotated-view',viewFile,...(packMode?['--model-pack',pack.sha256]:['--model',path.join(fixture,'model.onnx'),'--inference-profile',profileFile])];
      const extract=result=>result.kind==='model_pack_preview'?result.value.preview:result.value;
      const canonicalProfile=await run('/usr/bin/python3',['-c',"import sqlite3,json,hashlib,sys;d=sqlite3.connect('file:'+sys.argv[1]+'?mode=ro',uri=True);m=json.loads(d.execute('SELECT manifest FROM jobs').fetchone()[0]);b=json.dumps(m['operation']['arguments']['settings']['inference_profile'],separators=(',',':')).encode();open(sys.argv[2],'wb').write(b);print(json.dumps({'sha256':hashlib.sha256(b).hexdigest(),'byte_len':len(b)}))",db,path.join(root,'canonical-profile.json')]);assert.equal(canonicalProfile.code,0,canonicalProfile.err);const canonical=JSON.parse(canonicalProfile.out);
      const previews=[],previewErrors=[];
      for(let i=0;i<14;i++){
        const p=extract(await ok(previewArgs((i*2+1)*125000000,i%2===0)));assert.equal(p.source_frame_index,i*2+1);assert.equal(p.observations.length,0);assert.equal(p.coordinate_map,undefined);assert.equal(p.analysis.status,'available');
        const e=p.annotated;assert.deepEqual(e.spec.view,binding);assert.equal(e.previous.source_frame_index,i*2);assert.equal(e.qualified,false);assert.deepEqual(e.model,profile.model);assert.deepEqual(e.runtime,profile.runtime);assert.equal(e.profile.sha256,canonical.sha256);assert.equal(e.profile.byte_len,canonical.byte_len);
        previewErrors.push(Math.abs(e.position-expected.positions[i]));
        assert.deepEqual(fs.readFileSync(p.frame.artifact.path),pairs.subarray((i*2+1)*64*64*3,(i*2+2)*64*64*3));assert.deepEqual(fs.readFileSync(e.previous.frame.artifact.path),pairs.subarray(i*2*64*64*3,(i*2+1)*64*64*3));
        assert.equal(p.source_origin.numerator/p.source_origin.denominator,2);assert.equal(p.elapsed_source_time.numerator/p.elapsed_source_time.denominator,(i*2+1)/8);previews.push(p);
      }
      assert.ok(Math.max(...previewErrors)<=1e-6);assert.equal((await ok(['project','open',project])).value.revision,before.revision);
      const first=extract(await ok(previewArgs(0)));assert.equal(first.annotated.previous,null);assert.equal(first.annotated.position,null);assert.equal(first.analysis.status,'unavailable');
      const gap=extract(await ok([...previewArgs(1000000000),'--source-sample-stride','8']));assert.equal(gap.source_frame_index,8);assert.equal(gap.annotated.previous.source_frame_index,0);assert.equal(gap.annotated.position,null);assert.equal(gap.analysis.status,'unavailable');
      const wrongPreview=await cli(previewArgs(125000000,false).map(a=>a===viewFile?wrongFile:a===path.join(fixture,'model.onnx')?'/must-not-open.onnx':a));assert.notEqual(wrongPreview.code,0);assert.match(wrongPreview.err,/not bound to the imported source/);
      let sequence=0;
      const rpc=(operation,args,project,revision)=>new Promise((resolve,reject)=>{
        const credentials=JSON.parse(fs.readFileSync(path.join(env.PULSAR_STATE_DIR,'first-party.session.json')));
        const data=Buffer.from(JSON.stringify({version:2,request_id:`annotated-preview-rpc-${++sequence}`,...credentials,...(project?{project,...(revision!==undefined?{expected_revision:revision}:{})}:{}),command:{operation,arguments:args}}));
        const header=Buffer.alloc(4);header.writeUInt32BE(data.length);const socket=net.createConnection(path.join(env.PULSAR_STATE_DIR,'engine.sock'));let received=Buffer.alloc(0);socket.setTimeout(cudaPolicy?90000:10000,()=>socket.destroy(new Error('IPC timeout')));socket.on('error',reject);socket.on('close',()=>reject(new Error('IPC closed before response')));socket.on('connect',()=>socket.write(Buffer.concat([header,data])));socket.on('data',b=>{received=Buffer.concat([received,b]);if(received.length>=4&&received.length>=received.readUInt32BE(0)+4){resolve(JSON.parse(received.subarray(4)).result);socket.destroy();}});
      });
      const sourceRequest={context:previews[0].context,time:{kind:'source',source_time:{numerator:17,denominator:8},time_basis:'source_pts'},spec:previews[0].annotated.spec,model:{kind:'pack',choice:selected.choice}};
      const pts=await rpc('preview_annotated_frame',sourceRequest,project);assert.ok(pts.Ok,JSON.stringify(pts.Err));assert.equal(pts.Ok.value.preview.source_frame_index,1);assert.ok(Math.abs(pts.Ok.value.preview.annotated.position-expected.positions[0])<=1e-6);
      const badContext=await rpc('preview_annotated_frame',{...sourceRequest,context:{...sourceRequest.context,transform:'source-identity'}},project);assert.ok(badContext.Err);assert.match(badContext.Err.message,/context differs/);
      let revision=before.revision;const placedPreviews=[];
      for(const rate of [1,-1]){
        const placed=await rpc('set_source_placement',{spec:{source_version:source,source_start:{numerator:0,denominator:1},source_end:{numerator:7,denominator:2},source_anchor:{numerator:rate===1?0:3,denominator:rate===1?1:2},project_anchor:0,rate_numerator:rate,rate_denominator:1},replace:null},project,revision);assert.ok(placed.Ok,JSON.stringify(placed.Err));revision=placed.Ok.value.revision;const id=placed.Ok.value.placements[0].placement_id;
        const p=extract(await ok(['project','preview-annotated',project,'875000000','--placement-id',id,'--annotated-view',viewFile,'--model-pack',pack.sha256]));assert.equal(p.source_frame_index,rate===1?7:5);assert.equal(p.annotated.previous.source_frame_index,rate===1?6:4);assert.ok(Math.abs(p.annotated.position-expected.positions[rate===1?3:2])<=1e-6);assert.equal(p.context.source_placement,id);assert.equal((await ok(['project','open',project])).value.revision,revision);placedPreviews.push(p);
        const removed=await rpc('remove_source_placement',{placement_id:id},project,revision);assert.ok(removed.Ok,JSON.stringify(removed.Err));revision=removed.Ok.value.revision;
      }
      const cutRaw=path.join(root,'cut.rgb'),cutVideo=path.join(root,'cut.mkv');fs.writeFileSync(cutRaw,Buffer.concat([Buffer.alloc(192*64*3,0),Buffer.alloc(192*64*3,255)]));const cutEncode=await run('/usr/bin/ffmpeg',['-v','error','-nostdin','-f','rawvideo','-pixel_format','rgb24','-video_size','192x64','-framerate','4','-i',cutRaw,'-threads','1','-c:v','ffv1',cutVideo]);assert.equal(cutEncode.code,0,cutEncode.err);
      const cutProject=(await rpc('create_project',{name:'Authored cut support'})).Ok.value;const importedCut=await rpc('import_source',{path:cutVideo},cutProject.project_id,cutProject.revision);assert.ok(importedCut.Ok,JSON.stringify(importedCut.Err));const cutHash=hash(fs.readFileSync(cutVideo)),cutView=path.join(root,'cut-view.json');fs.writeFileSync(cutView,JSON.stringify({...binding,source_sha256:cutHash}));
      const cut=extract(await ok(['project','preview-annotated',cutProject.project_id,'250000000','--source-version',cutHash,'--annotated-view',cutView,'--model',path.join(fixture,'model.onnx'),'--inference-profile',profileFile]));assert.equal(cut.source_frame_index,1);assert.equal(cut.annotated.position,null);assert.equal(cut.analysis.status,'unavailable');assert.deepEqual(fs.readFileSync(cut.frame.artifact.path),Buffer.alloc(64*64*3,255));
      await stop();start();await ready();const restarted=extract(await ok(previewArgs(125000000)));assert.ok(Math.abs(restarted.annotated.position-expected.positions[0])<=1e-6);
      const installed=path.join(env.PULSAR_STATE_DIR,'model-packs',pack.sha256,'model.onnx');fs.chmodSync(installed,0o600);fs.writeFileSync(installed,'changed owned fixture');const drift=await cli(previewArgs(125000000));assert.notEqual(drift.code,0);fs.writeFileSync(installed,model);fs.chmodSync(installed,0o400);
      await ok(['model','pack-deactivate','--expected-selection-revision',String(selected.selection_revision)]);const noOwner=await cli(previewArgs(125000000));assert.notEqual(noOwner.code,0);assert.match(noOwner.err,/No offline pack selected/);const noOwnerRpc=await rpc('preview_annotated_frame',sourceRequest,project);assert.ok(noOwnerRpc.Err);assert.match(noOwnerRpc.Err.message,/current owner selection/);
      const count=await run('/usr/bin/python3',['-c',"import sqlite3,sys;d=sqlite3.connect('file:'+sys.argv[1]+'?mode=ro',uri=True);assert d.execute('SELECT COUNT(*) FROM jobs').fetchone()[0]==1;assert d.execute('SELECT COUNT(*) FROM attempts').fetchone()[0]==1",db]);assert.equal(count.code,0,count.err);
      const after=(await ok(['project','open',project])).value;assert.equal(after.revision,revision);assert.deepEqual(after.motion.program,before.motion.program);assert.equal(hash(fs.readFileSync(binary)),binaryIdentity.sha256);
      const cudaPreviews=[];
      if(cudaPolicy){
        for(const [label,p]of [...previews.map((p,i)=>[`pair-${i}`,p]),['first',first],['gap',gap],['cut',cut],['restarted',restarted],['source-pts',pts.Ok.value.preview],...placedPreviews.map((p,i)=>[`placed-${i}`,p])]){
          const directory=path.dirname(p.frame.artifact.path),receiptBytes=fs.readFileSync(path.join(directory,'cuda-inference.json')),traceBytes=fs.readFileSync(path.join(directory,'cuda-trace.json'));
          const r=JSON.parse(receiptBytes),trace=JSON.parse(traceBytes),nodes=trace.filter(e=>e.cat==='Node');const inferred=p.annotated.position!==null;
          assert.equal(r.successful_runs,inferred?1:0);assert.equal(r.provider_evidence,inferred?'strict-cuda-node-trace':'cuda-session-initialized-without-inference');
          if(inferred){assert.ok(nodes.length>0);assert.ok(nodes.every(n=>n.args.provider==='CUDAExecutionProvider'));}else{assert.equal(nodes.length,0);assert.ok(trace.every(e=>e.cat==='Session'&&['model_loading_uri','model_loading_array','session_initialization'].includes(e.name)));assert.equal(r.trace.schema_version,2);}
          cudaPreviews.push({label,directory,successful_runs:r.successful_runs,cuda_nodes:nodes.length,provider_evidence:r.provider_evidence,receipt_sha256:hash(receiptBytes),trace_sha256:hash(traceBytes)});
        }
      }
      fs.writeFileSync(path.join(root,'result.json'),JSON.stringify({status:'PASS',provider:cudaPolicy?'cuda':'cpu',cudaPreviews,scope:'Authored target preview through shipped CLI and local IPC; no natural media or physical playback',binary:binaryIdentity,pack,selected,canonical,retained,request,receipt,program,previews,placedPreviews,first,gap,cut,restarted,source_pts_preview:pts.Ok.value.preview,python_positions:14,maximum_difference:Math.max(...previewErrors),exact_previous_current_rgb_pairs:14,nonzero_source_origin:2,wrong_source_before_model_access:true,wrong_context_refused:true,model_drift_refused:true,removed_approval_refused:true,no_new_jobs_or_authoritative_attempts:true,master_motion_unchanged:true,commands},null,2));
    } finally {if(engine?.exitCode===null)await stop();fs.writeFileSync(path.join(root,'commands.json'),JSON.stringify(commands,null,2));fs.writeFileSync(path.join(root,'engine.log'),engineError);}
  });
