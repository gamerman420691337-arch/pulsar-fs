import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {spawn} from 'node:child_process';
import {setTimeout as delay} from 'node:timers/promises';

test('native Handy inspection requires explicit consent and engine-local credentials without actuation',
  {skip:!process.env.PULSAR_STASH_TEST_BINARY,timeout:30000},async t=>{
    const binary=fs.realpathSync(process.env.PULSAR_STASH_TEST_BINARY);
    const root=fs.mkdtempSync(path.join(os.tmpdir(),'pulsar-handy-'));fs.chmodSync(root,0o700);
    t.diagnostic(`retained credential-free Handy fixture: ${root}`);
    const env={PATH:'/usr/bin:/bin',PULSAR_STATE_DIR:path.join(root,'state')};
    const cli=args=>new Promise((resolve,reject)=>{
      const p=spawn(binary,args,{env,stdio:['ignore','pipe','pipe']});let out='',err='';
      const timer=setTimeout(()=>p.kill('SIGKILL'),10000);
      p.stdout.on('data',b=>{out=(out+b).slice(-65536);});p.stderr.on('data',b=>{err=(err+b).slice(-65536);});
      p.once('error',e=>{clearTimeout(timer);reject(e);});p.once('exit',code=>{clearTimeout(timer);resolve({code,out,err});});
    });
    const args=['handy-inspect','--api-key-env','HANDY_ABSENT_FIXTURE_API','--connection-key-env','HANDY_ABSENT_FIXTURE_DEVICE'];
    const noConsent=await cli(args);assert.notEqual(noConsent.code,0);assert.match(noConsent.err,/allow-online-inspection/);
    assert.equal(fs.existsSync(env.PULSAR_STATE_DIR),false);
    const engine=spawn(binary,['engine'],{env,stdio:['ignore','ignore','pipe']});let stderr='';
    engine.stderr.on('data',b=>{stderr=(stderr+b).slice(-65536);});const exit=new Promise(resolve=>engine.once('exit',resolve));
    try {
      const deadline=Date.now()+15000;
      while(!fs.existsSync(path.join(env.PULSAR_STATE_DIR,'engine.sock'))){assert.equal(engine.exitCode,null,stderr);assert.ok(Date.now()<deadline,'engine readiness');await delay(25);}
      const result=await cli([...args,'--allow-online-inspection']);assert.notEqual(result.code,0);
      assert.match(result.err,/credential is absent from the engine environment/);
      const token=path.join(env.PULSAR_STATE_DIR,'handy-owner.token');assert.ok(fs.existsSync(token));
      assert.equal(fs.statSync(token).mode&0o777,0o600);
      const invalid=await cli(['handy-inspect','--api-key-env','plaintext-value','--connection-key-env','HANDY_ABSENT_FIXTURE_DEVICE','--allow-online-inspection']);
      assert.notEqual(invalid.code,0);assert.match(invalid.err,/environment variable names/);
      const capabilities=await cli(['capabilities']);assert.equal(capabilities.code,0,capabilities.err);
      const advertised=JSON.parse(capabilities.out).value;
      assert.ok(advertised.operations.includes('handy_inspect'));
      assert.equal(advertised.physical_playback,false);
    } finally {
      if(engine.exitCode===null && engine.signalCode===null){engine.kill('SIGTERM');const timer=setTimeout(()=>engine.kill('SIGKILL'),2000);try{await exit;}finally{clearTimeout(timer);}}
    }
  });
