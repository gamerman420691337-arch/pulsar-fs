// Shipped engine/CLI crash recovery with authored CPU fixtures only.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import {spawn, spawnSync} from 'node:child_process';
import {authoredOnnxFixture} from './benchmark-onnx-fixture.mjs';

const worker = process.env.PULSAR_BENCHMARK_WORKER;
const runtime = process.env.PULSAR_BENCHMARK_ORT;
const configured = worker && runtime && process.platform === 'linux';
const pause = () => new Promise(resolve => setTimeout(resolve, 10));
async function waitFor(observe, message, milliseconds = 10000) {
  const deadline = Date.now() + milliseconds;
  for (;;) {
    const value = observe();
    if (value) return value;
    assert.ok(Date.now() < deadline, message);
    await pause();
  }
}
function pin(filename) {
  const hash = crypto.createHash('sha256');
  const file = fs.openSync(filename, 'r');
  const buffer = Buffer.alloc(1024 * 1024);
  try {
    for (;;) {
      const count = fs.readSync(file, buffer, 0, buffer.length, null);
      if (!count) break;
      hash.update(buffer.subarray(0, count));
    }
    return {byte_len:fs.fstatSync(file).size, sha256:hash.digest('hex')};
  } finally { fs.closeSync(file); }
}
const databaseBytes = state => Object.fromEntries(['projects.sqlite3', 'projects.sqlite3-wal'].map(name => {
  const filename = path.join(state, name);
  return [name, fs.existsSync(filename) ? pin(filename) : null];
}));
function binding(state) {
  const code = "import json,sqlite3,sys;d=sqlite3.connect('file:'+sys.argv[1]+'?mode=ro',uri=True);print(json.dumps({'version':d.execute('PRAGMA user_version').fetchone()[0],'binding':d.execute('SELECT scope,minimum_ledger_version FROM worker_fence_binding').fetchone()}));d.close()";
  const result = spawnSync('/usr/bin/python3', ['-c', code, path.join(state, 'projects.sqlite3')], {encoding:'utf8', timeout:5000});
  assert.equal(result.status, 0, result.stderr);
  return JSON.parse(result.stdout);
}
function sameDirectory(filename, identity) {
  const actual = fs.lstatSync(filename);
  assert.ok(actual.isDirectory() && !actual.isSymbolicLink());
  assert.ok(Number.isSafeInteger(actual.dev) && Number.isSafeInteger(actual.ino));
  assert.deepEqual({device:actual.dev, inode:actual.ino}, identity);
}

for (const fault of ['deletion-permission', 'replacement-root', 'active-owner-death']) {
  test(`shipped engine kill preserves ${fault} obligation and reconciles only restored identity`, {skip:!configured}, async t => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'pulsar-engine-staging-kill-'));
    fs.chmodSync(root, 0o700);
    t.diagnostic(`retained authored engine crash evidence: ${root}`);
    const state = path.join(root, 'state');
    const env = {...process.env, PULSAR_STATE_DIR:state, PULSAR_ORT_DYLIB:runtime};
    const owned = [];
    let sequence = 0;
    let restore;
    function start(label) {
      const stderr = path.join(root, `${label}.stderr`);
      const log = fs.openSync(stderr, 'wx', 0o600);
      const child = spawn(worker, ['engine'], {env, stdio:['ignore','ignore',log]});
      fs.closeSync(log);
      const entry = {child, stderr, result:null};
      entry.closed = new Promise(resolve => {
        child.once('error', error => {entry.result = {error:error.message}; resolve(entry.result);});
        child.once('exit', (code, signal) => {entry.result = {code, signal}; resolve(entry.result);});
      });
      owned.push(entry);
      return entry;
    }
    async function stop(entry, signal) {
      if (!entry.result) assert.equal(entry.child.kill(signal), true);
      return entry.closed;
    }
    async function ready(entry) {
      await waitFor(() => {
        assert.equal(entry.result, null, fs.readFileSync(entry.stderr, 'utf8'));
        return fs.existsSync(path.join(state, 'engine.sock'));
      }, 'owned engine did not reach listener');
    }
    const cli = (args, expectedSuccess = true) => {
      const result = spawnSync(worker, args, {env, encoding:'utf8', timeout:15000, maxBuffer:1024*1024});
      fs.writeFileSync(path.join(root, `cli-${sequence++}.json`), JSON.stringify({args,status:result.status,error:result.error?.message,stdout:result.stdout,stderr:result.stderr}, null, 2), {mode:0o600});
      assert.equal(result.error, undefined);
      if (expectedSuccess) { assert.equal(result.status, 0, result.stderr); return JSON.parse(result.stdout); }
      return result;
    };
    t.after(async () => {
      // Only these direct test-owned engines are signaled and reaped.
      for (const entry of owned) if (!entry.result) await stop(entry, 'SIGKILL');
      if (restore) restore();
    });
    const first = start('owner');
    await ready(first);
    const model = path.join(root, 'authored-temporal.onnx');
    fs.writeFileSync(model, authoredOnnxFixture({temporal:true}), {mode:0o600});
    const profile = cli(['model','profile',model,'--width','64','--height','64','--class-count','1','--temporal-script-position']);
    const project = cli(['project','create','authored-staging-crash']).value.project_id;
    const options = path.join(root, 'options.json');
    fs.writeFileSync(options, JSON.stringify({policy:{maximum_analysis_age_ns:1000000000,added_buffer_ns:0},source_anchor:{numerator:0,denominator:1},project_anchor:0,presentation_anchor:0,lease_ms:30000,inference:{model_path:model,profile}}), {mode:0o600});
    const lease = cli(['live-capture','start',project,'0',options,'--request-id','authored-staging-crash-start']).value;
    await waitFor(() => {
      const report = cli(['live-capture','status',project,'0',lease.identity.epoch]).value;
      assert.equal(report.active, true, JSON.stringify(report.inference));
      return report.inference.state === 'running';
    }, 'authored live worker did not start');
    const ledgerPath = path.join(state, 'worker-fence.json');
    const roster = () => JSON.parse(fs.readFileSync(ledgerPath));
    const ledger = roster();
    assert.equal(ledger.version, 4);
    assert.deepEqual(ledger.creation.records, {});
    assert.deepEqual(binding(state), {version:20,binding:[ledger.scope,4]});
    const records = Object.values(ledger.staging);
    assert.equal(records.length, 1);
    const record = records[0];
    assert.equal(record.parent, fs.realpathSync(state));
    assert.ok(record.phase.Bound);
    const staged = path.join(state, record.name);
    sameDirectory(staged, record.phase.Bound);
    const files = fs.readdirSync(staged).sort();
    const stagedBytes = Object.fromEntries(files.map(name => [name, pin(path.join(staged, name))]));
    const expected = [pin(worker), pin(runtime), pin(model)].sort((a,b)=>a.sha256.localeCompare(b.sha256));
    assert.deepEqual(Object.values(stagedBytes).sort((a,b)=>a.sha256.localeCompare(b.sha256)), expected);
    let canary;
    if (fault !== 'replacement-root') {
      fs.chmodSync(staged, 0o500);
      restore = () => {sameDirectory(staged, record.phase.Bound); fs.chmodSync(staged, 0o700); restore = undefined;};
    } else {
      const moved = path.join(state, 'authored-held-original');
      canary = path.join(state, 'authored-preserved-canary');
      fs.renameSync(staged, moved);
      fs.mkdirSync(staged, {mode:0o700});
      fs.writeFileSync(path.join(staged, 'canary'), 'unrelated authored bytes', {mode:0o600});
      const replacement = fs.lstatSync(staged);
      const replacementIdentity = {device:replacement.dev, inode:replacement.ino};
      restore = () => {
        sameDirectory(staged, replacementIdentity); sameDirectory(moved, record.phase.Bound);
        fs.renameSync(staged, canary); fs.renameSync(moved, staged); restore = undefined;
      };
    }
    if (fault === 'active-owner-death') {
      const active = roster();
      assert.ok(active.staging[record.name].phase.Bound);
      assert.ok(Object.values(active.groups).some(group => /^populated 1$/m.test(fs.readFileSync(path.join(group.path, 'cgroup.events'), 'utf8'))));
    } else {
      // Retire actual worker users before the failed-deletion crash cut.
      const stopped = cli(['live-capture','stop',project,'0',lease.identity.epoch]).value;
      assert.equal(stopped.active, false);
      await waitFor(() => {
        const current = roster();
        if (!current.staging[record.name]?.phase.Retired) return false;
        for (const group of Object.values(current.groups)) {
          const identity = fs.statSync(group.path);
          assert.deepEqual({device:identity.dev,inode:identity.ino}, {device:group.device,inode:group.inode});
          if (!/^populated 0$/m.test(fs.readFileSync(path.join(group.path, 'cgroup.events'), 'utf8'))) return false;
        }
        return true;
      }, 'worker users did not retire into retained cleanup');
    }
    async function expectRefusal(label) {
      const refused = start(label);
      let timer;
      try {
        const result = await Promise.race([refused.closed, new Promise(resolve => {timer = setTimeout(()=>resolve({timeout:true}),10000);})]);
        assert.equal(result.timeout, undefined, 'restart did not refuse before listener');
        assert.deepEqual(result, {code:1,signal:null});
        assert.equal(fs.existsSync(path.join(state, 'engine.sock')), false);
        return fs.readFileSync(refused.stderr, 'utf8');
      } finally { clearTimeout(timer); }
    }
    const phaseAtKill = roster().staging[record.name].phase;
    const killed = await stop(first, 'SIGKILL');
    assert.deepEqual(killed, {code:null,signal:'SIGKILL'});
    if (fault === 'active-owner-death') {
      // Observe the captured cgroups themselves before any recovery opener can
      // reclaim them. Owner exit and an empty journal map are not substitutes.
      assert.ok(Object.values(ledger.groups).length > 0);
      await waitFor(() => Object.values(ledger.groups).every(group => {
        const identity = fs.statSync(group.path);
        assert.deepEqual({device:identity.dev,inode:identity.ino}, {device:group.device,inode:group.inode});
        return /^populated 0$/m.test(fs.readFileSync(path.join(group.path, 'cgroup.events'), 'utf8'));
      }), 'actual worker tree remained populated after owned engine death');
      assert.ok(roster().staging[record.name].phase.Bound);
    }

    const beforeRefusal = databaseBytes(state);
    const fenceBefore = pin(ledgerPath);
    const refusedText = await expectRefusal('refused-restart');
    const afterRefusal = databaseBytes(state);
    assert.deepEqual(afterRefusal, beforeRefusal);
    const fenceAfterRefusal = pin(ledgerPath);
    const pending = roster().staging[record.name];
    assert.ok(pending.phase.Retired);
    if (fault === 'replacement-root') {
      assert.deepEqual(pin(ledgerPath), fenceBefore);
      assert.equal(fs.readFileSync(path.join(staged, 'canary'), 'utf8'), 'unrelated authored bytes');
    }
    assert.match(refusedText, fault === 'replacement-root' ? /staging root identity differs/ : /Permission denied/);
    restore();
    for (const [name, identity] of Object.entries(stagedBytes)) assert.deepEqual(pin(path.join(staged, name)), identity);
    const recovered = start('recovered-restart');
    await ready(recovered);
    assert.equal(fs.existsSync(staged), false);
    const final = roster();
    assert.equal(final.scope, ledger.scope);
    assert.equal(final.version, 4);
    assert.deepEqual(final.creation.records, {});
    assert.deepEqual(final.staging, {}); assert.deepEqual(final.groups, {}); assert.deepEqual(final.retired, {});
    assert.deepEqual(binding(state), {version:20,binding:[ledger.scope,4]});
    if (canary) assert.equal(fs.readFileSync(path.join(canary, 'canary'), 'utf8'), 'unrelated authored bytes');
    const oldStatus = cli(['live-capture','status',project,'0',lease.identity.epoch], false);
    assert.notEqual(oldStatus.status, 0);
    assert.match(oldStatus.stderr, /absent|retired|engine boot/);
    const oldStart = cli(['live-capture','start',project,'0',options,'--request-id','authored-staging-crash-start'], false);
    if (oldStart.status === 0) assert.equal(JSON.parse(oldStart.stdout).value.active, false);
    assert.deepEqual(roster().staging, {}); assert.deepEqual(roster().groups, {});
    fs.writeFileSync(path.join(root, 'crash-receipt.json'), JSON.stringify({fault,signal:killed.signal,phase_at_kill:phaseAtKill,identity:record.phase.Bound,recorded_cgroups:ledger.groups,staged_files:stagedBytes,expected_input_files:expected,database_wal_before_refusal:beforeRefusal,database_wal_after_refusal:afterRefusal,fence_before_refusal:fenceBefore,fence_after_refusal:fenceAfterRefusal,database_wal_preserved_on_refusal:true,fence_preserved_on_refusal:fault==='replacement-root',restored_bytes_match:true,recovered_empty_rosters:true,canary_preserved:!!canary,physical_dispatch:false,active_at_kill:fault==='active-owner-death',actual_cgroups_empty_observed_before_recovery:fault==='active-owner-death',old_epoch_status_refused:true,old_status_exit:oldStatus.status,start_replay_did_not_rearm:true,start_replay_exit:oldStart.status}, null, 2), {mode:0o600});
    await stop(recovered, 'SIGTERM');
  });
}
