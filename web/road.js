// One world owns this transport, its immutable sources, and its unread result.
import {workerClockHandshake,workerDeliveryInterval,workerInputDeliveryInterval} from './cpu-profile.js';
const MAX_U64=18446744073709551615n;
const u64=value=>typeof value==='string'&&/^(0|[1-9][0-9]{0,19})$/.test(value)&&BigInt(value)<=MAX_U64;
const identityValid=id=>id?.kind==='road'&&u64(id.session)&&u64(id.revision)&&u64(id.sequence)&&Number.isInteger(id.owner)&&id.owner>=0&&id.owner<=0xffffffff;
const same=(a,b)=>identityValid(a)&&identityValid(b)&&a.session===b.session&&a.revision===b.revision&&a.owner===b.owner&&a.sequence===b.sequence;
const fromBytes=bytes=>{if(!(bytes instanceof Uint8Array)||bytes.length!==28)throw Error('Invalid road identity.');const v=new DataView(bytes.buffer,bytes.byteOffset,28);return {kind:'road',session:v.getBigUint64(0,true).toString(),revision:v.getBigUint64(8,true).toString(),owner:v.getUint32(16,true),sequence:v.getBigUint64(20,true).toString()};};
const metric=value=>({valid:Number.isFinite(value)&&value>=0,valueMs:Number.isFinite(value)&&value>=0?value:null,rawMs:Number.isFinite(value)?value:String(value),reason:!Number.isFinite(value)?'invalid-duration':value<0?'negative-duration':null});
export function createRoad(worker,profile,now=()=>performance.now()){
 let state='Idle',job=null,result=null,failure=null,disposed=false,initialized=false,capacityLimit=2097152,lastSequence=0n,wasmBytes=0,pendingInstalls=0;
 const sources=new Array(3).fill(null);
 let clock=null,handshakePending=false,packingStart=null,packingMs=null,publishedRecord=null;
 const diagnostic=fn=>{if(profile?.enabled){try{fn();}catch{/* Diagnostics never change transport ownership. */}}};
 const emit=record=>diagnostic(()=>profile.worker(record));
 const releaseRecord=(record,reason)=>{if(!record)return;diagnostic(()=>{record.releaseReason=reason;record.stagingReleasedAt=now();record.physicalReleaseAt=record.transferBuffersReleasedAt??record.stagingReleasedAt;record.totalJobMs=metric(record.stagingReleasedAt-record.mainCopyStart);emit(record);});};
 const clear=()=>{job=null;result=null;sources.fill(null);publishedRecord=null;state='Idle';initialized=false;wasmBytes=0;pendingInstalls=0;handshakePending=false;packingStart=packingMs=null;};
 const fail=message=>{if(disposed)return;failure=String(message);worker.terminate();diagnostic(()=>{if(job?.record)job.record.error=failure;for(const s of sources)if(s?.record)s.record.error=failure;emit({kind:'road',phase:'failure',error:failure});});releaseRecord(job?.record,'failure-termination');for(const s of sources)releaseRecord(s?.record,'failure-termination');releaseRecord(publishedRecord,'failure-termination');clear();state='Failed';};
 const live=()=>{if(disposed)throw Error('The road worker is disposed.');if(failure!==null)throw Error(failure);};
 const profileHandshake=()=>diagnostic(()=>{if(disposed||failure!==null||handshakePending)return;const sent=now();worker.postMessage({type:'handshake',windowOrigin:profile.timeOrigin,windowSent:sent});handshakePending=true;});
 const timing=(record,data,received)=>diagnostic(()=>{
  record.resultReceived=received;record.worker=data.cpuProfile??null;
  if(!data.cpuProfile){record.timing={valid:false,reason:'missing-worker-diagnostics'};return;}
  const raw=data.cpuProfile;
  const returned=workerDeliveryInterval(clock,{workerSent:raw.resultPostStart,windowReceived:received});
  const submitted=workerInputDeliveryInterval(clock,{windowSent:record.messageSubmitStart,workerReceived:raw.workerReceived});
  if(raw.workerOrigin!==clock?.workerOrigin)for(const interval of [returned,submitted]){interval.valid=false;interval.valueMs=null;interval.reason='worker-origin-mismatch';}
  record.clockHandshake=clock;record.timing={returnDelivery:returned,submitDelivery:submitted};record.returnDeliveryMs=returned.valueMs;record.submitDeliveryMs=submitted.valueMs;
  for(const key of ['workerReadyWaitMs','inputCopyMs','decodeMs','computeMs','extractionMs','installWithInputCopyMs'])if(key in raw)record.timing[key]=metric(raw[key]);
 });
 worker.addEventListener('message',({data})=>{
  if(disposed||failure!==null)return;
  let received;diagnostic(()=>{received=now();});
  if(!data||typeof data!=='object'){fail('Malformed road worker message.');return;}
  if(data.type==='fatal'){fail(data.error??'The road worker stopped.');return;}
  if(data.type!=='handshake'&&(!Number.isSafeInteger(data.wasmBytes)||data.wasmBytes<0||data.wasmBytes>capacityLimit)){fail('Road WASM capacity exceeds admission.');return;}
  if(data.type==='initialized'){wasmBytes=data.wasmBytes;initialized=true;return;}
  if(data.type==='handshake'){
   handshakePending=false;
   diagnostic(()=>{clock=workerClockHandshake({...data,windowReceived:received});emit({kind:'road',phase:'initialization',clockHandshake:clock,worker:{initializationMs:data.initializationMs??null,retainedWasmBytes:data.wasmBytes??null}});});return;
  }
  if(data.type==='installed'){
   const id=data.identity;if(!identityValid(id)||id.owner>=3){fail('Malformed road installation identity.');return;}const source=sources[id.owner];
   if(!source?.pending||!same(id,source.identity)){emit({kind:'road',phase:'rejected',identity:id,reason:'stale-installation'});return;}
   if(data.error!==undefined){fail(data.error);return;}
   if(!Number.isSafeInteger(data.wasmBytes)||data.wasmBytes<0){fail('Invalid road WASM capacity.');return;}
   source.pending=false;pendingInstalls--;source.profiled=false;wasmBytes=data.wasmBytes;
   if(source.record){diagnostic(()=>{timing(source.record,data,received);releaseRecord(source.record,'installation-complete');});source.record=null;}
   return;
  }
  // Replies cannot acquire ownership. Only the currently submitted identity can complete.
  if(data.type!=='result'||!identityValid(data.identity)){fail('Malformed road result identity.');return;}
  if(state!=='Busy'||!same(data.identity,job?.identity)){emit({kind:'road',phase:'rejected',identity:data.identity,reason:BigInt(data.identity.sequence)===lastSequence?'duplicate-result':'stale-result'});return;}
  if(data.error!==undefined){fail(data.error);return;}
  if(!Number.isSafeInteger(data.wasmBytes)||data.wasmBytes<0){fail('Invalid road WASM capacity.');return;}
  wasmBytes=data.wasmBytes;
  job.profiled=false;
  if(job.canceled){releaseRecord(job.record,'canceled-completion');job=null;result=null;state='Idle';return;}
  let validationStart;diagnostic(()=>{validationStart=now();});
  try{
   const [v,n,t,u,tr,g]=job.layout;
   for(const [name,Type,length] of [['origin',Float64Array,3],['vertices',Float32Array,v*3],['normals',Float32Array,n*3],['tangents',Float32Array,t*4],['uv',Float32Array,u*2],['triangles',Uint32Array,tr*3],['submeshes',Uint32Array,g*2]]){
    const a=data[name];if(!(a instanceof Type)||a.length!==length)throw Error('Malformed road '+name+'.');
    if(Type!==Uint32Array)for(const value of a)if(!Number.isFinite(value))throw Error('Nonfinite road '+name+'.');
   }
   if(data.origin.some((value,i)=>value!==job.origin[i]))throw Error('Mismatched road origin.');
   for(const index of data.triangles)if(index>=v)throw Error('Invalid road triangle.');
   let end=0;for(let i=0;i<g;i++){const start=data.submeshes[i*2],next=data.submeshes[i*2+1];if(start!==end||next<=start||next>tr)throw Error('Invalid road submesh.');end=next;}if(end!==tr)throw Error('Incomplete road submeshes.');
  }catch(error){fail(error.message);return;}
  diagnostic(()=>{if(job.record)job.record.validationMs=metric(now()-validationStart);});
  result=data;state='Ready';
  if(job.record)diagnostic(()=>{timing(job.record,data,received);job.record.outputBytes=data.origin.byteLength+data.vertices.byteLength+data.normals.byteLength+data.tangents.byteLength+data.uv.byteLength+data.triangles.byteLength+data.submeshes.byteLength;job.record.peakOutstandingTransferBytes=job.record.inputBytes+job.record.outputBytes;});
 });
 worker.addEventListener('error',event=>fail(event.message||'The road worker stopped.'));
 worker.addEventListener('messageerror',()=>fail('The road worker message could not be read.'));
 const copyPost=(message,input,record)=>{
  diagnostic(()=>{if(record)record.mainCopyStart=now();});
  let copy;try{copy=input.slice();}catch(error){fail(error.message);throw error;}
  diagnostic(()=>{if(record){record.mainInputCopyMs=metric(now()-record.mainCopyStart);record.messageSubmitStart=now();}});
  try{worker.postMessage({...message,input:copy,capacityLimit,profileCpu:Boolean(record)},[copy.buffer]);}
  catch(error){fail(error.message);throw error;}
  diagnostic(()=>{if(record)record.messageSubmitMs=metric(now()-record.messageSubmitStart);});
 };
 const api={
  profileHandshake,
  profileState:()=>({jobActive:Boolean(job?.profiled||sources.some(s=>s?.profiled)),handshakeActive:handshakePending,publicationActive:publishedRecord!==null}),
  wasmBytes:()=>wasmBytes,
  initialized(){live();return initialized;},
  capacityLimit(bytes){live();if(!Number.isSafeInteger(bytes)||bytes<capacityLimit)throw Error('Invalid road capacity admission.');capacityLimit=bytes;},
  packingStarted(){diagnostic(()=>{packingStart=now();});},
  packingEnded(){diagnostic(()=>{if(packingStart!==null)packingMs=metric(now()-packingStart);packingStart=null;});},
  install(identity,input){
   live();if(!initialized||state!=='Idle'||pendingInstalls!==0||!identityValid(identity)||BigInt(identity.sequence)<=lastSequence||identity.owner>=3||sources[identity.owner]!==null||!Array.isArray(identity.layout)||identity.layout.length!==9||identity.layout.some(v=>!Number.isSafeInteger(v)||v<0))throw Error('Invalid road installation.');
   const layout=identity.layout.slice(),id={kind:'road',session:identity.session,revision:identity.revision,owner:identity.owner,sequence:identity.sequence};
   let record=null;diagnostic(()=>{record={kind:'road',phase:'install',identity:id,inputBytes:input.byteLength,inputPackingMs:packingMs,retainedSourceBytes:layout[6]};});packingMs=null;
   sources[id.owner]={identity:id,layout,pending:true,profiled:Boolean(record),record,installBytes:input.byteLength};pendingInstalls++;
   copyPost({type:'install',identity:id},input,record);
  },
  submit(identity,input){
   live();if(!initialized||state!=='Idle'||!identityValid(identity)||BigInt(identity.sequence)<=lastSequence||input.byteLength!==156)throw Error('Invalid road submission or worker busy.');
   const view=new DataView(input.buffer,input.byteOffset,input.byteLength),owner=view.getUint32(0,true),revision=view.getBigUint64(4,true).toString(),source=sources[owner];
   if(!source||source.identity.session!==identity.session||source.identity.revision!==revision)throw Error('Road source is not installed.');
   const id={...identity},origin=[view.getFloat64(12,true),view.getFloat64(20,true),view.getFloat64(28,true)];
   let record=null;diagnostic(()=>{if(!clock)profileHandshake();record={kind:'road',phase:'job',identity:id,inputBytes:input.byteLength,inputPackingMs:packingMs,retainedSourceBytes:sources.reduce((sum,s)=>sum+(s?.layout[6]??0),0),reservedJobBytes:source.layout[7],workerScratchBytes:source.layout[8],reservedInstallCopyBytes:sources.reduce((sum,s)=>sum+(s?.pending?s.installBytes*3:0),0)};});packingMs=null;
   job={identity:id,source:owner,layout:source.layout,origin,record,canceled:false,profiled:Boolean(record)};state='Busy';lastSequence=BigInt(id.sequence);
   copyPost({type:'evaluate',identity:id,source:{owner,revision}},input,record);
  },
  poll(){live();return state==='Ready'?1:state==='Busy'||pendingInstalls!==0?-2:-1;},
  sourceOwner(){live();if(state!=='Ready')throw Error('The road worker has no result.');return job.source;},
  read(output){
   live();if(state!=='Ready')throw Error('The road worker has no result.');
   const record=job.record;let start;
   diagnostic(()=>{if(record){start=now();record.resultWaitMs=metric(start-record.resultReceived);}});
   for(const name of ['origin','vertices','normals','tangents','uv','triangles','submeshes']){if(output[name].length!==result[name].length)throw Error('Invalid road output target.');output[name].set(result[name]);}
   result=null;job=null;state='Idle';publishedRecord=record;
   diagnostic(()=>{if(record){record.mainResultCopyMs=metric(now()-start);record.consumedAt=now();record.transferBuffersReleasedAt=record.consumedAt;}});
  },
  discard(){if(disposed||failure!==null)return;if(state==='Busy'){job.canceled=true;return;}if(state==='Ready'){releaseRecord(job.record,'discard-ready');result=null;job=null;state='Idle';}},
  publicationStarted(){diagnostic(()=>{if(publishedRecord)publishedRecord.publicationStart=now();});},
  publicationEnded(accepted){const record=publishedRecord;publishedRecord=null;diagnostic(()=>{if(record){const at=now();record.accepted=accepted;record.publicationWaitMs=metric((record.publicationStart??at)-record.consumedAt);record.publicationMs=metric(at-(record.publicationStart??at));releaseRecord(record,accepted?'published':'publication-discard');}});},
  dispose(){if(disposed)return;worker.terminate();releaseRecord(job?.record,'owner-termination');for(const source of sources)releaseRecord(source?.record,'owner-termination');releaseRecord(publishedRecord,'owner-termination');clear();disposed=true;},
 };
 api.installRaw=(bytes,input,layout)=>api.install({...fromBytes(bytes),layout:Array.from(layout)},input);
 api.submitRaw=(bytes,input)=>api.submit(fromBytes(bytes),input);
 api.readRaw=(origin,vertices,normals,tangents,uv,triangles,submeshes)=>api.read({origin,vertices,normals,tangents,uv,triangles,submeshes});
 profileHandshake();
 return api;
}
export function installRoadBridge(target,profile){
 const clients=new Set();
 target.gameRoadCreate=()=>{
  const url=new URL('./road-worker.js',import.meta.url);if(profile?.enabled)url.searchParams.set('profileCpu','1');
  let creationStart;try{if(profile?.enabled)creationStart=performance.now();}catch{}
  const client=createRoad(new Worker(url,{type:'module'}),profile);clients.add(client);
  try{if(profile?.enabled)profile.worker({kind:'road',phase:'creation',workerCreationMs:metric(performance.now()-creationStart)});}catch{}
  const dispose=client.dispose;client.dispose=()=>{dispose();clients.delete(client);};return client;
 };
 return {
  profileHandshake(){for(const client of clients)client.profileHandshake();},
  profileState(){let jobActive=false,handshakeActive=false,publicationActive=false;for(const client of clients){const s=client.profileState();jobActive||=s.jobActive;handshakeActive||=s.handshakeActive;publicationActive||=s.publicationActive;}return {jobActive,handshakeActive,publicationActive};},
 };
}
