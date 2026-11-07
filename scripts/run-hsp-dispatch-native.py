#!/usr/bin/env python3
"""Bounded native HSP v9/v10 history campaign, without device or network effects.

The fixture is synthetic history. Startup does not confer HSP dispatch authority,
qualification, physical evidence, or acceptance. All child processes are joined.
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
# Multiple Engine lifetimes retain immutable worker snapshots in this fixture.
# This bounds test storage; it does not change any production admission limit.
RAM_LIMIT = 1024 * 1024 * 1024
DB_FILES = ('projects.sqlite3', 'projects.sqlite3-wal',
            'device-effects/journal.sqlite3', 'device-effects/journal.sqlite3-wal')
INTERRUPTED = False
HISTORY_QUERIES = {
    'streams': 'SELECT id,intent,route,namespace,device,record,state FROM hsp_streams ORDER BY id',
    'effects': 'SELECT id,stream,sequence,kind,record,state,outcome,outcome_sha256,late,late_sha256,lifecycle_sha256 FROM hsp_effects ORDER BY sequence,id',
    'census': 'SELECT singleton,streams,ordinary,stops,stream_bytes,ordinary_bytes,stop_bytes FROM hsp_census ORDER BY singleton',
}

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
    parser.add_argument('--fixture', type=Path, required=True)
    parser.add_argument('--fixture-sha256', required=True, help='SHA-256 of fixture.json')
    parser.add_argument('--inside', action='store_true', help=argparse.SUPPRESS)
    parser.add_argument('--scratch', type=Path, help=argparse.SUPPRESS)
    return parser.parse_args()


def load_fixture(args):
    directory = args.fixture.absolute()
    check(directory.is_dir() and directory.resolve() == directory, 'fixture directory must be canonical')
    args.fixture = directory
    metadata = regular(directory / 'fixture.json')
    check(metadata.stat().st_size <= LIMIT, 'fixture metadata exceeds bound')
    check(re.fullmatch('[0-9a-f]{64}', args.fixture_sha256) is not None, 'invalid fixture hash')
    check(digest(metadata) == args.fixture_sha256, 'fixture metadata hash differs')
    value = json.loads(metadata.read_text())
    check(value.get('schema_version') == 1 and value.get('journal_schema_version') == 10,
          'unsupported HSP fixture schema')
    check(value.get('synthetic_fixture') is True, 'fixture must be explicitly synthetic')
    for name in ('qualified', 'production_acceptance_created', 'real_device_contacted',
                 'physical_effect_observed', 'dispatch_authority_exported'):
        check(value.get(name) is False, 'fixture boundary differs: ' + name)
    names = {'journal.sqlite3', 'history.json', 'stream-record.json'}
    names.update('effect-' + str(index) + '.json' for index in range(1, 6))
    names.update('outcome-' + str(index) + '.json' for index in range(1, 6))
    entries = value.get('files')
    check(isinstance(entries, list) and len(entries) == len(names), 'fixture file census differs')
    check({entry.get('name') for entry in entries} == names, 'fixture filenames differ')
    for entry in entries:
        path = regular(directory / entry['name'])
        maximum = 64 * 1024 * 1024 if entry['name'] == 'journal.sqlite3' else LIMIT
        check(path.stat().st_size == entry.get('bytes') and path.stat().st_size <= maximum,
              'fixture file size differs: ' + path.name)
        check(digest(path) == entry.get('sha256'), 'fixture bytes differ: ' + path.name)
    history = json.loads((directory / 'history.json').read_text())
    check(len(history['streams']) == 1 and len(history['effects']) == 5, 'fixture history census differs')
    check(history['streams'][0][0] == value['stream_id'] and history['streams'][0][6] == 'custody',
          'fixture must retain unresolved stream custody')
    check([row[0] for row in history['effects']] == value['effect_ids'], 'fixture operation identity differs')
    check([row[3] for row in history['effects']] == ['setup', 'append', 'play', 'append', 'stop'],
          'fixture operation sequence differs')
    check([row[5] for row in history['effects']] == ['acknowledged', 'acknowledged', 'acknowledged', 'unknown', 'acknowledged'],
          'fixture first outcomes differ')
    check(history['streams'][0][5].encode() == (directory / 'stream-record.json').read_bytes(),
          'stream record snapshot differs')
    for index, row in enumerate(history['effects'], 1):
        check(row[4].encode() == (directory / ('effect-' + str(index) + '.json')).read_bytes(),
              'original effect snapshot differs')
        check(row[6].encode() == (directory / ('outcome-' + str(index) + '.json')).read_bytes(),
              'original outcome snapshot differs')
    return value, history

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
    def __init__(self, args, fixture, expected):
        self.args, self.out, self.root = args, args.output, args.scratch
        self.fixture, self.expected = fixture, expected
        self.state, self.temp = self.root / 's', self.root / 't'
        self.temp.mkdir(mode=0o700)
        self.engine = None
        self.processes, self.cases, self.logs, self.capabilities = [], {}, [], set()
        self.children, self.joined = {}, set()

    def spawn(self, label, argv, **kwargs):
        process = subprocess.Popen(argv, **kwargs)
        self.children[process.pid] = (process, label)
        return process

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
        check(process.pid not in self.joined, 'child joined twice: ' + label)
        self.joined.add(process.pid)
        self.processes.append(dict(pid=process.pid, label=label, exit=process.returncode,
                                   joined=True, problem=problem, forced_sigkill=forced))
        check(problem is None and not forced, problem or 'forced child termination')
        self.output_bounds()
        return process.returncode

    def run(self, argv, label, app=None, timeout=25):
        app = app or self.args.app
        out, err = self.root / (label + '.out'), self.root / (label + '.err')
        with out.open('x') as stdout, err.open('x') as stderr:
            process = self.spawn(label, [str(app), *map(str, argv)], env=self.environment(),
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
        check(isinstance(result['exit'], int) and result['exit'] > 0,
              'expected a normal nonzero refusal exit: ' + label)
        if contains is not None:
            check(contains in result['stderr'], 'native refusal differs: ' + label)
        return result

    def start(self, app, label):
        check(self.engine is None, 'another owned Engine is active')
        self.save(label + '-socket-paths.json', socket_paths(self.state))
        path = self.root / (label + '.log')
        with path.open('x') as log:
            process = self.spawn(label, [str(app), 'engine'], env=self.environment(),
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

    def join_remaining(self):
        errors = []
        for pid, (process, label) in self.children.items():
            if pid in self.joined:
                continue
            try:
                if process.poll() is None:
                    process.send_signal(signal.SIGTERM)
                self.wait(process, label, 15, cleanup=True)
            except Exception as error:
                errors.append(str(error))
        check(not errors, '; '.join(errors))
        check(set(self.children) == self.joined, 'direct child census contains unjoined processes')

    def assert_capabilities(self, label):
        value = self.response(['capabilities'], label, 'capabilities')
        check(value.get('physical_playback') is False, 'reader reports physical playback enabled')
        self.save(label + '.json', value)

    def current_history(self, label):
        with closing(sqlite3.connect((self.state / 'device-effects/journal.sqlite3').as_uri() + '?mode=ro',
                                     uri=True, timeout=2)) as db:
            check(db.execute('PRAGMA user_version').fetchone()[0] == 10, 'current journal is not v10')
            actual = {name: [list(row) for row in db.execute(query)]
                      for name, query in HISTORY_QUERIES.items()}
            boot = db.execute('SELECT boot FROM owner WHERE singleton=1').fetchone()[0]
        check(actual == self.expected, 'original HSP stream/effect/outcome bytes changed: ' + label)
        self.save(label + '-history.json', actual)
        return boot

    def clean_current(self):
        self.state = self.root / 'clean'
        self.start(self.args.app, 'clean-v10-engine')
        self.assert_capabilities('clean-v10-capabilities')
        with closing(self.db('device-effects/journal.sqlite3')) as db:
            check(db.execute('PRAGMA user_version').fetchone()[0] == 10, 'fresh current journal is not v10')
            counts = {name: db.execute('SELECT count(*) FROM ' + name).fetchone()[0]
                      for name in ('effects', 'setup_effects', 'clock_sync_effects', 'play_operations',
                                   'play_reference_members', 'hsp_streams', 'hsp_effects')}
            check(all(count == 0 for count in counts.values()), 'fresh current Engine created effect history')
            check(db.execute('SELECT * FROM hsp_census').fetchall() == [(1, 0, 0, 0, 0, 0, 0)],
                  'fresh HSP census differs')
        self.stop()
        self.save('clean-v10-counts.json', counts)
        self.state = self.root / 's'

    def campaign(self):
        check([name for _, name in socket.if_nameindex()] == ['lo'], 'private network namespace is required')
        self.clean_current()
        self.start(self.args.old_app, 'v9-baseline-engine')
        with closing(self.db('device-effects/journal.sqlite3')) as db:
            check(db.execute('PRAGMA user_version').fetchone()[0] == 9, 'retained executable did not create v9 journal')
        project = self.response(['project', 'create', 'SYNTHETIC HSP history reader QA'],
                                'v9-project-create', 'project', self.args.old_app)
        self.stop()
        self.save('v9-baseline-hashes.json', self.hashes())
        target = self.state / 'device-effects/journal.sqlite3'
        for suffix in ('-wal', '-shm'):
            sidecar = Path(str(target) + suffix)
            check(not os.path.lexists(sidecar), 'stopped baseline journal retains a sidecar')
        shutil.copyfile(self.args.fixture / 'journal.sqlite3', target)
        target.chmod(0o600)
        check(self.current_history('installed-fixture') == self.fixture['original_boot'],
              'installed fixture owner differs')
        self.old_refusal('v9-refuses-populated-v10')
        with ExitStack() as stack:
            for name in ('projects.sqlite3', 'device-effects/journal.sqlite3'):
                db = stack.enter_context(closing(self.db(name)))
                version = db.execute('PRAGMA user_version').fetchone()[0]
                check(db.execute('PRAGMA journal_mode=WAL').fetchone()[0] == 'wal', 'expected WAL mode')
                db.execute('PRAGMA wal_autocheckpoint=0')
                db.execute('PRAGMA user_version=' + str(version))
                db.commit()
                check((self.state / (name + '-wal')).stat().st_size > 0, 'expected held nonempty WAL')
            self.old_refusal('v9-refuses-populated-v10-held-wals')
        self.start(self.args.app, 'current-v10-populated-engine')
        self.assert_capabilities('current-v10-capabilities')
        current = self.response(['project', 'open', project['project_id']], 'current-project-open', 'project')
        check(current == project, 'current reader changed original project')
        first_boot = self.current_history('first-current-start')
        check(first_boot != self.fixture['original_boot'], 'current Engine did not replace journal owner')
        self.stop()
        self.current_history('first-current-shutdown')
        self.start(self.args.app, 'current-v10-reopen-engine')
        self.assert_capabilities('reopened-v10-capabilities')
        second_boot = self.current_history('second-current-start')
        check(second_boot not in (first_boot, self.fixture['original_boot']), 'reopen did not create a fresh boot')
        reopened = self.response(['project', 'open', project['project_id']], 'reopened-project', 'project')
        check(reopened == project, 'reopen changed original project')
        self.stop()
        self.current_history('second-current-shutdown')
        return dict(qualified=False, actual_v9_baseline=True, actual_clean_v10=True,
            populated_v10_old_reader_format_refusal=True, existing_databases_and_wals_unchanged=True,
            held_nonempty_wals_unchanged=True, current_start_shutdown_reopen=True,
            original_stream_effect_outcome_bytes_unchanged=True, unresolved_stream_retained=True,
            unknown_append_retained_after_acknowledged_stop=True, physical_playback_capability=False,
            restored_hsp_authority_claimed=False, production_acceptance_created=False,
            real_device_contacted=False, fixture_sha256=self.args.fixture_sha256,
            limitations=['Synthetic history exercises reader compatibility, not native HSP HTTP dispatch.',
                         'Fresh boot ownership and unchanged historical rows do not constitute restored HSP authority.',
                         'No device, physical effect, inference, qualification or human acceptance is exercised.'])

def cleanup_preflight(root):
    text = str(root)
    refs, denied, vanished, inspected = [], 0, 0, 0
    deadline = time.monotonic() + 30
    for proc in Path('/proc').iterdir():
        check(time.monotonic() < deadline, 'cleanup reference scan timed out; scratch preserved')
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
                    check(time.monotonic() < deadline, 'cleanup reference scan timed out; scratch preserved')
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
    check(os.environ.get('PULSAR_HSP_NATIVE_QA') == 'private', 'use outer supervisor')
    pinned = subjects(args)
    check(pinned == json.loads((args.output / 'subjects.json').read_text()), 'image subjects changed')
    fixture, expected = load_fixture(args)
    campaign, passed, summary = Campaign(args, fixture, expected), False, {}
    try:
        summary = campaign.campaign()
        check(subjects(args) == pinned, 'images changed during campaign')
        check(load_fixture(args) == (fixture, expected), 'input fixture changed during campaign')
        passed = True
    except Exception as error:
        campaign.save('failure.json', dict(passed=False, error=str(error)))
    finally:
        cleanup_errors = []
        for cleanup in (lambda: campaign.stop(cleanup=True), campaign.join_remaining):
            try:
                cleanup()
            except Exception as error:
                passed = False
                cleanup_errors.append(str(error))
        if cleanup_errors:
            campaign.save('cleanup-failure.json', dict(errors=cleanup_errors))
        campaign.save('cases.json', campaign.cases)
        campaign.save('processes.json', campaign.processes)
        joined_all = set(campaign.children) == campaign.joined
        if not joined_all or any(row['problem'] or row['forced_sigkill'] for row in campaign.processes):
            passed = False
        summary.update(passed=passed, qualified=False, direct_children_started=len(campaign.children),
                       direct_children_joined=len(campaign.joined), all_direct_children_joined=joined_all)
        campaign.save('summary.json', summary)
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
    load_fixture(args)
    check(args.output.parent.resolve() == args.output.parent, 'output parent must be canonical')
    check(not os.path.lexists(args.output) and args.output.parent.is_dir(),'fresh output directory required')
    check(OWNED_ROOT.is_dir() and not OWNED_ROOT.is_symlink(),'owned scratch root absent')
    scratch = OWNED_ROOT/('pulsar-hsp-native-'+uuid.uuid4().hex)
    socket_paths(scratch/'ram'/'s')
    socket_paths(scratch/'ram'/'clean')
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
            process=subprocess.Popen(command,env=dict(PATH='/usr/bin:/bin',PYTHONDONTWRITEBYTECODE='1',PULSAR_HSP_NATIVE_QA='private'),
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
        (args.output/'exit.json').write_text(json.dumps(dict(exit=code,passed=code==0 and not forced and problem is None,
            direct_children_started=int(process is not None),direct_children_joined=int(process is not None and joined),
            supervisor_joined=joined,forced_sigkill=forced,problem=problem,scratch_removed=str(scratch),qualified=False),indent=2)+'\n')
    return code if not forced and problem is None else 1


if __name__ == '__main__':
    raise SystemExit(main())
