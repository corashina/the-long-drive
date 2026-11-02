// One actor and its datasets belong to one presentation world.
import {workerClockHandshake,workerDeliveryInterval,workerInputDeliveryInterval} from './cpu-profile.js';
const MAX_U64=18446744073709551615n;
const u64=v=>typeof v==='string'&&/^(0|[1-9][0-9]{0,19})$/.test(v)&&BigInt(v)<=MAX_U64;
const valid=id=>id?.kind==='grass'&&u64(id.session)&&u64(id.revision)&&u64(id.entity);
const same=(a,b)=>valid(a)&&valid(b)&&a.session===b.session&&a.revision===b.revision&&a.entity===b.entity;
const identity=bytes=>{if(!(bytes instanceof Uint8Array)||bytes.length!==24)throw Error('Invalid grass identity.');const v=new DataView(bytes.buffer,bytes.byteOffset,24);return {kind:'grass',session:v.getBigUint64(0,true).toString(),revision:v.getBigUint64(8,true).toString(),entity:v.getBigUint64(16,true).toString()};};
const entity=bytes=>{if(!(bytes instanceof Uint8Array)||bytes.length!==8)throw Error('Invalid grass entity.');return new DataView(bytes.buffer,bytes.byteOffset,8).getBigUint64(0,true).toString();};
export function createGrass(worker,profile,now=()=>performance.now()){
 let state='Idle',job=null,result=null,failure=null,disposed=false,initialized=false,wasmBytes=0,capacityLimit=2097152,lastRevision=0n,clock=null,handshakePending=false,packingStart=null,packingMs=null;
 const sources=new Map(),publications=new Map();
 const diagnostic=fn=>{if(profile?.enabled)try{fn();}catch{}};
 const emit=r=>diagnostic(()=>profile.worker(r));
 const release=(reason)=>{if(job?.record)diagnostic(()=>{job.record.physicalReleaseAt=now();job.record.releaseReason=reason;emit(job.record);});job=null;result=null;state='Idle';};
 const fail=message=>{if(disposed||failure!==null)return;failure=String(message);worker.terminate();release('failure-termination');sources.clear();publications.clear();wasmBytes=0;state='Failed';emit({kind:'grass',phase:'failure',error:failure});};
 const live=()=>{if(disposed)throw Error('The grass worker is disposed.');if(failure!==null)throw Error(failure);};
 const post=data=>{try{worker.postMessage({...data,capacityLimit,profileCpu:Boolean(profile?.enabled)},data.input?[data.input.buffer]:[]);}catch(error){fail(error.message);throw error;}};
 const timing=(record,data,received)=>diagnostic(()=>{if(!record)return;record.worker=data.cpuProfile??null;if(data.cpuProfile){const raw=data.cpuProfile;record.clockHandshake=clock;record.timing={submitDelivery:workerInputDeliveryInterval(clock,{windowSent:record.messageSubmitStart,workerReceived:raw.workerReceived}),returnDelivery:workerDeliveryInterval(clock,{workerSent:raw.resultPostStart,windowReceived:received})};if(raw.workerOrigin!==clock?.workerOrigin)for(const v of Object.values(record.timing)){v.valid=false;v.valueMs=null;v.reason='worker-origin-mismatch';}record.submitDeliveryMs=record.timing.submitDelivery.valueMs;record.returnDeliveryMs=record.timing.returnDelivery.valueMs;}else record.timing={valid:false,reason:'missing-worker-diagnostics'};});
 const copyPost=(type,id,input)=>{let start;diagnostic(()=>{start=now();});const copy=input.slice();let record=null;diagnostic(()=>{record={kind:'grass',phase:type,identity:id,inputBytes:input.byteLength,inputPackingMs:packingMs,mainCopyStart:start,mainInputCopyMs:now()-start,messageSubmitStart:now()};});let submitted;diagnostic(()=>{submitted=now();});post({type,identity:id,input:copy});diagnostic(()=>{if(record)record.messageSubmitMs=now()-submitted;});packingMs=null;return record;};
 const profileHandshake=()=>diagnostic(()=>{if(disposed||failure!==null||handshakePending)return;handshakePending=true;worker.postMessage({type:'handshake',windowOrigin:profile.timeOrigin,windowSent:now()});});
 worker.addEventListener('message',({data})=>{
  if(disposed||failure!==null)return;let received;diagnostic(()=>{received=now();});
  if(!data||typeof data!=='object'){fail('Malformed grass worker message.');return;}
  if(data.type==='fatal'){fail(data.error??'The grass worker stopped.');return;}
  if(data.type==='handshake'){handshakePending=false;diagnostic(()=>{clock=workerClockHandshake({...data,windowReceived:received});emit({kind:'grass',phase:'initialization',clockHandshake:clock,worker:{initializationMs:data.initializationMs,retainedWasmBytes:data.wasmBytes}});});return;}
  if(!Number.isSafeInteger(data.wasmBytes)||data.wasmBytes<0||data.wasmBytes>capacityLimit){fail('Grass WASM capacity exceeds admission.');return;}wasmBytes=data.wasmBytes;
  if(data.type==='initialized'){initialized=true;return;}
  if(!valid(data.identity)){fail('Malformed grass reply identity.');return;}
  if(data.type==='installed'||data.type==='retired'){
   const s=sources.get(data.identity.entity),expected=data.type==='installed'?'Installing':'Retiring';
   if(!s||s.state!==expected||!same(s.identity,data.identity)){emit({kind:'grass',phase:'rejected',identity:data.identity,reason:'stale-dataset-reply'});return;}
   timing(s.record,data,received);if(s.record)emit({...s.record,physicalReleaseAt:received});s.record=null;
   if(data.type==='retired')sources.delete(data.identity.entity);else s.state='Installed';return;
  }
  if(!same(data.identity,job?.identity)){emit({kind:'grass',phase:'rejected',identity:data.identity,reason:'stale-job-reply'});return;}
  if(data.type==='released') {if(state!=='Canceling'){fail('Unexpected grass release.');return;}release('discard-acknowledged');return;}
  if(state==='Canceling')return;
  if(data.type==='planned'){
   if(state!=='Planning'){emit({kind:'grass',phase:'rejected',reason:'duplicate-plan',identity:data.identity});return;}
   const p=data.allocation;
   if(!Array.isArray(p)||p.length!==5||p.some(v=>!Number.isSafeInteger(v)||v<0||v>0xffffffff)||p[3]!==p[0]*40+p[1]*4||p[4]!==p[3]||p[2]!==job.scratch){fail('Invalid grass allocation.');return;}
   job.plan=p.slice();timing(job.record,data,received);diagnostic(()=>{job.plannedAt=now();if(job.record){job.record.planning=data.cpuProfile;job.record.planTiming=job.record.timing;job.record.planReceivedAt=job.plannedAt;}});state='Planned';return;
  }
  if(data.type!=='result'||state!=='Emitting'){emit({kind:'grass',phase:'rejected',reason:'duplicate-result',identity:data.identity});return;}
  let validationStart;diagnostic(()=>{validationStart=now();});
  try{
   const [v,i]=job.plan;
   for(const [name,Type,len] of [['anchors',Float32Array,v*3],['uvs',Float32Array,v*2],['blades',Float32Array,v*4],['patches',Uint32Array,v],['indices',Uint32Array,i]]){const a=data[name];if(!(a instanceof Type)||a.length!==len)throw Error('Malformed grass '+name+'.');if(Type===Float32Array)for(const n of a)if(!Number.isFinite(n))throw Error('Nonfinite grass '+name+'.');}
   for(const index of data.indices)if(index>=v)throw Error('Invalid grass index.');for(const patch of data.patches)if(patch>=job.patchCount)throw Error('Invalid grass patch.');
  }catch(error){fail(error.message);return;}
  diagnostic(()=>{if(job.record)job.record.validationMs=now()-validationStart;});
  timing(job.record,data,received);result=data;state='Ready';
 });
 worker.addEventListener('error',event=>fail(event.message||'The grass worker stopped.'));
 worker.addEventListener('messageerror',()=>fail('The grass worker message could not be read.'));
 const api={
  initialized(){live();return initialized;},wasmBytes:()=>wasmBytes,
  packingStarted(){diagnostic(()=>{packingStart=now();});},
  packingEnded(){diagnostic(()=>{if(packingStart!==null)packingMs=now()-packingStart;packingStart=null;});},
  publicationStartedRaw(bytes){const key=entity(bytes);diagnostic(()=>{const r=publications.get(key);if(r)r.publicationStart=now();});},
  publicationEndedRaw(bytes,accepted){const key=entity(bytes),r=publications.get(key);publications.delete(key);diagnostic(()=>{if(r){const at=now();emit({...r,phase:'publication',accepted,publicationWaitMs:(r.publicationStart??at)-r.consumedAt,publicationMs:at-(r.publicationStart??at)});}});},
  capacityLimit(bytes){live();if(!Number.isSafeInteger(bytes)||bytes<capacityLimit)throw Error('Invalid grass capacity admission.');capacityLimit=bytes;},
  install(id,input,scratch){live();if(!initialized||!valid(id)||sources.has(id.entity)||!Number.isSafeInteger(scratch)||scratch<0||input.byteLength<36)throw Error('Invalid grass installation.');const count=new DataView(input.buffer,input.byteOffset,input.byteLength).getUint32(32,true);if(input.byteLength!==36+count*60)throw Error('Invalid grass dataset size.');const record=copyPost('install',id,input);sources.set(id.entity,{identity:{...id},state:'Installing',count,scratch,record});},
  installed(key){live();return sources.get(key)?.state==='Installed';},
  retire(key){live();const s=sources.get(key);if(!s)return true;if(job?.identity.entity===key)return false;if(s.state!=='Retiring'){s.state='Retiring';post({type:'retire',identity:s.identity});}return false;},
  submit(id,input){live();const s=sources.get(id.entity);if(state!=='Idle'||!valid(id)||BigInt(id.revision)<=lastRevision||s?.state!=='Installed'||s.identity.session!==id.session||input.byteLength!==80)throw Error('Invalid grass submission.');const record=copyPost('plan',id,input);job={identity:{...id},record,plan:null,patchCount:s.count,scratch:s.scratch};lastRevision=BigInt(id.revision);state='Planning';},
  approve(id,bytes){live();if(state!=='Planned'||!same(id,job.identity)||bytes!==job.plan[3])throw Error('Invalid grass approval.');diagnostic(()=>{if(job.record){job.record.admissionWaitMs=now()-job.plannedAt;job.record.messageSubmitStart=now();job.record.outputBytes=bytes;}});post({type:'evaluate',identity:job.identity});state='Emitting';},
  poll(){live();return state==='Ready'?1:state==='Planned'?0:state==='Idle'?-1:-2;},
  read(output){live();if(state!=='Ready')throw Error('The grass worker has no result.');let start;diagnostic(()=>{start=now();});for(const name of ['anchors','uvs','blades','patches','indices']){if(output[name].length!==result[name].length)throw Error('Invalid grass output target.');output[name].set(result[name]);}diagnostic(()=>{if(job.record)job.record.mainResultCopyMs=now()-start;});diagnostic(()=>{if(job.record)publications.set(job.identity.entity,{...job.record,consumedAt:now()});});release('consumed');},
  discard(){if(disposed||failure!==null||!job||state==='Canceling')return;result=null;post({type:'discard',identity:job.identity});state='Canceling';},
  dispose(){if(disposed)return;worker.terminate();release('owner-termination');sources.clear();publications.clear();wasmBytes=0;disposed=true;},
  profileHandshake,profileState:()=>{let jobActive=Boolean(job?.record);for(const source of sources.values())jobActive||=Boolean(source.record);return {jobActive,handshakeActive:handshakePending,publicationActive:publications.size>0};},
 };
 api.installRaw=(id,input,scratch)=>api.install(identity(id),input,scratch);
 api.installedRaw=key=>api.installed(entity(key));api.retireRaw=key=>api.retire(entity(key));
 api.submitRaw=(id,input)=>{api.submit(identity(id),input);};
 api.planRaw=out=>{live();if(state!=='Planned'||out.length!==5)throw Error('No grass plan.');out.set(job.plan);};
 api.approveRaw=(id,bytes)=>api.approve(identity(id),bytes);
 api.readRaw=(anchors,uvs,blades,patches,indices)=>api.read({anchors,uvs,blades,patches,indices});
 profileHandshake();return api;
}
export function installGrassBridge(target,profile){
 const clients=new Set();target.gameGrassCreate=()=>{const url=new URL('./grass-worker.js',import.meta.url);if(profile?.enabled)url.searchParams.set('profileCpu','1');const client=createGrass(new Worker(url,{type:'module'}),profile);clients.add(client);const dispose=client.dispose;client.dispose=()=>{dispose();clients.delete(client);};return client;};
 return {profileHandshake(){for(const c of clients)c.profileHandshake();},profileState(){let jobActive=false,handshakeActive=false,publicationActive=false;for(const c of clients){const s=c.profileState();jobActive||=s.jobActive;handshakeActive||=s.handshakeActive;publicationActive||=s.publicationActive;}return {jobActive,handshakeActive,publicationActive};}};
}
