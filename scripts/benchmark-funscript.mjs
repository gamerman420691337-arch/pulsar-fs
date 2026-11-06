import crypto from 'node:crypto';

// Independent output adapter: never consult reference stroke boundaries.
// Constant-position edges split runs rather than becoming invented motion.
export function candidateFromFunscript(bytes, { subjectId, targetId, windowStartNs, windowEndNs }) {
  if (!Buffer.isBuffer(bytes) || bytes.length > 8 * 1024 * 1024) throw new Error('bounded raw funscript bytes required');
  for (const value of [subjectId, targetId]) {
    if (typeof value !== 'string' || value.length === 0 || value.length > 256) throw new Error('bounded subject and target identities required');
  }
  const time = value => {
    if (typeof value !== 'string' || !/^(0|[1-9][0-9]*)$/.test(value)) throw new Error('canonical nanosecond time required');
    const result = BigInt(value);
    if (result > 9223372036854775807n) throw new Error('time exceeds project range');
    return result;
  };
  const start = time(windowStartNs), end = time(windowEndNs);
  if (start >= end) throw new Error('nonempty evaluation window required');
  const raw = JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(bytes));
  if (!raw || typeof raw !== 'object' || Array.isArray(raw) || !Array.isArray(raw.actions)
      || raw.actions.length > 100000) throw new Error('bounded funscript actions required');
  const points = raw.actions.map(action => {
    if (!action || !Number.isSafeInteger(action.at) || action.at < 0
        || !Number.isInteger(action.pos) || action.pos < 0 || action.pos > 100) throw new Error('invalid funscript action');
    const t = BigInt(action.at) * 1000000n;
    if (t > 9223372036854775807n || t < start || t > end) throw new Error('action outside evaluation window');
    return { t, p: action.pos * 10000 };
  });
  for (let i = 1; i < points.length; i++) {
    if (points[i].t <= points[i - 1].t) throw new Error('actions must have strictly increasing times');
  }
  const strokes = [];
  let run = null;
  const flush = () => {
    if (!run) return;
    if (strokes.length >= 5000) throw new Error('too many candidate strokes');
    strokes.push({ id: `stroke-${strokes.length}`, target_id: targetId, axis: 'stroke',
      start_ns: run.start.t.toString(), end_ns: run.end.t.toString(),
      start_position_ppm: run.start.p, end_position_ppm: run.end.p });
    run = null;
  };
  for (let i = 1; i < points.length; i++) {
    const a = points[i - 1], b = points[i], direction = Math.sign(b.p - a.p);
    if (direction === 0) { flush(); continue; }
    if (run && run.direction === direction) run.end = b;
    else { flush(); run = { start: a, end: b, direction }; }
  }
  flush();
  return {
    candidate: { schema_version: 1, subject_id: subjectId,
      window: { start_ns: windowStartNs, end_ns: windowEndNs }, strokes, flags: [] },
    receipt: { schema_version: 1, adapter: 'funscript-monotone-plateau-v1',
      raw_sha256: crypto.createHash('sha256').update(bytes).digest('hex'), raw_byte_len: bytes.length,
      subject_id: subjectId, target_id: targetId, axis: 'stroke',
      quantization: { time_unit_ns: 1000000, position_unit_ppm: 10000 },
      review_evidence: 'not-present-in-standard-funscript', release_qualified: false },
  };
}
