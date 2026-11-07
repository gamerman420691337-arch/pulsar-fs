#!/usr/bin/env python3
"""Read-only training pixel/PTS audit; no fitting or dataset repair."""
import argparse
from fractions import Fraction
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import stat


def module(name, filename):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(filename))
    value = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(value)
    return value


training = module('pixel_training', 'train-reference-position.py')
holdout = module('pixel_holdout', 'train-reference-holdout.py')
require = training.require
FRAME_BYTES = 64 * 64 * 3


def snapshot_path(root, record, limit):
    """Admit contained regular snapshots and verify their saved byte identity."""
    relative = record['path']
    require(isinstance(relative, str) and len(relative) <= 4096 and
            not Path(relative).is_absolute() and '..' not in Path(relative).parts,
            'snapshot relative path admission')
    path = root / relative
    require(not path.is_symlink() and path.resolve(strict=True).is_relative_to(root.resolve()),
            'snapshot escapes root or is a symlink')
    meta = path.stat()
    require(stat.S_ISREG(meta.st_mode) and 0 < meta.st_size <= limit and
            meta.st_size == record['byte_len'], 'snapshot size/type differs')
    require(training.hash_file(path) == record['sha256'], 'snapshot hash differs')
    return path


def parse_frames(rgb, receipt, timebase):
    """Check native raw-frame hashes and demux-clock PTS without showinfo parsing."""
    require(0 < len(rgb) <= training.MAX_FRAMES * FRAME_BYTES and
            len(rgb) % FRAME_BYTES == 0, 'RGB byte admission')
    text = receipt.decode('ascii', errors='strict')
    bases = re.findall(r'^#tb 0: (\d+)/(\d+)$', text, re.MULTILINE)
    require(len(bases) == 1 and Fraction(*map(int, bases[0])) == timebase,
            'framehash clock differs')
    require('#hash: SHA256' in text and '#dimensions 0: 64x64' in text,
            'framehash format differs')
    rows = [line.split(',') for line in text.splitlines() if line and not line.startswith('#')]
    require(len(rows) == len(rgb) // FRAME_BYTES and 2 <= len(rows) <= training.MAX_FRAMES,
            'framehash count differs')
    pts = []
    for index, fields in enumerate(rows):
        require(len(fields) == 6, 'framehash fields differ')
        stream, dts, current, duration, size = map(int, fields[:5])
        require(stream == 0 and dts == current and duration >= 0 and size == FRAME_BYTES,
                'framehash packet contract differs')
        require(not pts or current > pts[-1], 'non-monotonic framehash PTS')
        payload = rgb[index * FRAME_BYTES:(index + 1) * FRAME_BYTES]
        require(fields[5].strip() == hashlib.sha256(payload).hexdigest(),
                'framehash payload differs')
        pts.append(current)
    return pts


def decode(video, clip, timebase, ffmpeg, stride):
    start = Fraction(clip['start'])
    duration = clip['duration_seconds']
    require(start >= 0 and start <= 86400 and type(duration) is int and 1 <= duration <= 120,
            'clip window admission')
    # Saved staging uses millisecond-rounded seek/trim parameters. No saved argv
    # is executed; the numeric fields alone form this fixed command.
    require(start * 1000 == int(start * 1000), 'clip start is not exact milliseconds')
    begin = str(float(start))
    end = str(float(start) + duration)
    filter_value = f'trim=start={begin}:end={end},select=not(mod(n\\,{stride})),scale=64:64:flags=neighbor,format=rgb24'
    command = [str(ffmpeg), '-v', 'quiet', '-nostdin', '-copyts', '-ss', begin,
               '-t', str(duration), '-threads', '2', '-filter_threads', '1',
               '-protocol_whitelist', 'file,pipe', '-noautorotate', '-i', str(video),
               '-map', '0:v:0', '-an', '-sn', '-dn', '-vf', filter_value,
               '-fps_mode', 'passthrough', '-enc_time_base', 'demux',
               '-threads', '1', '-c:v', 'rawvideo', '-f', 'tee',
               '[f=rawvideo:flush_packets=1]pipe:1|[f=framehash:hash=sha256:avoid_negative_ts=disabled:flush_packets=1]pipe:2']
    rgb, receipt = training.bounded_process(command, training.MAX_FRAMES * FRAME_BYTES,
                                           2 * 1024 * 1024, 120)
    pts = parse_frames(rgb, receipt, timebase)
    require(all(start <= value * timebase < start + duration for value in pts),
            'framehash PTS outside window')
    return rgb, pts


def check_clip(rgb, pts, entry, clip, arrays, cursor):
    """Compare one decoded clip with its receipt and ordered stored pair tensors."""
    import numpy as np
    frames, labels, saved_pts, ids = arrays
    require(pts == clip['decoded_pts'], 'saved clip PTS differs')
    require(hashlib.sha256(rgb).hexdigest() == clip['raw_rgb_sha256'], 'saved clip RGB differs')
    pixels = np.frombuffer(rgb, dtype=np.uint8).reshape((-1, 64, 64, 3))
    actions = entry['_actions']
    times = [a[0] for a in actions]
    base = Fraction(entry['source_timebase'])
    origin = Fraction(entry['source_origin'])
    count = 0
    for index in range(1, len(pts)):
        label = training.label_at(actions, times, pts[index] * base - origin + training.reference_offset(entry))
        if label is None or (pts[index] - pts[index - 1]) * base > Fraction(1, 2):
            continue
        require(cursor < len(frames) and str(ids[cursor]) == entry['id'] and
                saved_pts[cursor].tolist() == pts[index - 1:index + 1], 'stored pair identity/PTS differs')
        expected = np.concatenate([pixels[index - 1], pixels[index]], axis=2).transpose(2, 0, 1)
        require(np.array_equal(frames[cursor], expected), 'stored temporal pixels differ')
        cursor += 1
        count += 1
    require(count == clip['training_samples'] and len(pts) - 1 - count == clip['omitted_pairs'],
            'clip accepted/omitted counts differ')
    return cursor, count


def audit(root, dataset, ffmpeg, stride=4):
    require(type(stride) is int and 1 <= stride <= 64, 'stride admission')
    root = Path(root)
    dataset = Path(dataset)
    ffmpeg = Path(ffmpeg).resolve(strict=True)
    require(ffmpeg.is_file(), 'decoder executable absent')
    tool_hash = training.hash_file(ffmpeg)
    intake, intake_raw = training.read_json(root / 'intake.json')
    manifest, manifest_raw = training.read_json(root / 'manifest.json')
    require(intake['schema_version'] == 1 and intake['split'] == manifest['split'] == 'training',
            'training-only intake required')
    require(hashlib.sha256(manifest_raw).hexdigest() == intake['manifest_sha256'], 'manifest hash differs')
    training.validate_manifest(manifest)
    training.validate_reference_offsets(manifest, intake)
    require({(e['id'], e['group']) for e in intake['entries']} ==
            {(e['id'], e['group']) for e in manifest['entries']}, 'entry declarations differ')
    require(not dataset.is_symlink() and dataset.is_file() and dataset.stat().st_size <= 512 * 1024**2,
            'dataset path/size admission')
    require(training.hash_file(dataset) == intake['dataset_sha256'], 'dataset hash differs')
    arrays = holdout.load_dataset(dataset)
    require(len(arrays[0]) == intake['samples'], 'intake row count differs')
    rows, cursor = [], 0
    for entry in intake['entries']:
        source = snapshot_path(root, entry['source'], 16 * 1024**3)
        reference = snapshot_path(root, entry['reference'], training.JSON_LIMIT)
        entry['_actions'] = training.reference_actions(training.read_json(reference)[0])
        base = Fraction(entry['source_timebase'])
        require(base > 0 and 1 <= len(entry['clips']) <= 64, 'source clock/clip count admission')
        for clip in entry['clips']:
            rgb, pts = decode(source, clip, base, ffmpeg, stride)
            cursor, count = check_clip(rgb, pts, entry, clip, arrays, cursor)
            rows.append({'id': entry['id'], 'window': clip['window'], 'frames': len(pts),
                         'pairs': count, 'rgb_sha256': hashlib.sha256(rgb).hexdigest()})
            print(json.dumps({'phase': 'verified', 'id': entry['id'], 'window': clip['window'], 'pairs': count}), flush=True)
        snapshot_path(root, entry['source'], 16 * 1024**3)
        snapshot_path(root, entry['reference'], training.JSON_LIMIT)
    require(cursor == len(arrays[0]), 'unconsumed dataset rows')
    require(training.read_json(root / 'intake.json')[1] == intake_raw and
            training.read_json(root / 'manifest.json')[1] == manifest_raw and
            training.hash_file(dataset) == intake['dataset_sha256'] and
            training.hash_file(ffmpeg) == tool_hash, 'audit inputs changed during processing')
    return {'status': 'PASS', 'scope': 'saved training pixels/source PTS; not alignment or model quality',
            'intake_sha256': hashlib.sha256(intake_raw).hexdigest(),
            'dataset_sha256': intake['dataset_sha256'], 'ffmpeg_sha256': tool_hash,
            'stride': stride, 'samples': cursor, 'clips': rows, 'sterile_inputs_used': False,
            'fitting_or_repair': False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--staged-run', type=Path, required=True)
    parser.add_argument('--dataset', type=Path, required=True)
    parser.add_argument('--ffmpeg', type=Path, required=True)
    parser.add_argument('--stride', type=int, default=4)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    require(not args.output.exists(), 'audit output already exists')
    result = audit(args.staged_run, args.dataset, args.ffmpeg, args.stride)
    training.write_json(args.output, result)


if __name__ == '__main__':
    main()
