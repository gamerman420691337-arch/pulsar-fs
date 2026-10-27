// Ephemeral first-party signing keys in memory; no official publisher or private media.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import {spawn,spawnSync} from 'node:child_process';
import {once} from 'node:events';
import {authoredOnnxFixture} from './benchmark-onnx-fixture.mjs';
const binary=process.env.PULSAR_BENCHMARK_WORKER;
const sha=b=>crypto.createHash('sha256').update(b).digest('hex');
const canonical=v=>Array.isArray(v)?`[${v.map(canonical).join(',')}]`:v&&typeof v==='object'?`{${Object.keys(v).sort().map(k=>`${JSON.stringify(k)}:${canonical(v[k])}`).join(',')}}`:JSON.stringify(v);
test('owner-authorized offline catalog retains root and watermarks across restart, replay and refusals',{skip:!binary||!process.env.PULSAR_BENCHMARK_ORT,timeout:90000},async t=>{
 const root=fs.mkdtempSync(path.join(os.tmpdir(),'pulsar-catalog-')),state=path.join(root,'state'),source=path.join(root,'catalog'),bootstrap=path.join(root,'bootstrap.json');
 const env={...process.env,PULSAR_STATE_DIR:state,PULSAR_ORT_DYLIB:process.env.PULSAR_BENCHMARK_ORT};let engine,exited;
 const start=async()=>{const log=fs.openSync(path.join(root,`engine-${crypto.randomUUID()}.log`),'wx',0o600);engine=spawn(binary,['engine'],{env,stdio:['ignore','ignore',log]});fs.closeSync(log);exited=once(engine,'exit');const deadline=Date.now()+15000;while(true){assert.equal(engine.exitCode,null);assert.ok(Date.now()<deadline);if(fs.existsSync(path.join(state,'engine.sock'))){const r=spawnSync(binary,['capabilities'],{env,encoding:'utf8',timeout:2000});if(r.status===0)break;}await new Promise(r=>setTimeout(r,25));}};
 const stop=async()=>{if(!engine)return;if(engine.exitCode===null&&engine.signalCode===null)engine.kill('SIGTERM');const timer=setTimeout(()=>engine.kill('SIGKILL'),2000);try{await exited;}finally{clearTimeout(timer);}};t.after(stop);
 let seq=0;const cli=args=>{const r=spawnSync(binary,args,{env,encoding:'utf8',timeout:30000,maxBuffer:1024*1024});assert.equal(r.error,undefined);fs.writeFileSync(path.join(root,`cli-${seq++}.json`),JSON.stringify({args,status:r.status,stdout:r.stdout,stderr:r.stderr}),{mode:0o600});return r;};
 const ok=args=>{const r=cli(args);assert.equal(r.status,0,r.stderr);return JSON.parse(r.stdout).value;};const fail=args=>{const r=cli(args);assert.notEqual(r.status,0);return r;};
 const helper=process.env.PULSAR_PACK_RECOVERY_FIXTURE;
 const recover=(mode,id,...args)=>{const r=spawnSync(helper,[mode,state,id,...args],{env,encoding:'utf8',timeout:30000,maxBuffer:1024*1024});assert.equal(r.status,0,r.stderr);return JSON.parse(r.stdout);};
 const keys=Array.from({length:9},()=>crypto.generateKeyPairSync('ed25519'));
 const key=i=>({keytype:'ed25519',scheme:'ed25519',keyval:{public:keys[i].publicKey.export({type:'spki',format:'der'}).subarray(-32).toString('hex')}});const id=i=>sha(canonical(key(i)));
 const signed=(value,signers)=>Buffer.from(JSON.stringify({signed:value,signatures:signers.map(i=>({keyid:id(i),sig:crypto.sign(null,Buffer.from(canonical(value)),keys[i].privateKey).toString('hex')}))}));
 const rootDoc=(version,roots,signers,ts=4,ss=3)=>signed({_type:'root',spec_version:'1.0.0',version,expires:'2099-01-01T00:00:00Z',consistent_snapshot:false,keys:Object.fromEntries([...roots,2,ts,ss].map(i=>[id(i),key(i)])),roles:{root:{keyids:roots.map(id),threshold:2},targets:{keyids:[id(2)],threshold:1},timestamp:{keyids:[id(ts)],threshold:1},snapshot:{keyids:[id(ss)],threshold:1}}},signers);
 const initialRoot=rootDoc(1,[0,1],[0,1]);fs.writeFileSync(bootstrap,initialRoot);const digest=sha(initialRoot);
 const packSource=path.join(root,'pack-source');fs.mkdirSync(packSource);
 const model=authoredOnnxFixture(),runtime=fs.readFileSync(process.env.PULSAR_BENCHMARK_ORT),license=Buffer.from('Authored fixture license bytes');
 const identity=(b,id)=>({artifact_id:id,sha256:sha(b),byte_len:b.length});
 const profile={schema_version:1,model:{artifact_id:sha(model),sha256:sha(model),byte_len:model.length},runtime:{artifact_id:sha(runtime),sha256:sha(runtime),byte_len:runtime.length},preprocessing:{input:{width:64,height:64,channels:3,layout:'nchw',class_count:1,decoder:'yolo_channel_major'},resize:'stretch',color:'rgb',scale:[1/255,1/255,1/255],bias:[0,0,0],padding_byte:0},provider:'cpu',minimum_memory_bytes:134217728,minimum_output_bytes:1048576,cpu_threads:2};
 const packFiles=[['model.onnx','model',model],['runtime.so','runtime',runtime],['profile.json','profile',Buffer.from(JSON.stringify(profile))],['LICENSE','license',license]];
 for(const [name,,b]of packFiles)fs.writeFileSync(path.join(packSource,name),b);
 const manifest=Buffer.from(JSON.stringify({schema_version:1,pack_id:'authored-native-catalog',target_os:'linux',target_arch:'x86_64',ort_api_version:24,labels:['fixture'],assets:packFiles.map(([p,role,b])=>({path:p,role,byte_len:b.length,sha256:sha(b)})),license_path:'LICENSE',source_description:'First-party inert fixture; no supplier runtime execution'}));fs.writeFileSync(path.join(packSource,'manifest.json'),manifest);

 const manifestName=`manifests/${sha(manifest)}.json`;
 const write=(version,{rotated=false,expired=false,expiresAt=null,omitManifest=false,additional=[]}={})=>{fs.rmSync(source,{recursive:true,force:true});fs.mkdirSync(path.join(source,'metadata'),{recursive:true});fs.mkdirSync(path.join(source,'targets','manifests'),{recursive:true});
  const targets=signed({_type:'targets',spec_version:'1.0.0',version,expires:'2099-01-01T00:00:00Z',targets:omitManifest?{}:Object.fromEntries([manifest,...additional].map(b=>[`manifests/${sha(b)}.json`,{length:b.length,hashes:{sha256:sha(b)}}]))},[2]);
  const snapshot=signed({_type:'snapshot',spec_version:'1.0.0',version,expires:'2099-01-01T00:00:00Z',meta:{'targets.json':{version,length:targets.length,hashes:{sha256:sha(targets)}}}},[rotated?8:3]);
  const timestamp=signed({_type:'timestamp',spec_version:'1.0.0',version,expires:expired?'2000-01-01T00:00:00Z':expiresAt??'2099-01-01T00:00:00Z',meta:{'snapshot.json':{version,length:snapshot.length,hashes:{sha256:sha(snapshot)}}}},[rotated?7:4]);
  for(const [name,bytes]of [['targets.json',targets],['snapshot.json',snapshot],['timestamp.json',timestamp]])fs.writeFileSync(path.join(source,'metadata',name),bytes);
  if(rotated)fs.writeFileSync(path.join(source,'metadata','2.root.json'),rootDoc(2,[5,6],[0,1,5,6],7,8));fs.writeFileSync(path.join(source,'targets',manifestName),manifest);for(const b of additional)fs.writeFileSync(path.join(source,'targets','manifests',`${sha(b)}.json`),b);
 };
 const refresh=(revision,request)=>['model','catalog-refresh',source,bootstrap,digest,'--expected-revision',String(revision),'--approve-bootstrap','--request-id',request];const status=()=>ok(['model','catalog-status',digest]);
 write(1);await start();fail(['model','catalog-status',digest]);assert.equal(fs.existsSync(path.join(state,'model-catalogs')),false);
 const owner=path.join(state,'model-pack-owner.token'),proof=fs.readFileSync(owner);fs.writeFileSync(owner,'0'.repeat(proof.toString().trim().length));fail(refresh(0,'wrong-owner'));assert.equal(fs.existsSync(path.join(state,'model-catalogs')),false);fs.writeFileSync(owner,proof);
 const noApproval=refresh(0,'no-approval').filter(a=>a!=='--approve-bootstrap');fail(noApproval);assert.equal(fs.existsSync(path.join(state,'model-catalogs')),false);
 const first=ok(refresh(0,'catalog-first'));assert.equal(first.catalog_revision,1);assert.deepEqual(first.manifest_sha256s,[sha(manifest)]);assert.equal(first.authenticated_at_acceptance,true);for(const k of ['current_publisher_trust','native_execution_approved','rights_verified','compatible','qualified'])assert.equal(first[k],false);
 if(helper){recover('stage-catalog','catalog-first',source,bootstrap,digest,'0');assert.deepEqual(recover('lost-ack','catalog-first'),first);assert.equal(recover('list','catalog-first')[0].acknowledged,false);}
 const capsule=path.join(state,'model-catalogs',`${digest}.json`);assert.equal(fs.statSync(capsule).mode&0o777,0o600);assert.equal(fs.statSync(path.dirname(capsule)).mode&0o777,0o700);
 await stop();await start();assert.deepEqual(status(),first);
 write(2,{rotated:true});const second=ok(refresh(1,'catalog-second'));assert.equal(second.catalog_revision,2);assert.equal(second.root_version,2);
 await stop();await start();assert.deepEqual(status(),second);const retained=fs.readFileSync(capsule);
 if(helper){assert.deepEqual(recover('replay','catalog-first'),first);assert.equal(recover('list','catalog-first')[0].acknowledged,true);assert.deepEqual(status(),second);
  if(process.env.PULSAR_RECOVERY_SCHEMA1_TEST_BINARY){const files=fs.readdirSync(path.join(state,'desktop-pack-recovery')).filter(n=>n.endsWith('.json'));assert.equal(files.length,1);const name=path.join(state,'desktop-pack-recovery',files[0]),before=fs.readFileSync(name);const old=spawnSync(process.env.PULSAR_RECOVERY_SCHEMA1_TEST_BINARY,['list',state,'catalog-first'],{env,encoding:'utf8',timeout:10000});assert.notEqual(old.status,0);assert.deepEqual(fs.readFileSync(name),before);}
 }
 write(1,{rotated:true});fail(refresh(2,'rotated-role-rollback'));assert.deepEqual(fs.readFileSync(capsule),retained);
 write(3,{rotated:true,expired:true});fail(refresh(2,'expired-refresh'));assert.deepEqual(fs.readFileSync(capsule),retained);
 write(3,{rotated:true});fail(refresh(0,'stale-revision'));assert.deepEqual(fs.readFileSync(capsule),retained);
 const target=path.join(source,'targets',manifestName);fs.writeFileSync(target,'corrupt target');fail(refresh(2,'corrupt-target'));assert.deepEqual(fs.readFileSync(capsule),retained);
 write(3,{rotated:true});const bytes=fs.readFileSync(target),outside=path.join(root,'preserved-manifest');fs.writeFileSync(outside,bytes);fs.unlinkSync(target);fs.symlinkSync(outside,target);fail(refresh(2,'symlink-target'));assert.deepEqual(fs.readFileSync(capsule),retained);fs.unlinkSync(target);fs.linkSync(outside,target);fail(refresh(2,'hardlink-target'));assert.deepEqual(fs.readFileSync(capsule),retained);
 assert.deepEqual(ok(refresh(0,'catalog-first')),first);assert.deepEqual(status(),second);
 // Remove only the owned fixture's SQL response to exercise filesystem replay after a missing DB acknowledgement.
 await stop();const db=path.join(state,'projects.sqlite3');const repair=spawnSync('python3',['-c',"import sqlite3,sys;d=sqlite3.connect(sys.argv[1]);d.execute('DELETE FROM requests WHERE request=?',('catalog-first',));d.commit()",db],{encoding:'utf8'});assert.equal(repair.status,0,repair.stderr);await start();assert.deepEqual(ok(refresh(0,'catalog-first')),first);assert.deepEqual(status(),second);assert.deepEqual(fs.readFileSync(capsule),retained);
 fail(refresh(1,'catalog-first'));assert.deepEqual(fs.readFileSync(capsule),retained);
 fs.writeFileSync(capsule,'corrupt private capsule');fail(['model','catalog-status',digest]);fail(refresh(2,'preserve-corrupt-capsule'));assert.equal(fs.readFileSync(capsule,'utf8'),'corrupt private capsule');fs.writeFileSync(capsule,retained);
 fs.chmodSync(capsule,0o644);fail(['model','catalog-status',digest]);fs.chmodSync(capsule,0o600);fs.renameSync(capsule,`${capsule}.preserved`);fs.symlinkSync(`${capsule}.preserved`,capsule);fail(['model','catalog-status',digest]);fs.unlinkSync(capsule);fs.renameSync(`${capsule}.preserved`,capsule);
 write(3,{rotated:true});const alternateRoot=rootDoc(1,[5,6],[5,6],7,8),alternateBootstrap=path.join(root,'alternate-bootstrap.json'),alternateDigest=sha(alternateRoot);fs.writeFileSync(alternateBootstrap,alternateRoot);
 // Failed admissions may leave private lock entries; a first capsule needs one additional slot.
 for(let i=0;i<125;i++)fs.writeFileSync(path.join(path.dirname(capsule),`owned-fixture-filler-${i}.lock`),'',{mode:0o600});
 assert.equal(fs.readdirSync(path.dirname(capsule)).length,127);
 fail(['model','catalog-refresh',source,alternateBootstrap,alternateDigest,'--expected-revision','0','--approve-bootstrap','--request-id','first-capsule-capacity']);
 assert.equal(fs.readdirSync(path.dirname(capsule)).length,128);assert.equal(fs.existsSync(path.join(path.dirname(capsule),`${alternateDigest}.json`)),false);assert.deepEqual(fs.readFileSync(capsule),retained);
 await stop();await start();assert.deepEqual(status(),second);assert.equal(fs.existsSync(path.join(state,'model-packs')),false);const selection=ok(['model','pack-selection-status']);assert.equal(selection.choice,null);
 const check=(revision=2,hash=sha(manifest))=>['model','pack-catalog-check',digest,hash,'--expected-revision',String(revision)];
 fail(check());assert.equal(fs.existsSync(path.join(state,'model-packs')),false);
 ok(['model','pack-import',packSource,'--request-id','catalog-inert-pack-import']);
 fs.writeFileSync(owner,'0'.repeat(proof.toString().trim().length));fail(check());fs.writeFileSync(owner,proof);
 const association=ok(check());assert.equal(association.listed_in_catalog,true);assert.equal(association.online_revocation_checked,false);assert.equal(association.catalog.catalog_revision,2);assert.equal(association.pack.manifest_sha256,sha(manifest));assert.ok(association.metadata_expires_at_unix_ms>association.checked_at_unix_ms);
 assert.deepEqual(fs.readFileSync(capsule),retained);assert.equal(ok(['model','pack-selection-status']).choice,null);
 fail(check(1));fail(check(2,'a'.repeat(64)));
 const installedModel=path.join(state,'model-packs',sha(manifest),'model.onnx');fs.writeFileSync(installedModel,'corrupt installed model');fail(check());fs.writeFileSync(installedModel,model);
 const selectArgs=(catalogRevision,selectionRevision,request)=>['model','pack-catalog-select',digest,sha(manifest),'--expected-catalog-revision',String(catalogRevision),'--expected-selection-revision',String(selectionRevision),'--approve-unqualified-native-execution','--request-id',request];
 fail(selectArgs(1,0,'catalog-stale-select'));const selected=ok(selectArgs(2,0,'catalog-bound-select'));assert.equal(selected.schema_version,2);assert.deepEqual(selected.catalog_binding,{bootstrap_sha256:digest,catalog_revision:2});assert.equal(selected.choice.selection_revision,1);
 const media=path.join(root,'source.y4m');fs.writeFileSync(media,Buffer.concat([Buffer.from('YUV4MPEG2 W64 H64 F8:1 Ip A1:1 Cmono\n'),...Array.from({length:16},(_,frame)=>Buffer.concat([Buffer.from('FRAME\n'),Buffer.from(Array.from({length:4096},(_,i)=>(i%64*31+((Math.floor(i/64)+64-frame)%64)*17)%256))]))]));
 const generate=name=>['generate',media,'--model-pack',sha(manifest),'--width','64','--height','64','--output',path.join(root,name)];ok(generate('catalog-selected.funscript'));
 const schema=()=>{const r=spawnSync('/usr/bin/python3',['-c',"import sqlite3,sys;d=sqlite3.connect(sys.argv[1]);print(d.execute('PRAGMA user_version').fetchone()[0])",db],{encoding:'utf8'});assert.equal(r.status,0,r.stderr);return Number(r.stdout);};assert.equal(schema(),20);
 const jobs=()=>{const r=spawnSync('/usr/bin/python3',['-c',"import sqlite3,sys;d=sqlite3.connect(sys.argv[1]);print(d.execute('SELECT COUNT(*) FROM jobs').fetchone()[0])",db],{encoding:'utf8'});assert.equal(r.status,0,r.stderr);return Number(r.stdout);};const admitted=jobs();assert.equal(admitted,1);
 const jobRow=spawnSync('/usr/bin/python3',['-c',"import sqlite3,json,sys;d=sqlite3.connect(sys.argv[1]);print(json.dumps(d.execute('SELECT project,source FROM jobs').fetchone()))",db],{encoding:'utf8'});assert.equal(jobRow.status,0,jobRow.stderr);const [project,sourceVersion]=JSON.parse(jobRow.stdout);
 const preview=()=>['project','preview-pack',project,'500000000','--model-pack',sha(manifest),'--source-version',sourceVersion];ok(preview());
 await stop();if(process.env.PULSAR_SCHEMA13_CATALOG_TEST_BINARY){const before=fs.readFileSync(db),wal=fs.existsSync(`${db}-wal`)?fs.readFileSync(`${db}-wal`):null;const old=spawnSync(process.env.PULSAR_SCHEMA13_CATALOG_TEST_BINARY,['engine'],{env,encoding:'utf8',timeout:10000});assert.notEqual(old.status,0);assert.match(old.stderr,/explicit recoverable migration/);assert.deepEqual(fs.readFileSync(db),before);assert.deepEqual(fs.existsSync(`${db}-wal`)?fs.readFileSync(`${db}-wal`):null,wal);}await start();assert.deepEqual(ok(['model','pack-selection-status']),selected);
 const expiry=Date.now()+8000;write(3,{rotated:true,expiresAt:new Date(expiry).toISOString()});const third=ok(refresh(2,'catalog-expiring'));ok(check(3));fail(generate('changed-catalog-refused.funscript'));fail(preview());assert.equal(jobs(),admitted);assert.deepEqual(ok(selectArgs(2,0,'catalog-bound-select')),selected);const reapproved=ok(selectArgs(3,1,'catalog-reapproved'));assert.equal(reapproved.choice.selection_revision,2);
 await new Promise(r=>setTimeout(r,Math.max(1,expiry-Date.now()+20)));fail(check(3));fail(generate('expired-catalog-refused.funscript'));fail(preview());assert.equal(jobs(),admitted);assert.deepEqual(status(),third);
 write(4,{rotated:true,omitManifest:true});const fourth=ok(refresh(3,'catalog-delist'));assert.deepEqual(fourth.manifest_sha256s,[]);fail(check(3));fail(check(4));
 assert.equal(ok(['model','pack-status',sha(manifest)]).integrity_verified,true);assert.deepEqual(ok(['model','pack-selection-status']),reapproved);fail(generate('delisted-catalog-refused.funscript'));fail(preview());assert.equal(jobs(),admitted);fail(selectArgs(4,2,'delisted-select'));
 await stop();await start();assert.deepEqual(status(),fourth);fail(check(4));fail(generate('restarted-delisted-refused.funscript'));assert.equal(jobs(),admitted);ok(['model','pack-deactivate','--expected-selection-revision','2']);assert.equal(schema(),20);
 // Update/rollback changes the owner-selected pack, never authenticated catalog watermarks.
 const updateSource=path.join(root,'update-source');fs.mkdirSync(updateSource);const nextModel=authoredOnnxFixture({temporal:true});
 const nextProfile={...profile,model:{artifact_id:sha(nextModel),sha256:sha(nextModel),byte_len:nextModel.length},preprocessing:{...profile.preprocessing,input:{...profile.preprocessing.input,channels:6,decoder:'temporal_script_position'},resize:'nearest_stretch',color:'rgb_frame_pair',scale:Array(6).fill(1/255),bias:Array(6).fill(0)}};
 const updateFiles=[['model.onnx','model',nextModel],['runtime.so','runtime',runtime],['profile.json','profile',Buffer.from(JSON.stringify(nextProfile))],['LICENSE','license',license]];
 for(const [name,,b]of updateFiles)fs.writeFileSync(path.join(updateSource,name),b);
 const nextManifest=Buffer.from(JSON.stringify({...JSON.parse(manifest),pack_id:'authored-update-catalog',assets:updateFiles.map(([p,role,b])=>({path:p,role,byte_len:b.length,sha256:sha(b)}))}));fs.writeFileSync(path.join(updateSource,'manifest.json'),nextManifest);
 write(5,{rotated:true,additional:[nextManifest]});const updateCatalog=ok(refresh(4,'catalog-update-inventory'));const catalogBytes=fs.readFileSync(capsule);
 const beforeUpdate=ok(selectArgs(5,3,'catalog-before-update'));assert.equal(beforeUpdate.choice.selection_revision,4);
 const oldAsset=fs.readFileSync(installedModel);fs.writeFileSync(path.join(updateSource,'model.onnx'),'corrupt proposed update');fail(['model','pack-import',updateSource,'--request-id','corrupt-update-import']);assert.deepEqual(ok(['model','pack-selection-status']),beforeUpdate);assert.deepEqual(fs.readFileSync(installedModel),oldAsset);assert.equal(fs.existsSync(path.join(state,'model-packs',sha(nextManifest))),false);
 fs.writeFileSync(path.join(updateSource,'model.onnx'),nextModel);ok(['model','pack-import',updateSource,'--request-id','valid-update-import']);assert.deepEqual(ok(['model','pack-selection-status']),beforeUpdate);
 const updateArgs=['model','pack-catalog-select',digest,sha(nextManifest),'--expected-catalog-revision','5','--expected-selection-revision','4','--approve-unqualified-native-execution','--request-id','catalog-update-select'];const updated=ok(updateArgs);assert.equal(updated.choice.selection_revision,5);
 const generateNext=['generate',media,'--model-pack',sha(nextManifest),'--width','64','--height','64','--output',path.join(root,'updated.funscript')];ok(generateNext);assert.equal(jobs(),admitted+1);
 if(helper){recover('stage-catalog-select','catalog-update-select',sha(nextManifest),digest,'5','4');assert.deepEqual(recover('lost-ack','catalog-update-select'),updated);}
 const rolledBack=ok(selectArgs(5,5,'catalog-explicit-rollback'));assert.equal(rolledBack.choice.selection_revision,6);ok(generate('rolled-back.funscript'));assert.equal(jobs(),admitted+2);
 assert.deepEqual(status(),updateCatalog);assert.deepEqual(fs.readFileSync(capsule),catalogBytes);assert.deepEqual(fs.readFileSync(installedModel),oldAsset);assert.deepEqual(fs.readFileSync(path.join(state,'model-packs',sha(nextManifest),'model.onnx')),nextModel);
 assert.deepEqual(ok(updateArgs),updated);assert.deepEqual(ok(['model','pack-selection-status']),rolledBack);
 await stop();await start();assert.deepEqual(ok(['model','pack-selection-status']),rolledBack);if(helper){assert.deepEqual(recover('replay','catalog-update-select'),updated);assert.deepEqual(ok(['model','pack-selection-status']),rolledBack);}
 assert.equal(schema(),20);assert.deepEqual(status(),updateCatalog);
 const models=spawnSync('/usr/bin/python3',['-c',"import sqlite3,json,sys;d=sqlite3.connect(sys.argv[1]);print(json.dumps([json.loads(r[0])['operation']['arguments']['model']['identity']['sha256'] for r in d.execute('SELECT manifest FROM jobs ORDER BY rowid')]))",db],{encoding:'utf8'});assert.equal(models.status,0,models.stderr);assert.deepEqual(JSON.parse(models.stdout),[sha(model),sha(nextModel),sha(model)]);
 t.diagnostic(JSON.stringify({fixture:root,state:'catalog_bound_pack_update_and_explicit_rollback',catalog_revisions:[1,2,3,4,5],root_versions:[1,2],sql_ack_omission:'synthetic deletion of fixture response',database_schema:schema(),schema13_reader_refused:!!process.env.PULSAR_SCHEMA13_CATALOG_TEST_BINARY,jobs:jobs(),private_keys_stored:false,network_fetch:false,physical_device:false,native_window:false}));
});
