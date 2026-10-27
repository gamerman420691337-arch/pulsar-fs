// Authored correspondence only: no learned-quality or physical qualification.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import {spawn, spawnSync} from 'node:child_process';
import {setTimeout as delay} from 'node:timers/promises';

const enabled = process.env.PULSAR_ANNOTATED_CUDA_BINARY && process.env.PULSAR_ANNOTATED_CUDA_POLICY
  && process.env.PULSAR_ANNOTATED_MODEL_FIXTURE_DIR;
const digest = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
test('annotated file generation retains strict CUDA and model evidence through restart and portable clone',
  {skip: !enabled, timeout: 180000}, async t => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'pulsar-annotated-cuda-native-'));
    fs.chmodSync(root, 0o700);
    t.diagnostic(`retained private annotated CUDA campaign: ${root}`);
    const binary = fs.realpathSync(process.env.PULSAR_ANNOTATED_CUDA_BINARY);
    const fixture = fs.realpathSync(process.env.PULSAR_ANNOTATED_MODEL_FIXTURE_DIR);
    const policy = JSON.parse(fs.readFileSync(process.env.PULSAR_ANNOTATED_CUDA_POLICY));
    for (const lib of policy.native_dependencies.libraries) {
      const bytes = fs.readFileSync(lib.path);
      assert.equal(bytes.length, lib.byte_len);
      assert.equal(digest(bytes), lib.sha256);
    }
    const runtime = policy.native_dependencies.libraries.find(lib => lib.names.includes('libonnxruntime.so.1'));
    assert.ok(runtime);
    const policyFile = path.join(root, 'policy.json');
    fs.writeFileSync(policyFile, JSON.stringify(policy), {mode: 0o600});
    const expected = JSON.parse(fs.readFileSync(path.join(fixture, 'expected.json')));
    const pairs = fs.readFileSync(path.join(fixture, 'pairs.rgb'));
    const model = path.join(fixture, 'model.onnx'), modelBytes = fs.readFileSync(model);
    assert.equal(digest(pairs), expected.rgb_sha256);
    assert.equal(digest(modelBytes), expected.model_sha256);
    const env = {...process.env, PULSAR_STATE_DIR: path.join(root, 'state'),
      PULSAR_ORT_DYLIB: runtime.path, PULSAR_CUDA_OWNER_POLICY: policyFile};
    const commands = [];
    const run = (executable, args, commandEnv = env) => {
      const result = spawnSync(executable, args, {env: commandEnv, encoding: 'utf8', timeout: 90000, maxBuffer: 4 << 20});
      commands.push({executable, args, status: result.status});
      assert.ifError(result.error);
      return result;
    };
    const cli = args => run(binary, args);
    const ok = args => {const result = cli(args); assert.equal(result.status, 0, result.stderr); return result.stdout;};
    const db = sql => {
      const result = run('/usr/bin/python3', ['-c',
        "import sqlite3,json,sys;d=sqlite3.connect('file:'+sys.argv[1]+'?mode=ro',uri=True);print(json.dumps(d.execute(sys.argv[2]).fetchall()))",
        path.join(env.PULSAR_STATE_DIR, 'projects.sqlite3'), sql]);
      assert.equal(result.status, 0, result.stderr); return JSON.parse(result.stdout);
    };
    const packed = [];
    for (let i = 0; i < 28; i++) for (let y = 0; y < 64; y++) packed.push(
      Buffer.alloc(96 * 3, 17), Buffer.alloc(16 * 3, 231),
      pairs.subarray((i * 64 + y) * 64 * 3, (i * 64 + y + 1) * 64 * 3), Buffer.alloc(16 * 3, 231));
    const raw = path.join(root, 'stereo.rgb'), video = path.join(root, 'stereo.mkv');
    fs.writeFileSync(raw, Buffer.concat(packed));
    const encoded = run('/usr/bin/ffmpeg', ['-v', 'error', '-nostdin', '-f', 'rawvideo', '-pixel_format', 'rgb24',
      '-video_size', '192x64', '-framerate', '8', '-i', raw, '-threads', '1', '-c:v', 'ffv1', '-output_ts_offset', '2', video]);
    assert.equal(encoded.status, 0, encoded.stderr);
    const view = {source_sha256: digest(fs.readFileSync(video)), declaration: {schema_version: 1,
      source_dimensions: [192, 64], analysis_dimensions: [96, 64], target_region: [16, 0, 64, 64],
      target_id: 'authored', video_view: {layout: 'side_by_side_left_right', eye: 'right', projection: {kind: 'flat'}}}};
    const profile = JSON.parse(fs.readFileSync('crates/pulsar-engine/tests/fixtures/cuda-intent-request.json'))
      .operation.arguments.settings.inference_profile;
    profile.model = {artifact_id: digest(modelBytes), sha256: digest(modelBytes), byte_len: modelBytes.length};
    profile.runtime = {artifact_id: runtime.sha256, sha256: runtime.sha256, byte_len: runtime.byte_len};
    profile.preprocessing.resize = 'annotated_temporal_view';
    const viewFile = path.join(root, 'view.json'), profileFile = path.join(root, 'profile.json');
    fs.writeFileSync(viewFile, JSON.stringify(view)); fs.writeFileSync(profileFile, JSON.stringify(profile));
    let engine, stopped, ended = true;
    const start = async () => {
      const log = fs.openSync(path.join(root, 'engine.stderr'), 'a', 0o600);
      engine = spawn(binary, ['engine'], {env, stdio: ['ignore', 'ignore', log]}); fs.closeSync(log); ended = false;
      stopped = new Promise(resolve => {engine.once('error', error => {ended = true; resolve({error: String(error)});});
        engine.once('exit', (code, signal) => {ended = true; resolve({code, signal});});});
      const end = Date.now() + 15000;
      while (!fs.existsSync(path.join(env.PULSAR_STATE_DIR, 'engine.sock'))) {
        assert.equal(ended, false, fs.readFileSync(path.join(root, 'engine.stderr'), 'utf8'));
        assert.ok(Date.now() < end, 'Engine startup deadline'); await delay(25);
      }
      assert.equal(cli(['capabilities']).status, 0);
    };
    const stop = async () => {if (!ended) engine.kill('SIGTERM'); await stopped;};
    t.after(async () => {if (engine) await stop(); fs.writeFileSync(path.join(root, 'commands.json'), JSON.stringify(commands, null, 2));});
    const output = path.join(root, 'authored.funscript');
    await start();
    ok(['generate', video, '--model', model, '--inference-profile', profileFile, '--annotated-view', viewFile, '--output', output]);
    const [project, attempt, manifestRaw] = db('SELECT project,attempt,manifest FROM jobs')[0];
    const request = JSON.parse(manifestRaw), directory = request.output_dir;
    assert.deepEqual(request.operation.arguments.settings.annotated_view, view);
    assert.ok(request.cuda_execution);
    const receiptBytes = fs.readFileSync(path.join(directory, 'cuda-inference.json'));
    const traceBytes = fs.readFileSync(path.join(directory, 'cuda-trace.json'));
    const receipt = JSON.parse(receiptBytes), trace = JSON.parse(traceBytes);
    const general = JSON.parse(fs.readFileSync(path.join(directory, 'receipt.json')));
    const program = JSON.parse(fs.readFileSync(path.join(directory, 'program.json')));
    assert.equal(receipt.provider, 'cuda'); assert.equal(receipt.successful_runs, 27);
    const nodes = trace.filter(event => event.cat === 'Node'); assert.ok(nodes.length > 0);
    assert.ok(nodes.every(node => node.args.provider === 'CUDAExecutionProvider'));
    assert.equal(general.schema_version, 3); assert.equal(general.qualification, 'unqualified');
    assert.equal(general.source_origin_pts, 2000);
    assert.deepEqual(general.source_time_base, {numerator: 1, denominator: 1000});
    assert.equal(general.annotated_model.training_intake_sha256, expected.training_intake_sha256);
    assert.equal(program.tracks[0].actions.length, 28); assert.deepEqual(program.tracks[0].gaps, []);
    assert.deepEqual(program.tracks[0].actions.map(action => action.time), Array.from({length: 28}, (_, i) => i * 125000000));
    assert.deepEqual(general.samples.map(sample => [sample.source_frame_index, sample.source_pts, sample.project_time_ns]),
      Array.from({length: 28}, (_, i) => [i, 2000 + i * 125, i * 125000000]));
    const errors = expected.positions.map((position, i) => Math.abs(program.tracks[0].actions[i * 2 + 1].position - position));
    assert.ok(Math.max(...errors) <= 1e-5, JSON.stringify(errors));
    const membership = db('SELECT attempt,job,manifest_sha256 FROM annotated_generation_attempts');
    assert.equal(membership.length, 1); assert.equal(membership[0][0], attempt);
    const cudaMembership = db('SELECT attempt FROM cuda_candidate_evidence'); assert.deepEqual(cudaMembership, [[attempt]]);
    const oversizedPreview = cli(['project', 'preview-annotated', project, '125000000',
      '--source-version', request.source.source_version, '--annotated-view', viewFile,
      '--model', model, '--inference-profile', profileFile]);
    assert.notEqual(oversizedPreview.status, 0);
    assert.match(oversizedPreview.stderr, /inference profile exceeds attempt budget/);
    const previewProfile = {...profile, minimum_memory_bytes: 128 * 1024 * 1024,
      minimum_output_bytes: 1024 * 1024};
    const previewProfileFile = path.join(root, 'preview-profile.json');
    fs.writeFileSync(previewProfileFile, JSON.stringify(previewProfile));
    const preview = cli(['project', 'preview-annotated', project, '125000000',
      '--source-version', request.source.source_version, '--annotated-view', viewFile,
      '--model', model, '--inference-profile', previewProfileFile]);
    assert.equal(preview.status, 0, preview.stderr);
    const previewResult=JSON.parse(preview.stdout).value;
    assert.ok(Math.abs(previewResult.annotated.position-expected.positions[0])<=1e-5);
    assert.deepEqual(db('SELECT attempt FROM cuda_candidate_evidence'), cudaMembership);
    const end = Date.now() + 10000;
    for (;;) {
      const fence = JSON.parse(fs.readFileSync(path.join(env.PULSAR_STATE_DIR, 'worker-fence.json')));
      if (['groups', 'retired', 'staging'].every(key => Object.keys(fence[key]).length === 0)
        && Object.keys(fence.creation.records).length === 0) break;
      assert.ok(Date.now() < end, 'CUDA custody did not retire'); await delay(25);
    }
    await stop(); await start();
    assert.deepEqual(db('SELECT attempt,job,manifest_sha256 FROM annotated_generation_attempts'), membership);
    assert.deepEqual(fs.readFileSync(path.join(directory, 'cuda-inference.json')), receiptBytes);
    assert.deepEqual(fs.readFileSync(path.join(directory, 'cuda-trace.json')), traceBytes);
    const archive = path.join(root, 'authored.pulsar');
    ok(['package-export', project, archive, '--acknowledge-unencrypted-private-data']);
    const imported = JSON.parse(ok(['package-import', archive]));
    const clone = imported.receipt.project_id, cloneOutput = path.join(root, 'clone.funscript');
    ok(['export-axis', clone, cloneOutput]); assert.deepEqual(fs.readFileSync(cloneOutput), fs.readFileSync(output));
    await stop(); await start(); ok(['project', 'open', clone]);
    await stop();
    let precedingReaderRefused = null;
    if (process.env.PULSAR_ANNOTATED_CUDA_OLD_READER) {
      const legacyState = path.join(root, 'preceding-reader-state');
      fs.cpSync(env.PULSAR_STATE_DIR, legacyState, {recursive: true});
      fs.chmodSync(legacyState, 0o700);
      const database = path.join(legacyState, 'projects.sqlite3');
      const before = digest(fs.readFileSync(database));
      const refused = run(fs.realpathSync(process.env.PULSAR_ANNOTATED_CUDA_OLD_READER), ['engine'],
        {...env, PULSAR_STATE_DIR: legacyState});
      assert.notEqual(refused.status, 0);
      assert.match(refused.stderr, /annotated temporal preprocessing|annotated profile|annotated generation/);
      assert.equal(digest(fs.readFileSync(database)), before, 'preceding reader changed the database before refusing');
      precedingReaderRefused = true;
    }
    const checked = {root, binary_sha256: digest(fs.readFileSync(binary)), attempt, project, clone,
      maximum_cpu_cuda_position_error: Math.max(...errors), successful_runs: receipt.successful_runs,
      cuda_nodes: nodes.length, restart_preserved_original_evidence: true, portable_clone_export_equal: true,
      preceding_reader_refused_without_database_change: precedingReaderRefused,
      qualified: false, physical_dispatch: false};
    fs.writeFileSync(path.join(root, 'checked.json'), JSON.stringify(checked, null, 2));
    if (process.env.PULSAR_ANNOTATED_CUDA_EVIDENCE) {
      const out = path.resolve(process.env.PULSAR_ANNOTATED_CUDA_EVIDENCE); assert.equal(fs.existsSync(out), false);
      fs.mkdirSync(out, {recursive: true, mode: 0o700});
      for (const file of ['program.json', 'receipt.json', 'cuda-inference.json', 'cuda-trace.json', 'initial-checkpoint.json'])
        fs.copyFileSync(path.join(directory, file), path.join(out, file));
      for (const file of ['checked.json', 'authored.funscript', 'clone.funscript']) fs.copyFileSync(path.join(root, file), path.join(out, file));
    }
  });
