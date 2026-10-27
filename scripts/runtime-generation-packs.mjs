// Inert model-pack delivery metadata; never native approval or default selection.
import fs from 'node:fs';
import path from 'node:path';
const requireCondition=(condition,message)=>{if(!condition)throw new Error(message);};
const keys=(value,names)=>value&&Object.keys(value).sort().join(',')===names.split(',').sort().join(',');
const digest=value=>typeof value==='string'&&/^[0-9a-f]{64}$/.test(value);
const safePath=value=>typeof value==='string'&&value.length<=240&&value.split('/').every(p=>/^[A-Za-z0-9_.-]+$/.test(p)&&p.length<=128&&p!=='.'&&p!=='..'&&!p.endsWith('.')&&!/^(CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])$/i.test(p.split('.')[0]))&&!['manifest.json','receipt.json','.pulsar-stage.json'].includes(value.split('/')[0].toLowerCase());
export function generationInventory(packs=[]) {
 requireCondition(Array.isArray(packs)&&packs.length<=8,'Bounded generation pack inventory required');
 const expected=new Map();
 for(const p of packs){
  requireCondition(keys(p,'manifest_sha256,manifest,native_execution_approval_required,qualified')&&digest(p.manifest_sha256)&&p.native_execution_approval_required===true&&p.qualified===false,'Invalid inert generation declaration');
  const m=p.manifest;
  requireCondition(keys(m,'schema_version,pack_id,target_os,target_arch,ort_api_version,labels,assets,license_path,source_description')&&m.schema_version===1&&m.target_os==='linux'&&m.target_arch==='x86_64'&&m.ort_api_version===24&&typeof m.pack_id==='string'&&m.pack_id.length>0&&m.pack_id.length<=128&&/^[A-Za-z0-9._:-]+$/.test(m.pack_id),'Invalid generation manifest');
  requireCondition(typeof m.source_description==='string'&&m.source_description.length>0&&Buffer.byteLength(m.source_description)<=1024&&!/\p{Cc}/u.test(m.source_description),'Invalid pack source description');
  requireCondition(Array.isArray(m.labels)&&m.labels.length>0&&m.labels.length<=256&&new Set(m.labels).size===m.labels.length&&m.labels.every(l=>typeof l==='string'&&l.length>0&&Buffer.byteLength(l)<=128&&!/\p{Cc}/u.test(l)),'Invalid pack labels');
  requireCondition(Array.isArray(m.assets)&&m.assets.length>0&&m.assets.length<=64,'Bounded pack assets required');
  const prefix=`generation/${p.manifest_sha256}/`,manifest=`${prefix}manifest.json`;
  requireCondition(!expected.has(manifest),'Duplicate bundled pack');expected.set(manifest,{sha256:p.manifest_sha256,byte_len:null});
  const names=new Set(),spelling=new Map();let total=0;
  for(const a of m.assets){
   requireCondition(keys(a,'path,role,byte_len,sha256')&&safePath(a.path)&&digest(a.sha256)&&Number.isSafeInteger(a.byte_len)&&a.byte_len>0&&a.byte_len<=256*1024**2&&['model','runtime','profile','license','notice','fixture','native_dependency'].includes(a.role),'Invalid generation asset');
   const lower=a.path.toLowerCase();requireCondition(!names.has(lower),'Duplicate generation asset');names.add(lower);
   let partial='';for(const component of a.path.split('/')){partial+=`${partial?'/':''}${component}`;requireCondition(!spelling.has(partial.toLowerCase())||spelling.get(partial.toLowerCase())===partial,'Ambiguous generation path');spelling.set(partial.toLowerCase(),partial);}
   expected.set(prefix+a.path,{sha256:a.sha256,byte_len:a.byte_len});total+=a.byte_len;
  }
  requireCondition(total<=512*1024**2&&['model','runtime','profile'].every(role=>m.assets.filter(a=>a.role===role).length===1)&&m.assets.some(a=>a.path===m.license_path&&a.role==='license'),'Invalid pack roles/size/license');
 }
 for(const name of expected.keys()){let parent=path.posix.dirname(name);while(parent!=='.'){requireCondition(!expected.has(parent),'Generation file/directory collision');parent=path.posix.dirname(parent);}}
 return expected;
}
export function generationFiles(declaration,{readDeclaration,hash}) {
 if(!declaration)return {files:[],packs:[]};
 const spec=readDeclaration(declaration);
 requireCondition(keys(spec,'schema_version,packs')&&spec.schema_version===1&&Array.isArray(spec.packs)&&spec.packs.length>0&&spec.packs.length<=8,'Unsupported generation supplier declaration');
 const files=[],packs=[];
 for(const entry of spec.packs){
  requireCondition(keys(entry,'path,manifest_sha256')&&typeof entry.path==='string'&&path.isAbsolute(entry.path)&&digest(entry.manifest_sha256),'Invalid generation supplier');
  let current='/';for(const component of entry.path.split('/').filter(Boolean)){requireCondition(component!=='.'&&component!=='..','Normal supplier path required');current=path.join(current,component);requireCondition(fs.lstatSync(current).isDirectory(),'Generation supplier directory must avoid symlinks');}
  const source=path.join(entry.path,'manifest.json');
  const manifest=readDeclaration(source,128*1024);
  requireCondition(hash(source)===entry.manifest_sha256,'Generation manifest pin differs');
  const pack={manifest_sha256:entry.manifest_sha256,manifest,native_execution_approval_required:true,qualified:false};
  const inventory=generationInventory([pack]);packs.push(pack);
  for(const [relative,identity] of inventory){
   const suffix=relative.slice(`generation/${entry.manifest_sha256}/`.length),input=path.join(entry.path,suffix);
   let parent=entry.path;for(const part of suffix.split('/').slice(0,-1)){parent=path.join(parent,part);requireCondition(fs.lstatSync(parent).isDirectory(),'Symlinked generation directory');}
   requireCondition(fs.lstatSync(input).isFile()&&hash(input)===identity.sha256,'Generation supplier bytes differ');
   const size=fs.statSync(input).size;requireCondition(identity.byte_len===null?size>0&&size<=128*1024:size===identity.byte_len,'Generation supplier size differs');
   files.push([input,relative,{sha256:identity.sha256,byte_len:size}]);
  }
 }
 generationInventory(packs);
 return {files,packs};
}
