// One world owns this transport, its immutable sources, and its unread result.
import {createWorkerLink,installWorkerClients,metric,u64} from './worker-transport.js';
const identityValid=id=>id?.kind==='road'&&u64(id.session)&&u64(id.revision)&&u64(id.sequence)&&Number.isInteger(id.owner)&&id.owner>=0&&id.owner<=0xffffffff;
const same=(a,b)=>identityValid(a)&&identityValid(b)&&a.session===b.session&&a.revision===b.revision&&a.owner===b.owner&&a.sequence===b.sequence;
const fromBytes=bytes=>{if(!(bytes instanceof Uint8Array)||bytes.length!==28)throw Error('Invalid road identity.');const v=new DataView(bytes.buffer,bytes.byteOffset,28);return {kind:'road',session:v.getBigUint64(0,true).toString(),revision:v.getBigUint64(8,true).toString(),owner:v.getUint32(16,true),sequence:v.getBigUint64(20,true).toString()};};
export function createRoad(worker,profile,now=()=>performance.now()){
 let state='Idle',job=null,result=null,lastSequence=0n,pendingInstalls=0;
 const sources=new Array(3).fill(null);
 let packingStart=null,packingMs=null,publishedRecord=null;
 const releaseRecord=(record,reason)=>{if(!record)return;link.diagnostic(()=>{record.releaseReason=reason;record.stagingReleasedAt=now();record.physicalReleaseAt=record.transferBuffersReleasedAt??record.stagingReleasedAt;record.totalJobMs=metric(record.stagingReleasedAt-record.mainCopyStart);link.emit(record);});};
 const clear=()=>{job=null;result=null;sources.fill(null);publishedRecord=null;state='Idle';pendingInstalls=0;packingStart=packingMs=null;};
 const releaseAll=reason=>{releaseRecord(job?.record,reason);for(const s of sources)releaseRecord(s?.record,reason);releaseRecord(publishedRecord,reason);clear();};
 const receive=(data,received)=>{
  if(data.type==='installed'){
   const id=data.identity;if(!identityValid(id)||id.owner>=3){link.fail('Malformed road installation identity.');return;}const source=sources[id.owner];
   if(!source?.pending||!same(id,source.identity)){link.emit({kind:'road',phase:'rejected',identity:id,reason:'stale-installation'});return;}
   if(data.error!==undefined){link.fail(data.error);return;}
   source.pending=false;pendingInstalls--;source.profiled=false;
   if(source.record){link.timing(source.record,data,received);releaseRecord(source.record,'installation-complete');source.record=null;}
   return;
  }
  // Replies cannot acquire ownership. Only the currently submitted identity can complete.
  if(data.type!=='result'||!identityValid(data.identity)){link.fail('Malformed road result identity.');return;}
  if(state!=='Busy'||!same(data.identity,job?.identity)){link.emit({kind:'road',phase:'rejected',identity:data.identity,reason:BigInt(data.identity.sequence)===lastSequence?'duplicate-result':'stale-result'});return;}
  if(data.error!==undefined){link.fail(data.error);return;}
  job.profiled=false;
  if(job.canceled){releaseRecord(job.record,'canceled-completion');job=null;result=null;state='Idle';return;}
  let validationStart;link.diagnostic(()=>{validationStart=now();});
  try{
   const [v,n,t,u,tr,g]=job.layout;
   for(const [name,Type,length] of [['origin',Float64Array,3],['vertices',Float32Array,v*3],['normals',Float32Array,n*3],['tangents',Float32Array,t*4],['uv',Float32Array,u*2],['triangles',Uint32Array,tr*3],['submeshes',Uint32Array,g*2]]){
    const a=data[name];if(!(a instanceof Type)||a.length!==length)throw Error('Malformed road '+name+'.');
    if(Type!==Uint32Array)for(const value of a)if(!Number.isFinite(value))throw Error('Nonfinite road '+name+'.');
   }
   if(data.origin.some((value,i)=>value!==job.origin[i]))throw Error('Mismatched road origin.');
   for(const index of data.triangles)if(index>=v)throw Error('Invalid road triangle.');
   let end=0;for(let i=0;i<g;i++){const start=data.submeshes[i*2],next=data.submeshes[i*2+1];if(start!==end||next<=start||next>tr)throw Error('Invalid road submesh.');end=next;}if(end!==tr)throw Error('Incomplete road submeshes.');
  }catch(error){link.fail(error.message);return;}
  link.diagnostic(()=>{if(job.record)job.record.validationMs=metric(now()-validationStart);});
  result=data;state='Ready';
  if(job.record){link.timing(job.record,data,received);link.diagnostic(()=>{job.record.outputBytes=data.origin.byteLength+data.vertices.byteLength+data.normals.byteLength+data.tangents.byteLength+data.uv.byteLength+data.triangles.byteLength+data.submeshes.byteLength;job.record.peakOutstandingTransferBytes=job.record.inputBytes+job.record.outputBytes;});}
 };
 const link=createWorkerLink(worker,'road',profile,now,{receive,onFail(failure){link.diagnostic(()=>{if(job?.record)job.record.error=failure;for(const s of sources)if(s?.record)s.record.error=failure;});releaseAll('failure-termination');state='Failed';}});
 const copyPost=(message,input,record)=>{
  link.diagnostic(()=>{if(record)record.mainCopyStart=now();});
  let copy;try{copy=input.slice();}catch(error){link.fail(error.message);throw error;}
  link.diagnostic(()=>{if(record){record.mainInputCopyMs=metric(now()-record.mainCopyStart);record.messageSubmitStart=now();}});
  link.post({...message,input:copy,profileCpu:Boolean(record)},[copy.buffer]);
  link.diagnostic(()=>{if(record)record.messageSubmitMs=metric(now()-record.messageSubmitStart);});
 };
 const api={
  profileState:()=>({jobActive:Boolean(job?.profiled||sources.some(s=>s?.profiled)),publicationActive:publishedRecord!==null}),
  wasmBytes:link.wasmBytes,
  initialized(){link.live();return link.initialized();},
  capacityLimit(bytes){link.live();link.capacityLimit(bytes);},
  packingStarted(){link.diagnostic(()=>{packingStart=now();});},
  packingEnded(){link.diagnostic(()=>{if(packingStart!==null)packingMs=metric(now()-packingStart);packingStart=null;});},
  install(identity,input){
   link.live();if(!link.initialized()||state!=='Idle'||pendingInstalls!==0||!identityValid(identity)||BigInt(identity.sequence)<=lastSequence||identity.owner>=3||sources[identity.owner]!==null||!Array.isArray(identity.layout)||identity.layout.length!==9||identity.layout.some(v=>!Number.isSafeInteger(v)||v<0))throw Error('Invalid road installation.');
   const layout=identity.layout.slice(),id={kind:'road',session:identity.session,revision:identity.revision,owner:identity.owner,sequence:identity.sequence};
   let record=null;link.diagnostic(()=>{record={kind:'road',phase:'install',identity:id,inputBytes:input.byteLength,inputPackingMs:packingMs,retainedSourceBytes:layout[6]};});packingMs=null;
   sources[id.owner]={identity:id,layout,pending:true,profiled:Boolean(record),record,installBytes:input.byteLength};pendingInstalls++;
   copyPost({type:'install',identity:id},input,record);
  },
  submit(identity,input){
   link.live();if(!link.initialized()||state!=='Idle'||!identityValid(identity)||BigInt(identity.sequence)<=lastSequence||input.byteLength!==156)throw Error('Invalid road submission or worker busy.');
   const view=new DataView(input.buffer,input.byteOffset,input.byteLength),owner=view.getUint32(0,true),revision=view.getBigUint64(4,true).toString(),source=sources[owner];
   if(!source||source.identity.session!==identity.session||source.identity.revision!==revision)throw Error('Road source is not installed.');
   const id={...identity},origin=[view.getFloat64(12,true),view.getFloat64(20,true),view.getFloat64(28,true)];
   let record=null;link.diagnostic(()=>{record={kind:'road',phase:'job',identity:id,inputBytes:input.byteLength,inputPackingMs:packingMs,retainedSourceBytes:sources.reduce((sum,s)=>sum+(s?.layout[6]??0),0),reservedJobBytes:source.layout[7],workerScratchBytes:source.layout[8],reservedInstallCopyBytes:sources.reduce((sum,s)=>sum+(s?.pending?s.installBytes*3:0),0)};});packingMs=null;
   job={identity:id,source:owner,layout:source.layout,origin,record,canceled:false,profiled:Boolean(record)};state='Busy';lastSequence=BigInt(id.sequence);
   copyPost({type:'evaluate',identity:id,source:{owner,revision}},input,record);
  },
  poll(){link.live();return state==='Ready'?1:state==='Busy'||pendingInstalls!==0?-2:-1;},
  sourceOwner(){link.live();if(state!=='Ready')throw Error('The road worker has no result.');return job.source;},
  read(output){
   link.live();if(state!=='Ready')throw Error('The road worker has no result.');
   const record=job.record;let start;
   link.diagnostic(()=>{if(record){start=now();record.resultWaitMs=metric(start-record.resultReceived);}});
   for(const name of ['origin','vertices','normals','tangents','uv','triangles','submeshes']){if(output[name].length!==result[name].length)throw Error('Invalid road output target.');output[name].set(result[name]);}
   result=null;job=null;state='Idle';publishedRecord=record;
   link.diagnostic(()=>{if(record){record.mainResultCopyMs=metric(now()-start);record.consumedAt=now();record.transferBuffersReleasedAt=record.consumedAt;}});
  },
  discard(){if(link.closed())return;if(state==='Busy'){job.canceled=true;return;}if(state==='Ready'){releaseRecord(job.record,'discard-ready');result=null;job=null;state='Idle';}},
  publicationStarted(){link.diagnostic(()=>{if(publishedRecord)publishedRecord.publicationStart=now();});},
  publicationEnded(accepted){const record=publishedRecord;publishedRecord=null;link.diagnostic(()=>{if(record){const at=now();record.accepted=accepted;record.publicationWaitMs=metric((record.publicationStart??at)-record.consumedAt);record.publicationMs=metric(at-(record.publicationStart??at));releaseRecord(record,accepted?'published':'publication-discard');}});},
  dispose(){if(link.dispose())releaseAll('owner-termination');},
 };
 api.installRaw=(bytes,input,layout)=>api.install({...fromBytes(bytes),layout:Array.from(layout)},input);
 api.submitRaw=(bytes,input)=>api.submit(fromBytes(bytes),input);
 api.readRaw=(origin,vertices,normals,tangents,uv,triangles,submeshes)=>api.read({origin,vertices,normals,tangents,uv,triangles,submeshes});
 return api;
}
export function installRoadBridge(target,profile){
 return installWorkerClients(target,'gameRoadCreate',new URL('./road-worker.js',import.meta.url),createRoad,profile);
}
