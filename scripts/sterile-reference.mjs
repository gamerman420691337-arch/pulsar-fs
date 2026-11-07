// Supplied sterile reference intake. No training, model selection or alignment
// adjustment occurs here; reference knots are independent of generated output.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import {validateAnnotation,validateCorpus} from './benchmark-good.mjs';
const sha=bytes=>crypto.createHash('sha256').update(bytes).digest('hex');

export function suppliedReference(jsonBytes,csvBytes,{durationNs,subject='sterile-handy-reference',target='sterile-hand'}) {
  if (!Buffer.isBuffer(jsonBytes) || !Buffer.isBuffer(csvBytes) || jsonBytes.length>1024*1024 || csvBytes.length>1024*1024)
    throw new Error('bounded reference bytes required');
  if (typeof durationNs!=='string' || !/^[1-9][0-9]*$/.test(durationNs) || BigInt(durationNs)>9223372036854775807n)
    throw new Error('declared video duration in canonical nanoseconds required');
  const raw=JSON.parse(jsonBytes.toString('utf8'));
  if (!raw || !Array.isArray(raw.actions) || raw.actions.length<2 || raw.actions.length>5000) throw new Error('bounded reference actions required');
  let prior=-1;
  for (const action of raw.actions) {
    if (!Number.isSafeInteger(action.at) || action.at<=prior || action.at<0 || !Number.isInteger(action.pos) || action.pos<0 || action.pos>100)
      throw new Error('invalid supplied reference action');
    prior=action.at;
  }
  const csv=csvBytes.toString('utf8').split(/\r?\n/).filter(line=>line.trim() && !line.startsWith('#')).map(line=> {
    if (!/^(0|[1-9][0-9]*),(0|[1-9][0-9]*)$/.test(line)) throw new Error('invalid Handy CSV action');
    const [at,pos]=line.split(',').map(Number);return {at,pos};
  });
  if (JSON.stringify(raw.actions.map(({at,pos})=>({at,pos})))!==JSON.stringify(csv)) throw new Error('JSON and CSV references differ');
  const duration=BigInt(durationNs);
  const selected=raw.actions.filter(action=>BigInt(action.at)*1000000n<=duration);
  if (selected.length<2) throw new Error('video contains fewer than two complete reference anchors');
  // Exclude an incomplete terminal stroke rather than retiming the reference,
  // inventing an endpoint, or claiming visual correspondence was established.
  const endNs=(BigInt(selected.at(-1).at)*1000000n).toString();
  const reference={schema_version:1,subject_id:subject,window:{start_ns:'0',end_ns:endNs},strokes:[],material_sections:[]};
  for (let i=1;i<selected.length;i++) {
    const a=selected[i-1],b=selected[i];if (a.pos===b.pos) continue;
    reference.strokes.push({id:`supplied-edge-${i}`,target_id:target,axis:'stroke',
      start_ns:(BigInt(a.at)*1000000n).toString(),end_ns:(BigInt(b.at)*1000000n).toString(),
      start_position_ppm:a.pos*10000,end_position_ppm:b.pos*10000});
  }
  validateAnnotation(reference,'reference');
  return {reference,receipt:{schema_version:1,json_sha256:sha(jsonBytes),csv_sha256:sha(csvBytes),
    declared_video_duration_ns:durationNs,reference_last_ns:(BigInt(raw.actions.at(-1).at)*1000000n).toString(),
    evaluation_window:reference.window,excluded_terminal_anchors:raw.actions.length-selected.length,
    alignment_adjusted:false,visual_alignment_verified:false,reference_author_verified:false,
    training_permitted:false,release_qualified:false}};
}

export function prepareSterileReference({inputRoot,outputRoot,durationNs,authorizationRecord}) {
  if (typeof authorizationRecord!=='string' || !authorizationRecord || authorizationRecord.length>1024)
    throw new Error('explicit local evaluation authorization record required');
  const root=fs.realpathSync(inputRoot);
  if (path.basename(root)!=='Sterile-Reference') throw new Error('only the Sterile-Reference folder is an evaluation input; Live-Reference is training-only');
  const read=name=> {
    const fd=fs.openSync(path.join(root,name),fs.constants.O_RDONLY|fs.constants.O_NOFOLLOW|fs.constants.O_NONBLOCK);
    try {
      const limit=8*1024*1024;const size=fs.fstatSync(fd);
      if (!size.isFile() || size.size>limit) throw new Error('sterile reference file exceeds bound');
      const bytes=Buffer.alloc(size.size+1);let used=0;
      while (used<bytes.length) {const n=fs.readSync(fd,bytes,used,bytes.length-used,null);if (!n) break;used+=n;}
      if (used!==size.size) throw new Error('sterile reference file changed during read');
      return bytes.subarray(0,used);
    }
    finally {fs.closeSync(fd);}
  };
  const video=read('handonly_sync_animation.mp4');
  const funscript=read('353079142771.funscript');
  const json=read('353079142771.json');
  if (!funscript.equals(json)) throw new Error('supplied JSON and funscript bytes differ');
  const converted=suppliedReference(json,read('353079142771.csv'),{durationNs});
  fs.mkdirSync(outputRoot,{recursive:true,mode:0o700});
  const outputStat=fs.statSync(outputRoot);
  if (!outputStat.isDirectory() || (outputStat.mode&0o077)!==0 || outputStat.uid!==process.getuid()) throw new Error('evidence root must be private to the current user');
  const run=fs.mkdtempSync(path.join(fs.realpathSync(outputRoot),'sterile-reference-'));fs.chmodSync(run,0o700);
  const write=(name,bytes)=> {
    const fd=fs.openSync(path.join(run,name),'wx',0o600);
    try {fs.writeFileSync(fd,bytes);fs.fsyncSync(fd);fs.fchmodSync(fd,0o400);} finally {fs.closeSync(fd);}
  };
  const referenceBytes=Buffer.from(JSON.stringify(converted.reference,null,2)+'\n');
  const corpus={schema_version:1,policy_version:'B1.1',entries:[{
    id:'sterile-handy-reference',category:'supplied-sterile-animation',split:'development',source_group_id:'sterile-handy-reference',
    evidence_class:'synthetic',source_sha256:sha(video),
    rights:{status:'permitted',evidence_id:authorizationRecord,permitted_uses:['evaluation'],redistribution:'forbidden'},
    annotation_provenance:{reference_author:'supplied Handy SDK reference; authorship unverified',independent:true,
      method:'supplied non-plateau edges; JSON/CSV correspondence checked; no retiming',review_record:'visual/timing alignment unqualified; partial terminal stroke excluded'},
    reference:{path:'reference.json',sha256:sha(referenceBytes)},candidate:{path:'unused-candidate.json',sha256:'0'.repeat(64)}
  }]};
  validateCorpus(corpus);
  write('sterile.mp4',video);write('reference.json',referenceBytes);
  write('corpus.json',JSON.stringify(corpus,null,2)+'\n');
  write('sources.json',JSON.stringify({schema_version:1,entries:[{entry_id:'sterile-handy-reference',path:'sterile.mp4'}]},null,2)+'\n');
  write('reference-receipt.json',JSON.stringify({...converted.receipt,source_sha256:sha(video),authorization_record:authorizationRecord,
    sterile_use:'evaluation-only',live_reference_use:'training-only'},null,2)+'\n');
  const dir=fs.openSync(run,fs.constants.O_RDONLY);try {fs.fsyncSync(dir);} finally {fs.closeSync(dir);}
  return {directory:run,corpus:path.join(run,'corpus.json'),sources:path.join(run,'sources.json'),target:'sterile-hand',receipt:converted.receipt};
}
