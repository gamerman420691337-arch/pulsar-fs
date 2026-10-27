// Actual CPU pair output across one prepared authority switch. No replay or physical claims.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
export async function cpuPreparedOverlap({cli,waitFor,running,root,state,project,initial,options}) {
  const pixels=path.join(root,'cpu-pixels.rgb'),header=path.join(root,'cpu-frame.json');
  const observations=[];
  const status=epoch=>cli(['live-capture','status',project,'0',epoch]).value;
  const push=(report,frame,sourceNumerator,byte)=>{
    const data=Buffer.alloc(12288,byte);fs.writeFileSync(pixels,data,{mode:0o600});
    const h={frame,source_time:{numerator:sourceNumerator,denominator:10},capture_tick:report.server_elapsed,
      sha256:crypto.createHash('sha256').update(data).digest('hex')};
    fs.writeFileSync(header,JSON.stringify(h),{mode:0o600});
    cli(['live-capture','push',project,'0',report.identity.epoch,header,pixels]);observations.push({epoch:report.identity.epoch,byte,header:h});
  };
  push(status(initial.identity.epoch),0,0,0);push(status(initial.identity.epoch),1,1,64);
  const before=await waitFor(()=>{const r=status(initial.identity.epoch);
    return r.inference.accepted===1&&r.queue.pending_frames===0&&r.queue.in_flight_frames===0?r:false;},'initial CPU pair');
  const p=path.join(root,'cpu-prepare.json');fs.writeFileSync(p,JSON.stringify({...options,continuation_mode:'when_ready',source_anchor:{numerator:1,denominator:1},project_anchor:1000000000}),{mode:0o600});
  const next=cli(['live-capture','continue',project,'0',initial.identity.epoch,p,'--request-id','cpu-overlap']).value;
  assert.equal(next.inference.state,'starting');
  // This work is submitted after preparation admission, with no prior pending/in-flight work.
  push(before,2,2,128);
  push(next,0,10,32);
  await running(next.identity.epoch);
  const priorTerminal=status(initial.identity.epoch);assert.equal(priorTerminal.retirement,'superseded');assert.equal(priorTerminal.active,false);
  assert.ok(priorTerminal.inference.accepted>before.inference.accepted,'old CPU pair accepted during preparation');
  push(status(next.identity.epoch),1,11,96);
  const inferred=await waitFor(()=>{const r=status(next.identity.epoch);return r.inference.accepted===1?r:false;},'successor CPU pair');
  const v=inferred.inference.latest.result.payload;assert.equal(inferred.inference.latest.result.status,'estimated');
  assert.equal(v.identity.epoch,next.identity.epoch);assert.equal(v.input.pair.previous_frame,0);assert.equal(v.input.pair.current_frame,1);
  const expected=.25+.5*(32+96)/510;assert.ok(Math.abs(v.position-expected)<1e-4);
  assert.equal(cli(['live-capture','stop',project,'0',next.identity.epoch]).value.retirement,'stopped');
  await waitFor(()=>{const f=JSON.parse(fs.readFileSync(path.join(state,'worker-fence.json')));
    return ['groups','retired','staging'].every(k=>Object.keys(f[k]).length===0)&&Object.keys(f.creation.records).length===0;},'CPU custody');
  fs.writeFileSync(path.join(root,'cpu-overlap-receipt.json'),JSON.stringify({status:'PASS',qualified:false,before,next,priorTerminal,inferred,expected,observations,
    additional_predecessor_acceptances:priorTerminal.inference.accepted-before.inference.accepted,real_media:false,physical_dispatch:false},null,2),{mode:0o600});
}
