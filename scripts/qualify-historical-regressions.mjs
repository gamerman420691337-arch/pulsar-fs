#!/usr/bin/env node
// Historical experiment only. Never a production build or whole-B1 gate.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
const evidence=path.join(root,'research/phase-b/historical-execution');
const legacy=path.join(root,'legacy/pre-engine');
const fixture=path.join(root,'research/phase-b/historical-regressions.json');
const sha=b=>crypto.createHash('sha256').update(b).digest('hex');
const check=(ok,msg)=>{if(!ok)throw Error(msg);};
const read=p=>{const s=fs.lstatSync(p);check(s.isFile()&&!s.isSymbolicLink(),'not regular: '+p);return fs.readFileSync(p);};
const json=(p,v)=>fs.writeFileSync(p,JSON.stringify(v,null,2)+'\n',{flag:'wx'});
const files={'src/neural/yolo.rs':5,'src/neural/yolo26.rs':3,'src/neural/tracker.rs':4,'src/neural/point_tracker.rs':3,'src/signal.rs':4,'src/tracking.rs':1};
const related={
 'src/neural/yolo.rs':{file:'crates/pulsar-core/tests/perception.rs',tests:['detector_rejects_rank_count_and_negative_geometry','detector_rejects_nonfinite_even_below_threshold','detector_clips_endpoints_and_recomputes_center','detector_drops_fully_outside_and_validates_classes'],limit:'Declared tensor geometry; category correspondence, not identical legacy API.'},
 'src/neural/yolo26.rs':{file:'crates/pulsar-core/tests/perception.rs',tests:['detector_rows_contract_rejects_fractional_class_and_bad_shape'],limit:'Explicit pixel-corner layout differs from old normalized direct-output API.'},
 'src/neural/tracker.rs':{file:'crates/pulsar-core/tests/perception.rs',tests:['tracker_keeps_identity_with_permuted_detections_and_moves_boxes','tracker_prediction_translates_box_not_only_center','tracker_counts_skipped_frames_when_expiring','tracker_cut_retires_identity_even_for_same_geometry'],limit:'Velocity prediction is not historical dense-flow box propagation.'},
 'src/neural/point_tracker.rs':{file:'crates/pulsar-core/tests/perception.rs',tests:['flow_independent_reverse_pass_accepts_translation','flow_rejects_shape_mismatch_and_nonfinite_points','flow_budgets_and_config_are_checked'],limit:'Independently reversed images replace a single-forward cycle-error oracle.'},
 'src/signal.rs':{file:'crates/pulsar-engine/src/worker/good_tests.rs',tests:[],limit:'Earlier actual-worker synthetic hold/direction/plateau/pause-anchor checks; exact current names not asserted or rerun here.'},
 'src/tracking.rs':{file:'crates/pulsar-core/tests/perception.rs',tests:['flow_rejects_shape_mismatch_and_nonfinite_points','flow_budgets_and_config_are_checked'],limit:'Explicit buffer geometry replaces mutable legacy flow geometry; category correspondence only.'}
};
function tree(dir,rel='',skip=[]){
 const rows=[];
 for(const name of fs.readdirSync(dir).sort()){
  if(skip.includes(name))continue;
  const p=path.join(dir,name),key=path.posix.join(rel,name),s=fs.lstatSync(p);
  if(s.isDirectory())rows.push(...tree(p,key,skip));
  else{const b=read(p);rows.push({path:key,sha256:sha(b),byte_len:b.length});}
 }
 return rows;
}
function baseline(){
 const rows=tree(path.join(legacy,'src'),'src');
 for(const name of ['Cargo.toml','main.rs']){const b=read(path.join(legacy,name));rows.push({path:name,sha256:sha(b),byte_len:b.length});}
 rows.sort((a,b)=>a.path.localeCompare(b.path));
 return {files:rows,sha256:sha(Buffer.from(JSON.stringify(rows)))};
}
function archive(){
 const raw=read(fixture),values=JSON.parse(raw);
 check(Object.keys(values).sort().join('|')===Object.keys(files).sort().join('|'),'six-module set changed');
 const cases=[],modules=[];
 for(const [file,count]of Object.entries(files)){
  const text=values[file];
  check(typeof text==='string'&&text.startsWith('\n')&&text.includes('#[cfg(test)]')&&text.includes('mod m1_regression_tests'),'unexpected module encoding '+file);
  const names=[...text.matchAll(/#\[test\]\s*fn\s+([A-Za-z_][A-Za-z_0-9]*)\s*\(/g)].map(m=>m[1]);
  check(names.length===count,'original test count changed '+file);
  const prefix=file.replace(/^src\//,'').replace(/\.rs$/,'').replaceAll('/','::');
  for(const name of names)cases.push({identity:prefix+'::m1_regression_tests::'+name,preserved_file:file,original_test_name:name,current_contract_related_coverage:related[file],current_contract_execution:'not_run_by_this_campaign'});
  modules.push({file,sha256:sha(Buffer.from(text)),byte_len:Buffer.byteLength(text),tests:names,decode:'one JSON.parse; append exact UTF-8 value; no secondary unescaping'});
 }
 check(cases.length===20&&new Set(cases.map(c=>c.identity)).size===20,'twenty identities must be unique');
 return {raw,values,cases,modules,sha256:sha(raw)};
}
const limits=[
 'Full preserved production files unchanged before exact appended test modules; no extracted convenient functions or stubbed implementation.',
 'Experimental isolation manifest differs from historical default build; dependency differences and hashes are retained.',
 'No original legacy Cargo.lock was preserved; generated offline lock is not a recovered historical lock.',
 'Current-contract relationships are category evidence, not rerun or one-to-one closure.',
 'Historical single-forward zero-cycle oracle is semantically superseded by independently solved reverse flow.',
 'No model/runtime inference, private media, GPU, device actuation, or B1 completion is claimed.'
];
function prepare(){
 fs.mkdirSync(evidence,{recursive:true});
 const id='run-'+crypto.randomUUID(),dir=path.join(evidence,id),work=path.join(evidence,'.scratch',id);
 fs.mkdirSync(dir,{mode:0o700});fs.mkdirSync(work,{recursive:true,mode:0o700});
 const b=baseline(),a=archive();
 fs.cpSync(path.join(legacy,'src'),path.join(work,'src'),{recursive:true,errorOnExist:true,force:false});
 const main=read(path.join(legacy,'main.rs')),mainPath=path.join(work,'src/main.rs');
 if(fs.existsSync(mainPath))check(sha(read(mainPath))===sha(main),'two composition roots disagree');
 else fs.writeFileSync(mainPath,main,{flag:'wx'});
 for(const m of a.modules){
  const original=read(path.join(legacy,m.file));
  check(!original.includes(Buffer.from('mod m1_regression_tests')),'original already contains archived test module');
  fs.writeFileSync(path.join(work,m.file),Buffer.concat([original,Buffer.from(a.values[m.file])]));
 }
 const original=read(path.join(legacy,'Cargo.toml'));
 let manifest=original.toString('utf8');
 check((manifest.match(/^name = "pulsar"$/gm)||[]).length===2,'unexpected package/bin identity');
 manifest=manifest.replace(/^name = "pulsar"$/gm,'name = "pulsar-historical20"').replace(/^default-run = "pulsar"$/m,'default-run = "pulsar-historical20"');
 const ort='ort = { version = "2.0.0-rc.13", features = ["ndarray", "download-binaries"] }';
 check(manifest.includes(ort),'original ORT declaration changed');
 manifest=manifest.replace(ort,'ort = { version = "2.0.0-rc.13", default-features = false, features = ["std", "ndarray", "load-dynamic"] }');
 manifest+='\n# Isolated historical experiment, not a production workspace member.\n[workspace]\n';
 fs.writeFileSync(path.join(work,'Cargo.toml'),manifest,{flag:'wx'});
 fs.writeFileSync(path.join(dir,'original-Cargo.toml'),original,{flag:'wx'});
 fs.writeFileSync(path.join(dir,'execution-Cargo.toml'),manifest,{flag:'wx'});
 fs.writeFileSync(path.join(dir,'original-test-modules.json'),a.raw,{flag:'wx'});
 const subject={format_version:1,run_id:id,prepared_at_machine_utc:new Date().toISOString(),workspace_relative:path.relative(root,work),fixture_sha256:a.sha256,baseline:b,appended_modules:a.modules,cases:a.cases,prepared_files:tree(work,'',['Cargo.lock','target']),manifest_delta:[
  'Package/bin/default-run renamed to pulsar-historical20 to avoid overwriting current Pulsar.',
  'Preserved root main.rs restored byte-identically at src/main.rs.',
  'ORT same requested version; default-features=false with std,ndarray,load-dynamic instead of binary download.',
  'Independent empty workspace declaration.',
  'Original legacy Cargo.lock absent; experimental offline resolution recorded if generated.'
 ],qualification_limits:limits};
 json(path.join(dir,'subject.json'),subject);
 console.log(JSON.stringify({run_directory:path.relative(root,dir),workspace:subject.workspace_relative,baseline_sha256:b.sha256,fixture_sha256:a.sha256,cases:a.cases},null,2));
}
function run(arg){
 check(arg,'--run needs prepared directory');
 const dir=path.resolve(root,arg);
 check(path.dirname(dir)===evidence&&path.basename(dir).startsWith('run-'),'not an owned campaign directory');
 const subject=JSON.parse(read(path.join(dir,'subject.json'))),work=path.resolve(root,subject.workspace_relative);
 check(baseline().sha256===subject.baseline.sha256,'preserved baseline changed');
 check(sha(read(fixture))===subject.fixture_sha256,'original fixture changed');
 check(JSON.stringify(tree(work,'',['Cargo.lock','target']))===JSON.stringify(subject.prepared_files),'prepared source changed');
 const env={...process.env,CARGO_NET_OFFLINE:'true',CARGO_INCREMENTAL:'0',CARGO_BUILD_JOBS:'2',ORT_SKIP_DOWNLOAD:'1',CUDA_VISIBLE_DEVICES:'',HIP_VISIBLE_DEVICES:'',ROCR_VISIBLE_DEVICES:''};
 check(env.CARGO_TARGET_DIR,'explicit target directory required');
 const args=['test','--manifest-path',path.join(work,'Cargo.toml'),'--bin','pulsar-historical20','--no-default-features','--offline','m1_regression_tests','--','--nocapture','--test-threads=1'];
 const start=performance.now(),started=new Date().toISOString();
 const result=spawnSync('cargo',args,{cwd:root,env,encoding:'utf8',maxBuffer:16*1024*1024,timeout:600000});
 const stdout=result.stdout||'',stderr=result.stderr||'';
 fs.writeFileSync(path.join(dir,'cargo.stdout.log'),stdout,{flag:'wx'});fs.writeFileSync(path.join(dir,'cargo.stderr.log'),stderr,{flag:'wx'});
 const observed=new Map([...stdout.matchAll(/^test (\S+) \.\.\. (ok|FAILED|ignored)\s*$/gm)].map(m=>[m[1],m[2]]));
 const cases=subject.cases.map(c=>({...c,execution:observed.get(c.identity)==='ok'?'passed_on_preserved_subject':observed.get(c.identity)==='FAILED'?'failed_on_preserved_subject':observed.get(c.identity)==='ignored'?'ignored_not_executed':'unavailable_build_or_harness',semantic_disposition:c.preserved_file==='src/neural/point_tracker.rs'&&/forward|backward|cycle/i.test(c.original_test_name)?'single_forward_cycle_oracle_not_current_independent_reverse_contract':'original_assertion_preserved; current-contract evidence separate'}));
 const count=status=>cases.filter(c=>c.execution===status).length;
 const counts={expected:20,passed:count('passed_on_preserved_subject'),failed:count('failed_on_preserved_subject'),unavailable:count('unavailable_build_or_harness'),ignored:count('ignored_not_executed')};counts.observed=counts.passed+counts.failed;
 const summary={format_version:1,run_id:subject.run_id,started_at_machine_utc:started,elapsed_seconds:(performance.now()-start)/1000,command:{program:'cargo',args},environment:{CARGO_TARGET_DIR:env.CARGO_TARGET_DIR,CARGO_INCREMENTAL:'0',CARGO_BUILD_JOBS:'2',CARGO_NET_OFFLINE:'true',ORT_SKIP_DOWNLOAD:'1',gpu_visibility:'empty'},cargo_exit_code:result.status,signal:result.signal,spawn_error:result.error?.message??null,campaign_state:counts.observed===20?'all_original_tests_executed':'blocked_before_all_original_tests_executed',counts,baseline_unchanged:baseline().sha256===subject.baseline.sha256,fixture_unchanged:sha(read(fixture))===subject.fixture_sha256,cases,qualification_limits:limits};
 const lock=path.join(work,'Cargo.lock');if(fs.existsSync(lock)){const bytes=read(lock);fs.writeFileSync(path.join(dir,'resolved-Cargo.lock'),bytes,{flag:'wx'});summary.resolved_lock={sha256:sha(bytes),byte_len:bytes.length,historical_lock:false};}
 json(path.join(dir,'results.json'),summary);
 const lines=['# Historical twenty-regression execution receipt','','Isolated historical experiment, not B1 completion or production qualification.','',
  '- State: '+summary.campaign_state,'- Cargo exit: '+summary.cargo_exit_code,
  '- Executed: '+counts.observed+'/20; passed: '+counts.passed+'; failed: '+counts.failed+'; unavailable: '+counts.unavailable+'.',
  '- Preserved baseline unchanged: '+summary.baseline_unchanged+'; original fixture unchanged: '+summary.fixture_unchanged+'.','',
  '| Original test identity | Actual result | Related current-contract evidence |','| --- | --- | --- |',
  ...cases.map(c=>'| '+c.identity+' | '+c.execution+' | '+c.current_contract_related_coverage.limit+' |'),'',
  'Exact subject/test hashes and manifest deltas: subject.json. Process outputs: cargo.stdout.log and cargo.stderr.log.',
  'An original failing assertion is recorded, not repaired or relabelled as a passing suite.',
  'Current tests named in results.json are related category evidence and were not rerun.',
  '',...limits.map(s=>'- '+s),''];
 fs.writeFileSync(path.join(dir,'REPORT.md'),lines.join('\n'),{flag:'wx'});
 console.log(JSON.stringify(summary,null,2));
 // Zero means all20 observed, not that Cargo or the legacy suite passed.
 process.exitCode=counts.observed===20&&summary.baseline_unchanged&&summary.fixture_unchanged?0:2;
}
try{if(process.argv[2]==='--prepare')prepare();else if(process.argv[2]==='--run')run(process.argv[3]);else throw Error('usage: --prepare | --run prepared-directory');}
catch(error){console.error(error.stack||error);process.exitCode=2;}
