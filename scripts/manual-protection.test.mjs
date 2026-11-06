import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import net from 'node:net';
import crypto from 'node:crypto';
import {spawn,spawnSync} from 'node:child_process';
import {setTimeout as delay} from 'node:timers/promises';

test('manual edits acquire protection and existing-project regeneration preserves edits, missing axes and history',
 {skip:!process.env.PULSAR_STASH_TEST_BINARY,timeout:90000},async t=>{
  const root=fs.mkdtempSync(path.join(os.tmpdir(),'pulsar-manual-protection-'));fs.chmodSync(root,0o700);t.diagnostic(`retained authored manual-protection fixture: ${root}`);
  const binary=fs.realpathSync(process.env.PULSAR_STASH_TEST_BINARY),state=path.join(root,'state'),env={PATH:'/usr/bin:/bin',PULSAR_STATE_DIR:state};
  let engine,exit,stderr='';const start=()=>{engine=spawn(binary,['engine'],{env,stdio:['ignore','ignore','pipe']});engine.stderr.on('data',b=>{stderr=(stderr+b).slice(-65536);});exit=new Promise(resolve=>engine.once('exit',resolve));};
  const cli=args=>{const r=spawnSync(binary,args,{env,encoding:'utf8',timeout:30000,maxBuffer:4*1024*1024});assert.equal(r.error,undefined);return r;};const ok=args=>{const r=cli(args);assert.equal(r.status,0,r.stderr+r.stdout);return JSON.parse(r.stdout);};
  const ready=async()=>{const end=Date.now()+15000;while(true){assert.equal(engine.exitCode,null,stderr);assert.ok(Date.now()<end,'engine startup deadline');if(fs.existsSync(path.join(state,'engine.sock'))&&cli(['capabilities']).status===0)return;await delay(25);}};
  const stop=async()=>{if(engine?.exitCode===null && engine.signalCode===null){engine.kill('SIGTERM');const timer=setTimeout(()=>engine.kill('SIGKILL'),2000);try{await exit;}finally{clearTimeout(timer);}}};
  let sequence=0;const rpc=(project,revision,operation,args)=>new Promise((resolve,reject)=>{const credentials=JSON.parse(fs.readFileSync(path.join(state,'first-party.session.json')));const bytes=Buffer.from(JSON.stringify({version:2,request_id:`protection-fixture-${++sequence}`,...credentials,project,expected_revision:revision,command:{operation,...(args===undefined?{}:{arguments:args})}}));const header=Buffer.alloc(4);header.writeUInt32BE(bytes.length);const socket=net.createConnection(path.join(state,'engine.sock'));let received=Buffer.alloc(0);socket.setTimeout(10000,()=>socket.destroy(new Error('IPC deadline')));socket.once('error',reject);socket.once('close',()=>reject(new Error('IPC closed before response')));socket.once('connect',()=>socket.write(Buffer.concat([header,bytes])));socket.on('data',b=>{received=Buffer.concat([received,b]);if(received.length<4)return;const size=received.readUInt32BE();if(size>1024*1024){socket.destroy(new Error('response bound'));return;}if(received.length>=size+4){resolve(JSON.parse(received.subarray(4,size+4)).result);socket.destroy();}});});
  const hash=file=>crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
  try {
   start();await ready();const p=ok(['project','create','Authored protection reproduction']).value.project_id;
   const file=path.join(root,'original.funscript');fs.writeFileSync(file,JSON.stringify({actions:Array.from({length:8},(_,i)=>({at:i*1000,pos:40}))}));const originalHash=hash(file);
   const base=ok(['import-script',file,'--project',p]).value.candidate_id;ok(['commit-candidate',p,base,'--revision','0']);
   const before=ok(['project','open',p]).value;const values={tracks:before.program.tracks.map(track=>({axis:track.axis,actions:track.actions.map(a=>({time:a.time,position:a.time===3000000000?0.9:a.position})),gaps:track.gaps}))};values.tracks.push({axis:'yaw',actions:Array.from({length:8},(_,i)=>({time:i*1000000000,position:0.6})),gaps:[]});
   const draft=path.join(root,'values.json');fs.writeFileSync(draft,JSON.stringify(values));const c=ok(['edit-values',p,draft,'--revision','1']).value.candidate_id;
   assert.equal(ok(['protection','status',p]).value.protected_regions.length,0);assert.deepEqual(ok(['project','open',p]).value,before);
   const committed=ok(['commit-candidate',p,c,'--revision','1','--request-id','authored-manual-commit']).value;
   const policy=ok(['protection','status',p]).value.protected_regions;const point=project=>project.program.tracks.find(track=>track.axis==='stroke').actions.find(a=>a.time===3000000000)?.position;
   assert.equal(point(committed),0.9);
   const video=path.join(root,'stationary.y4m'),texture=Buffer.from(Array.from({length:4096},(_,i)=>(i%64*31+Math.floor(i/64)*17)%256));fs.writeFileSync(video,Buffer.concat([Buffer.from('YUV4MPEG2 W64 H64 F8:1 Ip A1:1 Cmono\n'),...Array.from({length:64},()=>Buffer.concat([Buffer.from('FRAME\n'),texture]))]));
   const imported=await rpc(p,committed.revision,'import_source',{path:video});assert.ok(imported.Ok);const source=imported.Ok.value.source_version;
   const current=ok(['project','open',p]).value;const regenerated=ok(['regenerate',p,source,'--revision',String(current.revision)]).value;
   if(process.env.PULSAR_MANUAL_PROTECTION_BASELINE==='1') {
    assert.equal(policy.length,0);assert.equal(point(regenerated),0.5);const observation={state:'reproduced_missing_automatic_manual_protection',binary_sha256:hash(binary),before_revision:before.revision,manual_revision:committed.revision,protected_regions:policy,authored_position_before_regeneration:point(committed),position_after_regeneration:point(regenerated),source_sha256:originalHash};
    if(process.env.PULSAR_MANUAL_PROTECTION_EVIDENCE){fs.mkdirSync(process.env.PULSAR_MANUAL_PROTECTION_EVIDENCE,{recursive:true});fs.writeFileSync(path.join(process.env.PULSAR_MANUAL_PROTECTION_EVIDENCE,'baseline.json'),JSON.stringify(observation,null,2)+'\n',{flag:'wx'});}return;
   }
   assert.equal(policy.length,2);assert.ok(policy.some(r=>r.axis==='stroke' && r.range.start<=3000000000 && r.range.end>3000000000));assert.ok(policy.some(r=>r.axis==='yaw'));
   assert.equal(point(regenerated),0.9);assert.deepEqual(regenerated.program.tracks.find(track=>track.axis==='yaw'),committed.program.tracks.find(track=>track.axis==='yaw'));
   assert.equal(regenerated.program.tracks.find(track=>track.axis==='stroke').actions.find(a=>a.time===0).position,0.5);assert.deepEqual(ok(['protection','status',p]).value.protected_regions,policy);
   const undone=await rpc(p,regenerated.revision,'undo');assert.ok(undone.Ok);assert.equal(point(ok(['project','open',p]).value),0.9);assert.deepEqual(ok(['protection','status',p]).value.protected_regions,policy);
   const redo=await rpc(p,undone.Ok.value.revision,'redo');assert.ok(redo.Ok);const restored=ok(['project','open',p]).value;assert.deepEqual(restored.program,regenerated.program);
   const replacement={tracks:restored.program.tracks.map(track=>({axis:track.axis,actions:track.actions.map(a=>({time:a.time,position:track.axis==='stroke' && a.time===3000000000?0.8:a.position})),gaps:track.gaps}))};fs.writeFileSync(draft,JSON.stringify(replacement));const rejectedCandidate=ok(['edit-values',p,draft,'--revision',String(restored.revision)]).value.candidate_id;
   const denied=cli(['commit-candidate',p,rejectedCandidate,'--revision',String(restored.revision)]);assert.notEqual(denied.status,0);assert.match(denied.stderr,/Forbidden/);assert.deepEqual(ok(['project','open',p]).value,restored);
   const list=path.join(root,'unlock.json');fs.writeFileSync(list,'[]');const unlockArgs=['protection','set',p,list,'--revision',String(restored.revision),'--request-id','explicit-fixture-unlock'];const unlocked=ok(unlockArgs).value;assert.equal(ok(['protection','status',p]).value.protected_regions.length,0);assert.deepEqual(ok(unlockArgs).value,unlocked);
   const fresh=ok(['edit-values',p,draft,'--revision',String(unlocked.revision)]).value.candidate_id;const replaced=ok(['commit-candidate',p,fresh,'--revision',String(unlocked.revision)]).value;assert.equal(point(replaced),0.8);const newPolicy=ok(['protection','status',p]).value.protected_regions;assert.ok(newPolicy.length>0);
   const historical=ok(['commit-candidate',p,c,'--revision','1','--request-id','authored-manual-commit']).value;assert.deepEqual(historical,committed);assert.deepEqual(ok(['project','open',p]).value,replaced);assert.deepEqual(ok(['protection','status',p]).value.protected_regions,newPolicy);
   await stop();start();await ready();assert.deepEqual(ok(['project','open',p]).value,replaced);assert.deepEqual(ok(['protection','status',p]).value.protected_regions,newPolicy);assert.equal(hash(file),originalHash);
   const q=ok(['project','create','Authored protection fragmentation refusal']).value.project_id;
   const fragmented=path.join(root,'fragmented.funscript');fs.writeFileSync(fragmented,JSON.stringify({actions:Array.from({length:2303},(_,i)=>({at:i*100,pos:40}))}));
   const qc=ok(['import-script',fragmented,'--project',q]).value.candidate_id;ok(['commit-candidate',q,qc,'--revision','0']);const qBefore=ok(['project','open',q]).value;
   const many={tracks:qBefore.program.tracks.map(track=>({axis:track.axis,actions:track.actions.map((a,i)=>({time:a.time,position:i%2?0.8:a.position})),gaps:track.gaps}))};fs.writeFileSync(draft,JSON.stringify(many));const manyCandidate=ok(['edit-values',q,draft,'--revision','1']).value.candidate_id;
   const overflow=cli(['commit-candidate',q,manyCandidate,'--revision','1']);assert.notEqual(overflow.status,0);assert.match(overflow.stderr,/ResourceExhausted/);assert.deepEqual(ok(['project','open',q]).value,qBefore);assert.equal(ok(['protection','status',q]).value.protected_regions.length,0);
   if(process.env.PULSAR_MANUAL_PROTECTION_EVIDENCE){fs.mkdirSync(process.env.PULSAR_MANUAL_PROTECTION_EVIDENCE,{recursive:true});fs.writeFileSync(path.join(process.env.PULSAR_MANUAL_PROTECTION_EVIDENCE,'authored-cli.json'),JSON.stringify({initial:before,manual:committed,automatic_protection:policy,regenerated,replaced,replacement_protection:newPolicy,fragmentation_refused_without_revision_or_motion_change:true,source_sha256:originalHash,original_preserved:true,actual_restart:true},null,2)+'\n',{flag:'wx'});}
  } finally {await stop();}
 });
