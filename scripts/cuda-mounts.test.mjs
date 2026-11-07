// Actual descriptor mounts and hash checks; no native inference or GPU device IO.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import crypto from 'node:crypto';
import {spawn} from 'node:child_process';

const hash=file=>{
  const fd=fs.openSync(file,'r'),digest=crypto.createHash('sha256'),buffer=Buffer.alloc(1048576);
  try {for(;;){const n=fs.readSync(fd,buffer);if(!n)break;digest.update(buffer.subarray(0,n));}}finally{fs.closeSync(fd);}
  return digest.digest('hex');
};
test('confined CUDA mount inspection pins aliases and refuses altered mounted identities and bytes',
 {skip:process.platform!=='linux'||!process.env.PULSAR_CUDA_MOUNT_INSPECTOR||!process.env.PULSAR_CUDA_DEVICE_EXPECTATION||!process.env.PULSAR_NATIVE_LIBRARY_SPEC||!process.env.PULSAR_STASH_TEST_BINARY,timeout:120000},async t=>{
  const helper=fs.realpathSync(process.env.PULSAR_CUDA_MOUNT_INSPECTOR),binary=fs.realpathSync(process.env.PULSAR_STASH_TEST_BINARY);
  const spec=fs.realpathSync(process.env.PULSAR_NATIVE_LIBRARY_SPEC),device=fs.realpathSync(process.env.PULSAR_CUDA_DEVICE_EXPECTATION);
  const subjects=[helper,binary,spec,device];const before=subjects.map(hash);
  const root=fs.mkdtempSync(path.join(os.tmpdir(),'pulsar-cuda-mounts-'));fs.chmodSync(root,0o700);
  const args=[binary,spec,device,path.join(root,'inspection')];
  const child=spawn(helper,args,{stdio:['ignore','pipe','pipe']});let stdout='',stderr='',timedOut=false;
  const timer=setTimeout(()=>{timedOut=true;child.kill('SIGKILL');},110000);
  for(const [stream,name] of [[child.stdout,'stdout'],[child.stderr,'stderr']])stream.on('data',bytes=>{
    if(name==='stdout')stdout+=bytes.toString();else stderr+=bytes.toString();
    if(Buffer.byteLength(stdout)+Buffer.byteLength(stderr)>1048576)child.kill('SIGKILL');
  });
  let result;try {result=await new Promise((resolve,reject)=>{child.once('error',reject);child.once('exit',(code,signal)=>resolve({code,signal}));});}finally{clearTimeout(timer);}
  assert.equal(timedOut,false,'inspection helper deadline');assert.equal(result.code,0,stderr);assert.equal(result.signal,null);
  const checked=JSON.parse(stdout);assert.equal(checked.state,'passed');assert.equal(checked.native_execution,false);
  assert.equal(checked.production_gpu_admission,false);assert.equal(checked.capture_removed,true);
  assert.deepEqual(checked.cases.map(c=>c.case),['baseline','replacement','wrong-alias-inode','wrong-device-inode','corrupt-backing']);
  for(const [i,c] of checked.cases.entries()){
    assert.equal(c.tree_empty,true);assert.equal(c.exit_code,i<2?0:1);
    if(i<2){assert.equal(c.checked.device_nodes,3);assert.equal(c.checked.native_execution,false);assert.equal(c.checked.gpu_admission,false);assert.ok(c.checked.aliases>0);}
  }
  assert.match(checked.cases[2].stderr,/mounted native alias differs/);
  assert.match(checked.cases[3].stderr,/mounted CUDA node differs/);
  assert.match(checked.cases[4].stderr,/mounted native alias hash differs/);
  assert.deepEqual(fs.readdirSync(path.join(root,'inspection','capture')),[]);
  assert.deepEqual(subjects.map(hash),before);
  const libraries=JSON.parse(fs.readFileSync(spec,'utf8')).libraries;
  for(const library of libraries){assert.equal(fs.statSync(library.path).size,library.byte_len);assert.equal(hash(library.path),library.sha256);}
  const evidence={...checked,command:{argv:[helper,...args]},subjects:subjects.map((p,i)=>({path:p,sha256:before[i]})),native_library_source_readbacks:libraries.length};
  if(process.env.PULSAR_CUDA_MOUNT_EVIDENCE){fs.mkdirSync(process.env.PULSAR_CUDA_MOUNT_EVIDENCE,{mode:0o700});fs.writeFileSync(path.join(process.env.PULSAR_CUDA_MOUNT_EVIDENCE,'checked.json'),JSON.stringify(evidence,null,2)+'\n',{flag:'wx',mode:0o600});}
  t.diagnostic(JSON.stringify({root,scope:checked.scope,cases:checked.cases.length,native_library_source_readbacks:libraries.length,native_execution:false}));
});
