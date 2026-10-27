// Authored shipped CLI -> authenticated Engine -> measured owned CUDA worker.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import {spawn,spawnSync} from 'node:child_process';
const pause=()=>new Promise(resolve=>setTimeout(resolve,10));
const hash=file=>{const fd=fs.openSync(file,'r'),h=crypto.createHash('sha256'),b=Buffer.alloc(1048576);try{for(;;){const n=fs.readSync(fd,b);if(!n)break;h.update(b.subarray(0,n));}}finally{fs.closeSync(fd);}return h.digest('hex');};
const configured=process.env.PULSAR_ENGINE_CUDA_WORKER&&process.env.PULSAR_NATIVE_LIBRARY_SPEC&&process.env.PULSAR_NVML_LIBRARY&&process.env.PULSAR_AUTHORED_CUDA_MODEL;
test('shipped CLI generates authored CUDA motion with retained original evidence and no physical dispatch',{skip:!configured,timeout:120000},async t=>{
 const worker=fs.realpathSync(process.env.PULSAR_ENGINE_CUDA_WORKER);
 const root=fs.mkdtempSync(path.join(os.tmpdir(),'pcg-'));fs.chmodSync(root,0o700);t.diagnostic(`retained authored shipped CUDA evidence: ${root}`);
 const spec=JSON.parse(fs.readFileSync(process.env.PULSAR_NATIVE_LIBRARY_SPEC));
 const nvml=fs.realpathSync(process.env.PULSAR_NVML_LIBRARY);spec.libraries.push({path:nvml,sha256:hash(nvml),byte_len:fs.statSync(nvml).size,names:['libnvidia-ml.so.1']});
 const template=JSON.parse(fs.readFileSync('crates/pulsar-engine/tests/fixtures/cuda-intent-request.json'));
 const device=template.cuda_execution;
 const policy={schema_version:1,device:{device_uuid:device.device_uuid,pci_bus_address:device.pci_bus_address,device_minor:device.device_minor},accounting:{device_uuid:device.device_uuid,device_ordinal:device.device_ordinal,maximum_reserved_bytes:128*1024**2,interactive_reserved_bytes:64*1024**2,control_reserve_bytes:256*1024**2},native_dependencies:spec};
 const policyPath=path.join(root,'policy.json');fs.writeFileSync(policyPath,JSON.stringify(policy),{mode:0o600});
 const model=fs.realpathSync(process.env.PULSAR_AUTHORED_CUDA_MODEL),runtime=spec.libraries.find(lib=>lib.names.includes('libonnxruntime.so.1'));
 const profile=template.operation.arguments.settings.inference_profile;
 profile.model={artifact_id:hash(model),sha256:hash(model),byte_len:fs.statSync(model).size};profile.runtime={artifact_id:runtime.sha256,sha256:runtime.sha256,byte_len:runtime.byte_len};
 const profilePath=path.join(root,'profile.json');fs.writeFileSync(profilePath,JSON.stringify(profile));
 const source=path.join(root,'pair.rgb');fs.writeFileSync(source,Buffer.concat([Buffer.alloc(64*64*3),Buffer.alloc(64*64*3,128)]));
 const pair=path.join(root,'pair.json');fs.writeFileSync(pair,JSON.stringify({source_epoch:'authored-cli-pair',previous_frame:0,current_frame:1,previous_time:{numerator:0,denominator:1},current_time:{numerator:1,denominator:10}}));
 const state=path.join(root,'state'),env={...process.env,PULSAR_STATE_DIR:state,PULSAR_ORT_DYLIB:runtime.path,PULSAR_CUDA_OWNER_POLICY:policyPath};
 const log=fs.openSync(path.join(root,'engine.stderr'),'wx',0o600);const child=spawn(worker,['engine'],{env,stdio:['ignore','ignore',log]});fs.closeSync(log);
 let ended=false;const stopped=new Promise(resolve=>{child.once('error',error=>{ended=true;resolve({error:String(error)});});child.once('exit',(code,signal)=>{ended=true;resolve({code,signal});});});
 t.after(async()=>{if(!ended)child.kill('SIGTERM');await stopped;});
 const until=Date.now()+15000;while(!fs.existsSync(path.join(state,'engine.sock'))){assert.equal(ended,false,fs.readFileSync(path.join(root,'engine.stderr'),'utf8'));assert.ok(Date.now()<until,'Engine startup timeout');await pause();}
 let n=0;const cli=args=>{const r=spawnSync(worker,args,{env,encoding:'utf8',timeout:60000,maxBuffer:4*1024*1024});fs.writeFileSync(path.join(root,`cli-${n++}.json`),JSON.stringify({args,status:r.status,error:r.error?.message,stdout:r.stdout,stderr:r.stderr},null,2),{mode:0o600});assert.ifError(r.error);assert.equal(r.status,0,r.stderr);return r;};
 const catalog=JSON.parse(cli(['model','status']).stdout);assert.ok(catalog.providers.includes('cuda'));assert.deepEqual(catalog.qualified_profiles,[]);assert.match(catalog.limitation,/automatic GPU recovery remains unavailable/);
 const output=path.join(root,'authored.funscript');cli(['generate',source,'--model',model,'--inference-profile',profilePath,'--raw-frame-pair',pair,'--output',output]);
 assert.deepEqual(JSON.parse(fs.readFileSync(output)).actions,[{at:0,pos:50},{at:100,pos:38}]);
 const attempts=path.join(state,'attempts'),names=fs.readdirSync(attempts);assert.equal(names.length,1);const attempt=path.join(attempts,names[0]);
 const receipt=JSON.parse(fs.readFileSync(path.join(attempt,'cuda-inference.json'))),trace=JSON.parse(fs.readFileSync(path.join(attempt,'cuda-trace.json'))),general=JSON.parse(fs.readFileSync(path.join(attempt,'receipt.json')));
 assert.equal(receipt.provider,'cuda');assert.equal(receipt.provider_evidence,'strict-cuda-node-trace');assert.equal(receipt.successful_runs,1);
 const nodes=trace.filter(event=>event.cat==='Node');assert.equal(nodes.length,3);assert.ok(nodes.every(node=>node.args.provider==='CUDAExecutionProvider'));
 assert.equal(general.physical_dispatch,false);assert.equal(general.qualification,'unqualified');
 const deadline=Date.now()+5000;for(;;){const ledger=JSON.parse(fs.readFileSync(path.join(state,'worker-fence.json')));if(['groups','retired','staging'].every(name=>Object.keys(ledger[name]).length===0)&&Object.keys(ledger.creation.records).length===0)break;assert.ok(Date.now()<deadline,'CUDA ownership not retired');await pause();}
 if(process.env.PULSAR_ENGINE_CUDA_CLI_EVIDENCE){const dest=path.resolve(process.env.PULSAR_ENGINE_CUDA_CLI_EVIDENCE);assert.equal(fs.existsSync(dest),false);fs.mkdirSync(dest,{recursive:true,mode:0o700});for(const name of ['program.json','receipt.json','cuda-inference.json','cuda-trace.json','initial-checkpoint.json'])fs.copyFileSync(path.join(attempt,name),path.join(dest,name));fs.copyFileSync(output,path.join(dest,'authored.funscript'));fs.writeFileSync(path.join(dest,'checked.json'),JSON.stringify({scope:'shipped authored CUDA CLI generation only',root,physical_dispatch:false,model_quality_qualified:false,cuda_computation_nodes:nodes.length,worker_sha256:hash(worker)},null,2));}
});
