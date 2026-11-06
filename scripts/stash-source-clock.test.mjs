import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import http from 'node:http';
import net from 'node:net';
import crypto from 'node:crypto';
import {spawn} from 'node:child_process';
import {setTimeout as delay} from 'node:timers/promises';

test('native explicit Stash source-clock publication retains proof and replays after restart',
  {skip:!process.env.PULSAR_STASH_TEST_BINARY,timeout:180000},async t=>{
    const root=fs.mkdtempSync(path.join(os.tmpdir(),'pulsar-cp-'));fs.chmodSync(root,0o700);
    t.diagnostic(`retained private source-clock fixture: ${root}`);
    const media=path.join(root,'media');fs.mkdirSync(media);
    const frameBytes=Buffer.concat([Buffer.from('YUV4MPEG2 W64 H64 F8:1 Ip A1:1 Cmono\n'),...Array.from({length:8},(_,frame)=>Buffer.concat([Buffer.from('FRAME\n'),Buffer.from(Array.from({length:64*64},(_,i)=>{const x=i%64,y=Math.floor(i/64),sy=(y+64-frame)%64;return (x*31+sy*17+Math.floor(x/7)*Math.floor(sy/5)*13)%256;}))]))]);
    fs.writeFileSync(path.join(media,'mock.mp4'),frameBytes);
    const queries=[];
    const server=http.createServer(async(req,res)=>{const parts=[];for await(const b of req)parts.push(b);const body=JSON.parse(Buffer.concat(parts));queries.push(body);assert.ok(body.query.trim().startsWith('query'));
      res.writeHead(200,{'Content-Type':'application/json',Connection:'close'});res.end(JSON.stringify({data:{findScene:{id:'s1',title:'Clock fixture',interactive:false,files:[{id:'f1',path:'/stash/mock.mp4'}]}}}));});
    await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
    const url=`http://127.0.0.1:${server.address().port}/graphql`,binary=fs.realpathSync(process.env.PULSAR_STASH_TEST_BINARY);
    const env={PATH:'/usr/bin:/bin',PULSAR_STATE_DIR:path.join(root,'engine')};
    let engine,exit,stderr='';
    const start=()=>{engine=spawn(binary,['engine'],{env,stdio:['ignore','ignore','pipe']});engine.stderr.on('data',b=>{stderr=(stderr+b).slice(-65536);});exit=new Promise(resolve=>engine.once('exit',resolve));};
    const cli=(args,executable=binary)=>new Promise((resolve,reject)=>{const p=spawn(executable,args,{env,stdio:['ignore','pipe','pipe']});const timer=setTimeout(()=>p.kill('SIGKILL'),30000);let out='',err='';p.stdout.on('data',b=>out+=b);p.stderr.on('data',b=>err+=b);p.once('error',error=>{clearTimeout(timer);reject(error);});p.once('exit',code=>{clearTimeout(timer);resolve({code,out,err});});});
    const ready=async()=>{const deadline=Date.now()+15000;while(true){assert.equal(engine.exitCode,null,stderr);assert.equal(engine.signalCode,null,stderr);assert.ok(Date.now()<deadline,'engine deadline: '+stderr);if(fs.existsSync(path.join(env.PULSAR_STATE_DIR,'engine.sock'))&&(await cli(['capabilities'])).code===0)return;await delay(25);}};
    let sequence=0;
    const rpc=(project,revision,operation,args,overrideCredentials)=>new Promise((resolve,reject)=>{const credentials=overrideCredentials??JSON.parse(fs.readFileSync(path.join(env.PULSAR_STATE_DIR,'first-party.session.json')));const id=`clock-${++sequence}`;
      const bytes=Buffer.from(JSON.stringify({version:2,request_id:id,...credentials,project,expected_revision:revision,command:{operation,arguments:args}}));const header=Buffer.alloc(4);header.writeUInt32BE(bytes.length);
      const socket=net.createConnection(path.join(env.PULSAR_STATE_DIR,'engine.sock'));let received=Buffer.alloc(0);socket.setTimeout(10000,()=>socket.destroy(new Error('IPC deadline')));socket.once('error',reject);socket.once('close',()=>reject(new Error('IPC closed before response')));socket.once('connect',()=>socket.write(Buffer.concat([header,bytes])));socket.on('data',b=>{received=Buffer.concat([received,b]);if(received.length<4)return;const size=received.readUInt32BE();if(size>1024*1024){socket.destroy(new Error('IPC size'));return;}if(received.length>=size+4){resolve(JSON.parse(received.subarray(4,size+4)).result);socket.destroy();}});});
    const ok=async args=>{const result=await cli(args);assert.equal(result.code,0,result.err+result.out);return JSON.parse(result.out);};
    try {
      start();await ready();let forwardPositions;
      const paired=await rpc(null,null,'pair',{client_name:'clock grant fixture',pairing_token:fs.readFileSync(path.join(env.PULSAR_STATE_DIR,'pairing.token'),'utf8').trim()});assert.ok(paired.Ok,'fixture pairing must succeed');const grantCredentials=paired.Ok.value;
      const scopes=['read','edit','generate','export','manage_grants','manage_protection','import_source','package_project'];
      for(const rate of [3,-3]) {
      fs.rmSync(path.join(media,'mock.funscript'),{force:true});
      const created=await ok(['project','create',`source-clock publication ${rate}`]);const project=created.value.project_id;
      assert.ok((await rpc(project,null,'grant',{session:grantCredentials.session,project_id:project,scopes})).Ok,'fixture manager grant must succeed');
      const mappings=path.join(root,'mappings.json');fs.writeFileSync(mappings,JSON.stringify([{server_root:'/stash',local_root:media}]));
      const captured=await ok(['stash','capture-source',project,'s1','f1','--url',url,'--mappings',mappings]);const source=captured.value.source_version;
      const spec={source_version:source,source_start:{numerator:0,denominator:1},source_end:{numerator:875000001,denominator:1000000000},source_anchor:{numerator:0,denominator:1},project_anchor:2000000001,rate_numerator:rate,rate_denominator:1};
      const file=path.join(root,'placement.json');fs.writeFileSync(file,JSON.stringify(spec));const placed=await ok(['project','place',project,'--spec',file]);const placement=placed.value.placements[0],base=placed.value.revision;
      const generated=await rpc(project,base,'generate',{source_version:source,preset:'fast',model_path:null,settings:{source_placement:placement,analysis_width:64,analysis_height:64,source_sample_stride:1,axis:'stroke',vr_mode:false,model_input:null}});assert.ok(generated.Ok,JSON.stringify(generated.Err));
      const job=generated.Ok.value.job_id;let status;const deadline=Date.now()+30000;
      while(true){const response=await rpc(project,null,'job_status',{job_id:job});assert.ok(response.Ok,JSON.stringify(response.Err));status=response.Ok.value;if(status.state==='completed')break;assert.ok(['queued','running'].includes(status.state),JSON.stringify(status));assert.ok(Date.now()<deadline,'generation deadline');await delay(50);}
      const candidate=status.candidate_id;const committed=await rpc(project,base,'commit_candidate',{candidate_id:candidate});assert.ok(committed.Ok,JSON.stringify(committed.Err));
      let before=await ok(['project','open',project]);let retimeReceipt;
      if(rate===3){
        const changed={...spec,rate_numerator:-2,project_anchor:3000000000};fs.writeFileSync(file,JSON.stringify(changed));const replaced=await ok(['project','place',project,'--spec',file,'--replace',placement.placement_id]);const target=replaced.value.placements[0].placement_id;
        before=await ok(['project','retime',project,placement.placement_id,target,'--axes','stroke','--revision',String(replaced.value.revision),'--request-id','source-clock-retime-fixture']);
        retimeReceipt=(await ok(['project','retime-status',project,'--revision',String(before.value.revision)])).value;
      }
      const revision=before.value.revision;
      const args=['stash','publish-script',project,'s1','f1','--url',url,'--source-version',source,'--candidate',candidate,'--job',job,'--revision',String(revision),'--request-id',`clock-publication-${rate}`];
      if(retimeReceipt){
        const db=path.join(env.PULSAR_STATE_DIR,'projects.sqlite3');
        const corrupt=await cli(['-c',"import sqlite3,sys; d=sqlite3.connect(sys.argv[1]); row=d.execute('SELECT sha256 FROM motion_retime_receipts WHERE project=? AND revision=?',(sys.argv[2],int(sys.argv[3]))).fetchone(); d.execute('UPDATE motion_retime_receipts SET sha256=? WHERE project=? AND revision=?',('0'*64,sys.argv[2],int(sys.argv[3]))); d.commit(); print(row[0])",db,project,String(retimeReceipt.revision)],'/usr/bin/python3');
        assert.equal(corrupt.code,0,corrupt.err);const refused=await cli([...args.map(a=>a===`clock-publication-${rate}`?'corrupt-retime-publication':a),'--source-time']);assert.notEqual(refused.code,0);assert.match(refused.err,/DependencyMismatch/);assert.equal(fs.existsSync(path.join(media,'mock.funscript')),false);
        const restored=await cli(['-c',"import sqlite3,sys; d=sqlite3.connect(sys.argv[1]); d.execute('UPDATE motion_retime_receipts SET sha256=? WHERE project=? AND revision=?',(sys.argv[4],sys.argv[2],int(sys.argv[3]))); d.commit()",db,project,String(retimeReceipt.revision),corrupt.out.trim()],'/usr/bin/python3');assert.equal(restored.code,0,restored.err);
      }

      const refused=await cli([...args.slice(0,-1),`clock-default-refused-${rate}`]);assert.notEqual(refused.code,0);assert.match(refused.err,/explicit source-time export/);assert.equal(fs.existsSync(path.join(media,'mock.funscript')),false);
      const published=await ok([...args,'--source-time']);const receipt=published.value;assert.equal(receipt.release_qualified,false);assert.equal(receipt.timing_proof.mapped_actions,8);assert.equal(receipt.timing_proof.reused_sample_clocks,8);
      const script=fs.readFileSync(receipt.path),actions=JSON.parse(script).actions;assert.deepEqual(actions.map(a=>a.at),[0,125,250,375,500,625,750,875]);
      if(rate===3)forwardPositions=actions.map(a=>a.pos);else assert.deepEqual(actions.map(a=>a.pos),forwardPositions,'reverse placement restores source-order positions');
      const proofBytes=fs.readFileSync(receipt.timing_proof.path);assert.equal(crypto.createHash('sha256').update(proofBytes).digest('hex'),receipt.timing_proof.identity.sha256);assert.equal(proofBytes.length,receipt.timing_proof.identity.byte_len);
      const proof=JSON.parse(proofBytes);assert.equal(proof.context.master_sha256,before.value.motion.program.sha256);assert.equal(proof.context.script.sha256,receipt.identity.sha256);assert.equal(proof.context.placement.placement_id,placement.placement_id);assert.equal(proof.release_qualified,false);assert.equal(proof.schema_version,4);assert.equal(proof.context.action_ancestry.selections.length,proof.mappings.length);assert.ok(proof.context.action_ancestry.carriers.length>0);
      if(retimeReceipt){assert.deepEqual(proof.context.retimings,[retimeReceipt]);assert.equal(proof.context.retimings[0].after.sha256,before.value.motion.program.sha256);}
      const after=await ok(['project','open',project]);assert.deepEqual(after.value.motion,before.value.motion);assert.equal(after.value.revision,revision);
      let queryCount=queries.length;assert.deepEqual((await ok([...args,'--source-time'])).value,receipt);assert.equal(queries.length,queryCount);
      const observationArgs=['stash','publication-status',project,'--publication-request-id',`clock-publication-${rate}`];
      const observe=async()=>{const response=await ok(observationArgs);assert.equal(response.kind,'stash_publication');assert.equal(queries.length,queryCount);return response.value;};
      let observation=await observe();assert.equal(observation.dispatch,'complete');assert.equal(observation.proof_observation,'verified');assert.equal(observation.source_and_script_observation,'verified');assert.ok(observation.observed_at_unix_ms>0);
      fs.writeFileSync(receipt.path,'changed-script');observation=await observe();assert.equal(observation.source_and_script_observation,'unverified');assert.equal(observation.proof_observation,'verified');fs.writeFileSync(receipt.path,script);
      fs.writeFileSync(receipt.timing_proof.path,'changed-proof');observation=await observe();assert.equal(observation.proof_observation,'unverified');assert.equal(observation.source_and_script_observation,'verified');fs.writeFileSync(receipt.timing_proof.path,proofBytes);
      // Rehash forged proof bytes and update both retained references. Refusal
      // must come from semantic rederivation, not merely a stale content digest.
      for(const mutation of ['carrier','selection','mapping','downgrade']) {
        const backup=path.join(root,`proof-reference-${rate}-${mutation}.json`);
        const forge=await cli(['-c',`import sqlite3,json,sys,hashlib,pathlib
D=sqlite3.connect(sys.argv[1]);project,request,mutation,backup=sys.argv[2:]
session,raw,response=D.execute('SELECT e.session,p.proof,r.response FROM exports e JOIN stash_clock_proofs p USING(session,request) JOIN requests r USING(session,request) WHERE e.project=? AND e.request=?',(project,request)).fetchone()
meta=json.loads(raw);proof=json.loads(pathlib.Path(meta['path']).read_bytes())
if mutation=='carrier': proof['context']['action_ancestry']['carriers'][0]['original_receipt']['sha256']='0'*64
elif mutation=='selection': proof['context']['action_ancestry']['selections'][0]['carrier']=2048
elif mutation=='mapping': proof['mappings'][0]['original_source_frame']=777
else: proof['schema_version']=2
payload=json.dumps(proof,separators=(',',':')).encode();hash=hashlib.sha256(payload).hexdigest();file=pathlib.Path(meta['path']).parent/(hash+'.json');file.write_bytes(payload)
meta['identity']={'artifact_id':hash,'sha256':hash,'byte_len':len(payload)};meta['path']=str(file)
reply=json.loads(response)
def replace(value):
 if isinstance(value,dict):
  for key,item in list(value.items()):
   if key=='timing_proof': value[key]=meta
   else: replace(item)
 elif isinstance(value,list):
  for item in value: replace(item)
replace(reply)
pathlib.Path(backup).write_text(json.dumps({'session':session,'request':request,'proof':raw,'response':response,'forged_file':str(file)}))
D.execute('UPDATE stash_clock_proofs SET proof=? WHERE session=? AND request=?',(json.dumps(meta),session,request));D.execute('UPDATE requests SET response=? WHERE session=? AND request=?',(json.dumps(reply),session,request));D.commit()
`,path.join(env.PULSAR_STATE_DIR,'projects.sqlite3'),project,`clock-publication-${rate}`,mutation,backup],'/usr/bin/python3');
        assert.equal(forge.code,0,forge.err);
        try {const checked=await observe();assert.equal(checked.proof_observation,'unverified',mutation);fs.writeFileSync(path.join(root,`proof-refused-${rate}-${mutation}.json`),JSON.stringify(checked,null,2));}
        finally {
          const restored=await cli(['-c',`import sqlite3,json,sys,pathlib
D=sqlite3.connect(sys.argv[1]);b=json.loads(pathlib.Path(sys.argv[2]).read_text());D.execute('UPDATE stash_clock_proofs SET proof=? WHERE session=? AND request=?',(b['proof'],b['session'],b['request']));D.execute('UPDATE requests SET response=? WHERE session=? AND request=?',(b['response'],b['session'],b['request']));D.commit()
`,path.join(env.PULSAR_STATE_DIR,'projects.sqlite3'),backup],'/usr/bin/python3');assert.equal(restored.code,0,restored.err);
        }
        assert.equal((await observe()).proof_observation,'verified');
      }
      const rejectedOwner=await rpc(project,null,'stash_publication_status',{publication_request_id:`clock-publication-${rate}`,owner_proof:'fixture-not-owner'});assert.equal(rejectedOwner.Err.code,'forbidden');
      const sessionId=JSON.parse(fs.readFileSync(path.join(env.PULSAR_STATE_DIR,'first-party.session.json'))).session;
      const revoked=await rpc(project,null,'revoke',{session:sessionId,project_id:project},grantCredentials);assert.ok(revoked.Ok,JSON.stringify(revoked.Err));
      const deniedGrant=await cli(observationArgs);assert.notEqual(deniedGrant.code,0);assert.equal(queries.length,queryCount);
      const granted=await rpc(project,null,'grant',{session:sessionId,project_id:project,scopes},grantCredentials);assert.ok(granted.Ok,JSON.stringify(granted.Err));assert.equal((await observe()).dispatch,'complete');

      const changedIntent=await cli(args);assert.notEqual(changedIntent.code,0);assert.match(changedIntent.err,/RequestConflict/);assert.equal(queries.length,queryCount);
      engine.kill('SIGTERM');await exit;start();await ready();assert.deepEqual((await ok([...args,'--source-time'])).value,receipt);assert.equal(queries.length,queryCount);assert.deepEqual(fs.readFileSync(receipt.path),script);assert.deepEqual(fs.readFileSync(receipt.timing_proof.path),proofBytes);
      if(process.env.PULSAR_SCHEMA3_TEST_BINARY) {
        engine.kill('SIGTERM');await exit;
        const old=await cli(['engine'],fs.realpathSync(process.env.PULSAR_SCHEMA3_TEST_BINARY));assert.notEqual(old.code,0);assert.match(old.err,/schema requires an explicit recoverable migration/);
        start();await ready();assert.deepEqual((await ok([...args,'--source-time'])).value,receipt);assert.equal(queries.length,queryCount);assert.deepEqual(fs.readFileSync(receipt.timing_proof.path),proofBytes);assert.deepEqual(fs.readFileSync(receipt.path),script);
      }
      // Historical clocks resolve from retained objects through two clones,
      // even while the original worker output directory is unavailable.
      let priorProject=project,priorCandidate=candidate;
      const reports=[];
      for(let cloneIndex=0;cloneIndex<2;cloneIndex++) {
        const allowed=await cli(['allow-packaging',priorProject,'--acknowledge-unencrypted-private-data']);assert.equal(allowed.code,0,allowed.err);
        const archive=path.join(root,`clock-${rate}-${cloneIndex}.pulsar`);
        const exported=await cli(['package-export',priorProject,archive,'--acknowledge-unencrypted-private-data']);assert.equal(exported.code,0,exported.err);
        const clone=(await ok(['package-import',archive])).receipt.project_id;
        if(cloneIndex===1){assert.equal(fs.readFileSync(archive).readUInt16LE(8),9);if(process.env.PULSAR_SCHEMA10_REBASE_TEST_BINARY){const refused=await cli(['package-import',archive],fs.realpathSync(process.env.PULSAR_SCHEMA10_REBASE_TEST_BINARY));assert.notEqual(refused.code,0);assert.match(refused.err,/unsupported.*format/i);}}
        const mapped=await cli(['-c',"import sqlite3,json,sys; d=sqlite3.connect('file:'+sys.argv[1]+'?mode=ro',uri=True); m=json.loads(d.execute('SELECT manifest FROM project_import_archives WHERE project=?',(sys.argv[2],)).fetchone()[0]); e=next(e for e in m['imported_origins'] if e['container']['project_id']==sys.argv[3]); print(next(c['local'] for c in e['candidates'] if c['origin']==sys.argv[4]))",path.join(env.PULSAR_STATE_DIR,'projects.sqlite3'),clone,priorProject,priorCandidate],'/usr/bin/python3');assert.equal(mapped.code,0,mapped.err);
        const alias=mapped.out.trim(),inspect=['project','archived-generation-status',clone,alias];
        const attempts=path.join(env.PULSAR_STATE_DIR,'attempts'),parked=path.join(env.PULSAR_STATE_DIR,'attempts-test-parked');
        fs.renameSync(attempts,parked);
        let report;
        try {report=(await ok(inspect)).value;} finally {fs.renameSync(parked,attempts);}
        assert.equal(report.archival,true);assert.equal(report.active_job,false);assert.equal(report.publication_authority,false);assert.equal(report.release_qualified,false);
        assert.equal(report.origins.length,1);const anchor=report.origins[0];assert.equal(anchor.job_id,job);assert.equal(anchor.candidate_id,candidate);assert.equal(anchor.source_version,source);
        assert.equal(anchor.clock.sample_count,8);assert.equal(anchor.settings.source_placement.placement_id,placement.placement_id);assert.deepEqual(anchor.clock.receipt,anchor.worker_receipt);
        const cloneBefore=(await ok(['project','open',clone])).value;
        const verifyMaster=async rev=>(await ok([...inspect,'--master-revision',String(rev)])).value;
        const ancestry=await verifyMaster(cloneBefore.revision);
        if(cloneIndex===1){
          const db=path.join(env.PULSAR_STATE_DIR,'projects.sqlite3');
          const corrupted=await cli(['-c',"import sqlite3,json,sys; d=sqlite3.connect(sys.argv[1]); m=json.loads(d.execute('SELECT manifest FROM project_import_archives WHERE project=?',(sys.argv[2],)).fetchone()[0]); c=next(c for c in m['candidates'] if c.get('rebased_from')); old=d.execute('SELECT base_revision FROM candidates WHERE id=?',(c['candidate_id'],)).fetchone()[0]; d.execute('UPDATE candidates SET base_revision=? WHERE id=?',(m['captured_revision']+999,c['candidate_id'])); d.commit(); print(json.dumps([c['candidate_id'],old]))",db,clone],'/usr/bin/python3');assert.equal(corrupted.code,0,corrupted.err);const [id,base]=JSON.parse(corrupted.out);
          try {const refused=await cli([...inspect,'--master-revision',String(cloneBefore.revision)]);assert.notEqual(refused.code,0);assert.match(refused.err,/DependencyMismatch/);}
          finally {const restored=await cli(['-c',"import sqlite3,sys; d=sqlite3.connect(sys.argv[1]); d.execute('UPDATE candidates SET base_revision=? WHERE id=?',(int(sys.argv[3]),sys.argv[2])); d.commit()",db,id,String(base)],'/usr/bin/python3');assert.equal(restored.code,0,restored.err);}
          assert.deepEqual(await verifyMaster(cloneBefore.revision),ancestry);
          const forged=await cli(['-c',"import sqlite3,sys; d=sqlite3.connect(sys.argv[1]); d.execute('INSERT INTO candidate_rebases(candidate,parent) VALUES(?,?)',(sys.argv[2],sys.argv[3])); d.commit()",db,id,alias],'/usr/bin/python3');assert.equal(forged.code,0,forged.err);
          try {const refused=await cli([...inspect,'--master-revision',String(cloneBefore.revision)]);assert.notEqual(refused.code,0);assert.match(refused.err,/DependencyMismatch/);}
          finally {const removed=await cli(['-c',"import sqlite3,sys; d=sqlite3.connect(sys.argv[1]); d.execute('DELETE FROM candidate_rebases WHERE candidate=?',(sys.argv[2],)); d.commit()",db,id],'/usr/bin/python3');assert.equal(removed.code,0,removed.err);}
          assert.deepEqual(await verifyMaster(cloneBefore.revision),ancestry);
        }
        assert.equal(ancestry.master_derivation.master.sha256,cloneBefore.motion.program.sha256);
        assert.equal(ancestry.master_derivation.anchor_candidate,alias);
        assert.equal(ancestry.master_derivation.publication_authority,false);
        assert.deepEqual(ancestry.master_derivation.retiming_revisions,retimeReceipt?[retimeReceipt.revision]:[]);
        assert.deepEqual((await ok(['project','open',clone])).value,cloneBefore);
        const editedFile=path.join(root,`clock-edit-${rate}-${cloneIndex}.json`);
        fs.writeFileSync(editedFile,JSON.stringify({tracks:cloneBefore.program.tracks.map(track=>({axis:track.axis,actions:track.actions.map((a,i)=>({time:a.time,position:i===(cloneIndex===0?0:track.actions.length-1)?(a.position===0.13?0.27:0.13):a.position})),gaps:track.gaps}))}));
        const edited=(await ok(['edit-values',clone,editedFile,'--revision',String(cloneBefore.revision)])).value;
        let cloneAfter=(await ok(['commit-candidate',clone,edited.candidate_id,'--revision',String(cloneBefore.revision)])).value;
        let afterAncestry=await verifyMaster(cloneAfter.revision);
        assert.equal(afterAncestry.master_derivation.master.sha256,cloneAfter.motion.program.sha256);
        assert.deepEqual(afterAncestry.master_derivation.retiming_revisions,ancestry.master_derivation.retiming_revisions);
        const stale=await cli([...inspect,'--master-revision',String(cloneBefore.revision)]);assert.notEqual(stale.code,0);assert.match(stale.err,/RevisionConflict/);
        const db=path.join(env.PULSAR_STATE_DIR,'projects.sqlite3');
        const alteredLink=await cli(['-c',"import sqlite3,sys; d=sqlite3.connect(sys.argv[1]); op=d.execute('SELECT operation FROM revision_lineage WHERE project=? AND revision=?',(sys.argv[2],int(sys.argv[3]))).fetchone()[0]; d.execute(\"UPDATE revision_lineage SET operation='edit' WHERE project=? AND revision=?\",(sys.argv[2],int(sys.argv[3]))); d.commit(); print(op)",db,clone,String(cloneBefore.revision)],'/usr/bin/python3');assert.equal(alteredLink.code,0,alteredLink.err);
        try {const changed=await cli([...inspect,'--master-revision',String(cloneAfter.revision)]);assert.notEqual(changed.code,0);assert.match(changed.err,/DependencyMismatch/);assert.deepEqual((await ok(inspect)).value,report);}
        finally {const restored=await cli(['-c',"import sqlite3,sys; d=sqlite3.connect(sys.argv[1]); d.execute('UPDATE revision_lineage SET operation=? WHERE project=? AND revision=?',(sys.argv[4],sys.argv[2],int(sys.argv[3]))); d.commit()",db,clone,String(cloneBefore.revision),alteredLink.out.trim()],'/usr/bin/python3');assert.equal(restored.code,0,restored.err);}
        assert.deepEqual(await verifyMaster(cloneAfter.revision),afterAncestry);
        if(cloneIndex===0) {
          const foreignFile=path.join(root,`clock-unrelated-${rate}.funscript`),stroke=cloneAfter.program.tracks.find(t=>t.axis==='stroke');
          fs.writeFileSync(foreignFile,JSON.stringify({actions:stroke.actions.map(a=>({at:Math.round(a.time/1e6),pos:50}))}));
          const foreign=(await ok(['import-script',foreignFile,'--project',clone])).value;
          const mixed=await rpc(clone,cloneAfter.revision,'merge_candidate',{candidate_id:foreign.candidate_id,axes:['stroke'],range:{start:stroke.actions[2].time,end:stroke.actions[5].time+1}});assert.ok(mixed.Ok,JSON.stringify(mixed.Err));
          const refusedMixed=await cli([...inspect,'--master-revision',String(mixed.Ok.value.revision)]);assert.notEqual(refusedMixed.code,0);assert.match(refusedMixed.err,/DependencyMismatch/);
          const restored=await rpc(clone,mixed.Ok.value.revision,'undo');assert.ok(restored.Ok,JSON.stringify(restored.Err));
          const restoredState=(await ok(['project','open',clone])).value;
          assert.deepEqual(restoredState.program,cloneAfter.program);cloneAfter=restoredState;afterAncestry=await verifyMaster(cloneAfter.revision);
          assert.deepEqual(afterAncestry.master_derivation.retiming_revisions,ancestry.master_derivation.retiming_revisions);
          const rebased=await rpc(clone,cloneAfter.revision,'rebase_candidate',{candidate_id:edited.candidate_id});assert.ok(rebased.Ok,JSON.stringify(rebased.Err));
          cloneAfter=(await ok(['commit-candidate',clone,rebased.Ok.value.candidate_id,'--revision',String(cloneAfter.revision)])).value;
          afterAncestry=await verifyMaster(cloneAfter.revision);assert.equal(afterAncestry.master_derivation.master.sha256,cloneAfter.motion.program.sha256);
          assert.deepEqual(afterAncestry.master_derivation.retiming_revisions,ancestry.master_derivation.retiming_revisions);
        }
        const retained=path.join(env.PULSAR_STATE_DIR,'imported-project-evidence',anchor.worker_receipt.sha256);assert.ok(fs.existsSync(retained));
        const bytes=fs.readFileSync(retained),mode=fs.statSync(retained).mode & 0o777;
        fs.chmodSync(retained,0o600);
        try {fs.writeFileSync(retained,'corrupt-clock');assert.notEqual((await cli(inspect)).code,0);}
        finally {fs.writeFileSync(retained,bytes);fs.chmodSync(retained,mode);}
        engine.kill('SIGTERM');await exit;start();await ready();assert.deepEqual((await ok(inspect)).value,report);
        assert.deepEqual(await verifyMaster(cloneAfter.revision),afterAncestry);
        assert.deepEqual((await ok(['project','open',project])).value.motion,before.value.motion);assert.equal(queries.length,queryCount);
        const reboundMedia=path.join(root,`archive-media-${rate}-${cloneIndex}`);fs.mkdirSync(reboundMedia);fs.writeFileSync(path.join(reboundMedia,'mock.mp4'),frameBytes);
        const reboundMappings=path.join(root,`archive-mappings-${rate}-${cloneIndex}.json`);fs.writeFileSync(reboundMappings,JSON.stringify([{server_root:'/stash',local_root:reboundMedia}]));
        const reference=path.join(root,`archive-origin-${rate}-${cloneIndex}.json`);fs.writeFileSync(reference,JSON.stringify(anchor.origin_manifest));
        const archiveArgs=['stash','publish-script',clone,'s1','f1','--url',url,'--source-version',source,'--candidate',alias,'--job',job,'--revision',String(cloneAfter.revision),'--archived-origin',reference,'--source-time','--request-id',`archive-clock-${rate}-${cloneIndex}`];
        const missingBinding=await cli(archiveArgs);assert.notEqual(missingBinding.code,0);assert.match(missingBinding.err,/captured source association is absent/);assert.equal(queries.length,queryCount);
        const rebound=(await ok(['stash','capture-source',clone,'s1','f1','--url',url,'--mappings',reboundMappings,'--expected-source-version',source,'--revision',String(cloneAfter.revision)])).value;
        assert.equal(rebound.source_version,source);
        const noMigration=await cli(archiveArgs.filter(a=>a!=='--source-time'));assert.notEqual(noMigration.code,0);assert.match(noMigration.err,/archival publication/);assert.equal(fs.existsSync(rebound.script_destination),false);
        const wrongRef=path.join(root,`wrong-archive-${rate}-${cloneIndex}.json`);fs.writeFileSync(wrongRef,JSON.stringify({...anchor.origin_manifest,sha256:'0'.repeat(64)}));
        const wrongNamespace=await cli(archiveArgs.map(a=>a===reference?wrongRef:a));assert.notEqual(wrongNamespace.code,0);assert.match(wrongNamespace.err,/DependencyMismatch/);assert.equal(fs.existsSync(rebound.script_destination),false);
        const badReceipt=fs.readFileSync(retained);fs.chmodSync(retained,0o600);
        try {fs.writeFileSync(retained,'damaged-origin');const bad=await cli(archiveArgs);assert.notEqual(bad.code,0);assert.equal(fs.existsSync(rebound.script_destination),false);}
        finally {fs.writeFileSync(retained,badReceipt);fs.chmodSync(retained,mode);}
        const attemptsArchive=path.join(env.PULSAR_STATE_DIR,'attempts'),parkedArchive=path.join(env.PULSAR_STATE_DIR,'attempts-archive-publish-parked');
        fs.renameSync(attemptsArchive,parkedArchive);let archival;
        try {archival=(await ok(archiveArgs)).value;} finally {fs.renameSync(parkedArchive,attemptsArchive);}
        const retainedProof=JSON.parse(fs.readFileSync(archival.timing_proof.path));
        assert.equal(archival.timing_proof.archived_context.active_job,false);assert.equal(archival.timing_proof.archived_context.archival,true);assert.equal(archival.timing_proof.archived_context.sampled_clocks_verified,true);
        assert.deepEqual(archival.timing_proof.archived_context.origin,anchor);assert.deepEqual(archival.timing_proof.archived_context.master,afterAncestry.master_derivation);
        assert.deepEqual(retainedProof.context.archived_context,archival.timing_proof.archived_context);
        assert.deepEqual(JSON.parse(fs.readFileSync(archival.path)).actions.map(a=>a.at),[0,125,250,375,500,625,750,875]);
        assert.deepEqual(retainedProof.context.archived_retimings.map(r=>r.revision),retimeReceipt?[retimeReceipt.revision]:[]);
        // Earlier CPU generation armed host ownership; cloning preserves that barrier.
        const counts=await cli(['-c',"import sqlite3,json,sys; d=sqlite3.connect('file:'+sys.argv[1]+'?mode=ro',uri=True); print(json.dumps([d.execute('PRAGMA user_version').fetchone()[0],d.execute('SELECT COUNT(*) FROM jobs WHERE project=?',(sys.argv[2],)).fetchone()[0]]))",path.join(env.PULSAR_STATE_DIR,'projects.sqlite3'),clone],'/usr/bin/python3');assert.equal(counts.code,0,counts.err);const [schema,jobs]=JSON.parse(counts.out);assert.equal(schema,20);assert.equal(jobs,0);
        queryCount=queries.length;assert.deepEqual((await ok(archiveArgs)).value,archival);assert.equal(queries.length,queryCount);
        const protectedScript=fs.readFileSync(archival.path);const existing=await cli(archiveArgs.map(a=>a===`archive-clock-${rate}-${cloneIndex}`?`archive-existing-${rate}-${cloneIndex}`:a));assert.notEqual(existing.code,0);assert.deepEqual(fs.readFileSync(archival.path),protectedScript);queryCount=queries.length;
        const publicationStatus=['stash','publication-status',clone,'--publication-request-id',`archive-clock-${rate}-${cloneIndex}`];
        assert.equal((await ok(publicationStatus)).value.proof_observation,'verified');assert.equal(queries.length,queryCount);
      for(const mutation of ['mapping','namespace','carrier','selection','downgrade']) {
        const backup=path.join(root,`archive-proof-reference-${rate}-${cloneIndex}-${mutation}.json`);
        const forge=await cli(['-c',`import sqlite3,json,sys,hashlib,pathlib
D=sqlite3.connect(sys.argv[1]);project,request,mutation,backup=sys.argv[2:]
session,raw,response=D.execute('SELECT e.session,p.proof,r.response FROM exports e JOIN stash_clock_proofs p USING(session,request) JOIN requests r USING(session,request) WHERE e.project=? AND e.request=?',(project,request)).fetchone()
meta=json.loads(raw);proof=json.loads(pathlib.Path(meta['path']).read_bytes())
if mutation=='namespace': proof['context']['action_ancestry']['carriers'][0]['origin_manifest']['sha256']='0'*64
elif mutation=='carrier': proof['context']['action_ancestry']['carriers'][0]['original_receipt']['sha256']='0'*64
elif mutation=='selection': proof['context']['action_ancestry']['selections'][0]['carrier']=2048
elif mutation=='mapping': proof['mappings'][0]['original_source_frame']=777
else: proof['schema_version']=3
payload=json.dumps(proof,separators=(',',':')).encode();hash=hashlib.sha256(payload).hexdigest();file=pathlib.Path(meta['path']).parent/(hash+'.json');file.write_bytes(payload)
meta['identity']={'artifact_id':hash,'sha256':hash,'byte_len':len(payload)};meta['path']=str(file)
reply=json.loads(response)
def replace(value):
 if isinstance(value,dict):
  for key,item in list(value.items()):
   if key=='timing_proof': value[key]=meta
   else: replace(item)
 elif isinstance(value,list):
  for item in value: replace(item)
replace(reply)
pathlib.Path(backup).write_text(json.dumps({'session':session,'request':request,'proof':raw,'response':response,'forged_file':str(file)}))
D.execute('UPDATE stash_clock_proofs SET proof=? WHERE session=? AND request=?',(json.dumps(meta),session,request));D.execute('UPDATE requests SET response=? WHERE session=? AND request=?',(json.dumps(reply),session,request));D.commit()
`,path.join(env.PULSAR_STATE_DIR,'projects.sqlite3'),clone,`archive-clock-${rate}-${cloneIndex}`,mutation,backup],'/usr/bin/python3');
        assert.equal(forge.code,0,forge.err);
        try {const checked=(await ok(publicationStatus)).value;assert.equal(checked.proof_observation,'unverified',mutation);fs.writeFileSync(path.join(root,`archive-proof-refused-${rate}-${cloneIndex}-${mutation}.json`),JSON.stringify(checked,null,2));}
        finally {
          const restored=await cli(['-c',`import sqlite3,json,sys,pathlib
D=sqlite3.connect(sys.argv[1]);b=json.loads(pathlib.Path(sys.argv[2]).read_text());D.execute('UPDATE stash_clock_proofs SET proof=? WHERE session=? AND request=?',(b['proof'],b['session'],b['request']));D.execute('UPDATE requests SET response=? WHERE session=? AND request=?',(b['response'],b['session'],b['request']));D.commit()
`,path.join(env.PULSAR_STATE_DIR,'projects.sqlite3'),backup],'/usr/bin/python3');assert.equal(restored.code,0,restored.err);
        }
        assert.equal(((await ok(publicationStatus)).value).proof_observation,'verified');
      }
        assert.equal(retainedProof.schema_version,5);
        assert.equal(retainedProof.context.action_ancestry.selections.length,retainedProof.mappings.length);
        assert.ok(retainedProof.context.action_ancestry.carriers.length>0);
        for(const carrier of retainedProof.context.action_ancestry.carriers)assert.deepEqual(carrier.origin_manifest,anchor.origin_manifest);

        engine.kill('SIGTERM');await exit;
        const databaseBytes=()=>[db,`${db}-wal`].map(file=>fs.existsSync(file)?crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex'):null),beforeOld=databaseBytes();
        if(process.env.PULSAR_SCHEMA9_ARCHIVE_TEST_BINARY){const old=await cli(['engine'],fs.realpathSync(process.env.PULSAR_SCHEMA9_ARCHIVE_TEST_BINARY));assert.notEqual(old.code,0);assert.match(old.err,/schema requires an explicit recoverable migration/);}
        if(schema>=11&&process.env.PULSAR_SCHEMA10_REBASE_TEST_BINARY){const old=await cli(['engine'],fs.realpathSync(process.env.PULSAR_SCHEMA10_REBASE_TEST_BINARY));assert.notEqual(old.code,0);assert.match(old.err,/schema requires an explicit recoverable migration/);}
        assert.deepEqual(databaseBytes(),beforeOld);
        start();await ready();assert.deepEqual((await ok(archiveArgs)).value,archival);assert.equal((await ok(publicationStatus)).value.proof_observation,'verified');assert.equal(queries.length,queryCount);
        assert.deepEqual((await ok(['project','open',clone])).value,cloneAfter);
        reports.push(report);priorProject=clone;priorCandidate=alias;
      }
      assert.deepEqual(reports[0].origins,reports[1].origins);
      // Synthetic unresolved-dispatch state; this is not a lost-acknowledgement reproduction.
      engine.kill('SIGTERM');await exit;
      const altered=await cli(['-c',`import sqlite3,sys
d=sqlite3.connect(sys.argv[1]);c=d.execute("UPDATE exports SET state='failed_or_unknown' WHERE project=? AND request=?",(sys.argv[2],sys.argv[3]));assert c.rowcount==1;d.execute("DELETE FROM requests WHERE request=? AND session IN (SELECT session FROM exports WHERE project=? AND request=?)",(sys.argv[3],sys.argv[2],sys.argv[3]));d.commit()` ,path.join(env.PULSAR_STATE_DIR,'projects.sqlite3'),project,`clock-publication-${rate}`],'/usr/bin/python3');assert.equal(altered.code,0,altered.err);
      start();await ready();observation=await observe();assert.equal(observation.dispatch,'failed_or_unknown');assert.equal(observation.proof_observation,'verified');assert.equal(observation.source_and_script_observation,'verified');assert.equal(observation.completed_receipt,null);
      const noResend=await cli([...args,'--source-time']);assert.notEqual(noResend.code,0);assert.match(noResend.err,/UnknownOutcome/);assert.equal(queries.length,queryCount);
      engine.kill('SIGTERM');await exit;start();await ready();observation=await observe();assert.equal(observation.dispatch,'failed_or_unknown');assert.equal(observation.completed_receipt,null);assert.equal(observation.proof_observation,'verified');assert.deepEqual(fs.readFileSync(receipt.path),script);
      }
      // Unplaced archival origins use the original elapsed convention without claiming sampled clocks.
      const unplaced=(await ok(['project','create','unplaced archival publication'])).value.project_id;
      const unplacedMedia=path.join(root,'unplaced-native-media');fs.mkdirSync(unplacedMedia);fs.writeFileSync(path.join(unplacedMedia,'mock.mp4'),frameBytes);
      const mappings=path.join(root,'unplaced-mappings.json');fs.writeFileSync(mappings,JSON.stringify([{server_root:'/stash',local_root:unplacedMedia}]));
      const source=(await ok(['stash','capture-source',unplaced,'s1','f1','--url',url,'--mappings',mappings])).value.source_version;
      const base=(await ok(['project','open',unplaced])).value.revision;
      const generated=await rpc(unplaced,base,'generate',{source_version:source,preset:'fast',model_path:null,settings:{analysis_width:64,analysis_height:64,source_sample_stride:1,axis:'stroke',vr_mode:false,model_input:null}});assert.ok(generated.Ok,JSON.stringify(generated.Err));
      const job=generated.Ok.value.job_id;let status;const deadline=Date.now()+30000;
      while(true){const response=await rpc(unplaced,null,'job_status',{job_id:job});assert.ok(response.Ok,JSON.stringify(response.Err));status=response.Ok.value;if(status.state==='completed')break;assert.ok(['queued','running'].includes(status.state),JSON.stringify(status));assert.ok(Date.now()<deadline,'unplaced generation deadline');await delay(50);}
      assert.ok((await rpc(unplaced,base,'commit_candidate',{candidate_id:status.candidate_id})).Ok);
      const allowed=await cli(['allow-packaging',unplaced,'--acknowledge-unencrypted-private-data']);assert.equal(allowed.code,0,allowed.err);const archive=path.join(root,'unplaced.pulsar');const exported=await cli(['package-export',unplaced,archive,'--acknowledge-unencrypted-private-data']);assert.equal(exported.code,0,exported.err);
      const clone=(await ok(['package-import',archive])).receipt.project_id;
      const aliasResult=await cli(['-c',"import sqlite3,json,sys; d=sqlite3.connect('file:'+sys.argv[1]+'?mode=ro',uri=True); m=json.loads(d.execute('SELECT manifest FROM project_import_archives WHERE project=?',(sys.argv[2],)).fetchone()[0]); e=next(e for e in m['imported_origins'] if e['container']['project_id']==sys.argv[3]); print(next(c['local'] for c in e['candidates'] if c['origin']==sys.argv[4]))",path.join(env.PULSAR_STATE_DIR,'projects.sqlite3'),clone,unplaced,status.candidate_id],'/usr/bin/python3');assert.equal(aliasResult.code,0,aliasResult.err);
      const alias=aliasResult.out.trim(),state=(await ok(['project','open',clone])).value;
      const report=(await ok(['project','archived-generation-status',clone,alias,'--master-revision',String(state.revision)])).value;assert.equal(report.origins[0].clock,null);
      const reboundMedia=path.join(root,'unplaced-archive-media');fs.mkdirSync(reboundMedia);fs.writeFileSync(path.join(reboundMedia,'mock.mp4'),frameBytes);
      const reboundMappings=path.join(root,'unplaced-rebound.json');fs.writeFileSync(reboundMappings,JSON.stringify([{server_root:'/stash',local_root:reboundMedia}]));
      await ok(['stash','capture-source',clone,'s1','f1','--url',url,'--mappings',reboundMappings,'--expected-source-version',source,'--revision',String(state.revision)]);
      const reference=path.join(root,'unplaced-origin.json');fs.writeFileSync(reference,JSON.stringify(report.origins[0].origin_manifest));
      const args=['stash','publish-script',clone,'s1','f1','--url',url,'--source-version',source,'--candidate',alias,'--job',job,'--revision',String(state.revision),'--archived-origin',reference,'--request-id','unplaced-archive'];
      const wrongClock=await cli([...args,'--source-time']);assert.notEqual(wrongClock.code,0);assert.equal(fs.existsSync(path.join(reboundMedia,'mock.funscript')),false);
      const receipt=(await ok(args)).value,proof=JSON.parse(fs.readFileSync(receipt.timing_proof.path));assert.equal(proof.schema_version,3);assert.deepEqual(proof.mappings,[]);assert.equal(receipt.timing_proof.reused_sample_clocks,0);assert.equal(receipt.timing_proof.archived_context.sampled_clocks_verified,false);assert.equal(receipt.timing_proof.archived_context.source_time,false);
      const queryCount=queries.length;assert.deepEqual((await ok(args)).value,receipt);assert.equal(queries.length,queryCount);
      const observation=['stash','publication-status',clone,'--publication-request-id','unplaced-archive'];assert.equal((await ok(observation)).value.proof_observation,'verified');assert.equal(queries.length,queryCount);
      engine.kill('SIGTERM');await exit;start();await ready();assert.deepEqual((await ok(args)).value,receipt);assert.equal((await ok(observation)).value.proof_observation,'verified');assert.equal(queries.length,queryCount);
    } finally {if(engine?.exitCode===null&&engine.signalCode===null){engine.kill('SIGTERM');await exit;}fs.writeFileSync(path.join(root,'engine-stderr.json'),JSON.stringify({stderr,exitCode:engine?.exitCode,signalCode:engine?.signalCode},null,2));await new Promise(resolve=>server.close(resolve));}
  });
