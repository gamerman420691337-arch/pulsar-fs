#!/usr/bin/env python3
"""Run the authored browser timing exchange against an actual private Engine.

No rendered browser, real media, physical Handy, or qualification is exercised.
The caller supplies and pins the final application, worker and client-test images.
"""
import argparse
from contextlib import closing
import hashlib
import json
import math
import os
from pathlib import Path
import re
import shutil
import signal
import socket
import sqlite3
import subprocess
import sys
import time
import uuid


ROOT = Path('/home/brianklam/Projects/Funscripts/pulsar-roadmap-m1')
RUNTIME_RECEIPT = Path('/mnt/4tb/artifacts/Funscripts/keep/pulsar-roadmap-m1/roadmap-2026-10-22/browser-player-control/borrowed-runtime.json')
OWNED_ROOT = Path('/mnt/4tb/tmp/pulsar-browser-timing/runs')
TEST = 'browser_live::browser_timing_native_tests::native_browser_timing_nonce_phase_continuation_and_retirement'
MEDIA = Path('/mnt/4tb/artifacts/Funscripts/keep/pulsar-roadmap-m1/roadmap-2026-10-13/browser-rendered-qa/authored-gray.webm')
MEDIA_SHA = '6643ea76ed68f8c0074bf1b45906990b5bd03ca0ae6839aae56901498f915acc'
LOG_LIMIT = 2 * 1024 * 1024
TMPFS_LIMIT = 1024 * 1024 * 1024
RUNTIME_FILES = ('runtimes/libonnxruntime.so.1', 'runtimes/ffmpeg', 'runtimes/ffprobe')


BROWSER_CAPABILITY_FIELDS = frozenset(('page_capability', 'player_capability'))
BROWSER_CAPABILITY_REDACTION = '[browser capability redacted]'
MEDIA_BEARER_PATTERN = re.compile(r'((?:\\)?/media(?:\\)?/)[0-9a-f]{64}(?![0-9a-f])', re.IGNORECASE)
# Quoted JSON keys, including JSON escaped inside a Rust assertion string. A
# serde_json::Value Debug representation may wrap the string in String(...).
BROWSER_CAPABILITY_PATTERN = re.compile(
    r'((?:\\*")(?:page_capability|player_capability)(?:\\*")'
    r'\s*:\s*(?:String\()?\s*(?:\\*"))([^"\\\r\n]{1,4096})((?:\\*"))'
)
BROWSER_DEBUG_CAPABILITY_PATTERN = re.compile(
    r'(\b(?:page_capability|player_capability)\s*:\s*")([^"\\\r\n]{1,4096})(")'
)


def redact_browser_text(text):
    text = MEDIA_BEARER_PATTERN.sub(lambda match: match[1] + '[media bearer redacted]', text)
    for pattern in (BROWSER_CAPABILITY_PATTERN, BROWSER_DEBUG_CAPABILITY_PATTERN):
        text = pattern.sub(lambda match: match[1] + BROWSER_CAPABILITY_REDACTION + match[3], text)
    return text


def check(condition, message):
    if not condition:
        raise RuntimeError(message)


def socket_path_preflight(state):
    # These bwrap supervisors target Linux AF_UNIX, whose sun_path has 108
    # bytes including the terminating NUL. Check encoded bytes, not characters.
    lengths = {name: len(os.fsencode(state / name)) + 1 for name in ('engine.sock', 'bulk.sock')}
    check(all(count <= 108 for count in lengths.values()),
          'owned Engine socket path exceeds Linux sun_path capacity before launch')
    return dict(platform='linux', sun_path_bytes=108, required_bytes_including_nul=lengths)


def digest(path):
    with path.open('rb') as source:
        return hashlib.file_digest(source, 'sha256').hexdigest()


def regular(path):
    check(path.is_file() and not path.is_symlink(), 'expected regular file: ' + str(path))
    return path


def arguments():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    for name in ('app', 'worker', 'client-test'):
        parser.add_argument('--' + name, type=Path, required=True)
        parser.add_argument('--' + name + '-sha256', required=True)
    parser.add_argument('--runtime-receipt', type=Path, default=RUNTIME_RECEIPT)
    parser.add_argument('--fixture-module', type=Path, default=ROOT / 'scripts/benchmark-onnx-fixture.mjs')
    parser.add_argument('--media', type=Path, default=MEDIA)
    parser.add_argument('--inside', action='store_true', help=argparse.SUPPRESS)
    parser.add_argument('--scratch', type=Path, help=argparse.SUPPRESS)
    return parser.parse_args()


def subjects(args):
    result = {}
    for name in ('app', 'worker', 'client_test'):
        path = getattr(args, name).absolute()
        regular(path)
        expected = getattr(args, name + '_sha256')
        check(re.fullmatch('[0-9a-f]{64}', expected) is not None, 'invalid image digest')
        check(digest(path) == expected, name + ' image differs from selected subject')
        setattr(args, name, path)
        result[name] = dict(path=str(path), sha256=expected, bytes=path.stat().st_size)
    receipt_path = regular(args.runtime_receipt.absolute())
    check(receipt_path.stat().st_size <= 64 * 1024, 'runtime receipt exceeds limit')
    receipt = json.loads(receipt_path.read_text())
    runtime = Path(receipt['root']).absolute()
    check(runtime.is_dir() and not runtime.is_symlink(), 'invalid borrowed runtime root')
    manifest_path = regular(runtime / 'runtime-candidate.json')
    check(manifest_path.stat().st_size <= 256 * 1024, 'runtime manifest exceeds limit')
    check(digest(manifest_path) == receipt['manifest_sha256'], 'borrowed runtime manifest changed')
    manifest = json.loads(manifest_path.read_text())
    entries = {entry['path']: entry for entry in manifest['files']}
    check(len(entries) == len(manifest['files']), 'duplicate runtime manifest paths')
    selected = {}
    for rel in RUNTIME_FILES:
        path = regular(runtime / rel)
        saved, declared = receipt['files'][rel], entries[rel]
        actual = dict(sha256=digest(path), bytes=path.stat().st_size)
        check(actual == saved, 'borrowed runtime payload changed: ' + rel)
        check(actual['sha256'] == declared['sha256'] and actual['bytes'] == declared['byte_len'],
              'selected runtime differs from manifest: ' + rel)
        selected[rel] = actual
    result['runtime'] = dict(root=str(runtime), manifest_sha256=receipt['manifest_sha256'],
                             receipt_path=str(receipt_path), receipt_sha256=digest(receipt_path),
                             files=selected, qualified=False)
    args.fixture_module = regular(args.fixture_module.absolute())
    check(args.fixture_module.stat().st_size <= 64 * 1024, 'authored fixture source exceeds limit')
    result['fixture_module'] = dict(path=str(args.fixture_module), sha256=digest(args.fixture_module),
                                    bytes=args.fixture_module.stat().st_size)
    args.media = regular(args.media.absolute())
    check(args.media.stat().st_size == 55544 and digest(args.media) == MEDIA_SHA, 'authored media fixture differs')
    result['media'] = dict(path=str(args.media), sha256=MEDIA_SHA, bytes=55544, authored=True, decoded=False, qualified=False)
    return result


class Campaign:
    def __init__(self, args, pinned):
        self.args, self.pinned = args, pinned
        self.out, self.root = args.output, args.scratch
        self.state, self.temp = self.root / 'state', self.root / 'tmp'
        self.temp.mkdir(mode=0o700)
        self.engine = None
        self.processes, self.cases, self.capabilities = [], {}, set()
        self.logs = []

    def redacted(self, text):
        if self.state.is_dir():
            for path in self.state.glob('*.token'):
                if path.is_file() and not path.is_symlink() and path.stat().st_size <= 4096:
                    value = path.read_text().strip()
                    if value:
                        self.capabilities.add(value)
            path = self.state / 'first-party.session.json'
            if path.is_file() and not path.is_symlink() and path.stat().st_size <= 4096:
                value = json.loads(path.read_text()).get('auth_token')
                if isinstance(value, str) and value:
                    self.capabilities.add(value)
        for value in self.capabilities:
            text = text.replace(value, '[private sandbox capability redacted]')
        return redact_browser_text(text)

    def redacted_value(self, value):
        if isinstance(value, str):
            return self.redacted(value)
        if isinstance(value, dict):
            return {key: BROWSER_CAPABILITY_REDACTION
                    if key in BROWSER_CAPABILITY_FIELDS and isinstance(item, str)
                    else self.redacted_value(item) for key, item in value.items()}
        if isinstance(value, (list, tuple)):
            return [self.redacted_value(item) for item in value]
        return value

    def save(self, name, value):
        (self.out / name).write_text(json.dumps(self.redacted_value(value), indent=2) + '\n')

    def environment(self):
        runtime = Path(self.pinned['runtime']['root']) / 'runtimes'
        return dict(PATH='/usr/bin:/bin', PULSAR_STATE_DIR=str(self.state),
                    PULSAR_WORKER_EXECUTABLE=str(self.args.worker),
                    PULSAR_ORT_DYLIB=str(runtime / 'libonnxruntime.so.1'),
                    PULSAR_FFMPEG=str(runtime / 'ffmpeg'),
                    PULSAR_FFPROBE=str(runtime / 'ffprobe'), TMPDIR=str(self.temp),
                    RUST_BACKTRACE='0')

    def output_bounds(self):
        for path in self.logs:
            check(path.stat().st_size <= LOG_LIMIT, 'owned process exceeded output limit: ' + path.name)

    def retire(self, process, label, timed_out=False, force=False):
        check(process.poll() is not None, 'owned process remains active: ' + label)
        self.processes.append(dict(pid=process.pid, label=label, exit=process.returncode,
                                   joined=True, timed_out=timed_out, forced_sigkill=force))

    def wait(self, process, label, seconds):
        deadline = time.monotonic() + seconds
        problem = None
        try:
            while process.poll() is None:
                self.output_bounds()
                check(time.monotonic() < deadline, 'owned process timed out: ' + label)
                time.sleep(0.025)
            self.output_bounds()
        except Exception as error:
            problem = error
        forced = False
        if process.poll() is None:
            process.send_signal(signal.SIGTERM)
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                forced = True
                process.kill()
                process.wait(timeout=5)
        process.wait(timeout=1)
        self.retire(process, label, problem is not None, forced)
        if problem is not None:
            raise problem
        return process.returncode

    def run(self, argv, label, env=None, timeout=30):
        out, err = self.root / (label + '.out'), self.root / (label + '.err')
        with out.open('x') as stdout, err.open('x') as stderr:
            process = subprocess.Popen([str(x) for x in argv], env=env or self.environment(),
                                       stdin=subprocess.DEVNULL, stdout=stdout, stderr=stderr)
        self.logs.extend([out, err])
        try:
            code = self.wait(process, label, timeout)
        finally:
            value = dict(argv=[str(x) for x in argv], exit=process.poll(),
                         stdout=out.read_bytes()[:LOG_LIMIT].decode(errors='replace'),
                         stderr=err.read_bytes()[:LOG_LIMIT].decode(errors='replace'))
            self.cases[label] = value
        check(code == 0, label + ' failed: ' + value['stderr'][:1000])
        return value['stdout']

    def response(self, argv, label, kind):
        value = json.loads(self.run([self.args.app, *argv], label))
        check(value['kind'] == kind, 'unexpected CLI response type: ' + label)
        return value['value']

    def start_engine(self):
        self.save('socket-path-preflight.json', socket_path_preflight(self.state))
        path = self.root / 'engine.log'
        with path.open('x') as log:
            process = subprocess.Popen([str(self.args.app), 'engine'], env=self.environment(),
                                       stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)
        self.logs.append(path)
        self.engine = process, path
        deadline = time.monotonic() + 25
        while time.monotonic() < deadline:
            check(process.poll() is None, 'Engine exited before readiness')
            self.output_bounds()
            ready = True
            for name in ('engine.sock', 'bulk.sock'):
                with socket.socket(socket.AF_UNIX) as client:
                    client.settimeout(0.2)
                    try:
                        client.connect(str(self.state / name))
                    except OSError:
                        ready = False
                        break
            if ready:
                return
            time.sleep(0.025)
        raise RuntimeError('both Engine sockets were not ready before deadline')

    def stop_engine(self, cleanup=False):
        if self.engine is None:
            return
        process, path = self.engine
        if process.poll() is None:
            process.send_signal(signal.SIGTERM)
        try:
            code = self.wait(process, 'engine', 15)
        finally:
            (self.out / 'engine.log').write_text(self.redacted(path.read_bytes()[:LOG_LIMIT].decode(errors='replace')))
            self.engine = None
        if not cleanup:
            check(code == 0, 'Engine did not shut down successfully')
            check(not any(os.path.lexists(self.state / name) for name in ('engine.sock', 'bulk.sock')),
                  'owned Engine sockets remain after shutdown')

    def empty_custody(self):
        deadline = time.monotonic() + 15
        while True:
            path = self.state / 'worker-fence.json'
            check(path.is_file() and path.stat().st_size <= LOG_LIMIT, 'worker fence missing or too large')
            custody = json.loads(path.read_text())
            empty = (all(not custody[name] for name in ('groups', 'retired', 'staging'))
                     and not custody['creation']['records'])
            if empty:
                self.save('custody.json', custody)
                return
            check(time.monotonic() < deadline, 'owned worker custody did not drain')
            time.sleep(0.05)

    def counts(self, report):
        with closing(sqlite3.connect('file:' + str(self.state / 'projects.sqlite3') + '?mode=ro',
                                     uri=True, timeout=2)) as db:
            result = {name: db.execute('SELECT count(*) FROM ' + name).fetchone()[0]
                      for name in ('projects', 'jobs', 'attempts', 'candidates', 'exports', 'sources')}
            rows = db.execute('SELECT CASE WHEN length(CAST(response AS BLOB))<=1048576 THEN response ELSE NULL END FROM requests LIMIT 10001')
            captures, preparations, timing_bindings = set(), set(), set()
            total_bytes = 0
            for index, (raw,) in enumerate(rows):
                check(index < 10000 and raw is not None, 'native request-history inspection exceeds bounds')
                total_bytes += len(raw.encode())
                check(total_bytes <= 8 * 1024 * 1024, 'native request-history aggregate exceeds bounds')
                value = json.loads(raw)
                if value.get('kind') == 'live_capture':
                    captures.add(value['value']['identity']['epoch'])
                elif value.get('kind') == 'browser_media_preparation':
                    preparations.add(value['value']['request_id'])
                elif value.get('kind') == 'browser_timing':
                    timing_bindings.add(value['value']['binding']['binding_id'])
            chain = report['capture_chain']
            check(isinstance(chain, list) and 2 <= len(chain) <= 4 and len(set(chain)) == len(chain)
                  and captures == set(chain) and report['intentional_start_capture_count'] == 1,
                  'capture history differs from the observed prepared continuation')
            check(len(preparations) == 2, 'unexpected media preparation identities')
            check(len(timing_bindings) == 3 and timing_bindings == set(report['timing_binding_ids']),
                  'timing open history differs from exact original player scopes')
            revision = db.execute('SELECT revision FROM projects').fetchone()[0]
            check(revision == 1, 'unexpected project mutation after placement')
        check(result == dict(projects=1, jobs=0, attempts=0, candidates=0, exports=0, sources=1),
              'unexpected ordinary project work: ' + str(result))
        path = self.state / 'device-effects/journal.sqlite3'
        with closing(sqlite3.connect('file:' + str(path) + '?mode=ro', uri=True, timeout=2)) as db:
            effects = {name: db.execute('SELECT count(*) FROM ' + name).fetchone()[0]
                       for name in ('effects', 'setup_effects', 'clock_sync_effects', 'play_operations')}
        check(all(count == 0 for count in effects.values()), 'unexpected device effects')
        self.save('counts.json', dict(project=result, device=effects, project_revision=revision, capture_epochs=len(captures), preparation_requests=len(preparations), timing_bindings=len(timing_bindings)))
        return result, effects

    def campaign(self):
        check([name for _, name in socket.if_nameindex()] == ['lo'], 'network namespace is not isolated')
        model = self.root / 'authored.onnx'
        code = ("import {pathToFileURL} from 'node:url';import fs from 'node:fs';"
                "const {authoredOnnxFixture}=await import(pathToFileURL(process.argv[1]));"
                "fs.writeFileSync(process.argv[2],authoredOnnxFixture({temporal:true}));")
        self.run(['node', '--input-type=module', '-e', code, self.args.fixture_module, model],
                 'make-authored-model', timeout=10)
        check(model.stat().st_size <= 64 * 1024, 'authored graph exceeded limit')
        shutil.copy2(model, self.out / 'authored.onnx')
        shutil.copy2(self.args.fixture_module, self.out / 'authored-fixture-source.mjs')
        shutil.copy2(self.args.media, self.out / 'authored-gray.webm')
        shutil.copy2(Path(__file__), self.out / 'runner.py')
        self.start_engine()
        project = self.response(['project', 'create', 'SYNTHETIC browser timing native fixture'],
                                'project-create', 'project')['project_id']
        profile = json.loads(self.run([self.args.app, 'model', 'profile', model,
                                       '--width', '64', '--height', '64', '--class-count', '1',
                                       '--temporal-script-position'], 'model-profile'))
        check(profile['model']['sha256'] == digest(model), 'profile selected another graph')
        check(profile['runtime']['sha256']
              == self.pinned['runtime']['files']['runtimes/libonnxruntime.so.1']['sha256'],
              'profile selected another runtime')
        options = dict(policy=dict(maximum_analysis_age_ns=1_000_000_000, added_buffer_ns=0),
                       source_anchor=dict(numerator=0, denominator=1), project_anchor=0,
                       presentation_anchor=0, lease_ms=10_000,
                       inference=dict(model_path=str(model), profile=profile))
        optionfile = self.root / 'options.json'
        optionfile.write_text(json.dumps(options))
        self.save('options.json', options)
        env = dict(self.environment(), PULSAR_TEST_BROWSER_ENDPOINT=str(self.state / 'engine.sock'),
                   PULSAR_TEST_BROWSER_BOOTSTRAP=str(self.state / 'pairing.token'),
                   PULSAR_TEST_BROWSER_OPTIONS=str(optionfile), PULSAR_TEST_BROWSER_PROJECT=project,
                   PULSAR_TEST_BROWSER_MEDIA=str(self.args.media),
                   PULSAR_TEST_BROWSER_RESULTS=str(self.root / 'controller-results.json'))
        try:
            self.run([self.args.client_test, TEST, '--exact', '--ignored', '--nocapture'],
                     'native-browser-timing', env=env, timeout=300)
        finally:
            progress_path = self.root / 'timing-progress.json'
            if progress_path.is_file() and not progress_path.is_symlink():
                try:
                    check(progress_path.stat().st_size <= 256 * 1024, 'timing progress exceeds bound')
                    self.save('timing-progress.json', json.loads(progress_path.read_text()))
                except (OSError, ValueError, RuntimeError) as error:
                    self.save('timing-progress-error.json', dict(error=str(error)))
        report_path = regular(self.root / 'controller-results.json')
        check(report_path.stat().st_size <= 256 * 1024, 'controller result exceeds limit')
        report = json.loads(report_path.read_text())
        self.save('controller-results.json', report)
        for field in ('passed', 'actual_engine', 'authored_callback_facts',
                      'without_handy_configuration', 'nonce_fresh_frame_exchange',
                      'whole_native_bracket_preserved', 'exact_declared_phase',
                      'timing_without_capture', 'continuation_keeps_timing',
                      'pause_preserves_media_retires_timing', 'stale_replacement_scope_refused',
                      'source_release_retires_pending_timing', 'server_joined'):
            check(report.get(field) is True, 'native browser timing evidence absent: ' + field)
        for field in ('qualified', 'device_output', 'rendered_browser', 'capture_active',
                      'callback_display_calibrated', 'source_clock_qualified'):
            check(report.get(field) is False, 'unexpected timing qualification claim: ' + field)
        check(report.get('media_sha256') == MEDIA_SHA, 'native timing uses another imported source')
        check(7_000_000 <= report.get('stream_wall_us', 0) < 11_000_000
              and report.get('capture_lease_ms') == 10000 and report.get('observations', 0) >= 10,
              'timing did not span the prepared capture continuation')
        check(len(report.get('timing_binding_ids', [])) == 3
              and len(set(report['timing_binding_ids'])) == 3,
              'timing player scopes were not distinct')
        observed, retired = report.get('observed_native', []), report.get('retired_native', [])
        check(len(observed) == 4 and all(row.get('current') is True and row.get('qualified') is False for row in observed),
              'fresh native observation records are incomplete')
        check(len(retired) == 3 and all(row.get('current') is False and row.get('qualified') is False for row in retired),
              'original native timing retirements are incomplete')
        check(observed[0]['binding'] == observed[1]['binding']
              and observed[1]['observation']['sequence'] > observed[0]['observation']['sequence'],
              'capture continuation reset native timing identity or sequence')
        check(report.get('independent_heartbeats', 0) > 0
              and 0 <= report.get('maximum_heartbeat_us', 1000000) < 1000000,
              'independent control evidence is incomplete')
        value, expected = report.get('diagnostic_position'), 0.25 + 0.5 * 64.0 / 255.0
        check(isinstance(value, (int, float)) and math.isfinite(value)
              and abs(value - expected) < 0.0001, 'authored scalar oracle differs')
        self.empty_custody()
        self.counts(report)
        self.stop_engine()
        check(subjects(self.args) == self.pinned, 'native inputs changed during the campaign')
        self.save('summary.json', dict(passed=True, qualified=False,
            authored_diagnostic_graph=True, useful_model_quality=False,
            expected_scalar=expected, observed_scalar=value, model_sha256=digest(model),
            actual_engine=True, actual_native_timing_exchange=True,
            source_sha256=MEDIA_SHA, observations=report['observations'],
            capture_chain=report['capture_chain'], timing_binding_ids=report['timing_binding_ids'],
            stream_wall_us=report['stream_wall_us'], rendered_browser=False,
            source_clock_qualified=False, callback_display_calibrated=False,
            real_device_contacted=False, physical_acceptance_created=False, test=TEST,
            limitations=['Native HTTP supplies authored callback facts; no rendered browser is observed.',
                         'The native nonce bracket is retained without claiming display calibration.',
                         'Declared browser/source mapping does not qualify source correspondence.',
                         'No Handy configuration or device effect is authorized.',
                         'Authored inference establishes plumbing, not learned motion accuracy.']))



def inside(args):
    check(os.environ.get('PULSAR_BROWSER_TIMING_NATIVE_ISOLATED') == '1', 'use the outer supervisor')
    args.output = args.output.absolute()
    check(args.scratch is not None and args.scratch.is_dir(), 'private scratch is absent')
    pinned = json.loads((args.output / 'subjects.json').read_text())
    check(subjects(args) == pinned, 'inputs changed before isolated execution')
    campaign = Campaign(args, pinned)
    passed = False
    try:
        campaign.campaign()
        passed = True
    except Exception as error:
        campaign.save('failure.json', dict(passed=False, error=str(error)))
    finally:
        try:
            campaign.stop_engine(cleanup=True)
        except Exception as error:
            passed = False
            campaign.save('cleanup-failure.json', dict(error=str(error)))
        custody = campaign.state / 'worker-fence.json'
        if custody.is_file() and not custody.is_symlink() and custody.stat().st_size <= LOG_LIMIT:
            try:
                campaign.save('cleanup-custody.json', json.loads(custody.read_text()))
            except (OSError, ValueError) as error:
                passed = False
                campaign.save('cleanup-custody-error.json', dict(error=str(error)))
        campaign.save('cases.json', campaign.cases)
        campaign.save('processes.json', campaign.processes)
        if any(not row['joined'] or row['timed_out'] or row['forced_sigkill'] for row in campaign.processes):
            passed = False
    return 0 if passed else 1


def cleanup_preflight(scratch):
    root = str(scratch)
    references, denied, vanished, inspected = [], 0, 0, 0
    def owns(text):
        return root in text
    for proc in Path('/proc').iterdir():
        if not proc.name.isdigit():
            continue
        for name in ('cwd', 'root'):
            try:
                target = os.readlink(proc / name)
                inspected += 1
                if owns(target): references.append(dict(pid=int(proc.name), kind=name))
            except PermissionError: denied += 1
            except (FileNotFoundError, ProcessLookupError): vanished += 1
            except OSError: vanished += 1
        for name in ('fd', 'map_files'):
            try:
                for entry in (proc / name).iterdir():
                    try:
                        target = os.readlink(entry)
                        inspected += 1
                        if owns(target): references.append(dict(pid=int(proc.name), kind=name))
                    except PermissionError: denied += 1
                    except OSError: vanished += 1
            except PermissionError: denied += 1
            except OSError: vanished += 1
        for name in ('maps', 'mountinfo'):
            try:
                with (proc / name).open() as stream:
                    matched = any(owns(line) for line in stream)
                inspected += 1
                if matched: references.append(dict(pid=int(proc.name), kind=name))
            except PermissionError: denied += 1
            except OSError: vanished += 1
    try:
        if owns(Path('/proc/net/unix').read_text()): references.append(dict(kind='unix-socket'))
        inspected += 1
    except PermissionError: denied += 1
    except OSError: vanished += 1
    return dict(owned_root=root, references=references, inspected=inspected,
                denied=denied, disappeared_or_unavailable=vanished,
                scope='Accessible process and Unix-socket references only; denied entries are not proven empty')


def main():
    args = arguments()
    os.umask(0o077)
    if args.inside:
        return inside(args)
    pinned = subjects(args)
    args.output = args.output.absolute()
    check(not os.path.lexists(args.output), 'output must be a new directory')
    check(args.output.parent.is_dir(), 'output parent is absent')
    check(OWNED_ROOT.is_dir() and not OWNED_ROOT.is_symlink(), 'owned scratch parent is absent')
    scratch = OWNED_ROOT / ('q-' + uuid.uuid4().hex)
    socket_path_preflight(scratch / 'ram' / 'state')
    scratch.mkdir(mode=0o700)
    (scratch / 'ram').mkdir(mode=0o700)
    args.output.mkdir(mode=0o700)
    (args.output / 'subjects.json').write_text(json.dumps(pinned, indent=2) + '\n')
    command = ['bwrap', '--unshare-net', '--unshare-pid', '--die-with-parent',
               '--ro-bind', '/', '/', '--dev-bind', '/dev', '/dev', '--proc', '/proc',
               '--bind', '/sys/fs/cgroup', '/sys/fs/cgroup',
               '--bind', str(scratch), str(scratch), '--bind', str(args.output), str(args.output),
               '--size', str(TMPFS_LIMIT), '--tmpfs', str(scratch / 'ram'),
               '--chmod', '0700', str(scratch / 'ram'), '/usr/bin/python3',
               str(Path(__file__).resolve()), *sys.argv[1:], '--inside', '--scratch', str(scratch / 'ram')]
    (args.output / 'invocation.json').write_text(json.dumps(dict(command=command,
        runner_sha256=digest(Path(__file__)), supervisor_timeout_seconds=480,
        private_tmpfs_limit_bytes=TMPFS_LIMIT), indent=2) + '\n')
    process, forced, code, problem = None, False, 125, None
    supervisor_log = scratch / 'supervisor.raw.log'
    try:
        with supervisor_log.open('x') as log:
            process = subprocess.Popen(command, env=dict(PATH='/usr/bin:/bin',
                PYTHONDONTWRITEBYTECODE='1', PULSAR_BROWSER_TIMING_NATIVE_ISOLATED='1'),
                stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)
            deadline = time.monotonic() + 480
            while process.poll() is None:
                if time.monotonic() >= deadline:
                    problem = 'supervisor timeout'
                    break
                if supervisor_log.stat().st_size > LOG_LIMIT:
                    problem = 'supervisor output limit'
                    break
                time.sleep(0.05)
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    forced = True
                    process.kill()
                    process.wait(timeout=5)
                code = 124 if problem == 'supervisor timeout' else 125
            else:
                code = process.wait(timeout=1)
    finally:
        joined = process is None or process.poll() is not None
        check(joined, 'supervisor remains active; scratch preserved')
        check(not scratch.is_symlink(), 'scratch was replaced by a symlink')
        if supervisor_log.is_file() and not supervisor_log.is_symlink():
            with supervisor_log.open('rb') as log:
                text = log.read(LOG_LIMIT).decode(errors='replace')
            (args.output / 'supervisor.log').write_text(redact_browser_text(text))
        cleanup = cleanup_preflight(scratch)
        (args.output / 'scratch-cleanup-preflight.json').write_text(json.dumps(cleanup, indent=2) + '\n')
        check(not cleanup['references'], 'owned scratch still has live references; preserved')
        shutil.rmtree(scratch)
        (args.output / 'exit.json').write_text(json.dumps(dict(exit=code,
            passed=code == 0 and not forced, supervisor_joined=joined,
            forced_sigkill=forced, supervisor_problem=problem,
            scratch_removed=str(scratch), qualified=False), indent=2) + '\n')
    return code


if __name__ == '__main__':
    raise SystemExit(main())
