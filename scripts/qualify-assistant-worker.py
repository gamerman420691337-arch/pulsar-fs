#!/usr/bin/env python3
"""Bounded Linux namespace probe of the actual Pulsar assistant worker role.

Development evidence only: no engine dispatch, aggregate admission, effect,
packaging, human acceptance or statistical model qualification.
"""
import argparse
import importlib.util
import json
from pathlib import Path
import sys
sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location('assistant_evaluation', Path(__file__).with_name('evaluate-local-assistant.py'))
evaluation = importlib.util.module_from_spec(spec)
spec.loader.exec_module(evaluation)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ['worker', 'runtime', 'model', 'system-context-file', 'output']:
        parser.add_argument('--' + name, type=Path, required=True)
    parser.add_argument('--worker-sha256', required=True)
    parser.add_argument('--runtime-sha256', required=True)
    parser.add_argument('--candidate', choices=list(evaluation.CANDIDATES), required=True)
    parser.add_argument('--supervisor-address-space-bytes', type=int, default=2 * 1024**3,
                        help='Per-process address space, not measured RAM or Engine admission')
    args = parser.parse_args()
    if not 256 * 1024**2 <= args.supervisor_address_space_bytes <= 8 * 1024**3:
        parser.error('supervisor address-space bound outside 256 MiB..8 GiB')
    for path in [args.worker, args.runtime, args.model, args.system_context_file, args.output]:
        if not path.is_absolute():
            parser.error('all paths must be absolute')
    model_hash, model_bytes = evaluation.CANDIDATES[args.candidate]
    if (evaluation.digest(args.worker) != args.worker_sha256 or evaluation.digest(args.runtime) != args.runtime_sha256
            or evaluation.digest(args.model) != model_hash or args.model.stat().st_size != model_bytes):
        parser.error('declared worker/runtime/model bytes changed')
    args.output.mkdir(exist_ok=False, parents=True)
    system = args.system_context_file.read_text()
    scope = {'target_ids': ['cup1', 'cup2', 'beaker1'], 'review_ids': ['flag1'],
             'edit_recipe_ids': ['smooth_selection'], 'allowed_sample_strides': [1, 4]}
    identity = lambda path, sha: {'artifact_id': sha, 'sha256': sha, 'byte_len': path.stat().st_size}
    results = []
    for index, (name, user, expected) in enumerate(evaluation.CASES):
        request = {'version': 2, 'attempt_id': 'assistant-probe-' + str(index),
                   'runtime': identity(args.runtime, args.runtime_sha256), 'model': identity(args.model, model_hash),
                   'system_context': system, 'user_request': user, 'scope': scope, 'cpu_threads': 4, 'wall_time_ms': 45000}
        path = args.output / (name + '.request.json')
        path.write_text(json.dumps(request))
        command = ['/usr/bin/bwrap', '--unshare-all', '--die-with-parent', '--new-session', '--clearenv']
        for root in ['/usr', '/lib', '/lib64', '/bin', '/etc/ld.so.cache']:
            if Path(root).exists():
                command.extend(['--ro-bind', root, root])
        command.extend(['--proc', '/proc', '--dev', '/dev', '--size', '16777216', '--tmpfs', '/tmp',
                        '--ro-bind', str(args.worker), '/pulsar', '--ro-bind', str(args.runtime), '/assistant/runtime',
                        '--ro-bind', str(args.model), '/assistant/model.gguf', '--chdir', '/tmp',
                        '--remount-ro', '/proc', '--remount-ro', '/dev', '--remount-ro', '/',
                        '--setenv', 'PATH', '/usr/bin:/bin', '--setenv', 'HOME', '/tmp',
                        '--setenv', 'PULSAR_WORKER_CONFINED', '1', '/pulsar', 'assistant-worker'])
        code, output, failure, elapsed = evaluation.supervise(command, 50, 32768, path, args.supervisor_address_space_bytes)
        for channel, data in output.items():
            (args.output / (name + '.' + channel)).write_bytes(data)
        response = None
        if not failure and code == 0:
            try:
                response = json.loads(output['stdout'])
                if (set(response) != {'version', 'attempt_id', 'runtime', 'model', 'proposal'}
                    or response['version'] != request['version'] or response['attempt_id'] != request['attempt_id']
                    or response['runtime'] != request['runtime'] or response['model'] != request['model']):
                    failure = 'mismatched response scope/identity'
            except (ValueError, KeyError, TypeError) as error:
                failure = 'invalid strict worker response: ' + str(error)
        result = {'case': name, 'exit_code': code, 'failure': failure, 'elapsed_seconds': elapsed,
                  'expected': expected, 'response': response,
                  'passed': failure is None and response is not None and response['proposal'] == expected,
                  'argv': command}
        results.append(result)
        print(json.dumps(result), flush=True)
    negative_results = []
    for name in ['unconfined-entry', 'extra-role-argument', 'unknown-request-field', 'oversized-input', 'wrong-model-digest', 'prelaunch-deadline', 'native-deadline']:
        altered = json.loads(json.dumps(request))
        altered_command = list(command)
        expected_diagnostic = None
        if name == 'unconfined-entry':
            altered_command = [str(args.worker), 'assistant-worker']
            expected_diagnostic = 'parent-owned confinement'
        elif name == 'extra-role-argument':
            altered_command.append('--arbitrary-command')
            expected_diagnostic = 'bounded data-only request'
        elif name == 'unknown-request-field':
            altered['approved'] = True
            expected_diagnostic = 'unknown field'
        elif name == 'wrong-model-digest':
            altered['model']['sha256'] = '0' * 64
            expected_diagnostic = 'digest mismatch'
        elif name == 'prelaunch-deadline':
            altered['wall_time_ms'] = 300
            expected_diagnostic = 'assistant whole-worker deadline exceeded'
        elif name == 'native-deadline':
            altered['wall_time_ms'] = 2000
            expected_diagnostic = 'worker wall deadline exceeded'
        path = args.output / (name + '.request.json')
        path.write_text(' ' * 32769 if name == 'oversized-input' else json.dumps(altered))
        if name == 'oversized-input':
            expected_diagnostic = 'transport exceeded'
        code, output, failure, elapsed = evaluation.supervise(altered_command, 10, 32768, path, args.supervisor_address_space_bytes)
        for channel, data in output.items():
            (args.output / (name + '.' + channel)).write_bytes(data)
        diagnostic = output['stderr'].decode('utf-8', errors='replace')
        result = {'case': name, 'exit_code': code, 'failure': failure, 'elapsed_seconds': elapsed,
                  'expected_diagnostic': expected_diagnostic, 'passed': failure is None and code != 0
                  and not output['stdout'] and expected_diagnostic in diagnostic, 'argv': altered_command}
        negative_results.append(result)
        print(json.dumps(result), flush=True)
    report = {'status': 'Linux-worker-role-probe-only', 'worker_sha256': args.worker_sha256,
              'supervisor_address_space_bytes': args.supervisor_address_space_bytes,
              'runtime_sha256': args.runtime_sha256, 'model_sha256': model_hash,
              'passed': sum(result['passed'] for result in results), 'total': len(results), 'results': results,
              'negative_passed': sum(result['passed'] for result in negative_results), 'negative_total': len(negative_results), 'negative_results': negative_results,
              'limitations': ['Private PID/network/filesystem namespaces and RLIMIT_AS; no Engine cgroup/fence/pool dispatch.',
                              'Host system libraries exposed read-only; no captured dependency closure proof.',
                              'Only synthetic contexts; no application edits/settings/review or model qualification.']}
    (args.output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    return 0 if report['passed'] == len(results) and report['negative_passed'] == len(negative_results) else 1


if __name__ == '__main__':
    raise SystemExit(main())
