import test from 'node:test';
import assert from 'node:assert/strict';
import {generationInventory} from './runtime-generation-packs.mjs';
const pack=()=>({manifest_sha256:'a'.repeat(64),native_execution_approval_required:true,qualified:false,manifest:{schema_version:1,pack_id:'authored-inert-test',target_os:'linux',target_arch:'x86_64',ort_api_version:24,labels:['fixture'],assets:[['models/model.onnx','model'],['runtime.bin','runtime'],['profile.json','profile'],['LICENSE','license']].map(([path,role])=>({path,role,byte_len:1,sha256:'b'.repeat(64)})),license_path:'LICENSE',source_description:'Authored inventory fixture; never quality evidence'}});
test('declared generation inventory preserves nested assets and explicit inert authority',()=>{
 const p=pack(),files=generationInventory([p]);assert.equal(files.size,5);assert.equal(files.get(`generation/${p.manifest_sha256}/models/model.onnx`).byte_len,1);
 assert.throws(()=>generationInventory([p,p]),/Duplicate/);
});
test('generation inventory rejects authority, traversal, ambiguous trees, invalid roles and identity',()=>{
 for(const mutate of [p=>p.qualified=true,p=>p.native_execution_approval_required=false,p=>p.manifest_sha256='../escape',p=>p.manifest.target_os='windows',p=>p.manifest.assets[0].path='../escape',p=>p.manifest.assets[0].path='CON.txt',p=>p.manifest.assets[0].path='.pulsar-stage.json',p=>p.manifest.assets.push({...p.manifest.assets[0]}),p=>p.manifest.assets.push({path:'models',role:'fixture',byte_len:1,sha256:'c'.repeat(64)}),p=>p.manifest.assets.push({path:'Models/other',role:'fixture',byte_len:1,sha256:'c'.repeat(64)}),p=>p.manifest.assets[0].byte_len=257*1024**2,p=>p.manifest.assets[0].role='fixture',p=>p.manifest.license_path='unknown']){
  const p=pack();mutate(p);assert.throws(()=>generationInventory([p]));
 }
});
