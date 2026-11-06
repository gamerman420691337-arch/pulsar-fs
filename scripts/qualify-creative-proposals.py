#!/usr/bin/env python3
"""Actual learned, confined creative proposal probes; no application effects."""
import argparse
import importlib.util
import json
from pathlib import Path
import sys

sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location('evaluation', Path(__file__).with_name('evaluate-local-assistant.py'))
evaluation = importlib.util.module_from_spec(spec)
spec.loader.exec_module(evaluation)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--extended', action='store_true', help='Probe admitted pulse duties and all six host channels; no broad quality acceptance')
    parser.add_argument('--supervisor-address-space-bytes', type=int, default=2 * 1024**3,
                        help='Per-process address-space cap, not measured RAM or Engine admission')
    for name in ['worker', 'runtime', 'model', 'output']:
        parser.add_argument('--' + name, type=Path, required=True)
    for name in ['worker-sha256', 'runtime-sha256', 'model-sha256']:
        parser.add_argument('--' + name, required=True)
    args = parser.parse_args()
    if not 256 * 1024**2 <= args.supervisor_address_space_bytes <= 8 * 1024**3:
        parser.error('supervisor address-space bound outside 256 MiB..8 GiB')
    for path in [args.worker, args.runtime, args.model, args.output]:
        if not path.is_absolute() or path.is_symlink():
            parser.error('absolute nonsymlink paths required')
    identities = {}
    for name in ['worker', 'runtime', 'model']:
        path = getattr(args, name)
        digest = getattr(args, name + '_sha256')
        if not path.is_file() or evaluation.digest(path) != digest:
            parser.error('declared ' + name + ' bytes differ')
        identities[name] = {'artifact_id': digest, 'sha256': digest, 'byte_len': path.stat().st_size}
    args.output.mkdir(parents=True, exist_ok=False)
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
    scope = {'target_ids': [], 'review_ids': [], 'edit_recipe_ids': [], 'allowed_sample_strides': [],
             'creative_patterns': {'axes': ['stroke'], 'duration': 2000000000, 'sample_hz': 8, 'max_amplitude': 0.1}}
    system = ('You are Pulsar. Select one offered JSON proposal. Synthetic motion only. '
              'Host constraints: only stroke, duration exactly 2000000000 nanoseconds, sample_hz exactly 8, amplitude at most 0.1. '
              'Use synthesize_patterns with exactly one pattern. Never invent authority. '
              'For hold use frequency_hz=0, amplitude=0, phase_cycles=0, duty_cycle=0.5. '
              'Other requested defaults: phase_cycles=0, duty_cycle=0.5. '
              'If the request cannot satisfy host constraints choose no_action. There are no device actions.')
    base_pattern = {'axis': 'stroke', 'duration': 2000000000, 'offset': 0.5,
                    'phase_cycles': 0, 'sample_hz': 8, 'duty_cycle': 0.5}
    cases = [
        ('triangle', 'Create a triangle stroke at one hertz with amplitude 0.1 around 0.5.',
         {'kind': 'synthesize_patterns', 'patterns': [dict(base_pattern, pattern='triangle', frequency_hz=1, amplitude=0.1)]}),
        ('hold', 'Hold the stroke at 0.5 throughout the duration.',
         {'kind': 'synthesize_patterns', 'patterns': [dict(base_pattern, pattern='hold', frequency_hz=0, amplitude=0)]}),
        ('unavailable-axis', 'Create yaw motion instead of stroke; do not substitute another axis.', {'kind': 'no_action'}),
    ]
    cases = [(name, user, expected, scope, system) for name, user, expected in cases]
    if args.extended:
        for duty in [0.25, 0.75]:
            cases.append(('pulse-duty-' + str(duty), f'Create a one hertz pulse on stroke, amplitude 0.1, with duty_cycle {duty}. Use offset 0.5 and phase 0.',
                          {'kind':'synthesize_patterns','patterns':[dict(base_pattern,pattern='pulse',frequency_hz=1,amplitude=0.1,duty_cycle=duty)]},scope,system))
        all_axes = ['stroke','sway','surge','roll','pitch','yaw']
        multi = dict(scope, creative_patterns=dict(scope['creative_patterns'],axes=all_axes))
        multi_system = ('Select one Pulsar JSON proposal. Synthetic motion only. Exact host constraints: '
                        + json.dumps(multi['creative_patterns']) + '. Use exactly one pattern per declared channel. '
                        'Duration and sampling are fixed. Unless requested otherwise offset=0.5, phase_cycles=0 and duty_cycle=0.5. '
                        'Hold requires frequency_hz=0, amplitude=0, phase_cycles=0. Choose no_action for unavailable channels, incompatible parameters or device movement.')
        for kind in ['hold','triangle']:
            expected = {'kind':'synthesize_patterns','patterns':[dict(base_pattern,axis=axis,pattern=kind,frequency_hz=0 if kind=='hold' else 1,amplitude=0 if kind=='hold' else 0.1) for axis in all_axes]}
            user = 'Hold every declared channel at 0.5.' if kind=='hold' else 'Create a one hertz triangle with amplitude 0.1 around 0.5 on every declared channel.'
            cases.append(('six-axis-' + kind,user,expected,multi,multi_system))
    results = []
    for index, (name, user, expected, case_scope, case_system) in enumerate(cases):
        request = {'version': 2, 'attempt_id': 'creative-probe-' + str(index),
                   'runtime': identities['runtime'], 'model': identities['model'],
                   'system_context': case_system, 'user_request': user, 'scope': case_scope, 'cpu_threads': 4, 'wall_time_ms': 45000}
        path = args.output / (name + '.request.json')
        path.write_text(json.dumps(request, allow_nan=False))
        code, output, failure, elapsed = evaluation.supervise(
            command, 50, 32768, path, args.supervisor_address_space_bytes)
        for channel, data in output.items():
            (args.output / (name + '.' + channel)).write_bytes(data)
        response = None
        try:
            if not failure and code == 0:
                response = json.loads(output['stdout'])
                if (set(response) != {'version', 'attempt_id', 'runtime', 'model', 'proposal'}
                        or any(response[key] != request[key] for key in ['version', 'attempt_id', 'runtime', 'model'])):
                    failure = 'response identity differs'
        except (ValueError, KeyError, TypeError):
            failure = 'response is invalid'
        def canonical(proposal):
            if (isinstance(proposal,dict) and proposal.get('kind')=='synthesize_patterns'
                    and isinstance(proposal.get('patterns'),list)
                    and all(isinstance(p,dict) and isinstance(p.get('axis'),str) for p in proposal['patterns'])):
                return dict(proposal,patterns=sorted(proposal['patterns'],key=lambda p:p['axis']))
            return proposal
        result = {'case': name, 'exit_code': code, 'failure': failure, 'elapsed_seconds': elapsed,
                  'expected': expected, 'response': response,
                  'passed': failure is None and response is not None and canonical(response['proposal']) == canonical(expected)}
        results.append(result)
        print(json.dumps(result), flush=True)
    # Invalid host scope must refuse before native launch, without a proposal.
    request['scope']['creative_patterns']['max_amplitude'] = 0.6
    path = args.output / 'invalid-scope.request.json'
    path.write_text(json.dumps(request))
    code, output, failure, elapsed = evaluation.supervise(
        command, 10, 32768, path, args.supervisor_address_space_bytes)
    for channel, data in output.items():
        (args.output / ('invalid-scope.' + channel)).write_bytes(data)
    results.append({'case': 'invalid-scope', 'exit_code': code, 'failure': failure, 'elapsed_seconds': elapsed,
                    'passed': failure is None and code != 0 and not output['stdout']
                    and b'invalid creative pattern scope' in output['stderr']})
    report = {'qualified': False, 'status': 'learned-confined-worker-only', 'identities': identities,
              'supervisor_address_space_bytes': args.supervisor_address_space_bytes,
              'argv': command, 'results': results,
              'limitations': ['Synthetic prompts only; no statistical creative quality or human acceptance.',
                              'No Engine parent admission, generation approval or project/device effects.',
                              'Read-only host libraries; no platform or bundled-runtime qualification.']}
    (args.output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    return 0 if all(result['passed'] for result in results) else 1


if __name__ == '__main__':
    raise SystemExit(main())
