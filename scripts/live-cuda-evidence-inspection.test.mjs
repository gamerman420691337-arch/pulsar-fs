// Explicit retained authored run; no runtime/model paths are followed by inspection.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import net from 'node:net';
import os from 'node:os';
import {spawn,spawnSync} from 'node:child_process';
const reader=process.env.PULSAR_RETAINED_CUDA_READER;
const root=process.env.PULSAR_RETAINED_CUDA_RUN_ROOT;
const desktopController=process.env.PULSAR_RETAINED_CUDA_DESKTOP_CONTROLLER;
const listing=process.env.PULSAR_RETAINED_CUDA_LISTING==='1';
const downloads=process.env.PULSAR_RETAINED_CUDA_DOWNLOADS==='1';
const removal=process.env.PULSAR_RETAINED_CUDA_REMOVAL==='1';
const originals=process.env.PULSAR_RETAINED_CUDA_ORIGINAL_DIR;
test('CPU-only shipped CLI inspects retained CUDA evidence after restart with current revision scope',
  {skip:!reader||!root||process.platform!=='linux',timeout:30000},async t=>{
    const output=fs.mkdtempSync(path.join(os.tmpdir(),'pulsar-cuda-inspection-'));fs.chmodSync(output,0o700);
    t.diagnostic(`retained authored inspection evidence: ${output}`);
    const receipt=JSON.parse(fs.readFileSync(path.join(root,'cancellation-receipt.json'),'utf8'));
    const initial=receipt.fresh,expected=receipt.completed.inference.cuda_evidence;
    const state=path.join(root,'state');const env={...process.env,PULSAR_STATE_DIR:state};delete env.PULSAR_CUDA_OWNER_POLICY;
    const stderr=path.join(output,'inspection-engine.stderr');const fd=fs.openSync(stderr,'wx',0o600);
    const engine=spawn(reader,['engine'],{env,stdio:['ignore','ignore',fd]});fs.closeSync(fd);
    let result;const closed=new Promise(resolve=>{engine.once('exit',(code,signal)=>{result={code,signal};resolve(result);});engine.once('error',error=>{result={error:error.message};resolve(result);});});
    const query=(label,args,code=0)=>{
      const r=spawnSync(reader,args,{env,encoding:'utf8',timeout:10000,maxBuffer:1048576});
      fs.writeFileSync(path.join(output,`inspection-${label}.json`),JSON.stringify({args,status:r.status,error:r.error?.message,stdout:r.stdout,stderr:r.stderr},null,2)+'\n',{mode:0o600});
      assert.equal(r.error,undefined);if(code===0){assert.equal(r.status,0,r.stderr);return JSON.parse(r.stdout);}
      assert.notEqual(r.status,0);assert.match(r.stderr,code);return r;
    };
    try {
      const end=Date.now()+10000;
      for(;;){
        assert.equal(result,undefined,fs.readFileSync(stderr,'utf8'));
        const ready=await new Promise(resolve=>{const s=net.createConnection(path.join(state,'engine.sock'));let done=false;const finish=value=>{if(done)return;done=true;s.destroy();resolve(value);};s.once('connect',()=>finish(true));s.once('error',()=>finish(false));s.setTimeout(100,()=>finish(false));});
        if(ready)break;assert.ok(Date.now()<end,'inspection Engine did not start');await new Promise(resolve=>setTimeout(resolve,10));
      }
      const args=['live-capture','evidence',initial.identity.project,'0',initial.identity.epoch];
      const response=query('retained',args);assert.equal(response.kind,'retained_live_cuda_evidence');
      const value=response.value;assert.deepEqual(value.identity,initial.identity);assert.equal(value.engine_epoch,initial.engine_epoch);assert.deepEqual(value.evidence,expected);assert.equal(value.evidence.qualified,false);
      query('cancelled',['live-capture','evidence',initial.identity.project,'0',receipt.initial.identity.epoch],/no retained CUDA evidence/);
      query('stale',['live-capture','evidence',initial.identity.project,'1',initial.identity.epoch],/expected revision is stale/);
      const other=query('foreign-project',['project','create','authored-inspection-foreign']).value.project_id;
      query('foreign',['live-capture','evidence',other,'0',initial.identity.epoch],/no retained CUDA evidence/);
      if(listing) {
        const first=query('listing',['live-capture','list-evidence',initial.identity.project,'0','--limit','1']);
        assert.equal(first.kind,'live_cuda_evidence_page');assert.deepEqual(first.value.entries,[initial.identity]);assert.equal(first.value.next_after,null);
        const empty=query('listing-after',['live-capture','list-evidence',initial.identity.project,'0','--after',initial.identity.epoch]);assert.deepEqual(empty.value.entries,[]);
        const foreign=query('listing-foreign',['live-capture','list-evidence',other,'0']);assert.deepEqual(foreign.value.entries,[]);
        query('listing-stale',['live-capture','list-evidence',initial.identity.project,'1'],/expected revision is stale/);
        query('listing-bound',['live-capture','list-evidence',initial.identity.project,'0','--limit','17'],/invalid value/);
      }
      if(removal) {
        const hash=value.evidence.manifest.sha256;assert.notEqual(hash,'0'.repeat(64));
        const base=['live-capture','remove-evidence',initial.identity.project,'0',initial.identity.epoch];
        query('removal-wrong-manifest',[...base,'0'.repeat(64),'--request-id',`wrong-manifest-${Date.now()}`,'--confirm-removal'],/manifest changed or differs/);
        query('removal-absent',['live-capture','remove-evidence',initial.identity.project,'0','absent-removal-capture','0'.repeat(64),'--request-id',`absent-${Date.now()}`,'--confirm-removal'],/no retained CUDA evidence/);
        query('removal-stale',['live-capture','remove-evidence',initial.identity.project,'1',initial.identity.epoch,hash,'--request-id',`stale-${Date.now()}`,'--confirm-removal'],/expected revision is stale/);
        query('removal-unconfirmed',[...base,hash,'--request-id',`unconfirmed-${Date.now()}`],/--confirm-removal/);
      }
      const downloaded=[];
      if(downloads) {
        assert.ok(originals,'download qualification requires explicit original-byte directory');
        for(const part of ['origin','manifest','summary','receipt','trace']) {
          const destination=path.join(output,`${part}.json`);
          const exported=query(`download-${part}`,['live-capture','download-evidence',initial.identity.project,'0',initial.identity.epoch,part,destination]);
          assert.deepEqual(fs.readFileSync(destination),fs.readFileSync(path.join(originals,`${part}.json`)));
          assert.equal(exported.part,part);assert.equal(exported.epoch,initial.identity.epoch);assert.equal(exported.identity.byte_len,fs.statSync(destination).size);downloaded.push(part);
        }
        const occupied=path.join(output,'occupied.json');fs.writeFileSync(occupied,'original canary',{mode:0o600});
        query('no-clobber',['live-capture','download-evidence',initial.identity.project,'0',initial.identity.epoch,'trace',occupied],/already exists/);
        assert.equal(fs.readFileSync(occupied,'utf8'),'original canary');
        const absent=path.join(output,'cancelled.json');
        query('download-cancelled',['live-capture','download-evidence',initial.identity.project,'0',receipt.initial.identity.epoch,'trace',absent],/no retained CUDA evidence/);
        assert.equal(fs.existsSync(absent),false);
        const stale=path.join(output,'stale.json');
        query('download-stale',['live-capture','download-evidence',initial.identity.project,'1',initial.identity.epoch,'trace',stale],/expected revision is stale/);
        assert.equal(fs.existsSync(stale),false);
        assert.equal(fs.readdirSync(output).filter(name=>name.startsWith('.pulsar-package-')).length,0);
      }
      if(desktopController) {
        assert.ok(originals,'desktop controller requires explicit original-byte directory');
        const controllerEnv={...env,PULSAR_EVIDENCE_PANEL_REMOVAL_REFUSALS:removal?'1':'0',PULSAR_EVIDENCE_PANEL_ENDPOINT:path.join(state,'engine.sock'),PULSAR_EVIDENCE_PANEL_BOOTSTRAP:path.join(state,'pairing.token'),PULSAR_EVIDENCE_PANEL_PROJECT:initial.identity.project,PULSAR_EVIDENCE_PANEL_REVISION:'0',PULSAR_EVIDENCE_PANEL_EPOCH:initial.identity.epoch,PULSAR_EVIDENCE_PANEL_OUTPUT:path.join(output,'desktop-originals'),PULSAR_EVIDENCE_PANEL_ORIGINALS:originals};
        const r=spawnSync(desktopController,['evidence_panel::tests::native_desktop_worker_browses_inspects_and_saves_originals','--exact','--ignored','--test-threads=1'],{env:controllerEnv,encoding:'utf8',timeout:20000,maxBuffer:1048576});
        fs.writeFileSync(path.join(output,'desktop-controller.json'),JSON.stringify({status:r.status,error:r.error?.message,stdout:r.stdout,stderr:r.stderr},null,2)+'\n',{mode:0o600});
        assert.equal(r.error,undefined);assert.equal(r.status,0,r.stderr+r.stdout);assert.match(r.stdout,/1 passed; 0 failed/);
        for(const part of ['origin','manifest','summary','receipt','trace'])assert.deepEqual(fs.readFileSync(path.join(output,'desktop-originals',`${part}.json`)),fs.readFileSync(path.join(originals,`${part}.json`)));
      }
      const again=query('retained-again',args);assert.deepEqual(again,response);
      fs.writeFileSync(path.join(output,'inspection-receipt.json'),JSON.stringify({result:'PASS',response,downloaded_original_parts:downloaded,listing_checked:listing,removal_refusals_checked:removal,desktop_controller_checked:!!desktopController,after_restart:true,cpu_only_reader:true,owner_policy_supplied:false,cancelled_refused:true,stale_revision_refused:true,foreign_project_refused:true,qualified:false},null,2)+'\n',{mode:0o600});
    } finally {
      if(!result)engine.kill('SIGKILL');await closed;
    }
  });
