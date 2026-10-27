#!/usr/bin/env python3
"""Owned native desktop-controller semantics; no rendered GUI/device qualification."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import signal
import subprocess
import tempfile
import time

spec = importlib.util.spec_from_file_location('assistant_probe', Path(__file__).with_name('qualify-assistant-cli.py'))
helper = importlib.util.module_from_spec(spec)
spec.loader.exec_module(helper)

def main():
    parser = argparse.ArgumentParser()
    for name in ('application', 'worker', 'panel-test', 'candidate-controller', 'runtime', 'model', 'media', 'output'):
        parser.add_argument('--' + name, type=Path, required=True)
    args = parser.parse_args()
    out = args.output.resolve()
    out.mkdir(mode=0o700)
    calls, server = [], None
    supervision = {'qualified': False, 'join_confirmed': False}
    with tempfile.TemporaryDirectory(prefix='creative-desktop-') as temporary:
        root = Path(temporary)
        try:
            def identity(path, label):
                return {'artifact_id': label, 'sha256': helper.digest(path), 'byte_len': path.stat().st_size}
            runtime, model = identity(args.runtime, 'pinned-runtime'), identity(args.model, 'pinned-model')
            assert runtime['sha256'] == 'f332257812cf358954a1701c9fb205e5ac50b4fbdf0e2c09944cc25ea58459ae'
            assert model['sha256'] == '9465e63a22add5354d9bb4b99e90117043c7124007664907259bd16d043bb031'
            policy = {'schema_version': 1, 'approve_inference': True, 'worker_sha256': helper.digest(args.worker), 'worker_bytes': args.worker.stat().st_size, 'runtime_path': str(args.runtime.resolve()), 'runtime': runtime, 'model_path': str(args.model.resolve()), 'model': model, 'cpu_threads': 4, 'wall_time_ms': 45000, 'worker_memory_bytes': 2147483648}
            policy_path = root / 'owner-policy.json'
            policy_path.write_text(json.dumps(policy))
            policy_path.chmod(0o600)
            env = {'PATH': '/usr/bin:/bin', 'HOME': os.environ['HOME'], 'TMPDIR': str(root), 'PULSAR_STATE_DIR': str(root / 'state'), 'PULSAR_WORKER_EXECUTABLE': str(args.worker.resolve()), 'PULSAR_ASSISTANT_OWNER_POLICY': str(policy_path), 'PULSAR_FFMPEG': str(args.media.resolve() / 'ffmpeg'), 'PULSAR_FFPROBE': str(args.media.resolve() / 'ffprobe'), 'PULSAR_ORT_DYLIB': str(args.media.resolve() / 'libonnxruntime.so.1'), 'PYTHONDONTWRITEBYTECODE': '1'}
            def cli(*words, refused=False):
                result = helper.call([str(args.application.resolve()), *words], env)
                calls.append(result)
                if refused:
                    assert result['exit_code'] != 0, result
                    return result
                assert result['exit_code'] == 0, result
                return json.loads(result['stdout'])['value'] if result['stdout'].lstrip().startswith('{') else result['stdout']
            with (out / 'server.log').open('wb') as log:
                server = subprocess.Popen([str(args.application.resolve()), 'engine'], env=env, stdin=subprocess.DEVNULL, stdout=log, stderr=log, start_new_session=True)
            supervision['server_pid'] = server.pid
            end = time.monotonic() + 50
            while not (root / 'state/engine.sock').exists():
                assert server.poll() is None and time.monotonic() < end, 'readiness failed'
                time.sleep(.05)
            project = cli('project', 'create', 'Creative desktop controller probe')['project_id']
            before = cli('project', 'open', project)
            env['PULSAR_DESKTOP_CREATIVE_STATE'] = str(root / 'state')
            env['PULSAR_DESKTOP_CREATIVE_PROJECT'] = project
            panel = helper.call([str(args.panel_test.resolve()), 'assistant_panel::tests::native_creative_desktop_controller', '--exact', '--ignored', '--nocapture', '--test-threads=1'], env, deadline=150)
            calls.append(panel)
            assert panel['exit_code'] == 0, panel
            lines = [line.split('PULSAR_DESKTOP_CREATIVE_RESULT=', 1)[1] for line in panel['stdout'].splitlines() if 'PULSAR_DESKTOP_CREATIVE_RESULT=' in line]
            assert len(lines) == 1, 'one original desktop result required'
            desktop = json.loads(lines[0])
            current = cli('project', 'open', project)
            assert current['revision'] == before['revision'] and current['program'] == before['program']
            controlled = helper.call([str(args.candidate_controller.resolve()), str(root / 'state'), project, desktop['job']['job_id']], env, deadline=70)
            calls.append(controlled)
            assert controlled['exit_code'] == 0, controlled
            oracle = json.loads(controlled['stdout'])
            export = root / 'triangle.funscript'
            cli('export-axis', project, str(export), '--axis', 'stroke')
            cycle = [0, .5, 1, .5, 0, -.5, -1, -.5]
            assert json.loads(export.read_text())['actions'] == [{'at': k * 125, 'pos': round(50 + 10 * cycle[k % 8])} for k in range(17)]
            cli('assistant', 'creative-candidate', project, desktop['report']['attempt_id'], '--revision', '0', '--request-id', 'stale-desktop-approval', '--approve-generation', refused=True)
            result = {'qualified': False, 'scope': 'Actual desktop execute/controller path against owned Linux Engine and confined learned worker; rendered GUI and broad model/device acceptance absent', 'desktop': desktop, 'independent_oracle': oracle, 'export_actions': 17, 'stale_revision_refused': True, 'application': identity(args.application, 'application'), 'worker': identity(args.worker, 'worker'), 'panel_test': identity(args.panel_test, 'panel-test'), 'candidate_controller': identity(args.candidate_controller, 'candidate-controller'), 'runtime': runtime, 'model': model}
            (out / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
            supervision['success'] = True
        except BaseException as error:
            supervision['failure'] = str(error)
            raise
        finally:
            if server is not None:
                if server.poll() is None:
                    try:
                        os.killpg(server.pid, signal.SIGTERM)
                        try:
                            server.wait(timeout=5)
                        except subprocess.TimeoutExpired:
                            os.killpg(server.pid, signal.SIGKILL)
                            server.wait(timeout=5)
                    except ProcessLookupError:
                        pass
                supervision['terminal'] = server.poll()
                supervision['join_confirmed'] = server.poll() is not None
            (out / 'calls.json').write_text(json.dumps(calls, indent=2) + '\n')
            (out / 'supervision.json').write_text(json.dumps(supervision, indent=2) + '\n')

if __name__ == '__main__':
    main()
