#!/usr/bin/env python3
"""Bind saved desktop target views to exact training entries; never label or fit."""
import argparse
import ctypes
import errno
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import stat
import sys
import time
import uuid

spec = importlib.util.spec_from_file_location('training', Path(__file__).with_name('train-reference-position.py'))
training = importlib.util.module_from_spec(spec)
spec.loader.exec_module(training)
require = training.require


def strict_json(raw):
    def object_pairs(pairs):
        result = {}
        for key, value in pairs:
            require(key not in result, 'duplicate JSON field')
            result[key] = value
        return result
    def constant(_):
        raise ValueError('non-finite JSON constant')
    return json.loads(raw, object_pairs_hook=object_pairs, parse_constant=constant)


def read_document(path, limit=training.JSON_LIMIT):
    _, raw = training.read_json(path)
    require(len(raw) <= limit, 'document byte admission')
    return strict_json(raw), raw


def fingerprint(fd):
    x = os.fstat(fd)
    return (x.st_dev, x.st_ino, x.st_size, x.st_mtime_ns, x.st_ctime_ns)


def hash_descriptor(fd, deadline):
    os.lseek(fd, 0, os.SEEK_SET)
    digest = hashlib.sha256()
    while True:
        require(time.monotonic() < deadline, 'source verification deadline')
        chunk = os.read(fd, 1024 * 1024)
        if not chunk:
            break
        digest.update(chunk)
    return digest.hexdigest()


def publish_new(output, files):
    """Linux descriptor-anchored private staging and atomic no-replace directory."""
    require(sys.platform.startswith('linux'), 'saved-view publication requires Linux')
    require(output.is_absolute() and output.name not in ('', '.', '..'), 'absolute output directory required')
    libc = ctypes.CDLL(None, use_errno=True)
    rename = getattr(libc, 'renameat2', None)
    require(rename is not None, 'atomic no-replace directory publication is unavailable')
    rename.argtypes = [ctypes.c_int, ctypes.c_char_p, ctypes.c_int, ctypes.c_char_p, ctypes.c_uint]
    rename.restype = ctypes.c_int
    parent = os.open(output.parent, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    stage = '.pulsar-views-' + uuid.uuid4().hex
    stage_fd = None
    stage_identity = None
    created = []
    published = False
    stage_created = False
    try:
        os.mkdir(stage, 0o700, dir_fd=parent)
        stage_created = True
        stage_fd = os.open(stage, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=parent)
        metadata = os.fstat(stage_fd)
        stage_identity = (metadata.st_dev, metadata.st_ino)
        for name, data in files.items():
            require(Path(name).name == name, 'publication leaf required')
            fd = os.open(name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600, dir_fd=stage_fd)
            metadata = os.fstat(fd)
            created.append((name, (metadata.st_dev, metadata.st_ino)))
            try:
                with os.fdopen(fd, 'wb', closefd=False) as stream:
                    stream.write(data)
                    stream.flush()
                    os.fsync(fd)
            finally:
                os.close(fd)
        os.fsync(stage_fd)
        metadata = os.stat(stage, dir_fd=parent, follow_symlinks=False)
        require((metadata.st_dev, metadata.st_ino) == stage_identity,
                "staging namespace changed; retained for inspection")
        if rename(parent, os.fsencode(stage), parent, os.fsencode(output.name), 1) != 0:
            code = ctypes.get_errno()
            if code in (errno.EEXIST, errno.ENOTEMPTY):
                raise FileExistsError('output already exists; no replacement permitted')
            raise OSError(code, 'atomic no-replace publication failed')
        published = True
        os.fsync(parent)
    finally:
        try:
            if stage_fd is not None:
                try:
                    if not published:
                        for name, identity in created:
                            try:
                                metadata = os.stat(name, dir_fd=stage_fd, follow_symlinks=False)
                            except FileNotFoundError:
                                continue
                            require((metadata.st_dev, metadata.st_ino) == identity,
                                    f'staging content changed; retained {output.parent / stage}')
                            os.unlink(name, dir_fd=stage_fd)
                finally:
                    os.close(stage_fd)
            if not published and stage_created:
                # Refuse deletion if another actor inserted unknown content.
                try:
                    metadata = os.stat(stage, dir_fd=parent, follow_symlinks=False)
                    require((metadata.st_dev, metadata.st_ino) == stage_identity,
                            f'staging namespace changed; retained {output.parent / stage}')
                    os.rmdir(stage, dir_fd=parent)
                except FileNotFoundError:
                    pass
        finally:
            os.close(parent)



def assemble(manifest_path, assignments, planner, ffprobe, output):
    require(sys.platform.startswith('linux'), 'saved-view assembly requires Linux descriptor-backed probing')
    deadline = time.monotonic() + 300
    manifest, manifest_raw = read_document(manifest_path)
    root = training.validate_manifest(manifest)
    expected = {entry['id'] for entry in manifest['entries']}
    require(isinstance(assignments, dict) and set(assignments) == expected, 'views must cover exact manifest IDs')
    planner = Path(planner).resolve(strict=True)
    ffprobe = Path(ffprobe).resolve(strict=True)
    require(planner.is_file() and ffprobe.is_file(), 'native planner/probe required')
    planner_hash, probe_hash = training.hash_file(planner), training.hash_file(ffprobe)
    combined, records = [], []
    files = {'manifest.json': manifest_raw}
    remaining = 64 * 1024**3
    seen = set()
    for entry in manifest['entries']:
        sidecar_path = Path(assignments[entry['id']])
        require(sidecar_path.is_absolute(), 'absolute saved-view path required')
        view, raw = read_document(sidecar_path, 4096)
        require(isinstance(view, dict) and set(view) == {'source_sha256', 'declaration'} and
                isinstance(view['declaration'], dict) and
                isinstance(view['source_sha256'], str) and re.fullmatch('[0-9a-f]{64}', view['source_sha256']),
                'saved desktop view fields/source digest')
        declaration = json.dumps(view['declaration'], separators=(',', ':'), allow_nan=False)
        require(len(declaration.encode()) <= 4096, 'declaration byte admission')
        video = root / entry['video']
        fd = os.open(video, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
        try:
            before = fingerprint(fd)
            require(stat.S_ISREG(os.fstat(fd).st_mode) and 0 < before[2] <= min(remaining, 16 * 1024**3),
                    'source type/byte admission')
            remaining -= before[2]
            source_hash = hash_descriptor(fd, deadline)
            require(source_hash == view['source_sha256'], 'saved view source identity differs')
            require(source_hash not in seen, 'duplicate source bytes in training corpus')
            seen.add(source_hash)
            probe_raw, _ = training.bounded_process([str(ffprobe), '-v', 'error', '-protocol_whitelist',
                'file,pipe', '-select_streams', 'v:0', '-show_entries', 'stream=width,height', '-of', 'json',
                '/proc/self/fd/' + str(fd)], 65536, 65536, 30, pass_fds=(fd,))
            probe = strict_json(probe_raw)
            require(isinstance(probe, dict) and isinstance(probe.get('streams'), list) and len(probe['streams']) == 1, 'one video stream required')
            stream = probe['streams'][0]
            require(isinstance(stream, dict), 'video stream object required')
            dimensions = [stream.get('width'), stream.get('height')]
            require(all(type(v) is int and 0 < v <= 16384 for v in dimensions) and
                    view['declaration'].get('source_dimensions') == dimensions,
                    'saved view source dimensions differ')
            plan_raw, _ = training.bounded_process([str(planner), 'training-input-plan', declaration],
                                                   65536, 65536, 30)
            plan = strict_json(plan_raw)
            require(isinstance(plan, dict) and plan.get('schema_version') == 1 and plan.get('recipe') == 'annotated-temporal-view-v1' and
                    plan.get('declaration') == view['declaration'] and plan.get('output_dimensions') == [64, 64],
                    'native plan correspondence differs')
            require(hash_descriptor(fd, deadline) == source_hash and fingerprint(fd) == before and
                    video.resolve(strict=True).is_relative_to(root) and
                    (video.stat().st_dev, video.stat().st_ino) == before[:2], 'source changed during verification')
        finally:
            os.close(fd)
        require(read_document(sidecar_path, 4096)[1] == raw, 'saved view changed during verification')
        combined.append({'id': entry['id'], **view})
        files[entry['id'] + '.view.json'] = raw
        records.append({'id': entry['id'], 'source_sha256': source_hash, 'source_byte_len': before[2],
                        'saved_view_sha256': hashlib.sha256(raw).hexdigest(), 'plan': plan,
                        'reference_offset_ms': int(training.reference_offset(entry) * 1000),
                        'human_target_and_script_alignment': 'unverified'})
    require(read_document(manifest_path)[1] == manifest_raw and training.hash_file(planner) == planner_hash and
            training.hash_file(ffprobe) == probe_hash, 'manifest/planner/probe changed during verification')
    views = json.dumps({'schema_version': 1, 'entries': combined}, indent=2, allow_nan=False).encode() + b'\n'
    require(len(views) <= training.JSON_LIMIT, 'aggregate view byte admission')
    files['input-views.json'] = views
    receipt = {'schema_version': 1, 'qualified': False, 'manifest_sha256': hashlib.sha256(manifest_raw).hexdigest(),
               'input_views_sha256': hashlib.sha256(views).hexdigest(), 'planner_sha256': planner_hash,
               'ffprobe_sha256': probe_hash, 'entries': records,
               'scope': 'saved geometry/source/native-plan binding only; no labels, fitting or human timing acceptance'}
    files['receipt.json'] = json.dumps(receipt, indent=2, allow_nan=False).encode() + b'\n'
    publish_new(Path(output), files)
    return receipt


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--manifest', type=Path, required=True)
    p.add_argument('--view', action='append', default=[], metavar='ID=ABSOLUTE_SAVED_VIEW')
    p.add_argument('--planner', type=Path, required=True)
    p.add_argument('--ffprobe', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    a = p.parse_args()
    assignments = {}
    for value in a.view:
        require('=' in value, 'view assignment requires ID=PATH')
        key, path = value.split('=', 1)
        require(key not in assignments, 'duplicate view assignment')
        assignments[key] = path
    receipt = assemble(a.manifest, assignments, a.planner, a.ffprobe, a.output)
    print(json.dumps({'qualified': False, 'entries': len(receipt['entries']),
                      'input_views_sha256': receipt['input_views_sha256']}))


if __name__ == '__main__':
    main()
