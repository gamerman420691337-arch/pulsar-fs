// Actual published generic model delivery; authored media, never target quality.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import crypto from 'node:crypto';
import {spawnSync,spawn} from 'node:child_process';
import {once} from 'node:events';
import {fileURLToPath} from 'node:url';
const script=fileURLToPath(new URL('./assemble-linux-runtime-candidate.mjs',import.meta.url));
const binary=process.env.PULSAR_TEST_INSTALLER,borrowed=process.env.PULSAR_TEST_INSTALL_SOURCE,packSource=process.env.PULSAR_TEST_GENERATION_PACK;
const sha=b=>crypto.createHash('sha256').update(b).digest('hex');
test('real generation pack survives assembly, archive, cache relocation and confined worker use',{skip:!binary||!borrowed||!packSource,timeout:180000},async t=>{
 const root=fs.mkdtempSync(path.join(os.tmpdir(),'pulsar-generation-delivery-'));fs.chmodSync(root,0o700);t.diagnostic(`fixture: ${root}`);
 const records=[];let engine,terminal;
 const execute=(exe,args,env={PATH:'/usr/bin:/bin'})=>{const result=spawnSync(exe,args,{env,encoding:'utf8',timeout:90000,maxBuffer:4*1024**2});assert.ifError(result.error);records.push({exe,args,status:result.status,stdout:result.stdout,stderr:result.stderr});return result;};
 try{
  const supplier=path.join(root,'supplier');fs.cpSync(packSource,supplier,{recursive:true});
  const pack=JSON.parse(fs.readFileSync(path.join(supplier,'manifest.json'))),model=pack.assets.find(a=>a.role==='model');
  const modelPath=model.path;model.path='models/deep/model.onnx';fs.mkdirSync(path.join(supplier,'models/deep'),{recursive:true});fs.renameSync(path.join(supplier,modelPath),path.join(supplier,model.path));
  const packBytes=Buffer.from(JSON.stringify(pack)),packHash=sha(packBytes);fs.writeFileSync(path.join(supplier,'manifest.json'),packBytes);
  const declared=path.join(root,'generation.json');fs.writeFileSync(declared,JSON.stringify({schema_version:1,packs:[{path:supplier,manifest_sha256:packHash}]}));
  const media=path.join(root,'media');fs.mkdirSync(media);for(const name of ['ffmpeg','ffprobe'])fs.copyFileSync(path.join(borrowed,'runtimes',name),path.join(media,name));for(const name of ['COPYING.LGPLv2.1','LICENSE.md','ffmpeg-9.0.2.tar.xz','configure.log'])fs.copyFileSync(path.join(borrowed,'media-source',name),path.join(media,name));for(const name of ['ffmpeg','ffprobe'])fs.chmodSync(path.join(media,name),0o755);
  const identity=f=>({path:f,byte_len:fs.statSync(f).size,sha256:sha(fs.readFileSync(f))});
  const cpu=path.join(root,'cpu.json');fs.writeFileSync(cpu,JSON.stringify({schema_version:1,runtime:identity(path.join(borrowed,'runtimes/libonnxruntime.so.1')),license:identity(path.join(borrowed,'runtime-source/ONNX-RUNTIME-LICENSE')),notices:identity(path.join(borrowed,'runtime-source/ONNX-RUNTIME-THIRD-PARTY-NOTICES'))}));
  const candidate=path.join(root,'candidate');let r=execute(process.execPath,[script,binary,media,candidate,'--generation-packs',declared,'--cpu-runtime',cpu]);assert.equal(r.status,0,r.stderr);
  const manifest=JSON.parse(fs.readFileSync(path.join(candidate,'runtime-candidate.json')));assert.equal(manifest.generation_packs.length,1);assert.equal(manifest.generation_packs[0].qualified,false);assert.equal(manifest.files.length,16);
  const archive=path.join(root,'runtime.pack');r=execute(binary,['runtime-pack',candidate,archive]);assert.equal(r.status,0,r.stderr);const cache=path.join(root,'cache');
  r=execute(binary,['runtime-launch',archive,cache,'--','--version']);assert.equal(r.status,0,r.stderr);
  const hash=sha(fs.readFileSync(path.join(candidate,'runtime-candidate.json'))),installed=path.join(cache,hash),nested=path.join(installed,'generation',packHash,'models/deep');
  assert.equal(sha(fs.readFileSync(path.join(nested,'model.onnx'))),model.sha256);
  r=execute(binary,['runtime-cache-inspect',cache,hash]);assert.equal(r.status,0,r.stderr);assert.match(r.stdout,/Verified retained runtime/);
  fs.writeFileSync(path.join(nested,'unknown'),'preserve');r=execute(binary,['runtime-cache-inspect',cache,hash]);assert.notEqual(r.status,0);r=execute(binary,['runtime-models',cache,hash]);assert.notEqual(r.status,0);assert.equal(fs.readFileSync(path.join(nested,'unknown'),'utf8'),'preserve');fs.unlinkSync(path.join(nested,'unknown'));
  fs.renameSync(candidate,path.join(root,'hidden-candidate'));fs.renameSync(archive,path.join(root,'hidden-archive'));
  r=execute(binary,['runtime-cache-launch',cache,hash,'--','--version']);assert.equal(r.status,0,r.stderr);
  const discoveryState=path.join(root,'discovery-must-not-create'),discoveryEnv={PATH:'/usr/bin:/bin',PULSAR_STATE_DIR:discoveryState};
  r=execute(binary,['runtime-models',cache,hash],discoveryEnv);assert.equal(r.status,0,r.stderr);const discovered=JSON.parse(r.stdout);
  assert.equal(discovered.integrity_verified,true);assert.equal(discovered.runtime_manifest_sha256,hash);assert.equal(discovered.packs[0].manifest_sha256,packHash);assert.deepEqual(discovered.packs[0].manifest,pack);
  for(const key of ['native_execution_approved','rights_verified','qualified'])assert.equal(discovered[key],false);
  r=execute(path.join(installed,'pulsar'),['bundled-models'],discoveryEnv);assert.equal(r.status,0,r.stderr);assert.deepEqual(JSON.parse(r.stdout),discovered);assert.equal(fs.existsSync(discoveryState),false);
  const app=path.join(installed,'pulsar'),state=path.join(root,'state'),env={PATH:'/usr/bin:/bin',PULSAR_STATE_DIR:state};
  const fd=fs.openSync(path.join(root,'engine.log'),'wx',0o600);engine=spawn(app,['engine'],{env,stdio:['ignore','ignore',fd]});fs.closeSync(fd);terminal=once(engine,'exit');
  const deadline=Date.now()+15000;while(!fs.existsSync(path.join(state,'engine.sock'))){assert.equal(engine.exitCode,null);assert.ok(Date.now()<deadline);await new Promise(r=>setTimeout(r,25));}
  const cli=args=>{const r=execute(app,args,env);assert.equal(r.status,0,r.stderr);return JSON.parse(r.stdout);};
  assert.equal(cli(['model','pack-selection-status']).value.selection_revision,0);
  const installedPack=discovered.packs[0].source;assert.equal(installedPack,path.join(installed,'generation',packHash));assert.equal(cli(['model','pack-import',installedPack]).value.active,false);
  assert.equal(cli(['model','pack-select',packHash,'--expected-selection-revision','0','--approve-unqualified-native-execution']).value.qualified,false);
  const source=path.join(root,'authored.y4m');fs.writeFileSync(source,Buffer.concat([Buffer.from('YUV4MPEG2 W64 H64 F2:1 Ip A1:1 Cmono\n'),...Array.from({length:4},(_,frame)=>Buffer.concat([Buffer.from('FRAME\n'),Buffer.from(Array.from({length:4096},(_,i)=>(i*31+frame*7)%256))]))]));
  r=execute(app,['generate',source,'--model-pack',packHash,'--width','640','--height','640','--output',path.join(root,'output.funscript')],env);assert.notEqual(r.status,0);assert.match(r.stderr,/unresolved evidence gaps/);assert.equal(fs.existsSync(path.join(root,'output.funscript')),false);
  const query=execute('/usr/bin/python3',['-c',"import sqlite3,json,sys;d=sqlite3.connect(sys.argv[1]);print(json.dumps(d.execute('SELECT project,source,state FROM jobs').fetchone()))",path.join(state,'projects.sqlite3')]);assert.equal(query.status,0,query.stderr);const job=JSON.parse(query.stdout);assert.equal(job[2],'completed');
  const preview=cli(['project','preview-pack',job[0],'500000000','--model-pack',packHash,'--source-version',job[1]]);assert.equal(preview.value.preview.analysis.status,'available');assert.equal(preview.value.qualified,false);
  fs.copyFileSync(preview.value.preview.frame.artifact.path,path.join(root,'preview-frame.raw'));
  fs.writeFileSync(path.join(root,'result.json'),JSON.stringify({runtime_manifest:manifest,runtime_sha256:hash,pack_sha256:packHash,job_state:job[2],preview,qualified:false},null,2));
  t.diagnostic('actual published generic weights: delivered, cached and executed; standard export refused missing motion evidence');
 }finally{
  if(engine){if(engine.exitCode===null&&engine.signalCode===null)engine.kill('SIGTERM');const timer=setTimeout(()=>engine.kill('SIGKILL'),2000);try{await terminal;}finally{clearTimeout(timer);}}
  fs.writeFileSync(path.join(root,'commands.json'),JSON.stringify(records,null,2));
  if(process.env.PULSAR_TEST_RESULT_DIR){
   const retained=process.env.PULSAR_TEST_RESULT_DIR;assert.ok(path.isAbsolute(retained));
   for(const name of ['result.json','commands.json','engine.log','preview-frame.raw'])if(fs.existsSync(path.join(root,name)))fs.copyFileSync(path.join(root,name),path.join(retained,name));
   const candidate=path.join(root,fs.existsSync(path.join(root,'hidden-candidate'))?'hidden-candidate':'candidate');
   if(!process.env.PULSAR_TEST_NO_CANDIDATE_COPY&&fs.existsSync(candidate))fs.cpSync(candidate,path.join(retained,'candidate'),{recursive:true,errorOnExist:true,force:false});
  }
  if(!process.env.PULSAR_TEST_KEEP_FIXTURE)fs.rmSync(root,{recursive:true,force:true});
 }
});
test('actual AppImage caches every generation payload and refuses damaged model bytes',{skip:!process.env.PULSAR_TEST_GENERATION_IMAGE,timeout:120000},t=>{
 const root=fs.mkdtempSync(path.join(os.tmpdir(),'pulsar-generation-image-'));fs.chmodSync(root,0o700);
 const tmp=path.join(root,'extract');fs.mkdirSync(tmp,{mode:0o700});const cache=path.join(root,'cache');
 const env={PATH:'/usr/bin:/bin',TMPDIR:tmp,APPIMAGE_EXTRACT_AND_RUN:'1',PULSAR_RUNTIME_CACHE_ROOT:cache};
 const records=[];const cli=args=>{const r=spawnSync(process.env.PULSAR_TEST_GENERATION_IMAGE,args,{env,encoding:'utf8',timeout:60000,maxBuffer:4*1024**2});assert.ifError(r.error);records.push({args,status:r.status,stdout:r.stdout,stderr:r.stderr});return r;};
 try{
  let r=cli(['--version']);assert.equal(r.status,0,r.stderr);assert.deepEqual(fs.readdirSync(tmp),[]);
  const hash=fs.readdirSync(cache).find(n=>/^[0-9a-f]{64}$/.test(n));assert.ok(hash);const installed=path.join(cache,hash),manifestBytes=fs.readFileSync(path.join(installed,'runtime-candidate.json')),manifest=JSON.parse(manifestBytes);assert.equal(sha(manifestBytes),hash);assert.equal(manifest.generation_packs.length,1);
  for(const record of manifest.files){const bytes=fs.readFileSync(path.join(installed,record.path));assert.equal(bytes.length,record.byte_len);assert.equal(sha(bytes),record.sha256);}
  r=cli(['runtime-cache-inspect',cache,hash]);assert.equal(r.status,0,r.stderr);assert.deepEqual(fs.readdirSync(tmp),[]);
  r=cli(['bundled-models']);assert.equal(r.status,0,r.stderr);const discovery=JSON.parse(r.stdout);assert.equal(discovery.runtime_manifest_sha256,hash);assert.equal(discovery.integrity_verified,true);assert.equal(discovery.qualified,false);assert.equal(discovery.native_execution_approved,false);assert.equal(discovery.packs.length,1);assert.deepEqual(fs.readdirSync(tmp),[]);
  const pack=manifest.generation_packs[0],model=pack.manifest.assets.find(a=>a.role==='model'),modelPath=path.join(installed,'generation',pack.manifest_sha256,model.path);fs.appendFileSync(modelPath,'damage');
  r=cli(['--version']);assert.notEqual(r.status,0);assert.equal(fs.statSync(modelPath).size,model.byte_len+6);assert.deepEqual(fs.readdirSync(tmp),[]);
  t.diagnostic(`actual AppImage: ${manifest.files.length} payloads verified; damaged model preserved/refused; extraction cleaned automatically`);
 }finally{
  if(process.env.PULSAR_TEST_IMAGE_RESULT)fs.writeFileSync(process.env.PULSAR_TEST_IMAGE_RESULT,JSON.stringify(records,null,2));
  fs.rmSync(root,{recursive:true,force:true});
 }
});
