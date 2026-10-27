import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import http from 'node:http';
import net from 'node:net';
import crypto from 'node:crypto';
import {spawn} from 'node:child_process';
import {setTimeout as delay} from 'node:timers/promises';

const enabled=process.env.PULSAR_STASH_TEST_BINARY && process.env.PULSAR_ANNOTATED_MODEL_FIXTURE_DIR && process.env.PULSAR_ORT_DYLIB;
test('annotated native Stash publication preserves clocks and unknown scan acknowledgement through restart',
  {skip:!enabled,timeout:90000},async t=>{
    const root=fs.mkdtempSync(path.join(os.tmpdir(),'pulsar-stash-annotated-'));fs.chmodSync(root,0o700);
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
    let interactive=false;const queries=[];
    const server=http.createServer(async(req,res)=>{
      const chunks=[];for await(const b of req)chunks.push(b);const body=JSON.parse(Buffer.concat(chunks));queries.push(body);
      let data;
      if(body.query.includes('metadataScan')) {assert.deepEqual(body.variables.input.paths,['/stash/stereo.mkv']);assert.equal(body.variables.input.rescan,true);for(const [key,value] of Object.entries(body.variables.input))if(key.startsWith('scanGenerate'))assert.equal(value,false);interactive=true;data={metadataScan:null};}
      else {assert.ok(body.query.trim().startsWith('query'),'only the explicitly submitted owned scan may mutate');data={findScene:{id:'s1',title:'Authored annotated fixture',interactive,files:[{id:'f1',path:'/stash/stereo.mkv'}]}};}
      res.writeHead(200,{'Content-Type':'application/json',Connection:'close'});res.end(JSON.stringify({data}));
    });
    await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));const url=`http://127.0.0.1:${server.address().port}/graphql`;
    let engine,exited,engineError='';
    const start=()=>{engine=spawn(binary,['engine'],{env,stdio:['ignore','ignore','pipe']});engine.stderr.on('data',b=>{engineError=(engineError+b).slice(-65536);});exited=new Promise(resolve=>engine.once('exit',resolve));};
    const stop=async()=>{engine.kill('SIGTERM');await exited;};
    const ready=async()=>{const deadline=Date.now()+15000;while(true){assert.equal(engine.exitCode,null,engineError);assert.ok(Date.now()<deadline,'engine startup deadline');if(fs.existsSync(path.join(env.PULSAR_STATE_DIR,'engine.sock'))&&(await cli(['capabilities'])).code===0)return;await delay(25);}};
    let sequence=0;
    const rpc=(project,revision,operation,args)=>new Promise((resolve,reject)=>{
      const credentials=JSON.parse(fs.readFileSync(path.join(env.PULSAR_STATE_DIR,'first-party.session.json')));
      const bytes=Buffer.from(JSON.stringify({version:2,request_id:`annotated-stash-${++sequence}`,...credentials,project,expected_revision:revision,command:{operation,arguments:args}}));const header=Buffer.alloc(4);header.writeUInt32BE(bytes.length);
      const socket=net.createConnection(path.join(env.PULSAR_STATE_DIR,'engine.sock'));let received=Buffer.alloc(0);socket.setTimeout(15000,()=>socket.destroy(new Error('IPC deadline')));socket.once('error',reject);socket.once('close',()=>reject(new Error('IPC closed before response')));socket.once('connect',()=>socket.write(Buffer.concat([header,bytes])));
      socket.on('data',b=>{received=Buffer.concat([received,b]);if(received.length<4)return;const n=received.readUInt32BE();if(n>1024*1024){socket.destroy(new Error('IPC response limit'));return;}if(received.length>=n+4){resolve(JSON.parse(received.subarray(4,n+4)).result);socket.destroy();}});
    });
    try {
      start();await ready();const project=(await ok(['project','create','Annotated Stash fixture'])).value.project_id;
      const mappings=path.join(root,'mappings.json');fs.writeFileSync(mappings,JSON.stringify([{server_root:'/stash',local_root:media}]));
      const capture=(await ok(['stash','capture-source',project,'s1','f1','--url',url,'--mappings',mappings])).value;
      const spec={source_version:capture.source_version,source_start:{numerator:0,denominator:1},source_end:{numerator:4,denominator:1},source_anchor:{numerator:0,denominator:1},project_anchor:2000000001,rate_numerator:3,rate_denominator:1};
      const placementFile=path.join(root,'placement.json');fs.writeFileSync(placementFile,JSON.stringify(spec));const placed=(await ok(['project','place',project,'--spec',placementFile])).value;
      const profile=(await ok(['model','profile',path.join(fixture,'model.onnx'),'--annotated-temporal-position','--width','64','--height','64','--class-count','1']));
      const binding={source_sha256:capture.identity.sha256,declaration:{schema_version:1,source_dimensions:[192,64],analysis_dimensions:[96,64],target_region:[16,0,64,64],target_id:'authored',video_view:{layout:'side_by_side_left_right',eye:'right',projection:{kind:'flat'}}}};
      const settings={annotated_view:binding,source_placement:placed.placements[0],analysis_width:64,analysis_height:64,source_sample_stride:1,axis:'stroke',vr_mode:false,model_input:profile.preprocessing.input,inference_profile:profile};
      const generate=async(currentPlacement,base)=>{
        const submitted=await rpc(project,base,'generate',{source_version:capture.source_version,preset:'fast',model_path:path.join(fixture,'model.onnx'),settings:{...settings,source_placement:currentPlacement}});assert.ok(submitted.Ok,JSON.stringify(submitted.Err));const job=submitted.Ok.value.job_id;
        let status;const deadline=Date.now()+30000;while(true){const result=await rpc(project,null,'job_status',{job_id:job});assert.ok(result.Ok,JSON.stringify(result.Err));status=result.Ok.value;if(status.state==='completed')break;assert.ok(['queued','running'].includes(status.state),JSON.stringify(status));assert.ok(Date.now()<deadline,'generation deadline');await delay(25);}
        const candidate=status.candidate_id;const committed=await rpc(project,base,'commit_candidate',{candidate_id:candidate});assert.ok(committed.Ok,JSON.stringify(committed.Err));return {job,candidate,attempt:status.attempt_id,committed:committed.Ok.value};
      };
      const publicationArgs=(g,id)=>['stash','publish-script',project,'s1','f1','--url',url,'--source-version',capture.source_version,'--candidate',g.candidate,'--job',g.job,'--revision',String(g.committed.revision),'--request-id',id];
      const gapped=await generate(placed.placements[0],placed.revision);
      const boundaryRefused=await cli([...publicationArgs(gapped,'annotated-boundary-gap'),'--source-time']);assert.notEqual(boundaryRefused.code,0);assert.match(boundaryRefused.err,/gap-resolved motion/);assert.equal(fs.existsSync(path.join(media,'stereo.funscript')),false);
      // Explicitly shorten the authored trim; no gap is silently discarded by publication.
      fs.writeFileSync(placementFile,JSON.stringify({...spec,source_end:{numerator:3375000001,denominator:1000000000}}));
      const trimmed=(await ok(['project','place',project,'--spec',placementFile,'--replace',placed.placements[0].placement_id])).value;
      const generated=await generate(trimmed.placements[0],trimmed.revision);const {job,candidate}=generated;const committed=generated.committed;const revision=generated.committed.revision;
      const originating=path.join(env.PULSAR_STATE_DIR,'attempts',generated.attempt);
      const workerRequest=JSON.parse(fs.readFileSync(path.join(originating,'initial-checkpoint.json'))),workerReceipt=JSON.parse(fs.readFileSync(path.join(originating,'receipt.json'))),workerProgram=JSON.parse(fs.readFileSync(path.join(originating,'program.json')));
      assert.equal(workerReceipt.schema_version,3);assert.equal(workerReceipt.qualification,'unqualified');assert.equal(workerReceipt.decoded_sample_count,28);assert.deepEqual(workerReceipt.annotated_input.native_plan.declaration,binding.declaration);
      const programActions=new Map(workerProgram.tracks[0].actions.map(a=>[a.time,a]));const errors=[];
      for(let i=0;i<14;i++){const sample=workerReceipt.samples.find(s=>s.source_frame_index===i*2+1),action=programActions.get(sample.project_time_ns);assert.equal(action.evidence,'inferred');errors.push(Math.abs(action.position-expected.positions[i]));}assert.ok(Math.max(...errors)<=1e-6);
      const args=publicationArgs(generated,'annotated-publication');
      const refused=await cli(args);assert.notEqual(refused.code,0);assert.match(refused.err,/explicit source-time export/);assert.equal(fs.existsSync(path.join(media,'stereo.funscript')),false);
      const publication=(await ok([...args,'--source-time'])).value;assert.equal(publication.release_qualified,false);assert.equal(publication.timing_proof.mapped_actions,28);assert.equal(publication.timing_proof.reused_sample_clocks,28);
      const script=fs.readFileSync(publication.path),actions=JSON.parse(script).actions;assert.deepEqual(actions.map(a=>a.at),Array.from({length:28},(_,i)=>i*125));for(let i=0;i<14;i++)assert.equal(actions[2*i+1].pos,Math.round(expected.positions[i]*100));
      const proofBytes=fs.readFileSync(publication.timing_proof.path),proof=JSON.parse(proofBytes);assert.equal(hash(proofBytes),publication.timing_proof.identity.sha256);assert.equal(proof.release_qualified,false);assert.equal(proof.context.original_candidate,candidate);
      const master=(await ok(['project','open',project])).value;assert.equal(master.revision,revision);assert.deepEqual(master.motion,committed.motion);
      const count=queries.length;assert.deepEqual((await ok([...args,'--source-time'])).value,publication);assert.equal(queries.length,count,'publication replay must not query Stash');
      const scanArgs=['stash','submit-scan',project,'--url',url,'--publication-request-id','annotated-publication','--request-id','annotated-scan'];
      const unknownScan=async()=>{const result=await cli(scanArgs);assert.notEqual(result.code,0);assert.match(result.err,/acknowledgement is unknown/);return JSON.parse(result.out).value;};
      const scan=await unknownScan();assert.equal(scan.state,'dispatch_unknown');assert.equal(scan.remote_job_id,null);assert.equal(queries.filter(q=>q.query.includes('metadataScan')).length,1);
      const attachmentArgs=['stash','reconcile-attachment',project,'--url',url,'--scan-request-id','annotated-scan'];const observed=(await ok(attachmentArgs)).value;assert.equal(observed.state,'dispatch_unknown');assert.equal(observed.attachment.state,'observed_linked');assert.equal(observed.attachment.remote_script_hash_verified,false);
      await stop();start();await ready();const replay=await unknownScan();assert.equal(replay.state,'dispatch_unknown');assert.equal(queries.filter(q=>q.query.includes('metadataScan')).length,1);
      const restored=(await ok(['stash','scan-status',project,'--url',url,'--scan-request-id','annotated-scan'])).value;assert.equal(restored.attachment.state,'observed_linked');assert.equal(restored.state,'dispatch_unknown');
      interactive=false;const negative=(await ok(attachmentArgs)).value;assert.equal(negative.attachment.state,'unattached');assert.equal(negative.state,'dispatch_unknown');
      const duplicate=await cli([...scanArgs.slice(0,-1),'different-scan']);assert.notEqual(duplicate.code,0);assert.match(duplicate.err,/already has a scan dispatch/);assert.equal(queries.filter(q=>q.query.includes('metadataScan')).length,1);
      assert.deepEqual(fs.readFileSync(publication.path),script);assert.deepEqual(fs.readFileSync(publication.timing_proof.path),proofBytes);assert.equal(hash(fs.readFileSync(binary)),binaryIdentity.sha256);
      const allowed=await cli(['allow-packaging',project,'--acknowledge-unencrypted-private-data']);assert.equal(allowed.code,0,allowed.err);const archive=path.join(root,'annotated.pulsar');
      const exported=await cli(['package-export',project,archive,'--acknowledge-unencrypted-private-data']);assert.equal(exported.code,0,exported.err);assert.equal(fs.readFileSync(archive).readUInt16LE(8),11);
      const clone=(await ok(['package-import',archive])).receipt.project_id;
      const mapped=await run('/usr/bin/python3',['-c',"import sqlite3,json,sys;d=sqlite3.connect('file:'+sys.argv[1]+'?mode=ro',uri=True);m=json.loads(d.execute('SELECT manifest FROM project_import_archives WHERE project=?',(sys.argv[2],)).fetchone()[0]);e=next(e for e in m['imported_origins'] if e['container']['project_id']==sys.argv[3]);print(next(c['local'] for c in e['candidates'] if c['origin']==sys.argv[4]));assert d.execute('SELECT COUNT(*) FROM jobs WHERE project=?',(sys.argv[2],)).fetchone()[0]==0",path.join(env.PULSAR_STATE_DIR,'projects.sqlite3'),clone,project,candidate]);assert.equal(mapped.code,0,mapped.err);const alias=mapped.out.trim();
      const cloneState=(await ok(['project','open',clone])).value;
      const report=(await ok(['project','archived-generation-status',clone,alias,'--master-revision',String(cloneState.revision)])).value;assert.equal(report.origins.length,1);assert.equal(report.active_job,false);assert.equal(report.publication_authority,false);assert.equal(report.origins[0].clock.sample_count,28);assert.deepEqual(report.origins[0].settings.annotated_view,binding);
      const reboundMedia=path.join(root,'rebound-media');fs.mkdirSync(reboundMedia,{mode:0o700});fs.copyFileSync(video,path.join(reboundMedia,'stereo.mkv'));
      const reboundMappings=path.join(root,'rebound-mappings.json');fs.writeFileSync(reboundMappings,JSON.stringify([{server_root:'/stash',local_root:reboundMedia}]));
      const reference=path.join(root,'origin.json');fs.writeFileSync(reference,JSON.stringify(report.origins[0].origin_manifest));
      const archivedArgs=['stash','publish-script',clone,'s1','f1','--url',url,'--source-version',capture.source_version,'--candidate',alias,'--job',job,'--revision',String(cloneState.revision),'--archived-origin',reference,'--source-time','--request-id','annotated-archived-publication'];
      const unbound=await cli(archivedArgs);assert.notEqual(unbound.code,0);assert.match(unbound.err,/captured source association is absent/);assert.equal(fs.existsSync(path.join(reboundMedia,'stereo.funscript')),false);
      const rebound=(await ok(['stash','capture-source',clone,'s1','f1','--url',url,'--mappings',reboundMappings,'--expected-source-version',capture.source_version,'--revision',String(cloneState.revision)])).value;assert.equal(rebound.identity.sha256,capture.identity.sha256);
      const attempts=path.join(env.PULSAR_STATE_DIR,'attempts'),parked=path.join(env.PULSAR_STATE_DIR,'attempts-parked'),model=workerRequest.operation.arguments.model.path,parkedModel=model+'.parked';
      assert.ok(model.startsWith(path.join(env.PULSAR_STATE_DIR,'snapshots')+path.sep));let archival,attemptsParked=false,modelParked=false;
      try{fs.renameSync(attempts,parked);attemptsParked=true;fs.renameSync(model,parkedModel);modelParked=true;archival=(await ok(archivedArgs)).value;}finally{try{if(modelParked)fs.renameSync(parkedModel,model);}finally{if(attemptsParked)fs.renameSync(parked,attempts);}}
      assert.deepEqual(fs.readFileSync(archival.path),script);assert.equal(archival.release_qualified,false);assert.equal(archival.timing_proof.archived_context.active_job,false);assert.equal(archival.timing_proof.archived_context.archival,true);assert.equal(archival.timing_proof.archived_context.sampled_clocks_verified,true);
      const archivedProof=JSON.parse(fs.readFileSync(archival.timing_proof.path));assert.equal(archivedProof.schema_version,3);assert.equal(archivedProof.mappings.length,28);assert.deepEqual(archivedProof.context.archived_context.origin.settings.annotated_view,binding);
      await stop();start();await ready();assert.deepEqual((await ok(archivedArgs)).value,archival);
      const archiveObserved=(await ok(['stash','publication-status',clone,'--publication-request-id','annotated-archived-publication'])).value;assert.equal(archiveObserved.proof_observation,'verified');assert.equal(queries.filter(q=>q.query.includes('metadataScan')).length,1);
      fs.writeFileSync(path.join(root,'result.json'),JSON.stringify({status:'PASS',scope:'Authored native CPU/CLI/local IPC and owned loopback Stash; no installed Stash or physical playback',binary:binaryIdentity,source_sha256:capture.identity.sha256,boundary_gap_refused:true,explicit_trim_required:true,schema3_clocks:28,source_time_actions:28,python_rounded_positions:14,python_program_positions:14,maximum_program_difference:Math.max(...errors),archived_publication:true,archived_job_inert:true,original_attempts_and_model_unavailable:true,archive_requires_fresh_binding:true,scan_mutations:1,unknown_scan_preserved:true,attachment_observation_separate:true,negative_observation_preserved_unknown:true,master_unchanged:true,publication,proof,archival,archivedProof,queries,commands},null,2));
    } finally {if(engine?.exitCode===null)await stop();await new Promise(resolve=>server.close(resolve));fs.writeFileSync(path.join(root,'commands.json'),JSON.stringify(commands,null,2));fs.writeFileSync(path.join(root,'engine.log'),engineError);}
  });
