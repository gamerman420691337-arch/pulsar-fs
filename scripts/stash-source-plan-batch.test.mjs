import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import http from 'node:http';
import crypto from 'node:crypto';
import {spawn} from 'node:child_process';
import {setTimeout as delay} from 'node:timers/promises';

const enabled=process.env.PULSAR_STASH_TEST_BINARY && process.env.PULSAR_STASH_PRECEDING_BINARY;
test('native source-plan Stash batches preserve placed interval clocks, restart and explicit migration',
  {skip:!enabled,timeout:120000},async t=>{
    const root=fs.mkdtempSync(path.join(os.tmpdir(),'pulsar-stash-source-plans-'));fs.chmodSync(root,0o700);
    t.diagnostic(`retained private source-plan batch fixture: ${root}`);
    const binary=fs.realpathSync(process.env.PULSAR_STASH_TEST_BINARY),preceding=fs.realpathSync(process.env.PULSAR_STASH_PRECEDING_BINARY);
    const hash=bytes=>crypto.createHash('sha256').update(bytes).digest('hex');
    const identity=file=>({path:file,sha256:hash(fs.readFileSync(file)),byte_len:fs.statSync(file).size});
    const media=path.join(root,'media');fs.mkdirSync(media,{mode:0o700});
    const files=Array.from({length:4},(_,index)=>{
      const file=path.join(media,`source-${index}.mkv`);
      const frames=Array.from({length:8},(_,frame)=>Buffer.concat([Buffer.from('FRAME\n'),Buffer.from(Array.from({length:4096},(_,i)=>{
        const x=i%64,y=Math.floor(i/64),sy=(y+64-frame)%64;
        return (x*31+sy*17+Math.floor(x/7)*Math.floor(sy/5)*13+index)%256;
      }))]));
      fs.writeFileSync(file,Buffer.concat([Buffer.from('YUV4MPEG2 W64 H64 F8:1 Ip A1:1 Cmono\n'),...frames]));return file;
    });
    const scenes=files.map((file,i)=>({id:`s${i}`,title:`Authored source ${i}`,interactive:false,files:[{id:`f${i}`,path:`/stash/${path.basename(file)}`}],tags:[]}));
    let selected=scenes.slice(0,2);const queries=[],scans=new Map();
    const tag={id:'tag-authored',name:'Source plan verified attachment'};
    const server=http.createServer(async(req,res)=>{
      const chunks=[];for await(const b of req)chunks.push(b);const body=JSON.parse(Buffer.concat(chunks));queries.push(body);let data;
      if(body.query.includes('metadataScan')) {
        assert.equal(body.variables.input.paths.length,1);const scene=scenes.find(s=>s.files[0].path===body.variables.input.paths[0]);assert.ok(scene);
        assert.ok(fs.existsSync(files[Number(scene.id.slice(1))].replace(/\.mkv$/,'.funscript')));
        scene.interactive=true;const id=`scan-${scene.id}`;assert.equal(scans.has(id),false,'no duplicate scan effect');scans.set(id,scene.id);data={metadataScan:id};
      }else if(body.query.includes('findJob')) {const id=body.variables.input.id;assert.ok(scans.has(id));data={findJob:{id,status:'FINISHED'}};}
      else if(body.query.includes('findTags')) {assert.equal(body.variables.name,tag.name);data={findTags:{count:1,tags:[tag]}};}
      else if(body.query.includes('bulkSceneUpdate')) {const input=body.variables.input;assert.equal(input.tag_ids.mode,'ADD');assert.deepEqual(input.tag_ids.ids,[tag.id]);const scene=scenes.find(s=>s.id===input.ids[0]);assert.ok(scene);assert.equal(scene.tags.length,0);scene.tags=[{id:tag.id}];data={bulkSceneUpdate:[{id:scene.id}]};}
      else if(body.query.includes('findScene(id:')) {data={findScene:scenes.find(s=>s.id===body.variables.id)};}
      else {assert.ok(body.query.includes('findScenes'));data={findScenes:{count:selected.length,scenes:selected}};}
      res.writeHead(200,{'Content-Type':'application/json',Connection:'close'});res.end(JSON.stringify({data}));
    });
    await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));const url=`http://127.0.0.1:${server.address().port}/graphql`;
    const env={PATH:'/usr/bin:/bin',PULSAR_STATE_DIR:path.join(root,'engine')};
    if(process.env.PULSAR_CGROUP_ROOT)env.PULSAR_CGROUP_ROOT=process.env.PULSAR_CGROUP_ROOT;
    const commands=[];
    const run=(executable,args)=>new Promise((resolve,reject)=>{
      const p=spawn(executable,args,{env,stdio:['ignore','pipe','pipe']});let out='',err='';
      p.stdout.on('data',b=>out=(out+b).slice(-4*1024*1024));p.stderr.on('data',b=>err=(err+b).slice(-1024*1024));p.once('error',reject);
      p.once('exit',code=>{commands.push({executable,args,code});resolve({code,out,err});});
    });
    const cli=args=>run(binary,args),ok=async args=>{const r=await cli(args);assert.equal(r.code,0,r.err+r.out);return JSON.parse(r.out);};
    let engine,exited,stderr='';
    const start=()=>{engine=spawn(binary,['engine'],{env,stdio:['ignore','ignore','pipe']});engine.stderr.on('data',b=>stderr=(stderr+b).slice(-65536));exited=new Promise(resolve=>engine.once('exit',resolve));};
    const stop=async()=>{if(engine?.exitCode===null){engine.kill('SIGTERM');await exited;}};
    const ready=async()=>{const end=Date.now()+15000;while(true){assert.equal(engine.exitCode,null,stderr);assert.ok(Date.now()<end,'startup deadline');if(fs.existsSync(path.join(env.PULSAR_STATE_DIR,'engine.sock'))&&(await cli(['capabilities'])).code===0)return;await delay(25);}};
    const db=path.join(env.PULSAR_STATE_DIR,'projects.sqlite3');
    const records=async()=>{const r=await run('/usr/bin/python3',['-c',"import sqlite3,json,sys;d=sqlite3.connect('file:'+sys.argv[1]+'?mode=ro',uri=True);print(json.dumps({'schema':d.execute('PRAGMA user_version').fetchone()[0],'records':[json.loads(r[0]) for r in d.execute('SELECT record FROM stash_batches ORDER BY request')],'jobs':[json.loads(r[0]) for r in d.execute('SELECT manifest FROM jobs')]}))",db]);assert.equal(r.code,0,r.err);return JSON.parse(r.out);};
    const plan=(index,rate,interval)=>({scene_id:`s${index}`,file_id:`f${index}`,source_sha256:hash(fs.readFileSync(files[index])),
      placement:{source_version:hash(fs.readFileSync(files[index])),source_start:{numerator:0,denominator:1},source_end:{numerator:875000001,denominator:1000000000},
        source_anchor:{numerator:0,denominator:1},project_anchor:rate>0?2000000001:3000000001,rate_numerator:rate,rate_denominator:1},project_interval:interval,source_time:true});
    const plans=[plan(0,2,{start:2125000001,end:2375000002}),plan(1,-2,{start:2687500001,end:2937500002})];
    const mappings=path.join(root,'mappings.json'),catalog=path.join(root,'plans.json');fs.writeFileSync(mappings,JSON.stringify([{server_root:'/stash',local_root:media}]));fs.writeFileSync(catalog,JSON.stringify(plans));
    const args=['stash','generate-missing','--url',url,'--mappings',mappings,'--source-plans',catalog,'--width','64','--height','64','--preset','fast','--limit','2','--tag',tag.name,'--detach','--request-id','placed-two-source-batch'];
    const status=async id=>(await ok(['stash','batch-status',id])).value;
    const terminal=async id=>{const end=Date.now()+45000;while(true){const r=await status(id);if(r.state!=='running')return r;assert.ok(Date.now()<end,'batch deadline');await delay(25);}};
    const results={root,binary:identity(binary),preceding:identity(preceding),plans};
    try {
      start();await ready();const initial=await ok(args);assert.equal(initial.schema_version,6);
      const end=Date.now()+15000;let cut;
      while(true){cut=await status(initial.batch_request_id);if(cut.items[0].requests.some(r=>r.phase==='place') && cut.items[0].job_id===null)break;assert.ok(Date.now()<end,'placement intent cut');await delay(10);}
      await stop();results.restart_cut=cut;
      const databaseFiles=[db,`${db}-wal`].filter(fs.existsSync),before=databaseFiles.map(identity);
      const old=await run(preceding,['engine']);assert.notEqual(old.code,0);assert.match(old.err,/source_plans|batch|reader/i);
      assert.deepEqual(databaseFiles.map(identity),before,'preceding reader must preserve observed database/WAL bytes');results.preceding_reader={code:old.code,error:old.err,before};
      start();await ready();let final=await terminal(initial.batch_request_id);assert.equal(final.state,'finished',JSON.stringify(final));
      results.completed=final;
      for(const [index,item] of final.items.entries()) {
        assert.equal(item.state,'tagged_unqualified');assert.equal(item.requests.filter(r=>r.phase==='place').length,1);assert.equal(item.requests.filter(r=>r.phase==='generate').length,1);
        const script=JSON.parse(fs.readFileSync(item.export.path)),proof=JSON.parse(fs.readFileSync(item.export.timing_proof.path));
        assert.deepEqual(script.actions.map(a=>a.at),index===0?[250,375,500,625,750]:[125,250,375,500,625]);
        assert.equal(hash(fs.readFileSync(item.export.path)),item.export.identity.sha256);assert.equal(hash(fs.readFileSync(item.export.timing_proof.path)),item.export.timing_proof.identity.sha256);
        assert.deepEqual(proof.context.placement.spec,plans[index].placement);assert.equal(item.export.timing_proof.mapped_actions,5);assert.equal(item.export.release_qualified,false);
        assert.equal(item.scan.state,'verified_linked');assert.equal(item.tag.state,'verified_added');
        const observation=(await ok(['stash','publication-status',item.project_id,'--publication-request-id',item.export.publication_request_id])).value;
        assert.equal(observation.source_and_script_observation,'verified');assert.equal(observation.proof_observation,'verified');
      }
      const retained=await records();assert.ok(retained.schema>=13);const rec=retained.records.find(r=>r.report.batch_request_id===initial.batch_request_id);
      for(const [i,item] of rec.items.entries()){assert.deepEqual(item.generation_plan,plans[i]);const job=retained.jobs.find(j=>j.source.source_version===plans[i].placement.source_version);assert.ok(job);assert.deepEqual(job.operation.arguments.settings.source_placement,item.placement);assert.deepEqual(job.operation.arguments.settings.project_interval,plans[i].project_interval);}
      results.archives=[];
      for(const [i,item] of final.items.entries()) {
        let r=await cli(['allow-packaging',item.project_id,'--acknowledge-unencrypted-private-data']);assert.equal(r.code,0,r.err);
        const archive=path.join(root,`interval-${i}.pulsar`);r=await cli(['package-export',item.project_id,archive,'--acknowledge-unencrypted-private-data']);assert.equal(r.code,0,r.err);
        const clone=(await ok(['package-import',archive])).receipt.project_id;
        const mapped=await run('/usr/bin/python3',['-c',"import sqlite3,json,sys;d=sqlite3.connect('file:'+sys.argv[1]+'?mode=ro',uri=True);m=json.loads(d.execute('SELECT manifest FROM project_import_archives WHERE project=?',(sys.argv[2],)).fetchone()[0]);e=next(e for e in m['imported_origins'] if e['container']['project_id']==sys.argv[3]);print(next(c['local'] for c in e['candidates'] if c['origin']==sys.argv[4]));assert d.execute('SELECT COUNT(*) FROM jobs WHERE project=?',(sys.argv[2],)).fetchone()[0]==0",db,clone,item.project_id,item.candidate_id]);assert.equal(mapped.code,0,mapped.err);const alias=mapped.out.trim();
        const state=(await ok(['project','open',clone])).value;
        const report=(await ok(['project','archived-generation-status',clone,alias,'--master-revision',String(state.revision)])).value;
        // The clock receipt retains all eight placement anchors; the interval
        // restricts generated/published actions to the five selected frames.
        assert.equal(report.active_job,false);assert.equal(report.publication_authority,false);assert.equal(report.origins[0].clock.sample_count,8);assert.deepEqual(report.origins[0].settings.project_interval,plans[i].project_interval);
        const rebound=path.join(root,`rebound-${i}`);fs.mkdirSync(rebound,{mode:0o700});fs.copyFileSync(files[i],path.join(rebound,path.basename(files[i])));
        const mapping=path.join(root,`rebound-${i}.json`);fs.writeFileSync(mapping,JSON.stringify([{server_root:'/stash',local_root:rebound}]));
        // Restore into a fresh missing-script library entry; the original scene
        // remains interactive and its existing sibling script is preserved.
        const reboundScene=`archive-s${i}`,reboundFile=`archive-f${i}`;
        scenes.push({id:reboundScene,title:'Authored archive restore',interactive:false,files:[{id:reboundFile,path:scenes[i].files[0].path}],tags:[]});
        await ok(['stash','capture-source',clone,reboundScene,reboundFile,'--url',url,'--mappings',mapping,'--expected-source-version',item.source_version,'--revision',String(state.revision)]);
        const origin=path.join(root,`origin-${i}.json`);fs.writeFileSync(origin,JSON.stringify(report.origins[0].origin_manifest));
        const published=(await ok(['stash','publish-script',clone,reboundScene,reboundFile,'--url',url,'--source-version',item.source_version,'--candidate',alias,'--job',item.job_id,'--revision',String(state.revision),'--archived-origin',origin,'--source-time','--request-id',`archived-interval-${i}`])).value;
        assert.deepEqual(fs.readFileSync(published.path),fs.readFileSync(item.export.path));assert.equal(published.timing_proof.archived_context.active_job,false);
        assert.deepEqual(published.timing_proof.archived_context.origin.settings.project_interval,plans[i].project_interval);
        results.archives.push({clone,alias,report,published});
      }
      selected=[scenes[2]];const correct=plan(2,3,null),bad={...correct,source_sha256:'b'.repeat(64)};const badFile=path.join(root,'bad.json');fs.writeFileSync(badFile,JSON.stringify([bad]));
      const repairArgs=args.map(a=>a===catalog?badFile:a==='placed-two-source-batch'?'placed-repair-batch':a);const submitted=await ok(repairArgs);const paused=await terminal(submitted.batch_request_id);
      assert.equal(paused.state,'needs_attention');assert.equal(paused.items[0].job_id,null);assert.equal(paused.items[0].requests.filter(r=>['place','generate'].includes(r.phase)).length,0);assert.equal(fs.existsSync(files[2].replace(/\.mkv$/,'.funscript')),false);
      const replacement=path.join(root,'replacement.json');fs.writeFileSync(replacement,JSON.stringify({preset:'fast',settings:rec.options.settings,model_path:null,source_plans:[correct]}));
      const migrate=['stash','migrate-batch',submitted.batch_request_id,'--expected-ledger-sha256',paused.ledger_sha256,'--generation',replacement,'--request-id','placed-source-repair'];
      const stale=await cli(migrate.map(a=>a===paused.ledger_sha256?'0'.repeat(64):a));assert.notEqual(stale.code,0);
      let migrated;const migrationEnd=Date.now()+5000;
      while(true){const r=await cli(migrate);if(r.code===0){migrated=JSON.parse(r.out).value;break;}assert.match(r.err,/scheduler is still active/);assert.ok(Date.now()<migrationEnd);await delay(25);}
      assert.equal(migrated.state,'needs_attention');await stop();start();await ready();assert.deepEqual((await ok(migrate)).value,migrated);
      assert.equal((await status(submitted.batch_request_id)).state,'needs_attention');await ok(['stash','resume-batch',submitted.batch_request_id]);
      const repaired=await terminal(submitted.batch_request_id);assert.equal(repaired.state,'finished',JSON.stringify(repaired));assert.equal(repaired.items[0].state,'tagged_unqualified');
      assert.deepEqual(JSON.parse(fs.readFileSync(repaired.items[0].export.path)).actions.map(a=>a.at),[0,125,250,375,500,625,750,875]);
      const scansBefore=queries.filter(q=>q.query.includes('metadataScan')).length;assert.equal(scansBefore,3);assert.equal(queries.filter(q=>q.query.includes('bulkSceneUpdate')).length,3);
      selected=[scenes[3]];const gap=plan(3,2,{start:2100000001,end:2400000001}),gapFile=path.join(root,'gap.json');fs.writeFileSync(gapFile,JSON.stringify([gap]));
      const gapArgs=args.map(a=>a===catalog?gapFile:a==='placed-two-source-batch'?'placed-gap-refusal':a);const gapSubmitted=await ok(gapArgs);const gapResult=await terminal(gapSubmitted.batch_request_id);
      assert.equal(gapResult.items[0].state,'failed_or_unknown');assert.match(gapResult.items[0].error,/gap-resolved motion/);assert.equal(gapResult.items[0].export,null);
      assert.equal(fs.existsSync(files[3].replace(/\.mkv$/,'.funscript')),false);assert.equal(queries.filter(q=>q.query.includes('metadataScan')).length,scansBefore);
      const gapState=(await ok(['project','open',gapResult.items[0].project_id])).value;assert.ok(gapState.program.tracks.find(t=>t.axis==='stroke').gaps.length>0);
      results.uncovered_interval_refusal=gapResult;
      await stop();start();await ready();assert.equal((await status(initial.batch_request_id)).state,'finished');assert.equal((await status(submitted.batch_request_id)).state,'finished');
      assert.equal(queries.filter(q=>q.query.includes('metadataScan')).length,scansBefore);results.repaired=repaired;results.retained=await records();results.queries=queries;results.commands=commands;results.status='PASS';
      fs.writeFileSync(path.join(root,'result.json'),JSON.stringify(results,null,2));
    }finally {await stop();await new Promise(resolve=>server.close(resolve));fs.writeFileSync(path.join(root,'commands.json'),JSON.stringify(commands,null,2));fs.writeFileSync(path.join(root,'engine.log'),stderr);}
  });
