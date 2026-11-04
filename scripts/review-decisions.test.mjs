import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import net from 'node:net';
import crypto from 'node:crypto';
import {spawn} from 'node:child_process';
import {setTimeout as delay} from 'node:timers/promises';

test('native CLI review declarations retain evidence and support exact replay, reopen and history',
  {skip:!process.env.PULSAR_STASH_TEST_BINARY,timeout:90000},async t=>{
    const root=fs.mkdtempSync(path.join(os.tmpdir(),'pulsar-review-'));fs.chmodSync(root,0o700);
    t.diagnostic(`retained synthetic review fixture: ${root}`);
    const binary=fs.realpathSync(process.env.PULSAR_STASH_TEST_BINARY),env={PATH:'/usr/bin:/bin',PULSAR_STATE_DIR:path.join(root,'state')};
    let engine,exit,stderr='';
    const start=()=>{engine=spawn(binary,['engine'],{env,stdio:['ignore','ignore','pipe']});engine.stderr.on('data',b=>{stderr=(stderr+b).slice(-65536);});exit=new Promise(resolve=>engine.once('exit',resolve));};
    const cli=(args,executable=binary)=>new Promise((resolve,reject)=>{const p=spawn(executable,args,{env,stdio:['ignore','pipe','pipe']});let out='',err='';const timer=setTimeout(()=>p.kill('SIGKILL'),15000);p.stdout.on('data',b=>{out=(out+b).slice(-1024*1024);});p.stderr.on('data',b=>{err=(err+b).slice(-65536);});p.once('error',e=>{clearTimeout(timer);reject(e);});p.once('exit',code=>{clearTimeout(timer);resolve({code,out,err});});});
    const ready=async()=>{const deadline=Date.now()+15000;while(true){assert.equal(engine.exitCode,null,stderr);assert.ok(Date.now()<deadline,'engine startup deadline');if(fs.existsSync(path.join(env.PULSAR_STATE_DIR,'engine.sock'))&&(await cli(['capabilities'])).code===0)return;await delay(25);}};
    const stop=async()=>{if(engine?.exitCode===null && engine.signalCode===null){engine.kill('SIGTERM');const timer=setTimeout(()=>engine.kill('SIGKILL'),2000);try{await exit;}finally{clearTimeout(timer);}}};
    const ok=async args=>{const r=await cli(args);assert.equal(r.code,0,r.err+r.out);return JSON.parse(r.out);};
    let sequence=0;
    const rpc=(project,revision,operation,args)=>new Promise((resolve,reject)=>{
      const credentials=JSON.parse(fs.readFileSync(path.join(env.PULSAR_STATE_DIR,'first-party.session.json')));
      const bytes=Buffer.from(JSON.stringify({version:2,request_id:`review-fixture-${++sequence}`,...credentials,project,expected_revision:revision,command:{operation,...(args===undefined?{}:{arguments:args})}}));const header=Buffer.alloc(4);header.writeUInt32BE(bytes.length);
      const socket=net.createConnection(path.join(env.PULSAR_STATE_DIR,'engine.sock'));let received=Buffer.alloc(0);socket.setTimeout(10000,()=>socket.destroy(new Error('IPC deadline')));socket.once('error',reject);socket.once('close',()=>reject(new Error(`IPC closed before ${operation} response`)));socket.once('connect',()=>socket.write(Buffer.concat([header,bytes])));
      socket.on('data',b=>{received=Buffer.concat([received,b]);if(received.length<4)return;const size=received.readUInt32BE();if(size>1024*1024){socket.destroy(new Error('IPC response admission'));return;}if(received.length>=size+4){resolve(JSON.parse(received.subarray(4,size+4)).result);socket.destroy();}});
    });
    try {
      start();await ready();const p=(await ok(['project','create','Synthetic review declarations'])).value.project_id;
      const file=path.join(root,'authored.funscript');fs.writeFileSync(file,JSON.stringify({actions:[{at:0,pos:20},{at:1000,pos:80}]}));
      const candidate=(await ok(['import-script',file,'--project',p])).value.candidate_id;
      assert.ok((await rpc(p,0,'commit_candidate',{candidate_id:candidate})).Ok);
      const before=(await ok(['project','open',p])).value;
      const initial=(await ok(['review-status',p])).value,entry=initial.entries.find(e=>e.flag.reason==='imported_motion');assert.ok(entry);assert.equal(entry.decision,null);
      const args=['review-disposition',p,entry.review_id,'--disposition','resolved','--note','Synthetic fixture declaration only','--revision','1','--request-id','review-resolution-fixture'];
      const resolved=(await ok(args)).value;assert.equal(resolved.revision,2);assert.deepEqual(resolved.motion.program,before.motion.program);
      const status=async()=>(await ok(['review-status',p])).value;
      const reviewed=await status();assert.deepEqual(reviewed.entries[0].flag,initial.entries[0].flag);assert.equal(reviewed.entries[0].decision.disposition,'resolved');assert.equal(reviewed.entries[0].decision.base_revision,1);assert.equal(reviewed.entries[0].decision.recorded_revision,2);assert.ok(reviewed.entries[0].decision.actor);assert.ok(reviewed.entries[0].decision.recorded_at_unix_ms>0);
      assert.equal((await ok(['review',p])).value.issues.some(i=>i.code==='imported_motion'),false);
      assert.deepEqual((await ok(args)).value,resolved);assert.equal((await status()).revision,2);
      const changed=await cli(args.map(a=>a==='Synthetic fixture declaration only'?'Changed rationale':a));assert.notEqual(changed.code,0);assert.match(changed.err,/RequestConflict/);
      const stale=await cli(args.map(a=>a==='review-resolution-fixture'?'stale-review-fixture':a));assert.notEqual(stale.code,0);assert.match(stale.err,/RevisionConflict/);
      const reopen=['review-disposition',p,entry.review_id,'--disposition','reopened','--note','Synthetic fixture reopened','--revision','2','--request-id','review-reopen-fixture'];assert.equal((await ok(reopen)).value.revision,3);
      assert.equal((await ok(['review',p])).value.issues.some(i=>i.code==='imported_motion'),true);
      assert.ok((await rpc(p,3,'undo')).Ok);assert.equal((await status()).entries[0].decision.disposition,'resolved');
      assert.ok((await rpc(p,4,'redo')).Ok);assert.equal((await status()).entries[0].decision.disposition,'reopened');
      const retained=await status();await stop();
      if(process.env.PULSAR_SCHEMA4_TEST_BINARY){const old=await cli(['engine'],fs.realpathSync(process.env.PULSAR_SCHEMA4_TEST_BINARY));assert.notEqual(old.code,0);assert.match(old.err,/schema requires an explicit recoverable migration/);}
      start();await ready();assert.deepEqual(await status(),retained);assert.deepEqual((await ok(args)).value,resolved);assert.equal((await status()).revision,5);assert.deepEqual((await ok(['project','open',p])).value.motion.program,before.motion.program);
      await stop();
      const altered=await cli(['-c',`import sqlite3,json,sys
d=sqlite3.connect(sys.argv[1]);raw=d.execute("SELECT review_state FROM revision_lineage WHERE project=? AND revision=5",(sys.argv[2],)).fetchone()[0];v=json.loads(raw);v['decisions'][0]['actor']='forged-fixture-actor';d.execute("UPDATE revision_lineage SET review_state=? WHERE project=? AND revision=5",(json.dumps(v),sys.argv[2]));d.commit();print(raw)`,path.join(env.PULSAR_STATE_DIR,'projects.sqlite3'),p],'/usr/bin/python3');assert.equal(altered.code,0,altered.err);
      start();await ready();const bad=await cli(['review-status',p]);assert.notEqual(bad.code,0);assert.match(bad.err,/DependencyMismatch/);
      await stop();const restored=await cli(['-c',`import sqlite3,sys
d=sqlite3.connect(sys.argv[1]);d.execute("UPDATE revision_lineage SET review_state=? WHERE project=? AND revision=5",(sys.argv[3],sys.argv[2]));d.commit()`,path.join(env.PULSAR_STATE_DIR,'projects.sqlite3'),p,altered.out.trim()],'/usr/bin/python3');assert.equal(restored.code,0,restored.err);start();await ready();assert.deepEqual(await status(),retained);

      const q=(await ok(['project','create','Synthetic interval review'])).value.project_id;
      const qc=(await ok(['import-script',file,'--project',q])).value.candidate_id;
      assert.ok((await rpc(q,0,'commit_candidate',{candidate_id:qc})).Ok);
      const original=(await ok(['review-status',q])).value.entries.find(e=>e.flag.reason==='imported_motion');
      const interval=(start,end,disposition,revision,id)=>['review-disposition',q,original.review_id,'--start-ns',String(start),'--end-ns',String(end),'--disposition',disposition,'--note','Synthetic interval declaration','--revision',String(revision),'--request-id',id];
      const partialArgs=interval(200000000,800000000,'resolved',1,'partial-resolve');
      const partial=(await ok(partialArgs)).value;assert.equal(partial.revision,2);
      assert.deepEqual((await ok(partialArgs)).value,partial);
      const conflict=await cli(interval(200000001,800000000,'resolved',1,'partial-resolve'));assert.notEqual(conflict.code,0);assert.match(conflict.err,/RequestConflict/);
      const qstatus=async()=>(await ok(['review-status',q])).value;
      const qs=await qstatus();assert.deepEqual(qs.entries[0].flag,original.flag);assert.equal(qs.entries[0].decision,null);assert.deepEqual(qs.entries[0].decision_history[0].range,{start:200000000,end:800000000});
      const warnings=async()=>(await ok(['review',q])).value.issues.filter(i=>i.code==='imported_motion').map(i=>i.range);
      assert.deepEqual(await warnings(),[{start:original.flag.range.start,end:200000000},{start:800000000,end:original.flag.range.end}]);
      const outside=await cli(interval(-1,800000000,'resolved',2,'outside-flag'));assert.notEqual(outside.code,0);assert.equal((await qstatus()).revision,2);
      assert.equal((await ok(interval(400000000,600000000,'reopened',2,'partial-reopen'))).value.revision,3);
      const expected=[{start:original.flag.range.start,end:200000000},{start:400000000,end:600000000},{start:800000000,end:original.flag.range.end}];assert.deepEqual(await warnings(),expected);
      assert.ok((await rpc(q,3,'undo')).Ok);assert.equal((await qstatus()).entries[0].decision_history.length,1);
      assert.ok((await rpc(q,4,'redo')).Ok);assert.deepEqual(await warnings(),expected);
      const qretained=await qstatus();await stop();
      const database=path.join(env.PULSAR_STATE_DIR,'projects.sqlite3');
      const changedRange=await cli(['-c',`import sqlite3,json,sys\nd=sqlite3.connect(sys.argv[1]);raw=d.execute('SELECT review_state FROM revision_lineage WHERE project=? AND revision=5',(sys.argv[2],)).fetchone()[0];v=json.loads(raw);v['decisions'][0]['range']['start']+=1;d.execute('UPDATE revision_lineage SET review_state=? WHERE project=? AND revision=5',(json.dumps(v),sys.argv[2]));d.commit();print(raw)`,database,q],'/usr/bin/python3');assert.equal(changedRange.code,0,changedRange.err);
      start();await ready();const damaged=await cli(['review-status',q]);assert.notEqual(damaged.code,0);assert.match(damaged.err,/DependencyMismatch/);await stop();
      const repair=await cli(['-c',`import sqlite3,sys\nd=sqlite3.connect(sys.argv[1]);d.execute('UPDATE revision_lineage SET review_state=? WHERE project=? AND revision=5',(sys.argv[3],sys.argv[2]));d.commit()`,database,q,changedRange.out.trim()],'/usr/bin/python3');assert.equal(repair.code,0,repair.err);
      if(process.env.PULSAR_SCHEMA6_TEST_BINARY){const old=fs.realpathSync(process.env.PULSAR_SCHEMA6_TEST_BINARY);assert.equal(crypto.createHash('sha256').update(fs.readFileSync(old)).digest('hex'),'4745315c0652997baf44667e8f7d2e209cf36a23f6c95b5ad25356e3a02bee31');const refused=await cli(['engine'],old);assert.notEqual(refused.code,0);assert.match(refused.err,/schema requires an explicit recoverable migration/);}
      start();await ready();assert.deepEqual(await qstatus(),qretained);
      await stop();const trigger=await cli(['-c',`import sqlite3,sys\nd=sqlite3.connect(sys.argv[1]);d.execute("CREATE TRIGGER fail_interval_receipt BEFORE INSERT ON review_interval_receipts BEGIN SELECT RAISE(ABORT,'fixture interval failure'); END");d.commit()`,database],'/usr/bin/python3');assert.equal(trigger.code,0,trigger.err);
      start();await ready();const rolledBack=await cli(interval(200000000,800000000,'resolved',5,'receipt-rollback'));assert.notEqual(rolledBack.code,0);assert.deepEqual(await qstatus(),qretained);await stop();
      const drop=await cli(['-c',`import sqlite3,sys\nd=sqlite3.connect(sys.argv[1]);d.execute('DROP TRIGGER fail_interval_receipt');d.commit()`,database],'/usr/bin/python3');assert.equal(drop.code,0,drop.err);start();await ready();
      const success=async args=>{const result=await cli(args);assert.equal(result.code,0,result.err+result.out);};
      await success(['allow-packaging',q,'--acknowledge-unencrypted-private-data']);const packageFile=path.join(root,'interval-review.pulsar');await success(['package-export',q,packageFile,'--acknowledge-unencrypted-private-data']);
      const bytes=fs.readFileSync(packageFile);assert.equal(bytes.readUInt16LE(8),6);const manifest=JSON.parse(bytes.subarray(52,52+Number(bytes.readBigUInt64LE(12))));assert.ok(manifest.revision_lineage.some(r=>r.review.decisions?.some(d=>d.range?.start===200000000)));
      if(process.env.PULSAR_SCHEMA6_TEST_BINARY){const refused=await cli(['package-import',packageFile],fs.realpathSync(process.env.PULSAR_SCHEMA6_TEST_BINARY));assert.notEqual(refused.code,0);assert.match(refused.err+refused.out,/unsupported.*(?:format|version)/i);}
      const cloned=await ok(['package-import',packageFile]);assert.equal(cloned.state,'completed');const cloneStatus=(await ok(['review-status',cloned.receipt.project_id])).value;assert.ok(cloneStatus.entries.every(e=>e.decision===null && !e.decision_history?.length));
      await success(['allow-packaging',cloned.receipt.project_id,'--acknowledge-unencrypted-private-data']);const cloneFile=path.join(root,'interval-clone.pulsar');await success(['package-export',cloned.receipt.project_id,cloneFile,'--acknowledge-unencrypted-private-data']);const cloneBytes=fs.readFileSync(cloneFile);assert.equal(cloneBytes.readUInt16LE(8),6);const cloneAgain=await ok(['package-import',cloneFile]);assert.equal(cloneAgain.state,'completed');assert.ok((await ok(['review-status',cloneAgain.receipt.project_id])).value.entries.every(e=>e.decision===null && !e.decision_history?.length));
    } finally {await stop();}
  });
