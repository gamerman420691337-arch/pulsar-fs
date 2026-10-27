// Assemble a Linux runtime candidate; this does not qualify Core or Complete.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import {spawnSync} from 'node:child_process';
import {generationFiles} from './runtime-generation-packs.mjs';

function requireCondition(condition, message) {
  if (!condition) throw new Error(message);
}
function inspect(file, option) {
  const result = spawnSync('/usr/bin/readelf', [option, file],
    {encoding: 'utf8', timeout: 10000, maxBuffer: 1 << 20});
  requireCondition(!result.error && result.status === 0, `ELF inspection failed: ${file}`);
  return result.stdout;
}
function hash(file) {
  const fd = fs.openSync(file, fs.constants.O_RDONLY | fs.constants.O_NONBLOCK);
  try {
    const before = fs.fstatSync(fd, {bigint:true});
    requireCondition(before.isFile(), `Not a regular hash input: ${file}`);
    const digest = crypto.createHash('sha256');
    const buffer = Buffer.alloc(65536);
    let total = 0n;
    for (;;) {
      const count = fs.readSync(fd, buffer, 0, buffer.length, null);
      if (!count) break;
      total += BigInt(count);
      requireCondition(total <= before.size, `Hash input grew: ${file}`);
      digest.update(buffer.subarray(0,count));
    }
    const after = fs.fstatSync(fd, {bigint:true});
    requireCondition(total === before.size && before.size === after.size
      && before.mtimeNs === after.mtimeNs && before.ctimeNs === after.ctimeNs,
      `Hash input changed: ${file}`);
    return digest.digest('hex');
  } finally {fs.closeSync(fd);}
}
function readDeclaration(declaration, limit=16384) {
  const fd = fs.openSync(declaration, fs.constants.O_RDONLY | fs.constants.O_NONBLOCK | fs.constants.O_NOFOLLOW);
  let spec;
  try {
    const before=fs.fstatSync(fd,{bigint:true});
    requireCondition(before.isFile() && before.size > 0n && before.size <= BigInt(limit),
      'Runtime declaration must be bounded regular bytes');
    const bytes=Buffer.alloc(limit+1);
    let count=0;
    while(count<bytes.length) {
      const n=fs.readSync(fd,bytes,count,bytes.length-count,null);
      if(!n) break;
      count+=n;
    }
    const after=fs.fstatSync(fd,{bigint:true});
    requireCondition(count<=limit && BigInt(count)===before.size && before.size===after.size
      && before.mtimeNs===after.mtimeNs && before.ctimeNs===after.ctimeNs,
      'Runtime declaration changed or exceeded capture bound');
    spec=JSON.parse(bytes.subarray(0,count).toString('utf8'));
  } finally {fs.closeSync(fd);}
  return spec;
}
function assistantFiles(declaration) {
  const spec=readDeclaration(declaration);
  requireCondition(spec && Object.keys(spec).sort().join(',') === 'model,model_license,runtime,runtime_license,schema_version'
    && spec.schema_version === 1, 'Unsupported assistant declaration');
  const outputNames = {runtime:'assistant/llama-completion', model:'assistant/model.gguf',
    runtime_license:'assistant/LLAMA-CPP-LICENSE', model_license:'assistant/MODEL-LICENSE'};
  return Object.entries(outputNames).map(([key,relative]) => {
    const entry = spec[key];
    requireCondition(entry && Object.keys(entry).sort().join(',') === 'byte_len,path,sha256'
      && typeof entry.path === 'string' && path.isAbsolute(entry.path)
      && Number.isSafeInteger(entry.byte_len) && entry.byte_len > 0
      && typeof entry.sha256 === 'string' && /^[0-9a-f]{64}$/.test(entry.sha256),
      `Invalid assistant ${key} identity`);
    requireCondition(fs.statSync(entry.path).isFile() && fs.statSync(entry.path).size === entry.byte_len
      && hash(entry.path) === entry.sha256, `Assistant ${key} identity mismatch`);
    if (key === 'runtime') {
      requireCondition(inspect(entry.path,'-h').includes('Advanced Micro Devices X86-64'),
        'Assistant runtime is not x86-64 ELF');
      fs.accessSync(entry.path,fs.constants.X_OK);
      requireCondition(!inspect(entry.path,'-l').includes('INTERP') && !inspect(entry.path,'-d').includes('NEEDED'),
        'Assistant runtime requires unpackaged dynamic dependencies');
    }
    return [entry.path,relative,entry];
  });
}
function cpuRuntimeFiles(declaration) {
  const spec=readDeclaration(declaration);
  requireCondition(spec && Object.keys(spec).sort().join(',')==='license,notices,runtime,schema_version'
    && spec.schema_version===1,'Unsupported CPU runtime declaration');
  const names={runtime:'runtimes/libonnxruntime.so.1',license:'runtime-source/ONNX-RUNTIME-LICENSE',
    notices:'runtime-source/ONNX-RUNTIME-THIRD-PARTY-NOTICES'};
  const files=Object.entries(names).map(([key,relative])=>{
    const entry=spec[key];
    requireCondition(entry && Object.keys(entry).sort().join(',')==='byte_len,path,sha256'
      && typeof entry.path==='string' && path.isAbsolute(entry.path)
      && Number.isSafeInteger(entry.byte_len) && entry.byte_len>0
      && typeof entry.sha256==='string' && /^[0-9a-f]{64}$/.test(entry.sha256),`Invalid CPU runtime ${key} identity`);
    requireCondition(fs.statSync(entry.path).isFile() && fs.statSync(entry.path).size===entry.byte_len
      && hash(entry.path)===entry.sha256,`CPU runtime ${key} identity mismatch`);
    return [entry.path,relative,entry];
  });
  const runtime=files[0][0];const header=inspect(runtime,'-h');const dynamic=inspect(runtime,'-d');
  requireCondition(header.includes('Advanced Micro Devices X86-64') && /Type:\s+DYN/.test(header)
    && !inspect(runtime,'-l').includes('INTERP'),'CPU runtime is not an x86-64 shared library');
  requireCondition(dynamic.includes('Library soname: [libonnxruntime.so.1]'),'CPU runtime SONAME differs');
  const needed=[...dynamic.matchAll(/Shared library: \[([^\]]+)\]/g)].map(m=>m[1]);
  const baseline=new Set(['libdl.so.2','librt.so.1','libpthread.so.0','libstdc++.so.6','libm.so.6','libgcc_s.so.1','libc.so.6','ld-linux-x86-64.so.2']);
  requireCondition(needed.every(n=>baseline.has(n)),'CPU runtime requires unpackaged non-baseline dependencies');
  return {files,needed};
}

function assemble() {
  requireCondition(process.platform === 'linux' && process.arch === 'x64',
    'This candidate assembler requires Linux x86-64');
  const args = process.argv.slice(2);
  let cpuDeclaration,generationDeclaration;
  while(args.at(-2)==='--cpu-runtime'||args.at(-2)==='--generation-packs') {
    const value=args.pop(),option=args.pop();
    if(option==='--cpu-runtime'){requireCondition(!cpuDeclaration,'Duplicate CPU option');cpuDeclaration=value;}
    else {requireCondition(!generationDeclaration,'Duplicate generation option');generationDeclaration=value;}
  }
  requireCondition((args.length === 3 || args.length === 4) && args.every(path.isAbsolute)
    && [cpuDeclaration,generationDeclaration].every(p=>p===undefined||path.isAbsolute(p)),
    'usage: node assemble-linux-runtime-candidate.mjs /absolute/pulsar /absolute/media-build /absolute/new-output [/absolute/assistant-declaration.json] [--cpu-runtime /absolute/cpu-runtime-declaration.json] [--generation-packs /absolute/generation-declaration.json]');
  const [binary, media] = args.slice(0, 2).map(p => fs.realpathSync(p));
  const output = args[2];
  requireCondition(!fs.existsSync(output), 'Output must be a new directory');
  const files = [
    [binary, 'pulsar'],
    [path.join(path.dirname(binary), 'pulsar-worker'), 'pulsar-worker'],
    [path.join(media, 'ffmpeg'), 'runtimes/ffmpeg'],
    [path.join(media, 'ffprobe'), 'runtimes/ffprobe'],
    [path.join(media, 'COPYING.LGPLv2.1'), 'media-source/COPYING.LGPLv2.1'],
    [path.join(media, 'LICENSE.md'), 'media-source/LICENSE.md'],
    [path.join(media, 'ffmpeg-9.0.2.tar.xz'), 'media-source/ffmpeg-9.0.2.tar.xz'],
    [path.join(media, 'configure.log'), 'media-source/configure.log'],
  ];
  for (const [file] of files) requireCondition(fs.statSync(file).isFile(), `Not a regular input: ${file}`);
  for (const [file] of files.slice(0, 4)) {
    requireCondition(inspect(file, '-h').includes('Advanced Micro Devices X86-64'),
      `Input is not an x86-64 ELF executable: ${file}`);
    fs.accessSync(file, fs.constants.X_OK);
  }
  for (const [file] of files.slice(2, 4)) {
    requireCondition(!inspect(file, '-l').includes('INTERP') && !inspect(file, '-d').includes('NEEDED'),
      `Media tool has dynamic runtime dependencies: ${file}`);
  }
  requireCondition(hash(files[6][0]) === '8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e',
    'Media source archive differs from the pinned recipe');
  const assistant = args[3] ? assistantFiles(args[3]) : [];
  const cpuRuntime=cpuDeclaration ? cpuRuntimeFiles(cpuDeclaration) : null;
  const generation=generationFiles(generationDeclaration,{readDeclaration,hash});
  files.push(...assistant,...(cpuRuntime?.files ?? []),...generation.files);
  const manifestFor=records=>JSON.stringify({
    schema_version: 1, kind: 'linux-x86-64-runtime-candidate', qualified: false,
    assistant: assistant.length ? {runtime_path:'assistant/llama-completion', model_path:'assistant/model.gguf', host_approval_required:true, elf_notes:inspect(assistant[0][0],'-n').split('\n').filter(line=>line.includes('x86 ISA needed:') || line.includes('OS: Linux, ABI:')).map(line=>line.trim())} : null,
    cpu_runtime:cpuRuntime ? {path:'runtimes/libonnxruntime.so.1',needed_system_libraries:cpuRuntime.needed,qualified:false} : null,
    generation_packs:generation.packs,
    limitations: ['Not Core or Complete', assistant.length ? 'Assistant candidate only; model/task/platform qualification open' : 'No bundled assistant',
      cpuRuntime ? 'Bundled CPU runtime candidate; system ABI/platform/license qualification open' : 'No bundled CPU inference runtime', 'GPU policy and drivers remain separate', 'Media and platform matrices remain unqualified'],
    files: records,
  },null,2)+'\n';
  const anticipated=files.map(([source,relative])=>({path:relative,byte_len:fs.statSync(source).size,sha256:hash(source)}));
  requireCondition(Buffer.byteLength(manifestFor(anticipated))<=65536,'Runtime manifest exceeds 64 KiB admission');
  fs.mkdirSync(output, {mode: 0o755});
  const records = [];
  for (const [source, relative, expected] of files) {
    const target = path.join(output, relative);
    fs.mkdirSync(path.dirname(target), {recursive: true});
    fs.copyFileSync(source, target, fs.constants.COPYFILE_EXCL);
    fs.chmodSync(target, relative === 'pulsar' || relative === 'pulsar-worker' || (relative === 'runtimes/ffmpeg' || relative === 'runtimes/ffprobe') || relative === 'assistant/llama-completion' ? 0o755 : 0o644);
    const sha256 = hash(target);
    requireCondition(sha256 === hash(source), `Input changed during assembly: ${source}`);
    const byte_len = fs.statSync(target).size;
    requireCondition(!expected || (expected.sha256 === sha256 && expected.byte_len === byte_len),
      `Declared payload bytes changed during assembly: ${source}`);
    records.push({path: relative, byte_len, sha256});
  }
  requireCondition(Buffer.byteLength(manifestFor(records))<=65536,'Runtime manifest exceeds 64 KiB admission');
  fs.writeFileSync(path.join(output,'runtime-candidate.json'),manifestFor(records),{flag:'wx'});
  console.log(`Assembled runtime candidate: ${output}`);
}
try { assemble(); } catch (error) { console.error(error.message); process.exitCode = 1; }
