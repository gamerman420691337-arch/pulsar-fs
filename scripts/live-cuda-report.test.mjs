// Compiled preceding/current report readers. Authored metadata, no native execution.
import test from 'node:test';import assert from 'node:assert/strict';import fs from 'node:fs';import crypto from 'node:crypto';import path from 'node:path';import {spawnSync} from 'node:child_process';
const preceding=process.env.PULSAR_PRECEDING_CAPTURE_READER,current=process.env.PULSAR_CURRENT_CAPTURE_READER;
test('CPU capture bytes remain equal; CUDA reports are provisional and provider-bound',{skip:!preceding||!current},()=>{
 const input=fs.readFileSync('crates/pulsar-protocol/tests/fixtures/live-cpu-capture-report.json');
 const run=(binary,value,args=[])=>{const r=spawnSync(binary,args,{input:Buffer.isBuffer(value)?value:JSON.stringify(value),encoding:'utf8',timeout:10000});assert.ifError(r.error);return r;};
 for(const binary of [preceding,current]){const r=run(binary,input);assert.equal(r.status,0,r.stderr);assert.equal(r.stdout,input.toString());}
 const gpu=JSON.parse(input);const p=gpu.options.inference.profile;p.schema_version=2;p.provider='cuda';p.cuda_resources={device_ordinal:0,device_uuid:'GPU-12345678-1234-1234-1234-123456789abc',address_space_bytes:8589934592,device_reservation_bytes:100663296,arena_limit_bytes:67108864};
 const encoded=run(current,gpu,['--profile-bytes']);assert.equal(encoded.status,0,encoded.stderr);const hash=crypto.createHash('sha256').update(encoded.stdout).digest('hex');
 gpu.inference.profile_sha256=hash;gpu.inference.cuda_manifest_sha256='b'.repeat(64);const latest=gpu.inference.latest;latest.result.status='cuda_provisional';latest.result.payload.profile_sha256=hash;latest.result.payload.manifest_sha256='b'.repeat(64);
 const accepted=run(current,gpu);assert.equal(accepted.status,0,accepted.stderr);
 const refused=run(preceding,gpu);assert.notEqual(refused.status,0);assert.equal(refused.stdout,'');
 // Reproduce the old metadata validation gap independently of the new fields.
 const disguise=structuredClone(gpu);delete disguise.inference.cuda_manifest_sha256;disguise.inference.latest.result.status='estimated';delete disguise.inference.latest.result.payload.manifest_sha256;
 const oldDisguise=run(preceding,disguise);assert.equal(oldDisguise.status,0,oldDisguise.stderr);
 const fixedDisguise=run(current,disguise);assert.notEqual(fixedDisguise.status,0);assert.equal(fixedDisguise.stdout,'');
 const cases=[];
 for(const name of ['missing-binding','changed-binding','qualified','foreign-profile','zero-duration','wrong-age']){
  const v=structuredClone(gpu);
  if(name==='missing-binding')delete v.inference.cuda_manifest_sha256;
  if(name==='changed-binding')v.inference.cuda_manifest_sha256='c'.repeat(64);
  if(name==='qualified')v.inference.latest.qualified=true;
  if(name==='foreign-profile')v.inference.latest.result.payload.profile_sha256='c'.repeat(64);
  if(name==='zero-duration')v.inference.latest.result.payload.pair_analysis_elapsed_ns=0;
  if(name==='wrong-age')v.inference.latest.capture_age_ns++;
  const r=run(current,v);assert.notEqual(r.status,0,name);assert.equal(r.stdout,'');cases.push(name);
 }
 if(process.env.PULSAR_CAPTURE_REPORT_EVIDENCE){const dest=path.resolve(process.env.PULSAR_CAPTURE_REPORT_EVIDENCE);assert.equal(fs.existsSync(dest),false);fs.mkdirSync(dest,{recursive:true});fs.writeFileSync(path.join(dest,'cpu.json'),input);fs.writeFileSync(path.join(dest,'cuda-provisional.json'),accepted.stdout);fs.writeFileSync(path.join(dest,'cpu-disguise.json'),JSON.stringify(disguise));fs.writeFileSync(path.join(dest,'checked.json'),JSON.stringify({cpu_bytes_equal:true,preceding_reader_refuses_cuda:true,preceding_reader_accepts_disguise:true,current_reader_refuses_disguise:true,negative_cases:cases,native_execution:false},null,2));}
});
