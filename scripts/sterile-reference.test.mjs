import {test} from 'node:test';
import assert from 'node:assert/strict';
import {suppliedReference,prepareSterileReference} from './sterile-reference.mjs';
const json=Buffer.from(JSON.stringify({actions:[{at:0,pos:0},{at:100,pos:100},{at:200,pos:100},{at:400,pos:0}]}));
const csv=Buffer.from('#Created by Handy SDK v3\n0,0\n100,100\n200,100\n400,0\n');
test('supplied reference excludes incomplete terminal stroke without retiming or inventing plateaus',()=> {
  const result=suppliedReference(json,csv,{durationNs:'300000000'});
  assert.deepEqual(result.reference.window,{start_ns:'0',end_ns:'200000000'});
  assert.equal(result.reference.strokes.length,1);
  assert.equal(result.reference.strokes[0].end_ns,'100000000');
  assert.equal(result.receipt.excluded_terminal_anchors,1);
  assert.equal(result.receipt.alignment_adjusted,false);
  assert.equal(result.receipt.visual_alignment_verified,false);
  assert.equal(result.receipt.release_qualified,false);
});
test('reference disagreement, invalid values and undeclared timing reject',()=> {
  assert.throws(()=>suppliedReference(json,Buffer.from('0,0\n100,90\n'),{durationNs:'300000000'}),/differ/);
  assert.throws(()=>suppliedReference(json,csv,{durationNs:'01'}),/canonical/);
  assert.throws(()=>suppliedReference(Buffer.from('{"actions":[{"at":0,"pos":101},{"at":1,"pos":0}]}'),csv,{durationNs:'1'}),/invalid/);
  assert.throws(()=>prepareSterileReference({inputRoot:'/does-not-exist',outputRoot:'/unused',durationNs:'1'}),/authorization/);
});
