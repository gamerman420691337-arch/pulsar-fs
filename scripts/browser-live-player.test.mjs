// Player state tests with a mock DOM/transport; these are not rendered browser QA.
import test from 'node:test';
import assert from 'node:assert/strict';
import vm from 'node:vm';
import fs from 'node:fs';
import {randomUUID} from 'node:crypto';
const html=fs.readFileSync(new URL('../crates/pulsar-clients/src/browser_live.html',import.meta.url),'utf8');
const code=html.match(/<script>([\s\S]*?)<\/script>/)[1];
const defaultPageScope={launch_id:'launch-one',page_id:'page-one',page_capability:'page-capability-one'};
const flush=()=>new Promise(resolve=>setImmediate(resolve));
function player({frameReply,holdStart=false,recording=false,limit="1024",candidateReplies={},failStageOnce=false,getDisplayMedia,getUserMedia,frameCallbacks=true,holdFrameAt=0,pageScope=defaultPageScope,holdRoute=null,challengeReply={pending:true},guard={configured:false,pending:false,current:false},replyOverride,mediaMetadata=true,mediaSeek=true,queuedPause=false,timing=false,handyConfigured=false,hspConfigured=false,lookahead=false,lookaheadMs="1000",analysisMetadata=true,analysisSeek=true,finishReply,finishStatusReply,failFinishOnce=false,leaseMs=30000}={}) {
  const events={},windowEvents={},documentEvents={},sent=[],queue=[],pauseEvents=[];let resolveStart,resolveFrame,resolveRoute,stageCalls=0,frameCalls=0,clock=0,timerId=0,playerCalls=0,presentedFrames=0,challengeCalls=0,heldRoute=false,videoSource,videoPosition=0,timingSequence=0,timingReport=null,nativeFeed=null,feedCalls=0,nativeLastUpload=null,lastCaptureReport=null,finishCalls=0,finishStatusCalls=0;
  const analysisEvents={},analysisQueue=[],drawSources=[];let analysisSource,analysisPosition=0,analysisPresentedFrames=0,analysisCallbackId=0,drawnReader;
  const timers=new Map();
  const setTimeout=(fn,delay)=>{const id=++timerId;timers.set(id,{fn,due:clock+delay});return id;};
  const clearTimeout=id=>timers.delete(id);
  function advance(ms){clock+=ms;for(let n=0;n<100;n++){const entry=[...timers].find(([,t])=>t.due<=clock);if(!entry)return;timers.delete(entry[0]);entry[1].fn();}throw Error("timer loop");}
  const video={paused:true,seeking:false,ended:false,playbackRate:1,readyState:0,currentSrc:'',requestVideoFrameCallback(fn){queue.push(fn);},addEventListener(name,fn){events[name]=fn;},removeEventListener(name,fn){if(events[name]===fn)delete events[name];},pause(){if(this.paused)return;this.paused=true;if(queuedPause)pauseEvents.push(()=>events.pause?.());else events.pause?.();},async play(){this.paused=false;events.play?.();},load(){this.loads=(this.loads||0)+1;},getAttribute(n){return n==='src'?(videoSource??null):(this[n]??null);},removeAttribute(n){if(n==='src'){videoSource=undefined;this.currentSrc='';this.readyState=0;}else delete this[n];}};
  const metadata=(source=videoSource)=>{video.readyState=1;video.currentSrc=source?new URL(source,'http://127.0.0.1:34000/').href:'';events.loadedmetadata?.();};
  const settleSeek=(time=videoPosition)=>{videoPosition=time;video.seeking=false;events.seeked?.();};
  Object.defineProperty(video,'src',{get:()=>videoSource,set(value){videoSource=value;video.readyState=0;video.currentSrc='';if(mediaMetadata&&value?.startsWith('/media/'))Promise.resolve().then(()=>{if(videoSource===value)metadata(value);});}});
  Object.defineProperty(video,'currentTime',{get:()=>videoPosition,set(value){videoPosition=value;video.seeking=true;const source=videoSource;events.seeking?.();if(mediaSeek)Promise.resolve().then(()=>{if(videoSource===source&&videoPosition===value)settleSeek(value);});}});
  const analysisVideo={id:'analysis-video',paused:true,seeking:false,ended:false,playbackRate:1,readyState:0,currentSrc:'',hidden:true,duration:2,error:null,loop:false,
    requestVideoFrameCallback(fn){const id=++analysisCallbackId;analysisQueue.push({id,fn,cancelled:false});return id;},
    cancelVideoFrameCallback(id){const item=analysisQueue.find(x=>x.id===id);if(item)item.cancelled=true;},
    addEventListener(name,fn){analysisEvents[name]=fn;},removeEventListener(name,fn){if(analysisEvents[name]===fn)delete analysisEvents[name];},
    pause(){if(this.paused)return;this.paused=true;analysisEvents.pause?.();},async play(){this.paused=false;this.ended=false;analysisEvents.play?.();},
    load(){this.loads=(this.loads||0)+1;},getAttribute(n){return n==='src'?(analysisSource??null):(this[n]??null);},
    removeAttribute(n){if(n==='src'){analysisSource=undefined;this.currentSrc='';this.readyState=0;}else delete this[n];}};
  const analysisLoaded=(source=analysisSource)=>{analysisVideo.readyState=1;analysisVideo.currentSrc=source?new URL(source,'http://127.0.0.1:34000/').href:'';analysisEvents.loadedmetadata?.();};
  const analysisSettled=(time=analysisPosition)=>{analysisPosition=time;analysisVideo.seeking=false;analysisEvents.seeked?.();};
  Object.defineProperty(analysisVideo,'src',{get:()=>analysisSource,set(value){analysisSource=value;analysisVideo.readyState=0;analysisVideo.currentSrc='';if(analysisMetadata&&value?.startsWith('/media/'))Promise.resolve().then(()=>{if(analysisSource===value)analysisLoaded(value);});}});
  Object.defineProperty(analysisVideo,'currentTime',{get:()=>analysisPosition,set(value){analysisPosition=value;analysisVideo.seeking=true;const source=analysisSource;analysisEvents.seeking?.();if(analysisSeek)Promise.resolve().then(()=>{if(analysisSource===source&&analysisPosition===value)analysisSettled(value);});}});
  if(!frameCallbacks)delete video.requestVideoFrameCallback;
  const elements={'analysis-continuity':{checked:false},'analysis-video':analysisVideo,'lookahead':{checked:lookahead},'lookahead-ms':{value:lookaheadMs},'lookahead-status':{textContent:''},'analysis-estimate':{textContent:''},camera:{},video,position:{value:0},status:{textContent:''},'guard-status':{textContent:''},'timing-status':{textContent:''},'inspect-timing':{},'handy-status':{textContent:''},'handy-selection':{textContent:''},'handy-inspect':{disabled:true},'handy-start':{disabled:true},'handy-original':{disabled:true},'handy-stop':{disabled:true},'handy-environment-ack':{checked:false},'handy-play-ack':{checked:false},'handy-online-ack':{checked:false},file:{},stop:{},"share-tab":{},record:{checked:recording},"record-limit":{value:limit},"recording-selection":{},"record-status":{},"record-details":{},"refresh-recordings":{},"read-recording":{},"retire-recordings":{},"retire-recordings-ack":{checked:false},"recording-history":{},"save-recording":{},"candidate-start":{value:"0"},"candidate-end":{value:"60"},"candidate-interval":{value:"200"},"stage-candidate":{},"retry-candidate":{},"candidate-status":{},"retire-candidate":{},"retire-candidate-ack":{checked:false},"candidate-history":{},"review-candidate":{},"candidate-message":{},"candidate-details":{},"commit-ack":{checked:false},"commit-candidate":{disabled:true},"retry-commit":{},"inspect-export":{},"download-script":{},"retry-export":{},"export-status":{},"retire-export":{},"retire-export-ack":{checked:false},"export-history":{},"export-history-selection":{},"history-download":{},"prepare-gaps":{},"stage-gaps":{},"gap-status":{},"retire-gap":{},"retire-gap-ack":{checked:false},"gap-history":{},"gap-ack":{checked:false}};
  Object.assign(elements,{'refresh-media':{},'media-selection':{value:'',children:[],replaceChildren(){this.children=[];this.value='';},appendChild(option){this.children.push(option);if(!this.value)this.value=option.value;}},'media-details':{},'media-presentation-anchor':{value:'0'},'media-elapsed-anchor':{value:'0'},'media-clock-ack':{checked:false},'prepare-media':{},'inspect-media':{},'release-media':{},'media-message':{textContent:''}});
  Object.assign(elements,Object.fromEntries(['hsp-inspect','hsp-prepare','hsp-preparation-status','hsp-cancel','hsp-bind','hsp-continuity-enable','hsp-continuity-status','hsp-play','hsp-stop','hsp-status'].map(id=>[id,{disabled:true}])));
  Object.assign(elements,Object.fromEntries(['hsp-prepare-ack','hsp-environment-ack','hsp-online-ack','hsp-play-ack','hsp-continuity-ack'].map(id=>[id,{checked:false}])));
  Object.assign(elements,Object.fromEntries(['hsp-selection','hsp-details','hsp-continuity-duration','hsp-status-message'].map(id=>[id,{textContent:''}])));
  const report={active:true,identity:{epoch:'epoch-one'},options:{policy:{maximum_analysis_age_ns:1e9}},inference:{completed:1,latest:{fresh:true,result:{payload:{position:.6,input:{pair:{current_time:{numerator:0,denominator:1}}}}}}}};
  const fetch=async(route,opts)=>{
   const request={route,body:opts.body,headers:{...opts.headers},keepalive:opts.keepalive===true};sent.push(request);
   const playerNumber=route==='/player/start'?++playerCalls:null;
   if(route===holdRoute&&!heldRoute){heldRoute=true;await new Promise(r=>{resolveRoute=r;});}
   if(route==='/start'&&holdStart)await new Promise(r=>{resolveStart=r;});
   if(route==='/frame'&&++frameCalls===holdFrameAt)await new Promise(r=>{resolveFrame=r;});
   if(route==='/stage-candidate'&&failStageOnce&&stageCalls++===0)return {ok:false,json:async()=>({error:'acknowledgement unavailable'})};
   const override=replyOverride?.(request);if(override)return override;
   let data;
   if(route==='/player/start'){
    const start=JSON.parse(Buffer.from(opts.body));data={player:{launch_id:opts.headers['X-Pulsar-Launch'],page_id:opts.headers['X-Pulsar-Page'],player_id:'player-'+playerNumber,...start},player_capability:'player-capability-'+playerNumber,heartbeat_interval_ms:100,guard,handy:{configured:handyConfigured},hsp:{configured:hspConfigured},timing:{configured:timing&&!!start.media_selection_id,pending:timing&&!!start.media_selection_id}};
   }else if(route==='/player/challenge'){data=typeof challengeReply==='function'?challengeReply(++challengeCalls):challengeReply;}
   else if(route==='/player/observe')data={current:true,synchronized:false};
   else if(route==='/player/stop')data={accepted:true};
   else if(route==='/player/handy-inspect')data={selection_checked:true,environment_sha256:'a'.repeat(64),source_sha256:'b'.repeat(64),script_sha256:'c'.repeat(64)};
   else if(route==='/player/handy-start')data={configured:true,original_request_id:'original-browser-start',state:'pending'};
   else if(route==='/player/handy-status'||route==='/player/handy-inspect-original')data={configured:true,original_request_id:'original-browser-start',state:'submitted',history_state:'acknowledged',physical_stop_pending:true};
   else if(route==='/player/handy-stop')data={configured:true,original_request_id:'original-browser-start',state:'released',physical_stop_pending:true};
   else if(route==='/player/timing-probe')data={sequence:String(++timingSequence),probe_id:'probe-'+timingSequence,probe_timeout_ms:1000,observation_timeout_ms:3000};
   else if(route==='/player/timing-observe'){const o=JSON.parse(Buffer.from(opts.body));data=timingReport={current:true,qualified:false,binding_id:'timing-binding',open_request_id:'timing-open',observation:{observation_id:'observation-'+o.sequence,sequence:o.sequence,bracket_width_ns:'10000000',project_phase:{numerator:'1',denominator:'2'},frame:o.frame}};}
   else if(route==='/player/timing-status')data=timingReport??{current:true,qualified:false,observation:null};
   else if(route==='/analysis/finish'||route==='/analysis/finish-status'){
    const count=route==='/analysis/finish'?++finishCalls:++finishStatusCalls,chosen=route==='/analysis/finish'?finishReply:finishStatusReply;
    data=(typeof chosen==='function'?chosen(count):chosen)??{...(lastCaptureReport??report),input_close:{original_request_id:'input-close-'+feedCalls,final_frame:nativeLastUpload?{native_only:true}:null,phase:'sealed',disposition:nativeLastUpload?'accepted':'no_frames',worker_joined:true,delivery_settled:true}};
    if(route==='/analysis/finish'&&failFinishOnce&&finishCalls===1)return {ok:false,json:async()=>({error:'original close reply unavailable'})};
   }
   else data=candidateReplies[route]??(route==='/stop'?{active:false}:route==='/frame'?((typeof frameReply==='function'?frameReply(frameCalls):frameReply)??report):report);
   if(route==='/start'){
    nativeLastUpload=null;lastCaptureReport=null;
    const start=JSON.parse(Buffer.from(opts.body));if(start.analysis){nativeFeed={feed_id:'analysis-feed-'+(++feedCalls),role:'lookahead',generation:Number(opts.headers['X-Pulsar-Generation']),player:{launch_id:opts.headers['X-Pulsar-Launch'],page_id:opts.headers['X-Pulsar-Page'],player_id:opts.headers['X-Pulsar-Player'],source_id:opts.headers['X-Pulsar-Source'],generation:Number(opts.headers['X-Pulsar-Generation']),time_basis:start.source_time_basis},capture_epoch:data.identity.epoch,policy:start.analysis.policy};}else nativeFeed=null;
   }
   if(route==='/frame'&&nativeFeed){const bytes=Buffer.from(opts.body),header=JSON.parse(bytes.subarray(0,bytes.indexOf(10)));nativeLastUpload={media_us:header.media_us,observation:header.analysis};lastCaptureReport=data;}
   if(['/start','/frame','/analysis/finish','/analysis/finish-status'].includes(route)&&nativeFeed)data={...data,options:{lease_ms:leaseMs,...data.options},browser:{analysis_final_result_matches:false,...data.browser,analysis_feed:nativeFeed,analysis_last_upload:nativeLastUpload,analysis_finish_request_id:route.startsWith('/analysis/')?'input-close-'+feedCalls:null}};
   return {ok:true,json:async()=>data};
  };
  const document={visibilityState:'visible',addEventListener(n,f){documentEvents[n]=f;},getElementById:id=>elements[id],createElement:()=>({click(){},getContext:()=>({drawImage(reader){drawnReader=reader;drawSources.push(reader===analysisVideo?"analysis":"visible");},getImageData:()=>({data:new Uint8Array(64*64*4).fill(drawnReader===analysisVideo?128:64)})})})};
  const context=vm.createContext({document,window:{location:{href:'http://127.0.0.1:34000/'},addEventListener(n,f){windowEvents[n]=f;}},navigator:{mediaDevices:getDisplayMedia||getUserMedia?{getDisplayMedia,getUserMedia}:undefined},fetch,TextEncoder,Uint8Array,URL,Blob,AbortSignal,crypto:{randomUUID},performance:{now:()=>clock},setTimeout,clearTimeout,console});
  vm.runInContext(code.replace('/*__PULSAR_PAGE_SCOPE__*/ null',JSON.stringify(pageScope)),context);
  const displayTimers=()=>{const id=vm.runInContext('displayTimer',context);return timers.has(id)?[timers.get(id)]:[];};
  return {elements,sent,events,analysisEvents,drawSources,analysisLoaded,analysisSettled,pendingContinuationTimers(){const id=vm.runInContext('analysisReader?.continuation?.timer',context);return timers.has(id)?[timers.get(id).fn]:[];},continuityState(){return JSON.parse(vm.runInContext('JSON.stringify({epoch,count:analysisReader?.continuationCount,phase:analysisReader?.continuation?.phase,prepareId:analysisReader?.continuation?.prepareId})',context));},analysisEnd(time){analysisPosition=time;analysisVideo.duration=time;analysisVideo.ended=true;analysisVideo.paused=true;analysisEvents.pause?.();analysisEvents.ended?.();},futurePreview(){return JSON.parse(vm.runInContext('JSON.stringify({limit:MAX_FUTURE_ESTIMATES,entries:futureEstimates,last:lastFutureEstimate})',context));},pendingFutureTimers(){const id=vm.runInContext('futureTimer',context);return timers.has(id)?[timers.get(id).fn]:[];},analysisCallback(){return analysisQueue.find(x=>!x.cancelled)?.fn;},analysisFrame(time,frames=++analysisPresentedFrames,metadata={}){analysisPosition=time;let item;while((item=analysisQueue.shift())&&item.cancelled){};assert.ok(item,"analysis callback available");return item.fn(clock,{mediaTime:time,presentedFrames:frames,...metadata});},windowEvents,documentEvents,document,advance,metadata,settleSeek,deliverPauses(){while(pauseEvents.length)pauseEvents.shift()();},seek(time){video.currentTime=time;},timerCount:()=>displayTimers().length,pendingTimers:()=>displayTimers().map(t=>t.fn),controlTimerCount:()=>timers.size-displayTimers().length,releaseFrame:()=>resolveFrame(),releaseRoute:()=>resolveRoute(),play(){video.paused=false;events.play();},frame(time,frames=++presentedFrames,metadata={}){videoPosition=time;return queue.shift()(clock,{mediaTime:time,presentedFrames:frames,...metadata});},release(){resolveStart();}};
}
test('player sends exact timestamp and packed RGB; pause clears motion',async()=>{
 const p=player();p.play();await p.frame(.125);assert.equal(p.elements.position.value,.6);
 const frame=p.sent.find(x=>x.route==='/frame');const packet=Buffer.from(frame.body);const n=packet.indexOf(10);assert.deepEqual(JSON.parse(packet.subarray(0,n)),{epoch:'epoch-one',media_us:125000});assert.equal(packet.length-n-1,12288);assert.ok(packet.subarray(n+1).every(x=>x===64));
 p.elements.video.pause();assert.equal(p.elements.position.value,0);await new Promise(r=>setImmediate(r));assert.equal(p.sent.at(-1).route,'/stop');
});
test('one outstanding frame and late Start cannot rearm after seek',async()=>{
 const p=player({holdStart:true});p.play();const pending=p.frame(0);await new Promise(r=>setImmediate(r));await p.frame(.1);assert.equal(p.sent.filter(x=>x.route==='/start').length,1);
 p.events.seeking();p.release();await pending;await new Promise(r=>setImmediate(r));assert.equal(p.elements.position.value,0);assert.equal(p.sent.filter(x=>x.route==='/frame').length,0);assert.ok(p.sent.some(x=>x.route==='/stop'));
});
test('source-stale estimate is not displayed as current motion',async()=>{
 const p=player();p.play();await p.frame(2);assert.equal(p.elements.position.value,0);assert.match(p.elements.status.textContent,/waiting|stale/i);
});
test('Engine handoff does not replace the browser playback identity',async()=>{
 const reply={active:true,identity:{epoch:'successor-engine-epoch'},options:{policy:{maximum_analysis_age_ns:1e9}},inference:{completed:1,latest:{fresh:true,result:{payload:{position:.4,input:{pair:{current_time:{numerator:0,denominator:1}}}}}}}};
 const p=player({frameReply:reply});p.play();await p.frame(.1);await p.frame(.2);
 const frames=p.sent.filter(x=>x.route==='/frame');assert.equal(frames.length,2);
 for(const frame of frames){const packet=Buffer.from(frame.body);assert.equal(JSON.parse(packet.subarray(0,packet.indexOf(10))).epoch,'epoch-one');}
 assert.equal(p.elements.position.value,.4);
});

test('recording declaration is explicit and captured before first pixel dispatch',async()=>{
 const p=player({recording:true,limit:'32'});p.play();await p.frame(.1);const start=p.sent.find(x=>x.route==='/start');assert.deepEqual(JSON.parse(Buffer.from(start.body)),{media_us:100000,recording_maximum:32,source_time_basis:'source_media_presentation'});assert.equal(p.sent.filter(x=>x.route==='/frame').length,1);
});
test('invalid recording limits refuse before capture or frame effects',async()=>{
 const p=player({recording:true,limit:'0'});p.play();await p.frame(.1);await new Promise(r=>setImmediate(r));assert.equal(p.sent.filter(x=>x.route==='/start'||x.route==='/frame').length,0);assert.match(p.elements.status.textContent,/1\.\.4096/);
});

const stagedReply={request_id:'original-stage',candidate_id:'candidate-one',base_revision:0,sample_count:2,samples:[{time:100000000,position:.375,evidence:'inferred'}],gaps:[{start:0,end:100000000}],qualified:false};
test('candidate staging prepares identity before dispatch and review performs no commit',async()=>{
 const p=player({candidateReplies:{'/prepare-candidate':{request_id:'original-stage'},'/stage-candidate':stagedReply,'/review-candidate':{candidate_id:'candidate-one',diagnostics:{issues:[{code:'live_recording_missing_coverage',message:'Unavailable interval'}]}}}});p.elements['recording-selection'].value='epoch-one';
 await p.elements['stage-candidate'].onclick();assert.deepEqual(p.sent.map(x=>x.route),['/prepare-candidate','/stage-candidate']);assert.deepEqual(JSON.parse(Buffer.from(p.sent[0].body)),{epoch:'epoch-one',range:{start:0,end:60000000000},maximum_interpolation_ns:200000000});assert.match(p.elements['candidate-details'].textContent,/inferred/);assert.match(p.elements['candidate-message'].textContent,/unavailable/);
 await p.elements['review-candidate'].onclick();assert.equal(p.sent.at(-1).route,'/review-candidate');assert.match(p.elements['candidate-details'].textContent,/missing_coverage/);assert.ok(!p.sent.some(x=>/commit|export/.test(x.route)));
});
test('lost staging acknowledgement retries retained intent without replacement preparation',async()=>{
 const p=player({failStageOnce:true,candidateReplies:{'/prepare-candidate':{request_id:'original-stage'},'/stage-candidate':stagedReply}});p.elements['recording-selection'].value='epoch-one';await p.elements['stage-candidate'].onclick();assert.match(p.elements['candidate-message'].textContent,/could not be confirmed/);
 await p.elements['retry-candidate'].onclick();assert.deepEqual(p.sent.map(x=>x.route),['/prepare-candidate','/stage-candidate','/stage-candidate']);assert.match(p.elements['candidate-details'].textContent,/original-stage/);
});

test('browser commit requires review and explicit acknowledgement before its retained request',async()=>{
 const review={candidate_id:'candidate-one',base_revision:0,program_sha256:'a'.repeat(64),commit_ready:true,diagnostics:{issues:[]}};
 const p=player({candidateReplies:{'/review-candidate':review,'/prepare-commit':{commit_request_id:'original-commit'},'/commit-candidate':{acknowledged_revision:1,current_revision:1,remaining_gaps:1}}});
 await p.elements['commit-candidate'].onclick();assert.equal(p.sent.length,0);
 await p.elements['review-candidate'].onclick();assert.equal(p.elements['commit-candidate'].disabled,true);p.elements['commit-ack'].checked=true;p.elements['commit-ack'].onchange();assert.equal(p.elements['commit-candidate'].disabled,false);
 await p.elements['commit-candidate'].onclick();assert.deepEqual(p.sent.map(x=>x.route),['/review-candidate','/prepare-commit','/commit-candidate']);assert.deepEqual(JSON.parse(Buffer.from(p.sent[1].body)),{candidate_id:'candidate-one',base_revision:0,program_sha256:'a'.repeat(64),acknowledge:true});assert.match(p.elements['candidate-message'].textContent,/1 gaps remain/);
 await p.elements['retry-commit'].onclick();assert.equal(p.sent.filter(x=>x.route==='/prepare-commit').length,1);assert.equal(p.sent.at(-1).route,'/commit-candidate');
});

test('browser download refuses uninspected/gapped masters and retains exact export preparation',async()=>{
 const p=player({candidateReplies:{'/inspect-export':{revision:2,program_sha256:'a'.repeat(64),sample_count:2,gaps:[],qualified:false},'/prepare-export':{request_id:'original-export'},'/download-script':{revision:2,script:{actions:[{at:0,pos:38}]},qualified:false}}});
 await p.elements['download-script'].onclick();assert.equal(p.sent.length,0);await p.elements['inspect-export'].onclick();await p.elements['download-script'].onclick();assert.deepEqual(p.sent.map(x=>x.route),['/inspect-export','/prepare-export','/download-script']);await p.elements['retry-export'].onclick();assert.equal(p.sent.filter(x=>x.route==='/prepare-export').length,1);
 const g=player({candidateReplies:{'/inspect-export':{revision:1,gaps:[{start:0,end:1}],sample_count:1}}});await g.elements['inspect-export'].onclick();await g.elements['download-script'].onclick();assert.deepEqual(g.sent.map(x=>x.route),['/inspect-export']);
});

test('gap proposals require inspected coverage and explicit authored-motion acknowledgement before staging',async()=>{
 const gaps=[{start:0,end:100000000}];const p=player({candidateReplies:{'/inspect-export':{revision:1,program_sha256:'b'.repeat(64),gaps},'/prepare-gaps':{request_id:'original-gap',gaps},'/stage-gaps':{candidate_id:'gap-one',base_revision:1},'/gap-status':{request_id:'original-gap',attempted:true}}});
 await p.elements['prepare-gaps'].onclick();assert.equal(p.sent.length,0);
 await p.elements['inspect-export'].onclick();await p.elements['prepare-gaps'].onclick();assert.equal(p.sent.length,1);
 p.elements['gap-ack'].checked=true;await p.elements['prepare-gaps'].onclick();assert.deepEqual(JSON.parse(Buffer.from(p.sent[1].body)),{revision:1,program_sha256:'b'.repeat(64),gaps,acknowledge_linear_edge_hold:true});
 await p.elements['stage-gaps'].onclick();assert.equal(p.elements['commit-candidate'].disabled,true);assert.ok(!p.sent.some(x=>/commit|download/.test(x.route)));assert.match(p.elements['candidate-message'].textContent,/Master unchanged/);
 await p.elements['gap-status'].onclick();assert.match(p.elements['candidate-message'].textContent,/original-gap/);
});

test('gap status describes original upload resume rather than replacement authority',async()=>{
 const p=player({candidateReplies:{'/gap-status':{request_id:'gap-original',attempted:true,candidate_id:null}}});
 await p.elements['gap-status'].onclick();assert.match(p.elements['candidate-message'].textContent,/original Begin, lease and Finish/);assert.match(p.elements['candidate-message'].textContent,/Fresh review/);assert.deepEqual(p.sent.map(x=>x.route),['/gap-status']);
});

test('recovered approved commit presents original identity and requires an explicit retry action',async()=>{
 const p=player({candidateReplies:{'/gap-status':{request_id:'gap-original',attempted:true,candidate_id:'gap-one',commit_request_id:'approved-original',commit_acknowledged_revision:null},'/commit-candidate':{acknowledged_revision:2,current_revision:2,remaining_gaps:0}}});
 await p.elements['gap-status'].onclick();assert.match(p.elements['candidate-message'].textContent,/approved-original/);assert.match(p.elements['candidate-message'].textContent,/Retry original commit/);assert.deepEqual(p.sent.map(x=>x.route),['/gap-status']);assert.equal(p.elements['commit-candidate'].disabled,true);
 await p.elements['retry-commit'].onclick();assert.deepEqual(p.sent.map(x=>x.route),['/gap-status','/commit-candidate']);assert.ok(!p.sent.some(x=>x.route==='/prepare-commit'));
});

test('export intent inspection is inert and retry does not prepare replacement work',async()=>{
 const p=player({candidateReplies:{'/export-status':{request_id:'export-original',revision:2,acknowledged_revision:null},'/download-script':{revision:2,script:{actions:[{at:0,pos:38}]}}}});
 await p.elements['export-status'].onclick();assert.deepEqual(p.sent.map(x=>x.route),['/export-status']);assert.match(p.elements['candidate-message'].textContent,/export-original/);assert.match(p.elements['candidate-message'].textContent,/Inspection creates no export/);
 await p.elements['retry-export'].onclick();assert.deepEqual(p.sent.map(x=>x.route),['/export-status','/download-script']);
});

test('export retirement requires confirmed inspection and acknowledgement, and never deletes output',async()=>{
 const p=player({candidateReplies:{'/export-status':{request_id:'confirmed-export',revision:2,acknowledged_revision:2,output_sha256:'a'.repeat(64)},'/retire-export':{retired_request_id:'confirmed-export',files_preserved:true}}});
 await p.elements['retire-export'].onclick();assert.equal(p.sent.length,0);await p.elements['export-status'].onclick();await p.elements['retire-export'].onclick();assert.equal(p.sent.length,1);
 p.elements['retire-export-ack'].checked=true;await p.elements['retire-export'].onclick();assert.deepEqual(p.sent.map(x=>x.route),['/export-status','/retire-export']);assert.deepEqual(JSON.parse(Buffer.from(p.sent[1].body)),{request_id:'confirmed-export',revision:2,output_sha256:'a'.repeat(64),acknowledge_preserve_files:true});assert.match(p.elements['candidate-message'].textContent,/Files and history preserved/);
 const q=player({candidateReplies:{'/export-status':{request_id:'pending',revision:2,acknowledged_revision:null,output_sha256:null}}});await q.elements['export-status'].onclick();q.elements['retire-export-ack'].checked=true;await q.elements['retire-export'].onclick();assert.deepEqual(q.sent.map(x=>x.route),['/export-status']);
});

test('recording candidate retirement requires confirmed inspection and acknowledgement; history stays inert',async()=>{
 const status={request_id:'stage-one',candidate_id:'candidate-one',base_revision:0,commit_request_id:'commit-one',commit_acknowledged_revision:1};
 const p=player({candidateReplies:{'/candidate-status':status,'/retire-candidate':{retired:{candidate:'candidate-one'}},'/candidate-history':{candidates:[{candidate:'candidate-one'}]}}});
 await p.elements['retire-candidate'].onclick();assert.equal(p.sent.length,0);
 await p.elements['candidate-status'].onclick();await p.elements['retire-candidate'].onclick();assert.equal(p.sent.filter(x=>x.route==='/retire-candidate').length,0);
 p.elements['retire-candidate-ack'].checked=true;await p.elements['retire-candidate'].onclick();const sent=p.sent.find(x=>x.route==='/retire-candidate');assert.deepEqual(JSON.parse(Buffer.from(sent.body)),{request_id:'stage-one',candidate_id:'candidate-one',commit_request_id:'commit-one',acknowledged_revision:1,acknowledge_preserve_history:true});assert.equal(p.elements['commit-candidate'].disabled,true);
 await p.elements['candidate-history'].onclick();assert.match(p.elements['candidate-message'].textContent,/1 retired/);assert.ok(!p.sent.some(x=>/stage-candidate|commit-candidate/.test(x.route)));
 const pending=player({candidateReplies:{'/candidate-status':{...status,commit_acknowledged_revision:null}}});await pending.elements['candidate-status'].onclick();pending.elements['retire-candidate-ack'].checked=true;await pending.elements['retire-candidate'].onclick();assert.equal(pending.sent.filter(x=>x.route==='/retire-candidate').length,0);
});

test('gap retirement requires confirmed inspection and acknowledgement; unknown commits stay fenced',async()=>{
 const i={request_id:'gap-one',candidate_id:'gap-candidate',base_revision:1,commit_request_id:'gap-commit',commit_acknowledged_revision:2};
 const p=player({candidateReplies:{'/gap-status':i,'/retire-gap':{retired:{candidate:'gap-candidate'}},'/gap-history':{proposals:[{candidate:'gap-candidate'}]}}});
 await p.elements['retire-gap'].onclick();assert.equal(p.sent.length,0);await p.elements['gap-status'].onclick();await p.elements['retire-gap'].onclick();assert.equal(p.sent.filter(x=>x.route==='/retire-gap').length,0);
 p.elements['retire-gap-ack'].checked=true;await p.elements['retire-gap'].onclick();assert.deepEqual(JSON.parse(Buffer.from(p.sent.find(x=>x.route==='/retire-gap').body)),{request_id:'gap-one',candidate_id:'gap-candidate',commit_request_id:'gap-commit',acknowledged_revision:2,acknowledge_preserve_history:true});
 await p.elements['gap-history'].onclick();assert.match(p.elements['candidate-message'].textContent,/1 retired/);assert.ok(!p.sent.some(x=>/stage-gaps|commit-candidate/.test(x.route)));
 const pending=player({candidateReplies:{'/gap-status':{...i,commit_acknowledged_revision:null}}});await pending.elements['gap-status'].onclick();pending.elements['retire-gap-ack'].checked=true;await pending.elements['retire-gap'].onclick();assert.equal(pending.sent.filter(x=>x.route==='/retire-gap').length,0);
});

test('recording-index retirement needs fresh stopped selection and acknowledgement',async()=>{
 const page={identity:{epoch:'record-one'},recording_request_id:'record-request',active:false,discarded:false,samples:[],total_samples:0};
 const p=player({candidateReplies:{'/recording-page':page,'/retire-recordings':{retired:[{epoch:'record-one'}]},'/recording-history':{recordings:[{epoch:'record-one'}]}}});p.elements['recording-selection'].value='record-one';
 await p.elements['retire-recordings'].onclick();assert.equal(p.sent.length,0);await p.elements['read-recording'].onclick();await p.elements['retire-recordings'].onclick();assert.equal(p.sent.filter(x=>x.route==='/retire-recordings').length,0);
 p.elements['retire-recordings-ack'].checked=true;await p.elements['retire-recordings'].onclick();assert.deepEqual(JSON.parse(Buffer.from(p.sent.find(x=>x.route==='/retire-recordings').body)),{epoch:'record-one',recording_request_id:'record-request',acknowledge_preserve_records:true});await p.elements['recording-history'].onclick();assert.match(p.elements['record-status'].textContent,/1 retired/);
 const running=player({candidateReplies:{'/recording-page':{...page,active:true}}});running.elements['recording-selection'].value='record-one';await running.elements['read-recording'].onclick();running.elements['retire-recordings-ack'].checked=true;await running.elements['retire-recordings'].onclick();assert.equal(running.sent.filter(x=>x.route==='/retire-recordings').length,0);
});

function sharedTab(surface='browser'){const listeners={};const track={stops:0,stop(){this.stops++;},getSettings:()=>({displaySurface:surface}),addEventListener(n,f){listeners[n]=f;}};return {track,listeners,stream:{getTracks:()=>[track],getVideoTracks:()=>[track]}};}
test('tab sharing is explicit, video-only and feeds the bounded frame path; revocation stops tracks',async()=>{
 const capture=sharedTab();let options,calls=0;const p=player({getDisplayMedia:async o=>{options=o;calls++;return capture.stream;}});assert.equal(calls,0);await p.elements['share-tab'].onclick();assert.equal(calls,1);assert.equal(options.audio,false);assert.equal(options.video.displaySurface,'browser');assert.equal(p.elements.video.srcObject,capture.stream);assert.equal(p.elements.video.muted,true);
 await p.frame(.1);assert.equal(p.sent.filter(x=>x.route==='/frame').length,1);assert.equal(JSON.parse(Buffer.from(p.sent.find(x=>x.route==='/start').body)).source_time_basis,'display_capture_presentation');capture.listeners.ended();await new Promise(r=>setImmediate(r));assert.ok(capture.track.stops>=1);assert.equal(p.elements.video.srcObject,null);assert.equal(p.elements.position.value,0);assert.ok(p.sent.some(x=>x.route==='/stop'));
});
test('tab picker denial, non-tab source and late consent create no capture frames',async()=>{
 const denied=player({getDisplayMedia:async()=>{throw Error('Permission denied');}});await denied.elements['share-tab'].onclick();assert.ok(!denied.sent.some(x=>x.route==='/start'||x.route==='/frame'));
 const wrong=sharedTab('monitor');const refused=player({getDisplayMedia:async()=>wrong.stream});await refused.elements['share-tab'].onclick();assert.ok(wrong.track.stops>=1);assert.ok(!refused.sent.some(x=>x.route==='/start'||x.route==='/frame'));
 let resolve;const late=sharedTab();const pending=player({getDisplayMedia:()=>new Promise(r=>{resolve=r;})});const choose=pending.elements['share-tab'].onclick();pending.elements.stop.onclick();resolve(late.stream);await choose;assert.ok(late.track.stops>=1);assert.ok(!pending.sent.some(x=>x.route==='/start'||x.route==='/frame'));assert.equal(pending.elements.video.srcObject,undefined);
});

test('tab tracks close on source switch and page exit; unsupported capture remains inert',async()=>{
 const first=sharedTab();const switched=player({getDisplayMedia:async()=>first.stream});await switched.elements['share-tab'].onclick();switched.elements.file.onchange({target:{files:[]}});assert.ok(first.track.stops>=1);assert.equal(switched.elements.video.srcObject,null);assert.equal(switched.elements.video.muted,false);
 const second=sharedTab();const exited=player({getDisplayMedia:async()=>second.stream});await exited.elements['share-tab'].onclick();exited.windowEvents.pagehide();await new Promise(r=>setImmediate(r));assert.ok(second.track.stops>=1);assert.equal(exited.elements.video.srcObject,null);assert.ok(exited.sent.some(x=>x.route==='/stop'));
 const unsupported=player();await unsupported.elements['share-tab'].onclick();assert.equal(unsupported.sent.length,0);assert.match(unsupported.elements.status.textContent,/unavailable/);
});

test('muted capture and post-play failure release tracks and fence inference',async()=>{
 const muted=sharedTab();const p=player({getDisplayMedia:async()=>muted.stream});await p.elements['share-tab'].onclick();await p.frame(.1);muted.listeners.mute();await new Promise(r=>setImmediate(r));assert.equal(p.elements.position.value,0);assert.equal(p.elements.video.srcObject,null);assert.ok(muted.track.stops>=1);const n=p.sent.filter(x=>x.route==='/frame').length;await p.frame(.2);assert.equal(p.sent.filter(x=>x.route==='/frame').length,n);
 const failed=sharedTab();const q=player({getDisplayMedia:async()=>failed.stream});q.elements.video.play=async()=>{q.play();throw Error('play failed after event');};await q.elements['share-tab'].onclick();assert.equal(q.elements.video.srcObject,null);assert.equal(q.elements.position.value,0);assert.ok(failed.track.stops>=1);assert.ok(!q.sent.some(x=>x.route==='/start'||x.route==='/frame'));
});

test('late permission rejection cannot stop a newer local playback',async()=>{
 let reject;const p=player({getDisplayMedia:()=>new Promise((_,r)=>{reject=r;})});const pending=p.elements['share-tab'].onclick();p.elements.file.onchange({target:{files:[]}});p.play();reject(Error('late denied'));await pending;assert.equal(p.elements.video.paused,false);await p.frame(.1);assert.equal(p.sent.filter(x=>x.route==='/frame').length,1);
});

test('already muted or ended selected tracks cannot start inference',async()=>{
 for(const state of [{muted:true},{readyState:'ended'}]){const c=sharedTab();Object.assign(c.track,state);const p=player({getDisplayMedia:async()=>c.stream});await p.elements['share-tab'].onclick();assert.ok(c.track.stops>=1);assert.ok(!p.sent.some(x=>x.route==='/start'||x.route==='/frame'));assert.equal(p.elements.video.srcObject,undefined);}
});


test('camera consent is explicit and video-only; bounded frames declare camera presentation',async()=>{
 const capture=sharedTab('camera');let calls=0,options;capture.track.getSettings=()=>{throw Error('device identity must not be read');};
 const p=player({getUserMedia:async o=>{calls++;options=o;return capture.stream;}});
 assert.equal(calls,0);await p.elements.camera.onclick();assert.equal(calls,1);assert.equal(options.audio,false);assert.equal(options.video.frameRate.max,30);
 assert.equal(p.elements.video.srcObject,capture.stream);assert.equal(p.elements.video.muted,true);
 await p.frame(.1);const start=JSON.parse(Buffer.from(p.sent.find(x=>x.route==='/start').body));assert.equal(start.source_time_basis,'camera_capture_presentation');
 const frame=p.sent.find(x=>x.route==='/frame');assert.ok(frame.body.length>=12288&&frame.body.length<13312);
 p.elements.stop.onclick();assert.ok(capture.track.stops>=1);assert.equal(p.elements.video.srcObject,null);assert.equal(p.elements.position.value,0);
});

test('camera denial and unavailable tracks never dispatch capture',async()=>{
 const denied=player({getUserMedia:async()=>{throw Error('permission denied');}});await denied.elements.camera.onclick();assert.ok(!denied.sent.some(x=>x.route==='/start'||x.route==='/frame'));
 for(const state of [{muted:true},{readyState:'ended'},{enabled:false}]){
  const c=sharedTab('camera');Object.assign(c.track,state);const p=player({getUserMedia:async()=>c.stream});await p.elements.camera.onclick();assert.ok(c.track.stops>=1);assert.ok(!p.sent.some(x=>x.route==='/start'||x.route==='/frame'));
 }
});

test('camera request refuses extra tracks and stops every returned track',async()=>{
 const c=sharedTab('camera'),audio={stops:0,stop(){this.stops++;}};c.stream.getTracks=()=>[c.track,audio];
 const p=player({getUserMedia:async()=>c.stream});await p.elements.camera.onclick();assert.ok(c.track.stops>=1&&audio.stops>=1);assert.ok(!p.sent.some(x=>x.route==='/start'||x.route==='/frame'));
});

test('pending camera request fences late consent and cannot overlap another picker',async()=>{
 let resolve,calls=0,tabCalls=0;const camera=sharedTab('camera');const p=player({getUserMedia:()=>{calls++;return new Promise(r=>{resolve=r;});},getDisplayMedia:async()=>{tabCalls++;return sharedTab().stream;}});
 const pending=p.elements.camera.onclick();await p.elements.camera.onclick();await p.elements['share-tab'].onclick();assert.equal(calls,1);assert.equal(tabCalls,0);
 p.elements.stop.onclick();resolve(camera.stream);await pending;assert.ok(camera.track.stops>=1);assert.ok(!p.sent.some(x=>x.route==='/start'||x.route==='/frame'));assert.equal(p.elements.camera.disabled,false);
});

test('camera tracks stop on mute, end, file change and page exit',async()=>{
 for(const reason of ['mute','ended','file','pagehide']){
  const c=sharedTab('camera'),p=player({getUserMedia:async()=>c.stream});await p.elements.camera.onclick();await p.frame(.1);
  if(reason==='file')p.elements.file.onchange({target:{files:[]}});else if(reason==='pagehide')p.windowEvents.pagehide();else c.listeners[reason]();
  await new Promise(r=>setImmediate(r));assert.ok(c.track.stops>=1);assert.equal(p.elements.video.srcObject,null);assert.equal(p.elements.position.value,0);
  const n=p.sent.filter(x=>x.route==='/frame').length;await p.frame(.2);assert.equal(p.sent.filter(x=>x.route==='/frame').length,n);
 }
});

test('late camera rejection and replaced-track events cannot stop newer input',async()=>{
 let reject;const p=player({getUserMedia:()=>new Promise((_,r)=>{reject=r;})});const pending=p.elements.camera.onclick();p.elements.file.onchange({target:{files:[]}});p.play();reject(Error('late denied'));await pending;assert.equal(p.elements.video.paused,false);await p.frame(.1);assert.equal(JSON.parse(Buffer.from(p.sent.find(x=>x.route==='/start').body)).source_time_basis,'source_media_presentation');
 const camera=sharedTab('camera'),tab=sharedTab(),q=player({getUserMedia:async()=>camera.stream,getDisplayMedia:async()=>tab.stream});await q.elements.camera.onclick();await q.elements['share-tab'].onclick();camera.listeners.ended();assert.equal(q.elements.video.srcObject,tab.stream);await q.frame(.1);assert.equal(JSON.parse(Buffer.from(q.sent.find(x=>x.route==='/start').body)).source_time_basis,'display_capture_presentation');
});

test('failed camera play and missing frame callbacks are inert',async()=>{
 const camera=sharedTab('camera'),p=player({getUserMedia:async()=>camera.stream});p.elements.video.play=async()=>{p.play();throw Error('play rejected');};await p.elements.camera.onclick();assert.ok(camera.track.stops>=1);assert.ok(!p.sent.some(x=>x.route==='/start'||x.route==='/frame'));
 let calls=0;const q=player({getUserMedia:async()=>{calls++;return camera.stream;},frameCallbacks:false});await q.elements.camera.onclick();assert.equal(calls,0);assert.equal(q.elements.camera.disabled,true);assert.equal(q.elements['share-tab'].disabled,true);
 const unsupported=player();await unsupported.elements.camera.onclick();assert.equal(unsupported.sent.length,0);
});


test('delayed frame response cannot display an estimate after its freshness budget',async()=>{
 const p=player({holdFrameAt:1});p.play();const pending=p.frame(0);await new Promise(r=>setImmediate(r));p.advance(1001);p.releaseFrame();await pending;
 assert.equal(p.elements.position.value,0);assert.match(p.elements.status.textContent,/stale|waiting/i);
});

test('slow Start is included in the sampled frame age',async()=>{
 const p=player({holdStart:true});p.play();const pending=p.frame(0);await new Promise(r=>setImmediate(r));p.advance(1001);p.release();await pending;
 assert.equal(p.elements.position.value,0);
});

test('display expires without further video frames or network responses',async()=>{
 const p=player();p.play();await p.frame(.125);assert.equal(p.elements.position.value,.6);
 p.advance(874);assert.equal(p.elements.position.value,.6);p.advance(1);assert.equal(p.elements.position.value,0);assert.match(p.elements.status.textContent,/stale|waiting/i);
});

test('busy frame callbacks still reveal a source-stale late response',async()=>{
 const p=player({holdFrameAt:1});p.play();const pending=p.frame(0);await new Promise(r=>setImmediate(r));await p.frame(2);p.releaseFrame();await pending;
 assert.equal(p.sent.filter(x=>x.route==='/frame').length,1);assert.equal(p.elements.position.value,0);
});

test('repeated estimate timestamps cannot extend or resurrect the display deadline',async()=>{
 const p=player();p.play();await p.frame(0);p.advance(800);await p.frame(0);assert.equal(p.elements.position.value,.6);
 p.advance(200);assert.equal(p.elements.position.value,0);await p.frame(0);assert.equal(p.elements.position.value,0);
});

test('a pending frame response cannot keep the previous estimate visible',async()=>{
 const p=player({holdFrameAt:2});p.play();await p.frame(0);p.advance(100);const pending=p.frame(.1);await new Promise(r=>setImmediate(r));
 p.advance(901);assert.equal(p.elements.position.value,0);p.releaseFrame();await pending;assert.equal(p.elements.position.value,0);
});

test('old display timers cannot clear replacement playback',async()=>{
 const p=player();p.play();await p.frame(0);p.advance(500);p.elements.stop.onclick();assert.equal(p.timerCount(),0);
 p.play();await p.frame(0);p.advance(500);assert.equal(p.elements.position.value,.6);assert.equal(p.timerCount(),1);
 p.advance(500);assert.equal(p.elements.position.value,0);assert.equal(p.timerCount(),0);
});


test('newer estimates recover after expiry and obsolete timer callbacks stay inert',async()=>{
 const reply=n=>({active:true,options:{policy:{maximum_analysis_age_ns:1e9}},inference:{completed:n,latest:{fresh:true,result:{payload:{position:.4,input:{pair:{current_time:{numerator:n===1?0:1200,denominator:1000}}}}}}}});
 const p=player({frameReply:reply});p.play();await p.frame(0);const oldTimer=p.pendingTimers()[0];p.advance(1100);assert.equal(p.elements.position.value,0);
 p.advance(100);await p.frame(1.2);assert.equal(p.elements.position.value,.4);oldTimer();assert.equal(p.elements.position.value,.4);
 p.advance(999);assert.equal(p.elements.position.value,.4);p.advance(1);assert.equal(p.elements.position.value,0);
});

test('presentation advances clear the displayed estimate while another request is pending',async()=>{
 const p=player({holdFrameAt:2});p.play();await p.frame(0);const pending=p.frame(.1);await new Promise(r=>setImmediate(r));
 await p.frame(2);assert.equal(p.elements.position.value,0);assert.equal(p.timerCount(),0);p.releaseFrame();await pending;assert.equal(p.elements.position.value,0);
});


test('missing or stale motion still reports the Engine inference state',async()=>{
 for(const state of ['starting','failed']){
  const p=player({frameReply:{active:true,options:{policy:{maximum_analysis_age_ns:1e9}},inference:{state,completed:0,latest:null}}});p.play();await p.frame(0);
  assert.equal(p.elements.position.value,0);assert.match(p.elements.status.textContent,new RegExp('Inference: '+state));
 }
});

const monitoredGuard={configured:true,pending:false,current:true};
const challenge=n=>({sequence:n,challenge_id:'challenge-'+n});
const jsonBody=request=>JSON.parse(Buffer.from(request.body));

test('player registration is explicit, page scoped and precedes capture registration',async()=>{
 const p=player();assert.equal(p.sent.length,0);p.play();await p.frame(.125);
 assert.equal(p.sent[0].route,'/player/start');
 const registration=jsonBody(p.sent[0]);assert.equal(registration.time_basis,'source_media_presentation');assert.ok(registration.generation>0);assert.match(registration.source_id,/^[0-9a-f-]{36}$/);
 for(const request of p.sent){assert.equal(request.headers['X-Pulsar-Launch'],defaultPageScope.launch_id);assert.equal(request.headers['X-Pulsar-Page'],defaultPageScope.page_id);assert.equal(request.headers['X-Pulsar-Page-Capability'],defaultPageScope.page_capability);}
 for(const route of ['/start','/frame','/player/challenge']){const request=p.sent.find(r=>r.route===route);assert.equal(request.headers['X-Pulsar-Player'],'player-1');assert.equal(request.headers['X-Pulsar-Source'],registration.source_id);assert.equal(request.headers['X-Pulsar-Generation'],String(registration.generation));assert.equal(request.headers['X-Pulsar-Player-Capability'],'player-capability-1');}
 assert.equal(p.sent[0].headers['X-Pulsar-Player'],undefined);assert.equal(p.sent.filter(r=>r.route==='/player/observe').length,0);
 assert.match(p.elements['guard-status'].textContent,/no device linkage configured/);
});

test('missing injected page scope fails closed before any request',async()=>{
 const p=player({pageScope:null});p.play();await p.frame(.1);await p.elements['candidate-status'].onclick();await flush();
 assert.equal(p.sent.length,0);assert.equal(p.elements.video.paused,true);assert.match(p.elements.status.textContent,/native page scope/);
});

test('independent heartbeat and native stop bypass a stalled capture Start',async()=>{
 const p=player({holdStart:true,guard:monitoredGuard,challengeReply:challenge});p.play();const pending=p.frame(.125);await flush();
 assert.ok(p.sent.some(r=>r.route==='/start'));assert.ok(p.sent.some(r=>r.route==='/player/observe'));
 p.advance(100);await p.frame(.25);await flush();
 const observed=jsonBody(p.sent.filter(r=>r.route==='/player/observe').at(-1));assert.deepEqual(observed.sample,{browser_tick_us:100000,media_time_us:250000,presented_frames:2,playing:true,seeking:false,ended:false,visible:true,playback_rate_milli:1000});
 p.elements.stop.onclick();assert.equal(p.sent.at(-1).route,'/player/stop');assert.equal(p.sent.filter(r=>r.route==='/stop').length,0);
 p.release();await pending;await flush();assert.equal(p.sent.filter(r=>r.route==='/frame').length,0);assert.ok(p.sent.some(r=>r.route==='/stop'));
});

test('independent heartbeat and native stop bypass a stalled frame and queued editor read',async()=>{
 const p=player({holdFrameAt:1,guard:monitoredGuard,challengeReply:challenge,candidateReplies:{'/candidate-status':{request_id:'old-intent'}}});p.play();const pending=p.frame(.1);await flush();
 const editor=p.elements['candidate-status'].onclick();p.advance(100);await p.frame(.2);await flush();assert.equal(p.sent.filter(r=>r.route==='/candidate-status').length,0);assert.equal(jsonBody(p.sent.filter(r=>r.route==='/player/observe').at(-1)).sample.media_time_us,200000);
 p.elements.stop.onclick();assert.equal(p.sent.at(-1).route,'/player/stop');p.play();await flush();p.elements['candidate-message'].textContent='Replacement playback';
 p.releaseFrame();await pending;await editor;await flush();
 const oldRead=p.sent.find(r=>r.route==='/candidate-status');assert.equal(oldRead.headers['X-Pulsar-Player'],'player-1');assert.equal(p.elements['candidate-message'].textContent,'Replacement playback');
 await p.frame(.3);assert.equal(p.sent.filter(r=>r.route==='/frame').at(-1).headers['X-Pulsar-Player'],'player-2');
});

test('stalled editor preparation cannot block player control or continue under a new scope',async()=>{
 const p=player({holdRoute:'/prepare-candidate',guard:monitoredGuard,challengeReply:challenge,candidateReplies:{'/prepare-candidate':{request_id:'retained-before-play'},'/stage-candidate':stagedReply}});p.elements['recording-selection'].value='epoch-one';
 const editor=p.elements['stage-candidate'].onclick();await flush();assert.equal(p.sent[0].route,'/prepare-candidate');assert.equal(p.sent[0].headers['X-Pulsar-Player'],undefined);
 p.play();const pending=p.frame(.1);await flush();assert.ok(p.sent.some(r=>r.route==='/player/observe'));assert.equal(p.sent.filter(r=>r.route==='/start').length,0);
 p.advance(100);await p.frame(.2);await flush();assert.ok(p.sent.filter(r=>r.route==='/player/observe').length>=2);
 p.elements.stop.onclick();assert.equal(p.sent.at(-1).route,'/player/stop');p.elements['candidate-message'].textContent='Stopped playback';p.releaseRoute();await editor;await pending;await flush();
 assert.equal(p.sent.filter(r=>r.route==='/stage-candidate').length,0);assert.equal(p.elements['candidate-message'].textContent,'Stopped playback');
});

test('retained editor actions after stop keep the exact retired player scope',async()=>{
 const p=player({candidateReplies:{'/prepare-candidate':{request_id:'retained-stage'},'/stage-candidate':stagedReply}});p.play();await p.frame(.1);p.elements.stop.onclick();await flush();p.elements['recording-selection'].value='epoch-one';
 await p.elements['stage-candidate'].onclick();
 const original=p.sent.find(r=>r.route==='/frame').headers;
 for(const request of p.sent.filter(r=>r.route==='/prepare-candidate'||r.route==='/stage-candidate'))assert.deepEqual(request.headers,original);
 assert.match(p.elements['candidate-details'].textContent,/original-stage/);
});

test('binding pending and unconfigured guards send local heartbeats without fake observations',async()=>{
 for(const configured of [false,true]){
  const p=player({guard:{configured,pending:configured,current:false},challengeReply:{pending:true,configured}});p.play();await p.frame(.1);p.advance(100);await flush();
  assert.ok(p.sent.filter(r=>r.route==='/player/challenge').length>=2);assert.equal(p.sent.filter(r=>r.route==='/player/observe').length,0);
  assert.match(p.elements['guard-status'].textContent,configured?/binding pending/:/no device linkage/);
 }
});

test('accepted observations describe a stop guard without synchronization or device-clock claims',async()=>{
 const p=player({guard:monitoredGuard,challengeReply:challenge});p.play();await p.frame(.1);await flush();
 const observation=jsonBody(p.sent.find(r=>r.route==='/player/observe'));
 assert.deepEqual(Object.keys(observation).sort(),['challenge_id','sample','sequence']);assert.ok(!('device_time_us' in observation.sample));assert.ok(!('boottime_ns' in observation.sample));assert.match(p.elements['guard-status'].textContent,/Synchronization and physical stopping remain unconfirmed/);assert.doesNotMatch(p.elements['guard-status'].textContent,/synced|synchronized|calibrated/i);
});

test('fresh challenge response samples flags and browser clock at response time',async()=>{
 const p=player({holdRoute:'/player/challenge',guard:monitoredGuard,challengeReply:challenge});p.play();await p.frame(.125);p.advance(37);Object.assign(p.elements.video,{paused:true,seeking:true,ended:true,playbackRate:.5});p.document.visibilityState='hidden';p.releaseRoute();await flush();
 const sample=jsonBody(p.sent.find(r=>r.route==='/player/observe')).sample;assert.deepEqual(sample,{browser_tick_us:37000,media_time_us:125000,presented_frames:1,playing:false,seeking:true,ended:true,visible:false,playback_rate_milli:500});
});

test('late registration after pagehide releases only its returned identity and never arms capture',async()=>{
 const p=player({holdRoute:'/player/start'});p.play();const pending=p.frame(.1);await flush();p.windowEvents.pagehide();p.releaseRoute();await pending;await flush();
 assert.equal(p.sent.filter(r=>r.route==='/start'||r.route==='/frame').length,0);const release=p.sent.find(r=>r.route==='/player/stop');assert.equal(release.headers['X-Pulsar-Player'],'player-1');assert.equal(release.headers['X-Pulsar-Source'],jsonBody(p.sent[0]).source_id);assert.equal(p.elements.position.value,0);
});

test('pagehide bypasses blocked work with keepalive and retains the original page and player',async()=>{
 const old=player({holdFrameAt:1});old.play();const pending=old.frame(.1);await flush();
 const replacement=player({pageScope:{launch_id:'launch-one',page_id:'page-two',page_capability:'page-capability-two'}});replacement.play();await replacement.frame(.1);
 old.windowEvents.pagehide();
 for(const route of ['/player/stop','/stop']){const release=old.sent.find(r=>r.route===route);assert.equal(release.keepalive,true);assert.equal(release.headers['X-Pulsar-Page'],'page-one');assert.equal(release.headers['X-Pulsar-Player'],'player-1');assert.equal(release.headers['X-Pulsar-Source'],jsonBody(old.sent[0]).source_id);}
 assert.equal(replacement.elements.position.value,.6);assert.equal(replacement.sent.filter(r=>r.route==='/player/stop').length,0);old.releaseFrame();await pending;assert.equal(old.elements.position.value,0);
});

test('late failed heartbeat cannot stop replacement playback or overwrite its display',async()=>{
 const p=player({holdRoute:'/player/challenge',replyOverride:request=>request.route==='/player/challenge'&&request.headers['X-Pulsar-Player']==='player-1'?{ok:false,json:async()=>({error:'old player retired'})}:null});p.play();await p.frame(.1);p.elements.stop.onclick();p.play();await p.frame(.1);assert.equal(p.elements.position.value,.6);const message=p.elements.status.textContent;
 p.releaseRoute();await flush();assert.equal(p.elements.position.value,.6);assert.equal(p.elements.status.textContent,message);assert.equal(p.sent.filter(r=>r.route==='/player/stop').length,1);assert.equal(p.elements.video.paused,false);
});

test('hidden pages release exact player scope and becoming visible does not rearm',async()=>{
 const p=player();p.play();await p.frame(.1);const original=p.sent.find(r=>r.route==='/frame').headers;
 p.document.visibilityState='hidden';p.documentEvents.visibilitychange();assert.deepEqual(p.sent.find(r=>r.route==='/player/stop').headers,original);assert.equal(p.elements.position.value,0);p.document.visibilityState='visible';p.documentEvents.visibilitychange();await p.frame(.2);assert.equal(p.sent.filter(r=>r.route==='/player/start').length,1);
 p.play();await p.frame(.2);const registrations=p.sent.filter(r=>r.route==='/player/start').map(jsonBody);assert.equal(registrations.length,2);assert.notEqual(registrations[0].source_id,registrations[1].source_id);assert.ok(registrations[1].generation>registrations[0].generation);
});

test('retired observation response stops only its active player and does not rearm',async()=>{
 const p=player({guard:monitoredGuard,challengeReply:challenge,replyOverride:request=>request.route==='/player/observe'?{ok:true,json:async()=>({current:false,synchronized:false})}:null});p.play();await p.frame(.1);await flush();assert.equal(p.elements.position.value,0);assert.ok(p.sent.some(r=>r.route==='/player/stop'));assert.equal(p.elements.video.paused,true);p.advance(1000);await flush();assert.equal(p.sent.filter(r=>r.route==='/player/start').length,1);
});

const sourceCatalog={catalog_generation:'catalog-one',entries:[{entry_id:'entry-one',label:'Imported movie',source_version:'source-version-one',placement_id:'placement-one',sha256:'a'.repeat(64),byte_len:'9007199254740993'}]};
const readyMedia={selection_id:'selection-one',preparation_request_id:'original-media-request',state:'ready',current:true,url:'/media/token-one',trim_start_us:0,trim_end_us:2000000};
const cancelledMedia={selection_id:'selection-one',state:'cancelled',current:false};
function mediaPlayer(options={}){return player({...options,candidateReplies:{'/media/catalog':sourceCatalog,'/media/prepare':readyMedia,'/media/status':readyMedia,'/media/release':cancelledMedia,...options.candidateReplies}});}
async function chooseMedia(p){await p.elements['refresh-media'].onclick();p.elements['media-clock-ack'].checked=true;}
async function prepareMedia(p){await chooseMedia(p);await p.elements['prepare-media'].onclick();}

test('Engine catalog displays exact string metadata and preparation requires explicit clock acknowledgement',async()=>{
 const p=mediaPlayer();await p.elements['refresh-media'].onclick();
 assert.match(p.elements['media-details'].textContent,/9007199254740993/);assert.match(p.elements['media-details'].textContent,/placement-one/);assert.equal(p.elements['media-selection'].children.length,1);
 await p.elements['prepare-media'].onclick();assert.equal(p.sent.filter(r=>r.route==='/media/prepare').length,0);assert.match(p.elements['media-message'].textContent,/acknowledge/i);
 p.elements['media-clock-ack'].checked=true;await p.elements['prepare-media'].onclick();
 const preparation=p.sent.find(r=>r.route==='/media/prepare');assert.deepEqual(jsonBody(preparation),{catalog_generation:'catalog-one',entry_id:'entry-one',presentation_anchor_us:'0',elapsed_anchor_us:'0',allow_declared_clock:true});assert.equal(preparation.headers['X-Pulsar-Page'],defaultPageScope.page_id);assert.equal(preparation.headers['X-Pulsar-Player'],undefined);
 assert.equal(p.sent.filter(r=>r.route==='/player/start'||r.route==='/start'||r.route==='/frame').length,0);assert.equal(p.elements.video.src,'/media/token-one');assert.equal(p.elements.video.paused,true);assert.match(p.elements['media-message'].textContent,/not calibrated or synchronized/);
});

test('clock anchors remain exact decimal strings above the JS safe integer range',async()=>{
 const p=mediaPlayer();await chooseMedia(p);p.elements['media-presentation-anchor'].value='9007199254740993';p.elements['media-elapsed-anchor'].value=' 00012 ';await p.elements['prepare-media'].onclick();
 const body=jsonBody(p.sent.find(r=>r.route==='/media/prepare'));assert.equal(body.presentation_anchor_us,'9007199254740993');assert.equal(body.elapsed_anchor_us,'12');assert.equal(typeof body.presentation_anchor_us,'string');
 for(const anchor of ['-1','0.1','1e6','9223372036854775808','']){const q=mediaPlayer();await chooseMedia(q);q.elements['media-elapsed-anchor'].value=anchor;await q.elements['prepare-media'].onclick();assert.equal(q.sent.filter(r=>r.route==='/media/prepare').length,0);}
});

test('changing an anchor or catalog selection clears its prior clock acknowledgement',async()=>{
 const p=mediaPlayer();await chooseMedia(p);p.elements['media-presentation-anchor'].oninput();assert.equal(p.elements['media-clock-ack'].checked,false);await p.elements['prepare-media'].onclick();assert.equal(p.sent.filter(r=>r.route==='/media/prepare').length,0);
 p.elements['media-clock-ack'].checked=true;p.elements['media-selection'].onchange();assert.equal(p.elements['media-clock-ack'].checked,false);
});

test('prepared Play binds only the native selection ID while capture request body stays unchanged',async()=>{
 const p=mediaPlayer();await prepareMedia(p);p.play();await p.frame(.125);
 const registration=jsonBody(p.sent.find(r=>r.route==='/player/start'));assert.equal(registration.source_id,'selection-one');assert.equal(registration.media_selection_id,'selection-one');assert.equal(registration.time_basis,'source_media_presentation');assert.ok(registration.generation>0);assert.equal(Object.hasOwn(registration,'binding'),false);
 assert.deepEqual(jsonBody(p.sent.find(r=>r.route==='/start')),{media_us:125000,source_time_basis:'source_media_presentation'});assert.equal(p.sent.find(r=>r.route==='/frame').headers['X-Pulsar-Source'],'selection-one');
});

test('prepared URL and source identity survive pause seek and explicit resume with fresh player generations',async()=>{
 const p=mediaPlayer();await prepareMedia(p);p.play();await p.frame(.1);p.elements.video.pause();assert.equal(p.elements.video.src,'/media/token-one');p.play();await p.frame(.2);p.events.seeking();assert.equal(p.elements.video.src,'/media/token-one');p.play();await p.frame(.3);
 const registrations=p.sent.filter(r=>r.route==='/player/start').map(jsonBody);assert.equal(registrations.length,3);assert.ok(registrations.every(r=>r.source_id==='selection-one'&&r.media_selection_id==='selection-one'));assert.ok(registrations[0].generation<registrations[1].generation&&registrations[1].generation<registrations[2].generation);assert.equal(p.sent.filter(r=>r.route==='/media/prepare').length,1);assert.equal(p.sent.filter(r=>r.route==='/media/release').length,0);
});

test('declared trim skips pretrim frames and stops at its half-open end without releasing bytes',async()=>{
 const p=mediaPlayer({candidateReplies:{'/media/prepare':{...readyMedia,trim_start_us:100000,trim_end_us:200000}}});await prepareMedia(p);p.play();await p.frame(.099999);assert.equal(p.sent.filter(r=>r.route==='/start'||r.route==='/frame').length,0);await p.frame(.1);assert.equal(p.sent.filter(r=>r.route==='/frame').length,1);await p.frame(.199999);assert.equal(p.sent.filter(r=>r.route==='/frame').length,2);await p.frame(.2);assert.equal(p.elements.video.paused,true);assert.equal(p.sent.filter(r=>r.route==='/frame').length,2);assert.equal(p.elements.video.src,'/media/token-one');assert.equal(p.sent.filter(r=>r.route==='/media/release').length,0);assert.match(p.elements.status.textContent,/trim ended/);
 p.seek(.1);await flush();p.play();await p.frame(.1);assert.equal(p.sent.filter(r=>r.route==='/player/start').length,2);
});

test('local file replacement revokes prepared selection and registers an explicitly unbound source',async()=>{
 const p=mediaPlayer();await prepareMedia(p);p.play();await p.frame(.1);p.elements.file.onchange({target:{files:[]}});p.play();await p.frame(.1);
 const release=p.sent.find(r=>r.route==='/media/release');assert.deepEqual(jsonBody(release),{selection_id:'selection-one'});assert.equal(release.headers['X-Pulsar-Player'],undefined);const registration=jsonBody(p.sent.filter(r=>r.route==='/player/start').at(-1));assert.notEqual(registration.source_id,'selection-one');assert.equal(Object.hasOwn(registration,'media_selection_id'),false);assert.equal(p.elements.video.src,'');
});

test('camera and tab replacement revoke Engine selection and preserve unbound capture basis',async()=>{
 for(const kind of ['camera','tab']){const shared=sharedTab(kind==='camera'?'camera':'browser');const p=mediaPlayer(kind==='camera'?{getUserMedia:async()=>shared.stream}:{getDisplayMedia:async()=>shared.stream});await prepareMedia(p);await p.elements[kind==='camera'?'camera':'share-tab'].onclick();await p.frame(.1);
  assert.equal(p.sent.filter(r=>r.route==='/media/release').length,1);const registration=jsonBody(p.sent.find(r=>r.route==='/player/start'));assert.equal(Object.hasOwn(registration,'media_selection_id'),false);assert.equal(registration.time_basis,kind==='camera'?'camera_capture_presentation':'display_capture_presentation');assert.equal(p.elements.video.srcObject,shared.stream);
 }
});

test('release failure removes the local URL but blocks fresh preparation until explicit release succeeds',async()=>{
 let releases=0;const p=mediaPlayer({replyOverride:request=>request.route==='/media/release'&&++releases===1?{ok:false,json:async()=>({error:'release outcome unknown'})}:null});await prepareMedia(p);await p.elements['release-media'].onclick();assert.equal(p.elements.video.src,undefined);await p.elements['prepare-media'].onclick();assert.equal(p.sent.filter(r=>r.route==='/media/prepare').length,1);
 await p.elements['release-media'].onclick();await p.elements['prepare-media'].onclick();assert.equal(p.sent.filter(r=>r.route==='/media/prepare').length,2);assert.equal(releases,2);
});

test('lost Prepare acknowledgement retains unresolved original and explicit Inspect uses null identity',async()=>{
 const p=mediaPlayer({replyOverride:request=>request.route==='/media/prepare'?Promise.reject(Error('lost acknowledgement')):null});await prepareMedia(p);await p.elements['prepare-media'].onclick();assert.equal(p.sent.filter(r=>r.route==='/media/prepare').length,1);assert.equal(p.elements.video.src,undefined);await p.elements['inspect-media'].onclick();assert.deepEqual(jsonBody(p.sent.find(r=>r.route==='/media/status')),{selection_id:null});assert.equal(p.elements.video.src,'/media/token-one');assert.equal(p.sent.filter(r=>r.route==='/media/prepare').length,1);assert.equal(p.sent.filter(r=>r.route==='/player/start').length,0);
});

test('explicit Release can recover a lost Prepare acknowledgement without a replacement preparation',async()=>{
 const p=mediaPlayer({replyOverride:request=>request.route==='/media/prepare'?Promise.reject(Error('lost acknowledgement')):null});await prepareMedia(p);await p.elements['release-media'].onclick();assert.deepEqual(jsonBody(p.sent.find(r=>r.route==='/media/release')),{selection_id:null});assert.match(p.elements['media-message'].textContent,/source released/);assert.equal(p.sent.filter(r=>r.route==='/media/prepare').length,1);
});

test('late Prepare Ready after file replacement releases exact original without replacing current video',async()=>{
 const p=mediaPlayer({holdRoute:'/media/prepare'});await chooseMedia(p);const pending=p.elements['prepare-media'].onclick();await flush();p.elements.file.onchange({target:{files:[]}});p.play();await p.frame(.1);const message=p.elements.status.textContent;p.releaseRoute();await pending;await flush();assert.equal(p.elements.video.src,'');assert.equal(p.elements.status.textContent,message);assert.equal(p.elements.position.value,.6);const releases=p.sent.filter(r=>r.route==='/media/release').map(jsonBody);assert.deepEqual(releases,[{selection_id:null},{selection_id:'selection-one'}]);assert.equal(p.sent.filter(r=>r.route==='/player/start').length,1);
});

test('late status from a released selection cannot publish its URL over a replacement',async()=>{
 let preparations=0;const second={...readyMedia,selection_id:'selection-two',url:'/media/token-two'};
 const p=mediaPlayer({holdRoute:'/media/status',replyOverride:request=>{
  if(request.route==='/media/prepare')return {ok:true,json:async()=>++preparations===1?{selection_id:'selection-one',state:'pending',current:false}:second};
  if(request.route==='/media/release')return {ok:true,json:async()=>({selection_id:jsonBody(request).selection_id,state:'cancelled',current:false})};
  return null;
 }});await prepareMedia(p);const pending=p.elements['inspect-media'].onclick();await flush();await p.elements['release-media'].onclick();await p.elements['prepare-media'].onclick();assert.equal(p.elements.video.src,'/media/token-two');p.releaseRoute();await pending;assert.equal(p.elements.video.src,'/media/token-two');assert.equal(jsonBody(p.sent.filter(r=>r.route==='/media/release').at(-1)).selection_id,'selection-one');p.play();await p.frame(.1);assert.equal(jsonBody(p.sent.find(r=>r.route==='/player/start')).media_selection_id,'selection-two');
});

test('pending media polling bypasses blocked inference and never starts playback automatically',async()=>{
 const p=mediaPlayer({holdStart:true,candidateReplies:{'/media/prepare':{selection_id:'selection-one',state:'pending',current:false}}});p.play();const oldFrame=p.frame(.1);await flush();await prepareMedia(p);assert.ok(p.sent.some(r=>r.route==='/player/stop'));p.advance(200);await flush();assert.ok(p.sent.some(r=>r.route==='/media/status'));assert.equal(p.elements.video.src,'/media/token-one');assert.equal(p.elements.video.paused,true);assert.equal(p.sent.filter(r=>r.route==='/player/start').length,1);p.release();await oldFrame;assert.equal(p.sent.filter(r=>r.route==='/frame').length,0);
});

test('pagehide revokes media through exact keepalive and a late Ready remains inert',async()=>{
 const p=mediaPlayer({holdRoute:'/media/status',candidateReplies:{'/media/prepare':{selection_id:'selection-one',state:'pending',current:false}}});await prepareMedia(p);const pending=p.elements['inspect-media'].onclick();await flush();p.windowEvents.pagehide();const release=p.sent.find(r=>r.route==='/media/release');assert.equal(release.keepalive,true);assert.deepEqual(jsonBody(release),{selection_id:'selection-one'});p.releaseRoute();await pending;assert.equal(p.elements.video.src,undefined);assert.equal(p.sent.filter(r=>r.route==='/player/start').length,0);assert.ok(p.sent.filter(r=>r.route==='/media/release').every(r=>r.keepalive));
});

test('noncurrent native selection removes its URL and preserves original identity for release',async()=>{
 const p=mediaPlayer({candidateReplies:{'/media/status':{selection_id:'selection-one',state:'ready',current:false}}});await prepareMedia(p);p.play();await p.frame(.1);await p.elements['inspect-media'].onclick();assert.equal(p.elements.video.src,undefined);assert.equal(p.elements.video.paused,true);assert.ok(p.sent.some(r=>r.route==='/player/stop'));await p.elements['prepare-media'].onclick();assert.equal(p.sent.filter(r=>r.route==='/media/prepare').length,1);await p.elements['release-media'].onclick();assert.equal(jsonBody(p.sent.find(r=>r.route==='/media/release')).selection_id,'selection-one');
});

test('foreign URLs unsafe trim endpoints and changed selection identities never authorize playback',async()=>{
 for(const invalid of [{url:'https://example.invalid/media/token'},{trim_end_us:Number.MAX_SAFE_INTEGER+1},{trim_start_us:2000000,trim_end_us:2000000}]){const p=mediaPlayer({candidateReplies:{'/media/prepare':{...readyMedia,...invalid}}});await prepareMedia(p);p.play();await p.frame(.1);assert.equal(p.elements.video.src,undefined);assert.equal(p.sent.filter(r=>r.route==='/player/start').length,0);assert.equal(p.sent.filter(r=>r.route==='/media/prepare').length,1);}
 const p=mediaPlayer({candidateReplies:{'/media/prepare':{selection_id:'selection-one',state:'pending',current:false},'/media/status':{...readyMedia,selection_id:'selection-wrong'}}});await prepareMedia(p);await p.elements['inspect-media'].onclick();assert.equal(p.elements.video.src,undefined);assert.match(p.elements['media-message'].textContent,/identity changed/);
});

test('failed status polling keeps the original preparation and waits for explicit inspection',async()=>{
 let statusCalls=0;const p=mediaPlayer({candidateReplies:{'/media/prepare':{selection_id:'selection-one',state:'pending',current:false}},replyOverride:request=>request.route==='/media/status'&&++statusCalls===1?{ok:false,json:async()=>({error:'status unavailable'})}:null});await prepareMedia(p);p.advance(200);await flush();assert.equal(statusCalls,1);p.advance(1000);await flush();assert.equal(statusCalls,1);await p.elements['prepare-media'].onclick();assert.equal(p.sent.filter(r=>r.route==='/media/prepare').length,1);await p.elements['inspect-media'].onclick();assert.equal(statusCalls,2);assert.equal(p.elements.video.src,'/media/token-one');assert.ok(p.sent.filter(r=>r.route==='/media/status').every(r=>jsonBody(r).selection_id==='selection-one'));
});

test('older preparation response cannot replace newer inspected status for the same selection',async()=>{
 const p=mediaPlayer({holdRoute:'/media/prepare',candidateReplies:{'/media/prepare':{selection_id:'selection-one',state:'pending',current:false}}});await chooseMedia(p);const preparing=p.elements['prepare-media'].onclick();await flush();await p.elements['inspect-media'].onclick();assert.equal(p.elements.video.src,'/media/token-one');p.releaseRoute();await preparing;assert.equal(p.elements.video.src,'/media/token-one');p.advance(500);await flush();assert.equal(p.sent.filter(r=>r.route==='/media/status').length,1);assert.equal(p.sent.filter(r=>r.route==='/media/release').length,0);
});

test('bound player registration waits for metadata and initial trim seek while capture anchors only the first eligible callback',async()=>{
 const p=mediaPlayer({mediaMetadata:false,mediaSeek:false,candidateReplies:{'/media/prepare':{...readyMedia,trim_start_us:1000000,trim_end_us:3000000}}});await prepareMedia(p);assert.equal(p.elements.video.readyState,0);assert.equal(p.elements.video.currentTime,0);p.play();assert.equal(p.sent.filter(r=>r.route==='/player/start').length,0);assert.equal(p.elements.video.paused,true);
 p.metadata('/media/obsolete-token');assert.equal(p.elements.video.currentTime,0);p.play();assert.equal(p.sent.filter(r=>r.route==='/player/start').length,0);
 p.metadata();assert.equal(p.elements.video.currentTime,1);assert.equal(p.elements.video.seeking,true);p.play();assert.equal(p.sent.filter(r=>r.route==='/player/start').length,0);p.settleSeek();assert.equal(p.sent.filter(r=>r.route==='/player/start'||r.route==='/start'||r.route==='/frame').length,0);
 p.play();await flush();assert.equal(p.sent.filter(r=>r.route==='/player/start').length,1);assert.equal(p.sent.filter(r=>r.route==='/start').length,0);await p.frame(1.25);const start=jsonBody(p.sent.find(r=>r.route==='/start'));assert.equal(start.media_us,1250000);assert.notEqual(start.media_us,1000000);assert.deepEqual(start,{media_us:1250000,source_time_basis:'source_media_presentation'});const frame=Buffer.from(p.sent.find(r=>r.route==='/frame').body);assert.equal(JSON.parse(frame.subarray(0,frame.indexOf(10))).media_us,1250000);assert.equal(jsonBody(p.sent.find(r=>r.route==='/media/prepare')).presentation_anchor_us,'0');
});

test('obsolete metadata and seek completion cannot settle or reposition a replacement Engine selection',async()=>{
 let preparations=0;const p=mediaPlayer({mediaMetadata:false,mediaSeek:false,replyOverride:request=>{
  if(request.route==='/media/prepare')return {ok:true,json:async()=>++preparations===1?{...readyMedia,trim_start_us:1000000,trim_end_us:3000000}:{...readyMedia,selection_id:'selection-two',url:'/media/token-two',trim_start_us:2000000,trim_end_us:4000000}};
  if(request.route==='/media/release')return {ok:true,json:async()=>({selection_id:jsonBody(request).selection_id,state:'cancelled',current:false})};return null;
 }});await prepareMedia(p);const oldMetadata=p.events.loadedmetadata,oldSeeked=p.events.seeked;await p.elements['release-media'].onclick();await p.elements['prepare-media'].onclick();p.metadata();assert.equal(p.elements.video.currentTime,2);oldMetadata();oldSeeked();assert.equal(p.elements.video.currentTime,2);assert.equal(p.elements.video.src,'/media/token-two');p.play();assert.equal(p.sent.filter(r=>r.route==='/player/start').length,0);p.settleSeek();p.play();await p.frame(2.125);assert.equal(jsonBody(p.sent.find(r=>r.route==='/player/start')).media_selection_id,'selection-two');assert.equal(jsonBody(p.sent.find(r=>r.route==='/start')).media_us,2125000);
});

test('bound Play outside trim refuses registration and manual seek inside trim never auto-rearms',async()=>{
 const p=mediaPlayer({candidateReplies:{'/media/prepare':{...readyMedia,trim_start_us:100000,trim_end_us:200000}}});await prepareMedia(p);assert.equal(p.elements.video.currentTime,.1);p.play();await p.frame(.12);p.seek(.05);await flush();p.play();assert.equal(p.sent.filter(r=>r.route==='/player/start').length,1);assert.equal(p.elements.video.paused,true);assert.match(p.elements.status.textContent,/Seek inside/);assert.equal(p.elements.video.currentTime,.05);
 p.seek(.15);await flush();assert.equal(p.sent.filter(r=>r.route==='/player/start').length,1);assert.equal(p.elements.video.paused,true);p.play();await p.frame(.16);assert.equal(p.sent.filter(r=>r.route==='/player/start').length,2);assert.equal(jsonBody(p.sent.filter(r=>r.route==='/start').at(-1)).media_us,160000);assert.equal(p.elements.video.src,'/media/token-one');assert.equal(p.sent.filter(r=>r.route==='/media/release').length,0);
 p.seek(.2);await flush();p.play();assert.equal(p.sent.filter(r=>r.route==='/player/start').length,2);assert.equal(p.elements.video.paused,true);
});

test('explicit null release accepts confirmed nondispatch after a rejected preparation without fabricating source identity',async()=>{
 let preparations=0;const p=mediaPlayer({replyOverride:request=>{
  if(request.route==='/media/prepare'&&++preparations===1)return {ok:false,json:async()=>({error:'catalog no longer current'})};
  if(request.route==='/media/release')return {ok:true,json:async()=>({selection_id:null,state:'cancelled',current:false,not_dispatched:true})};return null;
 }});await prepareMedia(p);await p.elements['release-media'].onclick();assert.deepEqual(jsonBody(p.sent.find(r=>r.route==='/media/release')),{selection_id:null});assert.equal(p.elements.video.src,undefined);assert.match(p.elements['media-message'].textContent,/source released/);await p.elements['prepare-media'].onclick();p.play();await p.frame(.1);assert.equal(p.sent.filter(r=>r.route==='/media/prepare').length,2);assert.equal(jsonBody(p.sent.find(r=>r.route==='/player/start')).source_id,'selection-one');
});

test('nondispatch null release cannot discard a known native selection',async()=>{
 const p=mediaPlayer({candidateReplies:{'/media/release':{selection_id:null,state:'cancelled',current:false,not_dispatched:true}}});await prepareMedia(p);await p.elements['release-media'].onclick();assert.deepEqual(jsonBody(p.sent.find(r=>r.route==='/media/release')),{selection_id:'selection-one'});await p.elements['prepare-media'].onclick();assert.equal(p.sent.filter(r=>r.route==='/media/prepare').length,1);assert.equal(p.elements.video.src,undefined);
});


test('queued programmatic pause preserves primary analysis error and adds cleanup uncertainty',async()=>{
 const p=player({queuedPause:true,replyOverride:r=>r.route==='/frame'?{ok:false,json:async()=>({error:'prepared successor could not be confirmed'})}:r.route==='/stop'?{ok:false,json:async()=>({error:'cleanup reply lost'})}:null});
 p.play();await p.frame(.1);await flush();
 p.deliverPauses();await flush();
 assert.match(p.elements.status.textContent,/Analysis stopped: prepared successor could not be confirmed/);
 assert.match(p.elements.status.textContent,/Stop could not be confirmed: cleanup reply lost/);
 assert.equal(p.sent.filter(r=>r.route==='/player/start').length,1);
 assert.equal(p.sent.filter(r=>r.route==='/stop').length,1);
});
test('queued pause preserves Engine retirement reason',async()=>{
 const p=player({queuedPause:true,frameReply:{active:false,retirement:'worker_failed',inference:{error:{message:'synthetic worker failure'}}}});
 p.play();await p.frame(.1);p.deliverPauses();await flush();
 assert.match(p.elements.status.textContent,/worker_failed/);
 assert.match(p.elements.status.textContent,/synthetic worker failure/);
 assert.equal(p.elements.video.paused,true);
 assert.equal(p.sent.filter(r=>r.route==='/stop').length,1);
});
test('old queued programmatic pause cannot retire explicit replacement playback',async()=>{
 const p=player({queuedPause:true});p.play();await p.frame(.1);
 p.elements.stop.onclick();p.play();await p.frame(.2);
 const message=p.elements.status.textContent;
 p.deliverPauses();await flush();await p.frame(.3);
 assert.equal(p.elements.video.paused,false);
 assert.equal(p.elements.status.textContent,message);
 assert.equal(p.sent.filter(r=>r.route==='/player/start').length,2);
 assert.equal(p.sent.filter(r=>r.route==='/frame').length,3);
 assert.equal(p.sent.filter(r=>r.route==='/stop').length,1);
});
test('genuine queued user pause still retires current playback',async()=>{
 const p=player({queuedPause:true});p.play();await p.frame(.1);
 p.elements.video.pause();p.deliverPauses();await flush();
 assert.match(p.elements.status.textContent,/Preview stopped: pause/);
 assert.equal(p.elements.position.value,0);
 assert.equal(p.sent.filter(r=>r.route==='/stop').length,1);
});
test('queued pause preserves seek and hidden-page stop reasons',async()=>{
 for(const reason of ['seek','hidden']){
  const p=player({queuedPause:true});p.play();await p.frame(.1);
  if(reason==='seek')p.events.seeking();else{p.document.visibilityState='hidden';p.documentEvents.visibilitychange();}
  p.deliverPauses();await flush();
  assert.match(p.elements.status.textContent,reason==='seek'?/Seek stopped analysis/:/Hidden page stopped analysis/);
  assert.equal(p.sent.filter(r=>r.route==='/stop').length,1);
 }
});


test('native timing uses one fresh callback atom and displays only an unqualified bracket and declared phase',async()=>{
 const p=mediaPlayer({timing:true});await prepareMedia(p);p.play();await flush();p.advance(10);
 await p.frame(.1,1,{presentationTime:8,expectedDisplayTime:12});await flush();
 const request=p.sent.find(r=>r.route==='/player/timing-observe');assert.ok(request);
 assert.deepEqual(jsonBody(request),{sequence:'1',probe_id:'probe-1',frame:{sample:{browser_tick_us:10000,media_time_us:100000,presented_frames:1,playing:true,seeking:false,ended:false,visible:true,playback_rate_milli:1000},callback_tick_us:10000,presentation_time_us:8000,expected_display_time_us:12000}});
 assert.match(p.elements['timing-status'].textContent,/10000000 ns/);assert.match(p.elements['timing-status'].textContent,/1\/2 s/);assert.match(p.elements['timing-status'].textContent,/Unqualified/);
 assert.ok(!p.sent.some(r=>Buffer.from(r.body).toString().includes('boottime')));
});
test('timing ignores cached or repeated frame counters after a delayed nonce reply',async()=>{
 const p=mediaPlayer({timing:true,holdRoute:'/player/timing-probe'});await prepareMedia(p);p.play();await p.frame(.1,1);await flush();
 assert.equal(p.sent.filter(r=>r.route==='/player/timing-observe').length,0);
 p.releaseRoute();await flush();await p.frame(.1,1);assert.equal(p.sent.filter(r=>r.route==='/player/timing-observe').length,0);
 p.advance(1);await p.frame(.2,2);await flush();assert.equal(p.sent.filter(r=>r.route==='/player/timing-observe').length,1);
 assert.equal(jsonBody(p.sent.find(r=>r.route==='/player/timing-observe')).frame.sample.media_time_us,200000);
});
test('timing observation lane continues while original inference Start is held',async()=>{
 const p=mediaPlayer({timing:true,holdStart:true});await prepareMedia(p);p.play();await flush();
 const pending=p.frame(.1);await flush();assert.equal(p.sent.filter(r=>r.route==='/player/timing-observe').length,1);
 p.advance(250);await flush();await p.frame(.2);await flush();
 assert.equal(p.sent.filter(r=>r.route==='/player/timing-observe').length,2);assert.equal(p.sent.filter(r=>r.route==='/frame').length,0);
 p.release();await pending;
});
test('old timing observation reply cannot overwrite or restart replacement playback',async()=>{
 const p=mediaPlayer({timing:true,holdRoute:'/player/timing-observe'});await prepareMedia(p);p.play();await flush();await p.frame(.1);await flush();
 const old=p.sent.find(r=>r.route==='/player/timing-observe');p.elements.stop.onclick();p.play();await flush();await p.frame(.2);await flush();
 const message=p.elements['timing-status'].textContent;p.releaseRoute();await flush();
 assert.equal(p.elements['timing-status'].textContent,message);assert.equal(p.elements.video.paused,false);
 const observations=p.sent.filter(r=>r.route==='/player/timing-observe');assert.notEqual(old.headers['X-Pulsar-Player'],observations.at(-1).headers['X-Pulsar-Player']);
 assert.equal(p.sent.filter(r=>r.route==='/player/start').length,2);
});
test('missing fresh frame expires only timing without automatic replacement or capture stop',async()=>{
 const p=mediaPlayer({timing:true});await prepareMedia(p);p.play();await flush();p.advance(1001);await flush();
 assert.match(p.elements['timing-status'].textContent,/No fresh frame/);assert.equal(p.elements.video.paused,false);
 p.advance(1000);await flush();assert.equal(p.sent.filter(r=>r.route==='/player/timing-probe').length,1);assert.equal(p.sent.filter(r=>r.route==='/stop').length,0);
});
test('timing failure and explicit history inspection never restart measurements or pause capture',async()=>{
 const p=mediaPlayer({timing:true,replyOverride:r=>r.route==='/player/timing-probe'?{ok:false,json:async()=>({error:'original Open unconfirmed: public-original-id'})}:null});await prepareMedia(p);p.play();await flush();await p.frame(.1);
 assert.match(p.elements['timing-status'].textContent,/public-original-id/);assert.equal(p.elements.video.paused,false);
 await p.elements['inspect-timing'].onclick();p.advance(500);await flush();assert.equal(p.sent.filter(r=>r.route==='/player/timing-probe').length,1);assert.equal(p.sent.filter(r=>r.route==='/player/start').length,1);
});
test('unbound local media never opens browser source timing',async()=>{
 const p=player({timing:true});p.play();await p.frame(.1);await flush();assert.equal(p.sent.filter(r=>r.route.startsWith('/player/timing-')).length,0);assert.match(p.elements['timing-status'].textContent,/unbound/);
});
test('malformed display metadata refuses timing without substituting media or receipt time',async()=>{
 const p=mediaPlayer({timing:true});await prepareMedia(p);p.play();await flush();await p.frame(.1,1,{presentationTime:NaN});await flush();
 assert.equal(p.sent.filter(r=>r.route==='/player/timing-observe').length,0);assert.match(p.elements['timing-status'].textContent,/Invalid browser timing/);assert.equal(p.elements.video.paused,false);
});
test('capture continuation preserves the native timing loop and browser player',async()=>{
 const p=mediaPlayer({timing:true,frameReply:{active:true,identity:{epoch:'engine-successor'},options:{policy:{maximum_analysis_age_ns:1e9}},inference:{completed:1,latest:null}}});await prepareMedia(p);p.play();await flush();await p.frame(.1);await flush();p.advance(250);await flush();await p.frame(.2);await flush();
 assert.equal(p.sent.filter(r=>r.route==='/player/start').length,1);assert.equal(p.sent.filter(r=>r.route==='/player/timing-observe').length,2);
 assert.deepEqual(p.sent.filter(r=>r.route==='/player/timing-observe').map(r=>jsonBody(r).sequence),['1','2']);
});


test('paused exact player can inspect original timing history without rearming',async()=>{
 const p=mediaPlayer({timing:true,replyOverride:r=>r.route==='/player/timing-status'?{ok:true,json:async()=>({current:false,qualified:false,observation:null})}:null});await prepareMedia(p);p.play();await flush();await p.frame(.1);await flush();
 const old=p.sent.find(r=>r.route==='/player/timing-observe').headers['X-Pulsar-Player'];p.elements.stop.onclick();await flush();await p.elements['inspect-timing'].onclick();
 const inspected=p.sent.find(r=>r.route==='/player/timing-status');assert.equal(inspected.headers['X-Pulsar-Player'],old);assert.match(p.elements['timing-status'].textContent,/not current/);
 p.advance(1000);await flush();assert.equal(p.sent.filter(r=>r.route==='/player/start').length,1);assert.equal(p.sent.filter(r=>r.route==='/player/timing-probe').length,1);assert.equal(p.elements.video.paused,true);
});
test('late paused-player timing history cannot overwrite a replacement player',async()=>{
 const p=mediaPlayer({timing:true,holdRoute:'/player/timing-status'});await prepareMedia(p);p.play();await flush();await p.frame(.1);await flush();p.elements.stop.onclick();await flush();
 const inspection=p.elements['inspect-timing'].onclick();await flush();p.play();await flush();await p.frame(.2);await flush();const message=p.elements['timing-status'].textContent;
 p.releaseRoute();await inspection;assert.equal(p.elements['timing-status'].textContent,message);assert.equal(p.elements.video.paused,false);
});


function acknowledgeHandy(p){for(const id of ['handy-environment-ack','handy-play-ack','handy-online-ack']){p.elements[id].checked=true;p.elements[id].onchange();}}
test('video Play with prepared Handy controls never requests device Start automatically',async()=>{
 const p=mediaPlayer({handyConfigured:true});await prepareMedia(p);acknowledgeHandy(p);p.play();await p.frame(.1);await flush();
 assert.equal(p.sent.filter(r=>r.route==='/player/handy-start').length,0);assert.equal(p.elements['handy-start'].disabled,true);assert.equal(p.elements['handy-play-ack'].checked,false);
});
test('explicit Handy Start requires inspected exact player and all three acknowledgments',async()=>{
 const p=mediaPlayer({handyConfigured:true});await prepareMedia(p);p.play();await p.frame(.1);await p.elements['handy-start'].onclick();assert.equal(p.sent.filter(r=>r.route==='/player/handy-start').length,0);
 await p.elements['handy-inspect'].onclick();await p.elements['handy-start'].onclick();assert.equal(p.sent.filter(r=>r.route==='/player/handy-start').length,0);
 acknowledgeHandy(p);await p.elements['handy-start'].onclick();const request=p.sent.find(r=>r.route==='/player/handy-start');assert.deepEqual(jsonBody(request),{acknowledge_current_environment:true,allow_play:true,allow_online_inspection:true});assert.equal(request.headers['X-Pulsar-Player'],'player-1');assert.ok(!Buffer.from(request.body).toString().includes('proof'));
 await p.elements['handy-start'].onclick();assert.equal(p.sent.filter(r=>r.route==='/player/handy-start').length,1);
});
test('ordinary fresh timing observations do not invalidate inspected browser Start selection',async()=>{
 const p=mediaPlayer({handyConfigured:true,timing:true});await prepareMedia(p);p.play();await flush();await p.frame(.1);await flush();await p.elements['handy-inspect'].onclick();acknowledgeHandy(p);
 p.advance(250);await flush();await p.frame(.2);await flush();assert.equal(p.elements['handy-start'].disabled,false);await p.elements['handy-start'].onclick();assert.equal(p.sent.filter(r=>r.route==='/player/handy-start').length,1);
});
test('late Handy inspection cannot enable a replacement player',async()=>{
 const p=mediaPlayer({handyConfigured:true,holdRoute:'/player/handy-inspect'});await prepareMedia(p);p.play();await p.frame(.1);const pending=p.elements['handy-inspect'].onclick();await flush();p.elements.stop.onclick();p.play();await p.frame(.2);p.releaseRoute();await pending;acknowledgeHandy(p);
 assert.equal(p.elements['handy-start'].disabled,true);await p.elements['handy-start'].onclick();assert.equal(p.sent.filter(r=>r.route==='/player/handy-start').length,0);
});
test('Handy Stop is independent of held Start response and late reply cannot restore pending playback',async()=>{
 const p=mediaPlayer({handyConfigured:true,holdRoute:'/player/handy-start'});await prepareMedia(p);p.play();await p.frame(.1);await p.elements['handy-inspect'].onclick();acknowledgeHandy(p);const pending=p.elements['handy-start'].onclick();await flush();
 await p.elements['handy-stop'].onclick();assert.equal(p.sent.filter(r=>r.route==='/player/handy-stop').length,1);assert.match(p.elements['handy-status'].textContent,/released/);
 p.releaseRoute();await pending;await flush();assert.match(p.elements['handy-status'].textContent,/stop_pending|released/);assert.equal(p.sent.filter(r=>r.route==='/player/handy-start').length,1);
});
test('lost Handy Start response preserves original inspection path without a replacement request',async()=>{
 const p=mediaPlayer({handyConfigured:true,replyOverride:r=>r.route==='/player/handy-start'?{ok:false,json:async()=>({error:'original request outcome unconfirmed'})}:null});await prepareMedia(p);p.play();await p.frame(.1);await p.elements['handy-inspect'].onclick();acknowledgeHandy(p);
 await p.elements['handy-start'].onclick();await p.elements['handy-start'].onclick();assert.equal(p.sent.filter(r=>r.route==='/player/handy-start').length,1);
 await p.elements['handy-original'].onclick();assert.equal(p.sent.filter(r=>r.route==='/player/handy-inspect-original').length,1);assert.match(p.elements['handy-status'].textContent,/original-browser-start/);
});
test('pause while Handy Start response is held retires exact player and late reply leaves successor alone',async()=>{
 const p=mediaPlayer({handyConfigured:true,holdRoute:'/player/handy-start'});await prepareMedia(p);p.play();await p.frame(.1);await p.elements['handy-inspect'].onclick();acknowledgeHandy(p);const pending=p.elements['handy-start'].onclick();await flush();
 p.elements.stop.onclick();p.play();await p.frame(.2);const message=p.elements['handy-status'].textContent;p.releaseRoute();await pending;await flush();assert.equal(p.elements['handy-status'].textContent,message);assert.equal(p.elements.video.paused,false);
 const stopped=p.sent.filter(r=>r.route==='/player/stop');assert.ok(stopped.every(r=>r.headers['X-Pulsar-Player']==='player-1'));assert.equal(p.sent.filter(r=>r.route==='/player/handy-start').length,1);
});

async function startLookahead(p){await prepareMedia(p);p.play();await flush();await p.frame(.1);await flush();}
function rgbPacket(request){const bytes=Buffer.from(request.body),split=bytes.indexOf(10);return {header:JSON.parse(bytes.subarray(0,split)),pixels:bytes.subarray(split+1)};}
test('lookahead uses the exact prepared URL and actual analysis callback PTS with a native feed',async()=>{
 const p=mediaPlayer({lookahead:true,timing:true,handyConfigured:true});await startLookahead(p);
 assert.equal(p.elements['analysis-video'].src,p.elements.video.src);assert.equal(p.elements['analysis-video'].muted,true);assert.equal(p.sent.filter(r=>r.route==='/start'||r.route==='/frame').length,0);
 await p.analysisFrame(1.1);
 const start=jsonBody(p.sent.find(r=>r.route==='/start'));assert.equal(start.media_us,1100000);assert.deepEqual(start.analysis,{role:'lookahead',policy:{target_lead_us:1000000,minimum_lead_us:500000,maximum_lead_us:2000000}});
 const packet=rgbPacket(p.sent.find(r=>r.route==='/frame'));assert.equal(packet.header.media_us,1100000);assert.equal(packet.header.epoch,'epoch-one');assert.equal(packet.header.analysis.feed_id,'analysis-feed-1');assert.equal(packet.header.analysis.role,'lookahead');assert.equal(packet.header.analysis.visible_media_us,100000);assert.equal(packet.header.analysis.presented_frames,1);assert.equal(packet.header.analysis.visible_presented_frames,1);assert.ok(packet.pixels.every(v=>v===128));assert.deepEqual(p.drawSources,['analysis']);
 assert.equal(p.sent.filter(r=>r.route==='/player/start').length,1);assert.match(p.elements['lookahead-status'].textContent,/Observed frame lead: 1000\.0 ms/);assert.ok(!p.sent.some(r=>r.route==='/player/handy-start'));assert.equal(p.elements['handy-start'].disabled,true);
});
test('lookahead keeps visible timing independent while analysis upload is blocked',async()=>{
 const p=mediaPlayer({lookahead:true,timing:true,holdFrameAt:1});await startLookahead(p);
 const pending=p.analysisFrame(1.1);await flush();p.advance(250);await flush();await p.frame(.2,2);await flush();
 const observations=p.sent.filter(r=>r.route==='/player/timing-observe').map(jsonBody);assert.ok(observations.length>=2);assert.equal(observations.at(-1).frame.sample.media_time_us,200000);assert.equal(p.sent.filter(r=>r.route==='/frame').length,1);
 p.releaseFrame();await pending;
});
test('lookahead stale callbacks cannot upload after pause or contaminate replacement generation',async()=>{
 const p=mediaPlayer({lookahead:true,timing:true});await startLookahead(p);await p.analysisFrame(1.1);const old=p.analysisCallback();const original=rgbPacket(p.sent.find(r=>r.route==='/frame')).header.analysis;
 p.elements.video.pause();assert.equal(p.elements['analysis-video'].paused,true);assert.equal(p.elements['analysis-video'].getAttribute('src'),null);
 p.play();await flush();await p.frame(.2,2);await flush();const before=p.sent.filter(r=>r.route==='/frame').length;await old(0,{mediaTime:1.2,presentedFrames:10});assert.equal(p.sent.filter(r=>r.route==='/frame').length,before);
 await p.analysisFrame(1.2,1);const replacement=rgbPacket(p.sent.filter(r=>r.route==='/frame').at(-1)).header.analysis;assert.notEqual(replacement.feed_id,original.feed_id);assert.ok(replacement.generation>original.generation);
});
test('lookahead refuses unbound media and actual lead loss retires both readers',async()=>{
 const unbound=player({lookahead:true});unbound.play();await unbound.frame(.1);assert.equal(unbound.sent.filter(r=>r.route==='/start'||r.route==='/player/start').length,0);assert.match(unbound.elements.status.textContent,/prepared Engine-media/);
 const p=mediaPlayer({lookahead:true,timing:true});await startLookahead(p);await p.analysisFrame(1.1);await p.frame(.8,2);await flush();assert.equal(p.elements.video.paused,true);assert.equal(p.elements['analysis-video'].paused,true);assert.match(p.elements.status.textContent,/lead bounds/);assert.equal(p.sent.filter(r=>r.route==='/frame').length,1);
});
test('lookahead source replacement seek visibility and analysis errors invalidate original feeds',async()=>{
 for(const stop of [p=>{p.elements.file.files=[];p.elements.file.onchange({target:p.elements.file});},p=>p.events.seeking(),p=>{p.document.visibilityState='hidden';p.documentEvents.visibilitychange();},p=>p.analysisEvents.error(),p=>p.analysisEvents.ratechange(),p=>p.analysisEvents.ended()]){
  const p=mediaPlayer({lookahead:true,timing:true});await startLookahead(p);await p.analysisFrame(1.1);const old=p.analysisCallback();stop(p);await flush();const before=p.sent.filter(r=>r.route==='/frame').length;await old(0,{mediaTime:1.2,presentedFrames:2});assert.equal(p.sent.filter(r=>r.route==='/frame').length,before);assert.equal(p.elements['analysis-video'].getAttribute('src'),null);assert.ok(p.sent.some(r=>r.route==='/player/stop'));
 }
});
test('lookahead shows future estimates separately and refuses silent capture continuation',async()=>{
 const result={active:true,identity:{epoch:'epoch-one'},options:{policy:{maximum_analysis_age_ns:1e9}},inference:{completed:1,latest:{fresh:true,result:{payload:{position:.7,input:{pair:{current_time:{numerator:11,denominator:10}}}}}}}};
 const p=mediaPlayer({lookahead:true,timing:true,frameReply:result});await startLookahead(p);await p.analysisFrame(1.1);assert.equal(p.elements.position.value,0);assert.match(p.elements['analysis-estimate'].textContent,/Ahead-of-player estimate: 70\.0% at source 1\.100/);
 const changed=mediaPlayer({lookahead:true,timing:true,frameReply:{...result,identity:{epoch:'unexpected-successor'}}});await startLookahead(changed);await changed.analysisFrame(1.1);assert.equal(changed.elements.video.paused,true);assert.match(changed.elements.status.textContent,/feed identity|continuation/);assert.equal(changed.sent.filter(r=>r.route==='/start').length,1);
});
test('lookahead waits for analysis seek and visible timing instead of stamping a requested offset',async()=>{
 const p=mediaPlayer({lookahead:true,timing:true,analysisSeek:false});await prepareMedia(p);p.play();await flush();await p.frame(.1);await flush();assert.equal(p.sent.filter(r=>r.route==='/start').length,0);assert.equal(p.analysisCallback(),undefined);
 p.analysisSettled(1.02);await flush();await p.analysisFrame(1.02);assert.equal(jsonBody(p.sent.find(r=>r.route==='/start')).media_us,1020000);assert.match(p.elements['lookahead-status'].textContent,/920\.0 ms/);
});

test('lookahead future estimate expiry cannot be renewed by repeating the original result',async()=>{
 const result={active:true,identity:{epoch:'epoch-one'},options:{policy:{maximum_analysis_age_ns:1e9}},inference:{completed:1,latest:{fresh:true,result:{payload:{position:.7,input:{pair:{current_time:{numerator:11,denominator:10}}}}}}}};
 const p=mediaPlayer({lookahead:true,timing:true,frameReply:result});await startLookahead(p);await p.analysisFrame(1.1);p.advance(1001);await flush();assert.match(p.elements['analysis-estimate'].textContent,/expired/);await p.frame(.2,2);await flush();await p.analysisFrame(1.2);assert.match(p.elements['analysis-estimate'].textContent,/expired/);assert.equal(p.elements.position.value,0);
});


function lookaheadEstimate(timeUs,position=.7,maximumAgeMs=2000){
 const current_time={numerator:timeUs,denominator:1e6},payload={position,input:{pair:{current_time}}};
 return {active:true,identity:{epoch:'epoch-one'},options:{policy:{maximum_analysis_age_ns:maximumAgeMs*1e6}},inference:{completed:1,latest:{fresh:true,result:{payload}}}};
}
function retainedLookaheadPlayer(options={}){
 const media={...readyMedia,trim_end_us:10000000};
 return mediaPlayer({lookahead:true,timing:true,...options,candidateReplies:{'/media/prepare':media,'/media/status':media,...options.candidateReplies}});
}
test('sustained lookahead retains earlier results until visible frames catch up within their original budget',async()=>{
 const p=retainedLookaheadPlayer({frameReply:n=>lookaheadEstimate(1000000+n*100000,n/100,2000)});await startLookahead(p);
 for(let i=0;i<=30;i++){
  if(i){p.advance(100);await flush();await p.frame((i+1)/10);await flush();}
  await p.analysisFrame((i+11)/10);
  if(i<10){assert.equal(p.elements.position.value,0);assert.match(p.elements.status.textContent,/waiting for the visible source time/);}
  else{assert.equal(p.elements.position.value,(i-9)/100);assert.match(p.elements.status.textContent,/Provisional Stroke at the visible source time/);}
 }
 const queued=p.futurePreview().entries;assert.equal(queued.length,10);assert.equal(queued[0].time,3.2);assert.equal(queued.at(-1).time,4.1);assert.equal(queued[0].deadline,4100);assert.equal(p.elements.video.paused,false);assert.equal(p.timerCount(),1);
});
test('native launch default leaves freshness margin without changing the original estimate deadline',async()=>{
 const p=mediaPlayer({lookahead:true,timing:true,pageScope:{...defaultPageScope,default_lookahead_ms:500},frameReply:lookaheadEstimate(600000,.7,1000)});
 await startLookahead(p);await p.analysisFrame(.6);await flush();
 const start=jsonBody(p.sent.find(r=>r.route==='/start'));
 assert.equal(start.analysis.policy.target_lead_us,500000);
 assert.equal(p.futurePreview().entries[0].deadline,1000);
 p.advance(500);await flush();await p.analysisFrame(1.1);await p.frame(.6);await flush();
 assert.equal(p.elements.position.value,.7);
 assert.match(p.elements.status.textContent,/Provisional Stroke at the visible source time/);
});
test('native launch default remains editable and cannot enlarge the original freshness budget',async()=>{
 const p=retainedLookaheadPlayer({pageScope:{...defaultPageScope,default_lookahead_ms:500},frameReply:lookaheadEstimate(1100000,.7,1000)});
 p.elements['lookahead-ms'].value='1000';await startLookahead(p);await p.analysisFrame(1.1);
 assert.equal(jsonBody(p.sent.find(r=>r.route==='/start')).analysis.policy.target_lead_us,1000000);
 assert.equal(p.elements.position.value,0);assert.match(p.elements.status.textContent,/insufficient freshness margin/);
});
test('malformed native lookahead defaults fail before any playback admission',()=>{
 for(const value of [0,15001,1.5,'500',null])assert.throws(()=>mediaPlayer({pageScope:{...defaultPageScope,default_lookahead_ms:value}}),/Invalid native lookahead default/);
});
test('one-second lead with the original one-second budget remains unavailable at the deadline boundary',async()=>{
 const p=retainedLookaheadPlayer({frameReply:n=>lookaheadEstimate(1000000+n*100000,n/100,1000)});await startLookahead(p);
 for(let i=0;i<=20;i++){
  if(i){p.advance(100);await flush();await p.frame((i+1)/10);await flush();}
  await p.analysisFrame((i+11)/10);assert.equal(p.elements.position.value,0);
  assert.match(p.elements.status.textContent,/insufficient freshness margin|estimate expired/);assert.doesNotMatch(p.elements.status.textContent,/Starting live analysis|Provisional Stroke/);
 }
 assert.equal(p.elements.video.paused,false);assert.ok(p.futurePreview().entries.every(value=>value.deadline>2000));
});
test('lookahead duplicates preserve the first position and original deadline before and after expiry',async()=>{
 const p=retainedLookaheadPlayer({frameReply:n=>lookaheadEstimate(1100000,n===1?.7:.9,2000)});await startLookahead(p);await p.analysisFrame(1.1);
 const original=p.futurePreview();assert.equal(original.entries.length,1);assert.equal(original.entries[0].deadline,2000);
 p.advance(400);await flush();await p.frame(.2);await flush();await p.analysisFrame(1.2);
 assert.deepEqual(p.futurePreview(),original);
 p.advance(1600);await flush();assert.equal(p.futurePreview().entries.length,0);assert.match(p.elements.status.textContent,/estimate expired/);
 await p.frame(.3);await flush();await p.analysisFrame(1.3);assert.equal(p.futurePreview().entries.length,0);assert.deepEqual(p.futurePreview().last,original.last);assert.equal(p.elements.position.value,0);assert.match(p.elements.status.textContent,/estimate expired/);
});
test('lookahead queue bounds retire the original feed without overwriting pending estimates',async()=>{
 const p=retainedLookaheadPlayer({frameReply:n=>lookaheadEstimate(1100000+n-1)});await startLookahead(p);
 const limit=p.futurePreview().limit;
 for(let i=0;i<limit;i++)await p.analysisFrame((1100000+i)/1e6);
 const full=p.futurePreview();assert.equal(full.entries.length,limit);assert.equal(full.entries[0].time,1.1);assert.equal(full.entries.at(-1).time,(1100000+limit-1)/1e6);assert.equal(p.elements.video.currentTime,.1);assert.equal(p.elements.position.value,0);
 await p.analysisFrame((1100000+limit)/1e6);await flush();assert.equal(p.elements.video.paused,true);assert.equal(p.elements['analysis-video'].paused,true);assert.equal(p.futurePreview().entries.length,0);assert.equal(p.futurePreview().last,null);assert.match(p.elements.status.textContent,/bounded future-estimate queue is full/);assert.ok(p.sent.some(r=>r.route==='/player/stop'));
});
test('a delayed visible callback selects the newest eligible due estimate and keeps later samples',async()=>{
 const p=retainedLookaheadPlayer({frameReply:n=>lookaheadEstimate(1000000+n*100000,n/100)});await startLookahead(p);await p.analysisFrame(1.1);
 for(let i=1;i<=10;i++){
  p.advance(100);await flush();if(i<=6){await p.frame((i+1)/10);await flush();}await p.analysisFrame((i+11)/10);
 }
 assert.equal(p.elements.video.currentTime,.7);assert.equal(p.elements.position.value,0);assert.equal(p.futurePreview().entries.length,11);
 p.advance(100);await flush();await p.frame(1.2);await flush();
 assert.equal(p.elements.position.value,.02);const remaining=p.futurePreview().entries;assert.equal(remaining.length,9);assert.equal(remaining[0].time,1.3);assert.equal(remaining.at(-1).time,2.1);assert.equal(remaining[0].deadline,2200);assert.equal(p.elements.video.paused,false);
});
test('late original frame responses cannot seed the replacement playback queue',async()=>{
 const p=retainedLookaheadPlayer({holdFrameAt:1,frameReply:n=>lookaheadEstimate(n===1?1100000:1200000,n===1?.8:.3)});await startLookahead(p);
 const pending=p.analysisFrame(1.1);await flush();p.elements.stop.onclick();p.play();await flush();await p.frame(.2);await flush();const message=p.elements.status.textContent;
 p.releaseFrame();await pending;await flush();assert.equal(p.elements.status.textContent,message);assert.equal(p.elements.video.paused,false);assert.equal(p.futurePreview().entries.length,0);assert.equal(p.elements.position.value,0);
 await p.analysisFrame(1.2,1);const queued=p.futurePreview().entries;assert.equal(queued.length,1);assert.equal(queued[0].time,1.2);assert.equal(queued[0].position,.3);
 const packets=p.sent.filter(r=>r.route==='/frame').map(rgbPacket);assert.notEqual(packets[0].header.analysis.feed_id,packets[1].header.analysis.feed_id);assert.ok(packets[1].header.analysis.generation>packets[0].header.analysis.generation);
});
test('cancelled original expiry callbacks cannot change a replacement lookahead queue',async()=>{
 const p=retainedLookaheadPlayer({frameReply:n=>lookaheadEstimate(n===1?1100000:1200000,n===1?.8:.3)});await startLookahead(p);await p.analysisFrame(1.1);const [oldTimer]=p.pendingFutureTimers();assert.equal(typeof oldTimer,'function');
 p.elements.stop.onclick();p.play();await flush();await p.frame(.2);await flush();await p.analysisFrame(1.2,1);const replacement=p.futurePreview(),message=p.elements.status.textContent;
 oldTimer();assert.deepEqual(p.futurePreview(),replacement);assert.equal(p.elements.status.textContent,message);assert.equal(p.elements.video.paused,false);assert.equal(p.pendingFutureTimers().length,1);
});
test('lookahead without a fresh result leaves Starting and reports that analysis is waiting',async()=>{
 const missing={active:true,identity:{epoch:'epoch-one'},options:{policy:{maximum_analysis_age_ns:1e9}},inference:{completed:0,latest:null}};
 const p=retainedLookaheadPlayer({frameReply:missing});await startLookahead(p);await p.analysisFrame(1.1);assert.match(p.elements.status.textContent,/Analysis is running; waiting for a fresh estimate/);assert.equal(p.elements.position.value,0);assert.equal(p.futurePreview().entries.length,0);
});

test('ordinary preview frame callbacks preserve inference status while an upload is pending',async()=>{
 const missing={active:true,identity:{epoch:'epoch-one'},options:{policy:{maximum_analysis_age_ns:1e9}},inference:{state:'waiting',completed:0,latest:null}};
 const p=player({frameReply:missing,holdFrameAt:2});p.play();await p.frame(.1);const message=p.elements.status.textContent;assert.match(message,/Inference: waiting/);
 const pending=p.frame(.2);await flush();await p.frame(.3);assert.equal(p.elements.status.textContent,message);p.releaseFrame();await pending;
});


function closedInput(report,phase='sealed',disposition='accepted',matched=false){
 return {...report,input_close:{original_request_id:'input-close-1',final_frame:{native_only:true},phase,disposition:phase==='sealed'?disposition:null,worker_joined:phase==='sealed',delivery_settled:phase==='sealed'},browser:{analysis_final_result_matches:matched}};
}
async function flushFinish(){await flush();await flush();}
test('natural analysis pause before ended closes once and preserves visible playback and queued estimates',async()=>{
 const p=mediaPlayer({lookahead:true,timing:true,frameReply:lookaheadEstimate(1100000,.7,2000)});await startLookahead(p);await p.analysisFrame(1.1);const queued=p.futurePreview(),old=p.analysisCallback(),draws=p.drawSources.length;
 p.analysisEnd(2);await flushFinish();p.analysisEvents.ended?.();await flushFinish();
 assert.equal(p.sent.filter(r=>r.route==='/analysis/finish').length,1);assert.equal(p.sent.filter(r=>r.route==='/frame').length,1);assert.equal(p.drawSources.length,draws);assert.equal(p.elements.video.paused,false);assert.deepEqual(p.futurePreview(),queued);assert.match(p.elements['lookahead-status'].textContent,/input sealed/);assert.ok(!p.sent.some(r=>r.route==='/player/stop'));
 const close=jsonBody(p.sent.find(r=>r.route==='/analysis/finish'));assert.equal(close.capture_epoch,'epoch-one');assert.equal(close.final_observation.media_us,1100000);assert.equal(close.final_observation.observation.presented_frames,1);assert.ok(!('final_frame' in close));
 await old(0,{mediaTime:1.9,presentedFrames:2});assert.equal(p.sent.filter(r=>r.route==='/frame').length,1);
 p.advance(1000);await flush();await p.frame(1.1);await flushFinish();assert.equal(p.elements.position.value,.7);assert.equal(p.elements.video.paused,false);assert.ok(p.sent.some(r=>r.route==='/player/timing-observe'));
 await p.frame(2);await flush();assert.equal(p.elements.video.paused,true);assert.ok(p.sent.some(r=>r.route==='/player/stop'));
});
test('analysis trim closes at the first outside callback without uploading it or redrawing the final image',async()=>{
 const p=mediaPlayer({lookahead:true,timing:true,frameReply:n=>lookaheadEstimate(n===1?1100000:1900000,.7,2000)});await startLookahead(p);await p.analysisFrame(1.1);
 p.advance(800);await flush();await p.analysisFrame(1.9);await p.frame(.9);await flush();const draws=p.drawSources.length;
 await p.analysisFrame(2);await flushFinish();assert.equal(p.drawSources.length,draws);assert.equal(p.sent.filter(r=>r.route==='/frame').length,2);assert.equal(p.elements.video.paused,false);assert.equal(p.elements['analysis-video'].paused,true);assert.match(p.elements['lookahead-status'].textContent,/input sealed/);
 assert.equal(jsonBody(p.sent.find(r=>r.route==='/analysis/finish')).final_observation.media_us,1900000);
 await p.frame(1.9);assert.equal(p.elements.position.value,.7);assert.equal(p.elements.video.paused,false);
});
test('EOF waits for the original held final frame response before selecting its exact close target',async()=>{
 const p=mediaPlayer({lookahead:true,timing:true,holdFrameAt:2,frameReply:n=>lookaheadEstimate(n===1?1100000:1900000,.7,2000)});await startLookahead(p);await p.analysisFrame(1.1);p.advance(800);await flush();
 const pending=p.analysisFrame(1.9);await flush();await p.frame(.9);p.analysisEnd(2);await flushFinish();assert.equal(p.sent.filter(r=>r.route==='/analysis/finish').length,0);assert.equal(p.elements.video.paused,false);
 p.releaseFrame();await pending;await flushFinish();const close=p.sent.filter(r=>r.route==='/analysis/finish');assert.equal(close.length,1);assert.equal(jsonBody(close[0]).final_observation.media_us,1900000);assert.equal(p.sent.filter(r=>r.route==='/frame').length,2);assert.equal(p.drawSources.length,2);
});
test('a failed final frame response aborts EOF without closing an older acknowledged frame',async()=>{
 let frames=0;const p=mediaPlayer({lookahead:true,timing:true,holdFrameAt:2,frameReply:lookaheadEstimate(1100000),replyOverride:r=>r.route==='/frame'&&++frames===2?{ok:false,json:async()=>({error:'final pixels unconfirmed'})}:null});await startLookahead(p);await p.analysisFrame(1.1);
 const pending=p.analysisFrame(1.2);await flush();p.analysisEnd(2);await flushFinish();p.releaseFrame();await pending;await flushFinish();assert.equal(p.sent.filter(r=>r.route==='/analysis/finish').length,0);assert.equal(p.elements.video.paused,true);assert.match(p.elements.status.textContent,/final pixels unconfirmed/);assert.ok(p.sent.some(r=>r.route==='/player/stop'));
});
test('lost finish acknowledgement inspects the same original request without a second close',async()=>{
 const p=mediaPlayer({lookahead:true,timing:true,failFinishOnce:true,frameReply:lookaheadEstimate(1100000)});await startLookahead(p);await p.analysisFrame(1.1);p.analysisEnd(2);await flushFinish();assert.match(p.elements['lookahead-status'].textContent,/reply unavailable/);
 p.advance(100);await flushFinish();assert.equal(p.sent.filter(r=>r.route==='/analysis/finish').length,1);const reads=p.sent.filter(r=>r.route==='/analysis/finish-status');assert.equal(reads.length,1);assert.deepEqual(jsonBody(reads[0]),jsonBody(p.sent.find(r=>r.route==='/analysis/finish')));assert.match(p.elements['lookahead-status'].textContent,/input sealed/);assert.equal(p.elements.video.paused,false);
});
test('a final result first observed by status keeps the original submitted callback deadline',async()=>{
 const missing={...lookaheadEstimate(600000,.8,1000),inference:{completed:0,latest:null}};
 const p=mediaPlayer({lookahead:true,lookaheadMs:'500',timing:true,frameReply:missing,finishReply:closedInput(missing,'joining'),finishStatusReply:closedInput(lookaheadEstimate(600000,.8,1000),'sealed','accepted',true)});await startLookahead(p);await p.analysisFrame(.6);p.analysisEnd(2);await flushFinish();assert.equal(p.futurePreview().entries.length,0);
 p.advance(400);await flushFinish();const [final]=p.futurePreview().entries;assert.equal(final.time,.6);assert.equal(final.deadline,1000);assert.equal(final.position,.8);
 await p.frame(.5);await flush();p.advance(100);await flushFinish();await p.frame(.6);assert.equal(p.elements.position.value,.8);assert.equal(p.elements.video.paused,false);
 p.advance(500);await flushFinish();assert.equal(p.elements.position.value,0);assert.equal(p.futurePreview().entries.length,0);assert.equal(p.futurePreview().last.deadline,1000);
});
test('draining refuses a sealed report that does not match the final frame and never substitutes an older latest',async()=>{
 const p=mediaPlayer({lookahead:true,timing:true,frameReply:{...lookaheadEstimate(1100000),inference:{completed:0,latest:null}},finishReply:closedInput(lookaheadEstimate(1000000,.9),'sealed','accepted',false)});await startLookahead(p);await p.analysisFrame(1.1);p.analysisEnd(2);await flushFinish();assert.equal(p.futurePreview().entries.length,0);assert.equal(p.elements.position.value,0);assert.equal(p.elements.video.paused,false);
});
test('unexpected analysis pause near EOF and visible user pause during drain still stop immediately',async()=>{
 const p=mediaPlayer({lookahead:true,timing:true,frameReply:lookaheadEstimate(1100000)});await startLookahead(p);await p.analysisFrame(1.1);p.analysisSettled(1.999999);p.elements['analysis-video'].pause();await flush();assert.equal(p.elements.video.paused,true);assert.equal(p.sent.filter(r=>r.route==='/analysis/finish').length,0);
 const q=mediaPlayer({lookahead:true,timing:true,frameReply:lookaheadEstimate(1100000)});await startLookahead(q);await q.analysisFrame(1.1);q.analysisEnd(2);await flushFinish();q.elements.video.pause();await flush();assert.equal(q.futurePreview().entries.length,0);assert.ok(q.sent.some(r=>r.route==='/player/stop'));
});
test('late original close reply cannot seal or publish into replacement playback',async()=>{
 const p=retainedLookaheadPlayer({holdRoute:'/analysis/finish',frameReply:n=>lookaheadEstimate(n===1?1100000:1200000,n===1?.8:.3)});await startLookahead(p);await p.analysisFrame(1.1);p.analysisEnd(10);await flush();p.elements.stop.onclick();p.play();await flush();await p.frame(.2);await flush();const message=p.elements.status.textContent;
 p.releaseRoute();await flushFinish();assert.equal(p.elements.status.textContent,message);assert.equal(p.elements.video.paused,false);assert.equal(p.futurePreview().entries.length,0);await p.analysisFrame(1.2,1);assert.equal(p.futurePreview().entries[0].position,.3);assert.ok(!p.sent.some(r=>r.route==='/analysis/finish-status'));
});
test('closed input keeps visible timing alive beyond the ordinary three-second upload timeout',async()=>{
 const p=retainedLookaheadPlayer({frameReply:lookaheadEstimate(1100000,.7,1000)});await startLookahead(p);await p.analysisFrame(1.1);p.analysisEnd(10);await flushFinish();
 for(let i=1;i<=35;i++){p.advance(100);await flushFinish();await p.frame(.1+i/10);await flush();}
 assert.equal(p.elements.video.paused,false);assert.equal(p.sent.filter(r=>r.route==='/frame').length,1);assert.ok(p.sent.filter(r=>r.route==='/analysis/finish-status').length>=30);assert.ok(jsonBody(p.sent.filter(r=>r.route==='/player/timing-observe').at(-1)).frame.sample.media_time_us>=3100000);assert.equal(p.elements.position.value,0);assert.ok(!p.sent.some(r=>r.route==='/player/stop'));
});
test('repeated pending finish inspections cannot renew the original settlement deadline',async()=>{
 const media={...readyMedia,trim_end_us:30000000},pending=closedInput(lookaheadEstimate(1100000),'closing');
 const p=mediaPlayer({lookahead:true,timing:true,frameReply:lookaheadEstimate(1100000),finishReply:pending,finishStatusReply:pending,candidateReplies:{'/media/prepare':media,'/media/status':media}});await startLookahead(p);await p.analysisFrame(1.1);p.analysisEnd(30);await flushFinish();
 for(let i=1;i<=120;i++){p.advance(100);await flushFinish();if(!p.elements.video.paused){await p.frame(.1+i/10);await flush();}}
 assert.equal(p.elements.video.paused,true);assert.match(p.elements.status.textContent,/Original input-close settlement deadline elapsed/);assert.equal(p.sent.filter(r=>r.route==='/analysis/finish').length,1);assert.ok(p.sent.some(r=>r.route==='/player/stop'));
});
test('original lease expiry and aborted close reports win over finite draining',async()=>{
 const short={...lookaheadEstimate(1100000),options:{policy:{maximum_analysis_age_ns:2e9},lease_ms:500}};
 const p=mediaPlayer({lookahead:true,timing:true,leaseMs:500,frameReply:short,finishReply:closedInput(short)});await startLookahead(p);await p.analysisFrame(1.1);p.analysisEnd(2);await flushFinish();p.advance(500);await flushFinish();assert.equal(p.elements.video.paused,true);assert.match(p.elements.status.textContent,/capture lease ended/);
 const q=mediaPlayer({lookahead:true,timing:true,frameReply:lookaheadEstimate(1100000),finishReply:closedInput(lookaheadEstimate(1100000),'aborted')});await startLookahead(q);await q.analysisFrame(1.1);q.analysisEnd(2);await flushFinish();assert.equal(q.elements.video.paused,true);assert.match(q.elements.status.textContent,/input close aborted/);
});

// Authored transport results exercise client ordering only; no native transfer proof.
function continuityPlayer({enabled=true,preparing=0,joining=0,wrongMedia=false,holdTransfer=false,mediaPreparing=0,holdRoute=null,joinPending=false}={}){
 let start,scope,nextFeed,oldUpload,nextUpload,prepareCalls=0,statusCalls=0,mediaCalls=0,transferred=false,timingSequence=0,predecessorJoined=!joinPending;
 const ok=data=>({ok:true,json:async()=>data});
 const base=()=>({active:true,identity:{epoch:transferred?'epoch-two':'epoch-one'},options:{lease_ms:30000,policy:{maximum_analysis_age_ns:2e9}},inference:{completed:0}});
 const oldFeed=()=>({feed_id:'analysis-feed-1',role:'lookahead',generation:scope.generation,player:scope,capture_epoch:'epoch-one',policy:start.analysis.policy});
 const transfer=phase=>({original_request_id:'transfer-one',preparation_request_id:'successor-prepare-one',predecessor:{epoch:'epoch-one'},successor:{epoch:'epoch-two'},boundary:{native_only:true},phase,predecessor_worker_joined:predecessorJoined||phase==='transferred',predecessor_delivery_settled:predecessorJoined||phase==='transferred',successor_pair_accepted:!!nextUpload,error:null});
 const pending=phase=>({...base(),browser:{analysis_feed:oldFeed(),analysis_last_upload:oldUpload,analysis_successor:{original_request_id:'successor-prepare-one',feed:nextFeed,report:{original_request_id:'successor-prepare-one',predecessor:{epoch:'epoch-one'},successor:{identity:{epoch:'epoch-two'},options:{lease_ms:30000}},phase:'prepared'},transfer:phase?transfer(phase):null,last_upload:nextUpload}}});
 const p=mediaPlayer({lookahead:true,timing:true,holdRoute,frameReply:{...lookaheadEstimate(1100000),browser:{analysis_continuity:{enabled:true,prepare_due:true}}},replyOverride:r=>{
  if(r.route==='/start'){start=jsonBody(r);scope={launch_id:r.headers['X-Pulsar-Launch'],page_id:r.headers['X-Pulsar-Page'],player_id:r.headers['X-Pulsar-Player'],source_id:r.headers['X-Pulsar-Source'],generation:Number(r.headers['X-Pulsar-Generation']),time_basis:start.source_time_basis};nextFeed={...oldFeed(),feed_id:'analysis-feed-2',capture_epoch:'epoch-two'};}
  if(r.route==='/frame'){const h=rgbPacket(r).header;oldUpload={media_us:h.media_us,observation:h.analysis};if(transferred)return ok({...base(),browser:{analysis_feed:nextFeed,analysis_last_upload:oldUpload,analysis_continuity:{enabled:true,prepare_due:false,media_preparation_request_id:'media-prepare-two'}}});}
  if(r.route==='/media/continue'||r.route==='/media/continue-status')return ok({...readyMedia,state:++mediaCalls<=mediaPreparing?'pending':'ready',url:wrongMedia?'/media/foreign':readyMedia.url,preparation_request_id:'media-prepare-two'});
  if(r.route==='/player/timing-next-open')return ok({pending:true});
  if(r.route==='/player/timing-next-probe')return ok({sequence:String(++timingSequence),probe_id:'next-probe-'+timingSequence,probe_timeout_ms:1000,observation_timeout_ms:3000});
  if(r.route==='/player/timing-next-observe'){const o=jsonBody(r);return ok({current:true,qualified:false,observation:{sequence:o.sequence}});}
  if(r.route==='/analysis/prepare'){const data=pending();data.browser.analysis_successor.report.phase=++prepareCalls<=preparing?'preparing':'prepared';return ok(data);}
  if(r.route==='/analysis/transfer')return ok(pending(predecessorJoined?'awaiting_successor_pair':'boundary_reserved'));
  if(r.route==='/analysis/successor-frame'){const h=rgbPacket(r).header;nextUpload={media_us:h.media_us,observation:h.analysis};return ok(pending('ready_for_transfer'));}
  if(r.route==='/analysis/transfer-status'){
   const phase=!predecessorJoined?'joining_predecessor':!nextUpload?'awaiting_successor_pair':holdTransfer||++statusCalls<=joining?'ready_for_transfer':'transferred';
   if(phase!=='transferred')return ok(pending(phase));transferred=true;return ok({...base(),browser:{analysis_feed:nextFeed,analysis_last_upload:nextUpload,analysis_transfer_completed:transfer(phase),analysis_continuity:{enabled:true,prepare_due:false,media_preparation_request_id:'media-prepare-two'}}});
  }
  if(r.route==='/analysis/cancel-successor')return ok({...base(),browser:{analysis_feed:oldFeed(),analysis_last_upload:oldUpload}});
  if(transferred&&(r.route==='/analysis/finish'||r.route==='/analysis/finish-status'))return ok({...base(),browser:{analysis_feed:nextFeed,analysis_last_upload:nextUpload,analysis_finish_request_id:'finish-two',analysis_final_result_matches:false},input_close:{original_request_id:'finish-two',phase:'sealed',disposition:'accepted',worker_joined:true,delivery_settled:true}});
 }});
 p.elements['analysis-continuity'].checked=enabled;p.confirmPredecessorJoin=()=>{predecessorJoined=true;};return p;
}
async function prepareContinuation(p){await startLookahead(p);await p.analysisFrame(1.1);await flush();await flush();await p.frame(.2,2);await flush();await flush();}
test('media continuation and its status retain the original complete player scope',async()=>{
 const p=continuityPlayer({mediaPreparing:1});await prepareContinuation(p);p.advance(100);await flush();await flush();
 const original=p.sent.find(r=>r.route==='/start').headers;
 for(const route of ['/media/continue','/media/continue-status']){
  const requests=p.sent.filter(r=>r.route===route);assert.equal(requests.length,1,route);
  assert.deepEqual(requests[0].headers,original,route+' must carry the original active player scope');
 }
});
test('cancelled media continuation polling cannot dispatch for a replacement player',async()=>{
 const p=continuityPlayer({mediaPreparing:1});await prepareContinuation(p);const [oldTimer]=p.pendingContinuationTimers();assert.equal(typeof oldTimer,'function');
 p.elements.stop.onclick();p.play();await flush();await p.frame(.2);await flush();const message=p.elements.status.textContent;
 const before=p.sent.length;oldTimer();await flush();await flush();assert.equal(p.sent.length,before);assert.equal(p.elements.status.textContent,message);assert.equal(p.elements.video.paused,false);
});
for(const route of ['/media/continue','/media/continue-status'])test('late '+route+' reply cannot continue or change a replacement player',async()=>{
 const p=continuityPlayer({mediaPreparing:route.endsWith('-status')?1:0,holdRoute:route});await prepareContinuation(p);
 if(route.endsWith('-status')){p.advance(100);await flush();}
 assert.ok(p.sent.some(r=>r.route===route));p.elements.stop.onclick();p.play();await flush();await p.frame(.2);await flush();
 const before=p.sent.length,message=p.elements.status.textContent;p.releaseRoute();await flush();await flush();
 assert.equal(p.sent.length,before);assert.equal(p.elements.status.textContent,message);assert.equal(p.elements.video.paused,false);
});
test('successor timing preparation keeps predecessor observations current',async()=>{
 const p=continuityPlayer({holdRoute:'/player/timing-next-probe'});await startLookahead(p);await p.analysisFrame(1.1);await flush();await flush();
 assert.ok(p.sent.some(r=>r.route==='/player/timing-next-probe'));assert.equal(p.continuityState().phase,'timing');
 const probes=p.sent.filter(r=>r.route==='/player/timing-probe').length,observations=p.sent.filter(r=>r.route==='/player/timing-observe').length;
 p.advance(250);await flush();assert.equal(p.sent.filter(r=>r.route==='/player/timing-probe').length,probes+1);
 await p.frame(.2,2);await flush();assert.equal(p.sent.filter(r=>r.route==='/player/timing-observe').length,observations+1);
 assert.ok(!p.sent.some(r=>r.route==='/player/timing-next-observe'||r.route==='/analysis/prepare'));
 p.releaseRoute();await flush();await p.frame(.3,3);await flush();await flush();
 assert.ok(p.sent.some(r=>r.route==='/player/timing-next-observe'));assert.ok(p.sent.some(r=>r.route==='/analysis/prepare'));
 const original=p.sent.find(r=>r.route==='/start').headers;
 for(const request of p.sent.filter(r=>r.route.startsWith('/player/timing-')))assert.deepEqual(request.headers,original);
 assert.equal(p.sent.filter(r=>r.route==='/player/start').length,1);
});
test('reserved boundary drains the existing predecessor probe without admitting probes that starve G',async()=>{
 const p=continuityPlayer({holdTransfer:true});await prepareContinuation(p);
 p.advance(250);await flush();
 await p.analysisFrame(1.2);await flush();assert.equal(p.continuityState().phase,'pair');
 const probes=p.sent.filter(r=>r.route==='/player/timing-probe').length;
 await p.frame(.3,3);await flush();await flush();
 await p.elements['inspect-timing'].onclick();await flush();
 p.advance(250);await flush();await p.analysisFrame(1.3);await flush();
 assert.equal(p.sent.filter(r=>r.route==='/analysis/successor-frame').length,1,'fresh ordinary probes must not starve the original successor G');
 assert.equal(p.sent.filter(r=>r.route==='/player/timing-probe').length,probes,'the already issued probe drains but no replacement probe is admitted');
 assert.equal(p.continuityState().phase,'joining');
 assert.equal(p.continuityState().epoch,'epoch-one','only native commit can promote');
});
test('predecessor timing remains frozen after the original successor G until commit',async()=>{
 const p=continuityPlayer({holdTransfer:true});await prepareContinuation(p);await p.analysisFrame(1.2);await p.analysisFrame(1.3);await flush();
 assert.equal(p.continuityState().phase,'joining');const probes=p.sent.filter(r=>r.route==='/player/timing-probe').length;
 p.advance(250);await flush();await p.frame(.3,3);await flush();
 assert.equal(p.sent.filter(r=>r.route==='/player/timing-probe').length,probes);
 assert.equal(p.sent.filter(r=>r.route==='/analysis/successor-frame').length,1);assert.equal(p.continuityState().phase,'joining');
});
test('successor capture waits for predecessor join without reusing a waiting callback',async()=>{
 const p=continuityPlayer({joinPending:true});await prepareContinuation(p);
 await p.analysisFrame(1.2);await flush();await flush();
 assert.equal(p.continuityState().phase,'joining');
 const draws=p.drawSources.length;
 await p.analysisFrame(1.3);await flush();
 assert.equal(p.sent.filter(r=>r.route==='/analysis/successor-frame').length,0);
 assert.equal(p.drawSources.length,draws,'waiting callback must not capture or retain G pixels');
 p.confirmPredecessorJoin();p.advance(100);await flush();await flush();
 assert.equal(p.continuityState().phase,'pair');
 await p.analysisFrame(1.4);await flush();
 const next=p.sent.filter(r=>r.route==='/analysis/successor-frame').map(rgbPacket);
 assert.equal(next.length,1);assert.equal(next[0].header.media_us,1400000);
});

test('continuity is explicit and prepares one successor without changing active epoch',async()=>{
 const ordinary=continuityPlayer({enabled:false});await prepareContinuation(ordinary);assert.ok(!ordinary.sent.some(r=>r.route==='/media/continue'));assert.equal(jsonBody(ordinary.sent.find(r=>r.route==='/start')).analysis.continuity,undefined);
 const p=continuityPlayer({preparing:1});await prepareContinuation(p);assert.equal(jsonBody(p.sent.find(r=>r.route==='/start')).analysis.continuity,true);assert.equal(p.continuityState().epoch,'epoch-one');assert.equal(p.continuityState().phase,'preparing');p.advance(100);await flush();assert.equal(p.continuityState().phase,'boundary');assert.equal(p.sent.filter(r=>r.route==='/media/continue').length,1);assert.equal(p.sent.filter(r=>r.route==='/analysis/prepare').length,2);assert.equal(p.continuityState().prepareId,'successor-prepare-one');assert.equal(p.sent.filter(r=>r.route==='/player/start').length,1);
});
test('one boundary F and one original G precede native promotion without video reload',async()=>{
 const p=continuityPlayer({joining:1});await prepareContinuation(p);const src=p.elements.video.src,loads=p.elements.video.loads,analysisLoads=p.elements['analysis-video'].loads;
 await p.analysisFrame(1.2);assert.equal(p.continuityState().epoch,'epoch-one');assert.equal(p.continuityState().phase,'pair');await p.analysisFrame(1.3);assert.equal(p.continuityState().epoch,'epoch-one');assert.equal(p.continuityState().phase,'joining');await p.analysisFrame(1.4);assert.equal(p.sent.filter(r=>r.route==='/analysis/successor-frame').length,1);
 p.advance(100);await flush();assert.equal(p.continuityState().epoch,'epoch-one');p.advance(100);await flush();assert.equal(p.continuityState().epoch,'epoch-two');assert.equal(p.continuityState().count,1);
 const uploads=p.sent.filter(r=>['/frame','/analysis/successor-frame'].includes(r.route)).map(rgbPacket);assert.deepEqual(uploads.map(r=>r.header.media_us),[1100000,1200000,1300000]);assert.deepEqual(uploads.map(r=>r.header.epoch),['epoch-one','epoch-one','epoch-two']);assert.ok(uploads.every(r=>r.pixels.every(v=>v===128)));assert.equal(p.drawSources.length,3);assert.equal(p.elements.video.src,src);assert.equal(p.elements['analysis-video'].src,src);assert.equal(p.elements.video.loads,loads);assert.equal(p.elements['analysis-video'].loads,analysisLoads);assert.equal(p.sent.filter(r=>r.route==='/player/start').length,1);assert.equal(p.sent.filter(r=>r.route==='/analysis/transfer').length,1);
});
test('successor media mismatch stops original playback without replacement source',async()=>{
 const p=continuityPlayer({wrongMedia:true});await startLookahead(p);await p.analysisFrame(1.1);await flush();assert.equal(p.elements.video.paused,true);assert.ok(!p.sent.some(r=>r.route==='/analysis/prepare'));assert.equal(p.sent.filter(r=>r.route==='/media/prepare').length,1);
});
test('EOF before boundary cancels unused successor and closes original epoch',async()=>{
 const p=continuityPlayer();await prepareContinuation(p);await p.analysisFrame(2);await flush();await flush();assert.ok(p.sent.some(r=>r.route==='/analysis/cancel-successor'));const close=p.sent.find(r=>r.route==='/analysis/finish');assert.equal(jsonBody(close).capture_epoch,'epoch-one');assert.ok(!p.sent.some(r=>r.route==='/analysis/transfer'));assert.equal(p.elements.video.paused,false);
});
test('EOF after reserved boundary cannot invent the missing successor pair',async()=>{
 const p=continuityPlayer();await prepareContinuation(p);await p.analysisFrame(1.2);await p.analysisFrame(2);await flush();assert.equal(p.elements.video.paused,true);assert.ok(!p.sent.some(r=>r.route==='/analysis/successor-frame'||r.route==='/analysis/finish'));assert.match(p.elements.status.textContent,/missing successor boundary pair/);
});
test('EOF during joined transfer closes only committed final epoch with original G',async()=>{
 const p=continuityPlayer();await prepareContinuation(p);await p.analysisFrame(1.2);await p.analysisFrame(1.3);await p.analysisFrame(2);await flush();await flush();const close=p.sent.find(r=>r.route==='/analysis/finish');assert.ok(close);const body=jsonBody(close);assert.equal(body.capture_epoch,'epoch-two');assert.equal(body.final_observation.media_us,1300000);assert.equal(body.final_observation.observation.feed_id,'analysis-feed-2');assert.equal(p.drawSources.length,3);assert.equal(p.elements.video.paused,false);
});


function hspView(){return {configured:true,phase:'analysis_current',selection_checked:true,can_prepare:true,can_bind:false,can_play:false,can_enable_continuity:false,can_stop:true,can_cancel:true,continuity_duration_ms:15000,pending_action:null,device:{session_id:'device-session',qualification_id:'device-dossier',qualification_revision:4,state:'accepted'},source:{qualification_id:'source-dossier',qualification_revision:8,environment_sha256:'d'.repeat(64),model_sha256:'e'.repeat(64),state:'accepted',origin:'physical'},preparation:null,stream:null,continuity:null,error:null};}

test('HSP cleanup projection preserves accepted action identity and filters private details',async()=>{
 const p=hspPlayer();await inspectHspPlayer(p);
 p.setHspView({phase:'cleanup_observed',can_bind:false,can_play:false,cleanup:{trigger:'public_stop',stop_request_id:'original-stop',cancel_request_id:'original-cancel',owner_proof:'never-show',native_error:'never-show'},previous_lifecycles:[{cleanup_trigger:'player_retired',phase:'cleanup_confirmed',owner_proof:'never-show'}]});
 await p.elements['hsp-status'].onclick();
 const details=JSON.parse(p.elements['hsp-details'].textContent);
 assert.deepEqual(details.cleanup,{trigger:'public_stop',stop_request_id:'original-stop',cancel_request_id:'original-cancel'});
 assert.equal(details.previous_lifecycles[0].cleanup_trigger,'player_retired');
 assert.match(p.elements['hsp-status-message'].textContent,/Cleanup: HSP Stop requested/);
 assert.ok(!p.elements['hsp-details'].textContent.includes('never-show'));
 p.setHspView({cleanup:{trigger:'player_retired',stop_request_id:'original-stop',cancel_request_id:'original-cancel'}});
 await p.elements['hsp-status'].onclick();
 assert.match(p.elements['hsp-status-message'].textContent,/Cleanup: player retired/);
 assert.doesNotMatch(p.elements['hsp-status-message'].textContent,/Cleanup: HSP Stop requested/);
});
function hspPlayer({hspReply,...options}={}){
 let view=hspView();const other=options.replyOverride;
 const p=mediaPlayer({lookahead:true,timing:true,...options,hspConfigured:true,replyOverride:r=>{
  if(!r.route.startsWith('/player/hsp-'))return other?.(r);
  const chosen=hspReply?.(r,view);if(chosen)return chosen;
  if(r.route==='/player/hsp-prepare')view={...view,phase:'ready',can_prepare:false,can_bind:true,preparation:{request_id:'original-prepare',phase:'ready',remaining_ms:7000,worker_active:false,metadata_pending:false,history_state:'acknowledged'}};
  if(r.route==='/player/hsp-bind')view={...view,phase:'prefilling',can_bind:false,can_play:true,can_enable_continuity:true,stream:{request_id:'original-stream',phase:'prefilling',delivered_samples:2,active_source_generation:1,metadata_pending:false,stop_pending:false}};
  if(r.route==='/player/hsp-continuity-enable')view={...view,can_enable_continuity:false,continuity:{request_id:'original-continuity',phase:'enabled',current:true,generation:1,worker_active:true,worker_joined:false}};
  if(r.route==='/player/hsp-play')view={...view,phase:'playing',can_play:false,stream:{...view.stream,phase:'playing'}};
  if(r.route==='/player/hsp-stop')view={...view,phase:'stopped',pending_action:null,can_bind:false,can_play:false,stream:{...view.stream,phase:'stopped',stop_pending:false}};
  if(r.route==='/player/hsp-cancel')view={...view,phase:'cancelled',pending_action:null,can_bind:false,can_play:false,preparation:{...view.preparation,phase:'retired',worker_active:false}};
  return {ok:true,json:async()=>JSON.parse(JSON.stringify(view))};
 }});
 p.setHspView=patch=>{view={...view,...patch};};return p;
}
function hspAck(p,...ids){for(const id of ids){p.elements['hsp-'+id+'-ack'].checked=true;p.elements['hsp-'+id+'-ack'].onchange();}}
async function inspectHspPlayer(p){await startLookahead(p);await p.analysisFrame(1.1);await p.elements['hsp-inspect'].onclick();}
async function bindHspPlayer(p){await inspectHspPlayer(p);hspAck(p,'prepare');await p.elements['hsp-prepare'].onclick();hspAck(p,'environment','online');await p.elements['hsp-bind'].onclick();}

test('HSP video Play starts only visible timing and real analysis callbacks',async()=>{
 const p=hspPlayer();hspAck(p,'prepare','environment','online','play','continuity');await startLookahead(p);await p.analysisFrame(1.1);
 assert.ok(!p.sent.some(r=>r.route.startsWith('/player/hsp-')));assert.equal(p.elements['hsp-prepare'].disabled,true);assert.equal(p.elements['hsp-play-ack'].checked,false);
 assert.equal(p.sent.filter(r=>r.route==='/player/start').length,1);assert.ok(p.sent.some(r=>r.route==='/player/timing-observe'));assert.deepEqual(p.drawSources,['analysis']);
 const unconfigured=player();unconfigured.play();await unconfigured.frame(.1);await unconfigured.elements['hsp-inspect'].onclick();await unconfigured.elements['hsp-stop'].onclick();assert.ok(!unconfigured.sent.some(r=>r.route.startsWith('/player/hsp-')));
});
test('HSP Prepare Bind and Play require separate fresh consent and send intent only',async()=>{
 const p=hspPlayer();await inspectHspPlayer(p);await p.elements['hsp-prepare'].onclick();assert.ok(!p.sent.some(r=>r.route==='/player/hsp-prepare'));
 hspAck(p,'prepare');await p.elements['hsp-prepare'].onclick();assert.equal(p.elements['hsp-prepare-ack'].checked,false);await p.elements['hsp-bind'].onclick();hspAck(p,'environment');await p.elements['hsp-bind'].onclick();assert.ok(!p.sent.some(r=>r.route==='/player/hsp-bind'));
 hspAck(p,'online');await p.elements['hsp-bind'].onclick();await p.elements['hsp-play'].onclick();assert.ok(!p.sent.some(r=>r.route==='/player/hsp-play'));hspAck(p,'play');await p.elements['hsp-play'].onclick();await p.elements['hsp-play'].onclick();
 for(const [route,body] of [['prepare',{allow_synchronization:true}],['bind',{acknowledge_current_environment:true,allow_online_control:true}],['play',{allow_play:true}]]){const requests=p.sent.filter(r=>r.route==='/player/hsp-'+route);assert.equal(requests.length,1);assert.deepEqual(jsonBody(requests[0]),body);assert.equal(requests[0].headers['X-Pulsar-Player'],'player-1');}
 assert.match(p.elements['hsp-selection'].textContent,/device-dossier/);assert.match(p.elements['hsp-selection'].textContent,/dddd/);assert.match(p.elements['hsp-status-message'].textContent,/Physical response and stopping remain unconfirmed/);
});
test('HSP pending metadata and expired preparation never confer readiness or retry automatically',async()=>{
 const p=hspPlayer();await inspectHspPlayer(p);p.setHspView({phase:'preparing',pending_action:'prepare',can_prepare:false,can_bind:false,preparation:{request_id:'original-prepare',phase:'checking',remaining_ms:null,worker_active:true,metadata_pending:true}});await p.elements['hsp-status'].onclick();hspAck(p,'environment','online');await p.elements['hsp-bind'].onclick();assert.match(p.elements['hsp-status-message'].textContent,/metadata pending/);assert.ok(!p.sent.some(r=>r.route==='/player/hsp-bind'));
 p.setHspView({phase:'expired',pending_action:null,can_prepare:true,preparation:{request_id:'original-prepare',phase:'retired',remaining_ms:0,worker_active:false,metadata_pending:false}});p.advance(500);await flush();assert.match(p.elements['hsp-status-message'].textContent,/Expiry requires fresh explicit preparation/);assert.equal(p.elements['hsp-prepare'].disabled,true);assert.ok(!p.sent.some(r=>r.route==='/player/hsp-prepare'));
});
test('HSP Stop bypasses a held Prepare and late completion cannot reenable Bind',async()=>{
 const p=hspPlayer({holdRoute:'/player/hsp-prepare'});await inspectHspPlayer(p);hspAck(p,'prepare');const pending=p.elements['hsp-prepare'].onclick();await flush();p.advance(250);await flush();await p.frame(.2,2);await flush();
 assert.ok(p.sent.filter(r=>r.route==='/player/timing-observe').length>=2);assert.equal(p.elements['hsp-stop'].disabled,false);await p.elements['hsp-stop'].onclick();assert.match(p.elements['hsp-status-message'].textContent,/stopped/);p.releaseRoute();await pending;
 assert.match(p.elements['hsp-status-message'].textContent,/stopped/);hspAck(p,'environment','online');assert.equal(p.elements['hsp-bind'].disabled,true);assert.equal(p.sent.filter(r=>r.route==='/player/hsp-prepare').length,1);
});
test('HSP Cancel bypasses held original inspection and fences its stale Ready report',async()=>{
 const p=hspPlayer({holdRoute:'/player/hsp-preparation-status'});await inspectHspPlayer(p);hspAck(p,'prepare');await p.elements['hsp-prepare'].onclick();const pending=p.elements['hsp-preparation-status'].onclick();await flush();await p.elements['hsp-cancel'].onclick();p.releaseRoute();await pending;
 assert.match(p.elements['hsp-status-message'].textContent,/cancelled/);assert.equal(p.elements['hsp-bind'].disabled,true);assert.equal(p.sent.filter(r=>r.route==='/player/hsp-cancel').length,1);
});
test('HSP lost Stop acknowledgment can retry original retained scope after player retirement',async()=>{
 let stops=0;const p=hspPlayer({hspReply:r=>r.route==='/player/hsp-stop'&&++stops===1?{ok:false,json:async()=>({error:'original stop acknowledgment unavailable'})}:null});await bindHspPlayer(p);hspAck(p,'play');await p.elements['hsp-play'].onclick();p.elements.video.pause();await flush();
 await p.elements['hsp-stop'].onclick();assert.match(p.elements['hsp-status-message'].textContent,/Retry the same original stop/);await p.elements['hsp-stop'].onclick();await p.elements['hsp-status'].onclick();
 const requests=p.sent.filter(r=>r.route==='/player/hsp-stop');assert.equal(requests.length,2);assert.deepEqual(requests[0].headers,requests[1].headers);assert.equal(requests[0].body.length,0);assert.equal(requests[1].body.length,0);assert.match(p.elements['hsp-status-message'].textContent,/original history only/);
});
test('HSP old inspection cannot display or enable a replacement player',async()=>{
 const p=hspPlayer({holdRoute:'/player/hsp-inspect'});await startLookahead(p);await p.analysisFrame(1.1);const pending=p.elements['hsp-inspect'].onclick();await flush();p.elements.stop.onclick();p.play();await flush();await p.frame(.2,2);await flush();const message=p.elements['hsp-status-message'].textContent;p.releaseRoute();await pending;
 assert.equal(p.elements['hsp-status-message'].textContent,message);assert.equal(p.elements['hsp-selection'].textContent,'');hspAck(p,'prepare');await p.elements['hsp-prepare'].onclick();assert.ok(!p.sent.some(r=>r.route==='/player/hsp-prepare'));
});
test('HSP continuity must be explicitly enabled and observed before selected device Play',async()=>{
 const p=hspPlayer();await bindHspPlayer(p);await p.elements['hsp-continuity-enable'].onclick();assert.ok(!p.sent.some(r=>r.route==='/player/hsp-continuity-enable'));hspAck(p,'continuity','play');await p.elements['hsp-play'].onclick();assert.ok(!p.sent.some(r=>r.route==='/player/hsp-play'));
 await p.elements['hsp-continuity-enable'].onclick();assert.deepEqual(jsonBody(p.sent.find(r=>r.route==='/player/hsp-continuity-enable')),{allow_continuity:true});assert.match(p.elements['hsp-continuity-duration'].textContent,/15000 ms/);assert.ok(!p.sent.some(r=>r.route==='/player/hsp-play'));await p.elements['hsp-play'].onclick();assert.equal(p.sent.filter(r=>r.route==='/player/hsp-play').length,1);
});
test('HSP continuity refusal or uncertainty never triggers the requested Play',async()=>{
 for(const refused of [true,false]){const p=hspPlayer({hspReply:(r,v)=>r.route==='/player/hsp-continuity-enable'?(refused?{ok:false,json:async()=>({error:'continuity qualification withdrawn'})}:{ok:true,json:async()=>({...v,pending_action:'continuity_enable',continuity:{request_id:'original-continuity',phase:'pending',current:false}})}):null});await bindHspPlayer(p);hspAck(p,'continuity','play');await p.elements['hsp-continuity-enable'].onclick();await p.elements['hsp-play'].onclick();assert.equal(p.sent.filter(r=>r.route==='/player/hsp-play').length,0);assert.match(p.elements['hsp-status-message'].textContent,refused?/qualification withdrawn/:/Pending/);}
});
test('HSP native view display allowlists fields and historical state cannot grant playback',async()=>{
 const p=hspPlayer();await inspectHspPlayer(p);p.setHspView({phase:'history_only',can_prepare:false,can_bind:false,can_play:false,device:{session_id:'selected-session',owner_proof:'never-show',credential:'never-show'},source:{qualification_id:'source-dossier',url:'never-show'},stream:{request_id:'original-stream',phase:'unknown',stop_pending:true,metadata_pending:true,command:'never-show'}});await p.elements['hsp-status'].onclick();hspAck(p,'play');await p.elements['hsp-play'].onclick();assert.ok(!p.sent.some(r=>r.route==='/player/hsp-play'));assert.match(p.elements['hsp-status-message'].textContent,/Stop acknowledgment pending or unknown/);assert.ok(!(p.elements['hsp-selection'].textContent+p.elements['hsp-details'].textContent).includes('never-show'));
});
test('HSP natural analysis tail keeps visible player; pause and seek still retire it',async()=>{
 for(const action of ['pause','seek']){const p=hspPlayer();await bindHspPlayer(p);hspAck(p,'play');await p.elements['hsp-play'].onclick();p.analysisEnd(2);await flushFinish();assert.equal(p.elements.video.paused,false);assert.equal(p.sent.filter(r=>r.route==='/analysis/finish').length,1);assert.ok(!p.sent.some(r=>r.route==='/player/stop'));
  if(action==='pause')p.elements.video.pause();else p.events.seeking();await flush();assert.equal(p.sent.filter(r=>r.route==='/player/stop').length,1);assert.equal(p.elements['hsp-play'].disabled,true);assert.equal(p.elements['hsp-stop'].disabled,false);assert.equal(p.sent.filter(r=>r.route==='/player/hsp-bind').length,1);assert.equal(p.sent.filter(r=>r.route==='/player/hsp-play').length,1);}
});


// These authored observations drive the real client callback/transfer state machine.
// They are ordering tests, not evidence of native promotion or physical playback.
function twoTransferHspPlayer(){
 let selectedPlayer,policy,current=1,upload=null,nextUpload=null,successor=null,sequence=0;
 const ok=data=>({ok:true,json:async()=>data});
 const epoch=n=>n===1?'epoch-one':n===2?'epoch-two':'epoch-three';
 const feed=n=>({feed_id:'analysis-feed-'+n,role:'lookahead',generation:selectedPlayer.generation,player:selectedPlayer,capture_epoch:epoch(n),policy});
 const base=()=>({active:true,identity:{epoch:epoch(current)},options:{lease_ms:30000,policy:{maximum_analysis_age_ns:2e9}},inference:{completed:0}});
 const transfer=phase=>({original_request_id:'transfer-'+current,preparation_request_id:'successor-prepare-'+current,predecessor:{epoch:epoch(current)},successor:{epoch:epoch(current+1)},boundary:{native_only:true},phase,predecessor_worker_joined:phase!=='boundary_reserved'&&phase!=='joining_predecessor',predecessor_delivery_settled:phase!=='boundary_reserved'&&phase!=='joining_predecessor',successor_pair_accepted:!!nextUpload});
 const pending=phase=>({...base(),browser:{analysis_feed:feed(current),analysis_last_upload:upload,analysis_successor:{original_request_id:'successor-prepare-'+current,feed:successor,report:{original_request_id:'successor-prepare-'+current,predecessor:{epoch:epoch(current)},successor:{identity:{epoch:epoch(current+1)},options:{lease_ms:30000}},phase:'prepared'},transfer:phase?transfer(phase):null,last_upload:nextUpload}}});
 const p=hspPlayer({replyOverride:r=>{
  if(r.route==='/start'){const start=jsonBody(r);policy=start.analysis.policy;selectedPlayer={launch_id:r.headers['X-Pulsar-Launch'],page_id:r.headers['X-Pulsar-Page'],player_id:r.headers['X-Pulsar-Player'],source_id:r.headers['X-Pulsar-Source'],generation:Number(r.headers['X-Pulsar-Generation']),time_basis:start.source_time_basis};}
  if(r.route==='/frame'){const h=rgbPacket(r).header;upload={media_us:h.media_us,observation:h.analysis};return ok({...base(),browser:{analysis_feed:feed(current),analysis_last_upload:upload,analysis_continuity:{enabled:true,prepare_due:current<3,media_preparation_request_id:'media-prepare-'+current}}});}
  if(r.route==='/media/continue'||r.route==='/media/continue-status')return ok({...readyMedia,preparation_request_id:'media-prepare-'+(current+1)});
  if(r.route==='/player/timing-next-open')return ok({pending:true});
  if(r.route==='/player/timing-next-probe')return ok({sequence:String(++sequence),probe_id:'next-probe-'+sequence,probe_timeout_ms:1000,observation_timeout_ms:3000});
  if(r.route==='/player/timing-next-observe')return ok({current:true,qualified:false,observation:{sequence:jsonBody(r).sequence}});
  if(r.route==='/analysis/prepare'){successor=feed(current+1);nextUpload=null;return ok(pending());}
  if(r.route==='/analysis/transfer')return ok(pending('awaiting_successor_pair'));
  if(r.route==='/analysis/successor-frame'){const h=rgbPacket(r).header;nextUpload={media_us:h.media_us,observation:h.analysis};return ok(pending('ready_for_transfer'));}
  if(r.route==='/analysis/transfer-status'){if(!nextUpload)return ok(pending('awaiting_successor_pair'));const completed=transfer('transferred');current++;upload=nextUpload;return ok({...base(),browser:{analysis_feed:feed(current),analysis_last_upload:upload,analysis_transfer_completed:completed,analysis_continuity:{enabled:true,prepare_due:current<3,media_preparation_request_id:'media-prepare-'+current}}});}
  if(r.route==='/analysis/finish'||r.route==='/analysis/finish-status')return ok({...base(),browser:{analysis_feed:feed(current),analysis_last_upload:upload,analysis_finish_request_id:'finish-'+current,analysis_final_result_matches:false},input_close:{original_request_id:'finish-'+current,phase:'sealed',disposition:'accepted',worker_joined:true,delivery_settled:true}});
 }});p.elements['analysis-continuity'].checked=true;return p;
}
test('HSP two committed analysis transfers preserve one Bind and Play without synthetic callbacks',async()=>{
 const p=twoTransferHspPlayer();await bindHspPlayer(p);hspAck(p,'continuity');await p.elements['hsp-continuity-enable'].onclick();hspAck(p,'play');await p.elements['hsp-play'].onclick();const src=p.elements.video.src,loads=p.elements.video.loads;
 await flush();await p.frame(.2,2);await flush();await flush();assert.equal(p.continuityState().phase,'boundary');await p.analysisFrame(1.2);await p.analysisFrame(1.3);p.advance(100);await flush();await flush();assert.equal(p.continuityState().count,1);
 await p.frame(.3,3);await flush();await p.analysisFrame(1.4);await flush();await flush();await p.frame(.4,4);await flush();await flush();assert.equal(p.continuityState().phase,'boundary');await p.analysisFrame(1.5);await p.analysisFrame(1.6);p.advance(100);await flush();await flush();assert.equal(p.continuityState().count,2);assert.equal(p.continuityState().epoch,'epoch-three');
 p.setHspView({stream:{request_id:'original-stream',phase:'playing',active_source_generation:3,delivered_samples:5}});await p.elements['hsp-status'].onclick();assert.match(p.elements['hsp-details'].textContent,/original-stream/);assert.match(p.elements['hsp-details'].textContent,/"active_source_generation": 3/);
 const uploads=p.sent.filter(r=>r.route==='/frame'||r.route==='/analysis/successor-frame').map(rgbPacket);assert.deepEqual(uploads.map(x=>x.header.media_us),[1100000,1200000,1300000,1400000,1500000,1600000]);assert.ok(uploads.every(x=>x.pixels.every(v=>v===128)));assert.equal(p.drawSources.length,6);assert.equal(p.elements.video.src,src);assert.equal(p.elements.video.loads,loads);assert.equal(p.sent.filter(r=>r.route==='/player/start').length,1);assert.equal(p.sent.filter(r=>r.route==='/analysis/transfer').length,2);
 for(const route of ['prepare','bind','play','continuity-enable'])assert.equal(p.sent.filter(r=>r.route==='/player/hsp-'+route).length,1);p.analysisEnd(2);await flushFinish();assert.equal(jsonBody(p.sent.find(r=>r.route==='/analysis/finish')).capture_epoch,'epoch-three');assert.equal(p.elements.video.paused,false);
});


test('HSP held Bind cannot block Stop or restore its stale Prefilling reply',async()=>{
 const p=hspPlayer({holdRoute:'/player/hsp-bind'});await inspectHspPlayer(p);hspAck(p,'prepare');await p.elements['hsp-prepare'].onclick();hspAck(p,'environment','online');const pending=p.elements['hsp-bind'].onclick();await flush();await p.elements['hsp-stop'].onclick();p.releaseRoute();await pending;hspAck(p,'play');await p.elements['hsp-play'].onclick();
 assert.equal(p.sent.filter(r=>r.route==='/player/hsp-stop').length,1);assert.ok(!p.sent.some(r=>r.route==='/player/hsp-play'));assert.match(p.elements['hsp-status-message'].textContent,/stopped/);
});
test('HSP native qualification refusal cannot be overridden by browser acknowledgments',async()=>{
 const p=hspPlayer();await inspectHspPlayer(p);p.setHspView({phase:'refused',can_prepare:false,can_bind:false,can_play:false,can_enable_continuity:false,error:'source qualification Pending/Synthetic; admission refused',source:{state:'pending',origin:'synthetic'}});await p.elements['hsp-inspect'].onclick();hspAck(p,'prepare','environment','online','continuity','play');for(const id of ['prepare','bind','play','continuity-enable'])await p.elements['hsp-'+id].onclick();
 assert.ok(!p.sent.some(r=>/^\/player\/hsp-(prepare|bind|play|continuity-enable)$/.test(r.route)));assert.match(p.elements['hsp-status-message'].textContent,/Pending\/Synthetic/);assert.match(p.elements['hsp-selection'].textContent,/synthetic/);
});


test('HSP pending Inspect adopts its later native status without clearing valid consent',async()=>{
 const p=hspPlayer({hspReply:(r,v)=>r.route==='/player/hsp-inspect'?{ok:true,json:async()=>({...v,pending_action:'inspect',selection_checked:false,can_prepare:false})}:null});await inspectHspPlayer(p);assert.match(p.elements['hsp-status-message'].textContent,/Pending: inspect/);assert.equal(p.elements['hsp-prepare'].disabled,true);
 p.advance(500);await flush();assert.equal(p.sent.filter(r=>r.route==='/player/hsp-status').length,1);assert.match(p.elements['hsp-status-message'].textContent,/analysis_current/);hspAck(p,'prepare');assert.equal(p.elements['hsp-prepare'].disabled,false);await p.elements['hsp-status'].onclick();assert.equal(p.elements['hsp-prepare-ack'].checked,true);assert.equal(p.elements['hsp-prepare'].disabled,false);await p.elements['hsp-prepare'].onclick();assert.equal(p.sent.filter(r=>r.route==='/player/hsp-prepare').length,1);
 const uninspected=hspPlayer();await startLookahead(uninspected);await uninspected.analysisFrame(1.1);await uninspected.elements['hsp-status'].onclick();hspAck(uninspected,'prepare');assert.equal(uninspected.elements['hsp-prepare'].disabled,true);
});


test('HSP lifecycle archive is bounded and allowlisted; explicit Inspect resets consent without playback',async()=>{
 const p=hspPlayer();await inspectHspPlayer(p);const previous_lifecycles=Array.from({length:10},(_,n)=>({player_id:'previous-player-'+n,prepare_request_id:'prepare-'+n,stream_request_id:'stream-'+n,play_request_id:'play-'+n,enable_request_id:null,stop_request_id:'stop-'+n,cancel_request_id:null,phase:'cleanup_confirmed',owner_proof:'archive-secret',raw:{credential:'archive-secret'},media_url:'archive-secret'}));p.setHspView({previous_lifecycles});hspAck(p,'prepare','environment','online','play','continuity');await p.elements['hsp-status'].onclick();
 const details=JSON.parse(p.elements['hsp-details'].textContent);assert.equal(details.previous_lifecycles.length,8);assert.equal(details.previous_lifecycles[0].player_id,'previous-player-2');assert.equal(details.previous_lifecycles.at(-1).stop_request_id,'stop-9');assert.equal(details.previous_lifecycles.at(-1).enable_request_id,null);assert.equal(details.previous_lifecycles.at(-1).phase,'cleanup_confirmed');assert.deepEqual(Object.keys(details.previous_lifecycles[0]),['player_id','prepare_request_id','stream_request_id','play_request_id','enable_request_id','stop_request_id','cancel_request_id','phase']);assert.ok(!p.elements['hsp-details'].textContent.includes('archive-secret'));assert.equal(p.elements['hsp-prepare-ack'].checked,true);
 await p.elements['hsp-inspect'].onclick();for(const id of ['prepare','environment','online','play','continuity'])assert.equal(p.elements['hsp-'+id+'-ack'].checked,false);assert.ok(!p.sent.some(r=>/^\/player\/hsp-(prepare|bind|play|continuity-enable)$/.test(r.route)));assert.equal(JSON.parse(p.elements['hsp-details'].textContent).previous_lifecycles.length,8);
});
