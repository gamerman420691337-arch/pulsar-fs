#!/usr/bin/env python3
"""Bounded journal-v10 barrier and clean-state browser-start refusal campaign.

No accepted fixture rows, human acceptance, vendor endpoint, device keys or
physical effects are created. Positive Engine/localhost cases run separately.
"""
import argparse
from contextlib import closing, ExitStack
import hashlib
import json
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

OWNED_ROOT = Path('/mnt/4tb/tmp')
LIMIT = 1024 * 1024
RAM_LIMIT = 256 * 1024 * 1024
DB_FILES = ('projects.sqlite3', 'projects.sqlite3-wal',
            'device-effects/journal.sqlite3', 'device-effects/journal.sqlite3-wal')
ROLES = ('environment_configuration', 'clock_and_display', 'media_progression', 'source_correspondence')
INTERRUPTED = False


def check(condition, message):
    if not condition:
        raise RuntimeError(message)


def interrupted(_signal, _frame):
    global INTERRUPTED
    INTERRUPTED = True


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def compact(value):
    return json.dumps(value, separators=(',', ':'), ensure_ascii=False)


def regular(path):
    check(path.is_file() and not path.is_symlink(), 'expected a regular file: ' + str(path))
    return path


def scrub_browser(text):
    text = re.sub(r'(/media/)[0-9a-f]{64}(?![0-9a-f])', r'\1[media bearer redacted]', text)
    pattern = r'((?:\\*")(?:page_capability|player_capability)(?:\\*")\s*:\s*(?:String\()?\s*(?:\\*"))([^"\\\r\n]{1,4096})((?:\\*"))'
    return re.sub(pattern, lambda match: match[1] + '[browser capability redacted]' + match[3], text)


def arguments():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    for name in ('app', 'worker', 'old-app'):
        parser.add_argument('--' + name, type=Path, required=True)
        parser.add_argument('--' + name + '-sha256', required=True)
    parser.add_argument('--inside', action='store_true', help=argparse.SUPPRESS)
    parser.add_argument('--scratch', type=Path, help=argparse.SUPPRESS)
    return parser.parse_args()


def subjects(args):
    result = {}
    for name in ('app', 'worker', 'old_app'):
        path = regular(getattr(args, name).absolute())
        expected = getattr(args, name + '_sha256')
        check(re.fullmatch('[0-9a-f]{64}', expected) is not None, 'invalid image hash')
        check(digest(path) == expected, 'selected executable bytes changed: ' + name)
        setattr(args, name, path)
        result[name] = dict(path=str(path), sha256=expected, bytes=path.stat().st_size)
    check(result['app']['sha256'] != result['old_app']['sha256'], 'old and current readers must differ')
    return result


def socket_paths(state):
    sizes = {name: len(os.fsencode(state / name)) + 1 for name in ('engine.sock', 'bulk.sock')}
    check(all(size <= 108 for size in sizes.values()), 'owned Linux AF_UNIX path exceeds sun_path')
    return sizes


class Campaign:
    def __init__(self, args):
        self.args, self.out, self.root = args, args.output, args.scratch
        self.state, self.temp = self.root / 's', self.root / 't'
        self.temp.mkdir(mode=0o700)
        self.engine = None
        self.processes, self.cases, self.logs, self.capabilities = [], {}, [], set()

    def redact(self, value):
        if self.state.is_dir():
            for path in self.state.glob('*.token'):
                if path.is_file() and not path.is_symlink() and path.stat().st_size <= 4096:
                    token = path.read_text().strip()
                    if token:
                        self.capabilities.add(token)
            credentials = self.state / 'first-party.session.json'
            if credentials.is_file() and not credentials.is_symlink() and credentials.stat().st_size <= 65536:
                token = json.loads(credentials.read_text()).get('auth_token')
                if isinstance(token, str) and token:
                    self.capabilities.add(token)
        text = value
        for token in self.capabilities:
            text = text.replace(token, '[private Engine capability redacted]')
        return scrub_browser(text)

    def save(self, name, value):
        (self.out / name).write_text(self.redact(json.dumps(value, indent=2)) + '\n')

    def environment(self):
        # Both readers use the pinned worker path, but this campaign launches no
        # worker. The old-reader case must refuse at the journal format barrier.
        return dict(PATH='/usr/bin:/bin', PULSAR_STATE_DIR=str(self.state),
                    PULSAR_WORKER_EXECUTABLE=str(self.args.worker), TMPDIR=str(self.temp),
                    RUST_BACKTRACE='0')

    def output_bounds(self):
        for path in self.logs:
            check(path.stat().st_size <= LIMIT, 'owned child exceeded output bound: ' + path.name)

    def wait(self, process, label, seconds, cleanup=False):
        deadline = time.monotonic() + seconds
        problem, forced = None, False
        while process.poll() is None:
            if not cleanup and INTERRUPTED:
                problem = 'campaign interrupted'
                break
            if time.monotonic() >= deadline:
                problem = 'owned child timed out'
                break
            try:
                self.output_bounds()
            except Exception as error:
                problem = str(error)
                break
            time.sleep(0.025)
        if process.poll() is None:
            process.send_signal(signal.SIGTERM)
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                forced = True
                process.kill()
                process.wait(timeout=5)
        process.wait(timeout=1)
        self.processes.append(dict(pid=process.pid, label=label, exit=process.returncode,
                                   joined=True, problem=problem, forced_sigkill=forced))
        check(problem is None and not forced, problem or 'forced child termination')
        self.output_bounds()
        return process.returncode

    def run(self, argv, label, app=None, timeout=25):
        app = app or self.args.app
        out, err = self.root / (label + '.out'), self.root / (label + '.err')
        with out.open('x') as stdout, err.open('x') as stderr:
            process = subprocess.Popen([str(app), *map(str, argv)], env=self.environment(),
                stdin=subprocess.DEVNULL, stdout=stdout, stderr=stderr)
        self.logs.extend((out, err))
        try:
            code = self.wait(process, label, timeout)
        finally:
            result = dict(arguments=list(map(str, argv)), binary=str(app), exit=process.poll(),
                          stdout=out.read_bytes()[:LIMIT].decode(errors='replace'),
                          stderr=err.read_bytes()[:LIMIT].decode(errors='replace'))
            self.cases[label] = result
        return result

    def response(self, argv, label, kind, app=None):
        result = self.run(argv, label, app)
        check(result['exit'] == 0, label + ' failed: ' + result['stderr'][:1000])
        body = json.loads(result['stdout'])
        check(body.get('kind') == kind, 'unexpected response kind: ' + label)
        return body['value']

    def refusal(self, argv, label, contains=None, app=None):
        result = self.run(argv, label, app)
        check(result['exit'] != 0, 'unexpected native command success: ' + label)
        if contains is not None:
            check(contains in result['stderr'], 'native refusal differs: ' + label)
        return result

    def start(self, app, label):
        check(self.engine is None, 'another owned Engine is active')
        self.save(label + '-socket-paths.json', socket_paths(self.state))
        path = self.root / (label + '.log')
        with path.open('x') as log:
            process = subprocess.Popen([str(app), 'engine'], env=self.environment(),
                stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)
        self.engine = process, label, path
        self.logs.append(path)
        deadline = time.monotonic() + 25
        while time.monotonic() < deadline:
            check(not INTERRUPTED, 'campaign interrupted during Engine startup')
            check(process.poll() is None, 'Engine exited before readiness: ' + label)
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
        raise RuntimeError('Engine readiness timed out: ' + label)

    def stop(self, cleanup=False):
        if self.engine is None:
            return
        process, label, path = self.engine
        if process.poll() is None:
            process.send_signal(signal.SIGTERM)
        try:
            code = self.wait(process, label, 15, cleanup=True)
        finally:
            (self.out / (label + '.log')).write_text(self.redact(path.read_bytes()[:LIMIT].decode(errors='replace')))
            self.engine = None
        if not cleanup:
            check(code == 0, 'owned Engine shutdown failed')
            check(not any(os.path.lexists(self.state / n) for n in ('engine.sock', 'bulk.sock')),
                  'owned Engine sockets remain after joined shutdown')

    def db(self, name='projects.sqlite3'):
        return sqlite3.connect(self.state / name, timeout=2)

    def hashes(self):
        return {name: digest(self.state / name) if (self.state / name).is_file() else None for name in DB_FILES}

    def inert(self):
        result = {}
        with closing(self.db('device-effects/journal.sqlite3')) as db:
            check(db.execute('PRAGMA user_version').fetchone()[0] == 10, 'current journal is not version 10')
            for table in ('effects', 'setup_effects', 'clock_sync_effects', 'play_operations', 'play_reference_members'):
                result[table] = db.execute('SELECT count(*) FROM ' + table).fetchone()[0]
                check(result[table] == 0, 'unexpected native effect or browser play record: ' + table)
            census = db.execute('SELECT operation_count FROM play_reference_census').fetchall()
            check(census == [(0,)], 'empty browser play census differs')
        with closing(self.db()) as db:
            for table in ('jobs', 'attempts', 'candidates', 'exports', 'sources'):
                result[table] = db.execute('SELECT count(*) FROM ' + table).fetchone()[0]
                check(result[table] == 0, 'clean-state refusal created work: ' + table)
            for table in ('handy_qualifications', 'handy_browser_start_qualifications'):
                exists = db.execute("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?)", (table,)).fetchone()[0]
                count = db.execute('SELECT count(*) FROM ' + table).fetchone()[0] if exists else 0
                result[table] = count
                check(count == 0, 'campaign created qualification or acceptance rows')
            receipts = db.execute("SELECT count(*) FROM requests WHERE json_extract(response,'$.kind') IN ('handy_browser_start_qualification_saved','handy_browser_start_qualification','handy_playback','handy_browser_playback')").fetchone()[0]
            result['successful_browser_or_play_receipts'] = receipts
            check(receipts == 0, 'clean-state refusals created successful effect/qualification receipts')
        return result

    def credentials(self):
        path = regular(self.state / 'first-party.session.json')
        check(path.stat().st_size <= 65536, 'private credential file exceeds bound')
        value = json.loads(path.read_text())
        self.capabilities.add(value['auth_token'])
        return value

    def owner_proof(self):
        path = regular(self.state / 'handy-owner.token')
        check(path.stat().st_size <= 4096, 'owner capability exceeds bound')
        value = path.read_text().strip()
        self.capabilities.add(value)
        return value

    def ipc(self, project, revision, operation, arguments, request_id, label, error_required=True):
        credentials = self.credentials()
        request = dict(version=2, request_id=request_id, session=credentials['session'],
            auth_token=credentials['auth_token'], project=project, expected_revision=revision,
            command=dict(operation=operation, arguments=arguments))
        payload = compact(request).encode()
        check(len(payload) <= LIMIT, 'native IPC request exceeds bound')
        deadline = time.monotonic() + 5
        with socket.socket(socket.AF_UNIX) as client:
            client.settimeout(5)
            client.connect(str(self.state / 'engine.sock'))
            client.sendall(len(payload).to_bytes(4, 'big') + payload)
            def receive(count):
                result = bytearray()
                while len(result) < count:
                    remaining = deadline - time.monotonic()
                    check(remaining > 0, 'native IPC response timed out')
                    client.settimeout(remaining)
                    chunk = client.recv(count-len(result))
                    check(chunk, 'native IPC response closed early')
                    result.extend(chunk)
                return bytes(result)
            size = int.from_bytes(receive(4), 'big')
            check(0 < size <= LIMIT, 'native IPC response exceeds bound')
            reply = json.loads(receive(size))
        check(reply['version'] == 2 and reply['request_id'] == request_id, 'native IPC correlation differs')
        result = reply['result']
        metadata = dict(transport='actual application Engine IPC', operation=operation,
                        request_id=request_id, project=project, expected_revision=revision, result=result)
        # Deliberately omit request arguments: they contain native owner proof.
        self.cases[label] = metadata
        if error_required:
            check('Err' in result, 'unknown clean-state selection unexpectedly succeeded: ' + label)
        elif 'Ok' in result:
            value = result['Ok']
            check(value.get('kind') == 'handy_browser_playback' and value['value']['current'] is False
                  and value['value'].get('controller') is None and value['value'].get('play_record_id') is None,
                  'unknown browser owner unexpectedly gained authority')
        else:
            check('Err' in result, 'IPC response lacks a result')
        return result

    def manifest(self, project):
        context = self.response(['handy-qualification-context'], 'current-control-context', 'handy_qualification_context')
        evidence = []
        for role in ROLES:
            path = self.root / (role + '.txt')
            path.write_text('SYNTHETIC REFUSAL FIXTURE; no measured evidence or acceptance: ' + role + '\n')
            evidence.append(dict(kind=role, path=str(path), sha256=digest(path), byte_len=path.stat().st_size))
        environment = evidence[0]
        source_bytes = b'SYNTHETIC descriptor only; source not imported'
        script_bytes = b'SYNTHETIC descriptor only; script not prepared'
        source_hash = hashlib.sha256(source_bytes).hexdigest()
        script_hash = hashlib.sha256(script_bytes).hexdigest()
        subject = dict(schema_version=1, origin='synthetic', context=context,
            device_qualification=dict(dossier_id='a'*64, revision=2),
            browser_environment=dict(artifact_id=environment['sha256'], sha256=environment['sha256'], byte_len=environment['byte_len']),
            project_id=project['project_id'], revision=project['revision'],
            selection=dict(source_version=source_hash,
                source=dict(artifact_id=source_hash, sha256=source_hash, byte_len=len(source_bytes)),
                placement=dict(placement_id='absent-source-placement', created_revision=project['revision'],
                    spec=dict(source_version=source_hash, source_start=dict(numerator=0,denominator=1),
                        source_end=dict(numerator=30,denominator=1), source_anchor=dict(numerator=0,denominator=1),
                        project_anchor=0,rate_numerator=1,rate_denominator=1))),
            clock=dict(presentation_anchor=dict(numerator=0,denominator=1),elapsed_anchor=dict(numerator=0,denominator=1)),
            master_sha256=project['motion']['program']['sha256'],
            script_actor_session=self.credentials()['session'], script_request_id='absent-script-request',
            script=dict(artifact_id=script_hash,sha256=script_hash,byte_len=len(script_bytes)),
            timing=dict(clock=dict(timestamp_error_ns=1000,relative_rate_error_ppm=20,
                    maximum_age_ns=1000000000,maximum_interval_ns=100000000),
                reference='callback',display_early_ns=2000000,display_late_ns=3000000,
                source_correspondence_error_ns=1000000,discrete_media_phase_error_ns=1000000,
                media_rate_error_ppm=20,physical_execution_error_ns=1000000,
                maximum_start_age_ns=500000000,maximum_alignment_error_ns=50000000))
        path = self.root / 'synthetic-browser-input.json'
        path.write_text(compact(dict(subject=subject,evidence=evidence)))
        self.save('synthetic-input.json', dict(subject=subject,evidence=evidence,
            note='Descriptor-only clean-state refusal fixture; no accepted prerequisite or real source/script'))
        return path

    def clean_refusals(self, project, manifest, suffix=''):
        project_id, revision = project['project_id'], str(project['revision'])
        dossier = 'f'*64
        imported = ['handy-browser-start-qualification-import',project_id,str(manifest),'--revision',revision,
            '--manifest-sha256',digest(manifest),'--request-id','browser-import-refused']
        self.refusal(imported, 'browser-import-missing-device-v6'+suffix, contains='browser-start qualification is not current')
        self.refusal(imported, 'browser-import-repeated-refusal'+suffix, contains='browser-start qualification is not current')
        wrong = imported.copy()
        wrong[wrong.index('--manifest-sha256')+1] = '0'*64
        wrong[wrong.index('--request-id')+1] = 'browser-import-wrong-hash'+suffix
        self.refusal(wrong, 'browser-import-hash-refusal'+suffix, contains='manifest input changed')
        self.refusal(['handy-browser-start-qualification-status',project_id,dossier], 'browser-status-missing'+suffix)
        accept = ['handy-browser-start-qualification-accept',project_id,dossier,'--revision',revision,
            '--record-revision','1','--operator','synthetic refusal fixture; no human acceptance',
            '--request-id','browser-accept-refused'+suffix]
        self.refusal(accept, 'browser-accept-consent-missing'+suffix, contains='--acknowledge-measured-evidence')
        self.refusal([*accept,'--acknowledge-measured-evidence'], 'browser-accept-dossier-missing'+suffix)
        self.refusal(['handy-browser-start-qualification-withdraw',project_id,dossier,'--record-revision','1',
            '--reason','synthetic missing-dossier refusal','--request-id','browser-withdraw-refused'+suffix],
            'browser-withdraw-dossier-missing'+suffix)
        history = self.response(['handy-play-status',project_id,'--play-request-id','browser-start-refused'],
            'original-play-status-empty'+suffix,'handy_playback')
        check(history['project_id'] == project_id and history['play_request_id'] == 'browser-start-refused'
              and history['history'] is None, 'unknown original play acquired history')
        owner = self.owner_proof()
        selection = dict(acquisition_request_id='absent-acquisition',controller_id='absent-controller',
            expected_generation=0,clock_request_id='absent-clock',browser_timing_binding_id='e'*64,
            qualification=dict(dossier_id=dossier,revision=2),acknowledge_current_environment=True,
            allow_play=True,owner_proof=owner)
        no_consent = dict(selection,allow_play=False)
        self.ipc(project_id,int(revision),'start_handy_playback_from_browser',no_consent,
                 'browser-no-consent'+suffix,'browser-start-consent-refusal'+suffix)
        self.ipc(project_id,int(revision),'start_handy_playback_from_browser',selection,
                 'browser-start-refused','browser-start-missing-native-context'+suffix)
        self.ipc(project_id,int(revision),'start_handy_playback_from_browser',selection,
                 'browser-start-refused','browser-start-repeated-refusal'+suffix)
        owner_selection = dict(browser_timing_binding_id='e'*64,play_request_id='browser-start-refused',
                               play_record_id=None,owner_proof=owner)
        for operation in ('handy_browser_playback_status','renew_handy_browser_playback','release_handy_browser_playback'):
            self.ipc(project_id,None,operation,owner_selection,operation+suffix,operation+suffix,error_required=False)
        self.ipc(project_id,None,'refresh_handy_browser_playback_timing',
                 dict(owner_selection,allow_online_inspection=False),'browser-refresh-no-consent'+suffix,
                 'browser-refresh-consent-refusal'+suffix)
        self.ipc(project_id,None,'refresh_handy_browser_playback_timing',
                 dict(owner_selection,allow_online_inspection=True),'browser-refresh-no-owner'+suffix,
                 'browser-refresh-owner-refusal'+suffix,error_required=False)
        self.save('inert-counts'+suffix+'.json',self.inert())

    def old_refusal(self, label):
        before = self.hashes()
        result = self.refusal(['engine'],label,app=self.args.old_app)
        check('device journal requires an explicit migration' in result['stderr'],
              'old reader did not identify the device-journal format barrier')
        after, created = self.hashes(), []
        for name, expected in before.items():
            if expected is None and name.endswith('-wal') and after[name] == hashlib.sha256(b'').hexdigest():
                created.append(name)
            else:
                check(after[name] == expected, 'old reader changed database/existing WAL: '+name)
        self.save(label+'-preservation.json',dict(before=before,after=after,
            existing_databases_and_wals_unchanged=True,created_empty_wals=created,
            shm_not_claimed_unchanged=True))

    def campaign(self):
        check([name for _,name in socket.if_nameindex()] == ['lo'], 'private network namespace is required')
        self.start(self.args.old_app,'v8-baseline-engine')
        with closing(self.db('device-effects/journal.sqlite3')) as db:
            check(db.execute('PRAGMA user_version').fetchone()[0] == 8, 'retained executable did not create v8 journal')
        project = self.response(['project','create','SYNTHETIC browser-start refusal QA'],
                                'baseline-project-create','project',self.args.old_app)
        self.stop()
        self.start(self.args.app,'current-v10-engine')
        self.save('current-v10-initial-counts.json',self.inert())
        current = self.response(['project','open',project['project_id']],'current-project-open','project')
        check(current == project,'reader migration changed original project')
        manifest = self.manifest(project)
        self.clean_refusals(project,manifest)
        self.stop()
        self.old_refusal('v8-refuses-v10')
        with ExitStack() as stack:
            for name in ('projects.sqlite3','device-effects/journal.sqlite3'):
                db = stack.enter_context(closing(self.db(name)))
                version = db.execute('PRAGMA user_version').fetchone()[0]
                db.execute('PRAGMA wal_autocheckpoint=0')
                db.execute('PRAGMA user_version='+str(version))
                db.commit()
                check((self.state/(name+'-wal')).stat().st_size > 0,'expected held nonempty WAL')
            self.old_refusal('v8-refuses-v10-held-wals')
        self.start(self.args.app,'current-reopen-engine')
        reopened = self.response(['project','open',project['project_id']],'current-project-after-old-refusal','project')
        check(reopened == project,'current reopen changed original project')
        self.clean_refusals(project,manifest,'-restart')
        counts = self.inert()
        self.stop()
        self.save('summary.json',dict(passed=True,qualified=False,
            actual_v8_baseline=True,actual_v10_migration=True,old_reader_format_refusal=True,
            old_reader_existing_database_and_wal_preservation=True,held_nonempty_wal_preservation=True,
            current_reader_reopened=True,clean_cli_refusals=True,current_engine_owner_ipc_refusals=True,
            repeated_refusal_inertness=True,positive_import_status_replay_exercised=False,
            browser_start_records_populated=False,production_acceptance_created=False,
            real_device_contacted=False,positive_engine_localhost_cases='separate play-worker campaign',counts=counts,
            limitations=['No accepted device-v6 prerequisite exists; positive browser dossier intake is unavailable.',
                         'Repeated rejected requests are not successful original-play replay.',
                         'Journal compatibility is exercised with current-created empty v10 membership.',
                         'No physical evidence, acceptance, browser rendering or vendor transport is exercised.']))


def cleanup_preflight(root):
    text = str(root)
    refs, denied, vanished, inspected = [], 0, 0, 0
    for proc in Path('/proc').iterdir():
        if not proc.name.isdigit():
            continue
        for name in ('cwd','root'):
            try:
                target = os.readlink(proc/name); inspected += 1
                if text in target: refs.append(dict(pid=int(proc.name),kind=name))
            except PermissionError: denied += 1
            except OSError: vanished += 1
        for name in ('fd','map_files'):
            try:
                for entry in (proc/name).iterdir():
                    try:
                        target = os.readlink(entry); inspected += 1
                        if text in target: refs.append(dict(pid=int(proc.name),kind=name))
                    except PermissionError: denied += 1
                    except OSError: vanished += 1
            except PermissionError: denied += 1
            except OSError: vanished += 1
        for name in ('maps','mountinfo'):
            try:
                with (proc/name).open() as stream: matched = any(text in line for line in stream)
                inspected += 1
                if matched: refs.append(dict(pid=int(proc.name),kind=name))
            except PermissionError: denied += 1
            except OSError: vanished += 1
    try:
        if text in Path('/proc/net/unix').read_text(): refs.append(dict(kind='unix-socket'))
        inspected += 1
    except PermissionError: denied += 1
    except OSError: vanished += 1
    return dict(owned_root=text,references=refs,inspected=inspected,denied=denied,
                disappeared_or_unavailable=vanished,scope='Accessible references only; denied entries are not proven empty')


def inside(args):
    check(os.environ.get('PULSAR_BROWSER_START_QA') == 'private','use outer supervisor')
    pinned = subjects(args)
    check(pinned == json.loads((args.output/'subjects.json').read_text()),'image subjects changed')
    campaign, passed = Campaign(args), False
    try:
        campaign.campaign()
        check(subjects(args) == pinned,'images changed during campaign')
        passed = True
    except Exception as error:
        campaign.save('failure.json',dict(passed=False,error=str(error)))
    finally:
        try: campaign.stop(cleanup=True)
        except Exception as error:
            passed = False
            campaign.save('cleanup-failure.json',dict(error=str(error)))
        campaign.save('cases.json',campaign.cases)
        campaign.save('processes.json',campaign.processes)
        if any(row['problem'] or row['forced_sigkill'] for row in campaign.processes): passed = False
    return 0 if passed else 1


def main():
    args = arguments()
    os.umask(0o077)
    signal.signal(signal.SIGTERM,interrupted)
    signal.signal(signal.SIGINT,interrupted)
    args.output = args.output.absolute()
    if args.inside:
        return inside(args)
    pinned = subjects(args)
    check(not os.path.lexists(args.output) and args.output.parent.is_dir(),'fresh output directory required')
    check(OWNED_ROOT.is_dir() and not OWNED_ROOT.is_symlink(),'owned scratch root absent')
    scratch = OWNED_ROOT/('pulsar-browser-start-'+uuid.uuid4().hex)
    socket_paths(scratch/'ram'/'s')
    scratch.mkdir(mode=0o700)
    (scratch/'ram').mkdir(mode=0o700)
    args.output.mkdir(mode=0o700)
    (args.output/'subjects.json').write_text(json.dumps(pinned,indent=2)+'\n')
    shutil.copyfile(Path(__file__),args.output/'runner.py')
    command=['bwrap','--unshare-net','--unshare-pid','--die-with-parent','--ro-bind','/','/',
        '--dev-bind','/dev','/dev','--proc','/proc','--bind','/sys/fs/cgroup','/sys/fs/cgroup',
        '--bind',str(scratch),str(scratch),'--bind',str(args.output),str(args.output),
        '--size',str(RAM_LIMIT),'--tmpfs',str(scratch/'ram'),'--chmod','0700',str(scratch/'ram'),
        '/usr/bin/python3',str(Path(__file__).resolve()),*sys.argv[1:],'--inside','--scratch',str(scratch/'ram')]
    (args.output/'invocation.json').write_text(json.dumps(dict(command=command,
        runner_sha256=digest(Path(__file__)),supervisor_timeout_seconds=480,private_tmpfs_bytes=RAM_LIMIT),indent=2)+'\n')
    process,code,forced,problem=None,125,False,None
    raw=scratch/'supervisor.raw.log'
    try:
        with raw.open('x') as log:
            process=subprocess.Popen(command,env=dict(PATH='/usr/bin:/bin',PYTHONDONTWRITEBYTECODE='1',PULSAR_BROWSER_START_QA='private'),
                stdin=subprocess.DEVNULL,stdout=log,stderr=subprocess.STDOUT)
            deadline=time.monotonic()+480
            while process.poll() is None:
                if INTERRUPTED: problem='supervisor interrupted';break
                if time.monotonic()>=deadline: problem='supervisor timeout';break
                if raw.stat().st_size>LIMIT: problem='supervisor output bound';break
                time.sleep(0.05)
            if process.poll() is None:
                process.send_signal(signal.SIGTERM)
                try:process.wait(timeout=15)
                except subprocess.TimeoutExpired:
                    forced=True;process.kill();process.wait(timeout=5)
                code=124
            else:code=process.wait(timeout=1)
    finally:
        joined=process is None or process.poll() is not None
        check(joined,'owned supervisor remains live; scratch preserved')
        if raw.is_file() and not raw.is_symlink():
            with raw.open('rb') as log:text=log.read(LIMIT).decode(errors='replace')
            (args.output/'supervisor.log').write_text(scrub_browser(text))
        check(not scratch.is_symlink(),'owned scratch was replaced')
        preflight=cleanup_preflight(scratch)
        (args.output/'cleanup-preflight.json').write_text(json.dumps(preflight,indent=2)+'\n')
        check(not preflight['references'],'owned scratch still referenced; preserved')
        shutil.rmtree(scratch)
        (args.output/'exit.json').write_text(json.dumps(dict(exit=code,passed=code==0 and not forced,
            supervisor_joined=joined,forced_sigkill=forced,problem=problem,scratch_removed=str(scratch),qualified=False),indent=2)+'\n')
    return code


if __name__=='__main__':
    raise SystemExit(main())
