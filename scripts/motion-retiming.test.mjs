import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import net from 'node:net';
import crypto from 'node:crypto';
import {spawn} from 'node:child_process';
import {setTimeout as delay} from 'node:timers/promises';

test('native explicit motion retiming preserves values, replays and restores across restart',
  {skip:!process.env.PULSAR_STASH_TEST_BINARY,timeout:90000},async t=>{
    const root=fs.mkdtempSync(path.join(os.tmpdir(),'pulsar-retiming-'));fs.chmodSync(root,0o700);
    t.diagnostic(`retained authored retiming fixture: ${root}`);
    const binary=fs.realpathSync(process.env.PULSAR_STASH_TEST_BINARY),env={PATH:'/usr/bin:/bin',PULSAR_STATE_DIR:path.join(root,'state')};
    let engine,exit,stderr='';
    const start=(executable=binary)=>{engine=spawn(executable,['engine'],{env,stdio:['ignore','ignore','pipe']});engine.stderr.on('data',b=>{stderr=(stderr+b).slice(-65536);});exit=new Promise(resolve=>engine.once('exit',resolve));};
    const cli=(args,executable=binary)=>new Promise((resolve,reject)=>{const p=spawn(executable,args,{env,stdio:['ignore','pipe','pipe']});let out='',err='';const timer=setTimeout(()=>p.kill('SIGKILL'),15000);p.stdout.on('data',b=>{out=(out+b).slice(-1024*1024);});p.stderr.on('data',b=>{err=(err+b).slice(-65536);});p.once('error',e=>{clearTimeout(timer);reject(e);});p.once('exit',code=>{clearTimeout(timer);resolve({code,out,err});});});
    const ready=async()=>{const deadline=Date.now()+15000;while(true){assert.equal(engine.exitCode,null,stderr);assert.ok(Date.now()<deadline,'engine startup deadline');if(fs.existsSync(path.join(env.PULSAR_STATE_DIR,'engine.sock'))&&(await cli(['capabilities'])).code===0)return;await delay(25);}};
    const stop=async()=>{if(engine?.exitCode===null && engine.signalCode===null){engine.kill('SIGTERM');const timer=setTimeout(()=>engine.kill('SIGKILL'),2000);try{await exit;}finally{clearTimeout(timer);}}};
    const success=async args=>{const r=await cli(args);assert.equal(r.code,0,r.err+r.out);return r;};
    const ok=async args=>JSON.parse((await success(args)).out);
    let sequence=0;
    const rpc=(project,revision,operation,args,overrideCredentials)=>new Promise((resolve,reject)=>{
      const credentials=overrideCredentials??JSON.parse(fs.readFileSync(path.join(env.PULSAR_STATE_DIR,'first-party.session.json')));
      const bytes=Buffer.from(JSON.stringify({version:2,request_id:`retiming-fixture-${++sequence}`,...credentials,project,expected_revision:revision,command:{operation,...(args===undefined?{}:{arguments:args})}}));const header=Buffer.alloc(4);header.writeUInt32BE(bytes.length);
      const socket=net.createConnection(path.join(env.PULSAR_STATE_DIR,'engine.sock'));let received=Buffer.alloc(0);socket.setTimeout(10000,()=>socket.destroy(new Error('IPC deadline')));socket.once('error',reject);socket.once('close',()=>reject(new Error(`IPC closed before ${operation} response`)));socket.once('connect',()=>socket.write(Buffer.concat([header,bytes])));
      socket.on('data',b=>{received=Buffer.concat([received,b]);if(received.length<4)return;const size=received.readUInt32BE();if(size>1024*1024){socket.destroy(new Error('IPC response admission'));return;}if(received.length>=size+4){resolve(JSON.parse(received.subarray(4,size+4)).result);socket.destroy();}});
    });
    try {
      start();await ready();const p=(await ok(['project','create','Authored retiming fixture'])).value.project_id;
      const file=path.join(root,'authored.funscript');fs.writeFileSync(file,JSON.stringify({actions:[{at:0,pos:20},{at:1000,pos:80},{at:2000,pos:40}]}));
      const candidate=(await ok(['import-script',file,'--project',p])).value.candidate_id;assert.ok((await rpc(p,0,'commit_candidate',{candidate_id:candidate})).Ok);
      const media=path.join(root,'inert-source');fs.writeFileSync(media,'Authored inert source fixture, never decoded');const capture=await rpc(p,null,'import_source',{path:media});const source=capture.Ok.value.source_version;
      const spec={source_version:source,source_start:{numerator:0,denominator:1},source_end:{numerator:10,denominator:1},source_anchor:{numerator:0,denominator:1},project_anchor:0,rate_numerator:1,rate_denominator:1};
      const specFile=path.join(root,'placement.json');fs.writeFileSync(specFile,JSON.stringify(spec));const placed=(await ok(['project','place',p,'--spec',specFile])).value;const from=placed.placements[0].placement_id;
      const original=placed.program;spec.rate_numerator=-2;spec.project_anchor=3000000000;fs.writeFileSync(specFile,JSON.stringify(spec));const replaced=(await ok(['project','place',p,'--spec',specFile,'--replace',from])).value;const to=replaced.placements[0].placement_id;
      assert.deepEqual(replaced.program,original);
      const args=['project','retime',p,from,to,'--axes','stroke','--revision','3','--request-id','authored-retime-request'];const changed=(await ok(args)).value;
      const actions=changed.program.tracks[0].actions;assert.deepEqual(actions.map(a=>a.time),[2000000000,2500000000,3000000000]);assert.deepEqual(actions.map(a=>a.position),[0.4,0.8,0.2]);assert.ok(actions.every(a=>a.evidence==='synthesized'));
      assert.deepEqual((await ok(args)).value,changed);const stale=await cli(args.map(a=>a==='authored-retime-request'?'stale-retime-request':a));assert.notEqual(stale.code,0);assert.match(stale.err,/RevisionConflict/);
      const receipt=(await ok(['project','retime-status',p,'--revision','4'])).value;
      assert.equal(receipt.schema_version,1);assert.equal(receipt.revision,4);assert.equal(receipt.base_revision,3);assert.equal(receipt.from.placement_id,from);assert.equal(receipt.to.placement_id,to);assert.deepEqual(receipt.axes,['stroke']);assert.equal(receipt.after.sha256,changed.motion.program.sha256);assert.equal(receipt.before.sha256,replaced.motion.program.sha256);
      const reviews=(await ok(['review-status',p])).value;assert.ok(reviews.entries.some(e=>e.flag.reason==='existing_motion_explicitly_retimed_review_required'));
      assert.ok((await rpc(p,4,'undo')).Ok);assert.deepEqual((await ok(['project','open',p])).value.program,original);
      assert.ok((await rpc(p,5,'redo')).Ok);assert.deepEqual((await ok(['project','open',p])).value.program,changed.program);
      await stop();
      if(process.env.PULSAR_SCHEMA5_TEST_BINARY){
        const old=fs.realpathSync(process.env.PULSAR_SCHEMA5_TEST_BINARY);assert.equal(crypto.createHash('sha256').update(fs.readFileSync(old)).digest('hex'),'77092989211a3158afd69cb76869104f7d3a771afeb05119f03c38e995b5a48d');
        const db=path.join(env.PULSAR_STATE_DIR,'projects.sqlite3');const checkpoint=await cli(['-c',"import sqlite3,sys; d=sqlite3.connect(sys.argv[1]); row=d.execute('PRAGMA wal_checkpoint(TRUNCATE)').fetchone(); d.close(); assert row[0]==0, row",db],'/usr/bin/python3');assert.equal(checkpoint.code,0,checkpoint.err);const beforeHash=crypto.createHash('sha256').update(fs.readFileSync(db)).digest('hex');
        const refused=await cli(['engine'],old);assert.notEqual(refused.code,0);assert.match(refused.err,/schema requires an explicit recoverable migration/);assert.equal(crypto.createHash('sha256').update(fs.readFileSync(db)).digest('hex'),beforeHash);
      }
      start();await ready();assert.deepEqual((await ok(['project','open',p])).value.program,changed.program);assert.deepEqual((await ok(args)).value,changed);assert.deepEqual((await ok(['project','retime-status',p,'--revision','4'])).value,receipt);
      await stop();
      const altered=await cli(['-c',`import sqlite3,json,hashlib,sys
d=sqlite3.connect(sys.argv[1]);row=d.execute('SELECT receipt,sha256 FROM motion_retime_receipts WHERE project=? AND revision=4',(sys.argv[2],)).fetchone();v=json.loads(row[0]);v['actor']='forged-fixture-actor';raw=json.dumps(v,separators=(',',':'));d.execute('UPDATE motion_retime_receipts SET receipt=?,sha256=? WHERE project=? AND revision=4',(raw,hashlib.sha256(raw.encode()).hexdigest(),sys.argv[2]));d.commit();print(json.dumps(row))`,path.join(env.PULSAR_STATE_DIR,'projects.sqlite3'),p],'/usr/bin/python3');assert.equal(altered.code,0,altered.err);
      start();await ready();const forged=await cli(['project','retime-status',p,'--revision','4']);assert.notEqual(forged.code,0);assert.match(forged.err,/DependencyMismatch/);await stop();
      const restored=await cli(['-c',`import sqlite3,json,sys
d=sqlite3.connect(sys.argv[1]);r=json.loads(sys.argv[3]);d.execute('UPDATE motion_retime_receipts SET receipt=?,sha256=? WHERE project=? AND revision=4',(r[0],r[1],sys.argv[2]));d.commit()`,path.join(env.PULSAR_STATE_DIR,'projects.sqlite3'),p,altered.out.trim()],'/usr/bin/python3');assert.equal(restored.code,0,restored.err);start();await ready();assert.deepEqual((await ok(['project','retime-status',p,'--revision','4'])).value,receipt);

      // Structured facts travel in their original namespace, never as local
      // authenticated edits. Successive clone archives retain the exact original.
      await success(['allow-packaging',p,'--acknowledge-unencrypted-private-data']);
      const packageFile=path.join(root,'retimed.pulsar');
      await success(['package-export',p,packageFile,'--acknowledge-unencrypted-private-data']);
      const readPackage=file=>{const data=fs.readFileSync(file);assert.deepEqual(data.subarray(0,8),Buffer.from('PULSPKG\0'));const length=Number(data.readBigUInt64LE(12));const manifest=JSON.parse(data.subarray(52,52+length));let offset=52+length;const objects=new Map();for(const o of manifest.objects){objects.set(o.sha256,data.subarray(offset,offset+o.byte_len));offset+=o.byte_len;}assert.equal(offset,data.length);return {manifest,objects,version:data.readUInt16LE(8)};};
      const exported=readPackage(packageFile);assert.equal(exported.version,5);assert.deepEqual(exported.manifest.retiming_receipts,[receipt]);
      if(process.env.PULSAR_FORMAT4_TEST_BINARY){
        const old=fs.realpathSync(process.env.PULSAR_FORMAT4_TEST_BINARY);assert.equal(crypto.createHash('sha256').update(fs.readFileSync(old)).digest('hex'),'cde5d9eea7482a39976b1706b0802f4ba850cc720e01704d4dc4d321b70a7185');
        const refused=await cli(['package-import',packageFile],old);assert.notEqual(refused.code,0);assert.match(refused.err+refused.out,/unsupported.*(?:format|version)/i);await stop();const oldOpen=await cli(['engine'],old);assert.notEqual(oldOpen.code,0);assert.match(oldOpen.err,/(unsupported package format|schema requires an explicit recoverable migration)/i);start();await ready();assert.deepEqual((await ok(['project','retime-status',p,'--revision','4'])).value,receipt);
      }
      let input=packageFile;
      for(let generation=0;generation<2;generation++){
        const imported=await ok(['package-import',input]);assert.equal(imported.state,'completed');const clone=imported.receipt.project_id;
        const snapshot=(await ok(['project','open',clone])).value;assert.deepEqual(snapshot.program,changed.program);
        const missing=await cli(['project','retime-status',clone,'--revision','4']);assert.notEqual(missing.code,0);assert.match(missing.err,/NotFound/);
        const archivalArgs=['project','archived-retime-status',clone,'--revision','4'];
        const archival=(await ok(archivalArgs)).value;assert.equal(archival.historical_attribution_only,true);assert.equal(archival.project_id,clone);assert.equal(archival.revision,4);assert.equal(archival.base_revision,3);assert.deepEqual(archival.origin_receipt,receipt);assert.notEqual(archival.from.placement_id,from);assert.notEqual(archival.to.placement_id,to);assert.deepEqual(archival.from.spec,receipt.from.spec);assert.deepEqual(archival.to.spec,receipt.to.spec);assert.match(archival.local_actor,/^archive:/);
        if(generation===0){
          const paired=await rpc(null,null,'pair',{client_name:'archival Read fixture',pairing_token:fs.readFileSync(path.join(env.PULSAR_STATE_DIR,'pairing.token'),'utf8').trim()});assert.ok(paired.Ok);const reader=paired.Ok.value;
          assert.equal((await rpc(clone,null,'archived_motion_retime_status',{revision:4},reader)).Err.code,'forbidden');assert.ok((await rpc(clone,null,'grant',{session:reader.session,project_id:clone,scopes:['read']})).Ok);
          assert.deepEqual((await rpc(clone,null,'archived_motion_retime_status',{revision:4},reader)).Ok.value,archival);assert.ok((await rpc(clone,null,'revoke',{session:reader.session,project_id:clone})).Ok);assert.equal((await rpc(clone,null,'archived_motion_retime_status',{revision:4},reader)).Err.code,'forbidden');
          const originPath=path.join(env.PULSAR_STATE_DIR,'imported-project-evidence',archival.origin_manifest.sha256);const originBytes=fs.readFileSync(originPath);fs.chmodSync(originPath,0o600);fs.writeFileSync(originPath,'tampered manifest');fs.chmodSync(originPath,0o400);
          const corrupt=await cli(archivalArgs);assert.notEqual(corrupt.code,0);assert.match(corrupt.err,/DependencyMismatch/);fs.chmodSync(originPath,0o600);fs.writeFileSync(originPath,originBytes);fs.chmodSync(originPath,0o400);assert.deepEqual((await ok(archivalArgs)).value,archival);
          const external=path.join(root,'outside-manifest');fs.writeFileSync(external,originBytes);const saved=`${originPath}.fixture-save`;fs.renameSync(originPath,saved);fs.symlinkSync(external,originPath);const linked=await cli(archivalArgs);assert.notEqual(linked.code,0);assert.deepEqual(fs.readFileSync(external),originBytes);fs.unlinkSync(originPath);fs.renameSync(saved,originPath);
          const db=path.join(env.PULSAR_STATE_DIR,'projects.sqlite3');const oversized=await cli(['-c',"import sqlite3,sys; d=sqlite3.connect(sys.argv[1]); raw=d.execute('SELECT manifest FROM project_import_archives WHERE project=?',(sys.argv[2],)).fetchone()[0]; d.execute('UPDATE project_import_archives SET manifest=? WHERE project=?',('x'*16777217,sys.argv[2])); d.commit(); print(raw)",db,clone],'/usr/bin/python3');assert.equal(oversized.code,0,oversized.err);const rejected=await cli(archivalArgs);assert.notEqual(rejected.code,0);assert.match(rejected.err,/DependencyMismatch/);const restored=await cli(['-c',"import sqlite3,sys; d=sqlite3.connect(sys.argv[1]); d.execute('UPDATE project_import_archives SET manifest=? WHERE project=?',(sys.argv[3],sys.argv[2])); d.commit()",db,clone,oversized.out.trim()],'/usr/bin/python3');assert.equal(restored.code,0,restored.err);
          const forgedActor=await cli(['-c',"import sqlite3,sys; d=sqlite3.connect(sys.argv[1]); d.execute('UPDATE revisions SET actor=? WHERE project=? AND revision=4',('x'*257,sys.argv[2])); d.commit()",db,clone],'/usr/bin/python3');assert.equal(forgedActor.code,0,forgedActor.err);const wrongActor=await cli(archivalArgs);assert.notEqual(wrongActor.code,0);assert.match(wrongActor.err,/DependencyMismatch/);const actorRestored=await cli(['-c',"import sqlite3,sys; d=sqlite3.connect(sys.argv[1]); d.execute('UPDATE revisions SET actor=? WHERE project=? AND revision=4',(sys.argv[3],sys.argv[2])); d.commit()",db,clone,archival.local_actor],'/usr/bin/python3');assert.equal(actorRestored.code,0,actorRestored.err);
        }
        await stop();start();await ready();assert.deepEqual((await ok(archivalArgs)).value,archival);assert.deepEqual((await ok(['project','open',clone])).value.motion,snapshot.motion);

        await success(['allow-packaging',clone,'--acknowledge-unencrypted-private-data']);input=path.join(root,`clone-${generation}.pulsar`);await success(['package-export',clone,input,'--acknowledge-unencrypted-private-data']);
        const cloned=readPackage(input);assert.equal(cloned.version,5);assert.equal(cloned.manifest.retiming_receipts,undefined);
        const original=cloned.manifest.imported_origins.find(o=>o.container.project_id===p);assert.ok(original);const originalBytes=cloned.objects.get(original.manifest.sha256);assert.ok(originalBytes);assert.equal(crypto.createHash('sha256').update(originalBytes).digest('hex'),original.manifest.sha256);assert.deepEqual(JSON.parse(originalBytes).retiming_receipts,[receipt]);
      }

      await stop();
      // An unknown schema committed only in WAL must not trigger a checkpoint.
      const dbFile=path.join(env.PULSAR_STATE_DIR,'projects.sqlite3');
      const writer=spawn('/usr/bin/python3',['-c',"import sqlite3,sys,time; d=sqlite3.connect(sys.argv[1]); d.execute('PRAGMA wal_autocheckpoint=0'); d.execute('PRAGMA user_version=21'); d.commit(); print('committed',flush=True); time.sleep(60)",dbFile],{env,stdio:['ignore','pipe','pipe']});
      const writerExit=new Promise(resolve=>writer.once('exit',resolve));let writerError='';writer.stderr.on('data',b=>writerError+=b);
      try {await new Promise((resolve,reject)=>{const timer=setTimeout(()=>reject(new Error('WAL fixture commit deadline')),5000);writer.stdout.once('data',b=>{clearTimeout(timer);assert.match(b.toString(),/committed/);resolve();});writer.once('exit',()=>{clearTimeout(timer);reject(new Error(writerError));});});} finally {writer.kill('SIGKILL');await writerExit;}
      const digest=name=>crypto.createHash('sha256').update(fs.readFileSync(name)).digest('hex');const files=[dbFile,`${dbFile}-wal`];const before=files.map(digest);const future=await cli(['engine']);assert.notEqual(future.code,0);assert.match(future.err,/schema requires an explicit recoverable migration/);assert.deepEqual(files.map(digest),before);
    } finally {await stop();}
  });
