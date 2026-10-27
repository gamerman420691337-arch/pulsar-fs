// Actual copied CLI/Engine/worker execution; authored media/model, no quality claim.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import crypto from 'node:crypto';
import {spawn,spawnSync} from 'node:child_process';
import {once} from 'node:events';
import {fileURLToPath} from 'node:url';
import {authoredOnnxFixture} from './benchmark-onnx-fixture.mjs';
const assembler=process.env.PULSAR_TEST_BUNDLE_ASSEMBLER ?? fileURLToPath(new URL('./assemble-linux-runtime-candidate.mjs',import.meta.url));
const binary=process.env.PULSAR_TEST_BUNDLE_BINARY;
const media=process.env.PULSAR_TEST_BUNDLE_MEDIA;
const declaration=process.env.PULSAR_TEST_CPU_RUNTIME_DECLARATION;
const assistant=process.env.PULSAR_TEST_ASSISTANT_DECLARATION;
const enabled=binary&&media&&declaration;
const digest=f=>crypto.createHash('sha256').update(fs.readFileSync(f)).digest('hex');
const assemble=(out,decl)=>spawnSync(process.execPath,[assembler,binary,media,out,...(assistant?[assistant]:[]),'--cpu-runtime',decl],{encoding:'utf8',timeout:60000,maxBuffer:1<<20});
test('relocated bundle generates through copied CLI, Engine and confined CPU worker without runtime overrides',{skip:!enabled,timeout:120000},async t=>{
 const root=fs.mkdtempSync(path.join(os.tmpdir(),'cpu-bundle-'));
 let engine,stopped;
 try {
  const original=path.join(root,'original');const built=assemble(original,declaration);assert.ifError(built.error);assert.equal(built.status,0,built.stderr);
  const spec=JSON.parse(fs.readFileSync(declaration));const manifest=JSON.parse(fs.readFileSync(path.join(original,'runtime-candidate.json')));
  assert.equal(manifest.files.length,assistant?15:11);assert.equal(manifest.qualified,false);assert.equal(manifest.cpu_runtime.qualified,false);
  for(const r of manifest.files){assert.equal(digest(path.join(original,r.path)),r.sha256);assert.equal(fs.statSync(path.join(original,r.path)).size,r.byte_len);}
  const moved=path.join(root,'relocated package');fs.renameSync(original,moved);
  const state=path.join(root,'state');const env={PATH:'/usr/bin:/bin',PULSAR_STATE_DIR:state};const app=path.join(moved,'pulsar');
  const cli=args=>{const r=spawnSync(app,args,{env,encoding:'utf8',timeout:30000,maxBuffer:4<<20});
   t.diagnostic(JSON.stringify({args,status:r.status,error:r.error?.message,stdout:r.stdout,stderr:r.stderr}));assert.ifError(r.error);return r;};
  const fd=fs.openSync(path.join(root,'engine.log'),'wx',0o600);engine=spawn(app,['engine'],{env,stdio:['ignore','ignore',fd]});fs.closeSync(fd);stopped=once(engine,'exit');
  const deadline=Date.now()+15000;
  while(!fs.existsSync(path.join(state,'engine.sock'))){assert.equal(engine.exitCode,null,fs.readFileSync(path.join(root,'engine.log'),'utf8'));assert.ok(Date.now()<deadline,'engine startup deadline');await new Promise(r=>setTimeout(r,25));}
  const status=cli(['model','status']);assert.equal(status.status,0,status.stderr);const catalog=JSON.parse(status.stdout);
  assert.ok(catalog.runtime,'bundled runtime unavailable');assert.equal(catalog.runtime.sha256,spec.runtime.sha256);assert.deepEqual(catalog.providers,['cpu']);assert.equal(catalog.qualified_fallback_available,false);
  const model=path.join(root,'authored.onnx');fs.writeFileSync(model,authoredOnnxFixture());
  const profile=cli(['model','profile',model,'--width','64','--height','64','--class-count','1']);assert.equal(profile.status,0,profile.stderr);
  assert.equal(JSON.parse(profile.stdout).runtime.sha256,spec.runtime.sha256);const profilePath=path.join(root,'profile.json');fs.writeFileSync(profilePath,profile.stdout);
  const source=path.join(root,'source.y4m');fs.writeFileSync(source,Buffer.concat([Buffer.from('YUV4MPEG2 W64 H64 F8:1 Ip A1:1 Cmono\n'),...Array.from({length:16},(_,frame)=>Buffer.concat([Buffer.from('FRAME\n'),Buffer.from(Array.from({length:4096},(_,i)=>(i%64*31+((Math.floor(i/64)+64-frame)%64)*17)%256))]))]));
  const output=path.join(root,'output.funscript');const generated=cli(['generate',source,'--model',model,'--inference-profile',profilePath,'--width','64','--height','64','--output',output]);assert.equal(generated.status,0,generated.stderr);assert.ok(JSON.parse(fs.readFileSync(output)).actions.length>0);
  const db=spawnSync('/usr/bin/python3',['-c',"import sqlite3,sys;d=sqlite3.connect('file:'+sys.argv[1]+'?mode=ro',uri=True);print(d.execute('SELECT manifest FROM jobs').fetchone()[0])",path.join(state,'projects.sqlite3')],{encoding:'utf8',timeout:5000});assert.equal(db.status,0,db.stderr);const job=JSON.parse(db.stdout);
  assert.equal(job.tools.onnx_runtime.identity.sha256,spec.runtime.sha256);assert.equal(job.tools.onnx_runtime.path,path.join(state,'snapshots',spec.runtime.sha256));assert.equal(digest(job.tools.onnx_runtime.path),spec.runtime.sha256);
  for(const name of ['ffmpeg','ffprobe']){const supplied=manifest.files.find(r=>r.path===`runtimes/${name}`);assert.equal(job.tools[name].identity.sha256,supplied.sha256);assert.equal(job.tools[name].path,path.join(moved,'runtimes',name));}
  assert.equal(digest(path.join(moved,'pulsar-worker')),manifest.files.find(r=>r.path==='pulsar-worker').sha256);
  const supplier=path.join(root,'pack');fs.mkdirSync(supplier);
  const items=[['model.onnx','model',fs.readFileSync(model)],['runtime.bin','runtime',fs.readFileSync(path.join(moved,manifest.cpu_runtime.path))],['profile.json','profile',Buffer.from(profile.stdout)],['LICENSE','license',Buffer.from('Authored functional fixture; no model quality qualification.')]];
  for(const [name,,bytes] of items)fs.writeFileSync(path.join(supplier,name),bytes);
  const pack={schema_version:1,pack_id:'authored-bundled-cpu-pack',target_os:'linux',target_arch:'x86_64',ort_api_version:24,labels:['fixture'],assets:items.map(([name,role,b])=>({path:name,role,byte_len:b.length,sha256:crypto.createHash('sha256').update(b).digest('hex')})),license_path:'LICENSE',source_description:'Authored functional fixture; not an edition model'};
  const packBytes=Buffer.from(JSON.stringify(pack));fs.writeFileSync(path.join(supplier,'manifest.json'),packBytes);const packHash=crypto.createHash('sha256').update(packBytes).digest('hex');
  const imported=cli(['model','pack-import',supplier]);assert.equal(imported.status,0,imported.stderr);assert.equal(JSON.parse(imported.stdout).value.active,false);
  const selected=cli(['model','pack-select',packHash,'--expected-selection-revision','0','--approve-unqualified-native-execution']);assert.equal(selected.status,0,selected.stderr);assert.equal(JSON.parse(selected.stdout).value.qualified,false);
  const packOutput=path.join(root,'pack-output.funscript');const packGenerated=cli(['generate',source,'--model-pack',packHash,'--width','64','--height','64','--output',packOutput]);assert.equal(packGenerated.status,0,packGenerated.stderr);assert.equal(fs.readFileSync(packOutput,'utf8'),fs.readFileSync(output,'utf8'));
  const fence=JSON.parse(fs.readFileSync(path.join(state,'worker-fence.json')));assert.deepEqual(fence.groups,{});assert.deepEqual(fence.creation.records,{});
  t.diagnostic(JSON.stringify({manifest,generation:'PASS; direct profile and owner-selected authored pack; authored model/media only',job_runtime:job.tools.onnx_runtime,output_actions:JSON.parse(fs.readFileSync(output)).actions.length,final_custody_empty:true}));
 } finally {
  if(engine){if(engine.exitCode===null&&engine.signalCode===null)engine.kill('SIGTERM');const timer=setTimeout(()=>engine.kill('SIGKILL'),2000);try{await stopped;}finally{clearTimeout(timer);}}
  fs.rmSync(root,{recursive:true,force:true});
 }
});
test('CPU runtime wrong pins, wrong ELF and missing notices refuse before output creation',{skip:!enabled},()=>{
 const root=fs.mkdtempSync(path.join(os.tmpdir(),'cpu-bundle-refusal-'));
 try {
  const original=JSON.parse(fs.readFileSync(declaration));const bad=path.join(root,'bad.json');
  for(const kind of ['pin','elf','notices']){
   const spec=structuredClone(original);
   if(kind==='pin')spec.runtime.sha256='0'.repeat(64);
   if(kind==='elf')spec.runtime={path:'/usr/bin/true',byte_len:fs.statSync('/usr/bin/true').size,sha256:digest('/usr/bin/true')};
   if(kind==='notices')delete spec.notices;
   fs.writeFileSync(bad,JSON.stringify(spec));const out=path.join(root,kind);const r=assemble(out,bad);assert.ifError(r.error);assert.notEqual(r.status,0,kind);assert.equal(fs.existsSync(out),false,kind);
  }
 }finally{fs.rmSync(root,{recursive:true,force:true});}
});
