// One actor and its datasets belong to one presentation world.
import {createWorkerLink,installWorkerClients,u64} from './worker-transport.js';
// Dataset layout written by `grass_mesh::encode_dataset`. The patch count is the last header word.
export const GRASS_DATASET_HEADER=44,GRASS_PATCH_BYTES=60;
const valid=id=>id?.kind==='grass'&&u64(id.session)&&u64(id.revision)&&u64(id.entity);
const same=(a,b)=>valid(a)&&valid(b)&&a.session===b.session&&a.revision===b.revision&&a.entity===b.entity;
const identity=bytes=>{if(!(bytes instanceof Uint8Array)||bytes.length!==24)throw Error('Invalid grass identity.');const v=new DataView(bytes.buffer,bytes.byteOffset,24);return {kind:'grass',session:v.getBigUint64(0,true).toString(),revision:v.getBigUint64(8,true).toString(),entity:v.getBigUint64(16,true).toString()};};
const entity=bytes=>{if(!(bytes instanceof Uint8Array)||bytes.length!==8)throw Error('Invalid grass entity.');return new DataView(bytes.buffer,bytes.byteOffset,8).getBigUint64(0,true).toString();};
export function createGrass(worker,profile,now=()=>performance.now()){
 let state='Idle',job=null,result=null,lastRevision=0n,packingStart=null,packingMs=null;
 const sources=new Map(),publications=new Map();
 const release=(reason)=>{if(job?.record)link.diagnostic(()=>{job.record.physicalReleaseAt=now();job.record.releaseReason=reason;link.emit(job.record);});job=null;result=null;state='Idle';};
 const post=data=>link.post({...data,profileCpu:Boolean(profile?.enabled)},data.input?[data.input.buffer]:[]);
 const copyPost=(type,id,input)=>{let start;link.diagnostic(()=>{start=now();});const copy=input.slice();let record=null;link.diagnostic(()=>{record={kind:'grass',phase:type,identity:id,inputBytes:input.byteLength,inputPackingMs:packingMs,mainCopyStart:start,mainInputCopyMs:now()-start,messageSubmitStart:now()};});let submitted;link.diagnostic(()=>{submitted=now();});post({type,identity:id,input:copy});link.diagnostic(()=>{if(record)record.messageSubmitMs=now()-submitted;});packingMs=null;return record;};
 const receive=(data,received)=>{
  if(!valid(data.identity)){link.fail('Malformed grass reply identity.');return;}
  if(data.type==='installed'||data.type==='retired'){
   const s=sources.get(data.identity.entity),expected=data.type==='installed'?'Installing':'Retiring';
   if(!s||s.state!==expected||!same(s.identity,data.identity)){link.emit({kind:'grass',phase:'rejected',identity:data.identity,reason:'stale-dataset-reply'});return;}
   link.timing(s.record,data,received);if(s.record)link.emit({...s.record,physicalReleaseAt:received});s.record=null;
   if(data.type==='retired')sources.delete(data.identity.entity);else s.state='Installed';return;
  }
  if(!same(data.identity,job?.identity)){link.emit({kind:'grass',phase:'rejected',identity:data.identity,reason:'stale-job-reply'});return;}
  if(data.type==='released') {if(state!=='Canceling'){link.fail('Unexpected grass release.');return;}release('discard-acknowledged');return;}
  if(state==='Canceling')return;
  if(data.type==='planned'){
   if(state!=='Planning'){link.emit({kind:'grass',phase:'rejected',reason:'duplicate-plan',identity:data.identity});return;}
   const p=data.allocation;
   if(!Array.isArray(p)||p.length!==5||p.some(v=>!Number.isSafeInteger(v)||v<0||v>0xffffffff)||p[3]!==p[0]*40+p[1]*4||p[4]!==p[3]||p[2]!==job.scratch){link.fail('Invalid grass allocation.');return;}
   job.plan=p.slice();link.timing(job.record,data,received);link.diagnostic(()=>{job.plannedAt=now();if(job.record){job.record.planning=data.cpuProfile;job.record.planTiming=job.record.timing;job.record.planReceivedAt=job.plannedAt;}});state='Planned';return;
  }
  if(data.type!=='result'||state!=='Emitting'){link.emit({kind:'grass',phase:'rejected',reason:'duplicate-result',identity:data.identity});return;}
  let validationStart;link.diagnostic(()=>{validationStart=now();});
  try{
   const [v,i]=job.plan;
   for(const [name,Type,len] of [['anchors',Float32Array,v*3],['uvs',Float32Array,v*2],['blades',Float32Array,v*4],['patches',Uint32Array,v],['indices',Uint32Array,i]]){const a=data[name];if(!(a instanceof Type)||a.length!==len)throw Error('Malformed grass '+name+'.');if(Type===Float32Array)for(const n of a)if(!Number.isFinite(n))throw Error('Nonfinite grass '+name+'.');}
   for(const index of data.indices)if(index>=v)throw Error('Invalid grass index.');for(const patch of data.patches)if(patch>=job.patchCount)throw Error('Invalid grass patch.');
  }catch(error){link.fail(error.message);return;}
  link.diagnostic(()=>{if(job.record)job.record.validationMs=now()-validationStart;});
  link.timing(job.record,data,received);result=data;state='Ready';
 };
 const link=createWorkerLink(worker,'grass',profile,now,{receive,onFail(){release('failure-termination');sources.clear();publications.clear();state='Failed';}});
 const live=link.live;
 const api={
  initialized(){live();return link.initialized();},wasmBytes:link.wasmBytes,
  packingStarted(){link.diagnostic(()=>{packingStart=now();});},
  packingEnded(){link.diagnostic(()=>{if(packingStart!==null)packingMs=now()-packingStart;packingStart=null;});},
  publicationStartedRaw(bytes){const key=entity(bytes);link.diagnostic(()=>{const r=publications.get(key);if(r)r.publicationStart=now();});},
  publicationEndedRaw(bytes,accepted){const key=entity(bytes),r=publications.get(key);publications.delete(key);link.diagnostic(()=>{if(r){const at=now();link.emit({...r,phase:'publication',accepted,publicationWaitMs:(r.publicationStart??at)-r.consumedAt,publicationMs:at-(r.publicationStart??at)});}});},
  capacityLimit(bytes){live();link.capacityLimit(bytes);},
  install(id,input,scratch){live();if(!link.initialized()||!valid(id)||sources.has(id.entity)||!Number.isSafeInteger(scratch)||scratch<0||input.byteLength<GRASS_DATASET_HEADER)throw Error('Invalid grass installation.');const count=new DataView(input.buffer,input.byteOffset,input.byteLength).getUint32(GRASS_DATASET_HEADER-4,true);if(input.byteLength!==GRASS_DATASET_HEADER+count*GRASS_PATCH_BYTES)throw Error('Invalid grass dataset size.');const record=copyPost('install',id,input);sources.set(id.entity,{identity:{...id},state:'Installing',count,scratch,record});},
  installed(key){live();return sources.get(key)?.state==='Installed';},
  retire(key){live();const s=sources.get(key);if(!s)return true;if(job?.identity.entity===key)return false;if(s.state!=='Retiring'){s.state='Retiring';post({type:'retire',identity:s.identity});}return false;},
  submit(id,input){live();const s=sources.get(id.entity);if(state!=='Idle'||!valid(id)||BigInt(id.revision)<=lastRevision||s?.state!=='Installed'||s.identity.session!==id.session||input.byteLength!==80)throw Error('Invalid grass submission.');const record=copyPost('plan',id,input);job={identity:{...id},record,plan:null,patchCount:s.count,scratch:s.scratch};lastRevision=BigInt(id.revision);state='Planning';},
  approve(id,bytes){live();if(state!=='Planned'||!same(id,job.identity)||bytes!==job.plan[3])throw Error('Invalid grass approval.');link.diagnostic(()=>{if(job.record){job.record.admissionWaitMs=now()-job.plannedAt;job.record.messageSubmitStart=now();job.record.outputBytes=bytes;}});post({type:'evaluate',identity:job.identity});state='Emitting';},
  poll(){live();return state==='Ready'?1:state==='Planned'?0:state==='Idle'?-1:-2;},
  read(output){live();if(state!=='Ready')throw Error('The grass worker has no result.');let start;link.diagnostic(()=>{start=now();});for(const name of ['anchors','uvs','blades','patches','indices']){if(output[name].length!==result[name].length)throw Error('Invalid grass output target.');output[name].set(result[name]);}link.diagnostic(()=>{if(job.record)job.record.mainResultCopyMs=now()-start;});link.diagnostic(()=>{if(job.record)publications.set(job.identity.entity,{...job.record,consumedAt:now()});});release('consumed');},
  discard(){if(link.closed()||!job||state==='Canceling')return;result=null;post({type:'discard',identity:job.identity});state='Canceling';},
  dispose(){if(!link.dispose())return;release('owner-termination');sources.clear();publications.clear();},
  profileHandshake:link.profileHandshake,profileState:()=>{let jobActive=Boolean(job?.record);for(const source of sources.values())jobActive||=Boolean(source.record);return {jobActive,handshakeActive:link.handshakePending(),publicationActive:publications.size>0};},
 };
 api.installRaw=(id,input,scratch)=>api.install(identity(id),input,scratch);
 api.installedRaw=key=>api.installed(entity(key));api.retireRaw=key=>api.retire(entity(key));
 api.submitRaw=(id,input)=>{api.submit(identity(id),input);};
 api.planRaw=out=>{live();if(state!=='Planned'||out.length!==5)throw Error('No grass plan.');out.set(job.plan);};
 api.approveRaw=(id,bytes)=>api.approve(identity(id),bytes);
 api.readRaw=(anchors,uvs,blades,patches,indices)=>api.read({anchors,uvs,blades,patches,indices});
 link.profileHandshake();return api;
}
export function installGrassBridge(target,profile){
 return installWorkerClients(target,'gameGrassCreate',new URL('./grass-worker.js',import.meta.url),createGrass,profile);
}
