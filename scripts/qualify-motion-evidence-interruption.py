#!/usr/bin/env python3
"""Owned Linux lease-ack/partial-chunk probes; no media/device qualification."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import socket
import struct
import subprocess
import tempfile
import threading
import time


def raw_frame(stream):
    def exact(count):
        parts = bytearray()
        while len(parts) < count:
            chunk = stream.recv(count - len(parts))
            if not chunk:
                raise EOFError()
            parts.extend(chunk)
        return bytes(parts)
    header = exact(4)
    count = struct.unpack('!I', header)[0]
    if not 0 < count <= 1024 * 1024:
        raise ValueError('control frame exceeds protocol bound')
    payload = exact(count)
    return header + payload, payload


def frame(stream):
    wire, payload = raw_frame(stream)
    return wire, json.loads(payload)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--app', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--fault', choices=['lease-ack', 'bulk-chunk'], default='lease-ack')
    args = parser.parse_args()
    app = args.app.resolve(strict=True)
    output = args.output.absolute()
    output.mkdir(mode=0o700)  # refuse replacement of evidence
    calls, observations, failures, partial_chunks = [], [], [], []
    result = {'qualified': False, 'physical_device_used': False, 'fault': args.fault}
    server = None
    stop = threading.Event()
    handlers = []
    proxy_thread = None
    listener = None
    bulk_listener = None
    bulk_thread = None
    with tempfile.TemporaryDirectory(prefix='pulsar-ack-loss-') as temporary:
        root = Path(temporary)
        state = root / 'state'
        env = dict(os.environ, PULSAR_STATE_DIR=str(state), TMPDIR=str(root))

        def cli(*words, refused=False):
            completed = subprocess.run([str(app), *map(str, words)], env=env,
                                       stdin=subprocess.DEVNULL, capture_output=True,
                                       text=True, timeout=120)
            record = {'args': list(map(str, words)), 'exit_code': completed.returncode,
                      'stdout': completed.stdout, 'stderr': completed.stderr}
            calls.append(record)
            if refused:
                assert completed.returncode != 0, record
                return record
            assert completed.returncode == 0, record
            value = json.loads(completed.stdout)
            return value.get('value', value)

        def handle(client):
            try:
                with client, socket.socket(socket.AF_UNIX) as upstream:
                    client.settimeout(10)
                    upstream.settimeout(10)
                    upstream.connect(str(state / 'upstream.sock'))
                    while not stop.is_set():
                        wire, request = frame(client)
                        upstream.sendall(wire)
                        reply_wire, reply = frame(upstream)
                        command = request['command']['operation']
                        body = reply.get('result', {}).get('Ok', {})
                        if command == 'begin_project_package_download' and body.get('kind') == 'package_download':
                            # Do not persist request authentication or the lease token.
                            observations.append({'request_id': request['request_id'],
                                                 'operation_id': request['command']['arguments']['operation_id'],
                                                 'engine_accepted_lease': True,
                                                 'response_delivered': args.fault != 'lease-ack'})
                            if args.fault == 'lease-ack':
                                return
                        client.sendall(reply_wire)
            except EOFError:
                pass  # one-call control clients normally close after their reply
            except Exception as error:
                failures.append(type(error).__name__)  # never dump authenticated frames

        def accept():
            while not stop.is_set():
                try:
                    client, _ = listener.accept()
                except socket.timeout:
                    continue
                except OSError:
                    if stop.is_set():
                        return
                    raise
                thread = threading.Thread(target=handle, args=(client,))
                handlers.append(thread)
                thread.start()

        def bulk_handle(client):
            try:
                with client, socket.socket(socket.AF_UNIX) as upstream:
                    client.settimeout(10)
                    upstream.settimeout(10)
                    upstream.connect(str(state / 'upstream-bulk.sock'))
                    handshake_wire, _ = frame(client)
                    upstream.sendall(handshake_wire)
                    ready_wire, ready = frame(upstream)
                    assert ready['status'] == 'ready', 'expected authenticated bulk lease'
                    client.sendall(ready_wire)
                    request_wire, request = frame(client)
                    upstream.sendall(request_wire)
                    reply_wire, reply = frame(upstream)
                    assert reply['status'] == 'download', 'expected actual chunk reply'
                    client.sendall(reply_wire)
                    raw_wire, payload = raw_frame(upstream)
                    assert len(payload) == request['byte_len'] and len(payload) > 128
                    client.sendall(raw_wire[:4 + 128])
                    partial_chunks.append({'offset': request['offset'],
                                           'declared_byte_len': len(payload),
                                           'payload_bytes_delivered': 128,
                                           'chunk_complete': False})
                    # Close after real payload bytes, preventing chunk acceptance.
            except Exception as error:
                failures.append(type(error).__name__)

        def bulk_accept():
            while not stop.is_set():
                try:
                    client, _ = bulk_listener.accept()
                except socket.timeout:
                    continue
                except OSError:
                    if stop.is_set():
                        return
                    raise
                thread = threading.Thread(target=bulk_handle, args=(client,))
                handlers.append(thread)
                thread.start()

        try:
            with (output / 'engine.log').open('wb') as log:
                server = subprocess.Popen([str(app), 'engine'], env=env,
                                          stdin=subprocess.DEVNULL, stdout=log, stderr=log)
            deadline = time.monotonic() + 45
            endpoint = state / 'engine.sock'
            while not endpoint.exists():
                assert server.poll() is None and time.monotonic() < deadline
                time.sleep(0.05)
            project = cli('project', 'create', 'Owned acknowledgement loss')['project_id']
            patterns = root / 'patterns.txt'
            patterns.write_text('pattern=triangle axis=stroke duration=2s frequency=1hz amplitude=0.1 offset=0.5 sample_hz=8\n')
            candidate = cli('pattern-candidate', project, patterns, '--revision', '0')['candidate_id']
            cli('commit-candidate', project, candidate, '--revision', '0')
            head = cli('project', 'open', project)
            revision = head['revision']
            # Rename only this campaign's socket; Engine remains bound to it.
            endpoint.rename(state / 'upstream.sock')
            listener = socket.socket(socket.AF_UNIX)
            listener.bind(str(endpoint))
            endpoint.chmod(0o600)
            listener.listen(16)
            listener.settimeout(0.2)
            proxy_thread = threading.Thread(target=accept)
            proxy_thread.start()
            if args.fault == 'bulk-chunk':
                bulk_endpoint = state / 'bulk.sock'
                bulk_endpoint.rename(state / 'upstream-bulk.sock')
                bulk_listener = socket.socket(socket.AF_UNIX)
                bulk_listener.bind(str(bulk_endpoint))
                bulk_endpoint.chmod(0o600)
                bulk_listener.listen(16)
                bulk_listener.settimeout(0.2)
                bulk_thread = threading.Thread(target=bulk_accept)
                bulk_thread.start()
            destination = root / 'paired'
            rejected = cli('motion-evidence-export', project, destination, '--revision', revision,
                           '--acknowledge-unencrypted-private-data', refused=True)
            assert observations and not failures, (observations, failures)
            stages = list(root.glob('.pulsar-motion-evidence-*'))
            assert len(stages) == 1 and not destination.exists()
            stage = stages[0]
            intent_bytes = (stage / 'intent.json').read_bytes()
            intent = json.loads(intent_bytes)
            assert (stage / 'download-started.json').exists()
            assert not (stage / 'project.pulsar').exists()
            assert all(item['request_id'] == intent['download_request'] for item in observations)
            operation = observations[0]['operation_id']
            assert all(item['operation_id'] == operation for item in observations)
            if args.fault == 'bulk-chunk':
                assert len(partial_chunks) == 2 and all(chunk['offset'] == 0 for chunk in partial_chunks)
                assert sorted(p.name for p in stage.iterdir()) == [
                    'download-started.json', 'intent.json', 'motion.funscript']
            before = len(observations)
            refusal = cli('motion-evidence-resume', stage, '--acknowledge-unencrypted-private-data', refused=True)
            assert 'Do not blindly replay' in refusal['stderr'] and len(observations) == before
            # Restore the real endpoint before deliberate exact-ID recovery.
            stop.set()
            listener.close()
            if bulk_listener is not None:
                bulk_listener.close()
            if bulk_thread is not None:
                bulk_thread.join(timeout=12)
                assert not bulk_thread.is_alive()
            proxy_thread.join(timeout=12)
            for thread in handlers:
                thread.join(timeout=12)
            assert not proxy_thread.is_alive() and not any(t.is_alive() for t in handlers)
            endpoint.unlink()
            (state / 'upstream.sock').rename(endpoint)
            if args.fault == 'bulk-chunk':
                bulk_endpoint.unlink()
                (state / 'upstream-bulk.sock').rename(bulk_endpoint)
                old = cli('package-download', project, operation, stage / 'project.pulsar',
                          '--request-id', intent['download_request'],
                          '--acknowledge-unencrypted-private-data', refused=True)
                assert 'package download expired, was abandoned, or belongs to a prior engine epoch' in old['stderr']
                assert not (stage / 'project.pulsar').exists()
                cli('package-download', project, operation, stage / 'project.pulsar',
                    '--acknowledge-unencrypted-private-data')
                result.update(abandoned_begin_refused=True, explicit_fresh_begin_recovered=True,
                              partial_member_removed=True, abandoned_begin_error=old['stderr'])
            else:
                cli('package-download', project, operation, stage / 'project.pulsar',
                    '--request-id', intent['download_request'], '--acknowledge-unencrypted-private-data')
            receipt = cli('motion-evidence-resume', stage, '--acknowledge-unencrypted-private-data')
            assert receipt['file_and_directory_synced'] and not receipt['engine_release_requested']
            assert (destination / 'intent.json').read_bytes() == intent_bytes and not stage.exists()
            checked = cli('motion-evidence-check', destination / 'motion.funscript', destination / 'project.pulsar',
                          '--project', project, '--revision', revision, '--axis', 'stroke')
            expected = [{'at': k * 125, 'pos': round(50 + 10 * [0, .5, 1, .5, 0, -.5, -1, -.5][k % 8])} for k in range(17)]
            assert json.loads((destination / 'motion.funscript').read_text())['actions'] == expected
            assert checked == receipt['association'] and not checked['qualified']
            assert cli('project', 'open', project) == head
            result.update(actual_lease_acknowledgements_dropped=sum(not o['response_delivered'] for o in observations),
                          ambiguous_resume_refused=True, exact_begin_identity_recovered=args.fault == 'lease-ack',
                          original_intent_preserved=True, master_unchanged=True,
                          independent_17_actions_match=True, association=checked,
                          initial_error=rejected['stderr'])
        finally:
            stop.set()
            if listener is not None:
                listener.close()
            if bulk_listener is not None:
                bulk_listener.close()
            if bulk_thread is not None:
                bulk_thread.join(timeout=12)
            if proxy_thread is not None:
                proxy_thread.join(timeout=12)
            for thread in handlers:
                thread.join(timeout=12)
            if server is not None and server.poll() is None:
                server.terminate()
                try:
                    server.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    server.kill()
                    server.wait(timeout=5)
            result.update(engine_joined=server is not None and server.poll() is not None,
                          engine_exit=server.returncode if server else None,
                          proxy_joined=proxy_thread is None or not proxy_thread.is_alive(),
                          bulk_proxy_joined=bulk_thread is None or not bulk_thread.is_alive(),
                          handlers_joined=all(not t.is_alive() for t in handlers),
                          proxy_failures=failures,
                          app_sha256=hashlib.sha256(app.read_bytes()).hexdigest())
            for name, value in [('result', result), ('calls', calls), ('acknowledgements', observations), ('partial-chunks', partial_chunks)]:
                (output / (name + '.json')).write_text(json.dumps(value, indent=2) + '\n')
        assert result['engine_joined'] and result['proxy_joined'] and result['bulk_proxy_joined'] and result['handlers_joined']


if __name__ == '__main__':
    main()
