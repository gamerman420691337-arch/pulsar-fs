import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { pathToFileURL } from 'node:url';
import { stageBenchmarkInputs } from './benchmark-intake.mjs';
import { evaluateClip, validateAnnotation } from './benchmark-good.mjs';
import { bindHumanEffort } from './benchmark-effort.mjs';
import { snapshotRunner, runAttempt } from './benchmark-process.mjs';

const sha = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
function readBounded(filename, limit) {
  const fd = fs.openSync(filename, fs.constants.O_RDONLY | fs.constants.O_NOFOLLOW | fs.constants.O_NONBLOCK);
  try {
    const stat = fs.fstatSync(fd);
    if (!stat.isFile() || stat.size > limit) throw new Error('input exceeds regular-file bound');
    const bytes = Buffer.alloc(stat.size + 1);
    let count = 0;
    while (count < bytes.length) {
      const n = fs.readSync(fd, bytes, count, bytes.length - count, null);
      if (!n) break;
      count += n;
    }
    if (count !== stat.size) throw new Error('input changed during read');
    return bytes.subarray(0, count);
  } finally { fs.closeSync(fd); }
}
function publish(filename, value) {
  const fd = fs.openSync(filename, 'wx', 0o600);
  try {
    fs.writeFileSync(fd, JSON.stringify(value, null, 2) + '\n');
    fs.fsyncSync(fd);
  } finally { fs.closeSync(fd); }
  const directory = fs.openSync(path.dirname(filename), fs.constants.O_RDONLY);
  try { fs.fsyncSync(directory); } finally { fs.closeSync(directory); }
}

// Read-only recovery inventory. An absent result is unknown, never a success or
// permission to relaunch a process whose outcome has not been reconciled.
export function inspectBenchmarkCampaign(directory) {
  const start = JSON.parse(readBounded(path.join(directory,'campaign-start.json'),16*1024*1024));
  if (start.schema_version !== 1 || !Array.isArray(start.intake?.entries)) throw new Error('invalid campaign start receipt');
  const results = start.intake.entries.map((entry, index) => {
    try {
      const result = JSON.parse(readBounded(path.join(directory,`result-${index}.json`),8*1024*1024));
      if (result.subject_id !== entry.declaration.id) throw new Error('result subject mismatch');
      if (result.error || result.exit_code !== 0) return {subject_id:result.subject_id,state:'failed',score:null};
      if (!result.score) throw new Error('successful result lacks score');
      return {subject_id:result.subject_id,state:'scored',score:result.score};
    } catch (error) {
      return {subject_id:entry.declaration.id,state:'unknown',score:null,
        reason:error.code === 'ENOENT' ? 'no durable result; may be active or interrupted' : `unreadable result: ${error.message}`};
    }
  });
  return {schema_version:1,runner_sha256:start.runner_sha256,results,
    automatic_retry_permitted:false,release_qualified:false};
}

// Runner path is an explicit trusted local executable, never shell source.
export async function runBenchmarkCampaign(config, { signal } = {}) {
  config = structuredClone(config);
  const timeout = config.attempt_timeout_ms;
  if (!Number.isSafeInteger(timeout) || timeout < 1000 || timeout > 3600000)
    throw new Error('attempt timeout outside 1s..1h');
  if (!Number.isSafeInteger(config.generation?.budget?.wall_time_ms)
      || config.generation.budget.wall_time_ms < 1 || config.generation.budget.wall_time_ms > timeout - 500)
    throw new Error('worker deadline must leave supervisor shutdown margin');
  const runner = snapshotRunner(config.runner, config.runner_sha256);
  try {
    const staged = stageBenchmarkInputs(config.intake);
    publish(path.join(staged.directory,'campaign-start.json'), {
      schema_version:1,runner_sha256:config.runner_sha256,intake:staged.receipt,
      attempt_timeout_ms:timeout,generation:config.generation,targets:config.targets,release_qualified:false,
    });
    const results = [];
    for (let index = 0; index < staged.receipt.entries.length; index++) {
      const entry = staged.receipt.entries[index];
      const referenceBytes = readBounded(path.join(staged.directory,entry.reference.path),8*1024*1024);
      if (sha(referenceBytes) !== entry.reference.sha256) throw new Error('staged reference changed');
      const reference = JSON.parse(referenceBytes);
      validateAnnotation(reference,'reference');
      const target = config.targets?.[entry.declaration.id];
      if (typeof target !== 'string' || !target.length) throw new Error('explicit candidate target mapping required');
      const job = { ...config.generation,
        source: { source_version:`campaign-source-${index}`,path:path.join(staged.directory,entry.source.path),
          identity:{artifact_id:entry.source.sha256,sha256:entry.source.sha256,byte_len:entry.source.byte_len} },
        subject:entry.declaration.id,target,
        window_start_ns:reference.window.start_ns,window_end_ns:reference.window.end_ns,
      };
      const input = path.join(staged.directory,`job-${index}.json`);
      const output = path.join(staged.directory,`attempt-${index}`);
      publish(input,job);
      const started = process.hrtime.bigint();
      const child = await runAttempt(runner.path,[input,output],{
        timeout,signal,logPrefix:path.join(staged.directory,`runner-${index}`),
        environment:{PATH:'/usr/bin:/bin',TMPDIR:staged.directory,...(process.env.PULSAR_CGROUP_ROOT ? {PULSAR_CGROUP_ROOT:process.env.PULSAR_CGROUP_ROOT}: {})},
      });
      const result = {subject_id:entry.declaration.id,wall_ns:(process.hrtime.bigint()-started).toString(),
        exit_code:child.status,signal:child.signal,error:child.error ?? null,
        stdout:child.stdout?.toString() ?? '',stderr:child.stderr?.toString() ?? '',score:null};
      if (child.status === 0 && !child.error) {
        try {
          const converted = JSON.parse(readBounded(path.join(output,'candidate.json'),8*1024*1024));
          validateAnnotation(converted.candidate,'candidate');
          result.score = evaluateClip(reference,converted.candidate);
          result.adapter_receipt = converted.receipt;
          const effort = config.human_effort?.[entry.declaration.id];
          if (effort) {
            const evidence = readBounded(effort.evidence_path,8*1024*1024);
            result.human_effort = bindHumanEffort(effort.record, {
              subject_id:entry.declaration.id,source_sha256:entry.source.sha256,
              candidate_sha256:converted.receipt.raw_sha256,
            },evidence);
            const evidencePath = `human-effort-${index}.bin`;
            fs.writeFileSync(path.join(staged.directory,evidencePath),evidence,{flag:'wx',mode:0o600});
            result.human_effort.evidence = {path:evidencePath,sha256:sha(evidence),byte_len:evidence.length};
          }
        } catch (error) {
          result.score = null;
          delete result.human_effort;
          result.error = `candidate/scoring failure: ${error.message}`;
        }
      }
      publish(path.join(staged.directory,`result-${index}.json`),result);
      results.push(result);
    }
    const suppliedEffort = results.filter(result => result.human_effort).map(result => result.human_effort);
    const receipt = {schema_version:1,runner_sha256:config.runner_sha256,
      intake:staged.receipt,results,human_effort:suppliedEffort.length ? suppliedEffort : null,release_qualified:false};
    publish(path.join(staged.directory,'campaign.json'),receipt);
    return {directory:staged.directory,receipt};
  } finally { runner.dispose(); }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    if (process.argv.length === 4 && process.argv[2] === '--inspect') {
      const report = inspectBenchmarkCampaign(process.argv[3]);
      console.log(JSON.stringify(report,null,2));
      if (report.results.some(result => result.state !== 'scored')) process.exitCode = 1;
    } else {
      if (process.argv.length !== 3) throw new Error('usage: node benchmark-campaign.mjs CONFIG.json');
      const controller = new AbortController();
      const cancel = () => controller.abort();
      process.on('SIGINT', cancel);
      process.on('SIGTERM', cancel);
      let result;
      try {
        result = await runBenchmarkCampaign(JSON.parse(readBounded(process.argv[2],1024*1024)), { signal: controller.signal });
      } finally {
        process.off('SIGINT', cancel);
        process.off('SIGTERM', cancel);
      }
      console.log(result.directory);
      if (result.receipt.results.some(r => r.error || r.exit_code !== 0)) process.exitCode = 1;
    }
  } catch (error) { console.error(error.message); process.exitCode = 1; }
}
