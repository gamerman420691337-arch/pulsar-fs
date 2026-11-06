import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import http from 'node:http';
import net from 'node:net';
import {spawn} from 'node:child_process';
import {setTimeout as delay} from 'node:timers/promises';

test('compiled CLI reaches the engine Stash broker and keeps owner/credential policy',
  {skip:!process.env.PULSAR_STASH_TEST_BINARY,timeout:60000},async t=> {
    const root=fs.mkdtempSync(path.join(os.tmpdir(),'pulsar-stash-cli-'));fs.chmodSync(root,0o700);
    t.diagnostic(`retained private Stash process fixture: ${root}`);
    const requests=[];let interactive=false,jobStatus="RUNNING",scanAcknowledgement="remote-job",fileId="f1",autoAttach=false;
    const tags=new Map([['old','Existing'],['authored','Authored'],['pending','Pending']]);
    const sceneTags=new Set(['old']);let createApplies=true,createAckLost=false,addApplies=true,addAckLost=false;
    let creationRelease,creationArrivedResolve;const creationArrived=new Promise(resolve=>{creationArrivedResolve=resolve;});
    let holdScan=false,scanRelease,scanArrivedResolve;const scanArrived=new Promise(resolve=>{scanArrivedResolve=resolve;});
    const server=http.createServer(async(req,res)=> {
      const chunks=[];for await(const chunk of req) chunks.push(chunk);
      const body=JSON.parse(Buffer.concat(chunks).toString('utf8'));requests.push(body);
      res.writeHead(200,{'Content-Type':'application/json',Connection:'close'});
      const scene={id:'s1',title:'Mock scene',interactive,files:[{id:fileId,path:'/stash/mock.mp4'}],tags:[...sceneTags].map(id=>({id}))};
      if(body.query.includes("metadataScan") && autoAttach) interactive=true;
      let data;
      if(body.query.includes('findTags')) {
        const selected=[...tags].filter(([,name])=>name===body.variables.name).map(([id,name])=>({id,name}));
        data={findTags:{count:selected.length,tags:selected}};
      } else if(body.query.includes('tagCreate')) {
        const name=body.variables.input.name,id=`created-${tags.size}`;
        if(createApplies) tags.set(id,name);
        if(name==='Concurrent Creation') {creationArrivedResolve();await new Promise(resolve=>{creationRelease=resolve;});}
        data={tagCreate:createAckLost?null:{id,name}};
      } else if(body.query.includes('bulkSceneUpdate')) {
        assert.equal(body.variables.input.tag_ids.mode,'ADD');assert.deepEqual(body.variables.input.ids,['s1']);
        if(addApplies) for(const id of body.variables.input.tag_ids.ids) sceneTags.add(id);
        data={bulkSceneUpdate:addAckLost?null:[{id:'s1'}]};
      } else data=body.query.includes('metadataScan')?{metadataScan:scanAcknowledgement}:body.query.includes('findJob')?{findJob:jobStatus===null?null:{id:'remote-job',status:jobStatus}}:body.query.includes('findScene(id:')?{findScene:scene}:{findScenes:{count:1,scenes:[scene]}};
      if(body.query.includes("metadataScan") && holdScan) {scanArrivedResolve();await new Promise(resolve=>{scanRelease=resolve;});}
      res.end(JSON.stringify({data}));
    });
    await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
    const binary=fs.realpathSync(process.env.PULSAR_STASH_TEST_BINARY);
    const env={PATH:'/usr/bin:/bin',PULSAR_STATE_DIR:path.join(root,'engine')};
    let engine=spawn(binary,['engine'],{env,stdio:['ignore','ignore','pipe']});
    let engineError='';engine.stderr.on('data',data=>{engineError=(engineError+data.toString()).slice(0,65536);});
    let engineExit=new Promise(resolve=>engine.once('exit',resolve));
    const cli=args=>new Promise((resolve,reject)=> {
      const child=spawn(binary,args,{env,stdio:['ignore','pipe','pipe']});let out='',err='';
      child.stdout.on('data',data=>{out=(out+data.toString()).slice(0,65536);});
      child.stderr.on('data',data=>{err=(err+data.toString()).slice(0,65536);});
      child.once('error',reject);child.once('exit',code=>resolve({code,out,err}));
    });
    try {
      const deadline=Date.now()+30000;
      while (!fs.existsSync(path.join(env.PULSAR_STATE_DIR,'engine.sock'))) {
        assert.equal(engine.exitCode,null,engineError);
        assert.ok(Date.now()<deadline,'private engine startup deadline');await delay(25);
      }
      const url=`http://127.0.0.1:${server.address().port}/graphql`;
      const result=await cli(['stash','query','--url',url,'--limit','2']);
      assert.equal(result.code,0,result.err);
      const response=JSON.parse(result.out);
      assert.equal(response.kind,'stash_discovery');
      assert.equal(response.value.scenes[0].id,'s1');
      assert.equal(response.value.pagination_complete,true);
      assert.equal(requests.length,1);
      assert.match(requests[0].query,/interactive: false/);
      const rejected=await cli(['stash','query','--url',url,'--api-key','authored-fake-fixture-key']);
      assert.notEqual(rejected.code,0);assert.match(rejected.err,/api-key-env/);
      assert.equal(requests.length,1);
      fs.chmodSync(path.join(env.PULSAR_STATE_DIR,'stash-owner.token'),0o644);
      const untrusted=await cli(['stash','query','--url',url]);
      assert.notEqual(untrusted.code,0);assert.match(untrusted.err,/0600/);
      assert.equal(requests.length,1);
      fs.chmodSync(path.join(env.PULSAR_STATE_DIR,'stash-owner.token'),0o600);
      const media=path.join(root,'media');fs.mkdirSync(media,{mode:0o700});
      fs.writeFileSync(path.join(media,'mock.mp4'),'authored media fixture');
      const mappings=path.join(root,'mappings.json');
      fs.writeFileSync(mappings,JSON.stringify([{server_root:'/stash',local_root:media}]),{mode:0o600});
      const planArgs=['stash','plan-missing','--url',url,'--mappings',mappings];
      const planned=await cli(planArgs);assert.equal(planned.code,0,planned.err);
      const plan=JSON.parse(planned.out);assert.equal(plan.kind,'stash_plan');
      assert.equal(plan.value.planning_only,true);assert.equal(plan.value.files[0].status,'ready_for_snapshot');
      assert.equal(plan.value.files[0].script_destination,path.join(media,'mock.funscript'));

      const created=await cli(['project','create','Stash source fixture']);assert.equal(created.code,0,created.err);
      const project=JSON.parse(created.out).value.project_id;
      const captureArgs=['stash','capture-source',project,'s1','f1','--url',url,'--mappings',mappings];
      const beforeStale=requests.length;
      const stale=await cli([...captureArgs,'--revision','1']);assert.notEqual(stale.code,0);
      assert.equal(requests.length,beforeStale,'stale capture must fail before the server request');
      const captured=await cli(captureArgs);assert.equal(captured.code,0,captured.err);
      const receipt=JSON.parse(captured.out);assert.equal(receipt.kind,'stash_source_captured');
      assert.equal(receipt.value.scene_id,'s1');assert.equal(receipt.value.file_id,'f1');
      const snapshot=path.join(env.PULSAR_STATE_DIR,'snapshots',receipt.value.identity.sha256);
      fs.writeFileSync(path.join(media,'mock.mp4'),'new mutable contents');
      assert.equal(fs.readFileSync(snapshot,'utf8'),'authored media fixture');
      fs.writeFileSync(path.join(media,'mock.funscript'),'existing script must survive');
      const existing=await cli(planArgs);assert.equal(existing.code,0,existing.err);
      assert.equal(JSON.parse(existing.out).value.files[0].status,'existing_script');
      assert.equal(fs.readFileSync(path.join(media,'mock.funscript'),'utf8'),'existing script must survive');
      fs.unlinkSync(path.join(media,'mock.funscript'));
      const batchArgs=['stash','generate-missing','--url',url,'--mappings',mappings,'--width','64','--height','64'];
      fs.writeFileSync(path.join(media,'mock.mp4'),Buffer.concat([Buffer.from('YUV4MPEG2 W64 H64 F8:1 Ip A1:1 Cmono\n'),...Array.from({length:8},()=>Buffer.concat([Buffer.from('FRAME\n'),Buffer.alloc(64*64,128)]))]));
      const gapped=await cli(batchArgs);assert.notEqual(gapped.code,0);
      const refused=JSON.parse(gapped.out).items[0];
      assert.equal(refused.state,'failed_or_unknown');assert.match(refused.error,/unresolved evidence gaps/);
      assert.ok(refused.project_id && refused.job_id && refused.candidate_id);
      assert.equal(fs.existsSync(path.join(media,'mock.funscript')),false);
      const frames=Buffer.concat([Buffer.from('YUV4MPEG2 W64 H64 F8:1 Ip A1:1 Cmono\n'),...Array.from({length:8},(_,frame)=>Buffer.concat([Buffer.from('FRAME\n'),Buffer.from(Array.from({length:64*64},(_,i)=>{const x=i%64,y=Math.floor(i/64),sy=(y+64-frame)%64;return (x*31+sy*17+Math.floor(x/7)*Math.floor(sy/5)*13)%256;}))]))]);
      fs.writeFileSync(path.join(media,'mock.mp4'),frames);
      const generated=await cli(batchArgs);assert.equal(generated.code,0,generated.err+generated.out);
      const batch=JSON.parse(generated.out);assert.equal(batch.release_qualified,false);
      assert.equal(batch.stash_mutations_requested,false);
      assert.equal(batch.items.length,1);assert.equal(batch.items[0].state,'published_unqualified');
      const item=batch.items[0],exported=item.export;
      assert.equal(exported.release_qualified,false);assert.equal(exported.job_id,item.job_id);
      assert.equal(exported.candidate_id,item.candidate_id);assert.equal(exported.source.source_version,item.source_version);
      assert.ok(exported.unresolved_review_count>0,'global-motion uncertainty remains visible');
      const published=fs.readFileSync(path.join(media,'mock.funscript'));
      assert.ok(JSON.parse(published).actions.length>0);
      const publishId=item.requests.find(request=>request.phase==='publish').request_id;
      const beforeReplay=requests.length;
      const replay=await cli(['stash','publish-script',item.project_id,'s1','f1','--url',url,'--source-version',item.source_version,'--candidate',item.candidate_id,'--job',item.job_id,'--revision',String(exported.revision),'--request-id',publishId]);
      assert.notEqual(replay.code,0);assert.match(replay.err,/RequestConflict/);
      const batchReplay=await cli([...batchArgs,'--request-id',batch.batch_request_id,'--detach']);
      assert.equal(batchReplay.code,0,batchReplay.err);
      assert.equal(requests.length,beforeReplay,'publication replay must not dispatch another Stash query');
      const repeated=await cli(batchArgs);assert.equal(repeated.code,0,repeated.err);
      assert.equal(JSON.parse(repeated.out).items[0].eligibility,'existing_script');
      assert.deepEqual(fs.readFileSync(path.join(media,'mock.funscript')),published);
      assert.ok(requests.every(request=>request.query.trim().startsWith('query')),'generation has no remote scan/tag mutations');

      const scanId='scan-authored-1';
      const scanArgs=['stash','submit-scan',item.project_id,'--publication-request-id',publishId,'--url',url,'--revision',String(exported.revision),'--request-id',scanId];
      const submitted=await cli(scanArgs);assert.equal(submitted.code,0,submitted.err+submitted.out);
      assert.equal(JSON.parse(submitted.out).value.state,'submitted');
      const mutation=requests.find(request=>request.query.includes('metadataScan'));
      assert.deepEqual(mutation.variables.input.paths,['/stash/mock.mp4']);assert.equal(mutation.variables.input.rescan,true);
      const beforeScanReplay=requests.length;
      assert.equal((await cli(scanArgs)).code,0);assert.equal(requests.length,beforeScanReplay);
      const duplicate=await cli([...scanArgs.slice(0,-1),'scan-authored-other']);assert.notEqual(duplicate.code,0);
      assert.equal(requests.length,beforeScanReplay,'another request cannot resubmit the same publication');
      const statusArgs=['stash','scan-status',item.project_id,'--scan-request-id',scanId,'--url',url];
      const status=async()=>{const result=await cli(statusArgs);assert.equal(result.code,0,result.err);return JSON.parse(result.out).value;};
      assert.equal((await status()).state,'running');
      const beforePrematureTag=requests.length;
      const prematureTag=await cli(['stash','add-tag',item.project_id,'--scan-request-id',scanId,'--tag','Premature','--create-tag','--url',url,'--revision',String(exported.revision),'--request-id','tag-premature']);
      assert.notEqual(prematureTag.code,0);assert.match(prematureTag.err,/verified scan attachment/);
      assert.equal(requests.length,beforePrematureTag,'unverified scans cannot dispatch tag lookup, creation or ADD');
      // A fresh engine must reconcile the previously recorded remote job.
      engine.kill('SIGTERM');await engineExit;
      engine=spawn(binary,['engine'],{env,stdio:['ignore','ignore','pipe']});
      engine.stderr.on('data',data=>{engineError=(engineError+data.toString()).slice(0,65536);});
      engineExit=new Promise(resolve=>engine.once('exit',resolve));
      const restartDeadline=Date.now()+10000;
      while(!fs.existsSync(path.join(env.PULSAR_STATE_DIR,'engine.sock'))) {assert.equal(engine.exitCode,null,engineError);assert.ok(Date.now()<restartDeadline);await delay(25);}
      assert.equal((await status()).remote_job_id,'remote-job');
      for (const [remote,state] of [['READY','ready'],['STOPPING','stopping'],['CANCELLED','cancelled'],['FAILED','failed'],[null,'job_missing']]) {
        jobStatus=remote;assert.equal((await status()).state,state);
      }
      jobStatus='FINISHED';assert.equal((await status()).state,'finished_unattached');
      interactive=true;fileId='changed';assert.equal((await status()).state,'finished_unattached');
      fileId='f1';assert.equal((await status()).state,'verified_linked');
      fileId='changed';const invalidated=await cli(['stash','reconcile-attachment',item.project_id,'--scan-request-id',scanId,'--url',url]);assert.equal(invalidated.code,0,invalidated.err);
      assert.equal(JSON.parse(invalidated.out).value.attachment.state,'identity_changed');assert.equal(JSON.parse(invalidated.out).value.state,'finished_unattached');
      fileId='f1';const verifiedAgain=await status();assert.equal(verifiedAgain.state,'verified_linked');assert.equal(verifiedAgain.attachment.state,'observed_linked');
      const tagArgs=(name,id)=>['stash','add-tag',item.project_id,'--scan-request-id',scanId,'--tag',name,'--url',url,'--revision',String(exported.revision),'--request-id',id];
      const oldTag=await cli(tagArgs('Existing','tag-existing'));assert.equal(oldTag.code,0,oldTag.err);
      assert.equal(JSON.parse(oldTag.out).value.state,'verified_added');assert.equal(JSON.parse(oldTag.out).value.add_dispatched,false);
      const added=await cli(tagArgs('Authored','tag-add-authored'));assert.equal(added.code,0,added.err+added.out);
      assert.equal(JSON.parse(added.out).value.state,'verified_added');assert.equal(JSON.parse(added.out).value.add_dispatched,true);
      assert.ok(sceneTags.has('old') && sceneTags.has('authored'),'ADD preserves the preexisting tag');
      const beforeTagReplay=requests.length;
      assert.equal((await cli(tagArgs('Authored','tag-add-authored'))).code,0);assert.equal(requests.length,beforeTagReplay);
      assert.notEqual((await cli(tagArgs('Authored','tag-other-request'))).code,0);assert.equal(requests.length,beforeTagReplay);
      assert.notEqual((await cli(tagArgs('Absent no create','tag-no-create'))).code,0);
      assert.equal([...tags.values()].includes('Absent no create'),false);
      createAckLost=true;addAckLost=true;
      const createdTag=await cli([...tagArgs('New Authored','tag-create-authored'),'--create-tag']);assert.equal(createdTag.code,0,createdTag.err+createdTag.out);
      assert.equal(JSON.parse(createdTag.out).value.state,'verified_added');assert.ok(JSON.parse(createdTag.out).value.tag_id);
      assert.ok(sceneTags.has('old'));
      addApplies=false;
      const pendingTag=await cli(tagArgs('Pending','tag-pending'));assert.notEqual(pendingTag.code,0);
      assert.equal(JSON.parse(pendingTag.out).value.state,'attach_unknown');
      const pendingStatusArgs=['stash','tag-status',item.project_id,'--tag-request-id','tag-pending','--url',url];
      const pendingStatus=await cli(pendingStatusArgs);assert.equal(pendingStatus.code,0,pendingStatus.err);assert.equal(JSON.parse(pendingStatus.out).value.state,'attach_unknown');
      const beforeResumeUnknown=requests.length;
      const resumeUnknown=await cli(['stash','resume-tag',item.project_id,'--tag-request-id','tag-pending','--url',url,'--revision',String(exported.revision)]);
      assert.notEqual(resumeUnknown.code,0);assert.equal(requests.length,beforeResumeUnknown);
      // A later observation can resolve ADD delivery without resubmitting it.
      sceneTags.add('pending');assert.equal(JSON.parse((await cli(pendingStatusArgs)).out).value.state,'verified_added');
      const existingTagStatus=['stash','tag-status',item.project_id,'--tag-request-id','tag-add-authored','--url',url];
      sceneTags.delete('authored');assert.equal(JSON.parse((await cli(existingTagStatus)).out).value.state,'attached_unverified');
      tags.delete('authored');assert.equal(JSON.parse((await cli(existingTagStatus)).out).value.state,'tag_missing');
      tags.set('authored','Authored');sceneTags.add('authored');assert.equal(JSON.parse((await cli(existingTagStatus)).out).value.state,'verified_added');
      createApplies=false;
      const unknownCreation=await cli([...tagArgs('Unknown Creation','tag-create-unknown'),'--create-tag']);assert.notEqual(unknownCreation.code,0);
      assert.equal(JSON.parse(unknownCreation.out).value.state,'creation_unknown');
      const unknownCreationStatus=['stash','tag-status',item.project_id,'--tag-request-id','tag-create-unknown','--url',url];
      const creationCount=requests.filter(r=>r.query.includes('tagCreate')).length;
      assert.equal(JSON.parse((await cli(unknownCreationStatus)).out).value.state,'creation_unknown');
      assert.equal(requests.filter(r=>r.query.includes('tagCreate')).length,creationCount);
      // Resolve delayed creation read-only, then explicitly dispatch ADD once.
      tags.set('late-created','Unknown Creation');
      assert.equal(JSON.parse((await cli(unknownCreationStatus)).out).value.state,'resolved');
      addApplies=true;addAckLost=false;
      const resumed=await cli(['stash','resume-tag',item.project_id,'--tag-request-id','tag-create-unknown','--url',url,'--revision',String(exported.revision),'--request-id','resume-resolved']);
      assert.equal(resumed.code,0,resumed.err+resumed.out);assert.equal(JSON.parse(resumed.out).value.state,'verified_added');
      assert.equal(requests.filter(r=>r.query.includes('tagCreate')).length,creationCount);
      assert.ok(sceneTags.has('old'));

      createApplies=true;createAckLost=true;
      const concurrentArgs=[...tagArgs('Concurrent Creation','tag-concurrent-creation'),'--create-tag'];
      const originalCreation=cli(concurrentArgs);
      await Promise.race([creationArrived,delay(10000).then(()=>{throw new Error('creation dispatch deadline');})]);
      try {
        const concurrentStatus=['stash','tag-status',item.project_id,'--tag-request-id','tag-concurrent-creation','--url',url];
        assert.equal(JSON.parse((await cli(concurrentStatus)).out).value.state,'resolved');
        const addsBefore=requests.filter(r=>r.query.includes('bulkSceneUpdate')).length;
        const concurrentResume=await cli(['stash','resume-tag',item.project_id,'--tag-request-id','tag-concurrent-creation','--url',url,'--revision',String(exported.revision),'--request-id','resume-concurrent']);
        assert.equal(concurrentResume.code,0,concurrentResume.err+concurrentResume.out);
        creationRelease();creationRelease=null;
        const originalOutcome=await originalCreation;assert.notEqual(originalOutcome.code,0,'older creation cannot replace a concurrent ADD dispatch');
        assert.equal(requests.filter(r=>r.query.includes('bulkSceneUpdate')).length,addsBefore+1);
        const reconciled=JSON.parse((await cli(concurrentStatus)).out).value;
        assert.equal(reconciled.state,'verified_added');assert.equal(reconciled.add_dispatched,true);
      } finally {if(creationRelease) creationRelease();await originalCreation;}

      fs.writeFileSync(path.join(media,'mock.funscript'),'changed');
      const changed=await cli(statusArgs);assert.notEqual(changed.code,0);assert.match(changed.err,/bytes or namespace changed/);
      fs.writeFileSync(path.join(media,'mock.funscript'),published);
      assert.equal((await status()).state,'verified_linked');
      // Lost/invalid mutation acknowledgements remain durable unknown outcomes.
      interactive=false;fs.unlinkSync(path.join(media,'mock.funscript'));
      const next=await cli(batchArgs);assert.equal(next.code,0,next.err);
      const nextItem=JSON.parse(next.out).items[0];const nextPublish=nextItem.requests.find(effect=>effect.phase==='publish').request_id;
      scanAcknowledgement=null;const unknownId='scan-unknown-authored';
      const unknownArgs=['stash','submit-scan',nextItem.project_id,'--publication-request-id',nextPublish,'--url',url,'--revision',String(nextItem.export.revision),'--request-id',unknownId];
      const unknown=await cli(unknownArgs);assert.notEqual(unknown.code,0);assert.equal(JSON.parse(unknown.out).value.state,'dispatch_unknown');
      const unknownCount=requests.length;
      const unknownReplay=await cli(unknownArgs);assert.notEqual(unknownReplay.code,0);assert.equal(requests.length,unknownCount);
      const unknownStatus=await cli(['stash','scan-status',nextItem.project_id,'--scan-request-id',unknownId,'--url',url]);
      assert.equal(JSON.parse(unknownStatus.out).value.state,'dispatch_unknown');assert.equal(requests.length,unknownCount);
      assert.notEqual((await cli([...unknownArgs.slice(0,-1),'scan-unknown-other'])).code,0);assert.equal(requests.length,unknownCount);

      const attachmentArgs=['stash','reconcile-attachment',nextItem.project_id,'--scan-request-id',unknownId,'--url',url];
      const observe=async()=>{const result=await cli(attachmentArgs);assert.equal(result.code,0,result.err);return JSON.parse(result.out).value;};
      const mutationCount=()=>requests.filter(request=>request.query.includes('metadataScan')).length;
      const mutationsBeforeObservation=mutationCount();
      assert.equal((await observe()).attachment.state,'unattached');
      interactive=true;const observed=await observe();assert.equal(observed.attachment.state,'observed_linked');
      assert.equal(observed.state,'dispatch_unknown');assert.equal(observed.remote_job_id,null);assert.equal(observed.attachment.remote_script_hash_verified,false);assert.ok(observed.attachment.observed_at_unix_ms>0);
      fileId='changed';assert.equal((await observe()).attachment.state,'identity_changed');fileId='f1';
      const unknownPublished=fs.readFileSync(path.join(media,'mock.funscript'));fs.writeFileSync(path.join(media,'mock.funscript'),'tampered');
      assert.equal((await observe()).attachment.state,'local_publication_unverified');fs.writeFileSync(path.join(media,'mock.funscript'),unknownPublished);
      assert.equal((await observe()).attachment.state,'observed_linked');assert.equal(mutationCount(),mutationsBeforeObservation);
      const beforeUnknownTag=requests.length;
      const unknownTag=await cli(['stash','add-tag',nextItem.project_id,'--scan-request-id',unknownId,'--tag','Unknown Job','--create-tag','--url',url,'--revision',String(nextItem.export.revision)]);
      assert.notEqual(unknownTag.code,0);assert.equal(requests.length,beforeUnknownTag,'attachment observation cannot authorize tagging of an unknown scan');
      engine.kill('SIGTERM');await engineExit;engine=spawn(binary,['engine'],{env,stdio:['ignore','ignore','pipe']});
      engine.stderr.on('data',data=>{engineError=(engineError+data.toString()).slice(0,65536);});engineExit=new Promise(resolve=>engine.once('exit',resolve));
      const observationRestartDeadline=Date.now()+10000;
      while(true) {assert.equal(engine.exitCode,null,engineError);assert.ok(Date.now()<observationRestartDeadline);if(fs.existsSync(path.join(env.PULSAR_STATE_DIR,'engine.sock')) && (await cli(['capabilities'])).code===0)break;await delay(25);}
      const retained=await cli(['stash','scan-status',nextItem.project_id,'--scan-request-id',unknownId,'--url',url]);assert.equal(retained.code,0,retained.err);
      const retainedReceipt=JSON.parse(retained.out).value;assert.equal(retainedReceipt.state,'dispatch_unknown');assert.equal(retainedReceipt.attachment.state,'observed_linked');assert.equal(retainedReceipt.remote_job_id,null);
      const duplicateAfterObservation=requests.length;assert.notEqual((await cli([...unknownArgs.slice(0,-1),'scan-after-observation'])).code,0);assert.equal(requests.length,duplicateAfterObservation);

      interactive=false;jobStatus='FINISHED';scanAcknowledgement='remote-job';autoAttach=true;
      fs.unlinkSync(path.join(media,'mock.funscript'));tags.set('auto','Auto Authored');
      const automatic=await cli([...batchArgs,'--scan','--tag','Auto Authored']);assert.equal(automatic.code,0,automatic.err+automatic.out);
      const autoReport=JSON.parse(automatic.out);assert.equal(autoReport.state,'finished');assert.equal(autoReport.stash_mutations_requested,true);
      assert.equal(autoReport.items[0].state,'tagged_unqualified');assert.equal(autoReport.items[0].scan.state,'verified_linked');assert.equal(autoReport.items[0].tag.state,'verified_added');
      assert.ok(sceneTags.has('old') && sceneTags.has('auto'));

      // Exercise recorded ancestry through the real authenticated engine IPC.
      const credentials=JSON.parse(fs.readFileSync(path.join(env.PULSAR_STATE_DIR,'first-party.session.json'),'utf8'));
      let sequence=0;
      const rpc=(project,revision,operation,args)=>new Promise((resolve,reject)=> {
        const requestId=`lineage-fixture-${++sequence}`;
        const message=Buffer.from(JSON.stringify({version:2,request_id:requestId,...credentials,project,expected_revision:revision,command:{operation,arguments:args}}));
        const header=Buffer.alloc(4);header.writeUInt32BE(message.length);
        const socket=net.createConnection(path.join(env.PULSAR_STATE_DIR,'engine.sock'));let bytes=Buffer.alloc(0);
        socket.setTimeout(10000,()=>socket.destroy(new Error('fixture IPC deadline')));socket.once('error',reject);
        socket.once('close',()=>reject(new Error('fixture IPC closed before a complete response')));
        socket.once('connect',()=>socket.write(Buffer.concat([header,message])));
        socket.on('data',chunk=>{bytes=Buffer.concat([bytes,chunk]);if(bytes.length<4)return;const length=bytes.readUInt32BE();if(length>1024*1024){socket.destroy(new Error('fixture response bound'));return;}if(bytes.length>=length+4){socket.end();const response=JSON.parse(bytes.subarray(4,length+4));assert.equal(response.request_id,requestId);resolve(response.result);}});
      });
      const derived=autoReport.items[0];interactive=false;autoAttach=false;fs.unlinkSync(path.join(media,'mock.funscript'));
      const rebased=await rpc(derived.project_id,derived.export.revision,'rebase_candidate',{candidate_id:derived.candidate_id});
      assert.ok(rebased.Ok,'rebasing retained generation must succeed');
      const merged=await rpc(derived.project_id,derived.export.revision,'commit_candidate',{candidate_id:rebased.Ok.value.candidate_id});
      assert.ok(merged.Ok,`rebased generation must commit through the authority: ${JSON.stringify(merged.Err)}`);
      const current=await cli(['project','open',derived.project_id]);assert.equal(current.code,0,current.err);const mergeRevision=JSON.parse(current.out).value.revision;
      const derivedArgs=['stash','publish-script',derived.project_id,'s1','f1','--url',url,'--source-version',derived.source_version,'--candidate',derived.candidate_id,'--job',derived.job_id,'--revision',String(mergeRevision),'--request-id','derived-publication'];
      const derivedPublication=await cli(derivedArgs);assert.equal(derivedPublication.code,0,derivedPublication.err);
      assert.equal(JSON.parse(derivedPublication.out).value.revision,mergeRevision);assert.equal(JSON.parse(derivedPublication.out).value.release_qualified,false);
      fs.unlinkSync(path.join(media,'mock.funscript'));
      const unrelated=path.join(root,'unrelated.funscript');fs.writeFileSync(unrelated,JSON.stringify({version:'1.0',actions:[{at:0,pos:0},{at:500,pos:100},{at:1000,pos:0}]}));
      const imported=await rpc(derived.project_id,mergeRevision,'import_funscript',{path:unrelated});assert.ok(imported.Ok);
      const contaminated=await rpc(derived.project_id,mergeRevision,'merge_candidate',{candidate_id:imported.Ok.value.candidate_id,axes:['stroke'],range:null});assert.ok(contaminated.Ok);
      const contaminatedProject=await cli(['project','open',derived.project_id]);assert.equal(contaminatedProject.code,0);
      const foreignPublication=await cli([...derivedArgs.slice(0,-4),'--revision',String(JSON.parse(contaminatedProject.out).value.revision),'--request-id','foreign-publication']);
      assert.notEqual(foreignPublication.code,0);assert.match(foreignPublication.err,/recorded derivation/);assert.equal(fs.existsSync(path.join(media,'mock.funscript')),false);

      const restored=await rpc(derived.project_id,JSON.parse(contaminatedProject.out).value.revision,'undo',undefined);assert.ok(restored.Ok);
      const restoredProject=await cli(['project','open',derived.project_id]);assert.equal(restoredProject.code,0);
      const restoredPublication=await cli([...derivedArgs.slice(0,-4),'--revision',String(JSON.parse(restoredProject.out).value.revision),'--request-id','restored-publication']);
      assert.equal(restoredPublication.code,0,restoredPublication.err);assert.equal(JSON.parse(restoredPublication.out).value.release_qualified,false);

      // Query-only attachment observation must survive an older mutation acknowledgement.
      scanAcknowledgement=null;holdScan=true;
      const heldArgs=['stash','submit-scan',derived.project_id,'--publication-request-id','restored-publication','--url',url,'--revision',String(JSON.parse(restoredProject.out).value.revision),'--request-id','held-scan'];
      const heldSubmission=cli(heldArgs);await Promise.race([scanArrived,delay(10000).then(()=>{throw new Error('owned scan dispatch deadline');})]);
      interactive=true;const heldObservation=await cli(['stash','reconcile-attachment',derived.project_id,'--scan-request-id','held-scan','--url',url]);assert.equal(heldObservation.code,0,heldObservation.err);
      assert.equal(JSON.parse(heldObservation.out).value.attachment.state,'observed_linked');scanRelease();holdScan=false;
      const heldResult=await heldSubmission;assert.notEqual(heldResult.code,0);const heldReceipt=JSON.parse(heldResult.out).value;
      assert.equal(heldReceipt.state,'dispatch_unknown');assert.equal(heldReceipt.attachment.state,'observed_linked');assert.equal(heldReceipt.remote_job_id,null);

      const invalidView=path.join(root,'invalid-view.json');fs.writeFileSync(invalidView,JSON.stringify({layout:'mono',eye:'right',projection:{kind:'flat'}}));
      const beforeInvalidView=requests.length;const refusedView=await cli([...batchArgs,'--video-view',invalidView]);
      assert.notEqual(refusedView.code,0);assert.match(refusedView.err,/invalid or ambiguous declared video view/);assert.equal(requests.length,beforeInvalidView,'invalid declared batch view is refused before Stash discovery');

      const beforeInvalid=requests.length;
      fs.writeFileSync(mappings,JSON.stringify([{server_root:'/stash/../other',local_root:media}]));
      const invalid=await cli(planArgs);assert.notEqual(invalid.code,0);
      assert.equal(requests.length,beforeInvalid,'invalid root mapping cannot reach the network');

    } finally {
      if(scanRelease)scanRelease();
      if (engine.exitCode===null) engine.kill('SIGTERM');await engineExit;
      fs.writeFileSync(path.join(root,'engine.stderr'),engineError,{mode:0o600});
      await new Promise(resolve=>server.close(resolve));
    }
  });

test('engine batch survives client disconnect and restart at a recorded capture phase',
  {skip:!process.env.PULSAR_STASH_TEST_BINARY,timeout:60000},async t=> {
    const root=fs.mkdtempSync(path.join(os.tmpdir(),'pulsar-batch-recovery-'));fs.chmodSync(root,0o700);
    t.diagnostic(`retained private batch recovery fixture: ${root}`);
    const media=path.join(root,'media');fs.mkdirSync(media,{mode:0o700});
    const frames=Buffer.concat([Buffer.from('YUV4MPEG2 W64 H64 F8:1 Ip A1:1 Cmono\n'),...Array.from({length:8},(_,frame)=>Buffer.concat([Buffer.from('FRAME\n'),Buffer.from(Array.from({length:64*64},(_,i)=>{const x=i%64,y=Math.floor(i/64),sy=(y+64-frame)%64;return (x*31+sy*17+Math.floor(x/7)*Math.floor(sy/5)*13)%256;}))]))]);
    fs.writeFileSync(path.join(media,'source.mp4'),frames);
    const mappings=path.join(root,'mappings.json');fs.writeFileSync(mappings,JSON.stringify([{server_root:'/stash',local_root:media}]),{mode:0o600});
    let hold=true,releaseCapture,captureResolve;const captured=new Promise(resolve=>{captureResolve=resolve;});let sceneQueries=0;
    const server=http.createServer(async(req,res)=> {
      const chunks=[];for await(const chunk of req)chunks.push(chunk);const body=JSON.parse(Buffer.concat(chunks));
      const scene={id:'recovery-scene',title:'Authored recovery',interactive:false,files:[{id:'file',path:'/stash/source.mp4'}]};
      if(body.query.includes('findScene(id:')) {sceneQueries++;
        if(hold){captureResolve();await new Promise(resolve=>{releaseCapture=resolve;});}
      }
      res.writeHead(200,{'Content-Type':'application/json',Connection:'close'});
      res.end(JSON.stringify({data:body.query.includes('findScene(id:')?{findScene:scene}:{findScenes:{count:1,scenes:[scene]}}}));
    });
    await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
    const binary=fs.realpathSync(process.env.PULSAR_STASH_TEST_BINARY),env={PATH:'/usr/bin:/bin',PULSAR_STATE_DIR:path.join(root,'engine')};
    let engine,engineExit,engineError='';
    const start=async()=> {
      engine=spawn(binary,['engine'],{env,stdio:['ignore','ignore','pipe']});engineExit=new Promise(resolve=>engine.once('exit',resolve));
      engine.stderr.on('data',data=>{engineError=(engineError+data).slice(0,65536);});
      const deadline=Date.now()+10000;
      while(true) {
        assert.equal(engine.exitCode,null,engineError);assert.ok(Date.now()<deadline,'same engine startup deadline');
        if(fs.existsSync(path.join(env.PULSAR_STATE_DIR,'engine.sock')) && (await cli(['capabilities'])).code===0) break;
        await delay(25);
      }
    };
    const cli=args=>new Promise((resolve,reject)=> {
      const child=spawn(binary,args,{env,stdio:['ignore','pipe','pipe']});let out='',err='';child.stdout.on('data',data=>{out+=data;});child.stderr.on('data',data=>{err+=data;});child.once('error',reject);child.once('exit',code=>resolve({code,out,err}));
    });
    try {
      await start();const url=`http://127.0.0.1:${server.address().port}/graphql`;
      const args=['stash','generate-missing','--mappings',mappings,'--url',url,'--width','64','--height','64','--detach','--request-id','durable-recovery-batch'];
      const submitted=await cli(args);assert.equal(submitted.code,0,submitted.err);const batch=JSON.parse(submitted.out);assert.equal(batch.state,'running');
      await Promise.race([captured,delay(10000).then(()=>{throw new Error('capture dispatch deadline');})]);
      const before=await cli(['stash','batch-status',batch.batch_request_id]);assert.equal(before.code,0,before.err);const original=JSON.parse(before.out).value.items[0];
      assert.ok(original.project_id);assert.equal(original.requests.at(-1).phase,'capture');
      engine.kill('SIGKILL');await engineExit;hold=false;releaseCapture();releaseCapture=null;
      // Startup reconciles the durable pending phase without a client resume.
      await start();const deadline=Date.now()+30000;let report;
      do {
        const status=await cli(['stash','batch-status',batch.batch_request_id]);assert.equal(status.code,0,status.err);report=JSON.parse(status.out).value;
        assert.ok(Date.now()<deadline,JSON.stringify(report));if(report.state==='running')await delay(100);
      }while(report.state==='running');
      assert.equal(report.state,'finished',JSON.stringify(report));assert.equal(report.items[0].state,'published_unqualified');
      assert.equal(report.items[0].project_id,original.project_id);
      assert.equal(report.items[0].requests.find(effect=>effect.phase==='capture').request_id,original.requests.find(effect=>effect.phase==='capture').request_id);
      assert.equal(report.items[0].requests.filter(effect=>effect.phase==='create').length,1);
      assert.ok(JSON.parse(fs.readFileSync(path.join(media,'source.funscript'))).actions.length>0);assert.ok(sceneQueries>=3);
      const replay=await cli(args);assert.equal(replay.code,0,replay.err);
      const current=JSON.parse((await cli(['stash','batch-status',batch.batch_request_id])).out).value;assert.equal(current.items[0].project_id,original.project_id);
      fs.unlinkSync(path.join(media,'source.funscript'));hold=true;
      const cancellationCaptured=new Promise(resolve=>{captureResolve=resolve;});
      const cancelledSubmit=await cli([...args.slice(0,-1),'cancel-recovery-batch']);assert.equal(cancelledSubmit.code,0,cancelledSubmit.err);
      const cancelledId=JSON.parse(cancelledSubmit.out).batch_request_id;
      await Promise.race([cancellationCaptured,delay(10000).then(()=>{throw new Error('cancel capture deadline');})]);
      const cancelled=await cli(['stash','cancel-batch',cancelledId,'--request-id','cancel-effect']);assert.equal(cancelled.code,0,cancelled.err);assert.equal(JSON.parse(cancelled.out).kind,'ack');
      hold=false;releaseCapture();releaseCapture=null;await delay(300);
      assert.equal(JSON.parse((await cli(['stash','batch-status',cancelledId])).out).value.state,'cancelled');
      assert.equal(fs.existsSync(path.join(media,'source.funscript')),false);
      engine.kill('SIGTERM');await engineExit;await start();
      const stopped=JSON.parse((await cli(['stash','batch-status',cancelledId])).out).value;
      assert.equal(stopped.state,'cancelled');assert.equal(stopped.items[0].requests.some(effect=>effect.phase==='generate'),false);
      assert.notEqual((await cli(['stash','resume-batch',cancelledId])).code,0);
      assert.equal(fs.existsSync(path.join(media,'source.funscript')),false);

    }finally {
      if(releaseCapture)releaseCapture();if(engine?.exitCode===null)engine.kill('SIGTERM');if(engineExit)await engineExit;
      fs.writeFileSync(path.join(root,'engine.stderr'),engineError,{mode:0o600});await new Promise(resolve=>server.close(resolve));
    }
  });
