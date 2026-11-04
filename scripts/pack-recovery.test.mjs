// Authored native process recovery; no desktop window or hardware is exercised.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import crypto from 'node:crypto';
import {spawn,spawnSync} from 'node:child_process';
import {once} from 'node:events';
import {setTimeout as delay} from 'node:timers/promises';
const sha=b=>crypto.createHash('sha256').update(b).digest('hex');
test('persistent desktop pack recovery survives actual lost acknowledgement and restart without rearming selection',
 {skip:!process.env.PULSAR_BENCHMARK_WORKER||!process.env.PULSAR_PACK_RECOVERY_FIXTURE,timeout:60000},async t=>{
 const binary=fs.realpathSync(process.env.PULSAR_BENCHMARK_WORKER),fixture=fs.realpathSync(process.env.PULSAR_PACK_RECOVERY_FIXTURE);
 const root=fs.mkdtempSync(path.join(os.tmpdir(),'pulsar-pack-recovery-'));fs.chmodSync(root,0o700);const state=path.join(root,'state'),env={PATH:'/usr/bin:/bin',PULSAR_STATE_DIR:state};let engine,exited,index=0;
 const run=(exe,args)=>{const r=spawnSync(exe,args,{env,encoding:'utf8',timeout:15000,maxBuffer:2*1024*1024});assert.equal(r.error,undefined);fs.writeFileSync(path.join(root,`command-${index++}.json`),JSON.stringify({exe,args,status:r.status,stdout:r.stdout,stderr:r.stderr}),{mode:0o600});return r;};
 const ok=(exe,args)=>{const r=run(exe,args);assert.equal(r.status,0,r.stderr);return JSON.parse(r.stdout);};
 const stop=async()=>{if(engine?.exitCode===null&&engine.signalCode===null){engine.kill('SIGTERM');const timer=setTimeout(()=>engine.kill('SIGKILL'),2000);try{await exited;}finally{clearTimeout(timer);}}};t.after(stop);
 const start=async()=>{const fd=fs.openSync(path.join(root,`engine-${index++}.log`),'wx',0o600);engine=spawn(binary,['engine'],{env,stdio:['ignore','ignore',fd]});fs.closeSync(fd);exited=once(engine,'exit');const end=Date.now()+15000;while(true){assert.equal(engine.exitCode,null);assert.ok(Date.now()<end);if(fs.existsSync(path.join(state,'engine.sock'))&&run(binary,['capabilities']).status===0)return;await delay(25);}};
 await start();ok(fixture,['stage',state,'retained-native-id']);const entry=ok(fixture,['list',state]);assert.equal(entry.length,1);assert.equal(entry[0].acknowledged,false);
 assert.notEqual(run(fixture,['dismiss',state,'retained-native-id']).status,0);assert.equal(ok(binary,['model','pack-selection-status']).value.selection_revision,0);
 const receipt=ok(fixture,['lost-ack',state,'retained-native-id']);assert.equal(receipt.selection_revision,1);assert.equal(ok(fixture,['list',state])[0].acknowledged,false);
 await stop();await start();assert.equal(ok(fixture,['list',state])[0].id,'retained-native-id');assert.equal(ok(binary,['model','pack-selection-status']).value.selection_revision,1);
 const later=ok(binary,['model','pack-deactivate','--expected-selection-revision','1']);assert.equal(later.value.selection_revision,2);
 const historical=ok(fixture,['replay',state,'retained-native-id']);assert.deepEqual(historical,receipt);assert.equal(ok(binary,['model','pack-selection-status']).value.selection_revision,2);assert.equal(ok(fixture,['list',state])[0].acknowledged,true);
 const files=fs.readdirSync(path.join(state,'desktop-pack-recovery')).filter(name=>name.endsWith('.json'));assert.equal(files.length,1);const journal=path.join(state,'desktop-pack-recovery',files[0]);const before=fs.readFileSync(journal);const credentials=JSON.parse(fs.readFileSync(path.join(state,'first-party.session.json'))),proof=fs.readFileSync(path.join(state,'model-pack-owner.token'),'utf8').trim();assert.equal(before.includes(Buffer.from(credentials.auth_token)),false);assert.equal(before.includes(Buffer.from(proof)),false);assert.equal(fs.statSync(journal).mode&0o777,0o600);
 assert.notEqual(run(fixture,['wrong-binding',state]).status,0);assert.deepEqual(fs.readFileSync(journal),before);assert.equal(ok(binary,['model','pack-selection-status']).value.selection_revision,2);
 ok(fixture,['dismiss',state,'retained-native-id']);await stop();await start();assert.deepEqual(ok(fixture,['list',state]),[]);assert.equal(ok(binary,['model','pack-selection-status']).value.selection_revision,2);
 t.diagnostic(JSON.stringify({root,state:'persistent_pack_recovery',binary_sha256:sha(fs.readFileSync(binary)),fixture_sha256:sha(fs.readFileSync(fixture)),actual_engine_receipt_unacknowledged_then_replayed:true,historical_revision:1,current_revision:2,owner_or_bearer_bytes_in_journal:false,explicit_acknowledged_dismissal:true,native_window_executed:false,physical_device_contacted:false}));
});
