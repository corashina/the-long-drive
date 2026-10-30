// Terrain tile jobs run in a Web Worker with its own WASM. One job is in flight at a time.
import {workerClockHandshake,workerDeliveryInterval,workerInputDeliveryInterval} from './cpu-profile.js';
export function createTerrain(worker,profile,now=()=>performance.now()){
 let next=0,job=null,result=null,error=null;
 let clock=null,handshakePending=false,profiledJob=false,record=null;
 const profileHandshake=()=>{if(!profile?.enabled||handshakePending)return;const sent=now();worker.postMessage({cpuHandshake:{windowOrigin:profile.timeOrigin,windowSent:sent}});handshakePending=true;};
 worker.addEventListener('message',({data})=>{
  if(data.cpuHandshake){
   if(profile?.enabled){clock=workerClockHandshake({...data.cpuHandshake,windowReceived:now()});profile.worker({kind:'terrain',phase:'initialization',clockHandshake:clock,worker:{initializationMs:data.cpuHandshake.initializationMs??null,retainedWasmBytes:data.cpuHandshake.wasmBytes??null}});}
   handshakePending=false;return;
  }
  if(data.id!==job)return;
  profiledJob=false;
  // Publish transport ownership before processing optional timing diagnostics.
  job=null;
  if(data.error!==undefined)error=data.error;else result=data;
  if(!profile?.enabled)record=null;
  if(record){const received=now();record.resultReceived=received;record.worker=data.cpuProfile??null;
   if(data.cpuProfile){record.outputBytes=data.cpuProfile.outputBytes;record.peakOutstandingTransferBytes=record.inputBytes+record.outputBytes;
    const returnDelivery=workerDeliveryInterval(clock,{workerSent:data.cpuProfile.resultPostStart,windowReceived:received}),submitDelivery=workerInputDeliveryInterval(clock,{windowSent:record.messageSubmitStart,workerReceived:data.cpuProfile.workerReceived});
    if(clock?.valid&&data.cpuProfile.workerOrigin!==clock.workerOrigin)for(const interval of [returnDelivery,submitDelivery]){interval.valid=false;interval.valueMs=null;interval.reason='worker-origin-mismatch';interval.raw.reportedWorkerOrigin=Number.isFinite(data.cpuProfile.workerOrigin)?data.cpuProfile.workerOrigin:String(data.cpuProfile.workerOrigin);}
    record.returnDeliveryMs=returnDelivery.valueMs;record.submitDeliveryMs=submitDelivery.valueMs;record.timing={valid:returnDelivery.valid&&submitDelivery.valid,returnDelivery,submitDelivery};record.clockHandshake=clock;
   }else record.timing={valid:false,reason:'missing-worker-diagnostics'};
   if(data.error!==undefined){record.error=data.error;profile.worker(record);record=null;}
  }
 });
 worker.addEventListener('error',event=>{error=event.message||'The terrain worker stopped.';if(profile?.enabled&&record){record.error=error;profile.worker(record);}record=null;profiledJob=false;handshakePending=false;});
 const ready=()=>{if(!result)throw Error('The terrain worker has no result.');return result;};
 profileHandshake();
 return {
  profileHandshake,
  profileState:()=>({jobActive:profiledJob,handshakeActive:handshakePending}),
  // The input is a view of WASM memory, so copy it before transfer.
  submit(input){
   if(job!==null||result)throw Error('The terrain worker is busy.');
   next=next+1&0x7fffffff;job=next;
   if(profile?.enabled){
    if(!clock)profileHandshake();
    record={kind:'terrain',id:job,inputBytes:input.byteLength,mainCopyStart:now()};
   }else record=null;
   profiledJob=record!==null;
   const copy=input.slice();
   if(record){record.mainInputCopyMs=now()-record.mainCopyStart;record.messageSubmitStart=now();}
   worker.postMessage(record?{id:job,input:copy,profileCpu:true}:{id:job,input:copy},[copy.buffer]);
   if(record)record.messageSubmitMs=now()-record.messageSubmitStart;
   return job;
  },
  poll(){if(error!==null)throw Error(error);return result?result.id:job===null?-1:-2;},
  vertexCount:()=>ready().vertices.length/3,
  triangleCount:()=>ready().triangles.length/3,
  read(vertices,normals,tangents,uv,colors,triangles){
   if(!profile?.enabled)record=null;
   const r=ready();
   const start=record?now():0;
   vertices.set(r.vertices);normals.set(r.normals);tangents.set(r.tangents);
   uv.set(r.uv);colors.set(r.colors);triangles.set(r.triangles);
   result=null;
   if(record){record.mainResultCopyMs=now()-start;record.resultWaitMs=start-record.resultReceived;record.totalJobMs=now()-record.mainCopyStart;record.discarded=false;profile.worker(record);record=null;}
  },
  discard(){result=null;if(!profile?.enabled)record=null;if(record){record.discarded=true;record.totalJobMs=now()-record.mainCopyStart;profile.worker(record);record=null;}},
 };
}

export function installTerrainBridge(target,terrain){
 target.gameTerrainSubmit=terrain.submit;
 target.gameTerrainPoll=terrain.poll;
 target.gameTerrainVertexCount=terrain.vertexCount;
 target.gameTerrainTriangleCount=terrain.triangleCount;
 target.gameTerrainRead=terrain.read;
 target.gameTerrainDiscard=terrain.discard;
}
