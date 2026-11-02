const CAPACITY=4096;
const statistics=values=>{const sorted=[...values].sort((a,b)=>a-b);return {count:sorted.length,meanMs:sorted.length?sorted.reduce((a,b)=>a+b,0)/sorted.length:null,p95Ms:sorted[Math.ceil(sorted.length*.95)-1]??null,p99Ms:sorted[Math.ceil(sorted.length*.99)-1]??null,maxMs:sorted.at(-1)??null};};

// Keep invalid clock evidence separate from game transport. Never invent a zero duration.
const clockEvidence=value=>Number.isFinite(value)?value:typeof value==='number'?String(value):value??null;
const arithmeticUncertainty=values=>Number.EPSILON*values.reduce((sum,value)=>sum+Math.abs(value),0);
export function workerClockHandshake({windowOrigin,windowSent,windowReceived,workerOrigin,workerReceived,workerSent}){
 const fields={windowOrigin,windowSent,windowReceived,workerOrigin,workerReceived,workerSent},values=Object.values(fields);
 const finite=values.every(Number.isFinite),contextTimes=[windowSent,windowReceived,workerReceived,workerSent].every(Number.isFinite);
 // Subtract context origins before combining local timestamps to limit cancellation.
 const receiveMs=finite?(workerOrigin-windowOrigin)+(workerReceived-windowSent):null;
 const returnMs=finite?(windowOrigin-workerOrigin)+(windowReceived-workerSent):null;
 const roundTripMs=contextTimes?(windowReceived-windowSent)-(workerSent-workerReceived):null;
 const reason=!finite?'invalid-clock-timestamp':windowReceived<windowSent||workerSent<workerReceived?'nonmonotonic-context-clock':receiveMs<0||returnMs<0||roundTripMs<0?'negative-handshake-delivery':null;
 return {valid:reason===null,reason,windowOrigin:Number.isFinite(windowOrigin)?windowOrigin:null,workerOrigin:Number.isFinite(workerOrigin)?workerOrigin:null,roundTripMs,receiveMs,returnMs,arithmeticUncertaintyMs:finite?arithmeticUncertainty(values):null,clockResolutionMs:null,raw:Object.fromEntries(Object.entries(fields).map(([key,value])=>[key,clockEvidence(value)]))};
}
function deliveryInterval(clock,localSent,remoteReceived,toWorker){
 const values=[clock?.windowOrigin,clock?.workerOrigin,localSent,remoteReceived],finite=values.every(Number.isFinite);
 const rawMs=finite?(toWorker?clock.workerOrigin-clock.windowOrigin:clock.windowOrigin-clock.workerOrigin)+(remoteReceived-localSent):null;
 const arithmeticUncertaintyMs=finite?arithmeticUncertainty(values):null;
 const reason=!finite?'invalid-clock-timestamp':!clock.valid?'invalid-clock-handshake':rawMs<0?Math.abs(rawMs)<=arithmeticUncertaintyMs?'below-zero-within-arithmetic-uncertainty':'negative-delivery-interval':null;
 return {valid:reason===null,reason,valueMs:reason===null?rawMs:null,rawMs,arithmeticUncertaintyMs,clockResolutionMs:null,uncertaintyBasis:'Floating-point arithmetic only. Browser timer coarsening is unknown.',raw:toWorker?{windowSent:clockEvidence(localSent),workerReceived:clockEvidence(remoteReceived)}:{workerSent:clockEvidence(localSent),windowReceived:clockEvidence(remoteReceived)}};
}
export function workerDeliveryInterval(clock,{workerSent,windowReceived}){return deliveryInterval(clock,workerSent,windowReceived,false);}
export function workerInputDeliveryInterval(clock,{windowSent,workerReceived}){return deliveryInterval(clock,windowSent,workerReceived,true);}

export function createCpuProfile(now,timeOrigin){
 const inputs=new Float64Array(CAPACITY*8),jobs=new Array(CAPACITY),jobBytes=new Uint32Array(CAPACITY),stages=new Map();
 let enabled=false,inputCount=0,jobCount=0,inputDropped=0,jobDropped=0,pendingFloor=0,lastConsumed=0,lastSubmitted=0,retainedJobBytes=0;
 const api={
  timeOrigin,
  get enabled(){return enabled;},
  enable(value){enabled=Boolean(value);if(enabled){inputCount=jobCount=inputDropped=jobDropped=pendingFloor=lastConsumed=lastSubmitted=retainedJobBytes=0;inputs.fill(0);jobs.fill(undefined);jobBytes.fill(0);stages.clear();}},
  begin(stage){if(!enabled)return;let row=stages.get(stage);if(!row){row={start:null,values:new Float64Array(CAPACITY),count:0,dropped:0};stages.set(stage,row);}row.start=now();},
  end(stage){if(!enabled)return;const row=stages.get(stage);if(!row||row.start===null)return;const duration=now()-row.start;row.start=null;row.values[row.count++%CAPACITY]=duration;if(row.count>CAPACITY)row.dropped++;},
  input(sequence,eventAt,trusted){if(!enabled)return;const offset=(inputCount++%CAPACITY)*8;inputs[offset]=sequence;inputs[offset+1]=eventAt;inputs[offset+2]=trusted?1:0;inputs[offset+3]=inputs[offset+4]=inputs[offset+5]=inputs[offset+6]=inputs[offset+7]=0;if(inputCount>CAPACITY)inputDropped++;},
  consume(sequence,step){if(!enabled||sequence<=lastConsumed||sequence<=pendingFloor)return;const at=now();for(let i=Math.max(0,inputCount-CAPACITY);i<inputCount;i++){const o=(i%CAPACITY)*8;if(inputs[o]<=pendingFloor||inputs[o]>sequence||inputs[o+7])continue;inputs[o+3]=step;inputs[o+4]=at;inputs[o+7]=1;}lastConsumed=sequence;},
  submitFrame(token){if(!enabled||token<=lastSubmitted)return;lastSubmitted=token;let at;for(let i=Math.max(0,inputCount-CAPACITY);i<inputCount;i++){const o=(i%CAPACITY)*8;if(inputs[o+7]!==1)continue;at??=now();inputs[o+5]=token;inputs[o+6]=at;inputs[o+7]=2;}},
  clearInput(){if(!enabled)return;pendingFloor=Math.max(pendingFloor,lastConsumed,inputCount?inputs[((inputCount-1)%CAPACITY)*8]:0);lastSubmitted=0;for(let i=Math.max(0,inputCount-CAPACITY);i<inputCount;i++){const o=(i%CAPACITY)*8;if(inputs[o+7]===1)inputs[o+7]=3;}},
  worker(record){if(!enabled)return;const index=jobCount++%CAPACITY;retainedJobBytes-=jobBytes[index];jobs[index]=structuredClone(record);jobBytes[index]=JSON.stringify(record).length*2;retainedJobBytes+=jobBytes[index];if(jobCount>CAPACITY)jobDropped++;},
  snapshot(){const records=[];for(let i=Math.max(0,inputCount-CAPACITY);i<inputCount;i++){const o=(i%CAPACITY)*8;records.push({sequence:inputs[o],eventAt:inputs[o+1],trusted:Boolean(inputs[o+2]),step:inputs[o+7]?inputs[o+3]:null,token:inputs[o+7]===2?inputs[o+5]:null,eventToConsumeMs:inputs[o+7]?inputs[o+4]-inputs[o+1]:null,consumeToSubmitMs:inputs[o+7]===2?inputs[o+6]-inputs[o+4]:null});}
   return {enabled,timeOrigin,stages:Object.fromEntries([...stages].map(([name,row])=>[name,{...statistics(row.values.subarray(0,Math.min(row.count,CAPACITY))),dropped:row.dropped}])),inputs:records,inputLatencies:{eventToConsume:statistics(records.filter(r=>r.eventToConsumeMs!==null).map(r=>r.eventToConsumeMs)),consumeToSubmit:statistics(records.filter(r=>r.consumeToSubmitMs!==null).map(r=>r.consumeToSubmitMs))},workers:Array.from({length:Math.min(jobCount,CAPACITY)},(_,i)=>jobs[(Math.max(0,jobCount-CAPACITY)+i)%CAPACITY]),retainedBytes:inputs.byteLength+jobBytes.byteLength+CAPACITY*8+stages.size*CAPACITY*8+retainedJobBytes,retainedBytesBasis:'Typed arrays, reference slots, and UTF-16 serialized job sizes. Engine object overhead is excluded.',dropped:{inputs:inputDropped,workers:jobDropped}};
  },
 };
 return api;
}

// Long animation frames include browser render work outside the Rust stage spans.
export function observeRenderMainThread(Observer=globalThis.PerformanceObserver){
 const supported=Boolean(Observer?.supportedEntryTypes?.includes('long-animation-frame'));
 const records=new Array(CAPACITY);let count=0,observer;
 const accept=entries=>{for(const entry of entries){records[count++%CAPACITY]={startTime:entry.startTime,duration:entry.duration,renderStart:entry.renderStart,styleAndLayoutStart:entry.styleAndLayoutStart,blockingDuration:entry.blockingDuration};}};
 if(supported){observer=new Observer(list=>accept(list.getEntries()));observer.observe({type:'long-animation-frame'});}
 return {stop(){if(observer){accept(observer.takeRecords());observer.disconnect();}return {supported,thresholdMs:50,completeFrameTrace:false,records:Array.from({length:Math.min(count,CAPACITY)},(_,i)=>records[(Math.max(0,count-CAPACITY)+i)%CAPACITY]),dropped:Math.max(0,count-CAPACITY)};}};
}
