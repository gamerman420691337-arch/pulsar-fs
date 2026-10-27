import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {spawn} from 'node:child_process';
import {setTimeout as delay} from 'node:timers/promises';

test('native engine boot fences device metadata and preserves prepared declarations without physical replay',
  {skip:!process.env.PULSAR_STASH_TEST_BINARY,timeout:45000},async t=>{
    const binary=fs.realpathSync(process.env.PULSAR_STASH_TEST_BINARY);
    const root=fs.mkdtempSync(path.join(os.tmpdir(),'pulsar-device-journal-'));fs.chmodSync(root,0o700);
    t.diagnostic(`retained synthetic device metadata fixture: ${root}`);
    const env={PATH:'/usr/bin:/bin',PULSAR_STATE_DIR:path.join(root,'state')};
    const run=(executable,args)=>new Promise((resolve,reject)=>{
      const p=spawn(executable,args,{env,stdio:['ignore','pipe','pipe']});let out='',err='';const timer=setTimeout(()=>p.kill('SIGKILL'),10000);
      p.stdout.on('data',b=>{out=(out+b).slice(-65536);});p.stderr.on('data',b=>{err=(err+b).slice(-65536);});
      p.once('error',e=>{clearTimeout(timer);reject(e);});p.once('exit',code=>{clearTimeout(timer);resolve({code,out,err});});
    });
    let engine,exit,stderr='';
    const start=()=>{stderr='';engine=spawn(binary,['engine'],{env,stdio:['ignore','ignore','pipe']});engine.stderr.on('data',b=>{stderr=(stderr+b).slice(-65536);});exit=new Promise(resolve=>engine.once('exit',resolve));};
    const stop=async()=>{if(engine?.exitCode===null && engine.signalCode===null){engine.kill('SIGTERM');const timer=setTimeout(()=>engine.kill('SIGKILL'),2000);try{await exit;}finally{clearTimeout(timer);}}};
    const ready=async()=>{const deadline=Date.now()+15000;while(true){assert.equal(engine.exitCode,null,stderr);assert.ok(Date.now()<deadline,'engine readiness');if(fs.existsSync(path.join(env.PULSAR_STATE_DIR,'engine.sock'))){const r=await run(binary,['capabilities']);if(r.code===0){assert.equal(JSON.parse(r.out).value.physical_playback,false);return;}}await delay(25);}};
    const journal=path.join(env.PULSAR_STATE_DIR,'device-effects/journal.sqlite3');
    const python=async lines=>{const r=await run('/usr/bin/python3',['-c',lines.join('\n'),journal]);assert.equal(r.code,0,r.err);return JSON.parse(r.out);};
    try {
      start();await ready();assert.equal(fs.statSync(journal).mode&0o777,0o600);
      const before=await python([
        'import sqlite3,json,hashlib,sys',
        "d=sqlite3.connect(sys.argv[1]);boot=d.execute('SELECT boot FROM owner WHERE singleton=1').fetchone()[0]",
        "v={'boot':boot,'device_session':'synthetic-device-epoch','sequence':1,'admission':{'project':'synthetic-project','revision':0,'configuration':{'device':'synthetic-device','firmware':'synthetic-firmware','driver':'synthetic-driver','transport':'synthetic-transport','profile':'synthetic-profile'},'program_artifact':'synthetic-program'},'controller':'synthetic-controller','qualification_record':'synthetic-declaration-only','stop':False,'request_sha256':'0'*64}",
        "raw=json.dumps(v,separators=(',',':'));identity=hashlib.sha256(raw.encode()).hexdigest();d.execute(\"INSERT INTO effects(id,device,record,outcome) VALUES(?,?,?,'prepared')\",(identity,'synthetic-device',raw));d.commit();print(json.dumps({'boot':boot,'id':identity,'raw':raw}))",
      ]);
      await stop();start();await ready();
      const after=await python([
        'import sqlite3,json,sys',
        "d=sqlite3.connect(sys.argv[1]);boot=d.execute('SELECT boot FROM owner WHERE singleton=1').fetchone()[0];row=d.execute('SELECT id,record,outcome FROM effects').fetchone();print(json.dumps({'boot':boot,'id':row[0],'raw':row[1],'outcome':row[2],'count':d.execute('SELECT count(*) FROM effects').fetchone()[0]}))",
      ]);
      assert.notEqual(after.boot,before.boot);assert.equal(after.id,before.id);assert.equal(after.raw,before.raw);assert.equal(after.outcome,'prepared');assert.equal(after.count,1);
      await stop();
      const digest=await python(['import sqlite3,json,hashlib,sys',"d=sqlite3.connect(sys.argv[1]);d.execute('PRAGMA user_version=11');d.close();print(json.dumps(hashlib.sha256(open(sys.argv[1],'rb').read()).hexdigest()))"]);
      const refused=await run(binary,['engine']);assert.notEqual(refused.code,0);assert.match(refused.err,/device journal requires an explicit migration/);
      const retained=await python(['import hashlib,json,sys',"print(json.dumps(hashlib.sha256(open(sys.argv[1],'rb').read()).hexdigest()))"]);assert.equal(retained,digest);
    } finally {await stop();}
  });
