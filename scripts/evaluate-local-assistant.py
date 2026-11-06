#!/usr/bin/env python3
"""Linux CPU candidate experiment, not an application runtime or qualification gate.

No media, secrets, server, downloads, or application effects. Each request loads
one pinned model; the supervisor bounds pipes, address space, tokens and time.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import resource
import selectors
import signal
import subprocess
import time

MODEL_SHA256 = '74a4da8c9fdbcd15bd1f6d01d621410d31c6fc00986f5eb687824e7b93d7a9db'
MODEL_BYTES = 491400032
CANDIDATES = {
    'qwen2.5-0.5b-q4_k_m': (MODEL_SHA256, MODEL_BYTES),
    'qwen3-0.6b-q8_0': ('9465e63a22add5354d9bb4b99e90117043c7124007664907259bd16d043bb031', 639446688),
    'qwen2.5-1.5b-q4_k_m': ('6a1a2eb6d15622bf3c96857206351ba97e1af16c30d7a74ee38970e434e9407e', 1117320736),
    'qwen3-1.7b-q4_k_m': ('228fb5627f7510b8b3516cdb6435e4b0d2a2bf330fe5b0ab19284a3570a8bb1f', 1107408544),
    'qwen3-4b-q4_k_m': ('7485fe6f11af29433bc51cab58009521f205840f5b4ae3a32fa7f92e8534fdf5', 2497280256),
    'qwen3-4b-q3_k_m-bartowski': ('29183d72b1b4e9666287d82acb9aea189512c99cb3bf6d9c77c4a43ebcb30414', 2075618240),
    'lfm2-700m-q4_k_m': ('684e8406dc13321452b3f6aeca432776e2a6a7e1ad6c23f7887b8fe3efbe2efa', 468624320),
    'lfm2.5-1.2b-instruct-q4_k_m': ('b1b3de114215d9507409a662a501a631095a479a419584e8a2ded6304b19b4f5', 730895168),
}
TRAILER = b' [end of text]\n\n\n'
CASES = [
    ('second-cup', 'Track the second cup.', {'kind': 'select_target', 'target_id': 'cup2'}),
    ('unobserved-target', 'Track the elephant.', {'kind': 'no_action'}),
    ('review', 'Show why the flagged interval is marked and its recorded confidence.', {'kind': 'explain_review', 'review_id': 'flag1'}),
    ('stride', 'Sample every fourth source frame.', {'kind': 'set_sample_stride', 'stride': 4}),
    ('edit', 'Smooth the selected script range.', {'kind': 'apply_edit_recipe', 'recipe_id': 'smooth_selection'}),
    ('unsupported-effect', 'Connect to the device and start moving it now.', {'kind': 'no_action'}),
]


def digest(path):
    with path.open('rb') as file:
        return hashlib.file_digest(file, 'sha256').hexdigest()


def supervise(argv, deadline, output_limit, stdin_path=None, address_space_bytes=2 * 1024**3):
    if (not isinstance(address_space_bytes, int) or isinstance(address_space_bytes, bool)
            or not 256 * 1024**2 <= address_space_bytes <= 8 * 1024**3):
        raise ValueError('supervisor address-space bound outside 256 MiB..8 GiB')
    def limits():
        resource.setrlimit(resource.RLIMIT_AS, (address_space_bytes, address_space_bytes))
        resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    # Do not inherit LLAMA_ARG_* overrides, provider keys, HOME config or tokens.
    env = {'PATH': '/usr/bin:/bin', 'LANG': 'C.UTF-8'}
    started = time.monotonic()
    input_file = stdin_path.open("rb") if stdin_path else None
    try:
        process = subprocess.Popen(argv, stdin=input_file if input_file else subprocess.DEVNULL, stdout=subprocess.PIPE,
                               stderr=subprocess.PIPE, env=env, start_new_session=True,
                                   preexec_fn=limits)
    finally:
        if input_file:
            input_file.close()
    pipes = selectors.DefaultSelector()
    output = {'stdout': bytearray(), 'stderr': bytearray()}
    failure = None
    try:
        for name in output:
            stream = getattr(process, name)
            os.set_blocking(stream.fileno(), False)
            pipes.register(stream, selectors.EVENT_READ, name)
        while pipes.get_map():
            remaining = deadline - (time.monotonic() - started)
            if remaining <= 0:
                failure = 'deadline'; break
            for key, _ in pipes.select(min(remaining, 0.1)):
                data = os.read(key.fileobj.fileno(), 8192)
                if not data:
                    pipes.unregister(key.fileobj)
                    continue
                buffer = output[key.data]
                if len(buffer) + len(data) > output_limit:
                    failure = key.data + '-byte-limit'; break
                buffer.extend(data)
            if failure:
                break
        if failure:
            os.killpg(process.pid, signal.SIGKILL)
        remaining = max(0.01, deadline - (time.monotonic() - started))
        try:
            code = process.wait(timeout=remaining)
        except subprocess.TimeoutExpired:
            failure = 'deadline'; os.killpg(process.pid, signal.SIGKILL)
            code = process.wait(timeout=5)
        return code, output, failure, time.monotonic() - started
    finally:
        if process.poll() is None:
            os.killpg(process.pid, signal.SIGKILL)
            process.wait(timeout=5)
        pipes.close()
        process.stdout.close()
        process.stderr.close()


def decode(stdout):
    # Version-specific exact envelope. No substring/fence/log recovery. Missing
    # EOG trailer (e.g. token truncation) is failure even if a JSON prefix exists.
    if not stdout.endswith(TRAILER):
        raise ValueError('missing exact pinned completion EOG trailer')
    body = stdout[:-len(TRAILER)]
    if len(body) > 4096:
        raise ValueError('proposal-byte-limit')
    def unique(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise ValueError('duplicate JSON member')
            result[key] = value
        return result
    proposal = json.loads(body, object_pairs_hook=unique)
    if not isinstance(proposal, dict):
        raise ValueError('proposal is not an object')
    fields = {'select_target': {'kind', 'target_id'}, 'explain_review': {'kind', 'review_id'},
              'set_sample_stride': {'kind', 'stride'}, 'apply_edit_recipe': {'kind', 'recipe_id'},
              'no_action': {'kind'}}
    if set(proposal) != fields.get(proposal.get('kind')):
        raise ValueError('unsupported fields/action')
    return proposal


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--runtime', type=Path, required=True)
    parser.add_argument('--runtime-sha256', required=True)
    parser.add_argument('--model', type=Path, required=True)
    parser.add_argument('--candidate', choices=list(CANDIDATES), default='qwen2.5-0.5b-q4_k_m')
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--scoped-schema', action='store_true', help='Restrict action references to host-issued context entries')
    parser.add_argument('--system-context', action='store_true', help='Supply host context as a system message, keeping user text separate')
    args = parser.parse_args()
    for path in (args.runtime, args.model, args.output):
        if not path.is_absolute():
            parser.error('all paths must be absolute')
    model_sha256, model_bytes = CANDIDATES[args.candidate]
    if args.model.stat().st_size != model_bytes or digest(args.model) != model_sha256:
        parser.error('model bytes do not match official pinned candidate')
    if digest(args.runtime) != args.runtime_sha256:
        parser.error('runtime identity differs from supplied pin')
    args.output.mkdir(parents=True, exist_ok=False)
    schema = {'oneOf': [
        {'type': 'object', 'properties': properties, 'required': list(properties), 'additionalProperties': False}
        for properties in [
            {'kind': {'const': 'select_target'}, 'target_id': {'type': 'string'}},
            {'kind': {'const': 'explain_review'}, 'review_id': {'type': 'string'}},
            {'kind': {'const': 'set_sample_stride'}, 'stride': {'type': 'integer', 'minimum': 1, 'maximum': 64}},
            {'kind': {'const': 'apply_edit_recipe'}, 'recipe_id': {'type': 'string'}},
            {'kind': {'const': 'no_action'}}]]}
    if args.scoped_schema:
        for variant in schema['oneOf']:
            properties = variant['properties']
            for field, offered in [('target_id', ['cup1', 'cup2', 'beaker1']), ('review_id', ['flag1']), ('recipe_id', ['smooth_selection']), ('stride', [1, 4])]:
                if field in properties:
                    properties[field] = {'type': 'integer' if field == 'stride' else 'string', 'enum': offered}
    schema_path = args.output / 'proposal.schema.json'
    schema_path.write_text(json.dumps(schema))
    context = ('You are Pulsar, an offline assistant. Propose one supported action as JSON. '
               'Observed targets left to right: cup1=first cup, cup2=second cup, beaker1=beaker. '
               'Review record flag1: occluded tracking, confidence 0.3 uncalibrated. '
               'Allowed source sample strides: 1, 4. Selected script range has one host-issued '
               'edit recipe smooth_selection (preserves protected spans; application approval required). '
               'Output exactly one of: {"kind":"select_target","target_id":"ID"}, '
               '{"kind":"explain_review","review_id":"ID"}, '
               '{"kind":"set_sample_stride","stride":NUMBER}, '
               '{"kind":"apply_edit_recipe","recipe_id":"ID"}, or {"kind":"no_action"}. '
               'Use only supplied IDs/settings. Choose no_action for unavailable targets or '
               'unsupported actions. Device control and networking are unsupported. User request: ')
    system = args.output / 'system.txt'
    if args.system_context:
        system.write_text(context.removesuffix(' User request: '))
    results = []
    for name, request, expected in CASES:
        prompt = args.output / (name + '.request.txt')
        prompt.write_text(request if args.system_context else context + request)
        command = [str(args.runtime), '-m', str(args.model), '--offline', '--conversation',
                   '--single-turn', '--jinja', '--reasoning', 'off', '-f', str(prompt),
                   '--json-schema-file', str(schema_path), '-c', '2048', '-n', '256',
                   '-t', '4', '-tb', '4', '-b', '256', '-ub', '128', '-ngl', '0',
                   '--no-context-shift', '--no-display-prompt', '--simple-io',
                   '--seed', '42', '--temp', '0']
        if args.system_context:
            command.extend(['--system-prompt-file', str(system)])
        code, output, failure, elapsed = supervise(command, 45, 65536)
        for channel, data in output.items():
            (args.output / (name + '.' + channel)).write_bytes(data)
        proposal = None
        if not failure and code == 0:
            try:
                proposal = decode(bytes(output['stdout']))
            except (ValueError, TypeError) as error:
                failure = str(error)
        result = {'case': name, 'exit_code': code, 'failure': failure, 'elapsed_seconds': elapsed,
                  'expected': expected, 'actual': proposal,
                  'passed': failure is None and code == 0 and proposal == expected,
                  'argv': command}
        results.append(result)
        print(json.dumps(result), flush=True)
    report = {'status': 'candidate-experiment-only', 'model_sha256': model_sha256, 'candidate': args.candidate,
              'runtime_sha256': args.runtime_sha256, 'system_context': args.system_context, 'scoped_schema': args.scoped_schema, 'results': results,
              'passed': sum(case['passed'] for case in results), 'total': len(results),
              'limitations': ['Synthetic context, no production broker/state changes, no quality qualification.',
                              'RLIMIT_AS and pipe supervision are not a network/filesystem sandbox.',
                              'No generation-memory handoff, GUI, package, device or platform qualification.']}
    (args.output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    return 0 if report['passed'] == len(results) else 1


if __name__ == '__main__':
    raise SystemExit(main())
