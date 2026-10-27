#!/usr/bin/env python3
"""Audit declared live-reference label clocks; never fit, decode or repair."""
import argparse
import bisect
from fractions import Fraction
import hashlib
import json
import math
import os
from pathlib import Path
import re
import stat
import zipfile

LIMIT = 8 * 1024 * 1024


def require(ok, message):
    if not ok:
        raise ValueError(message)


def read_json(path):
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    try:
        meta = os.fstat(fd)
        require(stat.S_ISREG(meta.st_mode) and meta.st_size <= LIMIT, 'JSON admission')
        with os.fdopen(fd, 'rb', closefd=False) as stream:
            raw = stream.read(LIMIT + 1)
        require(len(raw) <= LIMIT, 'JSON byte limit')
        return json.loads(raw), hashlib.sha256(raw).hexdigest()
    finally:
        os.close(fd)


def sha_file(path):
    h = hashlib.sha256()
    with Path(path).open('rb') as stream:
        for block in iter(lambda: stream.read(1048576), b''):
            h.update(block)
    return h.hexdigest()


def actions_of(value):
    actions = value.get('actions')
    require(isinstance(actions, list) and 2 <= len(actions) <= 250000, 'action count')
    result = []
    for a in actions:
        t, p = a.get('at'), a.get('pos')
        require(type(t) is int and 0 <= t <= 86400000 and type(p) is int and 0 <= p <= 100, 'action bounds')
        require(not result or t > result[-1][0], 'action order')
        result.append((t, p))
    return result


def interpolate(actions, times, milliseconds):
    if milliseconds < times[0] or milliseconds > times[-1]:
        return None
    right = bisect.bisect_left(times, milliseconds)
    if right < len(times) and times[right] == milliseconds:
        return Fraction(actions[right][1], 100)
    left = right - 1
    t0, p0 = actions[left]
    t1, p1 = actions[right]
    return (Fraction(p0) + (p1-p0) * (milliseconds-t0) / (t1-t0)) / 100


def pair_detail(actions, times, left, right, turning_times=None):
    require(left < right, 'pair order')
    a, b = interpolate(actions, times, left), interpolate(actions, times, right)
    if a is None or b is None:
        return None
    lo, hi = bisect.bisect_right(times, left), bisect.bisect_left(times, right)
    points = [a] + [Fraction(p, 100) for _, p in actions[lo:hi]] + [b]
    low, high = min(a, b), max(a, b)
    hidden = max([Fraction(0)] + [max(low-p, p-high) for p in points])
    if turning_times is None:
        turning_times = [times[i] for i in range(1, len(times)-1)
                         if (actions[i][1]-actions[i-1][1]) * (actions[i+1][1]-actions[i][1]) < 0]
    turns = bisect.bisect_left(turning_times, right) - bisect.bisect_right(turning_times, left)
    variation = sum((abs(y-x) for x, y in zip(points, points[1:])), Fraction(0))
    return hidden, turns, variation, abs(b-a)


def load_columns(path):
    import numpy as np
    expected = {'frames.npy', 'labels.npy', 'source_pts.npy', 'groups.npy'}
    headers = {}
    with zipfile.ZipFile(path) as archive:
        items = archive.infolist()
        require(len(items) == 4 and {i.filename for i in items} == expected and
                sum(i.file_size for i in items) <= 512*1024**2, 'dataset archive admission')
        for item in items:
            require(not item.flag_bits & 1, 'encrypted dataset')
            with archive.open(item) as member:
                version = np.lib.format.read_magic(member)
                require(version in ((1, 0), (2, 0)), 'array version')
                reader = np.lib.format.read_array_header_1_0 if version == (1, 0) else np.lib.format.read_array_header_2_0
                shape, fortran, dtype = reader(member, max_header_size=10000)
                require(not fortran and not dtype.hasobject and 1 <= len(shape) <= 4 and
                        all(type(v) is int and 0 < v <= 65536 for v in shape) and
                        math.prod(shape)*dtype.itemsize <= item.file_size, 'array header admission')
                headers[item.filename] = (shape, dtype)
    n = headers['frames.npy'][0][0]
    require(2 <= n <= 65536 and headers['frames.npy'] == ((n, 6, 64, 64), np.dtype('uint8')), 'frame header')
    with np.load(path, allow_pickle=False) as data:
        labels, pts, ids = data['labels'], data['source_pts'], data['groups']
    require(labels.shape == (n, 1) and labels.dtype == np.float32 and np.isfinite(labels).all() and
            ((labels >= 0) & (labels <= 1)).all(), 'label contract')
    require(pts.shape == (n, 2) and pts.dtype == np.int64 and (pts[:, 1] > pts[:, 0]).all(), 'PTS contract')
    require(ids.shape == (n,) and ids.dtype.kind == 'U' and ids.dtype.itemsize <= 256, 'entry identity contract')
    return labels[:, 0], pts, ids


def summarize(rows):
    supported = [r for r in rows if r['detail'] is not None]
    return {'samples': len(rows), 'label_float32_mismatches': sum(r['mismatch'] for r in rows),
            'unsupported_previous_script_endpoint': len(rows)-len(supported),
            'pairs_with_strict_interior_turn': sum(r['detail'][1] > 0 for r in supported),
            'strict_interior_turns': sum(r['detail'][1] for r in supported),
            'pairs_with_hidden_script_excursion': sum(r['detail'][0] > 0 for r in supported),
            'maximum_hidden_script_excursion': float(max((r['detail'][0] for r in supported), default=0)),
            'mean_script_polyline_variation': float(sum((r['detail'][2] for r in supported), Fraction(0))/len(supported)) if supported else None,
            'mean_endpoint_displacement': float(sum((r['detail'][3] for r in supported), Fraction(0))/len(supported)) if supported else None,
            'minimum_pair_seconds': float(min(r['gap'] for r in rows)),
            'maximum_pair_seconds': float(max(r['gap'] for r in rows))}


def audit(staged, dataset_run):
    import numpy as np
    manifest, manifest_sha = read_json(staged/'manifest.json')
    intake, intake_sha = read_json(dataset_run/'intake.json')
    split, split_sha = read_json(dataset_run/'split.json')
    require(manifest['schema_version'] in (1, 2) and manifest['split'] == intake['split'] == 'training', 'training-only scope')
    require(intake['manifest_sha256'] == manifest_sha and intake['dataset_sha256'] == split['dataset_sha256'] == sha_file(dataset_run/'dataset.npz'), 'input identity')
    require(split['sterile_inputs_used'] is False and split['parent_intake_sha256'] == intake_sha, 'split provenance')
    declared = {e['id']: e for e in manifest['entries']}
    require(len(declared) == len(manifest['entries']) == len(intake['entries']) and 1 <= len(declared) <= 64, 'entry count')
    require(set(declared) == {e['id'] for e in intake['entries']} and
            all(re.fullmatch(r'[a-z0-9_-]{1,64}', e['id']) and e['permitted_uses'] == ['training'] for e in declared.values()), 'entry rights/identity')
    train, dev = set(split['training_groups']), set(split['heldout_groups'])
    require(train and dev and not train & dev and train | dev == {e['group'] for e in declared.values()}, 'split membership')
    labels, pts, ids = load_columns(dataset_run/'dataset.npz')
    require(len(labels) == intake['samples'], 'sample count')
    rows, identities, offset = [], [], 0
    for entry in intake['entries']:
        d = declared[entry['id']]
        require(entry['group'] == d['group'] and entry['reference']['path'] == entry['id']+'.funscript', 'entry correspondence')
        clock_offset = d.get('reference_offset_ms', 0)
        require(type(clock_offset) is int and abs(clock_offset) <= 86400000 and entry.get('reference_offset_ms', 0) == clock_offset, 'offset declaration')
        reference_path = staged/entry['reference']['path']
        value, reference_sha = read_json(reference_path)
        require(reference_sha == entry['reference']['sha256'] and reference_path.stat().st_size == entry['reference']['byte_len'], 'reference snapshot identity')
        identities.append({'id': entry['id'], 'reference_sha256': reference_sha})
        actions = actions_of(value)
        times = [t for t, _ in actions]
        turns = [times[i] for i in range(1, len(times)-1)
                 if (actions[i][1]-actions[i-1][1])*(actions[i+1][1]-actions[i][1]) < 0]
        base, origin = Fraction(entry['source_timebase']), Fraction(entry['source_origin'])
        require(base > 0 and 1 <= len(entry['clips']) <= 64, 'source clock/clip admission')
        for clip in entry['clips']:
            decoded = clip['decoded_pts']
            require(2 <= len(decoded) <= 4096 and all(type(p) is int for p in decoded) and
                    all(a < b for a, b in zip(decoded, decoded[1:])), 'decoded PTS order')
            accepted = 0
            for previous, current in zip(decoded, decoded[1:]):
                left = (previous*base-origin)*1000+clock_offset
                right = (current*base-origin)*1000+clock_offset
                label = interpolate(actions, times, right)
                gap = (current-previous)*base
                if label is None or gap > Fraction(1, 2):
                    continue
                require(offset < len(labels) and ids[offset] == entry['id'] and
                        tuple(map(int, pts[offset])) == (previous, current), 'dataset row correspondence')
                expected = np.float32(float(label))
                mismatch = bool(expected.view(np.uint32) != labels[offset].view(np.uint32))
                rows.append({'id': entry['id'], 'group': entry['group'], 'mismatch': mismatch,
                             'gap': gap, 'detail': pair_detail(actions, times, left, right, turns)})
                offset += 1
                accepted += 1
            require(accepted == clip['training_samples'] and len(decoded)-1-accepted == clip['omitted_pairs'], 'clip sample/omission correspondence')
    require(offset == len(labels), 'unconsumed dataset rows')
    require(sha_file(dataset_run/'dataset.npz') == intake['dataset_sha256'] and
            read_json(staged/'manifest.json')[1] == manifest_sha and
            read_json(dataset_run/'intake.json')[1] == intake_sha and
            read_json(dataset_run/'split.json')[1] == split_sha, 'inputs changed during audit')
    for entry in intake['entries']:
        require(sha_file(staged/entry['reference']['path']) == entry['reference']['sha256'], 'reference changed during audit')
    aggregate = summarize(rows)
    return {'schema_version': 1, 'status': 'PASS' if aggregate['label_float32_mismatches'] == 0 else 'FAIL',
            'scope': 'consumed live training/development supplied-label bookkeeping and sampling diagnostic',
            'aggregate': aggregate,
            'optimizer': summarize([r for r in rows if r['group'] in train]),
            'development': summarize([r for r in rows if r['group'] in dev]),
            'by_entry': {e: summarize([r for r in rows if r['id'] == e]) for e in declared},
            'manifest_sha256': manifest_sha, 'intake_sha256': intake_sha, 'split_sha256': split_sha,
            'dataset_sha256': intake['dataset_sha256'], 'reference_identities': identities,
            'frames_loaded_or_decoded': False, 'model_read_or_changed': False,
            'sterile_inputs_read': False, 'labels_or_offsets_changed': False, 'qualified': False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--staged-run', type=Path, required=True)
    parser.add_argument('--dataset-run', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    result = audit(args.staged_run, args.dataset_run)
    with args.output.open('x', encoding='utf8') as stream:
        json.dump(result, stream, indent=2, allow_nan=False)
        stream.write('\n')
    raise SystemExit(0 if result['status'] == 'PASS' else 1)


if __name__ == '__main__':
    main()
