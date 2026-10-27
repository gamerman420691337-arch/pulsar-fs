import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import {spawn,execFileSync} from 'node:child_process';
import {setTimeout as delay} from 'node:timers/promises';

test('native doctor retains an uncommitted capture; scoped segment-speed queries preserve state across restart',
  {skip:!process.env.PULSAR_STASH_TEST_BINARY,timeout:60000},async t=>{
    const root=fs.mkdtempSync(path.join(os.tmpdir(),'pulsar-segment-speed-'));fs.chmodSync(root,0o700);
    t.diagnostic(`retained authored diagnostic fixture: ${root}`);
    const binary=fs.realpathSync(process.env.PULSAR_STASH_TEST_BINARY),env={PATH:'/usr/bin:/bin',PULSAR_STATE_DIR:path.join(root,'state')};
    let engine,exit,stderr='';
    const start=()=>{engine=spawn(binary,['engine'],{env,stdio:['ignore','ignore','pipe']});engine.stderr.on('data',b=>{stderr=(stderr+b).slice(-65536);});exit=new Promise(resolve=>engine.once('exit',resolve));};
    const cli=args=>new Promise((resolve,reject)=>{const p=spawn(binary,args,{env,stdio:['ignore','pipe','pipe']});let out='',err='';const timer=setTimeout(()=>p.kill('SIGKILL'),15000);p.stdout.on('data',b=>{out=(out+b).slice(-1024*1024);});p.stderr.on('data',b=>{err=(err+b).slice(-65536);});p.once('error',e=>{clearTimeout(timer);reject(e);});p.once('exit',code=>{clearTimeout(timer);resolve({code,out,err});});});
    const ready=async()=>{const deadline=Date.now()+15000;while(true){assert.equal(engine.exitCode,null,stderr);assert.ok(Date.now()<deadline,'engine startup deadline');if(fs.existsSync(path.join(env.PULSAR_STATE_DIR,'engine.sock'))&&(await cli(['capabilities'])).code===0)return;await delay(25);}};
    const stop=async()=>{if(engine?.exitCode===null && engine.signalCode===null){engine.kill('SIGTERM');const timer=setTimeout(()=>engine.kill('SIGKILL'),2000);try{await exit;}finally{clearTimeout(timer);}}};
    const ok=async args=>{const r=await cli(args);assert.equal(r.code,0,r.err+r.out);return JSON.parse(r.out);};
    const hash=file=>crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
    try {
      start();await ready();
      const file=path.join(root,'authored.funscript');fs.writeFileSync(file,JSON.stringify({actions:[{at:0,pos:0},{at:100,pos:100},{at:200,pos:100},{at:400,pos:10}]}));
      const projectCount=()=>Number(execFileSync('/usr/bin/python3',['-c','import sqlite3,sys; d=sqlite3.connect(sys.argv[1]); print(d.execute("SELECT COUNT(*) FROM projects").fetchone()[0])',path.join(env.PULSAR_STATE_DIR,'projects.sqlite3')],{env,encoding:'utf8',timeout:10000}));
      const original=hash(file),beforeCount=projectCount();
      for(const limit of ['0','-1','NaN','inf','100000000001']) {assert.notEqual((await cli(['doctor',file,`--max-speed=${limit}`])).code,0);}
      assert.equal(projectCount(),beforeCount);
      const report=(await ok(['doctor',file])).value;
      assert.ok(report.candidate_id);assert.equal(report.revision,0);assert.equal(report.base_revision,0);
      const scan=report.segment_speed;assert.equal(scan.maximum_percent_per_second,450);assert.equal(scan.scanned_segments,3);assert.equal(scan.unavailable_segments,0);assert.equal(scan.truncated,false);assert.equal(scan.violations.length,1);assert.equal(scan.violations[0].average_percent_per_second,1000);
      assert.deepEqual(scan.violations[0].range,{start:0,end:100000000});assert.equal(scan.violations[0].axis,'stroke');
      assert.ok(report.issues.some(i=>i.code==='imported_motion'));assert.ok(report.issues.some(i=>i.code==='segment_average_speed_exceeded'));
      const p=report.project_id,c=report.candidate_id,master=(await ok(['project','open',p])).value,review=await ok(['review-status',p]);
      assert.ok(master.motion.program.axes.every(axis=>axis.action_count===0));
      const query=limit=>['review',p,'--candidate',c,'--max-speed',String(limit)];
      const permissive=(await ok(query(1000))).value;assert.equal(permissive.segment_speed.violations.length,0);
      const lower=(await ok(query(449))).value;assert.equal(lower.segment_speed.violations.length,2);assert.equal(lower.segment_speed.violations[1].average_percent_per_second,450);
      assert.equal((await ok(['review',p,'--candidate',c])).value.segment_speed,undefined);
      for(let i=0;i<3;i++)assert.deepEqual((await ok(query(450))).value,report);
      assert.deepEqual((await ok(['project','open',p])).value,master);assert.deepEqual(await ok(['review-status',p]),review);assert.equal(hash(file),original);
      const overflowFile=path.join(root,'overflow.funscript');fs.writeFileSync(overflowFile,JSON.stringify({actions:Array.from({length:200},(_,i)=>({at:i,pos:i%2?100:0}))}));
      const overflow=(await ok(['doctor',overflowFile])).value.segment_speed;
      assert.equal(overflow.violations.length,128);assert.equal(overflow.truncated,true);assert.equal(overflow.scanned_segments,129);assert.equal(overflow.unavailable_segments,0);
      const retained={report,permissive,lower,overflow,source_sha256:original,master_revision:master.revision,master_empty:true};
      await stop();start();await ready();assert.deepEqual((await ok(query(450))).value,report);assert.deepEqual((await ok(['project','open',p])).value,master);assert.deepEqual(await ok(['review-status',p]),review);assert.equal(hash(file),original);
      if(process.env.PULSAR_SEGMENT_SPEED_EVIDENCE){fs.mkdirSync(process.env.PULSAR_SEGMENT_SPEED_EVIDENCE,{recursive:true});fs.writeFileSync(path.join(process.env.PULSAR_SEGMENT_SPEED_EVIDENCE,'authored-cli.json'),JSON.stringify(retained,null,2)+'\n',{flag:'wx'});}
    } finally {await stop();}
  });
