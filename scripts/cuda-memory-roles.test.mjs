// Actual shipped dispatch refuses unconfined native observation/relay execution.
import test from 'node:test';
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
const worker=process.env.PULSAR_BENCHMARK_WORKER;
test('memory observation roles refuse direct unconfined execution before native loading', {skip:!worker},()=>{
  for(const [role,message] of [
    ['cuda-memory-observer',/CUDA bootstrap requires parent confinement/],
    ['cuda-memory-observer-exec',/CUDA execution requires its private sealed relay/],
  ]) {
    const result=spawnSync(worker,[role],{env:{},input:Buffer.alloc(0),timeout:2000,maxBuffer:65536});
    assert.equal(result.error,undefined);
    assert.equal(result.signal,null);
    assert.equal(result.status,1);
    assert.equal(result.stdout.length,0);
    assert.match(result.stderr.toString(),message);
  }
});
