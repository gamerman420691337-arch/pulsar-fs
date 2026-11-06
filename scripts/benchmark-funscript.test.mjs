import test from 'node:test';
import assert from 'node:assert/strict';
import { candidateFromFunscript } from './benchmark-funscript.mjs';
import { validateAnnotation } from './benchmark-good.mjs';

const options = { subjectId: 'clip', targetId: 'target', windowStartNs: '0', windowEndNs: '1000000000' };
const convert = actions => candidateFromFunscript(Buffer.from(JSON.stringify({ actions })), options);

test('independent monotone segmentation preserves plateaus, shallow travel, reversals', () => {
  const { candidate, receipt } = convert([
    { at: 0, pos: 40 }, { at: 100, pos: 42 }, { at: 200, pos: 44 },
    { at: 300, pos: 44 }, { at: 400, pos: 46 }, { at: 500, pos: 43 },
  ]);
  validateAnnotation(candidate, 'candidate');
  assert.deepEqual(candidate.strokes.map(s => [s.start_ns, s.end_ns, s.start_position_ppm, s.end_position_ppm]), [
    ['0', '200000000', 400000, 440000],
    ['300000000', '400000000', 440000, 460000],
    ['400000000', '500000000', 460000, 430000],
  ]);
  assert.equal(receipt.release_qualified, false);
  assert.equal(receipt.raw_sha256.length, 64);
});

test('stationary and empty outputs invent no motion or review flags', () => {
  for (const actions of [[], [{ at: 0, pos: 50 }], [{ at: 0, pos: 50 }, { at: 10, pos: 50 }]]) {
    assert.deepEqual(convert(actions).candidate.strokes, []);
    assert.deepEqual(convert(actions).candidate.flags, []);
  }
});

test('rejects ambiguous timing, out-of-range positions and implicit clipping', () => {
  for (const actions of [
    [{ at: 1, pos: 50 }, { at: 1, pos: 60 }],
    [{ at: 2, pos: 50 }, { at: 1, pos: 60 }],
    [{ at: 0.5, pos: 50 }], [{ at: 0, pos: 101 }],
    [{ at: 1001, pos: 50 }], [{ at: -1, pos: 50 }],
  ]) assert.throws(() => convert(actions));
});
