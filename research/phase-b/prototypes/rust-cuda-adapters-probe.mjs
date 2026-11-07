// Authored namespace experiment; no production jobs or private media.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import {authoredOnnxFixture} from '../../../scripts/benchmark-onnx-fixture.mjs';
const [gateArg, probeArg, specArg, kernelArg, sdkArg, rootArg] = process.argv.slice(2);
assert.equal(process.argv.length, 8, 'GATE RUST-PROBE SPEC KERNEL-EXPECTATION SDK-EXPECTATION NEW-ROOT');
const gate=fs.realpathSync(gateArg),probe=fs.realpathSync(probeArg),spec=fs.realpathSync(specArg);
const kernel=fs.realpathSync(kernelArg), sdk=fs.realpathSync(sdkArg);
fs.mkdirSync(rootArg,{mode:0o700});const root=fs.realpathSync(rootArg);
const captures=path.join(root,'captures');fs.mkdirSync(captures,{mode:0o700});
const identity=p=>({sha256:crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex'),byte_len:fs.statSync(p).size});
const before=Object.fromEntries([gate,probe,spec,kernel,sdk].map(p=>[p,identity(p)]));
const model=path.join(root,'authored.onnx');fs.writeFileSync(model,authoredOnnxFixture({temporal:true}),{flag:'wx'});
// One string operator has only CPU placement. This graph must fail the strict
// provider gate before the adapter's subsequent float32 contract check.
function cpuOnlyGraph() {
  const join=(...x)=>Buffer.concat(x.flat());
  const v=n=>{n=BigInt(n);const b=[];do{const x=Number(n&127n);n>>=7n;b.push(x|(n?128:0));}while(n);return Buffer.from(b);};
  const i=(f,n)=>join(v(f*8),v(n));const m=(f,b)=>join(v(f*8+2),v(b.length),b);const s=(f,t)=>m(f,Buffer.from(t));
  const info=n=>join(s(1,n),m(2,m(1,join(i(1,8),m(2,m(1,i(1,1)))))));
  const node=join(s(1,'frame_pair'),s(2,'script_position'),s(3,'cpu-only-string-node'),s(4,'StringNormalizer'));
  const graph=join(m(1,node),s(2,'pulsar-authored-cpu-only-refusal'),m(11,info('frame_pair')),m(12,info('script_position')));
  return join(i(1,8),s(2,'pulsar-authored-refusal'),m(7,graph),m(8,i(2,13)));
}
const cpuModel=path.join(root,'cpu-only.onnx');fs.writeFileSync(cpuModel,cpuOnlyGraph(),{flag:'wx'});
const detectorModel=path.join(root,'detector.onnx');fs.writeFileSync(detectorModel,authoredOnnxFixture(),{flag:'wx'});
const invalidModel=path.join(root,'invalid-output.onnx');const invalid=Buffer.from(fs.readFileSync(model));const base=Buffer.alloc(4);base.writeFloatLE(0.25);const offset=invalid.indexOf(base);assert(offset>=0);invalid.writeFloatLE(1.25,offset);fs.writeFileSync(invalidModel,invalid,{flag:'wx'});
const invalidDetector=path.join(root,'invalid-detector.onnx');const invalidBoxes=Buffer.from(fs.readFileSync(detectorModel));const width=Buffer.alloc(4);width.writeFloatLE(20);const boxOffset=invalidBoxes.indexOf(width);assert(boxOffset>=0);invalidBoxes.writeFloatLE(-20,boxOffset);fs.writeFileSync(invalidDetector,invalidBoxes,{flag:'wx'});
const ordinal=path.join(root,'ordinal.json');fs.writeFileSync(ordinal,JSON.stringify({...JSON.parse(fs.readFileSync(sdk)),device_ordinal:63})+'\n',{flag:'wx'});
const cases=[];
for(const c of [
  {name:'temporal-rgb',model,sdk,pass:true},
  {name:'detector-rgb',model:detectorModel,sdk,mode:'detector',pass:true},
  {name:'unavailable-ordinal',model,sdk:ordinal,pattern:/outside the visible SDK inventory/},
  {name:'cpu-only',model:cpuModel,sdk,pattern:/CPU EP|CPU execution provider|CPU fallback/i},
  {name:'invalid-input',model,sdk,mode:'invalid-input',pattern:/CUDA_ADAPTER_REUSE_AND_RECEIPT_REFUSED/},
  {name:'invalid-output',model:invalidModel,sdk,mode:'invalid-output',pattern:/CUDA_ADAPTER_REUSE_AND_RECEIPT_REFUSED/},
  {name:'invalid-detector-output',model:invalidDetector,sdk,mode:'invalid-detector-output',pattern:/CUDA_ADAPTER_REUSE_AND_RECEIPT_REFUSED/},
  {name:'preinitialized-runtime',model,sdk,mode:'preinitialized-runtime',pattern:/already initialized; a new attempt process is required/}
]) {
  const output=path.join(root,c.name);const argv=[spec,captures,probe,c.model,output,kernel,c.sdk];
  if(c.mode) argv.push(c.mode);
  const result=spawnSync(gate,argv,{env:{TMPDIR:root},encoding:'utf8',timeout:90_000,maxBuffer:2*1024*1024});
  fs.writeFileSync(path.join(root,`${c.name}.stdout`),result.stdout??'');fs.writeFileSync(path.join(root,`${c.name}.stderr`),result.stderr??'');
  assert.ifError(result.error);assert.equal(result.signal,null);
  const childStderr=fs.readFileSync(path.join(output,'stderr.log'),'utf8');
  if(c.pass) {
    assert.equal(result.status,0,result.stderr+'\n'+childStderr);
    const checked=JSON.parse(fs.readFileSync(path.join(output,'checked.json')));
    assert.equal(checked.oracle_passed,true);assert.equal(checked.strict_trace.computation_events,12);
    assert.equal(checked.rust_session.successful_runs,4);assert.equal(checked.rust_session.trace.trace_sha256,checked.strict_trace.trace_sha256);
    assert.equal(checked.capture.capture_cleanup,'closed_successfully');
    const parent=JSON.parse(fs.readFileSync(path.join(output,'parent-egress-checked.json')));
    assert.equal(parent.trace.path,'/output/cuda-trace.json');
    assert.equal(parent.trace.identity.sha256,checked.strict_trace.trace_sha256);
    assert.equal(identity(path.join(output,'cuda-trace.json')).sha256,parent.trace.identity.sha256);
  } else {
    assert.notEqual(result.status,0);assert.match(childStderr,c.pattern);
    assert.equal(fs.readFileSync(path.join(output,'stdout.json'),'utf8'),'');
    assert(!fs.existsSync(path.join(output,'checked.json')));
  }
  assert.deepEqual(fs.readdirSync(captures),[],'capture cleanup after terminal child');
  cases.push({name:c.name,argv,exit_code:result.status,expected:c.pass?'strict_cuda_success':'refusal',cleanup_readback:true});
}
const after=Object.fromEntries(Object.keys(before).map(p=>[p,identity(p)]));assert.deepEqual(after,before);
fs.writeFileSync(path.join(root,'checked.json'),JSON.stringify({scope:'authored Rust CUDA namespace adapter, not production job admission',before,after,model:identity(model),cpu_only_model:identity(cpuModel),cases},null,2)+'\n',{flag:'wx'});
