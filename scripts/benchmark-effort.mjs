import crypto from 'node:crypto';

// Structural binding of supplied human evidence, not authentication of its author.
export function bindHumanEffort(record, binding, evidenceBytes) {
  const fields = ['schema_version','subject_id','source_sha256','candidate_sha256','actor','session_id',
    'active_edit_ms','wall_elapsed_ms','method','evidence_sha256'];
  if (!record || typeof record !== 'object' || Array.isArray(record)
      || Object.keys(record).length !== fields.length || fields.some(k => !(k in record)))
    throw new Error('exact human-effort receipt schema required');
  if (record.schema_version !== 1 || !['human-stopwatch','instrumented-editor'].includes(record.method))
    throw new Error('unsupported human-effort method');
  for (const k of ['subject_id','actor','session_id']) {
    if (typeof record[k] !== 'string' || !record[k].trim() || record[k].length > 256)
      throw new Error('bounded effort identity required');
  }
  for (const k of ['source_sha256','candidate_sha256','evidence_sha256']) {
    if (typeof record[k] !== 'string' || !/^[a-f0-9]{64}$/.test(record[k])) throw new Error('invalid effort digest');
  }
  for (const k of ['subject_id','source_sha256','candidate_sha256']) {
    if (record[k] !== binding[k]) throw new Error('human effort belongs to different campaign output');
  }
  if (!Number.isSafeInteger(record.active_edit_ms) || record.active_edit_ms < 0
      || !Number.isSafeInteger(record.wall_elapsed_ms) || record.wall_elapsed_ms < record.active_edit_ms
      || record.wall_elapsed_ms > 30*24*60*60*1000) throw new Error('invalid human-effort timing');
  if (!Buffer.isBuffer(evidenceBytes) || !evidenceBytes.length || evidenceBytes.length > 8*1024*1024
      || crypto.createHash('sha256').update(evidenceBytes).digest('hex') !== record.evidence_sha256)
    throw new Error('human-effort evidence digest mismatch');
  return {record:{...record},status:'supplied-human-claim-not-independently-authenticated'};
}
