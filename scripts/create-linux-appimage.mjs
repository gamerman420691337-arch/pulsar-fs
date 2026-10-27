// Produce an actual Linux AppImage candidate with persistent first-use custody.
// Tool bytes are pinned; runtime includes appimage-runtime-cleanup.patch.
// No network request or inference is performed here.
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import crypto from 'node:crypto';
import {spawnSync} from 'node:child_process';
import {generationInventory} from './runtime-generation-packs.mjs';
import {isDeepStrictEqual} from 'node:util';

const toolSha='ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0';
const runtimeSha='91f13cb9d16594b5aebd7b8e7d9a6183961c5444e2054ff39e5fb332f31c3155';
const requireCondition=(value,message)=>{if(!value)throw new Error(message);};
function hash(file) {
 const fd=fs.openSync(file,fs.constants.O_RDONLY|fs.constants.O_NOFOLLOW|fs.constants.O_NONBLOCK);
 try {
  const before=fs.fstatSync(fd,{bigint:true});requireCondition(before.isFile(),'Not a regular input');
  const h=crypto.createHash('sha256'),b=Buffer.alloc(65536);let count=0n;
  for(;;){const n=fs.readSync(fd,b,0,b.length,null);if(!n)break;count+=BigInt(n);requireCondition(count<=before.size,'Input grew');h.update(b.subarray(0,n));}
  const after=fs.fstatSync(fd,{bigint:true});requireCondition(count===before.size&&before.size===after.size&&before.mtimeNs===after.mtimeNs&&before.ctimeNs===after.ctimeNs,'Input changed');
  return h.digest('hex');
 } finally {fs.closeSync(fd);}
}
const base=['pulsar','pulsar-worker','runtimes/ffmpeg','runtimes/ffprobe','media-source/COPYING.LGPLv2.1','media-source/LICENSE.md','media-source/ffmpeg-9.0.2.tar.xz','media-source/configure.log'];
const assistant=['assistant/llama-completion','assistant/model.gguf','assistant/LLAMA-CPP-LICENSE','assistant/MODEL-LICENSE'];
const cpu=['runtimes/libonnxruntime.so.1','runtime-source/ONNX-RUNTIME-LICENSE','runtime-source/ONNX-RUNTIME-THIRD-PARTY-NOTICES'];
const appRun=`#!/bin/sh
set -eu
appdir="\${APPDIR:-}"
if [ -z "$appdir" ]; then appdir=$(CDPATH= cd -- "\${0%/*}" && pwd -P); fi
cache="\${PULSAR_RUNTIME_CACHE_ROOT:-}"
if [ -z "$cache" ]; then
 if [ -n "\${XDG_CACHE_HOME:-}" ]; then cache="$XDG_CACHE_HOME/pulsar/runtime";
 elif [ -n "\${HOME:-}" ]; then cache="$HOME/.cache/pulsar/runtime";
 else printf '%s\\n' 'Pulsar cannot determine a persistent runtime cache; set PULSAR_RUNTIME_CACHE_ROOT.' >&2; exit 1; fi
fi
exec "$appdir/pulsar" runtime-launch "$appdir" "$cache" -- "$@"
`;
const icon='<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64" viewBox="0 0 64 64"><rect width="64" height="64" rx="12" fill="#15243d"/><path d="M8 32h12l5-16 12 32 6-16h13" fill="none" stroke="#6ed5ef" stroke-width="4"/></svg>\n';

let scratch,temporary;
try {
 const args=process.argv.slice(2);requireCondition(args.length===4,'Usage: node create-linux-appimage.mjs ABS_CANDIDATE ABS_PINNED_APPIMAGETOOL ABS_PINNED_RUNTIME NEW_ABS_APPIMAGE');
 requireCondition(args.every(path.isAbsolute),'All paths must be absolute');
 const [source,tool,runtime,destination]=args;
 requireCondition(process.platform==='linux'&&process.arch==='x64','Linux x86-64 required');
 requireCondition(!fs.existsSync(destination),'Destination must be new');
 const parent=path.dirname(destination),parentMeta=fs.lstatSync(parent);
 requireCondition(parentMeta.isDirectory()&&parentMeta.uid===process.getuid()&&(parentMeta.mode&0o022)===0,'Destination parent must be owned and not writable by other users');
 for(const input of [source,parent]) {
  let current='/';for(const component of input.split('/').filter(Boolean)){requireCondition(component!=='.'&&component!=='..','Normal components required');current=path.join(current,component);requireCondition(fs.lstatSync(current).isDirectory(),'Directory path must avoid symlinks');}
 }
 requireCondition(hash(tool)===toolSha&&hash(runtime)===runtimeSha,'AppImage tool/runtime differs from pinned build inputs');
 const manifestPath=path.join(source,'runtime-candidate.json');requireCondition(fs.lstatSync(manifestPath).isFile()&&fs.statSync(manifestPath).size<=65536,'Bounded regular manifest required');
 const original=fs.readFileSync(manifestPath),manifest=JSON.parse(original);
 requireCondition(manifest.schema_version===1&&manifest.kind==='linux-x86-64-runtime-candidate'&&manifest.qualified===false,'Unqualified schema-1 candidate required');
 requireCondition(Array.isArray(manifest.files)&&manifest.files.length<=535,'Finite file inventory required');
 const expected=new Set([...base,...(manifest.assistant?assistant:[]),...(manifest.cpu_runtime?cpu:[])]);
 const generation=generationInventory(manifest.generation_packs);
 for(const name of generation.keys())expected.add(name);
 requireCondition(manifest.files.length===expected.size,'Candidate file inventory differs');
 let total=0;
 for(const record of manifest.files){requireCondition(expected.delete(record.path)&&Number.isSafeInteger(record.byte_len)&&record.byte_len>0&&record.byte_len<=(record.path==='assistant/model.gguf'?32:1)*1024**3&&/^[0-9a-f]{64}$/.test(record.sha256),'Invalid candidate record');const identity=generation.get(record.path);if(identity)requireCondition(record.sha256===identity.sha256&&(identity.byte_len===null?record.byte_len<=128*1024:record.byte_len===identity.byte_len),'Generation payload declaration differs');total+=record.byte_len;}
 requireCondition(expected.size===0&&total<=64*1024**3,'Candidate inventory/size differs');
 const available=fs.statfsSync(parent);requireCondition(available.bavail*available.bsize>total*2+64*1024**2,'Insufficient AppImage publication space');
 scratch=fs.mkdtempSync(path.join(os.tmpdir(),'pulsar-appimage-'));fs.chmodSync(scratch,0o700);
 const appdir=path.join(scratch,'Pulsar.AppDir');fs.mkdirSync(appdir,{mode:0o700});
 for(const record of manifest.files){
  const input=path.join(source,record.path);let parent=source;for(const part of record.path.split('/').slice(0,-1)){parent=path.join(parent,part);requireCondition(fs.lstatSync(parent).isDirectory(),'Symlinked payload directory');}
  requireCondition(fs.lstatSync(input).isFile()&&fs.statSync(input).size===record.byte_len&&hash(input)===record.sha256,'Candidate payload differs');
  if(record.path.startsWith('generation/')&&record.path.endsWith('/manifest.json')) {
   const declaration=manifest.generation_packs.find(p=>record.path===`generation/${p.manifest_sha256}/manifest.json`);
   requireCondition(declaration&&isDeepStrictEqual(JSON.parse(fs.readFileSync(input)),declaration.manifest),'Embedded generation manifest differs');
  }
  const target=path.join(appdir,record.path);fs.mkdirSync(path.dirname(target),{recursive:true,mode:0o700});fs.copyFileSync(input,target,fs.constants.COPYFILE_EXCL);
  fs.chmodSync(target,['pulsar','pulsar-worker','runtimes/ffmpeg','runtimes/ffprobe','assistant/llama-completion'].includes(record.path)?0o755:0o644);
  requireCondition(hash(target)===record.sha256,'Copied candidate payload differs');
 }
 requireCondition(fs.readFileSync(manifestPath).equals(original),'Manifest changed during assembly');
 fs.writeFileSync(path.join(appdir,'runtime-candidate.json'),original,{flag:'wx',mode:0o644});
 fs.writeFileSync(path.join(appdir,'AppRun'),appRun,{flag:'wx',mode:0o755});fs.chmodSync(path.join(appdir,'AppRun'),0o755);
 fs.writeFileSync(path.join(appdir,'pulsar.svg'),icon,{flag:'wx',mode:0o644});
 fs.writeFileSync(path.join(appdir,'pulsar.desktop'),'[Desktop Entry]\nType=Application\nName=Pulsar\nExec=pulsar\nIcon=pulsar\nCategories=AudioVideo;\nTerminal=false\n',{flag:'wx',mode:0o644});
 temporary=path.join(parent,`.pulsar-appimage-${crypto.randomUUID()}`);
 const built=spawnSync(tool,['--runtime-file',runtime,'--no-appstream','--mksquashfs-opt','-processors','--mksquashfs-opt','2',appdir,temporary],{env:{PATH:'/usr/bin:/bin',TMPDIR:scratch,APPIMAGE_EXTRACT_AND_RUN:'1',ARCH:'x86_64'},encoding:'utf8',timeout:600000,maxBuffer:1<<20});
 requireCondition(!built.error&&built.status===0,`AppImage build failed: ${built.error||built.stderr}`);
 fs.chmodSync(temporary,0o700);const fd=fs.openSync(temporary,'r');try{fs.fsyncSync(fd);}finally{fs.closeSync(fd);}
 const imageSha=hash(temporary),size=fs.statSync(temporary).size;
 fs.linkSync(temporary,destination);fs.unlinkSync(temporary);temporary=undefined;
 const directory=fs.openSync(parent,'r');try{fs.fsyncSync(directory);}catch(e){throw new Error(`AppImage published but parent sync failed; inspect before retry: ${e}`);}finally{fs.closeSync(directory);}
 console.log(JSON.stringify({status:'PASS',destination,sha256:imageSha,byte_len:size,manifest_sha256:crypto.createHash('sha256').update(original).digest('hex'),files:manifest.files.length,tool_sha256:toolSha,runtime_sha256:runtimeSha,qualified:false,source_manifest_unchanged:true},null,2));
}catch(e){console.error(`AppImage candidate: ${e.message}`);process.exitCode=1;}
finally{if(temporary&&fs.existsSync(temporary))fs.unlinkSync(temporary);if(scratch)fs.rmSync(scratch,{recursive:true,force:true});}
