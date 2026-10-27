// Explicit authored Linux fixture: mutate only the held cgroup of this test's worker.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';

export async function finishCancellation(t,{root,state,cli,waitFor,running,initial,startArgs,optionsPath,project,ownerPid}) {
  const save=(name,value)=>fs.writeFileSync(path.join(root,name),JSON.stringify(value,null,2)+'\n',{mode:0o600});
  const roster=()=>JSON.parse(fs.readFileSync(path.join(state,'worker-fence.json'),'utf8'));
  const status=epoch=>cli(['live-capture','status',project,'0',epoch]).value;
  const pixels=path.join(root,'pixels.rgb'),header=path.join(root,'frame.json');
  const pair=epoch=>{
    for(let frame=0;frame<2;frame++) {
      const bytes=Buffer.alloc(12288,frame*64);fs.writeFileSync(pixels,bytes,{mode:0o600});
      fs.writeFileSync(header,JSON.stringify({frame,source_time:{numerator:frame,denominator:10},capture_tick:status(epoch).server_elapsed,sha256:crypto.createHash('sha256').update(bytes).digest('hex')}),{mode:0o600});
      cli(['live-capture','push',project,'0',epoch,header,pixels]);
    }
  };
  const estimated=epoch=>waitFor(()=>{
    const value=status(epoch);assert.equal(value.active,true,JSON.stringify(value.inference));
    return value.inference.completed===1?value:false;
  },'scheduled CUDA pair did not complete');
  pair(initial.identity.epoch);const before=await estimated(initial.identity.epoch);save('before.json',before);
  const ledger=roster();assert.equal(ledger.version,4);assert.deepEqual(ledger.retired,{});
  const groups=Object.entries(ledger.groups);assert.equal(groups.length,1,'fixture requires exactly one owned worker group');
  const [name,group]=groups[0];assert.equal(path.basename(group.path),name);assert.ok(name.startsWith('pulsar-worker-'));
  assert.ok(group.path.startsWith('/sys/fs/cgroup/'));assert.equal(fs.realpathSync(group.path),group.path);
  const directory=fs.openSync(group.path,fs.constants.O_RDONLY|fs.constants.O_DIRECTORY|fs.constants.O_NOFOLLOW);
  let open=true;
  const held=`/proc/self/fd/${directory}`;
  const events=()=>fs.readFileSync(`${held}/cgroup.events`,'utf8');
  const freezer=value=>{
    const fd=fs.openSync(`${held}/cgroup.freeze`,fs.constants.O_WRONLY|fs.constants.O_NOFOLLOW);
    try{fs.writeSync(fd,value);}finally{fs.closeSync(fd);}
  };
  // A held directory prevents cleanup from following a reused path.
  t.after(()=>{
    if(!open)return;
    try{freezer('0');}catch(error){if(!['ENOENT','ENODEV'].includes(error.code))throw error;}
    finally{fs.closeSync(directory);open=false;}
  });
  const meta=fs.fstatSync(directory);
  assert.equal(meta.dev,group.device);assert.equal(meta.ino,group.inode);assert.equal(meta.uid,process.getuid());
  const pids=fs.readFileSync(`${held}/cgroup.procs`,'utf8').trim().split(/\s+/).map(Number);
  assert.ok(pids.length>0&&pids.length<=16);
  const members=pids.map(pid=>{
    assert.ok(Number.isSafeInteger(pid)&&pid>1);
    const parent=fs.readFileSync(`/proc/${pid}/status`,'utf8').match(/^PPid:\s+(\d+)$/m);
    assert.ok(parent);return {pid,parent:Number(parent[1])};
  });
  // Bubblewrap adds namespace/supervision processes; all must descend from this Engine.
  const parents=new Map(members.map(member=>[member.pid,member.parent]));
  assert.equal(members.filter(member=>member.parent===ownerPid).length,1);
  for(const member of members) {
    const seen=new Set();let cursor=member.pid;
    while(cursor!==ownerPid) {
      assert.ok(parents.has(cursor)&&!seen.has(cursor),'foreign process or parent cycle in owned worker group');
      seen.add(cursor);cursor=parents.get(cursor);
    }
  }
  assert.match(events(),/^populated 1$/m);
  freezer('1');await waitFor(()=>/^frozen 1$/m.test(events()),'owned worker did not freeze',1000);
  save('frozen-group.json',{owner_pid:ownerPid,members,ledger_scope:ledger.scope,name,...group,events:events(),held_directory:true});
  const finishArgs=['live-capture','finish',project,'0',initial.identity.epoch,'--request-id','blocked-finish'];
  const requested=cli(finishArgs).value;assert.equal(requested.inference.state,'finish_requested');save('requested.json',requested);
  const blocked=await waitFor(()=>{
    const value=status(initial.identity.epoch);assert.equal(value.active,true,JSON.stringify(value.inference));
    return value.inference.state==='finishing'?value:false;
  },'CLI Finish did not reach Finishing',1000);save('blocked.json',blocked);
  assert.equal(blocked.inference.latest,null);assert.equal(Object.hasOwn(blocked.inference,'cuda_evidence'),false);
  await new Promise(resolve=>setTimeout(resolve,100));
  const still=status(initial.identity.epoch);assert.equal(still.inference.state,'finishing');assert.equal(still.active,true);
  assert.match(events(),/^frozen 1$/m);save('still-blocked.json',still);
  const stopArgs=['live-capture','stop',project,'0',initial.identity.epoch,'--request-id','blocked-stop'];
  const stopBegin=process.hrtime.bigint();const stopped=cli(stopArgs).value;const stopReturn=process.hrtime.bigint();
  assert.equal(stopped.retirement,'stopped');assert.equal(stopped.active,false);
  await waitFor(()=>{
    const value=roster();return ['groups','retired','staging'].every(key=>Object.keys(value[key]).length===0)&&Object.keys(value.creation.records).length===0;
  },'Stop did not retire recorded ownership',2000);
  const cleanup=process.hrtime.bigint();assert.ok(cleanup-stopReturn<2000000000n);
  assert.equal(typeof stopped.server_elapsed,'number');assert.ok(stopped.server_elapsed<30000000000,'Stop crossed original lease');
  assert.equal(fs.existsSync(group.path),false,'retired cgroup path remains');
  fs.closeSync(directory);open=false;
  const terminal=status(initial.identity.epoch),startReplay=cli(startArgs).value,finishReplay=cli(finishArgs).value,stopReplay=cli(stopArgs).value;
  for(const value of [terminal,startReplay,finishReplay,stopReplay])assert.deepEqual(value,stopped);
  assert.equal(stopped.inference.latest,null);assert.equal(Object.hasOwn(stopped.inference,'cuda_evidence'),false);
  for(const [name,value] of Object.entries({stopped,terminal,'start-replay':startReplay,'finish-replay':finishReplay,'stop-replay':stopReplay}))save(`${name}.json`,value);
  cli(['live-capture','push',project,'0',initial.identity.epoch,header,pixels],false);
  save('cancelled-ledger.json',roster());
  // A new capture must reacquire actual resources and complete through the same CLI.
  const fresh=cli(['live-capture','start',project,'0',optionsPath,'--request-id','fresh-after-cancel']).value;
  assert.equal(fresh.engine_epoch,initial.engine_epoch);assert.notEqual(fresh.identity.epoch,initial.identity.epoch);
  assert.notEqual(fresh.identity.source,initial.identity.source);await running(fresh.identity.epoch);
  pair(fresh.identity.epoch);const freshEstimate=await estimated(fresh.identity.epoch);
  const freshRequested=cli(['live-capture','finish',project,'0',fresh.identity.epoch,'--request-id','fresh-finish']).value;
  const completed=await waitFor(()=>{const value=status(fresh.identity.epoch);return value.active?false:value;},'fresh Finish did not retire');
  assert.equal(completed.retirement,'completed',JSON.stringify(completed.inference));
  assert.equal(completed.inference.cuda_evidence.summary.successful_runs,1);assert.equal(completed.inference.cuda_evidence.qualified,false);
  await waitFor(()=>{const value=roster();return ['groups','retired','staging'].every(key=>Object.keys(value[key]).length===0)&&Object.keys(value.creation.records).length===0;},'fresh Finish did not retire custody');
  save('fresh.json',fresh);save('fresh-estimate.json',freshEstimate);save('fresh-requested.json',freshRequested);save('completed.json',completed);
  save('cancellation-receipt.json',{initial,before,requested,blocked,still,stopped,terminal,startReplay,finishReplay,stopReplay,fresh,completed,
    stop_cli_ns:Number(stopReturn-stopBegin),cleanup_after_stop_ns:Number(cleanup-stopReturn),final_ledger:roster(),
    original_lease_ms:30000,clock_modified:false,held_owned_group_frozen:true,actual_worker_reply_read_observed:false,
    cli_local_ipc:true,production_fault_hook:false,provider:'cuda',authored_model:true,real_media:false,physical_dispatch:false,qualified:false});
}
