// Portable logical projects must not transfer a format-4 host ownership scope.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import crypto from 'node:crypto';
import {spawn,spawnSync} from 'node:child_process';
import {authoredOnnxFixture} from './benchmark-onnx-fixture.mjs';
const worker=process.env.PULSAR_BENCHMARK_WORKER, runtime=process.env.PULSAR_BENCHMARK_ORT;
const delay=()=>new Promise(resolve=>setTimeout(resolve,10));
const hash=bytes=>crypto.createHash('sha256').update(bytes).digest('hex');
test('healthy format4 host exports and clones logical motion across independent ownership scopes', {skip:!worker||!runtime||process.platform!=='linux'}, async t=>{
  const root=fs.mkdtempSync(path.join(os.tmpdir(),'pulsar-staging-portable-'));fs.chmodSync(root,0o700);
  t.diagnostic(`retained authored format4 portable evidence: ${root}`);
  const engines=[];
  const makeHost=name=>({name,state:path.join(root,name),env:{...process.env,PULSAR_STATE_DIR:path.join(root,name),PULSAR_ORT_DYLIB:runtime}});
  const source=makeHost('source'), target=makeHost('target');let sequence=0;
  const cli=(host,args)=>{const r=spawnSync(worker,args,{env:host.env,encoding:'utf8',timeout:30000,maxBuffer:4*1024*1024});fs.writeFileSync(path.join(root,`cli-${sequence++}.json`),JSON.stringify({host:host.name,args,status:r.status,error:r.error?.message,stdout:r.stdout,stderr:r.stderr},null,2),{mode:0o600});assert.equal(r.error,undefined);assert.equal(r.status,0,r.stderr);return ['allow-packaging','package-export'].includes(args[0])?r.stdout:JSON.parse(r.stdout);};
  const ledger=host=>JSON.parse(fs.readFileSync(path.join(host.state,'worker-fence.json')));
  const start=async host=>{
    const log=fs.openSync(path.join(root,`${host.name}-${sequence++}.stderr`),'wx',0o600);
    const child=spawn(worker,['engine'],{env:host.env,stdio:['ignore','ignore',log]});fs.closeSync(log);
    const entry={child,result:null};entry.closed=new Promise(resolve=>{child.once('error',e=>{entry.result={error:e.message};resolve();});child.once('exit',(code,signal)=>{entry.result={code,signal};resolve();});});engines.push(entry);host.current=entry;
    const until=Date.now()+10000;
    while(!fs.existsSync(path.join(host.state,'engine.sock'))){assert.equal(entry.result,null);assert.ok(Date.now()<until,'engine startup deadline');await delay();}
    // The listener is created only after ownership recovery completes.
    cli(host,['capabilities']);
  };
  const stop=async host=>{const e=host.current;if(e&&!e.result){assert.equal(e.child.kill('SIGTERM'),true);await e.closed;}};
  t.after(async()=>{for(const e of engines)if(!e.result){e.child.kill('SIGKILL');await e.closed;}});
  const packageInfo=file=>{const bytes=fs.readFileSync(file);assert.deepEqual(bytes.subarray(0,8),Buffer.from('PULSPKG\0'));const length=Number(bytes.readBigUInt64LE(12));assert.ok(Number.isSafeInteger(length));const raw=bytes.subarray(52,52+length);assert.equal(hash(raw),bytes.subarray(20,52).toString('hex'));return {bytes,manifest:JSON.parse(raw),version:bytes.readUInt16LE(8)};};
  const exportPackage=(host,project,name)=>{cli(host,['allow-packaging',project,'--acknowledge-unencrypted-private-data']);const file=path.join(root,name);cli(host,['package-export',project,file,'--acknowledge-unencrypted-private-data']);const info=packageInfo(file);for(const forbidden of ['worker_fence_binding','minimum_ledger_version',path.join(source.state,'pulsar-stage-'),path.join(target.state,'pulsar-stage-')])assert.equal(info.bytes.includes(Buffer.from(forbidden)),false,`host ownership leaked: ${forbidden}`);return {file,...info};};
  const exportMotion=(host,project,name)=>{const file=path.join(root,name);cli(host,['export-axis',project,file,'--axis','stroke']);return JSON.parse(fs.readFileSync(file)).actions;};
  await start(source);
  const project=cli(source,['project','create','Authored format4 portability']).value.project_id;
  const script=path.join(root,'authored.funscript');fs.writeFileSync(script,JSON.stringify({actions:[{at:0,pos:20},{at:1000,pos:80},{at:2000,pos:35},{at:3000,pos:65}]}),{mode:0o600});
  const candidate=cli(source,['import-script',script,'--project',project]).value.candidate_id;
  cli(source,['commit-candidate',project,candidate,'--revision','0']);
  const originalActions=exportMotion(source,project,'original.funscript');assert.equal(originalActions.length,4);
  const model=path.join(root,'authored.onnx');fs.writeFileSync(model,authoredOnnxFixture({temporal:true}),{mode:0o600});
  const profile=cli(source,['model','profile',model,'--width','64','--height','64','--class-count','1','--temporal-script-position']);
  const options=path.join(root,'options.json');fs.writeFileSync(options,JSON.stringify({policy:{maximum_analysis_age_ns:1000000000,added_buffer_ns:0},source_anchor:{numerator:0,denominator:1},project_anchor:0,presentation_anchor:0,lease_ms:30000,inference:{model_path:model,profile}}),{mode:0o600});
  const lease=cli(source,['live-capture','start',project,'1',options]).value;
  let until=Date.now()+10000;
  for(;;){const report=cli(source,['live-capture','status',project,'1',lease.identity.epoch]).value;assert.equal(report.active,true,JSON.stringify(report.inference));if(report.inference.state==='running')break;assert.ok(Date.now()<until);await delay();}
  assert.equal(ledger(source).version,4);assert.deepEqual(ledger(source).creation.records,{});assert.equal(Object.keys(ledger(source).staging).length,1);
  cli(source,['live-capture','stop',project,'1',lease.identity.epoch]);
  until=Date.now()+10000;
  while(Object.keys(ledger(source).staging).length||Object.keys(ledger(source).groups).length){assert.ok(Date.now()<until,'live ownership did not retire');await delay();}
  const sourceScope=ledger(source).scope;
  const first=exportPackage(source,project,'source.pulsar');
  await start(target);const targetScope=ledger(target).scope;assert.notEqual(sourceScope,targetScope);assert.equal(ledger(target).version,2);
  const clone=cli(target,['package-import',first.file]).receipt.project_id;assert.notEqual(clone,project);
  assert.deepEqual(exportMotion(target,clone,'first-clone.funscript'),originalActions);
  await stop(target);await start(target);assert.equal(ledger(target).scope,targetScope);assert.equal(ledger(target).version,2);
  assert.deepEqual(exportMotion(target,clone,'restarted-clone.funscript'),originalActions);
  const second=exportPackage(target,clone,'clone.pulsar');
  const cloneAgain=cli(source,['package-import',second.file]).receipt.project_id;assert.notEqual(cloneAgain,project);assert.notEqual(cloneAgain,clone);
  assert.deepEqual(exportMotion(source,cloneAgain,'second-clone.funscript'),originalActions);
  await stop(source);await start(source);assert.equal(ledger(source).scope,sourceScope);assert.equal(ledger(source).version,4);assert.deepEqual(ledger(source).staging,{});assert.deepEqual(ledger(source).creation.records,{});
  assert.deepEqual(exportMotion(source,cloneAgain,'restarted-second-clone.funscript'),originalActions);
  const third=exportPackage(source,cloneAgain,'second-clone.pulsar');
  assert.equal(third.manifest.imported_origins.length,2);
  const sql="import json,sqlite3,sys;d=sqlite3.connect('file:'+sys.argv[1]+'?mode=ro',uri=True);tables={r[0] for r in d.execute('SELECT name FROM sqlite_master WHERE type=\"table\"')};print(json.dumps({'version':d.execute('PRAGMA user_version').fetchone()[0],'binding':d.execute('SELECT scope,minimum_ledger_version FROM worker_fence_binding').fetchall() if 'worker_fence_binding' in tables else [],'jobs':d.execute('SELECT COUNT(*) FROM jobs').fetchone()[0]}))";
  const db=host=>{const r=spawnSync('/usr/bin/python3',['-c',sql,path.join(host.state,'projects.sqlite3')],{encoding:'utf8',timeout:5000});assert.equal(r.status,0,r.stderr);return JSON.parse(r.stdout);};
  const sourceDb=db(source),targetDb=db(target);assert.equal(sourceDb.version,20);assert.deepEqual(sourceDb.binding,[[sourceScope,4]]);assert.deepEqual(targetDb.binding,[]);assert.equal(sourceDb.jobs,0);assert.equal(targetDb.jobs,0);
  fs.writeFileSync(path.join(root,'portable-receipt.json'),JSON.stringify({source_scope:sourceScope,target_scope:targetScope,source_database:sourceDb,target_database:targetDb,package_versions:[first.version,second.version,third.version],package_sha256:[hash(first.bytes),hash(second.bytes),hash(third.bytes)],original_project:project,clones:[clone,cloneAgain],actions:originalActions,restarted_motion_preserved:true,host_staging_paths_absent:true,implicit_jobs:0,physical_dispatch:false},null,2),{mode:0o600});
  await stop(source);await stop(target);
});
