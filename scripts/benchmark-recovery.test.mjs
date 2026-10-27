import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { inspectBenchmarkCampaign } from './benchmark-campaign.mjs';

test('interruption inventory preserves scored, failed, missing and corrupt results distinctly', t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'pulsar-campaign-recovery-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const subjects = ['scored', 'failed', 'missing', 'corrupt', 'wrong-subject'];
  fs.writeFileSync(path.join(root,'campaign-start.json'), JSON.stringify({schema_version:1,runner_sha256:'a'.repeat(64),
    intake:{entries:subjects.map(id=>({declaration:{id}}))}}));
  fs.writeFileSync(path.join(root,'result-0.json'), JSON.stringify({subject_id:'scored',exit_code:0,error:null,score:{fixture_only:true}}));
  fs.writeFileSync(path.join(root,'result-1.json'), JSON.stringify({subject_id:'failed',exit_code:1,error:'deadline',score:{invalid:true}}));
  fs.writeFileSync(path.join(root,'result-3.json'), '{partial');
  fs.writeFileSync(path.join(root,'result-4.json'), JSON.stringify({subject_id:'different',exit_code:0,error:null,score:{fixture_only:true}}));
  const before = fs.readdirSync(root);
  const report = inspectBenchmarkCampaign(root);
  assert.deepEqual(report.results.map(result=>result.state), ['scored','failed','unknown','unknown','unknown']);
  assert.ok(report.results.slice(1).every(result=>result.score === null));
  assert.equal(report.automatic_retry_permitted, false);
  assert.equal(report.release_qualified, false);
  assert.deepEqual(fs.readdirSync(root),before);
});
