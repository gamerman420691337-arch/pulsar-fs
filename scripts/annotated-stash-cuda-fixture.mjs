// Authored native verification only; no quality or physical qualification.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
const hash = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
export const cudaPolicy = process.env.PULSAR_ANNOTATED_BATCH_CUDA_POLICY;
export function configureCudaFixture(root, env) {
  if (!cudaPolicy) return;
  const policy = JSON.parse(fs.readFileSync(cudaPolicy));
  for (const library of policy.native_dependencies.libraries) {
    const bytes = fs.readFileSync(library.path);
    assert.equal(bytes.length, library.byte_len);
    assert.equal(hash(bytes), library.sha256);
  }
  const runtime = policy.native_dependencies.libraries.find(l => l.names.includes('libonnxruntime.so.1'));
  assert.ok(runtime);
  assert.equal(env.PULSAR_ORT_DYLIB, fs.realpathSync(runtime.path));
  env.PULSAR_CUDA_OWNER_POLICY = path.join(root, 'policy.json');
  fs.writeFileSync(env.PULSAR_CUDA_OWNER_POLICY, JSON.stringify(policy), {mode: 0o600});
}
export function promoteCudaFixtureProfile(profile) {
  if (!cudaPolicy) return;
  const template = JSON.parse(fs.readFileSync(new URL('../crates/pulsar-engine/tests/fixtures/cuda-intent-request.json', import.meta.url)))
    .operation.arguments.settings.inference_profile;
  profile.schema_version = 2;
  profile.provider = 'cuda';
  profile.cuda_resources = template.cuda_resources;
}
export function collectCudaEvidence(state, attempts) {
  if (!cudaPolicy) return [];
  return attempts.map(attempt => {
    const directory = path.join(state, 'attempts', attempt);
    const receiptBytes = fs.readFileSync(path.join(directory, 'cuda-inference.json'));
    const traceBytes = fs.readFileSync(path.join(directory, 'cuda-trace.json'));
    const receipt = JSON.parse(receiptBytes), nodes = JSON.parse(traceBytes).filter(e => e.cat === 'Node');
    assert.equal(receipt.provider, 'cuda');
    assert.equal(receipt.successful_runs, 27);
    assert.ok(nodes.length > 0);
    assert.ok(nodes.every(node => node.args.provider === 'CUDAExecutionProvider'));
    return {attempt, successful_runs: receipt.successful_runs, cuda_nodes: nodes.length,
      receipt_sha256: hash(receiptBytes), trace_sha256: hash(traceBytes)};
  });
}
export function verifyOriginalCudaEvidence(state, evidence) {
  for (const item of evidence) {
    const directory = path.join(state, 'attempts', item.attempt);
    assert.equal(hash(fs.readFileSync(path.join(directory, 'cuda-inference.json'))), item.receipt_sha256);
    assert.equal(hash(fs.readFileSync(path.join(directory, 'cuda-trace.json'))), item.trace_sha256);
  }
}
