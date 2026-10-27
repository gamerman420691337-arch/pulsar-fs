#!/usr/bin/env python3
"""Owned learned CLI candidate probe. No device, GUI or model qualification."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import signal
import stat
import subprocess
import tempfile
import time
import uuid
import sys

sys.dont_write_bytecode = True

spec = importlib.util.spec_from_file_location('assistant_probe', Path(__file__).with_name('qualify-assistant-cli.py'))
helper = importlib.util.module_from_spec(spec)
spec.loader.exec_module(helper)

def custody_fields(ledger):
    def identifier(value):
        if not isinstance(value, str) or str(uuid.UUID(value)) != value:
            raise ValueError('invalid custody identifier')
        return value
    def group(value):
        if (not isinstance(value, dict) or set(value) != {'path', 'device', 'inode'}
                or not isinstance(value['path'], str) or not value['path'].startswith('/sys/fs/cgroup/')
                or len(value['path']) > 4096
                or any(type(value[k]) is not int or not 0 < value[k] < 2**64 for k in ('device', 'inode'))):
            raise ValueError('invalid custody cgroup identity')
        return value
    scope = identifier(ledger['scope'])
    def groups(values, creation=False):
        if not isinstance(values, dict) or len(values) > 4096:
            raise ValueError('invalid custody group roster')
        result = {}
        for key, value in values.items():
            if not isinstance(key, str):
                raise ValueError('invalid custody group key')
            prefix = 'pulsar-worker-' + scope + '-'
            identifier(key[len(prefix):] if key.startswith(prefix) else key)
            if creation and value == 'Intent':
                result[key] = value
            elif creation and isinstance(value, dict) and len(value) == 1 and next(iter(value)) in ('Bound', 'Retired'):
                result[key] = {k: group(v) for k, v in value.items()}
            else:
                result[key] = group(value)
        return result
    if type(ledger['version']) is not int or not 1 <= ledger['version'] <= 4:
        raise ValueError('invalid custody ledger version')
    result = {'version': ledger['version'], 'scope': scope, 'groups': groups(ledger['groups']),
              'retired': groups(ledger.get('retired', {}))}
    if ledger.get('creation') is not None:
        value = ledger['creation']
        if not isinstance(value, dict) or set(value) != {'parent', 'boot', 'records'}:
            raise ValueError('invalid custody creation fields')
        result['creation'] = {'parent': group(value['parent']), 'boot': identifier(value['boot']),
                              'records': groups(value['records'], True)}
    return result

def capture_custody(root, out, label):
    """Retain bounded nonsecret cgroup identities before temporary state removal."""
    result = {'qualified': False, 'label': label}
    try:
        fd = os.open(root / 'state/worker-fence.json', os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
        with os.fdopen(fd, 'rb') as stream:
            metadata = os.fstat(stream.fileno())
            if (not stat.S_ISREG(metadata.st_mode) or metadata.st_uid != os.geteuid()
                    or metadata.st_mode & 0o7777 != 0o600 or metadata.st_size > 1024**2):
                raise ValueError('custody metadata is not private bounded regular data')
            data = stream.read(1024**2 + 1)
        if len(data) > 1024**2:
            raise ValueError('custody snapshot exceeds bound')
        ledger = json.loads(data)
        if not isinstance(ledger, dict):
            raise ValueError('custody ledger is not an object')
        # These known ledger fields carry cgroup identities, never session tokens.
        result['ledger'] = custody_fields(ledger)
    except (OSError, ValueError, TypeError, KeyError) as error:
        result['observation_error'] = str(error)[:1024]
    try:
        (out / ('custody-' + label + '.json')).write_text(json.dumps(result, indent=2) + '\n')
    except OSError as error:
        # Diagnostics must never prevent owned Engine stop/join in finally.
        print(f'custody snapshot write failed (errno {error.errno})', file=sys.stderr)

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--pattern-case', choices=['triangle','pulse25','pulse75','unavailable-axis'], default='triangle')
    for name in ('application', 'worker', 'controller', 'runtime', 'model', 'media', 'output'):
        parser.add_argument('--' + name, type=Path, required=True)
    parser.add_argument('--model-sha256', default='9465e63a22add5354d9bb4b99e90117043c7124007664907259bd16d043bb031',
                        help='Exact test candidate pin; does not select a production default')
    parser.add_argument('--runtime-sha256', default='f332257812cf358954a1701c9fb205e5ac50b4fbdf0e2c09944cc25ea58459ae')
    parser.add_argument('--bundled-runtime', type=Path, help='Owned writable candidate: verify manifest, use adjacent assets and actual portable assistant-setup')
    parser.add_argument('--persist-assistant-selection', action='store_true', help='Exercise explicit selection and disable through the owned bundled app')
    args = parser.parse_args()
    if args.persist_assistant_selection and args.bundled_runtime is None:
        parser.error('persistent selection probe requires an owned bundled runtime')
    if (len(args.runtime_sha256) != 64 or any(c not in '0123456789abcdef' for c in args.runtime_sha256)
            or not args.runtime.is_file() or helper.digest(args.runtime) != args.runtime_sha256):
        parser.error('declared runtime bytes differ from candidate pin')
    if args.bundled_runtime is not None:
        bundle = args.bundled_runtime.resolve()
        manifest = json.loads((bundle / 'runtime-candidate.json').read_text())
        if manifest.get('kind') != 'linux-x86-64-runtime-candidate' or manifest.get('qualified') is not False:
            parser.error('expected an unqualified runtime candidate manifest')
        entries = manifest.get('files')
        if not isinstance(entries, list) or not 1 <= len(entries) <= 128:
            parser.error('invalid candidate file roster')
        seen = set()
        for entry in entries:
            relative = Path(entry['path'])
            path = bundle / relative
            if (relative.is_absolute() or '..' in relative.parts or entry['path'] in seen
                    or path.is_symlink() or not path.resolve().is_relative_to(bundle)
                    or not path.is_file() or path.stat().st_size != entry['byte_len']
                    or helper.digest(path) != entry['sha256']):
                parser.error('candidate member identity differs')
            seen.add(entry['path'])
        for supplied, relative in ((args.application, 'pulsar'), (args.worker, 'pulsar-worker'),
                                   (args.runtime, 'assistant/llama-completion'), (args.model, 'assistant/model.gguf')):
            if supplied.resolve() != bundle / relative or relative not in seen:
                parser.error('supplied asset is not the declared adjacent candidate member')
        if args.media.resolve() != bundle / 'runtimes':
            parser.error('media directory is not adjacent candidate runtimes')
        if not {'runtimes/ffmpeg', 'runtimes/ffprobe', 'runtimes/libonnxruntime.so.1'} <= seen:
            parser.error('candidate lacks media or CPU runtime assets')
    if (len(args.model_sha256) != 64 or any(c not in '0123456789abcdef' for c in args.model_sha256)
            or not args.model.is_file() or helper.digest(args.model) != args.model_sha256):
        parser.error('declared model bytes differ from candidate pin')
    out = args.output.resolve()
    out.mkdir(mode=0o700)
    calls, server = [], None
    supervision = {'qualified': False, 'join_confirmed': False}
    owned_policy = None
    saved_selection_digest = None
    with tempfile.TemporaryDirectory(prefix='creative-cli-') as temporary:
        root = Path(temporary)
        try:
            def identity(path, label):
                return {'artifact_id': label, 'sha256': helper.digest(path), 'byte_len': path.stat().st_size}
            runtime, model = identity(args.runtime, 'pinned-runtime'), identity(args.model, 'pinned-model')
            assert runtime['sha256'] == args.runtime_sha256
            assert model['sha256'] == args.model_sha256
            policy = {'schema_version': 1, 'approve_inference': True, 'worker_sha256': helper.digest(args.worker), 'worker_bytes': args.worker.stat().st_size, 'runtime_path': str(args.runtime.resolve()), 'runtime': runtime, 'model_path': str(args.model.resolve()), 'model': model, 'cpu_threads': 4, 'wall_time_ms': 45000, 'worker_memory_bytes': 2147483648}
            policy_path = root / 'owner-policy.json'
            policy_path.write_text(json.dumps(policy))
            policy_path.chmod(0o600)
            env = {'PATH': '/usr/bin:/bin', 'HOME': os.environ['HOME'], 'TMPDIR': str(root), 'PULSAR_STATE_DIR': str(root / 'state'), 'PULSAR_WORKER_EXECUTABLE': str(args.worker.resolve()), 'PULSAR_ASSISTANT_OWNER_POLICY': str(policy_path), 'PULSAR_FFMPEG': str(args.media.resolve() / 'ffmpeg'), 'PULSAR_FFPROBE': str(args.media.resolve() / 'ffprobe'), 'PULSAR_ORT_DYLIB': str(args.media.resolve() / 'libonnxruntime.so.1')}
            if args.bundled_runtime is not None:
                env = {key: env[key] for key in ('PATH', 'HOME', 'TMPDIR', 'PULSAR_STATE_DIR')}
                policy_path = bundle / ('.creative-probe-' + uuid.uuid4().hex + '.json')
                setup = helper.call([str(args.application.resolve()), 'assistant-setup', '--output', str(policy_path),
                    '--runtime', str(args.runtime.resolve()), '--model', str(args.model.resolve()),
                    '--cpu-threads', '4', '--wall-time-ms', '45000', '--worker-memory-bytes', '2147483648',
                    '--approve-host-inference'], env)
                calls.append(setup)
                if policy_path.exists():
                    metadata = policy_path.lstat()
                    owned_policy = (policy_path, metadata.st_dev, metadata.st_ino, helper.digest(policy_path))
                assert setup['exit_code'] == 0, setup
                portable = json.loads(policy_path.read_text())
                assert portable['schema_version'] == 2
                assert not Path(portable['runtime_path']).is_absolute()
                assert not Path(portable['model_path']).is_absolute()
                if args.persist_assistant_selection:
                    selected = helper.call([str(args.application.resolve()), 'assistant-select', '--policy', str(policy_path), '--approve-host-inference'], env)
                    calls.append(selected)
                    assert selected['exit_code'] == 0, selected
                    saved_selection_digest = helper.digest(root / 'state/assistant-startup-policy.json')
                    supervision['saved_selection_without_environment_override'] = True
                else:
                    env['PULSAR_ASSISTANT_OWNER_POLICY'] = str(policy_path)
                (out / 'bundle-mode.json').write_text(json.dumps({
                    'qualified': False, 'manifest_sha256': helper.digest(bundle / 'runtime-candidate.json'),
                    'verified_members': len(seen), 'portable_policy_schema': 2,
                    'asset_environment_overrides': False, 'explicit_host_approval': True,
                    'persistent_selection': args.persist_assistant_selection}, indent=2) + '\n')
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
            project = cli('project', 'create', 'Creative CLI probe')['project_id']
            profile = cli('assistant', 'profiles', project)[0]['profile_id']
            before = cli('project', 'open', project)
            scope = root / 'scope.json'
            scope.write_text(json.dumps({'axes': ['stroke'], 'duration': 2000000000, 'sample_hz': 8, 'max_amplitude': .1}))
            start = root / 'start.json'
            duty = {'triangle': .5, 'pulse25': .25, 'pulse75': .75, 'unavailable-axis': .5}[args.pattern_case]
            pattern = 'triangle' if args.pattern_case == 'triangle' else 'pulse'
            prompt = ('Create a triangle stroke at one hertz with amplitude 0.1 around 0.5.' if pattern == 'triangle' else f'Create a one hertz pulse on stroke, amplitude 0.1, with duty_cycle {duty}. Use offset 0.5 and phase 0.')
            if args.pattern_case == 'unavailable-axis':
                prompt = 'Create yaw motion instead of stroke; do not substitute another axis.'
            cli('assistant', 'prepare', project, '--profile', profile, '--revision', '0', '--prompt', prompt, '--creative-scope', str(scope), '--request-id', 'creative-start-' + uuid.uuid4().hex, '--output', str(start))
            assert start.stat().st_mode & 0o777 == 0o600
            assert cli('project', 'open', project) == before
            accepted = cli('assistant', 'start', '--intent', str(start), '--approve-inference')
            end = time.monotonic() + 60
            while True:
                report = cli('assistant', 'status', project, accepted['attempt_id'])
                if report['state'] != 'running':
                    break
                assert time.monotonic() < end, 'assistant deadline'
                time.sleep(.1)
            if args.pattern_case == 'unavailable-axis':
                unchanged = cli('project', 'open', project) == before
                (out / 'observation.json').write_text(json.dumps({
                    'qualified': False, 'expected_proposal': {'kind': 'no_action'},
                    'report': report, 'master_unchanged': unchanged,
                    'generation_approved': False,
                    'intent_passed': report['state'] == 'completed' and report['proposal'] == {'kind': 'no_action'}
                }, indent=2) + '\n')
                assert unchanged
                assert report['state'] == 'completed' and report['proposal'] == {'kind': 'no_action'}, report
                assert cli('project', 'open', project) == before
                cli('assistant', 'prepare-creative-generation', project, accepted['attempt_id'],
                    '--revision', '0', '--request-id', 'no-action-generation-' + uuid.uuid4().hex,
                    '--output', str(root / 'generation.json'), refused=True)
                assert not (root / 'generation.json').exists()
                assert cli('project', 'open', project) == before
                result = {'qualified': False, 'pattern_case': args.pattern_case,
                          'scope': 'One actual Engine/CLI unavailable-axis request; no broad model quality qualification',
                          'report': report, 'master_unchanged': True, 'generation_refused': True,
                          'saved_generation_absent': True,
                          'application': identity(args.application, 'application'),
                          'worker': identity(args.worker, 'worker'), 'runtime': runtime, 'model': model}
                (out / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
                supervision['success'] = True
                return
            expected = {'kind': 'synthesize_patterns', 'patterns': [{'pattern': pattern, 'axis': 'stroke', 'duration': 2000000000, 'frequency_hz': 1, 'amplitude': .1, 'offset': .5, 'phase_cycles': 0, 'sample_hz': 8, 'duty_cycle': duty}]}
            assert report['state'] == 'completed' and report['proposal'] == expected, report
            assert cli('project', 'open', project) == before
            generation = root / 'generation.json'
            request = 'creative-generation-' + uuid.uuid4().hex
            cli('assistant', 'prepare-creative-generation', project, accepted['attempt_id'], '--revision', '0', '--request-id', request, '--output', str(generation))
            saved = generation.read_bytes()
            assert generation.stat().st_mode & 0o777 == 0o600
            cli('assistant', 'prepare-creative-generation', project, accepted['attempt_id'], '--revision', '0', '--request-id', request, '--output', str(generation), refused=True)
            assert generation.read_bytes() == saved
            cli('assistant', 'generate-saved', '--intent', str(generation), refused=True)
            assert cli('project', 'open', project) == before
            job = cli('assistant', 'generate-saved', '--intent', str(generation), '--approve-generation')
            replay = cli('assistant', 'generate-saved', '--intent', str(generation), '--approve-generation')
            assert replay['job_id'] == job['job_id']
            controlled = helper.call([str(args.controller.resolve()), str(root / 'state'), project, job['job_id'],args.pattern_case], env, deadline=70)
            calls.append(controlled)
            assert controlled['exit_code'] == 0, controlled
            oracle = json.loads(controlled['stdout'])
            export = root / 'triangle.funscript'
            cli('export-axis', project, str(export), '--axis', 'stroke')
            cycle = [0, .5, 1, .5, 0, -.5, -1, -.5]
            if pattern == 'pulse':
                cycle = [1 if k < duty * 8 else -1 for k in range(8)]
            assert json.loads(export.read_text())['actions'] == [{'at': k * 125, 'pos': round(50 + 10 * cycle[k % 8])} for k in range(17)]
            cli('assistant', 'creative-candidate', project, accepted['attempt_id'], '--revision', '0', '--request-id', 'stale-' + uuid.uuid4().hex, '--approve-generation', refused=True)
            result = {'qualified': False, 'pattern_case':args.pattern_case, 'scope': 'Actual Linux Engine/CLI learned proposal and separately approved synthetic candidate; no GUI/device or broad model acceptance', 'report': report, 'independent_oracle': oracle, 'exact_generation_replay': True, 'private_inert_intents': True, 'non_overwrite': True, 'missing_approval_refused': True, 'stale_revision_refused': True, 'export_actions': 17, 'application': identity(args.application, 'application'), 'worker': identity(args.worker, 'worker'), 'controller': identity(args.controller, 'controller'), 'runtime': runtime, 'model': model}
            (out / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
            supervision['success'] = True
        except BaseException as error:
            supervision['failure'] = str(error)
            raise
        finally:
            capture_custody(root, out, 'before-stop')
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
            capture_custody(root, out, 'after-stop')
            cleanup_error = None
            try:
                if saved_selection_digest is not None and supervision['join_confirmed']:
                    disabled = helper.call([str(args.application.resolve()), 'assistant-disable',
                        '--expected-policy-sha256', saved_selection_digest, '--approve-disable'], env)
                    calls.append(disabled)
                    assert disabled['exit_code'] == 0, disabled
                    assert not (root / 'state/assistant-startup-policy.json').exists()
                    supervision['saved_selection_disabled'] = True
                    restarted = None
                    try:
                        with (out / 'disabled-server.log').open('wb') as log:
                            restarted = subprocess.Popen([str(args.application.resolve()), 'engine'], env=env,
                                stdin=subprocess.DEVNULL, stdout=log, stderr=log, start_new_session=True)
                        supervision['disabled_server_pid'] = restarted.pid
                        end = time.monotonic() + 20
                        while True:
                            assert restarted.poll() is None and time.monotonic() < end, 'disabled restart readiness failed'
                            profiles = helper.call([str(args.application.resolve()), 'assistant', 'profiles', project], env)
                            calls.append(profiles)
                            if profiles['exit_code'] == 0:
                                assert json.loads(profiles['stdout'])['value'] == [], profiles
                                supervision['disabled_restart_has_no_profiles'] = True
                                break
                            time.sleep(.1)
                    finally:
                        if restarted is not None:
                            if restarted.poll() is None:
                                os.killpg(restarted.pid, signal.SIGTERM)
                                try:
                                    restarted.wait(timeout=5)
                                except subprocess.TimeoutExpired:
                                    os.killpg(restarted.pid, signal.SIGKILL)
                                    restarted.wait(timeout=5)
                            supervision['disabled_server_join_confirmed'] = restarted.poll() is not None
                            capture_custody(root, out, 'disabled-after-stop')
            except BaseException as error:
                cleanup_error = error
                supervision['selection_cleanup_failure'] = str(error)
            if owned_policy is not None:
                path, device, inode, digest = owned_policy
                metadata = path.lstat()
                if (metadata.st_dev, metadata.st_ino, helper.digest(path)) != (device, inode, digest):
                    raise RuntimeError('owned probe policy identity changed; retained for inspection')
                path.unlink()
                supervision['owned_policy_removed'] = True
            (out / 'calls.json').write_text(json.dumps(calls, indent=2) + '\n')
            (out / 'supervision.json').write_text(json.dumps(supervision, indent=2) + '\n')
            if cleanup_error is not None:
                raise cleanup_error

if __name__ == '__main__':
    main()
