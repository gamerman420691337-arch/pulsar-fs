import test from 'node:test';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import {bindHumanEffort} from './benchmark-effort.mjs';
const bytes=Buffer.from('synthetic receipt validation fixture; not real human effort');
const binding={subject_id:'clip',source_sha256:'a'.repeat(64),candidate_sha256:'b'.repeat(64)};
const record={schema_version:1,...binding,actor:'fixture-author',session_id:'session',active_edit_ms:100,
  wall_elapsed_ms:200,method:'human-stopwatch',evidence_sha256:crypto.createHash('sha256').update(bytes).digest('hex')};
test('binds supplied claim without promoting it to authenticated human evidence',()=>{
  assert.equal(bindHumanEffort(record,binding,bytes).status,'supplied-human-claim-not-independently-authenticated');
});
test('rejects substitution, impossible timing and fabricated execution-time conversion',()=>{
  for(const altered of [{...record,candidate_sha256:'c'.repeat(64)},{...record,active_edit_ms:201},
    {...record,method:'runner-wall-time'},{...record,active_edit_ms:-1}])
    assert.throws(()=>bindHumanEffort(altered,binding,bytes));
  assert.throws(()=>bindHumanEffort(record,binding,Buffer.from('changed')));
});
