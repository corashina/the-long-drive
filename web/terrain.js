// Terrain tile jobs run in a Web Worker with its own WASM. One job is in flight at a time.
import {createWorkerLink} from './worker-transport.js';
export function createTerrain(worker,profile,now=()=>performance.now()){
 let next=0,job=null,result=null,record=null;
 const receive=(data,received)=>{
  if(data.type!=='result'||data.id!==job)return;
  job=null;result=data;
  if(record){link.timing(record,data,received);record.outputBytes=data.vertices.byteLength+data.normals.byteLength+data.tangents.byteLength+data.uv.byteLength+data.colors.byteLength+data.triangles.byteLength;}
 };
 // Terrain admits any WASM size.
 const link=createWorkerLink(worker,'terrain',profile,now,{receive,capacity:Infinity,onFail(failure){if(record){record.error=failure;link.emit(record);}record=null;job=null;result=null;}});
 // Before the worker is initialized a job waits in the worker queue.
 const ready=()=>{link.live();if(!result)throw Error('The terrain worker has no result.');return result;};
 const finish=discarded=>{if(record){link.diagnostic(()=>{record.discarded=discarded;record.totalJobMs=now()-record.mainCopyStart;});link.emit(record);record=null;}};
 link.profileHandshake();
 return {
  profileHandshake:link.profileHandshake,
  profileState:()=>({jobActive:job!==null&&record!==null,handshakeActive:link.handshakePending()}),
  // The input is a view of WASM memory, so copy it before transfer.
  submit(input){
   link.live();if(job!==null||result)throw Error('The terrain worker is busy.');
   next=next+1&0x7fffffff;job=next;
   record=null;link.diagnostic(()=>{if(!link.clock())link.profileHandshake();record={kind:'terrain',id:job,inputBytes:input.byteLength,mainCopyStart:now()};});
   const copy=input.slice();
   link.diagnostic(()=>{if(record){record.mainInputCopyMs=now()-record.mainCopyStart;record.messageSubmitStart=now();}});
   link.post({type:'evaluate',id:job,input:copy,profileCpu:record!==null},[copy.buffer]);
   link.diagnostic(()=>{if(record)record.messageSubmitMs=now()-record.messageSubmitStart;});
   return job;
  },
  poll(){link.live();return result?result.id:job===null?-1:-2;},
  vertexCount:()=>ready().vertices.length/3,
  triangleCount:()=>ready().triangles.length/3,
  read(vertices,normals,tangents,uv,colors,triangles){
   const r=ready();let start;link.diagnostic(()=>{start=now();});
   vertices.set(r.vertices);normals.set(r.normals);tangents.set(r.tangents);
   uv.set(r.uv);colors.set(r.colors);triangles.set(r.triangles);
   result=null;
   link.diagnostic(()=>{if(record){record.mainResultCopyMs=now()-start;record.resultWaitMs=start-record.resultReceived;}});
   finish(false);
  },
  discard(){result=null;finish(true);},
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
