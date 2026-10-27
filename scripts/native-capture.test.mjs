// Actual bwrap/cgroup capture, authored inputs and optional declared library set.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import crypto from 'node:crypto';
import {spawnSync} from 'node:child_process';
const worker=process.env.PULSAR_BENCHMARK_WORKER;
const inspector=process.env.PULSAR_NATIVE_CAPTURE_INSPECTOR;
test('supervised native capture preserves exact bytes and independently retires interrupted children',
  {skip:!worker||!inspector},t=>{
    const root=fs.mkdtempSync(path.join(os.tmpdir(),'pulsar-native-capture-'));
    t.diagnostic(`retained capture fixture: ${root}`);
    const record=(name,result)=>fs.writeFileSync(path.join(root,`${name}.process.json`),JSON.stringify({status:result.status,
      stdout:result.stdout,stderr:result.stderr,error:result.error?.message},null,2),{mode:0o600});
    const env={...process.env};delete env.PULSAR_WORKER_CONFINED;delete env.PULSAR_CUDA_OWNER_POLICY;
    const direct=spawnSync(worker,['native-capture-worker'],{env,encoding:'utf8',timeout:10000});record('direct',direct);
    assert.equal(direct.error,undefined);assert.notEqual(direct.status,0);assert.match(direct.stderr,/requires parent-owned confinement/);
    const source=path.join(root,'source');const bytes=Buffer.from('original native bytes');fs.writeFileSync(source,bytes);
    const hash=crypto.createHash('sha256').update(bytes).digest('hex');
    const spec={schema_version:1,libraries:[{path:source,sha256:hash,byte_len:bytes.length,names:['libfixture.so','libfixture.so.1']}]};
    function inspect(name,declaration,timeout=10000,cancel=-1) {
      const specPath=path.join(root,`${name}.spec.json`);fs.writeFileSync(specPath,JSON.stringify(declaration),{mode:0o600});
      const output=path.join(root,name);
      const result=spawnSync(inspector,[worker,specPath,output,String(timeout),String(cancel)],
        {env,encoding:'utf8',timeout:120000,maxBuffer:1024*1024});record(name,result);
      assert.equal(result.error,undefined);assert.equal(result.status,0,result.stderr);
      const checked=JSON.parse(result.stdout);assert.equal(checked.cleanup_confirmed,true);assert.equal(checked.storage_after_cleanup,0);
      assert.equal(checked.gpu_execution,false);assert.equal(checked.captured_library_loading,false);
      assert.equal(fs.readdirSync(path.join(output,'captures')).length,0);
      return checked;
    }
    const before=[worker,inspector].map(p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex'));
    const complete=inspect('complete',spec);assert.equal(complete.result.state,'captured');
    assert.equal(complete.result.independent_byte_readback,true);assert.equal(complete.result.cpu_worker_slot_released,true);
    assert.equal(complete.result.completion.declared_bytes,bytes.length);
    const wrong=structuredClone(spec);wrong.libraries[0].sha256='f'.repeat(64);
    assert.equal(inspect('wrong-hash',wrong).result.error.code,'dependency_mismatch');
    const link=path.join(root,'input-symlink');fs.symlinkSync(source,link);const symlink=structuredClone(spec);symlink.libraries[0].path=link;
    assert.equal(inspect('symlink',symlink).result.error.code,'invalid_request');
    const fifo=path.join(root,'input-fifo');const made=spawnSync('mkfifo',['-m','600',fifo],{encoding:'utf8'});assert.equal(made.status,0,made.stderr);
    const pipe=structuredClone(spec);pipe.libraries[0].path=fifo;
    assert.equal(inspect('fifo',pipe).result.error.code,'invalid_request');
    assert.equal(inspect('pre-cancelled',spec,10000,0).result.error.code,'cancelled');
    assert.equal(inspect('invalid-deadline',spec,3600001).result.error.code,'invalid_request');
    const realPath=process.env.PULSAR_NATIVE_LIBRARY_SPEC;
    if(realPath) {
      const real=JSON.parse(fs.readFileSync(realPath,'utf8'));const total=real.libraries.reduce((n,l)=>n+l.byte_len,0);
      const complete=inspect('declared-real-image',real,60000);assert.equal(complete.result.state,'captured');
      assert.equal(complete.result.completion.declared_bytes,total);
      const deadline=inspect('deadline',real,250);assert.equal(deadline.result.state,'refused');
      assert.equal(deadline.result.error.code,'unavailable');assert.match(deadline.result.error.message,/deadline/);
      const cancelled=inspect('cancelled',real,60000,250);assert.equal(cancelled.result.state,'refused');
      assert.equal(cancelled.result.error.code,'cancelled');assert.equal(cancelled.cancel_requested,true);
      t.diagnostic(JSON.stringify({declared_libraries:real.libraries.length,declared_bytes:total,
        full_capture_ms:complete.capture_elapsed_ms,deadline_return_ms:deadline.capture_elapsed_ms,cancel_return_ms:cancelled.capture_elapsed_ms,
        gpu_execution:false,scope:'expected-byte capture and child-tree retirement; no execution image qualification'}));
    }
    assert.deepEqual([worker,inspector].map(p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex')),before);
  });
