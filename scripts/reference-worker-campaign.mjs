// Frozen development candidate through the confined production benchmark worker.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import {pathToFileURL} from 'node:url';
import {snapshotRunner,runAttempt} from './benchmark-process.mjs';
import {evaluateClip,validateAnnotation} from './benchmark-good.mjs';

const sha=bytes=>crypto.createHash('sha256').update(bytes).digest('hex');
function read(filename,maximum=8*1024*1024) {
  const fd=fs.openSync(filename,fs.constants.O_RDONLY|fs.constants.O_NOFOLLOW|fs.constants.O_NONBLOCK);
  try {const stat=fs.fstatSync(fd);if(!stat.isFile()||stat.size>maximum)throw new Error('artifact type/size admission');
    const b=Buffer.alloc(maximum+1);let n=0,k;while(n<b.length&&(k=fs.readSync(fd,b,n,b.length-n,null)))n+=k;
    if(n>maximum)throw new Error('artifact grew beyond bound');const bytes=b.subarray(0,n);return {value:JSON.parse(bytes),sha256:sha(bytes)};
  } finally {fs.closeSync(fd);}
}
function write(filename,value) {const fd=fs.openSync(filename,'wx',0o600);try{fs.writeFileSync(fd,JSON.stringify(value,null,2)+'\n');fs.fsyncSync(fd);}finally{fs.closeSync(fd);}}

export function admitCampaign(root,manifest) {
  if(manifest.schema_version!==1||manifest.qualification!=='unqualified'||manifest.training_authorized!==true
      ||manifest.sterile_inputs_used!==false||manifest.model_retrained!==false||!Array.isArray(manifest.cases)
      ||manifest.cases.length<1||manifest.cases.length>32||!/^[a-f0-9]{64}$/.test(manifest.model_sha256))throw new Error('development scope admission');
  const seen=new Set();
  const resolve=relative=>{
    if(typeof relative!=='string'||path.isAbsolute(relative)||relative.split('/').some(v=>!v||v==='.'||v==='..'))throw new Error('contained case path required');
    const actual=fs.realpathSync(path.join(root,relative)),rel=path.relative(root,actual);if(rel.startsWith('../')||path.isAbsolute(rel))throw new Error('case escapes private root');return actual;
  };
  return manifest.cases.map(c=>{
    if(typeof c.id!=='string'||!/^development-clip-[0-9]{2}$/.test(c.id)||seen.has(c.id)||!Number.isInteger(c.expected_runs)||c.expected_runs<1||c.expected_runs>256)throw new Error('case identity/count admission');seen.add(c.id);
    const inputPath=resolve(c.input),input=read(inputPath),reference=read(resolve(c.reference));
    if(input.sha256!==c.input_sha256||reference.sha256!==c.reference_sha256)throw new Error('case bytes changed');
    validateAnnotation(reference.value,'reference');
    if(input.value.subject!==c.id||reference.value.subject_id!==c.id||input.value.model?.identity.sha256!==manifest.model_sha256
        ||input.value.window_start_ns!==reference.value.window.start_ns||input.value.window_end_ns!==reference.value.window.end_ns)throw new Error('case subject/model/window binding');
    return {declaration:c,inputPath,reference:reference.value};
  });
}

export async function runCampaign(root) {
  root=fs.realpathSync(root);const stat=fs.statSync(root);
  if(!stat.isDirectory()||(stat.mode&0o077)!==0||stat.uid!==process.getuid())throw new Error('private current-user evidence root required');
  const manifest=read(path.join(root,'campaign.json'));const cases=admitCampaign(root,manifest.value);
  const ledger=path.join(root,'execution-intent.json');write(ledger,{manifest_sha256:manifest.sha256,cases:cases.map(c=>c.declaration.id),qualification:'unqualified'});
  let runner;const results=[];
  try {
    runner=snapshotRunner(manifest.value.runner.path,manifest.value.runner.identity.sha256);
    for(const c of cases) {
      const directory=path.dirname(c.inputPath),output=path.join(directory,'worker-output');
      const raw=await runAttempt(runner.path,[c.inputPath,output],{timeout:150000,environment:{...process.env},logPrefix:path.join(directory,'runner')});
      write(path.join(directory,'process.json'),raw);
      let result={id:c.declaration.id,declared_group:c.declaration.declared_group,state:'failed',process_status:raw.status,score:null};
      if(raw.status===0&&!raw.error) {
        const candidate=read(path.join(output,'candidate.json')),execution=read(path.join(output,'execution.json'));
        if(!execution.value.result?.Ok||execution.value.release_qualified!==false)throw new Error('successful process lacks unqualified worker result');
        const program=execution.value.result.Ok;
        if(program.kind!=='generated')throw new Error('worker did not generate motion');
        const receipt=read(program.value.receipt.path);
        if(receipt.sha256!==program.value.receipt.identity.sha256||receipt.value.source_version!==c.declaration.id
            ||execution.value.worker.sha256!==manifest.value.artifacts.worker.sha256)throw new Error('worker receipt/executable binding');
        const runs=receipt.value.inference_execution?.successful_runs;
        if(!Number.isInteger(runs)||runs<0||runs>c.declaration.expected_runs)throw new Error('worker run accounting differs');
        result={...result,state:'scored',score:evaluateClip(c.reference,candidate.value.candidate),
          candidate_sha256:candidate.sha256,execution_sha256:execution.sha256,receipt_sha256:receipt.sha256,
          conversion_receipt:candidate.value.receipt,
          worker_runs:runs,staged_pairs:c.declaration.expected_runs,worker_peak_memory_bytes:execution.value.worker_peak_memory_bytes,
          inference_execution:receipt.value.inference_execution,
          elapsed_ns:execution.value.elapsed_ns,skipped_gap_edges:candidate.value.receipt.skipped_gap_edges,
          review:receipt.value.review,source_timebase:receipt.value.source_time_base,source_origin_pts:receipt.value.source_origin_pts};
      }
      results.push(result);write(path.join(directory,'result.json'),result);
      process.stdout.write(JSON.stringify({id:result.id,state:result.state,stroke:result.score?.axes?.stroke??null})+'\n');
    }
    const report={schema_version:1,state:results.every(r=>r.state==='scored')?'measured':'incomplete',qualification:'unqualified',
      scope:manifest.value.scope,manifest_sha256:manifest.sha256,model_sha256:manifest.value.model_sha256,results,
      sterile_inputs_used:false,model_retrained:false,production_activation:false,limitations:manifest.value.limitations};
    write(path.join(root,'report.json'),report);return report;
  } catch(error) {write(path.join(root,'campaign-failure.json'),{state:'failed',message:String(error).slice(0,2048),completed_cases:results.length,qualification:'unqualified'});throw error;
  } finally {runner?.dispose();}
}

if(process.argv[1]&&import.meta.url===pathToFileURL(process.argv[1]).href) {
  if(process.argv.length!==3)throw new Error('usage: reference-worker-campaign.mjs PRIVATE_PREPARED_RUN');
  const report=await runCampaign(process.argv[2]);if(report.state!=='measured')process.exitCode=1;
}
