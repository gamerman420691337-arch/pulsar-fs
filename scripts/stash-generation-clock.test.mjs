// Authored native publication: one source identity does not imply one clock.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import http from 'node:http';
import net from 'node:net';
import {spawn} from 'node:child_process';
import {setTimeout as delay} from 'node:timers/promises';

test('live Stash publication preserves equal generation clocks and refuses mixed placements',
 {skip:!process.env.PULSAR_STASH_TEST_BINARY,timeout:90000},async t=>{
 const root=fs.mkdtempSync(path.join(os.tmpdir(),'pulsar-generation-clock-'));fs.chmodSync(root,0o700);
 t.diagnostic(`retained authored generation-clock evidence: ${root}`);
 const media=path.join(root,'media');fs.mkdirSync(media);
 fs.writeFileSync(path.join(media,'fixture.y4m'),Buffer.concat([Buffer.from('YUV4MPEG2 W64 H64 F8:1 Ip A1:1 Cmono\n'),...Array.from({length:8},(_,f)=>Buffer.concat([Buffer.from('FRAME\n'),Buffer.from(Array.from({length:4096},(_,i)=>{const x=i%64,y=(Math.floor(i/64)+64-f)%64;return (x*31+y*17+Math.floor(x/7)*Math.floor(y/5)*13)%256;}))]))]));
 const audioPath=path.join(root,'audio.wav'),pcm=Buffer.alloc(32000);for(let i=0;i<16000;i++)pcm.writeInt16LE(13107,i*2);
 const wav=Buffer.alloc(44);wav.write('RIFF');wav.writeUInt32LE(36+pcm.length,4);wav.write('WAVEfmt ',8);wav.writeUInt32LE(16,16);wav.writeUInt16LE(1,20);wav.writeUInt16LE(1,22);wav.writeUInt32LE(16000,24);wav.writeUInt32LE(32000,28);wav.writeUInt16LE(2,32);wav.writeUInt16LE(16,34);wav.write('data',36);wav.writeUInt32LE(pcm.length,40);fs.writeFileSync(audioPath,Buffer.concat([wav,pcm]));
 let queryCount=0;
 const server=http.createServer(async(req,res)=>{try{const parts=[];for await(const b of req)parts.push(b);const body=JSON.parse(Buffer.concat(parts));assert.ok(body.query.trim().startsWith('query'));queryCount++;res.writeHead(200,{'Content-Type':'application/json',Connection:'close'});res.end(JSON.stringify({data:{findScene:{id:'scene',title:'Authored clocks',interactive:false,files:[{id:'file',path:'/fixture/fixture.y4m'}]}}}));}catch(error){res.writeHead(500);res.end();}});
 await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
 const binary=fs.realpathSync(process.env.PULSAR_STASH_TEST_BINARY),state=path.join(root,'state');
 const env={PATH:'/usr/bin:/bin',TMPDIR:process.env.TMPDIR??root,PULSAR_STATE_DIR:state};
 const engine=spawn(binary,['engine'],{env,stdio:['ignore','ignore','pipe']});let stderr='';engine.stderr.on('data',b=>stderr=(stderr+b).slice(-65536));const exit=new Promise(resolve=>engine.once('exit',resolve));let sequence=0;
 const cli=(args,executable=binary)=>new Promise((resolve,reject)=>{const child=spawn(executable,args,{env,stdio:['ignore','pipe','pipe']});let stdout='',stderr='';const timer=setTimeout(()=>child.kill('SIGKILL'),30000);child.stdout.on('data',b=>stdout+=b);child.stderr.on('data',b=>stderr+=b);child.once('error',error=>{clearTimeout(timer);reject(error);});child.once('exit',status=>{clearTimeout(timer);const result={status,stdout,stderr};fs.writeFileSync(path.join(root,`cli-${++sequence}.json`),JSON.stringify({args,...result},null,2));resolve(result);});});
 const ok=async args=>{const r=await cli(args);assert.equal(r.status,0,r.stderr+r.stdout);return JSON.parse(r.stdout).value;};
 const rpc=(project,revision,operation,args)=>new Promise((resolve,reject)=>{
  const credentials=JSON.parse(fs.readFileSync(path.join(state,'first-party.session.json')));const body=Buffer.from(JSON.stringify({version:2,request_id:`generation-clock-${++sequence}`,...credentials,project,expected_revision:revision,command:{operation,arguments:args}}));const header=Buffer.alloc(4);header.writeUInt32BE(body.length);const socket=net.createConnection(path.join(state,'engine.sock'));let received=Buffer.alloc(0);
  socket.setTimeout(10000,()=>socket.destroy(new Error('IPC deadline')));socket.once('error',reject);socket.once('close',()=>reject(new Error('IPC closed before reply')));socket.once('connect',()=>socket.write(Buffer.concat([header,body])));socket.on('data',b=>{received=Buffer.concat([received,b]);if(received.length<4)return;const length=received.readUInt32BE();if(length>1024*1024){socket.destroy(new Error('IPC size'));return;}if(received.length>=length+4){resolve(JSON.parse(received.subarray(4,length+4)).result);socket.destroy();}});
 });
 const good=async(...args)=>{const r=await rpc(...args);assert.ok(r.Ok,JSON.stringify(r.Err));return r.Ok.value;};
 try{
  const startup=Date.now()+15000;while(true){assert.equal(engine.exitCode,null,stderr);assert.ok(Date.now()<startup,'startup deadline');if(fs.existsSync(path.join(state,'engine.sock'))&&(await cli(['capabilities'])).status===0)break;await delay(25);}
  const mappings=path.join(root,'mappings.json');fs.writeFileSync(mappings,JSON.stringify([{server_root:'/fixture',local_root:media}]));
  for(const rate of [0,1,2,3]){
   fs.rmSync(path.join(media,'fixture.funscript'),{force:true});
   const project=(await ok(['project','create',`generation clock ${rate}`])).project_id;
   const capture=await ok(['stash','capture-source',project,'scene','file','--url',`http://127.0.0.1:${server.address().port}/graphql`,'--mappings',mappings]);
   const spec={source_version:capture.source_version,source_start:{numerator:0,denominator:1},source_end:{numerator:rate===3?750000001:875000001,denominator:1000000000},source_anchor:{numerator:0,denominator:1},project_anchor:0,rate_numerator:1,rate_denominator:1};
   let audioSource;
   const generate=async(rate,stride=1)=>{
    const file=path.join(root,'placement.json');fs.writeFileSync(file,JSON.stringify({...spec,rate_numerator:rate}));const placed=await ok(rate===0?['project','open',project]:['project','place',project,'--spec',file]);const placement=rate===0?null:placed.placements.reduce((a,b)=>a.created_revision>b.created_revision?a:b);
    if(placement)assert.equal(placement.spec.rate_numerator,rate);
    const job=(await good(project,placed.revision,'generate',{source_version:capture.source_version,preset:'fast',model_path:null,settings:{source_placement:placement,analysis_width:64,analysis_height:64,source_sample_stride:stride,axis:"stroke",vr_mode:false,model_input:null,...(audioSource?{mixed_audio:{intent:"creative_audio",mapping:{axis:"stroke",amplitude:0.2,offset:0.4},independent:{source_version:audioSource,alignment:{video_anchor:{numerator:0,denominator:1},audio_anchor:{numerator:0,denominator:1}}}}}:{})}})).job_id;
    const deadline=Date.now()+30000;while(true){const status=await good(project,null,'job_status',{job_id:job});if(status.state==='completed')return {job,candidate:status.candidate_id,base:placed.revision};assert.ok(['queued','running'].includes(status.state),JSON.stringify(status));assert.ok(Date.now()<deadline,'generation deadline');await delay(25);}
   };
   if(rate===3)audioSource=(await good(project,(await ok(['project','open',project])).revision,'import_source',{path:audioPath})).source_version;
   const original=await generate(rate===0?0:rate===3?3:1,rate===3?3:1);await good(project,original.base,'commit_candidate',{candidate_id:original.candidate});
   audioSource=(await good(project,(await ok(["project","open",project])).revision,"import_source",{path:audioPath})).source_version;
   const contributed=await generate(rate);const merged=await good(project,contributed.base,'merge_candidate',{candidate_id:contributed.candidate,axes:['stroke'],range:{start:125000000,end:375000001}});
   const before=await ok(['project','open',project]);assert.equal(before.revision,merged.revision);const queriesBefore=queryCount;
   if(rate===0||rate===1){
    const attempts=path.join(state,'attempts');
    const receipts=fs.readdirSync(attempts).map(name=>path.join(attempts,name,'receipt.json')).filter(file=>{
     if(!fs.existsSync(file))return false;return JSON.parse(fs.readFileSync(file)).job_id===contributed.job;
    });
    assert.equal(receipts.length,1,'one immutable contributor receipt');
    for(const mode of ['corrupt','missing']){
     const receipt=receipts[0],backing=receipt+'.saved';fs.renameSync(receipt,backing);if(mode==='corrupt')fs.writeFileSync(receipt,'{}');
     try{
      const refused=await cli(['stash','publish-script',project,'scene','file','--url',`http://127.0.0.1:${server.address().port}/graphql`,'--source-version',capture.source_version,'--candidate',original.candidate,'--job',original.job,'--revision',String(merged.revision),...(rate===0?[]:['--source-time']),'--request-id',`${rate}-${mode}-contributor-publication`]);
      fs.writeFileSync(path.join(root,`${rate}-${mode}-contributor.json`),JSON.stringify({original,contributed,receipt,refused},null,2));
      if(refused.status===0){const saved=JSON.parse(refused.stdout).value;fs.copyFileSync(saved.path,path.join(root,`recorded-${rate}-${mode}-contributor.funscript`));if(saved.timing_proof)fs.copyFileSync(saved.timing_proof.path,path.join(root,`recorded-${rate}-${mode}-contributor-proof.json`));}
      assert.notEqual(refused.status,0,'an unverified contributing clock must not publish');
      assert.match(refused.stderr,/DependencyMismatch/);assert.equal(fs.existsSync(path.join(media,'fixture.funscript')),false);
     }finally{fs.rmSync(receipt,{force:true});fs.renameSync(backing,receipt);}
    }
   }
   const result=await cli(['stash','publish-script',project,'scene','file','--url',`http://127.0.0.1:${server.address().port}/graphql`,'--source-version',capture.source_version,'--candidate',original.candidate,'--job',original.job,'--revision',String(merged.revision),...(rate===0?[]:['--source-time']),'--request-id',`clock-publication-${rate}`]);
   fs.writeFileSync(path.join(root,`publication-${rate}.json`),JSON.stringify({rate,original,contributed,revision:merged.revision,result,queriesBefore,queriesAfter:queryCount},null,2));
   if(result.status===0){const receipt=JSON.parse(result.stdout).value;fs.copyFileSync(receipt.path,path.join(root,`recorded-script-${rate}.funscript`));if(receipt.timing_proof)fs.copyFileSync(receipt.timing_proof.path,path.join(root,`recorded-proof-${rate}.json`));}
   if(rate===0||rate===1){assert.equal(result.status,0,result.stderr);assert.ok(fs.existsSync(path.join(media,'fixture.funscript')));}
   else{assert.notEqual(result.status,0,'different generation/sample clocks must not publish under the selected anchor');assert.match(result.stderr,rate===3?/action-derived clocks|multi-carrier proof/:/different generation clocks|correspondence proof/);assert.equal(fs.existsSync(path.join(media,'fixture.funscript')),false);}
   if(rate===3){
    const allowed=await cli(['allow-packaging',project,'--acknowledge-unencrypted-private-data']);assert.equal(allowed.status,0,allowed.stderr);
    const archive=path.join(root,'mixed-sampling.pulsar');const exported=await cli(['package-export',project,archive,'--acknowledge-unencrypted-private-data']);assert.equal(exported.status,0,exported.stderr);
    const imported=await cli(['package-import',archive]);assert.equal(imported.status,0,imported.stderr);const clone=JSON.parse(imported.stdout).receipt.project_id;
    const mapped=await cli(['-c',"import sqlite3,json,sys; d=sqlite3.connect('file:'+sys.argv[1]+'?mode=ro',uri=True); m=json.loads(d.execute('SELECT manifest FROM project_import_archives WHERE project=?',(sys.argv[2],)).fetchone()[0]); e=next(e for e in m['imported_origins'] if e['container']['project_id']==sys.argv[3]); print(next(c['local'] for c in e['candidates'] if c['origin']==sys.argv[4]))",path.join(state,'projects.sqlite3'),clone,project,original.candidate],'/usr/bin/python3');
    assert.equal(mapped.status,0,mapped.stderr);const alias=mapped.stdout.trim();
    const report=await ok(['project','archived-generation-status',clone,alias]);const origin=report.origins[0];assert.ok(origin.clock);
    const reference=path.join(root,'mixed-sampling-origin.json');fs.writeFileSync(reference,JSON.stringify(origin.origin_manifest));
    const cloneHead=await ok(['project','open',clone]);await ok(['stash','capture-source',clone,'scene','file','--url',`http://127.0.0.1:${server.address().port}/graphql`,'--mappings',mappings,'--expected-source-version',capture.source_version,'--revision',String(cloneHead.revision)]);
    const refused=await cli(['stash','publish-script',clone,'scene','file','--url',`http://127.0.0.1:${server.address().port}/graphql`,'--source-version',capture.source_version,'--candidate',alias,'--job',origin.job_id,'--revision',String(cloneHead.revision),'--source-time','--archived-origin',reference,'--request-id','archived-mixed-sampling']);
    fs.writeFileSync(path.join(root,'archived-mixed-sampling.json'),JSON.stringify({clone,alias,report,cloneHead,refused},null,2));
    assert.notEqual(refused.status,0,'a clone must not bypass the mixed-sampling proof requirement');assert.match(refused.stderr,/DependencyMismatch/);assert.equal(fs.existsSync(path.join(media,'fixture.funscript')),false);
   }
   assert.deepEqual(await ok(['project','open',project]),before);
   if(rate===0||rate===1){
    fs.rmSync(path.join(media,'fixture.funscript'));
    const baseProgram=JSON.parse(fs.readFileSync(path.join(state,'motion-objects',before.motion.program.sha256)));
    const edits=path.join(root,'native-edit-values.json');fs.writeFileSync(edits,JSON.stringify({tracks:baseProgram.tracks.map(track=>({axis:track.axis,actions:track.actions.map((a,i)=>({time:a.time,position:i===0?0.13:a.position})),gaps:track.gaps}))}));
    const edited=await ok(['edit-values',project,edits,'--revision',String(before.revision)]);
    const head=await ok(['commit-candidate',project,edited.candidate_id,'--revision',String(before.revision)]);
    const receipts=fs.readdirSync(path.join(state,'edit-receipts')).filter(name=>JSON.parse(fs.readFileSync(path.join(state,'edit-receipts',name))).project_id===project);assert.equal(receipts.length,1);
    const receiptPath=path.join(state,'edit-receipts',receipts[0]);const immutable=JSON.parse(fs.readFileSync(receiptPath));
    for(const mode of ['corrupt-receipt','missing-receipt','corrupt-input']){
     const target=mode==='corrupt-input'?path.join(state,'edit-inputs',immutable.upload_sha256):receiptPath;
     const saved=target+'.saved';fs.renameSync(target,saved);if(mode!=='missing-receipt')fs.writeFileSync(target,'{}');
     try{
      const refused=await cli(['stash','publish-script',project,'scene','file','--url',`http://127.0.0.1:${server.address().port}/graphql`,'--source-version',capture.source_version,'--candidate',original.candidate,'--job',original.job,'--revision',String(head.revision),...(rate===0?[]:['--source-time']),'--request-id',`native-edit-${rate}-${mode}`]);
      fs.writeFileSync(path.join(root,`native-edit-${rate}-${mode}.json`),JSON.stringify({edited,head,refused},null,2));
      if(refused.status===0){const pub=JSON.parse(refused.stdout).value;fs.copyFileSync(pub.path,path.join(root,`wrong-native-edit-${rate}-${mode}.funscript`));if(pub.timing_proof)fs.copyFileSync(pub.timing_proof.path,path.join(root,`wrong-native-edit-${rate}-${mode}-proof.json`));}
      assert.notEqual(refused.status,0,'an unverified authored edit must not publish');assert.match(refused.stderr,/DependencyMismatch/);assert.equal(fs.existsSync(path.join(media,'fixture.funscript')),false);
     }finally{fs.rmSync(target,{force:true});fs.renameSync(saved,target);}
    }
    const restored=await cli(['stash','publish-script',project,'scene','file','--url',`http://127.0.0.1:${server.address().port}/graphql`,'--source-version',capture.source_version,'--candidate',original.candidate,'--job',original.job,'--revision',String(head.revision),...(rate===0?[]:['--source-time']),'--request-id',`native-edit-${rate}-restored`]);
    fs.writeFileSync(path.join(root,`native-edit-${rate}-restored.json`),JSON.stringify(restored,null,2));assert.equal(restored.status,0,restored.stderr);assert.equal(fs.existsSync(path.join(media,'fixture.funscript')),true);assert.deepEqual(await ok(['project','open',project]),head);
   }
  }
 }finally{
  if(engine.exitCode===null&&engine.signalCode===null){engine.kill('SIGTERM');const timer=setTimeout(()=>engine.kill('SIGKILL'),2000);try{await exit;}finally{clearTimeout(timer);}}
  fs.writeFileSync(path.join(root,'engine-stderr.json'),JSON.stringify({stderr,exitCode:engine.exitCode,signalCode:engine.signalCode},null,2));
  await new Promise(resolve=>server.close(resolve));
 }
});
