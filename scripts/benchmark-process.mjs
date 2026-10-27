// Campaign-owned executables and process groups. No shell interpolation.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import { spawn } from 'node:child_process';

export function snapshotRunner(filename, expected) {
  if (!/^[a-f0-9]{64}$/.test(expected)) throw new Error('invalid runner digest');
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'pulsar-runner-'));
  const destination = path.join(directory, 'runner');
  let input, output;
  try {
    input = fs.openSync(fs.realpathSync(filename), fs.constants.O_RDONLY | fs.constants.O_NOFOLLOW | fs.constants.O_NONBLOCK);
    const stat = fs.fstatSync(input);
    if (!stat.isFile() || stat.size === 0 || stat.size > 1024 ** 3) throw new Error('runner exceeds regular-file bound');
    let cloned = false;
    if (process.platform === 'linux') {
      try {
        // Resolve the held source FD, never the caller's replaceable pathname.
        fs.copyFileSync(`/proc/self/fd/${input}`, destination,
          fs.constants.COPYFILE_EXCL | fs.constants.COPYFILE_FICLONE_FORCE);
        cloned = true;
      } catch (error) {
        if (!['ENOTSUP', 'EOPNOTSUPP', 'EINVAL', 'EXDEV', 'ENOSYS'].includes(error.code)) throw error;
        fs.rmSync(destination, { force: true });
      }
    }
    output = cloned
      ? fs.openSync(destination, fs.constants.O_RDONLY | fs.constants.O_NOFOLLOW | fs.constants.O_NONBLOCK)
      : fs.openSync(destination, 'wx+', 0o600);
    fs.fchmodSync(output, 0o600);
    const buffer = Buffer.alloc(64 * 1024);
    if (!cloned) {
      let count = 0;
      while (true) {
        const n = fs.readSync(input, buffer, 0, buffer.length, null);
        if (!n) break;
        count += n;
        if (count > stat.size) throw new Error('runner grew during snapshot');
        let offset = 0;
        while (offset < n) offset += fs.writeSync(output, buffer, offset, n - offset);
      }
      if (count !== stat.size) throw new Error('campaign runner identity mismatch');
    }
    fs.fsyncSync(output);
    const hash = crypto.createHash('sha256');
    let count = 0;
    while (true) {
      const n = fs.readSync(output, buffer, 0, buffer.length, count);
      if (!n) break;
      count += n;
      if (count > stat.size) throw new Error('captured runner exceeds source size');
      hash.update(buffer.subarray(0, n));
    }
    if (count !== stat.size || hash.digest('hex') !== expected) throw new Error('campaign runner identity mismatch');
    fs.fchmodSync(output, 0o500);
    fs.fsyncSync(output);
    return { path: destination, dispose: () => fs.rmSync(directory, { recursive: true, force: true }) };
  } catch (error) {
    fs.rmSync(directory, { recursive: true, force: true });
    throw error;
  } finally {
    if (input !== undefined) fs.closeSync(input);
    if (output !== undefined) fs.closeSync(output);
  }
}

export async function runAttempt(runner, args, { timeout, signal, environment, logPrefix }) {
  // Current production workers are Linux-only. Refuse weaker tree cleanup.
  if (process.platform !== 'linux') throw new Error('campaign process-tree supervision requires Linux');
  if (signal?.aborted) return { status: null, signal: null, error: 'campaign cancelled', stdout: '', stderr: '' };
  const limit = 1024 * 1024;
  const logs = [];
  try {
    for (const kind of ['stdout', 'stderr']) logs.push({ fd: fs.openSync(`${logPrefix}.${kind}`, 'wx', 0o600), bytes: 0, chunks: [] });
    return await new Promise(resolve => {
      let error = null, finished = false;
      const child = spawn(runner, args, { shell: false, detached: true, env: environment, stdio: ['ignore', 'pipe', 'pipe'] });
      const killTree = () => {
        if (child.pid) {
          try { process.kill(-child.pid, 'SIGKILL'); }
          catch (failure) { if (failure.code !== 'ESRCH') error ??= `process-tree cleanup failed: ${failure.message}`; }
        }
      };
      const stop = reason => { error ??= reason; killTree(); };
      const abort = () => stop('campaign cancelled');
      const timer = setTimeout(() => stop('campaign attempt deadline exceeded'), timeout);
      signal?.addEventListener('abort', abort, { once: true });
      if (signal?.aborted) abort();
      [child.stdout, child.stderr].forEach((stream, index) => stream.on('data', chunk => {
        const log = logs[index], kept = chunk.subarray(0, Math.max(0, limit - log.bytes));
        try {
          let offset = 0;
          while (offset < kept.length) offset += fs.writeSync(log.fd, kept, offset, kept.length - offset);
          log.chunks.push(kept);
          log.bytes += kept.length;
          if (kept.length < chunk.length) stop('runner output exceeded retained 1 MiB stream bound');
        } catch (failure) { stop(`runner log retention failed: ${failure.message}`); }
      }));
      child.on('error', failure => { error ??= failure.message; });
      // A runner exiting while descendants retain its pipes must not stall close.
      child.on('exit', killTree);
      child.on('close', (status, killedBy) => {
        if (finished) return;
        finished = true;
        clearTimeout(timer);
        signal?.removeEventListener('abort', abort);
        resolve({ status, signal: killedBy, error,
          stdout: Buffer.concat(logs[0].chunks).toString(), stderr: Buffer.concat(logs[1].chunks).toString() });
      });
    });
  } finally {
    for (const log of logs) fs.closeSync(log.fd);
  }
}
