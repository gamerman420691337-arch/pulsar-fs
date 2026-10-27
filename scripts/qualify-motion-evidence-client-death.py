#!/usr/bin/env python3
"""Owned client SIGKILL after one accepted package chunk; no real media/device use."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import threading
import time

from importlib import util
spec = util.spec_from_file_location('fault_wire', Path(__file__).with_name('qualify-motion-evidence-interruption.py'))
wire = util.module_from_spec(spec)
spec.loader.exec_module(wire)


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--app', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--expect-recovered', action='store_true')
    p.add_argument('--cleanup-kill-at', type=int, choices=[3, 4], help='Owned strace SIGKILL before directory removal (3) or parent receipt removal (4)')
    p.add_argument('--receipt-write-fault', choices=['enospc', 'file-sync', 'directory-sync'])
    p.add_argument('--receipt-writer-kill', choices=['write', 'file-sync', 'directory-sync'])
    args = p.parse_args()
    if sum(bool(x) for x in [args.receipt_write_fault, args.cleanup_kill_at, args.receipt_writer_kill]) > 1:
        p.error('Select one cleanup fault per owned campaign')
    if (args.receipt_write_fault or args.cleanup_kill_at or args.receipt_writer_kill) and not args.expect_recovered:
        p.error('Cleanup fault campaigns require --expect-recovered')
    app = args.app.resolve(strict=True)
    out = args.output.absolute()
    out.mkdir(mode=0o700)
    stop = threading.Event()
    accepted = threading.Event()
    handlers = []
    threads = []
    listeners = []
    calls, begins, chunks, errors = [], [], [], []
    server = exporter = None
    result = {'qualified': False, 'physical_device_used': False, 'fault': 'owned-client-sigkill-after-accepted-prefix'}
    with tempfile.TemporaryDirectory(prefix='pulsar-client-death-') as temp:
        root = Path(temp)
        state = root / 'state'
        env = dict(os.environ, PULSAR_STATE_DIR=str(state), TMPDIR=str(root))

        def cli(*words, refused=False):
            r = subprocess.run([str(app), *map(str, words)], env=env, stdin=subprocess.DEVNULL,
                               capture_output=True, text=True, timeout=120)
            record = {'args': list(map(str, words)), 'exit_code': r.returncode, 'stdout': r.stdout, 'stderr': r.stderr}
            calls.append(record)
            if refused:
                assert r.returncode != 0, record
                return record
            assert r.returncode == 0, record
            value = json.loads(r.stdout)
            return value.get('value', value)

        def control_handler(client):
            try:
                with client, socket.socket(socket.AF_UNIX) as upstream:
                    client.settimeout(10)
                    upstream.settimeout(10)
                    upstream.connect(str(state / 'real-control.sock'))
                    while not stop.is_set():
                        data, request = wire.frame(client)
                        upstream.sendall(data)
                        reply, response = wire.frame(upstream)
                        if request['command']['operation'] == 'begin_project_package_download':
                            body = response.get('result', {}).get('Ok', {})
                            if body.get('kind') == 'package_download':
                                begins.append({'request_id': request['request_id'], 'operation_id': request['command']['arguments']['operation_id']})
                        client.sendall(reply)
            except EOFError:
                pass
            except Exception as e:
                errors.append(type(e).__name__)

        def bulk_handler(client):
            try:
                with client, socket.socket(socket.AF_UNIX) as upstream:
                    client.settimeout(15)
                    upstream.settimeout(15)
                    upstream.connect(str(state / 'real-bulk.sock'))
                    data, _ = wire.frame(client)
                    upstream.sendall(data)
                    data, ready = wire.frame(upstream)
                    assert ready['status'] == 'ready'
                    client.sendall(data)
                    data, first = wire.frame(client)
                    assert first['offset'] == 0
                    upstream.sendall(data)
                    data, reply = wire.frame(upstream)
                    assert reply['status'] == 'download'
                    client.sendall(data)
                    data, payload = wire.raw_frame(upstream)
                    assert len(payload) == first['byte_len']
                    client.sendall(data)
                    _, second = wire.frame(client)
                    assert second['offset'] == len(payload) and second['offset'] > 0
                    chunks.append({'accepted_prefix_bytes': len(payload), 'next_requested_offset': second['offset'], 'prefix_sha256': hashlib.sha256(payload).hexdigest(), 'second_request_forwarded': False})
                    accepted.set()
                    # The owning test kills its exporter while the second request
                    # is blocked. No second range reaches the owned Engine.
                    stop.wait(15)
            except Exception as e:
                errors.append(type(e).__name__)
                accepted.set()

        def accept(listener, handler):
            while not stop.is_set():
                try:
                    client, _ = listener.accept()
                except socket.timeout:
                    continue
                except OSError:
                    if stop.is_set(): return
                    raise
                t = threading.Thread(target=handler, args=(client,))
                handlers.append(t)
                t.start()

        def restore():
            stop.set()
            for listener in listeners: listener.close()
            for t in threads + handlers:
                t.join(timeout=20)
                assert not t.is_alive()
            for public, real in [('engine.sock', 'real-control.sock'), ('bulk.sock', 'real-bulk.sock')]:
                (state / public).unlink()
                (state / real).rename(state / public)

        try:
            with (out / 'engine.log').open('wb') as log:
                server = subprocess.Popen([str(app), 'engine'], env=env, stdin=subprocess.DEVNULL, stdout=log, stderr=log)
            deadline = time.monotonic() + 45
            while not (state / 'engine.sock').exists():
                assert server.poll() is None and time.monotonic() < deadline
                time.sleep(.05)
            project = cli('project', 'create', 'Owned client process death')['project_id']
            pattern = root / 'pattern.txt'
            pattern.write_text('pattern=triangle axis=stroke duration=50s frequency=1hz amplitude=0.1 offset=0.5 sample_hz=100\n')
            candidate = cli('pattern-candidate', project, pattern, '--revision', '0')['candidate_id']
            cli('commit-candidate', project, candidate, '--revision', '0')
            head = cli('project', 'open', project)
            revision = head['revision']
            for public, real, handler in [('engine.sock', 'real-control.sock', control_handler), ('bulk.sock', 'real-bulk.sock', bulk_handler)]:
                (state / public).rename(state / real)
                listener = socket.socket(socket.AF_UNIX)
                listener.bind(str(state / public))
                (state / public).chmod(0o600)
                listener.listen(16)
                listener.settimeout(.2)
                listeners.append(listener)
                t = threading.Thread(target=accept, args=(listener, handler))
                threads.append(t)
                t.start()
            destination = root / 'paired'
            with (out / 'exporter.log').open('wb') as log:
                exporter = subprocess.Popen([str(app), 'motion-evidence-export', project, str(destination), '--revision', str(revision), '--acknowledge-unencrypted-private-data'], env=env, stdin=subprocess.DEVNULL, stdout=log, stderr=log)
                assert accepted.wait(90) and chunks and not errors, (chunks, errors)
                assert exporter.poll() is None
                exporter.kill()  # only this test's owned PID
                exporter.wait(timeout=10)
            assert exporter.returncode == -9
            stages = list(root.glob('.pulsar-motion-evidence-*'))
            assert len(stages) == 1 and not destination.exists()
            stage = stages[0]
            intent_bytes = (stage / 'intent.json').read_bytes()
            intent = json.loads(intent_bytes)
            partials = list(stage.glob('.pulsar-package-*'))
            assert len(partials) == 1
            body = partials[0] / 'bundle'
            original_partial_stat = partials[0].stat()
            original_bundle_stat = body.stat()
            assert original_bundle_stat.st_size == chunks[0]['accepted_prefix_bytes']
            assert hashlib.sha256(body.read_bytes()).hexdigest() == chunks[0]['prefix_sha256']
            assert (stage / 'download-started.json').exists() and not (stage / 'project.pulsar').exists()
            before = len(begins)
            refusal = cli('motion-evidence-resume', stage, '--acknowledge-unencrypted-private-data', refused=True)
            assert 'Do not blindly replay' in refusal['stderr'] and len(begins) == before
            operation = begins[0]['operation_id']
            restore()
            old = cli('package-download', project, operation, stage / 'project.pulsar', '--request-id', intent['download_request'], '--acknowledge-unencrypted-private-data', refused=True)
            assert 'not the complete requested range' in old['stderr'] or 'package download expired' in old['stderr']
            cli('package-download', project, operation, stage / 'project.pulsar', '--acknowledge-unencrypted-private-data')
            result.update(client_killed_after_full_chunk=True, client_exit=exporter.returncode, crashed_partial_member_observed=True, accepted_prefix_bytes=chunks[0]['accepted_prefix_bytes'], ambiguous_resume_refused=True, old_begin_refused=True, explicit_fresh_download_verified=True, original_intent_sha256=hashlib.sha256(intent_bytes).hexdigest())
            if args.expect_recovered:
                if args.cleanup_kill_at:
                    killed = subprocess.run(['strace', '--kill-on-exit', '-o', str(out / 'cleanup-syscalls.log'), '-e', 'trace=unlinkat', '-e', f'inject=unlinkat:signal=SIGKILL:when={args.cleanup_kill_at}', str(app), 'motion-evidence-resume', str(stage), '--acknowledge-unencrypted-private-data'], env=env, stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=120)
                    (out / 'cleanup-killed.stderr').write_text(killed.stderr)
                    assert killed.returncode == -9
                    trace = (out / 'cleanup-syscalls.log').read_text()
                    assert 'killed by SIGKILL' in trace
                    records = list(stage.glob('.pulsar-download-cleanup-*.json'))
                    assert len(records) == 1
                    cleanup_bytes = records[0].read_bytes()
                    cleanup = json.loads(cleanup_bytes)
                    assert cleanup['download']['binding']['request'] == intent['download_request']
                    assert cleanup['download']['binding']['operation'] == operation
                    binding = cleanup['download']['binding']
                    assert binding['credential_binding'] == intent['credential_binding']
                    assert binding['project'] == project
                    assert binding['artifact']['sha256'] == hashlib.sha256((stage / 'project.pulsar').read_bytes()).hexdigest()
                    assert binding['artifact']['captured_revision'] == revision
                    assert cleanup['download']['destination_name'] == list(b'project.pulsar')
                    assert cleanup['download']['directory'] == {'device': original_partial_stat.st_dev, 'inode': original_partial_stat.st_ino}
                    assert cleanup['download']['bundle'] == {'device': original_bundle_stat.st_dev, 'inode': original_bundle_stat.st_ino}

                    if args.cleanup_kill_at == 3:
                        assert partials[0].is_dir() and not any(partials[0].iterdir())
                        assert 'AT_REMOVEDIR) = ?' in trace
                    else:
                        assert not partials[0].exists()
                        assert 'AT_REMOVEDIR) = 0' in trace and 'pulsar-download-cleanup-' in trace
                    result.update(cleanup_client_killed=True, cleanup_exit=killed.returncode,
                        cleanup_kill_at=args.cleanup_kill_at, durable_parent_cleanup_receipt_observed=True,
                        cleanup_receipt_sha256=hashlib.sha256(cleanup_bytes).hexdigest())
                if args.receipt_write_fault:
                    fault = {'enospc': 'write:error=ENOSPC:when=3', 'file-sync': 'fsync:error=EIO:when=1', 'directory-sync': 'fsync:error=EIO:when=2'}[args.receipt_write_fault]
                    failed = subprocess.run(['strace', '--kill-on-exit', '-o', str(out / 'receipt-write-syscalls.log'), '-e', 'trace=write,writev,openat,fsync,unlinkat', '-e', 'raw=write,writev', '-e', f'inject={fault}', str(app), 'motion-evidence-resume', str(stage), '--acknowledge-unencrypted-private-data'], env=env, stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=120)
                    (out / 'receipt-write-error.stderr').write_text(failed.stderr)
                    assert failed.returncode != 0
                    trace = (out / 'receipt-write-syscalls.log').read_text()
                    expected_error = 'ENOSPC' if args.receipt_write_fault == 'enospc' else 'EIO'
                    assert expected_error in trace and 'INJECTED' in trace
                    assert 'pulsar-download-cleanup-' in trace or 'O_TMPFILE' in trace
                    assert not list(stage.glob('.pulsar-download-cleanup-*'))
                    assert body.is_file() and body.stat().st_size == chunks[0]['accepted_prefix_bytes']
                    assert hashlib.sha256(body.read_bytes()).hexdigest() == chunks[0]['prefix_sha256']
                    assert (partials[0] / 'recovery.json').is_file()
                    assert (stage / 'intent.json').read_bytes() == intent_bytes and not destination.exists()
                    result.update(receipt_write_fault=args.receipt_write_fault, handled_io_failure=True,
                        failed_receipt_removed=True, original_partial_preserved=True)
                if args.receipt_writer_kill:
                    fault = {'write': 'write:signal=SIGKILL:when=3', 'file-sync': 'fsync:signal=SIGKILL:when=1', 'directory-sync': 'fsync:signal=SIGKILL:when=2'}[args.receipt_writer_kill]
                    killed = subprocess.run(['strace', '--kill-on-exit', '-o', str(out / 'receipt-writer-syscalls.log'), '-e', 'trace=write,writev,openat,fsync,linkat,unlinkat', '-e', 'raw=write,writev', '-e', f'inject={fault}', str(app), 'motion-evidence-resume', str(stage), '--acknowledge-unencrypted-private-data'], env=env, stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=120)
                    (out / 'receipt-writer-killed.stderr').write_text(killed.stderr)
                    assert killed.returncode == -9
                    trace = (out / 'receipt-writer-syscalls.log').read_text()
                    assert 'killed by SIGKILL' in trace
                    records = list(stage.glob('.pulsar-download-cleanup-*.json'))
                    observed = [{'byte_len': f.stat().st_size, 'sha256': hashlib.sha256(f.read_bytes()).hexdigest()} for f in records]
                    (out / 'receipt-writer-boundary.json').write_text(json.dumps({'boundary': args.receipt_writer_kill, 'records': observed, 'exit': killed.returncode}) + '\n')
                    if args.receipt_writer_kill == 'directory-sync':
                        assert len(records) == 1
                        cleanup = json.loads(records[0].read_bytes())
                        assert cleanup['download']['binding']['request'] == intent['download_request']
                        assert cleanup['download']['binding']['operation'] == operation
                    else:
                        assert not records, 'Abrupt receipt writer left a published incomplete receipt'
                    assert body.is_file() and body.stat().st_size == chunks[0]['accepted_prefix_bytes']
                    assert hashlib.sha256(body.read_bytes()).hexdigest() == chunks[0]['prefix_sha256']
                    assert (partials[0] / 'recovery.json').is_file()
                    assert (stage / 'intent.json').read_bytes() == intent_bytes and not destination.exists()
                    result.update(receipt_writer_kill=args.receipt_writer_kill, writer_killed=True, published_receipt_absent_or_complete=True, original_partial_preserved=True)
                receipt = cli('motion-evidence-resume', stage, '--acknowledge-unencrypted-private-data')
                assert receipt['file_and_directory_synced'] and not stage.exists()
                assert (destination / 'intent.json').read_bytes() == intent_bytes
                assert not list(destination.glob('.pulsar-package-*'))
                assert not list(destination.glob('.pulsar-download-cleanup-*'))
                actual = json.loads((destination / 'motion.funscript').read_text())['actions']
                expected = []
                for k in range(5001):
                    q = (k % 100) / 25
                    v = q if q <= 1 else 2 - q if q <= 3 else q - 4
                    expected.append({'at': k * 10, 'pos': round(50 + 10 * v)})
                assert actual == expected
                check = cli('motion-evidence-check', destination / 'motion.funscript', destination / 'project.pulsar', '--project', project, '--revision', revision, '--axis', 'stroke')
                assert check == receipt['association'] and not check['qualified']
                result.update(recovery_passed=True, independent_actions=5001, original_intent_preserved=True, association=check)
            else:
                refusal = cli('motion-evidence-resume', stage, '--acknowledge-unencrypted-private-data', refused=True)
                assert 'Unexpected staging member' in refusal['stderr']
                assert body.exists() and not destination.exists()
                result.update(reproduced_orphan_blocks_publication=True, refusal=refusal['stderr'])
            assert cli('project', 'open', project) == head
            result['master_unchanged'] = True
        finally:
            stop.set()
            if exporter is not None and exporter.poll() is None:
                exporter.kill()
                exporter.wait(timeout=10)
            for listener in listeners: listener.close()
            for t in threads + handlers: t.join(timeout=20)
            if server is not None and server.poll() is None:
                server.terminate()
                try: server.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    server.kill()
                    server.wait(timeout=10)
            result.update(engine_joined=server is not None and server.poll() is not None, exporter_joined=exporter is not None and exporter.poll() is not None, handlers_joined=all(not t.is_alive() for t in threads + handlers), proxy_errors=errors, app_sha256=hashlib.sha256(app.read_bytes()).hexdigest())
            for name, value in [('result', result), ('calls', calls), ('begins', begins), ('chunks', chunks)]:
                (out / (name + '.json')).write_text(json.dumps(value, indent=2) + '\n')
        assert result['engine_joined'] and result['exporter_joined'] and result['handlers_joined'] and not errors

if __name__ == '__main__': main()
