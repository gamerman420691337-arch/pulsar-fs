// Authored media through the shipped engine/CLI; no real-media qualification.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import net from 'node:net';
import {spawn, spawnSync} from 'node:child_process';
import {setTimeout as delay} from 'node:timers/promises';

for (const rate of [null,{numerator:1,denominator:2}]) test(`independent audio uses captured bytes and ${rate?'non-unit':'unit'} clocks through native generation and portable clones`,
  {skip:!process.env.PULSAR_STASH_TEST_BINARY,timeout:90000},async t=>{
    const root=fs.mkdtempSync(path.join(os.tmpdir(),'pulsar-independent-audio-'));fs.chmodSync(root,0o700);
    t.diagnostic(`retained authored independent audio evidence: ${root}`);
    const binary=fs.realpathSync(process.env.PULSAR_STASH_TEST_BINARY),state=path.join(root,'state');
    const env={...process.env,PULSAR_STATE_DIR:state,PULSAR_ORT_DYLIB:process.env.PULSAR_BENCHMARK_ORT};
    let engine,exit,stderr='',sequence=0;
    const start=()=>{engine=spawn(binary,['engine'],{env,stdio:['ignore','ignore','pipe']});engine.stderr.on('data',b=>{stderr=(stderr+b).slice(-65536);});exit=new Promise(resolve=>engine.once('exit',resolve));};
    const stop=async()=>{if(engine?.exitCode===null&&engine.signalCode===null){engine.kill('SIGTERM');const timer=setTimeout(()=>engine.kill('SIGKILL'),2000);try{await exit;}finally{clearTimeout(timer);}}};
    const cli=args=>{
      const r=spawnSync(binary,args,{env,timeout:30000,encoding:'utf8',maxBuffer:1024*1024});assert.equal(r.error,undefined);
      fs.writeFileSync(path.join(root,`cli-${sequence++}.json`),JSON.stringify({args,status:r.status,stdout:r.stdout,stderr:r.stderr},null,2),{mode:0o600});return r;
    };
    const ready=async()=>{const deadline=Date.now()+15000;while(true){assert.equal(engine.exitCode,null,stderr);assert.ok(Date.now()<deadline,'startup deadline');if(fs.existsSync(path.join(state,'engine.sock'))&&cli(['capabilities']).status===0)return;await delay(25);}};
    const success=args=>{const r=cli(args);assert.equal(r.status,0,r.stderr+r.stdout);return r;};
    const ok=args=>JSON.parse(success(args).stdout);
    const sha=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
    const rpc=(project,revision,operation,args)=>new Promise((resolve,reject)=>{
      const credentials=JSON.parse(fs.readFileSync(path.join(state,'first-party.session.json')));
      const bytes=Buffer.from(JSON.stringify({version:2,request_id:`independent-audio-${++sequence}`,...credentials,project,expected_revision:revision,command:{operation,arguments:args}}));
      const header=Buffer.alloc(4);header.writeUInt32BE(bytes.length);const socket=net.createConnection(path.join(state,'engine.sock'));let received=Buffer.alloc(0);
      socket.setTimeout(10000,()=>socket.destroy(new Error('IPC deadline')));socket.once('error',reject);socket.once('close',()=>reject(new Error('IPC closed before response')));
      socket.once('connect',()=>socket.write(Buffer.concat([header,bytes])));socket.on('data',b=>{received=Buffer.concat([received,b]);if(received.length<4)return;const size=received.readUInt32BE();if(size>1024*1024){socket.destroy(new Error('IPC bound'));return;}if(received.length>=size+4){resolve(JSON.parse(received.subarray(4,size+4)).result);socket.destroy();}});
    });
    try {
      const video=path.join(root,'video.y4m');const texture=Buffer.from(Array.from({length:4096},(_,i)=>(i%64*31+Math.floor(i/64)*17+Math.floor(i%64/7)*Math.floor(i/64/5)*13)%256));
      fs.writeFileSync(video,Buffer.concat([Buffer.from('YUV4MPEG2 W64 H64 F8:1 Ip A1:1 Cmono\n'),...Array.from({length:8},()=>Buffer.concat([Buffer.from('FRAME\n'),texture]))]));
      const pcm=path.join(root,'audio.f32'),bytes=Buffer.alloc(16000*4);for(let i=0;i<16000;i++)bytes.writeFloatLE(0.4,i*4);fs.writeFileSync(pcm,bytes);
      const audio=path.join(root,'audio.mka');const mux=spawnSync('/usr/bin/ffmpeg',['-v','error','-nostdin','-f','f32le','-ar','16000','-ac','1','-i',pcm,'-af','asetpts=PTS+0.125/TB','-c:a','pcm_f32le','-threads','1',audio],{timeout:15000,encoding:'utf8'});assert.equal(mux.status,0,mux.stderr);
      const audioHash=sha(audio),policy=path.join(root,'mixed.json'),alignmentPath=path.join(root,'alignment.json');
      const mapping={axis:'stroke',amplitude:0.2,offset:0.4};const alignment={video_anchor:{numerator:0,denominator:1},audio_anchor:{numerator:1,denominator:8},...(rate?{rate}:{})};
      fs.writeFileSync(policy,JSON.stringify({intent:'faithful',mapping}));fs.writeFileSync(alignmentPath,JSON.stringify(alignment));
      start();await ready();const baseline=path.join(root,'baseline.funscript');success(['generate',video,'--width','64','--height','64','--output',baseline]);
      const args=output=>['generate',video,'--width','64','--height','64','--output',output,'--mixed-audio',policy,'--audio-source',audio,'--audio-alignment',alignmentPath];
      const attempts=()=>fs.readdirSync(path.join(state,'attempts'));
      const before=attempts(),faithful=path.join(root,'faithful.funscript');success(args(faithful));assert.deepEqual(JSON.parse(fs.readFileSync(faithful)).actions,JSON.parse(fs.readFileSync(baseline)).actions);
      const attempt=attempts().find(x=>!before.includes(x));assert.ok(attempt);const dir=path.join(state,'attempts',attempt),checkpoint=JSON.parse(fs.readFileSync(path.join(dir,'initial-checkpoint.json'))),receipt=JSON.parse(fs.readFileSync(path.join(dir,'receipt.json')));
      assert.equal(checkpoint.independent_audio.identity.sha256,audioHash);assert.notEqual(checkpoint.independent_audio.path,audio);assert.equal(sha(checkpoint.independent_audio.path),audioHash);
      assert.deepEqual(checkpoint.operation.arguments.settings.mixed_audio.independent.alignment,alignment);
      assert.equal(receipt.mixed_audio.independent_source.identity.sha256,audioHash);assert.equal(receipt.samples[0].mixed_audio.audio_window_index,0);assert.equal(receipt.samples[1].mixed_audio.audio_window_index,rate?6:12);
      assert.deepEqual(receipt.samples[1].mixed_audio.aligned_audio_time,rate?{numerator:'3',denominator:'16'}:{numerator:'1',denominator:'4'});assert.ok(receipt.dependencies.some(d=>d.sha256===audioHash));
      for (const invalid of [{numerator:0,denominator:1},{numerator:-1,denominator:1},{numerator:1,denominator:0}]) {
        fs.writeFileSync(alignmentPath,JSON.stringify({...alignment,rate:invalid}));
        const retained=attempts();const refused=cli(args(path.join(root,'invalid-rate.funscript')));
        assert.notEqual(refused.status,0);assert.match(refused.stderr,/audio clock rate must be positive/);
        assert.deepEqual(attempts(),retained);assert.ok(!fs.existsSync(path.join(root,'invalid-rate.funscript')));
      }
      fs.writeFileSync(alignmentPath,JSON.stringify(alignment));
      const project=checkpoint.project_id,current=ok(['project','open',project]).value;
      const foreign=ok(['project','create','foreign audio owner']).value.project_id;
      const imported=await rpc(foreign,0,'import_source',{path:video});assert.ok(imported.Ok);
      const foreignState=ok(['project','open',foreign]).value;
      const wrong=await rpc(foreign,foreignState.revision,'generate',{source_version:imported.Ok.value.source_version,preset:'fast',settings:checkpoint.operation.arguments.settings,model_path:null});assert.equal(wrong.Err.code,'not_found');
      fs.writeFileSync(path.join(root,'foreign-audio-refusal.json'),JSON.stringify(wrong,null,2),{mode:0o600});
      const missing=cli(args(path.join(root,'missing.funscript')).slice(0,-2));assert.notEqual(missing.status,0);assert.match(missing.stderr,/audio-alignment/);
      fs.writeFileSync(policy,JSON.stringify({intent:'creative_audio',mapping}));const creative=path.join(root,'creative.funscript');success(args(creative));const actions=JSON.parse(fs.readFileSync(creative)).actions;assert.ok(actions.length>0);assert.ok(actions.every(a=>a.pos===48));assert.deepEqual(actions.map(a=>a.at),JSON.parse(fs.readFileSync(faithful)).actions.map(a=>a.at));
      fs.writeFileSync(alignmentPath,JSON.stringify({...alignment,audio_anchor:{numerator:0,denominator:1}}));const gapBefore=attempts();const gap=cli(args(path.join(root,'gap.funscript')));assert.notEqual(gap.status,0);assert.ok(!fs.existsSync(path.join(root,'gap.funscript')));
      const gapAttempt=attempts().find(x=>!gapBefore.includes(x));assert.ok(gapAttempt);const gapReceipt=JSON.parse(fs.readFileSync(path.join(state,'attempts',gapAttempt,'receipt.json')));assert.equal(gapReceipt.samples[0].mixed_audio.audio_window_index,null);assert.ok(gapReceipt.unresolved_gaps.length>0);
      // Remove only authored supplier fixtures. Captured private source bytes must remain usable.
      fs.unlinkSync(video);fs.unlinkSync(audio);assert.equal(sha(checkpoint.independent_audio.path),audioHash);
      const verifyArchivedAlignment=file=>{
        const bytes=fs.readFileSync(file);assert.ok(bytes.length<=32*1024*1024);
        assert.equal(bytes.subarray(0,8).toString('ascii'),'PULSPKG\0');
        const length=Number(bytes.readBigUInt64LE(12));assert.ok(length<=bytes.length-52);
        const manifestBytes=bytes.subarray(52,52+length);
        assert.equal(crypto.createHash('sha256').update(manifestBytes).digest('hex'),bytes.subarray(20,52).toString('hex'));
        const manifest=JSON.parse(manifestBytes);let offset=52+length,matching=0;
        for (const object of manifest.objects) {
          const body=bytes.subarray(offset,offset+object.byte_len);assert.equal(body.length,object.byte_len);
          assert.equal(crypto.createHash('sha256').update(body).digest('hex'),object.sha256);offset+=object.byte_len;
          if (object.roles.includes('worker_manifest')) {
            const retained=JSON.parse(body);
            if (retained.attempt_id===checkpoint.attempt_id) {
              assert.deepEqual(retained.operation.arguments.settings.mixed_audio.independent.alignment,alignment);
              assert.deepEqual(retained.independent_audio.identity,checkpoint.independent_audio.identity);matching++;
            }
          }
        }
        assert.equal(offset,bytes.length);assert.equal(matching,1);
      };
      success(['allow-packaging',project,'--acknowledge-unencrypted-private-data']);const packageFile=path.join(root,'independent.pulsar');success(['package-export',project,packageFile,'--acknowledge-unencrypted-private-data']);
      verifyArchivedAlignment(packageFile);const clone=ok(['package-import',packageFile]);assert.equal(clone.state,'completed');assert.deepEqual(ok(['project','open',clone.receipt.project_id]).value.program,current.program);
      success(['allow-packaging',clone.receipt.project_id,'--acknowledge-unencrypted-private-data']);const second=path.join(root,'clone.pulsar');success(['package-export',clone.receipt.project_id,second,'--acknowledge-unencrypted-private-data']);verifyArchivedAlignment(second);const cloneAgain=ok(['package-import',second]);assert.equal(cloneAgain.state,'completed');assert.deepEqual(ok(['project','open',cloneAgain.receipt.project_id]).value.program,current.program);
      await stop();start();await ready();assert.deepEqual(ok(['project','open',project]).value.program,current.program);assert.equal(sha(checkpoint.independent_audio.path),audioHash);
      fs.writeFileSync(path.join(root,'independent-audio.json'),JSON.stringify({state:'passed',qualification:'unqualified',captured_audio_identity:checkpoint.independent_audio.identity,alignment,faithful_actions_unchanged:true,creative_action_times_unchanged:true,missing_audio_support_gap_retained:true,foreign_project_refused:true,missing_alignment_refused:true,supplier_removal_preserved_snapshots:true,portable_clone_and_reexport:true,portable_original_alignment_bytes_verified:true,invalid_rates_refused_before_attempt:true,restart_preserved:true},null,2),{mode:0o600});
      fs.copyFileSync(path.join(dir,'receipt.json'),path.join(root,'faithful-receipt.json'));
    } finally {await stop();}
  });
